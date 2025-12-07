#!/bin/bash
#
# Test edgeProxy Routing Behavior
# Run this while start_local_env.sh is running
#

set -e

PROXY_HOST="localhost"
PROXY_PORT="8080"

echo "=========================================="
echo "  edgeProxy Routing Tests"
echo "=========================================="
echo ""

# Test 1: Basic connectivity
echo "[Test 1] Basic Connectivity"
echo "----------------------------"
RESPONSE=$(echo "" | nc -w 2 $PROXY_HOST $PROXY_PORT 2>/dev/null || echo "FAILED")
if [[ "$RESPONSE" == *"Backend:"* ]]; then
    echo "SUCCESS: Connected to backend"
    echo "Response: $RESPONSE"
else
    echo "FAILED: Could not connect"
    echo "Response: $RESPONSE"
fi
echo ""

# Test 2: Multiple connections (should hit same backend - affinity)
echo "[Test 2] Client Affinity (Sticky Sessions)"
echo "-------------------------------------------"
echo "Making 3 consecutive connections..."
for i in 1 2 3; do
    RESPONSE=$(echo "" | nc -w 1 $PROXY_HOST $PROXY_PORT 2>/dev/null | head -1)
    echo "  Connection $i: $RESPONSE"
done
echo ""

# Test 3: Echo test
echo "[Test 3] Echo Through Proxy"
echo "----------------------------"
RESPONSE=$(echo "Hello from client!" | nc -w 2 $PROXY_HOST $PROXY_PORT 2>/dev/null)
echo "Sent: Hello from client!"
echo "Received:"
echo "$RESPONSE"
echo ""

# Test 4: Check backend distribution (different source simulation would need more setup)
echo "[Test 4] Backend Health Check"
echo "------------------------------"
echo "Testing direct backend connectivity..."
for port in 9001 9002 9003; do
    RESPONSE=$(echo "" | nc -w 1 localhost $port 2>/dev/null | head -1)
    if [[ -n "$RESPONSE" ]]; then
        echo "  Port $port: $RESPONSE"
    else
        echo "  Port $port: NOT RESPONDING"
    fi
done
echo ""

echo "=========================================="
echo "  Tests Complete"
echo "=========================================="
