#!/bin/bash

echo "==================================="
echo "  REDISTILL vs DRAGONFLY vs REDIS"
echo "==================================="

# Start each server on different ports
echo "Starting Redis..."
redis-server --port 6379 --save "" --appendonly no &
REDIS_PID=$!

echo "Starting Dragonfly..."
./dragonfly --port 6380 --alsologtostderr &
DRAGONFLY_PID=$!

echo "Starting Redistill..."
./redistill &
REDISTILL_PID=$!

sleep 5

# Run benchmarks
for port in 6379 6380 6381; do
    name=$([ $port -eq 6379 ] && echo "Redis" || [ $port -eq 6380 ] && echo "Dragonfly" || echo "Redistill")
    echo ""
    echo "======================================="
    echo "Testing $name on port $port"
    echo "======================================="

    memtier_benchmark \
        --server=localhost \
        --port=$port \
        --protocol=redis \
        --clients=20 \
        --threads=8 \
        --pipeline=30 \
        --data-size=256 \
        --key-pattern=R:R \
        --ratio=1:1 \
        --test-time=60 \
        --json-out-file="results_${name}.json"
done

# Cleanup
kill $REDIS_PID $DRAGONFLY_PID $REDISTILL_PID