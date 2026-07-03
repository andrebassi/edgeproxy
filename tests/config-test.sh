#!/bin/bash
# edgeProxy Configuration Test Suite v0.4.0
# Tests all environment variable combinations

set -e

BINARY="./target/release/edge-proxy"
TEST_RESULTS=""
PASSED=0
FAILED=0

log_result() {
    local test_name="$1"
    local status="$2"
    local details="$3"

    if [ "$status" = "PASS" ]; then
        echo "[PASS] $test_name"
        TEST_RESULTS="$TEST_RESULTS\n| $test_name | PASS | $details |"
        ((PASSED++))
    else
        echo "[FAIL] $test_name: $details"
        TEST_RESULTS="$TEST_RESULTS\n| $test_name | FAIL | $details |"
        ((FAILED++))
    fi
}

start_proxy() {
    local name="$1"
    shift

    # Kill any existing
    pkill -f edge-proxy 2>/dev/null || true
    sleep 0.5

    # Start with env vars
    "$@" $BINARY &
    PROXY_PID=$!
    sleep 2

    # Check if running
    if kill -0 $PROXY_PID 2>/dev/null; then
        return 0
    else
        return 1
    fi
}

stop_proxy() {
    pkill -f edge-proxy 2>/dev/null || true
    sleep 0.5
}

echo "═══════════════════════════════════════════════════════════"
echo "  edgeProxy Configuration Test Suite v0.4.0"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Test 1: Default configuration (TCP only)
echo "Test 1: Default configuration (TCP only)"
if start_proxy "default" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=sa; then

    # Verify TCP is listening
    if nc -z 127.0.0.1 18080 2>/dev/null; then
        log_result "TCP Proxy Default" "PASS" "Listening on 127.0.0.1:18080"
    else
        log_result "TCP Proxy Default" "FAIL" "Not listening"
    fi
else
    log_result "TCP Proxy Default" "FAIL" "Failed to start"
fi
stop_proxy

# Test 2: All services enabled
echo ""
echo "Test 2: All services enabled"
if start_proxy "all-services" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=eu \
    EDGEPROXY_TLS_ENABLED=true \
    EDGEPROXY_TLS_LISTEN_ADDR=127.0.0.1:18443 \
    EDGEPROXY_API_ENABLED=true \
    EDGEPROXY_API_LISTEN_ADDR=127.0.0.1:18081 \
    EDGEPROXY_DNS_ENABLED=true \
    EDGEPROXY_DNS_LISTEN_ADDR=127.0.0.1:15353 \
    EDGEPROXY_DNS_DOMAIN=test.local \
    EDGEPROXY_REPLICATION_ENABLED=true \
    EDGEPROXY_REPLICATION_NODE_ID=test-node-1 \
    EDGEPROXY_REPLICATION_GOSSIP_ADDR=127.0.0.1:14001 \
    EDGEPROXY_REPLICATION_TRANSPORT_ADDR=127.0.0.1:14002 \
    EDGEPROXY_REPLICATION_CLUSTER_NAME=test-cluster; then

    sleep 2

    # Verify all services
    nc -z 127.0.0.1 18080 2>/dev/null && log_result "TCP Proxy (all)" "PASS" "18080" || log_result "TCP Proxy (all)" "FAIL" "Not listening"
    nc -z 127.0.0.1 18443 2>/dev/null && log_result "TLS Server" "PASS" "18443 (self-signed)" || log_result "TLS Server" "FAIL" "Not listening"
    nc -z 127.0.0.1 18081 2>/dev/null && log_result "API Server" "PASS" "18081" || log_result "API Server" "FAIL" "Not listening"
    nc -z 127.0.0.1 14002 2>/dev/null && log_result "Replication Transport" "PASS" "14002 (QUIC)" || log_result "Replication Transport" "FAIL" "Not listening"

    # Test API endpoint
    if curl -s http://127.0.0.1:18081/health | grep -q '"status":"ok"'; then
        log_result "API Health Endpoint" "PASS" "Returns OK"
    else
        log_result "API Health Endpoint" "FAIL" "Invalid response"
    fi
else
    log_result "All Services" "FAIL" "Failed to start"
fi
stop_proxy

# Test 3: Custom regions
echo ""
echo "Test 3: Region configurations"
for region in sa us eu ap; do
    if start_proxy "region-$region" env \
        EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
        EDGEPROXY_DB_PATH=routing.db \
        EDGEPROXY_REGION=$region; then
        log_result "Region $region" "PASS" "Started with region=$region"
    else
        log_result "Region $region" "FAIL" "Failed to start"
    fi
    stop_proxy
done

# Test 4: API endpoints
echo ""
echo "Test 4: API endpoints"
if start_proxy "api-test" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=eu \
    EDGEPROXY_API_ENABLED=true \
    EDGEPROXY_API_LISTEN_ADDR=127.0.0.1:18081 \
    EDGEPROXY_HEARTBEAT_TTL_SECS=30; then

    sleep 2

    # Test health
    curl -s http://127.0.0.1:18081/health | grep -q '"status":"ok"' && \
        log_result "GET /health" "PASS" "Returns status ok" || \
        log_result "GET /health" "FAIL" "Invalid response"

    # Test register
    REGISTER_RESP=$(curl -s -X POST http://127.0.0.1:18081/api/v1/register \
        -H "Content-Type: application/json" \
        -d '{"id":"test-1","app":"test","region":"eu","ip":"127.0.0.1","port":9001}')
    echo "$REGISTER_RESP" | grep -q '"registered":true' && \
        log_result "POST /api/v1/register" "PASS" "Backend registered" || \
        log_result "POST /api/v1/register" "FAIL" "Registration failed"

    # Test list backends
    curl -s http://127.0.0.1:18081/api/v1/backends | grep -q '"total":1' && \
        log_result "GET /api/v1/backends" "PASS" "Lists 1 backend" || \
        log_result "GET /api/v1/backends" "FAIL" "Invalid response"

    # Test get backend
    curl -s http://127.0.0.1:18081/api/v1/backends/test-1 | grep -q '"id":"test-1"' && \
        log_result "GET /api/v1/backends/:id" "PASS" "Returns backend details" || \
        log_result "GET /api/v1/backends/:id" "FAIL" "Backend not found"

    # Test heartbeat
    curl -s -X POST http://127.0.0.1:18081/api/v1/heartbeat/test-1 | grep -q '"status":"ok"' && \
        log_result "POST /api/v1/heartbeat/:id" "PASS" "Heartbeat updated" || \
        log_result "POST /api/v1/heartbeat/:id" "FAIL" "Heartbeat failed"

    # Test delete
    curl -s -X DELETE http://127.0.0.1:18081/api/v1/backends/test-1 | grep -q '"deleted":true' && \
        log_result "DELETE /api/v1/backends/:id" "PASS" "Backend deleted" || \
        log_result "DELETE /api/v1/backends/:id" "FAIL" "Delete failed"
else
    log_result "API Test Setup" "FAIL" "Failed to start"
fi
stop_proxy

# Test 5: TLS with self-signed cert
echo ""
echo "Test 5: TLS server"
if start_proxy "tls-test" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=eu \
    EDGEPROXY_TLS_ENABLED=true \
    EDGEPROXY_TLS_LISTEN_ADDR=127.0.0.1:18443; then

    sleep 2

    # Test TLS connection (ignore cert validation)
    if curl -sk https://127.0.0.1:18443/ --max-time 2 2>/dev/null || [ $? -eq 52 ]; then
        log_result "TLS Self-Signed" "PASS" "TLS handshake successful"
    else
        log_result "TLS Self-Signed" "FAIL" "TLS handshake failed"
    fi
else
    log_result "TLS Setup" "FAIL" "Failed to start"
fi
stop_proxy

# Test 6: Binding TTL configuration
echo ""
echo "Test 6: Binding configuration"
if start_proxy "binding-test" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=eu \
    EDGEPROXY_BINDING_TTL_SECS=300 \
    EDGEPROXY_BINDING_GC_INTERVAL_SECS=30; then
    log_result "Binding TTL Config" "PASS" "TTL=300s, GC=30s"
else
    log_result "Binding TTL Config" "FAIL" "Failed to start"
fi
stop_proxy

# Test 7: DB reload interval
echo ""
echo "Test 7: Database reload"
if start_proxy "db-reload" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=eu \
    EDGEPROXY_DB_RELOAD_SECS=2; then
    log_result "DB Reload Config" "PASS" "Reload every 2s"
else
    log_result "DB Reload Config" "FAIL" "Failed to start"
fi
stop_proxy

# Test 8: Debug mode
echo ""
echo "Test 8: Debug mode"
if start_proxy "debug" env \
    EDGEPROXY_LISTEN_ADDR=127.0.0.1:18080 \
    EDGEPROXY_DB_PATH=routing.db \
    EDGEPROXY_REGION=eu \
    DEBUG=1; then
    log_result "Debug Mode" "PASS" "DEBUG=1 accepted"
else
    log_result "Debug Mode" "FAIL" "Failed to start"
fi
stop_proxy

# Summary
echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  Test Summary"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "Total: $((PASSED + FAILED)) tests"
echo "Passed: $PASSED"
echo "Failed: $FAILED"
echo ""

if [ $FAILED -eq 0 ]; then
    echo "All tests passed!"
    exit 0
else
    echo "Some tests failed!"
    exit 1
fi
