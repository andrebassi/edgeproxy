#!/bin/bash
#
# Test edgeProxy Multi-Region in Docker
# Runs inside the test-runner container
#

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=========================================="
echo "  edgeProxy Docker Multi-Region Tests"
echo -e "==========================================${NC}"
echo ""

PASSED=0
FAILED=0

test_result() {
    if [ "$1" -eq 0 ]; then
        echo -e "  ${GREEN}✓${NC} $2"
        ((PASSED++))
    else
        echo -e "  ${RED}✗${NC} $2"
        ((FAILED++))
    fi
}

# ============================================
# Test 1: POP Connectivity
# ============================================

echo -e "${YELLOW}[Test 1] POP Connectivity${NC}"
echo "Testing if all POPs are reachable"
echo "-----------------------------------"

for pop in "pop-sa:10.10.0.10" "pop-us:10.10.0.11" "pop-eu:10.10.0.12"; do
    name=$(echo $pop | cut -d: -f1)
    ip=$(echo $pop | cut -d: -f2)

    if nc -z -w 2 $ip 8080 2>&1; then
        test_result 0 "$name ($ip:8080) is reachable"
    else
        test_result 1 "$name ($ip:8080) is NOT reachable"
    fi
done
echo ""

# ============================================
# Test 2: Regional Routing
# ============================================

echo -e "${YELLOW}[Test 2] Regional Routing${NC}"
echo "Each POP should route to its own region's backends"
echo "---------------------------------------------------"

for pop in "SA:10.10.0.10" "US:10.10.0.11" "EU:10.10.0.12"; do
    region=$(echo $pop | cut -d: -f1)
    ip=$(echo $pop | cut -d: -f2)
    region_lower=$(echo $region | tr '[:upper:]' '[:lower:]')

    response=$(printf '\n' | nc -w 2 $ip 8080 2>/dev/null | head -1)

    if [[ "$response" == *"$region_lower-node"* ]]; then
        test_result 0 "POP $region -> $response"
    else
        test_result 1 "POP $region -> $response (expected ${region_lower}-node-*)"
    fi
done
echo ""

# ============================================
# Test 3: Backend Connectivity
# ============================================

echo -e "${YELLOW}[Test 3] Backend Connectivity${NC}"
echo "Testing direct connectivity to all 9 backends"
echo "----------------------------------------------"

for backend in "sa-node-1:10.10.1.1" "sa-node-2:10.10.1.2" "sa-node-3:10.10.1.3" \
               "us-node-1:10.10.2.1" "us-node-2:10.10.2.2" "us-node-3:10.10.2.3" \
               "eu-node-1:10.10.3.1" "eu-node-2:10.10.3.2" "eu-node-3:10.10.3.3"; do
    name=$(echo $backend | cut -d: -f1)
    ip=$(echo $backend | cut -d: -f2)

    response=$(printf '\n' | nc -w 2 $ip 8080 2>/dev/null | head -1)

    if [[ "$response" == *"$name"* ]]; then
        test_result 0 "$name ($ip)"
    else
        test_result 1 "$name ($ip) - response: $response"
    fi
done
echo ""

# ============================================
# Test 4: Client Affinity
# ============================================

echo -e "${YELLOW}[Test 4] Client Affinity${NC}"
echo "5 connections from same client should hit same backend"
echo "-------------------------------------------------------"

for pop in "SA:10.10.0.10" "US:10.10.0.11" "EU:10.10.0.12"; do
    region=$(echo $pop | cut -d: -f1)
    ip=$(echo $pop | cut -d: -f2)

    backends=()
    for i in 1 2 3 4 5; do
        response=$(printf '\n' | nc -w 1 $ip 8080 2>/dev/null | head -1 | grep -o 'Backend: [^ ]*' | cut -d' ' -f2)
        backends+=("$response")
    done

    unique=$(printf '%s\n' "${backends[@]}" | sort -u | wc -l | tr -d ' ')

    if [[ "$unique" == "1" ]]; then
        test_result 0 "POP $region: All 5 -> ${backends[0]}"
    else
        test_result 1 "POP $region: Got $unique different backends"
    fi
done
echo ""

# ============================================
# Test 5: Data Transfer
# ============================================

echo -e "${YELLOW}[Test 5] Data Transfer${NC}"
echo "Testing echo through proxy"
echo "--------------------------"

test_msg="Hello from Docker test"
response=$(printf '%s\n' "$test_msg" | nc -w 2 10.10.0.10 8080 2>/dev/null)

if [[ "$response" == *"Echo"* ]] && [[ "$response" == *"$test_msg"* ]]; then
    test_result 0 "Echo working through proxy"
else
    test_result 1 "Echo test failed"
fi
echo ""

# ============================================
# Summary
# ============================================

echo -e "${BLUE}=========================================="
echo "  Test Summary"
echo -e "==========================================${NC}"
echo ""
echo -e "  ${GREEN}Passed:${NC} $PASSED"
echo -e "  ${RED}Failed:${NC} $FAILED"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed${NC}"
    exit 1
fi
