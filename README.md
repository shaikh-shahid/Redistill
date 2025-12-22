# Redistill

> A high-performance, Redis-compatible in-memory cache written in Rust that's 2x faster than Redis.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/shaikh-shahid/redistill/workflows/CI/badge.svg)](https://github.com/shaikh-shahid/redistill/actions/workflows/ci.yml)
[![Release](https://github.com/shaikh-shahid/redistill/workflows/Release/badge.svg)](https://github.com/shaikh-shahid/redistill/actions/workflows/release.yml)

## Overview

Redistill is a drop-in Redis replacement optimized for caching and ephemeral data workloads. It implements the Redis protocol (RESP) and achieves 2x higher throughput than Redis by eliminating persistence overhead and optimizing for in-memory operations.

**Key characteristics:**
- Redis protocol compatible
- Up to 5.9M GET operations/second (vs Redis 3.1M ops/s)
- 49% faster SET operations under production workloads
- Zero persistence overhead
- Production-ready security and monitoring features

## Performance

Benchmark results on AWS c7i-flex.8xlarge (32 cores, production workload with `batch_size=16`):

| Workload | Redistill | Redis | Improvement |
|----------|-----------|-------|-------------|
| Interactive (-c 1, -P 1) | 20K ops/s | 20K ops/s | Similar |
| Production SET (-c 50, -P 16) | 2.34M ops/s | 1.57M ops/s | **+49%** |
| Production GET (-c 50, -P 16) | 2.38M ops/s | 1.95M ops/s | **+21%** |
| High Concurrency GET (-c 300, -P 32) | 3.16M ops/s | 2.70M ops/s | **+17%** |
| Extreme GET (-c 100, -P 64) | 5.99M ops/s | 3.10M ops/s | **+93%** |
| Ultra GET (-c 500, -P 128) | 5.87M ops/s | 3.40M ops/s | **+72%** |

**Key Takeaways:**
- GET operations with high pipelining: up to **93% faster** than Redis
- Production workloads: **21-49% improvement** over Redis
- Linear scalability with pipelining and concurrency

### Tuning for Extreme Pipelines

For workloads with very deep pipelining (P > 64), increase the `batch_size` to match your pipeline depth.

**Performance with `batch_size=256` on AWS c7i-flex.8xlarge:**

| Workload | Redistill (batch=16) | Redistill (batch=256) | Redis | Improvement |
|----------|----------------------|-----------------------|-------|-------------|
| Extreme SET (-c 100, -P 64) | 2.26M ops/s | **2.53M ops/s** | 2.51M ops/s | **+0.8%** vs Redis |
| Extreme GET (-c 100, -P 64) | 5.99M ops/s | **6.80M ops/s** | 3.23M ops/s | **+110%** vs Redis |
| Ultra SET (-c 500, -P 128) | 2.18M ops/s | **2.52M ops/s** | 2.67M ops/s | -5.6% vs Redis |
| Ultra GET (-c 500, -P 128) | 5.87M ops/s | **6.83M ops/s** | 3.47M ops/s | **+97%** vs Redis |

**Configuration:**
```toml
[server]
batch_size = 256  # Match or exceed your pipeline depth
```

**Performance Gains with Larger Batch Size:**
- SET operations: +12-16% improvement at extreme pipeline depths
- GET operations: +14-16% improvement, maintaining ~2x Redis performance
- Fewer write syscalls = lower overhead at high concurrency

## Quick Start

### Requirements

- Rust 1.70 or higher
- Linux, macOS, or Windows
- 1GB RAM minimum (configurable)

### Installation

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.sh | sh

# Build from source
git clone https://github.com/yourusername/redistill
cd redistill
cargo build --release

# Run server
./target/release/redistill
```

### Basic Usage

```bash
# Test connection
redis-cli ping
# PONG

# Set and get values
redis-cli set mykey "hello world"
# OK

redis-cli get mykey
# "hello world"
```

## Configuration

Create `redistill.toml` in the working directory:

```toml
[server]
bind = "127.0.0.1"
port = 6379
max_connections = 10000
health_check_port = 8080

[security]
password = "your-password"
tls_enabled = false

[memory]
max_memory = 2147483648  # 2GB
eviction_policy = "allkeys-lru"
```

See [Configuration Reference](docs/CONFIG.md) for all options.

## Production Features

**Security**:
- Password authentication
- TLS/SSL encryption
- Connection limits and rate limiting

**Reliability**:
- Memory limits with automatic eviction (LRU, Random, No-eviction)
- Graceful shutdown
- Health check HTTP endpoint

**Monitoring**:
- INFO command with server statistics
- HTTP health endpoint (JSON)
- Real-time metrics tracking

## Supported Commands

Core caching commands:
- `SET key value [EX seconds]` - Store with optional TTL
- `GET key` - Retrieve value
- `DEL key [key ...]` - Delete keys
- `EXISTS key` - Check existence
- `PING` - Health check
- `INFO` - Server statistics
- `FLUSHDB` - Clear all data
- `AUTH password` - Authenticate

## Use Cases

**Recommended for:**
- HTTP session storage
- API response caching
- Rate limiting counters
- Real-time leaderboards
- Temporary data storage

**Not recommended for:**
- Persistent data storage (no disk persistence)
- Financial or transactional data
- Data that cannot be regenerated

## Client Libraries

Redistill is compatible with all Redis clients:

**Python** (`redis-py`):
```python
import redis
r = redis.Redis(host='localhost', port=6379, password='your-password')
r.set('key', 'value')
value = r.get('key')
```

**Node.js** (`ioredis`):
```javascript
const Redis = require('ioredis');
const redis = new Redis({host: 'localhost', port: 6379, password: 'your-password'});
await redis.set('key', 'value');
const value = await redis.get('key');
```

**Go** (`go-redis`):
```go
import "github.com/go-redis/redis/v8"

client := redis.NewClient(&redis.Options{
    Addr: "localhost:6379",
    Password: "your-password",
})
client.Set(ctx, "key", "value", 0)
```

## Monitoring

**Health check** (HTTP endpoint):
```bash
curl http://localhost:8080/health
```

**Server statistics** (Redis protocol):
```bash
redis-cli INFO
```

## Documentation

- [Quick Start](docs/QUICKSTART.md) - Get started in 5 minutes
- [Production Guide](docs/PRODUCTION_GUIDE.md) - Deployment best practices
- [Configuration Reference](docs/CONFIG.md) - Complete configuration options
- [Features](docs/FEATURES.md) - Supported features and roadmap

## Design Philosophy

Redistill trades persistence for performance. By eliminating disk I/O, AOF logging, and RDB snapshots, it achieves 2x higher throughput than Redis. This makes it ideal for caching workloads where data can be regenerated.

For workloads requiring persistence, use Redis with AOF/RDB, or use Redistill as a cache tier in front of a persistent database.

## Frequently Asked Questions

**Q: Is this production-ready?**  
A: Yes. Redistill includes authentication, TLS, memory limits, connection limits, and health checks.

**Q: Can I migrate from Redis?**  
A: Yes, for caching workloads. Redistill implements the Redis protocol but does not support persistence. Review the [Features](docs/FEATURES.md) document for command compatibility.

**Q: How do I handle high availability?**  
A: Use client-side sharding or a proxy like Twemproxy. Clustering support is on the roadmap.

**Q: What about memory management?**  
A: Configure `max_memory` and `eviction_policy` in the configuration. Redistill automatically evicts keys when the limit is reached.

**Q: Is it stable?**  
A: Yes. Redistill has been tested with redis-benchmark and production workloads. All core functionality is stable.

## Benchmarking

```bash
# Run built-in test suite
./tests/benchmarks/run_benchmarks.sh

# Use redis-benchmark
redis-benchmark -t set,get -n 1000000 -c 50 -P 16 -q
```

## Contributing

Contributions are welcome. Please:
1. Open an issue to discuss proposed changes
2. Follow Rust coding conventions
3. Include tests for new features
4. Update relevant documentation

## License

MIT License - see LICENSE file for details.

## Acknowledgments

Built with:
- [Tokio](https://tokio.rs/) - Async runtime
- [DashMap](https://github.com/xacrimon/dashmap) - Concurrent hash map
- [Redis Protocol](https://redis.io/docs/reference/protocol-spec/) - RESP compatibility

## Comparison with Redis

| Feature | Redistill | Redis |
|---------|-----------|-------|
| Throughput (pipelined GET) | 5.9M ops/s | 3.1M ops/s |
| Throughput (pipelined SET) | 2.3M ops/s | 1.6M ops/s |
| Latency (p50, pipelined) | 0.21ms | 0.35ms |
| Memory overhead | Minimal | Moderate |
| Persistence | No | Yes (AOF/RDB) |
| Replication | No | Yes |
| Clustering | No | Yes |
| Data types | String | String, List, Set, Hash, etc. |
| Use case | High-performance caching | General purpose |

Redistill is designed for specific use cases where maximum cache performance is required and persistence is not needed.
