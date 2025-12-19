# TLS/SSL Setup Guide for Redistill

This guide explains how to enable TLS/SSL encryption for Redistill connections.

## Table of Contents

- [Why TLS?](#why-tls)
- [Quick Start](#quick-start)
- [Development Setup](#development-setup)
- [Production Setup](#production-setup)
- [Client Configuration](#client-configuration)
- [Troubleshooting](#troubleshooting)

## Why TLS?

TLS (Transport Layer Security) provides:
- **Encryption**: Protects data in transit from eavesdropping
- **Authentication**: Verifies server identity
- **Integrity**: Prevents tampering with data

**When to use TLS:**
- Communication over untrusted networks (internet, public clouds)
- Compliance requirements (PCI-DSS, HIPAA, etc.)
- Multi-tenant environments
- Production deployments with sensitive data

**When TLS might not be needed:**
- Local development
- Internal networks with network-level security
- Performance-critical deployments on secure networks (adds ~5-10% latency)

## Quick Start

### 1. Generate Test Certificates (Development Only)

```bash
./tests/scripts/generate_test_certs.sh
```

This creates self-signed certificates in `tests/certs/`:
- `server-cert.pem` - Public certificate
- `server-key.pem` - Private key

⚠️ **WARNING**: These are for testing only. Do NOT use in production!

### 2. Enable TLS in Configuration

Edit `redistill.toml`:

```toml
[security]
tls_enabled = true
tls_cert_path = "tests/certs/server-cert.pem"
tls_key_path = "tests/certs/server-key.pem"
```

### 3. Start the Server

```bash
./target/release/redistill
```

You should see:

```
🔐 TLS/SSL enabled
   • Certificate: tests/certs/server-cert.pem
   • Private Key: tests/certs/server-key.pem
```

### 4. Test with redis-cli

```bash
# With self-signed certs, use --insecure
redis-cli -p 6379 --tls --insecure PING

# Set and get a value
redis-cli -p 6379 --tls --insecure SET mykey "encrypted!"
redis-cli -p 6379 --tls --insecure GET mykey
```

## Development Setup

### Using Custom Config Files

```bash
# Create a TLS-specific config
cp redistill.toml redistill-tls.toml

# Edit and enable TLS
vim redistill-tls.toml

# Run with custom config
REDISTILL_CONFIG=redistill-tls.toml ./target/release/redistill
```

### Running Test Suite

```bash
./tests/scripts/test_tls.sh
```

This tests both plain TCP and TLS connections.

## Production Setup

### 1. Get Real Certificates

**Option A: Let's Encrypt (Free, Automated)**

```bash
# Install certbot
sudo apt-get install certbot  # Ubuntu/Debian
brew install certbot           # macOS

# Get certificate (requires domain name)
sudo certbot certonly --standalone -d your-domain.com

# Certificates will be in:
# /etc/letsencrypt/live/your-domain.com/fullchain.pem
# /etc/letsencrypt/live/your-domain.com/privkey.pem
```

**Option B: Cloud Provider Certificates**
- AWS Certificate Manager (ACM)
- Google Cloud Certificate Manager
- Azure Key Vault Certificates

**Option C: Corporate CA**
- Get certificate from your organization's Certificate Authority
- Follow your company's certificate request process

### 2. Configure Redistill

```toml
[server]
bind = "0.0.0.0"
port = 6379

[security]
tls_enabled = true
tls_cert_path = "/etc/letsencrypt/live/your-domain.com/fullchain.pem"
tls_key_path = "/etc/letsencrypt/live/your-domain.com/privkey.pem"
password = "your-strong-password-here"
```

### 3. Set File Permissions

```bash
# Certificates should be readable by Redistill user
sudo chown redistill:redistill /path/to/cert.pem
sudo chown redistill:redistill /path/to/key.pem
sudo chmod 644 /path/to/cert.pem
sudo chmod 600 /path/to/key.pem  # Private key should be protected
```

### 4. Certificate Renewal

Let's Encrypt certificates expire every 90 days. Set up auto-renewal:

```bash
# Test renewal
sudo certbot renew --dry-run

# Add cron job for auto-renewal
sudo crontab -e

# Add this line (checks twice daily)
0 0,12 * * * certbot renew --quiet --post-hook "systemctl restart redistill"
```

### 5. Firewall Configuration

```bash
# Allow TLS port
sudo ufw allow 6379/tcp
```

## Client Configuration

### redis-cli

```bash
# With proper CA-signed certificate
redis-cli -h your-domain.com -p 6379 --tls

# With self-signed certificate (development)
redis-cli -h localhost -p 6379 --tls --insecure

# With authentication
redis-cli -h your-domain.com -p 6379 --tls -a your-password

# Test connection
redis-cli -h your-domain.com -p 6379 --tls PING
```

### Node.js (ioredis)

```javascript
const Redis = require('ioredis');

const client = new Redis({
  host: 'your-domain.com',
  port: 6379,
  tls: {
    // For production with proper certs
    rejectUnauthorized: true
    
    // For self-signed certs (development only)
    // rejectUnauthorized: false
  },
  password: 'your-password'
});

client.ping((err, result) => {
  console.log(result); // PONG
});
```

### Python (redis-py)

```python
import redis

# Production
client = redis.Redis(
    host='your-domain.com',
    port=6379,
    ssl=True,
    ssl_cert_reqs='required',
    password='your-password'
)

# Development (self-signed)
client = redis.Redis(
    host='localhost',
    port=6379,
    ssl=True,
    ssl_cert_reqs='none',
    password='your-password'
)

print(client.ping())  # True
```

### Go (go-redis)

```go
import (
    "crypto/tls"
    "github.com/go-redis/redis/v8"
)

client := redis.NewClient(&redis.Options{
    Addr:     "your-domain.com:6379",
    Password: "your-password",
    TLSConfig: &tls.Config{
        // Production
        InsecureSkipVerify: false,
        
        // Development (self-signed)
        // InsecureSkipVerify: true,
    },
})

pong, err := client.Ping(ctx).Result()
fmt.Println(pong) // PONG
```

### Java (Jedis)

```java
import redis.clients.jedis.Jedis;
import redis.clients.jedis.JedisShardInfo;
import javax.net.ssl.SSLSocketFactory;

JedisShardInfo shardInfo = new JedisShardInfo("your-domain.com", 6379, true);
shardInfo.setPassword("your-password");

// Production
shardInfo.setSslSocketFactory(SSLSocketFactory.getDefault());

Jedis jedis = new Jedis(shardInfo);
System.out.println(jedis.ping()); // PONG
```

## Troubleshooting

### Connection Refused

**Problem**: `Could not connect to Redis at 127.0.0.1:6379: Connection refused`

**Solutions**:
1. Make sure server is running: `ps aux | grep redistill`
2. Check if TLS is enabled: Look for "🔐 TLS/SSL enabled" in startup message
3. Verify port is correct in both server config and client

### Certificate Errors

**Problem**: `certificate verify failed` or `SSL certificate problem`

**Solutions**:
1. **Development**: Use `--insecure` flag or `rejectUnauthorized: false`
2. **Production**: Verify certificate paths are correct
3. Check certificate hasn't expired: `openssl x509 -in cert.pem -noout -dates`
4. Ensure certificate matches domain name
5. Check file permissions: `ls -l /path/to/cert.pem`

### TLS Handshake Failed

**Problem**: `TLS handshake failed: received corrupt message`

**Cause**: Client trying to connect with plain TCP to TLS port

**Solution**: Add `--tls` flag to redis-cli or enable TLS in your client library

### Permission Denied

**Problem**: `Failed to load TLS configuration: Permission denied`

**Solutions**:
```bash
# Check file permissions
ls -l /path/to/cert.pem /path/to/key.pem

# Fix permissions
sudo chmod 644 /path/to/cert.pem
sudo chmod 600 /path/to/key.pem
sudo chown redistill:redistill /path/to/*.pem
```

### Performance Concerns

**Problem**: TLS is too slow

**Solutions**:
1. Use TLS 1.3 (automatically used with rustls)
2. Keep connections alive (connection pooling)
3. Consider TLS offloading with a load balancer
4. For internal secure networks, plain TCP might be acceptable
5. Profile to confirm TLS is the bottleneck (usually adds 5-10% latency)

### Testing Certificate Validity

```bash
# Check certificate details
openssl x509 -in cert.pem -text -noout

# Check certificate expiry
openssl x509 -in cert.pem -noout -dates

# Test TLS connection manually
openssl s_client -connect localhost:6379 -tls1_3

# Verify certificate and key match
openssl x509 -noout -modulus -in cert.pem | openssl md5
openssl rsa -noout -modulus -in key.pem | openssl md5
# The MD5 hashes should match
```

## Security Best Practices

1. **Always use TLS for production** over untrusted networks
2. **Use strong passwords** with TLS (authentication + encryption)
3. **Rotate certificates** before expiry
4. **Protect private keys**: chmod 600, never commit to git
5. **Use Let's Encrypt** for automated certificate management
6. **Monitor certificate expiry**: Set up alerts
7. **Test certificate renewal** before expiry
8. **Keep rustls updated**: Security patches are important
9. **Use TLS 1.3**: Faster and more secure (default with rustls)
10. **Log TLS errors**: Monitor for handshake failures

## Performance Impact

| Configuration | Throughput | Latency Impact |
|--------------|------------|----------------|
| Plain TCP | Baseline | 0% |
| TLS 1.3 | ~95% | +5-10% |

**Note**: TLS overhead is minimal with:
- Connection pooling (amortizes handshake cost)
- TLS 1.3 (faster than 1.2)
- Keep-alive connections
- Modern CPUs with AES-NI

For Redistill's use case (high-performance caching), TLS is recommended for production deployments over untrusted networks.

## Additional Resources

- [Let's Encrypt Getting Started](https://letsencrypt.org/getting-started/)
- [TLS 1.3 Spec](https://datatracker.ietf.org/doc/html/rfc8446)
- [rustls Documentation](https://docs.rs/rustls/)
- [Redis TLS Documentation](https://redis.io/docs/manual/security/encryption/)

## Support

For issues or questions:
1. Check this guide first
2. Review test scripts in `tests/scripts/`
3. Run TLS tests: `./tests/scripts/test_tls.sh`
4. Open an issue on GitHub with details

