#!/bin/bash

echo "======================================"
echo "  REDIS KILLER BENCHMARK TEST"
echo "======================================"
echo ""

# Check if server is running
if ! lsof -i :6379 > /dev/null 2>&1; then
    echo "❌ Error: Server is not running on port 6379"
    echo "Please start the server first: ./target/release/redis-killer"
    exit 1
fi

echo "✅ Server is running"
echo "File descriptor limit: $(ulimit -n)"
echo ""

echo "Test 1: Without pipelining (-P 1, interactive mode)"
echo "-----------------------------------------------------"
redis-benchmark -h 127.0.0.1 -p 6379 -t set,get -n 1000000 -c 200 -P 1 -q

echo ""
echo "Test 2: With pipelining (-P 16, typical production)"
echo "-----------------------------------------------------"
redis-benchmark -h 127.0.0.1 -p 6379 -t set,get -n 10000000 -c 50 -P 16 -q

echo ""
echo "Test 3: High concurrency (-c 300, -P 32)"
echo "-----------------------------------------------------"
sleep 2  # Let connections close
redis-benchmark -h 127.0.0.1 -p 6379 -t set,get -n 5000000 -c 300 -P 32 -q

echo ""
echo "Test 4: Extreme pipelining (-c 50, -P 64)"
echo "-----------------------------------------------------"
redis-benchmark -h 127.0.0.1 -p 6379 -t set,get -n 10000000 -c 50 -P 64 -q

echo ""
echo "======================================"
echo "  BENCHMARK COMPLETE"
echo "======================================"
echo ""
echo "Summary:"
echo "  Test 1: Single commands (redis-cli like)"
echo "  Test 2: Production workload (BEST PERFORMANCE)"
echo "  Test 3: High concurrency stress test"
echo "  Test 4: Maximum pipelining test"

