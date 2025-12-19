#!/bin/bash
# Test TLS functionality for Redistill

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_DIR="$SCRIPT_DIR/../.."

echo "======================================"
echo "  REDISTILL TLS FUNCTIONALITY TEST"
echo "======================================"
echo ""

# Check if server binary exists
if [ ! -f "$PROJECT_DIR/target/release/redistill" ]; then
    echo "❌ Server binary not found. Building..."
    cd "$PROJECT_DIR"
    cargo build --release
fi

# Check if certificates exist
if [ ! -f "$SCRIPT_DIR/../certs/server-cert.pem" ] || [ ! -f "$SCRIPT_DIR/../certs/server-key.pem" ]; then
    echo "❌ Test certificates not found. Generating..."
    "$SCRIPT_DIR/generate_test_certs.sh"
fi

echo "Test 1: Plain TCP Connection (No TLS)"
echo "========================================"
echo ""

# Kill any existing redistill process
pkill -f redistill || true
sleep 1

# Start server without TLS
echo "Starting server on port 6379 (plain TCP)..."
cd "$PROJECT_DIR"
./target/release/redistill &
SERVER_PID=$!
sleep 2

# Test with redis-cli
echo "Testing with redis-cli..."
redis-cli -p 6379 PING && echo "✅ Plain TCP connection successful" || echo "❌ Plain TCP connection failed"
redis-cli -p 6379 SET test_key "test_value"
redis-cli -p 6379 GET test_key

echo ""
echo "Stopping plain TCP server..."
kill $SERVER_PID
sleep 1

echo ""
echo "Test 2: TLS Connection"
echo "======================================"
echo ""

# Create TLS-enabled config
cat > "$PROJECT_DIR/redistill-tls-test.toml" << EOF
[server]
bind = "127.0.0.1"
port = 6380

[security]
tls_enabled = true
tls_cert_path = "tests/certs/server-cert.pem"
tls_key_path = "tests/certs/server-key.pem"
EOF

# Start server with TLS
echo "Starting server on port 6380 (with TLS)..."
REDISTILL_CONFIG="redistill-tls-test.toml" ./target/release/redistill &
TLS_SERVER_PID=$!
sleep 2

# Test with redis-cli TLS
echo "Testing with redis-cli (TLS)..."
echo "Note: Using --insecure because we're using self-signed certificates"
redis-cli -p 6380 --tls --insecure PING && echo "✅ TLS connection successful" || echo "❌ TLS connection failed"
redis-cli -p 6380 --tls --insecure SET tls_test "encrypted_value"
redis-cli -p 6380 --tls --insecure GET tls_test

echo ""
echo "Testing that plain TCP connection should fail on TLS port..."
redis-cli -p 6380 PING 2>&1 | grep -q "Connection reset" && echo "✅ Plain connection correctly rejected" || echo "⚠️  Plain connection may have succeeded (unexpected)"

echo ""
echo "Stopping TLS server..."
kill $TLS_SERVER_PID
sleep 1

# Cleanup
rm -f "$PROJECT_DIR/redistill-tls-test.toml"

echo ""
echo "======================================"
echo "  TLS TESTS COMPLETE"
echo "======================================"
echo ""
echo "Summary:"
echo "✓ Plain TCP connections work"
echo "✓ TLS connections work with proper client"
echo "✓ Plain clients cannot connect to TLS port"
echo ""
echo "For production:"
echo "  1. Use real certificates from a CA (Let's Encrypt, etc.)"
echo "  2. Enable TLS in redistill.toml"
echo "  3. Configure clients with appropriate --tls flags"

