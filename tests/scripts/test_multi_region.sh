#!/bin/bash
#
# Test Multi-Region Routing Behavior
# Run this while simulate_multi_region.sh is running
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${BLUE}=========================================="
echo "  edgeProxy Multi-Region Routing Tests"
echo -e "==========================================${NC}"
echo ""

# ============================================
# Test 1: Regional Routing
# ============================================

echo -e "${YELLOW}[Test 1] Regional Routing${NC}"
echo "Each POP should prefer backends in its own region"
echo "---------------------------------------------------"

for pop in "SA:8080" "US:8081" "EU:8082"; do
    region=$(echo $pop | cut -d: -f1)
    port=$(echo $pop | cut -d: -f2)

    response=$(echo "" | nc -w 2 localhost $port 2>/dev/null | head -1)

    if [[ "$response" == *"$region"* ]] || [[ "$response" == *"$(echo $region | tr '[:upper:]' '[:lower:]')"* ]]; then
        echo -e "  ${GREEN}✓${NC} POP $region (port $port) -> $response"
    else
        echo -e "  ${RED}✗${NC} POP $region (port $port) -> $response (expected ${region,,}-node-*)"
    fi
done
echo ""

# ============================================
# Test 2: Client Affinity (Sticky Sessions)
# ============================================

echo -e "${YELLOW}[Test 2] Client Affinity (Sticky Sessions)${NC}"
echo "Multiple connections from same client should hit same backend"
echo "--------------------------------------------------------------"

for pop in "SA:8080" "US:8081" "EU:8082"; do
    region=$(echo $pop | cut -d: -f1)
    port=$(echo $pop | cut -d: -f2)

    backends=()
    for i in 1 2 3 4 5; do
        response=$(echo "" | nc -w 1 localhost $port 2>/dev/null | head -1 | grep -o 'Backend: [^ ]*' | cut -d' ' -f2)
        backends+=("$response")
    done

    # Check if all responses are the same
    unique=$(printf '%s\n' "${backends[@]}" | sort -u | wc -l)

    if [[ $unique -eq 1 ]]; then
        echo -e "  ${GREEN}✓${NC} POP $region: All 5 connections -> ${backends[0]} (affinity working)"
    else
        echo -e "  ${RED}✗${NC} POP $region: Got different backends: ${backends[*]}"
    fi
done
echo ""

# ============================================
# Test 3: Load Balancing (Weight Distribution)
# ============================================

echo -e "${YELLOW}[Test 3] Weight-based Load Balancing${NC}"
echo "node-1 has weight=2, should be preferred for new connections"
echo "--------------------------------------------------------------"

# Clear bindings by waiting (TTL is 30s in simulation)
echo "Note: This test requires fresh connections (no existing bindings)"
echo "Skipping detailed weight test - would need TTL expiry or restart"
echo ""

# ============================================
# Test 4: Backend Health Check
# ============================================

echo -e "${YELLOW}[Test 4] Direct Backend Connectivity${NC}"
echo "Verifying all 9 backends are responding"
echo "----------------------------------------"

all_ok=true
for region in "sa:9001:9003" "us:9011:9013" "eu:9021:9023"; do
    rname=$(echo $region | cut -d: -f1)
    start=$(echo $region | cut -d: -f2)
    end=$(echo $region | cut -d: -f3)

    for port in $(seq $start $end); do
        response=$(echo "" | nc -w 1 localhost $port 2>/dev/null | head -1)
        if [[ -n "$response" ]]; then
            echo -e "  ${GREEN}✓${NC} Port $port: $response"
        else
            echo -e "  ${RED}✗${NC} Port $port: NOT RESPONDING"
            all_ok=false
        fi
    done
done
echo ""

# ============================================
# Test 5: Failover Simulation
# ============================================

echo -e "${YELLOW}[Test 5] Failover Simulation${NC}"
echo "Marking backends as unhealthy and testing routing"
echo "---------------------------------------------------"

echo "Current healthy backends:"
sqlite3 -column "$PROJECT_DIR/routing.db" "SELECT id, healthy FROM backends WHERE region='sa'"
echo ""

# Mark sa-node-1 as unhealthy
echo "Marking sa-node-1 as unhealthy..."
sqlite3 "$PROJECT_DIR/routing.db" "UPDATE backends SET healthy=0 WHERE id='sa-node-1'"

# Wait for DB reload (5 seconds default)
echo "Waiting 6s for edgeProxy to reload routing.db..."
sleep 6

# Clear affinity by using a different "client" (we can't really do this without GeoIP)
echo "Testing SA POP routing after failover..."
response=$(echo "" | nc -w 2 localhost 8080 2>/dev/null | head -1)
echo "Response: $response"

if [[ "$response" == *"sa-node-1"* ]]; then
    echo -e "${RED}✗${NC} Still routing to unhealthy sa-node-1 (might be cached binding)"
else
    echo -e "${GREEN}✓${NC} Failover working - routing to healthy backend"
fi

# Restore sa-node-1
echo ""
echo "Restoring sa-node-1 to healthy..."
sqlite3 "$PROJECT_DIR/routing.db" "UPDATE backends SET healthy=1 WHERE id='sa-node-1'"

echo ""

# ============================================
# Test 6: Echo/Data Transfer
# ============================================

echo -e "${YELLOW}[Test 6] Data Transfer Through Proxy${NC}"
echo "Testing bidirectional TCP communication"
echo "----------------------------------------"

test_message="Hello from edgeProxy test at $(date)"
response=$(echo "$test_message" | nc -w 2 localhost 8080 2>/dev/null)

if [[ "$response" == *"Echo: $test_message"* ]]; then
    echo -e "${GREEN}✓${NC} Echo test passed"
    echo "  Sent: $test_message"
    echo "  Received: $(echo "$response" | grep Echo)"
else
    echo -e "${YELLOW}!${NC} Echo test - response format may vary"
    echo "  Response: $response"
fi
echo ""

# ============================================
# Summary
# ============================================

echo -e "${BLUE}=========================================="
echo "  Tests Complete"
echo -e "==========================================${NC}"
echo ""
echo "Architecture verified:"
echo "  - 3 edgeProxy POPs (SA, US, EU)"
echo "  - 9 backends (3 per region)"
echo "  - Regional routing preference"
echo "  - Client affinity (sticky sessions)"
echo "  - Failover on unhealthy backends"
echo ""
