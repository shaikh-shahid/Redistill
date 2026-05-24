// Prometheus metrics.
//
// One global registry lives behind a `Lazy<Registry>` static. Metric handles
// are pre-created with `Lazy<…>` so the hot path is a single atomic add on a
// cached `IntCounter` — no map lookup, no allocation.
//
// Gauges that mirror existing atomics (memory, connections, keys) are not
// continuously synced; instead they're rehydrated on scrape via
// `update_scrape_time_gauges()`. This keeps the hot path zero-cost while still
// giving Prometheus consistent point-in-time numbers.

use once_cell::sync::Lazy;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::store::{
    ACTIVE_CONNECTIONS, EVICTED_KEYS, MEMORY_USED, REJECTED_CONNECTIONS, TOTAL_COMMANDS,
    TOTAL_CONNECTIONS,
};

// ==================== Hot-path enable flags ====================
//
// Mirror of `config.metrics.command_counter` and `.command_histogram`. Set
// once at startup by `init_runtime_flags()`. The hot path reads these
// instead of going through `CONFIG.deref()` so we avoid an atomic Lazy
// state-load per dispatch (which costs ~3% throughput on multi-million-rps
// pipelined SET because all writer threads hammer the same Lazy<Config>
// cache line).

static COMMAND_COUNTER_ENABLED: AtomicBool = AtomicBool::new(false);
static COMMAND_HISTOGRAM_ENABLED: AtomicBool = AtomicBool::new(false);

/// Stash the runtime metrics flags. Call once during startup, after `CONFIG`
/// has been loaded.
pub fn init_runtime_flags(command_counter: bool, command_histogram: bool) {
    COMMAND_COUNTER_ENABLED.store(command_counter, Ordering::Relaxed);
    COMMAND_HISTOGRAM_ENABLED.store(command_histogram, Ordering::Relaxed);
}

#[inline(always)]
pub fn command_counter_enabled() -> bool {
    COMMAND_COUNTER_ENABLED.load(Ordering::Relaxed)
}

#[inline(always)]
pub fn command_histogram_enabled() -> bool {
    COMMAND_HISTOGRAM_ENABLED.load(Ordering::Relaxed)
}

// ==================== Registry ====================

pub static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

// ==================== Counters ====================

/// Per-command success counter. Cardinality is bounded by the known set of
/// commands; see `command_label` for the canonical name list.
pub static COMMANDS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let m = IntCounterVec::new(
        Opts::new(
            "redistill_commands_total",
            "Total commands processed, labelled by command name",
        ),
        &["cmd"],
    )
    .expect("register commands_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

/// Histograms of command duration in seconds. Same `cmd` label as the counter.
pub static COMMAND_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let opts = HistogramOpts::new(
        "redistill_command_duration_seconds",
        "Latency histogram for command dispatch, labelled by command name",
    )
    .buckets(vec![
        0.000_001, 0.000_005, 0.000_010, 0.000_050, 0.000_100, 0.000_500, 0.001, 0.005, 0.010,
        0.050, 0.100, 0.500, 1.0,
    ]);
    let m = HistogramVec::new(opts, &["cmd"]).expect("register command_duration_seconds");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static AOF_APPENDS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new(
        "redistill_aof_appends_total",
        "Total commands appended to the AOF",
    )
    .expect("register aof_appends_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static AOF_REWRITES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new(
        "redistill_aof_rewrites_total",
        "Total AOF rewrites completed",
    )
    .expect("register aof_rewrites_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static AOF_REPLAY_COMMANDS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new(
        "redistill_aof_replay_commands_total",
        "Commands replayed from the AOF during startup",
    )
    .expect("register aof_replay_commands_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

// ==================== Scrape-time mirror gauges ====================

pub static MEMORY_USED_BYTES: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::new(
        "redistill_memory_used_bytes",
        "Approximate bytes resident in the store",
    )
    .expect("register memory_used_bytes");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static CONNECTIONS_ACTIVE: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::new(
        "redistill_connections_active",
        "Current TCP connections being handled",
    )
    .expect("register connections_active");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static CONNECTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new(
        "redistill_connections_total",
        "Total connections accepted (lifetime)",
    )
    .expect("register connections_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static REJECTED_CONNECTIONS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new(
        "redistill_rejected_connections_total",
        "Connections rejected (max_connections or rate_limit)",
    )
    .expect("register rejected_connections_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static EVICTED_KEYS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new("redistill_evicted_keys_total", "Total keys evicted")
        .expect("register evicted_keys_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static COMMANDS_DISPATCHED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::new(
        "redistill_commands_dispatched_total",
        "Commands dispatched across all clients (lifetime, mirrors TOTAL_COMMANDS atomic)",
    )
    .expect("register commands_dispatched_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static AOF_SIZE_BYTES: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::new(
        "redistill_aof_size_bytes",
        "Current AOF log size in bytes (-1 when AOF disabled)",
    )
    .expect("register aof_size_bytes");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static AOF_LAST_REWRITE_SIZE_BYTES: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::new(
        "redistill_aof_last_rewrite_size_bytes",
        "AOF size after the most recent rewrite (-1 when AOF disabled or never rewritten)",
    )
    .expect("register aof_last_rewrite_size_bytes");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static AOF_DIRTY: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::new(
        "redistill_aof_dirty",
        "1 if AOF has unsynced writes, 0 otherwise (-1 when AOF disabled)",
    )
    .expect("register aof_dirty");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

pub static KEYS_TOTAL: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::new(
        "redistill_keys_total",
        "Live keys across all shards (scrape-time count)",
    )
    .expect("register keys_total");
    REGISTRY.register(Box::new(m.clone())).ok();
    m
});

// ==================== Build info ====================

pub static BUILD_INFO: Lazy<IntGaugeVec> = Lazy::new(|| {
    let m = IntGaugeVec::new(
        Opts::new("redistill_build_info", "Static build info, value always 1"),
        &["version"],
    )
    .expect("register build_info");
    REGISTRY.register(Box::new(m.clone())).ok();
    // Initialise the single labelled series to 1 so the metric is present.
    m.with_label_values(&[env!("CARGO_PKG_VERSION")]).set(1);
    m
});

// ==================== Hot-path helpers ====================

/// Cached metric handles per command label. Both `IntCounter` and `Histogram`
/// are cheap-to-clone Arc-wrappers; storing them by value means the hot path
/// gets a `&` to the original without any allocation or label-vector lookup
/// inside `prometheus`.
pub struct CommandMetrics {
    pub counter: IntCounter,
    pub duration: Histogram,
}

/// All command labels we want to track. Anything not in this list buckets
/// into `"other"` (kept last by convention). The hot path goes through this
/// allowlist exactly once at startup.
const COMMAND_LABELS: &[&str] = &[
    "get",
    "set",
    "del",
    "ttl",
    "pttl",
    "incr",
    "decr",
    "incrby",
    "decrby",
    "mget",
    "mset",
    "keys",
    "scan",
    "exists",
    "expire",
    "persist",
    "type",
    "hset",
    "hget",
    "hdel",
    "hgetall",
    "ping",
    "info",
    "auth",
    "dbsize",
    "save",
    "bgsave",
    "flushdb",
    "flushall",
    "lastsave",
    "bgrewriteaof",
    "other",
];

static COMMAND_METRICS: Lazy<HashMap<&'static str, CommandMetrics>> = Lazy::new(|| {
    let mut m = HashMap::with_capacity(COMMAND_LABELS.len());
    for &name in COMMAND_LABELS {
        let counter = COMMANDS_TOTAL.with_label_values(&[name]);
        let duration = COMMAND_DURATION_SECONDS.with_label_values(&[name]);
        m.insert(name, CommandMetrics { counter, duration });
    }
    m
});

/// Look up the cached counter+histogram for a label. Returns the `"other"`
/// bucket on miss so the hot path never panics. The map lookup is one ~10ns
/// hash on a `&'static str` — no allocation, no Vec<String> construction.
#[inline]
pub fn command_metrics(label: &'static str) -> &'static CommandMetrics {
    COMMAND_METRICS.get(label).unwrap_or_else(|| {
        COMMAND_METRICS
            .get("other")
            .expect("other label registered")
    })
}

/// Map a command's raw bytes to a low-cardinality `&'static str` label so we
/// don't allocate a String per dispatch. Anything unrecognised is bucketed
/// into `"other"` to keep label cardinality bounded.
#[inline]
pub fn command_label(cmd: &[u8]) -> &'static str {
    if cmd.len() > 12 {
        return "other";
    }
    let mut buf = [0u8; 12];
    for (i, b) in cmd.iter().enumerate() {
        buf[i] = b | 0x20; // lowercase ASCII
    }
    let lower = &buf[..cmd.len()];
    match lower {
        b"get" => "get",
        b"set" => "set",
        b"del" => "del",
        b"ttl" => "ttl",
        b"pttl" => "pttl",
        b"incr" => "incr",
        b"decr" => "decr",
        b"incrby" => "incrby",
        b"decrby" => "decrby",
        b"mget" => "mget",
        b"mset" => "mset",
        b"keys" => "keys",
        b"scan" => "scan",
        b"exists" => "exists",
        b"expire" => "expire",
        b"persist" => "persist",
        b"type" => "type",
        b"hset" => "hset",
        b"hget" => "hget",
        b"hdel" => "hdel",
        b"hgetall" => "hgetall",
        b"ping" => "ping",
        b"info" => "info",
        b"auth" => "auth",
        b"dbsize" => "dbsize",
        b"save" => "save",
        b"bgsave" => "bgsave",
        b"flushdb" => "flushdb",
        b"flushall" => "flushall",
        b"lastsave" => "lastsave",
        b"bgrewriteaof" => "bgrewriteaof",
        _ => "other",
    }
}

// ==================== Scrape encoder ====================

/// Refresh gauges that mirror live atomics, then encode the whole registry
/// in Prometheus text format. Called from the /metrics handler.
pub fn encode(store: &crate::store::ShardedStore) -> Vec<u8> {
    // Lazy-init build_info on first scrape.
    let _ = &*BUILD_INFO;

    MEMORY_USED_BYTES.set(MEMORY_USED.load(Ordering::Relaxed) as i64);
    CONNECTIONS_ACTIVE.set(ACTIVE_CONNECTIONS.load(Ordering::Relaxed) as i64);
    // Counters are monotonic. We sync them by setting the delta from prior scrape.
    sync_counter_to_atomic(
        &CONNECTIONS_TOTAL,
        TOTAL_CONNECTIONS.load(Ordering::Relaxed),
    );
    sync_counter_to_atomic(
        &REJECTED_CONNECTIONS_TOTAL,
        REJECTED_CONNECTIONS.load(Ordering::Relaxed),
    );
    sync_counter_to_atomic(&EVICTED_KEYS_TOTAL, EVICTED_KEYS.load(Ordering::Relaxed));
    sync_counter_to_atomic(
        &COMMANDS_DISPATCHED_TOTAL,
        TOTAL_COMMANDS.load(Ordering::Relaxed),
    );
    KEYS_TOTAL.set(store.len() as i64);

    if let Some(aof) = crate::AOF.get() {
        AOF_SIZE_BYTES.set(aof.size_bytes() as i64);
        AOF_LAST_REWRITE_SIZE_BYTES.set(aof.last_rewrite_size() as i64);
        AOF_DIRTY.set(if aof.is_dirty() { 1 } else { 0 });
    } else {
        AOF_SIZE_BYTES.set(-1);
        AOF_LAST_REWRITE_SIZE_BYTES.set(-1);
        AOF_DIRTY.set(-1);
    }

    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buf = Vec::with_capacity(4096);
    encoder.encode(&metric_families, &mut buf).ok();
    buf
}

/// Bring an `IntCounter` up to a known absolute value from a mirror atomic.
/// Prometheus counters only support add, so we add the positive delta. If the
/// atomic somehow went backwards (it shouldn't), we skip the update rather
/// than panic.
fn sync_counter_to_atomic(counter: &IntCounter, current: u64) {
    let prev = counter.get();
    if current > prev {
        counter.inc_by(current - prev);
    }
}
