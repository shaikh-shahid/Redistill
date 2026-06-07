// Redistill library - exposes internal components for testing and potential future SDK usage
// This module provides access to the core data structures and functions without requiring
// the full binary to be run.

#![allow(dead_code)] // Some items may only be used in tests

use ahash::AHasher;
pub use bytes::{Bytes, BytesMut};
pub use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hasher;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ==================== Configuration Structures ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_num_shards")]
    pub num_shards: usize,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,
    #[serde(default = "default_buffer_pool_size")]
    pub buffer_pool_size: usize,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,
    #[serde(default)]
    pub connection_rate_limit: u64,
    #[serde(default)]
    pub health_check_port: u16,
    #[serde(default = "default_shutdown_grace_period_secs")]
    pub shutdown_grace_period_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub tls_enabled: bool,
    #[serde(default)]
    pub tls_cert_path: String,
    #[serde(default)]
    pub tls_key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    #[serde(default = "default_true")]
    pub tcp_nodelay: bool,
    #[serde(default = "default_tcp_keepalive")]
    pub tcp_keepalive: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default)]
    pub max_memory: u64,
    #[serde(default = "default_eviction_policy")]
    pub eviction_policy: String,
    #[serde(default = "default_eviction_sample_size")]
    pub eviction_sample_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub performance: PerformanceConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
}

// Default functions
fn default_bind() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    6379
}
fn default_num_shards() -> usize {
    256
}
fn default_batch_size() -> usize {
    16
}
fn default_buffer_size() -> usize {
    16 * 1024
}
fn default_buffer_pool_size() -> usize {
    1024
}
fn default_max_connections() -> usize {
    10000
}
fn default_connection_timeout() -> u64 {
    300
}
fn default_shutdown_grace_period_secs() -> u64 {
    30
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_log_format() -> String {
    "text".to_string()
}
fn default_true() -> bool {
    true
}
fn default_tcp_keepalive() -> u64 {
    60
}
fn default_eviction_policy() -> String {
    "allkeys-lru".to_string()
}
fn default_eviction_sample_size() -> usize {
    5
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
            num_shards: default_num_shards(),
            batch_size: default_batch_size(),
            buffer_size: default_buffer_size(),
            buffer_pool_size: default_buffer_pool_size(),
            max_connections: default_max_connections(),
            connection_timeout: default_connection_timeout(),
            connection_rate_limit: 0,
            health_check_port: 0,
            shutdown_grace_period_secs: default_shutdown_grace_period_secs(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            tcp_nodelay: default_true(),
            tcp_keepalive: default_tcp_keepalive(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_memory: 0,
            eviction_policy: default_eviction_policy(),
            eviction_sample_size: default_eviction_sample_size(),
        }
    }
}

// ==================== Eviction Policy ====================

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum EvictionPolicy {
    NoEviction,
    #[default]
    AllKeysLru,
    AllKeysRandom,
    AllKeysS3Fifo,
}

impl FromStr for EvictionPolicy {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "allkeys-lru" => EvictionPolicy::AllKeysLru,
            "allkeys-random" => EvictionPolicy::AllKeysRandom,
            "allkeys-s3fifo" => EvictionPolicy::AllKeysS3Fifo,
            "noeviction" => EvictionPolicy::NoEviction,
            _ => EvictionPolicy::AllKeysLru, // default
        })
    }
}

impl EvictionPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvictionPolicy::NoEviction => "noeviction",
            EvictionPolicy::AllKeysLru => "allkeys-lru",
            EvictionPolicy::AllKeysRandom => "allkeys-random",
            EvictionPolicy::AllKeysS3Fifo => "allkeys-s3fifo",
        }
    }
}

// ==================== Storage Entry ====================

#[derive(Clone)]
pub enum EntryValue {
    String(Bytes),
    Hash(Arc<RwLock<HashMap<Bytes, Bytes>>>),
}

pub struct Entry {
    pub value: EntryValue,
    pub expiry: Option<u64>,
    pub last_accessed: AtomicU32,
    pub queue_type: AtomicU8,
    pub access_count: AtomicU8,
}

impl Clone for Entry {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            expiry: self.expiry,
            last_accessed: AtomicU32::new(self.last_accessed.load(Ordering::Relaxed)),
            queue_type: AtomicU8::new(self.queue_type.load(Ordering::Relaxed)),
            access_count: AtomicU8::new(self.access_count.load(Ordering::Relaxed)),
        }
    }
}

// ==================== Sharded Store ====================

pub struct ShardedStore {
    pub shards: Vec<Arc<DashMap<Bytes, Entry>>>,
    pub num_shards: usize,
}

impl Clone for ShardedStore {
    fn clone(&self) -> Self {
        Self {
            shards: self.shards.clone(),
            num_shards: self.num_shards,
        }
    }
}

impl ShardedStore {
    pub fn new(num_shards: usize) -> Self {
        let mut shards = Vec::with_capacity(num_shards);
        for _ in 0..num_shards {
            shards.push(Arc::new(DashMap::with_capacity(1000)));
        }
        Self { shards, num_shards }
    }

    // Fast AHash with hardware acceleration (AES-NI)
    #[inline(always)]
    pub fn hash(&self, key: &[u8]) -> usize {
        let mut hasher = AHasher::default();
        hasher.write(key);
        hasher.finish() as usize % self.num_shards
    }

    #[inline(always)]
    pub fn set(&self, key: Bytes, value: Bytes, ttl: Option<u64>, now: u64) {
        let expiry = ttl.map(|s| now + s);
        let shard = &self.shards[self.hash(&key)];
        shard.insert(
            key,
            Entry {
                value: EntryValue::String(value),
                expiry,
                last_accessed: AtomicU32::new(0),
                queue_type: AtomicU8::new(0),
                access_count: AtomicU8::new(0),
            },
        );
    }

    #[inline(always)]
    pub fn get(&self, key: &[u8], now: u64) -> Option<Bytes> {
        let shard = &self.shards[self.hash(key)];

        if let Some(entry) = shard.get(key) {
            if let Some(expiry) = entry.expiry
                && now >= expiry
            {
                drop(entry);
                shard.remove(key);
                return None;
            }

            if let EntryValue::String(ref val) = entry.value {
                return Some(val.clone());
            }
        }
        None
    }

    /// Redis `TYPE` reply: `"none"`, `"string"`, or `"hash"`. Lazy-expires like `get`.
    #[inline(always)]
    pub fn key_type(&self, key: &[u8], now: u64) -> &'static str {
        let shard = &self.shards[self.hash(key)];

        let Some(entry) = shard.get(key) else {
            return "none";
        };

        if let Some(expiry) = entry.expiry
            && now >= expiry
        {
            drop(entry);
            shard.remove(key);
            return "none";
        }

        match &entry.value {
            EntryValue::String(_) => "string",
            EntryValue::Hash(_) => "hash",
        }
    }

    #[inline(always)]
    pub fn delete(&self, keys: &[Bytes]) -> usize {
        // Group by shard for efficiency
        let mut shard_keys: Vec<Vec<&Bytes>> = vec![Vec::new(); self.num_shards];
        for key in keys {
            shard_keys[self.hash(key)].push(key);
        }

        let mut count = 0;
        for (idx, keys_in_shard) in shard_keys.iter().enumerate() {
            if !keys_in_shard.is_empty() {
                let shard = &self.shards[idx];
                for key in keys_in_shard {
                    if shard.remove(*key).is_some() {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    #[inline(always)]
    pub fn exists(&self, keys: &[Bytes], now: u64) -> usize {
        let mut count = 0;
        for key in keys {
            let shard = &self.shards[self.hash(key)];
            if let Some(entry) = shard.get(key.as_ref())
                && entry.expiry.is_none_or(|exp| exp > now)
            {
                count += 1;
            }
        }
        count
    }

    pub fn keys(&self, now: u64) -> Vec<Bytes> {
        let mut result = Vec::new();
        for shard in &self.shards {
            for entry in shard.iter() {
                let (key, val) = entry.pair();
                if val.expiry.is_none_or(|exp| exp > now) {
                    result.push(key.clone());
                }
            }
        }
        result
    }

    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.shards.iter().all(|s| s.is_empty())
    }

    pub fn clear(&self) {
        for shard in &self.shards {
            shard.clear();
        }
    }

    /// Set one or more fields in a hash. Returns the number of new fields added.
    pub fn hset(
        &self,
        key: Bytes,
        fields: &[(Bytes, Bytes)],
        now: u64,
    ) -> Result<usize, &'static str> {
        let shard = &self.shards[self.hash(&key)];
        let hash_map = loop {
            match shard.entry(key.clone()) {
                dashmap::mapref::entry::Entry::Occupied(occ) => {
                    let entry = occ.get();
                    if let Some(expiry) = entry.expiry
                        && now >= expiry
                    {
                        occ.remove();
                        continue;
                    }
                    match &entry.value {
                        EntryValue::Hash(h) => break h.clone(),
                        EntryValue::String(_) => {
                            return Err(
                                "WRONGTYPE Operation against a key holding the wrong kind of value",
                            );
                        }
                    }
                }
                dashmap::mapref::entry::Entry::Vacant(vac) => {
                    let hash_map = Arc::new(RwLock::new(HashMap::new()));
                    vac.insert(Entry {
                        value: EntryValue::Hash(hash_map.clone()),
                        expiry: None,
                        last_accessed: AtomicU32::new(0),
                        queue_type: AtomicU8::new(0),
                        access_count: AtomicU8::new(0),
                    });
                    break hash_map;
                }
            }
        };

        let mut new_count = 0;
        {
            let mut map_guard = hash_map.write().unwrap();
            for (field, value) in fields {
                if map_guard.insert(field.clone(), value.clone()).is_none() {
                    new_count += 1;
                }
            }
        }

        Ok(new_count)
    }

    /// Get a field value from a hash.
    pub fn hget(&self, key: &[u8], field: &[u8], now: u64) -> Result<Option<Bytes>, &'static str> {
        let shard = &self.shards[self.hash(key)];

        if let Some(entry) = shard.get(key) {
            if let Some(expiry) = entry.expiry
                && now >= expiry
            {
                drop(entry);
                shard.remove(key);
                return Ok(None);
            }

            match &entry.value {
                EntryValue::Hash(hash_map) => {
                    let map_guard = hash_map.read().unwrap();
                    Ok(map_guard.get(field).cloned())
                }
                EntryValue::String(_) => {
                    Err("WRONGTYPE Operation against a key holding the wrong kind of value")
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Get all field-value pairs from a hash.
    pub fn hgetall(&self, key: &[u8], now: u64) -> Result<Vec<Bytes>, &'static str> {
        let shard = &self.shards[self.hash(key)];

        if let Some(entry) = shard.get(key) {
            if let Some(expiry) = entry.expiry
                && now >= expiry
            {
                drop(entry);
                shard.remove(key);
                return Ok(Vec::new());
            }

            match &entry.value {
                EntryValue::Hash(hash_map) => {
                    let map_guard = hash_map.read().unwrap();
                    let mut result = Vec::with_capacity(map_guard.len() * 2);
                    for (k, v) in map_guard.iter() {
                        result.push(k.clone());
                        result.push(v.clone());
                    }
                    Ok(result)
                }
                EntryValue::String(_) => {
                    Err("WRONGTYPE Operation against a key holding the wrong kind of value")
                }
            }
        } else {
            Ok(Vec::new())
        }
    }

    /// Delete one or more fields from a hash. Returns the number of fields removed.
    /// If the hash becomes empty after deletion, the key itself is removed.
    pub fn hdel(&self, key: &[u8], fields: &[Bytes], now: u64) -> Result<usize, &'static str> {
        let shard = &self.shards[self.hash(key)];

        if let Some(entry) = shard.get(key) {
            if let Some(expiry) = entry.expiry
                && now >= expiry
            {
                drop(entry);
                shard.remove(key);
                return Ok(0);
            }

            match &entry.value {
                EntryValue::Hash(hash_map) => {
                    let mut removed = 0;
                    let is_empty;
                    {
                        let mut map_guard = hash_map.write().unwrap();
                        for field in fields {
                            if map_guard.remove(field.as_ref()).is_some() {
                                removed += 1;
                            }
                        }
                        is_empty = map_guard.is_empty();
                    }

                    if removed > 0 && is_empty {
                        drop(entry);
                        shard.remove(key);
                    }

                    Ok(removed)
                }
                EntryValue::String(_) => {
                    Err("WRONGTYPE Operation against a key holding the wrong kind of value")
                }
            }
        } else {
            Ok(0)
        }
    }
}

// ==================== Helper Functions ====================

#[inline(always)]
pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// Calculate approximate size of an entry
#[inline(always)]
pub fn entry_size(key_len: usize, value_len: usize) -> usize {
    key_len + value_len + 64 // ~64 bytes overhead for Arc, Entry struct, etc.
}

// Format bytes as human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

// Fast byte comparison helpers
#[inline(always)]
pub fn eq_ignore_case_3(a: &[u8], b: &[u8; 3]) -> bool {
    a.len() == 3 && (a[0] | 0x20) == b[0] && (a[1] | 0x20) == b[1] && (a[2] | 0x20) == b[2]
}

#[inline(always)]
pub fn eq_ignore_case_6(a: &[u8], b: &[u8; 6]) -> bool {
    a.len() == 6
        && (a[0] | 0x20) == b[0]
        && (a[1] | 0x20) == b[1]
        && (a[2] | 0x20) == b[2]
        && (a[3] | 0x20) == b[3]
        && (a[4] | 0x20) == b[4]
        && (a[5] | 0x20) == b[5]
}

// Connection state for authentication
pub struct ConnectionState {
    pub authenticated: bool,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionState {
    pub fn new() -> Self {
        Self {
            // If no password is set, authentication is not required
            authenticated: true, // Default to true for testing
        }
    }
}

// Eviction stub for testing (actual implementation uses global config)
#[inline(always)]
pub fn evict_if_needed(_store: &ShardedStore, _needed_size: usize) -> bool {
    // In tests with default config (max_memory = 0), always allow
    true
}

// ==================== In-Memory Snapshot (Replication) ====================

/// In-memory snapshot serialization/deserialization for replication.
///
/// Produces and consumes the same RDST binary format as `persistence.rs` so
/// that a snapshot can be shipped over a socket and loaded on a replica.
pub mod persistence {
    use super::{Bytes, EntryValue, ShardedStore};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU8, AtomicU32};
    use std::sync::{Arc, RwLock};

    use super::Entry;

    const SNAPSHOT_MAGIC: &[u8; 4] = b"RDST";
    const SNAPSHOT_VERSION: u8 = 1;

    #[derive(Serialize, Deserialize)]
    enum SnapshotValue {
        String(Vec<u8>),
        Hash(Vec<(Vec<u8>, Vec<u8>)>),
    }

    #[derive(Serialize, Deserialize)]
    struct SnapshotEntry {
        key: Vec<u8>,
        value: SnapshotValue,
        expiry: Option<u64>,
    }

    #[derive(Serialize, Deserialize)]
    struct LegacySnapshotEntry {
        key: Vec<u8>,
        value: Vec<u8>,
        expiry: Option<u64>,
    }

    fn write_snapshot<W: std::io::Write>(w: &mut W, store: &ShardedStore) -> Result<usize, String> {
        let now = super::get_timestamp();
        let keys = store.keys(now);
        let key_count = keys.len() as u64;

        w.write_all(SNAPSHOT_MAGIC)
            .map_err(|e| format!("Failed to write header: {}", e))?;
        w.write_all(&[SNAPSHOT_VERSION])
            .map_err(|e| format!("Failed to write version: {}", e))?;
        w.write_all(&now.to_le_bytes())
            .map_err(|e| format!("Failed to write timestamp: {}", e))?;
        w.write_all(&key_count.to_le_bytes())
            .map_err(|e| format!("Failed to write entry count: {}", e))?;

        let mut count: usize = 0;
        for key in &keys {
            let shard = &store.shards[store.hash(key)];
            let Some(entry) = shard.get(key.as_ref()) else {
                continue;
            };

            if let Some(expiry) = entry.expiry
                && now >= expiry
            {
                continue;
            }

            let snapshot_value = match &entry.value {
                EntryValue::String(bytes) => SnapshotValue::String(bytes.to_vec()),
                EntryValue::Hash(hash_map) => {
                    let map_guard = hash_map.read().unwrap();
                    let mut fields = Vec::with_capacity(map_guard.len());
                    for (k, v) in map_guard.iter() {
                        fields.push((k.to_vec(), v.to_vec()));
                    }
                    SnapshotValue::Hash(fields)
                }
            };

            let snapshot_entry = SnapshotEntry {
                key: key.to_vec(),
                value: snapshot_value,
                expiry: entry.expiry,
            };

            let encoded = bincode::serialize(&snapshot_entry)
                .map_err(|e| format!("Failed to serialize entry: {}", e))?;

            w.write_all(&(encoded.len() as u32).to_le_bytes())
                .map_err(|e| format!("Failed to write entry length: {}", e))?;
            w.write_all(&encoded)
                .map_err(|e| format!("Failed to write entry data: {}", e))?;

            count += 1;
        }

        w.flush().map_err(|e| format!("Failed to flush: {}", e))?;
        Ok(count)
    }

    fn read_snapshot<R: std::io::Read>(r: &mut R, store: &ShardedStore) -> Result<usize, String> {
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)
            .map_err(|e| format!("Failed to read magic: {}", e))?;
        if &magic != SNAPSHOT_MAGIC {
            return Err("Invalid snapshot (bad magic)".to_string());
        }

        let mut version = [0u8; 1];
        r.read_exact(&mut version)
            .map_err(|e| format!("Failed to read version: {}", e))?;
        if version[0] != SNAPSHOT_VERSION {
            return Err(format!("Unsupported snapshot version: {}", version[0]));
        }

        let mut timestamp_bytes = [0u8; 8];
        r.read_exact(&mut timestamp_bytes)
            .map_err(|e| format!("Failed to read timestamp: {}", e))?;
        let _snapshot_time = u64::from_le_bytes(timestamp_bytes);

        let mut count_bytes = [0u8; 8];
        r.read_exact(&mut count_bytes)
            .map_err(|e| format!("Failed to read entry count: {}", e))?;
        let entry_count = u64::from_le_bytes(count_bytes);

        let now = super::get_timestamp();
        let mut loaded: usize = 0;
        let mut skipped_expired: usize = 0;

        for _ in 0..entry_count {
            let mut len_bytes = [0u8; 4];
            if r.read_exact(&mut len_bytes).is_err() {
                break;
            }
            let entry_len = u32::from_le_bytes(len_bytes) as usize;

            let mut entry_data = vec![0u8; entry_len];
            r.read_exact(&mut entry_data)
                .map_err(|e| format!("Failed to read entry data: {}", e))?;

            let (key, entry_value, expiry) =
                match bincode::deserialize::<SnapshotEntry>(&entry_data) {
                    Ok(se) => {
                        let expiry = se.expiry;
                        let value = match se.value {
                            SnapshotValue::String(bytes) => EntryValue::String(Bytes::from(bytes)),
                            SnapshotValue::Hash(fields) => {
                                let mut map = HashMap::new();
                                for (field, val) in fields {
                                    map.insert(Bytes::from(field), Bytes::from(val));
                                }
                                EntryValue::Hash(Arc::new(RwLock::new(map)))
                            }
                        };
                        (Bytes::from(se.key), value, expiry)
                    }
                    Err(_) => {
                        let le: LegacySnapshotEntry = bincode::deserialize(&entry_data)
                            .map_err(|e| format!("Failed to deserialize entry: {}", e))?;
                        (
                            Bytes::from(le.key),
                            EntryValue::String(Bytes::from(le.value)),
                            le.expiry,
                        )
                    }
                };

            if let Some(exp) = expiry
                && now >= exp
            {
                skipped_expired += 1;
                continue;
            }

            let ttl = expiry.and_then(|exp| if exp > now { Some(exp - now) } else { None });

            let shard_idx = store.hash(&key);
            let shard = &store.shards[shard_idx];
            shard.insert(
                key,
                Entry {
                    value: entry_value,
                    expiry: ttl.map(|t| now + t),
                    last_accessed: AtomicU32::new(0),
                    queue_type: AtomicU8::new(0),
                    access_count: AtomicU8::new(0),
                },
            );

            loaded += 1;
        }

        if skipped_expired > 0 {
            eprintln!("(skipped {} expired keys during load)", skipped_expired);
        }

        Ok(loaded)
    }

    /// Serialize the entire store to a `Vec<u8>` using the RDST snapshot format.
    ///
    /// The returned bytes can be shipped over a socket and loaded on a replica
    /// with [`load_store_from_bytes`].
    pub fn dump_store_to_bytes(store: &ShardedStore) -> Result<Vec<u8>, String> {
        let mut buf = std::io::Cursor::new(Vec::with_capacity(64 * 1024));
        let _count = write_snapshot(&mut buf, store)?;
        Ok(buf.into_inner())
    }

    /// Populate `store` from a byte slice produced by [`dump_store_to_bytes`].
    ///
    /// Returns the number of non-expired entries loaded.
    pub fn load_store_from_bytes(store: &ShardedStore, data: &[u8]) -> Result<usize, String> {
        let mut cur = std::io::Cursor::new(data);
        read_snapshot(&mut cur, store)
    }
}
