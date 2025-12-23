# Redistill Performance Tuning Guide

> Squeeze every last drop of performance from your Redistill deployment

## Quick Reference

| Configuration | Use Case | Performance | Memory |
|--------------|----------|-------------|---------|
| **2048 shards** (default) | Balanced workloads | 6.87M GET, 2.74M SET | Moderate |
| **4096 shards** | GET-heavy (>90% reads) | 7.52M GET (+9%) | High |
| **256 shards** | Memory-constrained | 6.49M GET (-6%) | Low |

## Optimal Configuration (Default)

Based on extensive benchmarking on AWS c7i.8xlarge (Intel, 32 cores):

```toml
[server]
num_shards = 2048        # Best balance
batch_size = 256         # Match your pipeline depth
buffer_size = 16384      # 16KB
buffer_pool_size = 2048  # Optimal for tail latency
max_connections = 10000  # Adjust based on your needs

[performance]
tcp_nodelay = true       # Lower latency
tcp_keepalive = 60       # Connection reuse

[memory]
max_memory = 0                  # Set limit based on available RAM
eviction_policy = "allkeys-lru" # LRU eviction
```

**Results:**
- 6.87M GET ops/s (98% faster than Redis)
- 2.74M SET ops/s (matches Redis)
- Excellent for all workload types

---

## Tuning Parameters

### 1. Number of Shards (`num_shards`)

Controls parallelism and contention.

#### 256 Shards (Memory-Constrained)
```toml
num_shards = 256
```

**When to use:**
- Limited memory (<2GB available)
- Small datasets (<1M keys)
- Moderate concurrency (<100 clients)

**Performance:**
- Ultra GET: 6.49M ops/s
- Ultra SET: 2.56M ops/s

**Memory:** ~32MB for shard overhead

---

#### 2048 Shards (Recommended Default)
```toml
num_shards = 2048
```

**When to use:**
- General purpose deployments
- Balanced read/write workloads
- High concurrency (100-500 clients)
- Best all-around choice

**Performance:**
- Ultra GET: 6.87M ops/s (+6% vs 256)
- Ultra SET: 2.74M ops/s (+7% vs 256)

**Memory:** ~256MB for shard overhead

**Why it works:**
- 64,000 ops / 2048 shards = 31 ops per shard
- Sweet spot for DashMap contention
- Minimal overhead for atomic operations

---

#### 4096 Shards (Maximum GET Performance)
```toml
num_shards = 4096
```

**When to use:**
- Read-heavy workloads (>90% GETs)
- Maximum throughput requirements
- Memory is not a constraint
- API caching, session storage

**Performance:**
- Ultra GET: 7.52M ops/s (+9% vs 2048)
- Ultra SET: 2.71M ops/s (similar to 2048)

**Memory:** ~512MB for shard overhead

**Trade-off:**
- Extra 9% GET performance
- 2x memory overhead vs 2048
- Diminishing returns beyond 4096

---

### 2. Batch Size (`batch_size`)

Controls how many commands are buffered before flushing to network.

#### Pipeline Depth Guidelines

| Your Pipeline (-P) | Recommended `batch_size` | Impact |
|-------------------|--------------------------|---------|
| P = 1-16 | 16 | Low latency |
| P = 16-64 | 64-128 | Balanced |
| P = 64-128 | 256 | Optimal for deep pipelines |
| P > 128 | 512 | Maximum batching |

**Example:**
```toml
# For redis-benchmark -P 128
batch_size = 256  # 2x pipeline depth

# For redis-benchmark -P 16
batch_size = 32   # 2x pipeline depth
```

**Impact:**
- Too small: Excess syscalls (poor performance)
- Too large: Increased latency
- **Rule of thumb:** 2x your pipeline depth

---

### 3. Buffer Pool Size (`buffer_pool_size`)

Controls number of pre-allocated response buffers.

```toml
# Default (recommended)
buffer_pool_size = 2048

# Memory-constrained
buffer_pool_size = 1024

# High connection count (>1000)
buffer_pool_size = 4096
```

**Sizing guide:**
- Minimum: `max_connections × 1.5`
- Recommended: `max_connections × 2`
- Maximum: No benefit beyond 4096

**Performance impact:**
- 1024 buffers: p99 latency = 145ms
- 2048 buffers: p99 latency = 106ms (**27% better**)
- 4096 buffers: p99 latency = 105ms (diminishing returns)

---

### 4. TCP Settings

#### TCP NoDelay (Nagle's Algorithm)
```toml
[performance]
tcp_nodelay = true  # Recommended for caching
```

**Impact:**
- `true`: Lower latency (disable Nagle's algorithm)
- `false`: Better throughput for bulk operations

**Use `true` for:**
- Interactive workloads
- Low-latency requirements
- Small messages (<1KB)

---

#### TCP Keep-Alive
```toml
[performance]
tcp_keepalive = 60  # seconds
```

**Impact:**
- Prevents idle connection timeouts
- Detects dead connections
- Enables connection reuse

**Tuning:**
- High churn: 30 seconds
- Stable connections: 60-300 seconds
- Long-lived: 600+ seconds

---

## Workload-Specific Configurations

### Read-Heavy Caching (90%+ GETs)

**Goal:** Maximum GET throughput

```toml
[server]
num_shards = 4096        # Extra 9% GET performance
batch_size = 256
buffer_pool_size = 2048
max_connections = 1000

[memory]
max_memory = 8589934592  # 8GB
eviction_policy = "allkeys-lru"
```

**Expected:**
- 7.52M GET ops/s
- 2.71M SET ops/s
- <1ms p50 latency

---

### Balanced Workload (50/50 Read/Write)

**Goal:** Best overall performance

```toml
[server]
num_shards = 2048        # Balanced
batch_size = 256
buffer_pool_size = 2048
max_connections = 500

[memory]
max_memory = 4294967296  # 4GB
eviction_policy = "allkeys-lru"
```

**Expected:**
- 6.87M GET ops/s
- 2.74M SET ops/s
- ~1ms p50 latency

---

### Write-Heavy Workload (>50% SETs)

**Goal:** Optimize for write throughput

```toml
[server]
num_shards = 2048        # Good write performance
batch_size = 512         # Larger batches for writes
buffer_pool_size = 4096  # More buffers for write traffic
max_connections = 500

[memory]
max_memory = 4294967296  # 4GB
eviction_policy = "allkeys-lru"
```

**Expected:**
- 2.74M SET ops/s (matches Redis)
- 6.87M GET ops/s
- Higher write throughput

---

### Low-Latency Interactive

**Goal:** Minimize latency

```toml
[server]
num_shards = 2048
batch_size = 16          # Small batches
buffer_pool_size = 2048
max_connections = 100

[performance]
tcp_nodelay = true       # Critical!
tcp_keepalive = 30

[memory]
max_memory = 2147483648  # 2GB
eviction_policy = "allkeys-lru"
```

**Expected:**
- Sub-millisecond p50 latency
- Good interactive performance
- Lower overall throughput

---

### High Connection Count (>1000 connections)

**Goal:** Handle many concurrent clients

```toml
[server]
num_shards = 4096        # More parallelism
batch_size = 256
buffer_pool_size = 4096  # 4x typical
max_connections = 5000   # Increase limit

[performance]
tcp_nodelay = true
tcp_keepalive = 120      # Longer keep-alive
```

**System tuning required:**
```bash
# Increase file descriptor limit
ulimit -n 65536

# Increase ephemeral port range
sysctl -w net.ipv4.ip_local_port_range="1024 65535"

# Enable TCP reuse
sysctl -w net.ipv4.tcp_tw_reuse=1
```

---

## Memory Planning

### Estimating Memory Usage

**Formula:**
```
Total Memory = Data Memory + Overhead Memory

Data Memory = num_keys × (key_size + value_size + 100 bytes)
Overhead Memory = (num_shards × 128KB) + (buffer_pool_size × 16KB)
```

**Example (2048 shards, 2048 buffers):**
```
1M keys × 100 bytes avg key = 100MB
1M keys × 1KB avg value = 1000MB
1M keys × 100 bytes entry overhead = 100MB
Shard overhead = 2048 × 128KB = 256MB
Buffer pool = 2048 × 16KB = 32MB

Total = ~1.5GB
```

**Recommendation:** Set `max_memory` to 1.5-2x your calculated data size for safety margin.

---

### Eviction Policies

```toml
[memory]
eviction_policy = "allkeys-lru"  # Recommended
# eviction_policy = "allkeys-random"  # Faster, less accurate
# eviction_policy = "noeviction"      # Return errors when full
```

**Policy comparison:**

| Policy | Accuracy | Performance | Use Case |
|--------|----------|-------------|----------|
| `allkeys-lru` | High | Good | General purpose (recommended) |
| `allkeys-random` | Low | Excellent | When eviction doesn't matter |
| `noeviction` | N/A | Best | Strict memory control |

---

## Benchmarking Your Configuration

### Using redis-benchmark

```bash
# Test your configuration
redis-benchmark -h localhost -p 6379 \
  -t set,get \
  -n 2000000 \
  -c 500 \
  -P 128 \
  --csv

# Compare configurations
./test_config.sh 2048  # Test with 2048 shards
./test_config.sh 4096  # Test with 4096 shards
```

### Custom Benchmark Script

```bash
#!/bin/bash
# benchmark_config.sh

SHARD_COUNT=$1
CONFIG_FILE="redistill-${SHARD_COUNT}.toml"

# Create config
cat > $CONFIG_FILE << EOF
[server]
num_shards = $SHARD_COUNT
batch_size = 256
buffer_pool_size = 2048
EOF

# Start server
./target/release/redistill --config $CONFIG_FILE &
PID=$!
sleep 2

# Run benchmark
redis-benchmark -c 500 -P 128 -n 2000000 -t set,get --csv

# Cleanup
kill $PID
rm $CONFIG_FILE
```

---

## Production Checklist

Before deploying to production:

- [ ] Set appropriate `max_memory` limit
- [ ] Choose eviction policy
- [ ] Configure `max_connections` based on expected load
- [ ] Tune `batch_size` to match your pipeline depth
- [ ] Set `tcp_nodelay = true` for low latency
- [ ] Enable password authentication
- [ ] Consider TLS for sensitive data
- [ ] Set up health check monitoring
- [ ] Benchmark with production-like traffic
- [ ] Monitor memory usage and cache hit rate

---

## Troubleshooting Performance Issues

### High Latency (>10ms p50)

**Possible causes:**
1. `tcp_nodelay = false` (enable it!)
2. `batch_size` too large (reduce to 2x pipeline depth)
3. Too few shards (increase to 2048)
4. Memory pressure (increase `max_memory` or reduce data)

**Fix:**
```toml
tcp_nodelay = true
batch_size = 256
num_shards = 2048
```

---

### Low Throughput (<1M ops/s)

**Possible causes:**
1. Too few shards (increase to 2048)
2. `batch_size` too small (increase to 256)
3. Too few connections (increase clients)
4. Network bottleneck (check bandwidth)

**Fix:**
```toml
num_shards = 2048
batch_size = 256
```

---

### Memory Leaks / Growing Memory

**Possible causes:**
1. No `max_memory` limit set
2. Eviction not working
3. No TTLs on keys

**Fix:**
```toml
[memory]
max_memory = 4294967296  # Set a limit!
eviction_policy = "allkeys-lru"
```

Use TTLs:
```bash
# Set keys with expiry
SET mykey value EX 3600  # 1 hour TTL
```

---

### High CPU Usage

**Possible causes:**
1. Atomic counter overhead (fixed in v1.0.4+)
2. Too many shards (reduce to 2048)
3. LRU updates on every operation (fixed in v1.0.4+)

**Verify optimizations are enabled:**
- Batched atomic counters (256 ops per global update)
- Probabilistic LRU (90% skip rate)

---

## Advanced Optimizations

### Kernel Tuning (Linux)

```bash
# Increase network buffer sizes
sysctl -w net.core.rmem_max=134217728
sysctl -w net.core.wmem_max=134217728

# TCP optimization
sysctl -w net.ipv4.tcp_rmem="4096 87380 134217728"
sysctl -w net.ipv4.tcp_wmem="4096 65536 134217728"

# Reduce TIME_WAIT
sysctl -w net.ipv4.tcp_fin_timeout=15
sysctl -w net.ipv4.tcp_tw_reuse=1

# File descriptor limits
ulimit -n 65536
```

### CPU Pinning (NUMA Systems)

For multi-socket systems, pin Redistill to one NUMA node:

```bash
# Check NUMA topology
numactl --hardware

# Pin to node 0
numactl --cpunodebind=0 --membind=0 ./target/release/redistill
```

### Huge Pages

Enable huge pages for better memory performance:

```bash
# Reserve huge pages
echo 1024 > /proc/sys/vm/nr_hugepages

# Run with huge pages
MALLOC_CONF="thp:always" ./target/release/redistill
```

---

## Performance Monitoring

### Key Metrics to Track

1. **Throughput:** ops/s (from `INFO` command)
2. **Latency:** p50, p99, p999 (from redis-benchmark)
3. **Memory:** Used vs max (from health endpoint)
4. **Connections:** Active connections (from `INFO`)
5. **Cache hits:** Hit rate (application-level)

### Health Check Endpoint

```bash
curl http://localhost:8080/health
```

Response:
```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "active_connections": 245,
  "total_connections": 10532,
  "rejected_connections": 0,
  "memory_used": 1073741824,
  "max_memory": 4294967296,
  "evicted_keys": 1234,
  "total_commands": 125000000
}
```

### Prometheus Metrics (Coming Soon)

Integration with Prometheus for production monitoring.

---

## Summary

**Best Practices:**
1. Start with default configuration (2048 shards)
2. Tune `batch_size` to match your pipeline depth
3. Set `max_memory` and choose eviction policy
4. Enable `tcp_nodelay` for low latency
5. Benchmark with production-like traffic
6. Monitor and adjust based on real workload

**Performance Tiers:**

| Configuration | GET ops/s | SET ops/s | Memory | Use Case |
|--------------|-----------|-----------|---------|----------|
| Conservative (256 shards) | 6.5M | 2.6M | Low | Memory-constrained |
| **Recommended (2048 shards)** | **6.9M** | **2.7M** | **Moderate** | **General purpose** |
| Maximum (4096 shards) | 7.5M | 2.7M | High | Read-heavy |

For most deployments, **2048 shards is the sweet spot** balancing performance, memory, and ease of use.

---

## Getting Help

- **Issues:** https://github.com/shaikh-shahid/redistill/issues
- **Discussions:** https://github.com/shaikh-shahid/redistill/discussions
- **Documentation:** https://github.com/shaikh-shahid/redistill/tree/main/docs

