#!/bin/bash
#
# Simulate Multi-Region edgeProxy Environment
#
# This script creates a local simulation of:
# - 3 edgeProxy POPs (SA, US, EU) on different ports
# - 3 Backend servers per region (9 total)
# - Tests routing, affinity, and failover
#

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PIDS=()

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

cleanup() {
    echo ""
    echo -e "${YELLOW}Shutting down all services...${NC}"
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    # Kill any remaining python mock backends
    pkill -f "mock_backend.py" 2>/dev/null || true
    echo -e "${GREEN}Done.${NC}"
    exit 0
}

trap cleanup SIGINT SIGTERM

echo -e "${BLUE}=========================================="
echo "  edgeProxy Multi-Region Simulation"
echo -e "==========================================${NC}"
echo ""

cd "$PROJECT_DIR"

# ============================================
# Setup Database with Multiple Backends
# ============================================

echo -e "${YELLOW}Setting up routing.db with 9 backends (3 per region)...${NC}"

sqlite3 routing.db << 'EOF'
DELETE FROM backends;
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  -- South America backends
  ('sa-node-1', 'myapp', 'sa', '127.0.0.1', 9001, 1, 2, 50, 100),
  ('sa-node-2', 'myapp', 'sa', '127.0.0.1', 9002, 1, 1, 50, 100),
  ('sa-node-3', 'myapp', 'sa', '127.0.0.1', 9003, 1, 1, 50, 100),
  -- US backends
  ('us-node-1', 'myapp', 'us', '127.0.0.1', 9011, 1, 2, 50, 100),
  ('us-node-2', 'myapp', 'us', '127.0.0.1', 9012, 1, 1, 50, 100),
  ('us-node-3', 'myapp', 'us', '127.0.0.1', 9013, 1, 1, 50, 100),
  -- EU backends
  ('eu-node-1', 'myapp', 'eu', '127.0.0.1', 9021, 1, 2, 50, 100),
  ('eu-node-2', 'myapp', 'eu', '127.0.0.1', 9022, 1, 1, 50, 100),
  ('eu-node-3', 'myapp', 'eu', '127.0.0.1', 9023, 1, 1, 50, 100);
EOF

echo -e "${GREEN}Backends configured:${NC}"
sqlite3 -header -column routing.db "SELECT id, region, port, weight, healthy FROM backends ORDER BY region, id"
echo ""

# ============================================
# Start Mock Backends (9 total)
# ============================================

echo -e "${YELLOW}Starting 9 mock backends...${NC}"

# SA backends
for i in 1 2 3; do
    port=$((9000 + i))
    python3 "$SCRIPT_DIR/mock_backend.py" $port "sa-node-$i" "sa" &
    PIDS+=($!)
done

# US backends
for i in 1 2 3; do
    port=$((9010 + i))
    python3 "$SCRIPT_DIR/mock_backend.py" $port "us-node-$i" "us" &
    PIDS+=($!)
done

# EU backends
for i in 1 2 3; do
    port=$((9020 + i))
    python3 "$SCRIPT_DIR/mock_backend.py" $port "eu-node-$i" "eu" &
    PIDS+=($!)
done

sleep 1
echo -e "${GREEN}Mock backends started${NC}"
echo ""

# ============================================
# Start 3 edgeProxy POPs
# ============================================

echo -e "${YELLOW}Starting 3 edgeProxy POPs...${NC}"

# SA POP (port 8080)
EDGEPROXY_REGION=sa \
EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080" \
EDGEPROXY_DB_PATH="$PROJECT_DIR/routing.db" \
EDGEPROXY_BINDING_TTL_SECS=30 \
"$PROJECT_DIR/target/release/edge-proxy" 2>&1 | sed 's/^/[POP-SA] /' &
PIDS+=($!)

sleep 0.5

# US POP (port 8081)
EDGEPROXY_REGION=us \
EDGEPROXY_LISTEN_ADDR="0.0.0.0:8081" \
EDGEPROXY_DB_PATH="$PROJECT_DIR/routing.db" \
EDGEPROXY_BINDING_TTL_SECS=30 \
"$PROJECT_DIR/target/release/edge-proxy" 2>&1 | sed 's/^/[POP-US] /' &
PIDS+=($!)

sleep 0.5

# EU POP (port 8082)
EDGEPROXY_REGION=eu \
EDGEPROXY_LISTEN_ADDR="0.0.0.0:8082" \
EDGEPROXY_DB_PATH="$PROJECT_DIR/routing.db" \
EDGEPROXY_BINDING_TTL_SECS=30 \
"$PROJECT_DIR/target/release/edge-proxy" 2>&1 | sed 's/^/[POP-EU] /' &
PIDS+=($!)

sleep 1

echo ""
echo -e "${GREEN}=========================================="
echo "  Environment Ready!"
echo -e "==========================================${NC}"
echo ""
echo -e "${BLUE}edgeProxy POPs:${NC}"
echo "  SA POP: localhost:8080 (prefers sa-node-* backends)"
echo "  US POP: localhost:8081 (prefers us-node-* backends)"
echo "  EU POP: localhost:8082 (prefers eu-node-* backends)"
echo ""
echo -e "${BLUE}Backends (9 total):${NC}"
echo "  SA: ports 9001-9003 (sa-node-1 has weight=2)"
echo "  US: ports 9011-9013 (us-node-1 has weight=2)"
echo "  EU: ports 9021-9023 (eu-node-1 has weight=2)"
echo ""
echo -e "${YELLOW}Test Commands:${NC}"
echo ""
echo "  # Connect via SA POP (should route to sa-node-*)"
echo "  echo 'hello' | nc localhost 8080"
echo ""
echo "  # Connect via US POP (should route to us-node-*)"
echo "  echo 'hello' | nc localhost 8081"
echo ""
echo "  # Connect via EU POP (should route to eu-node-*)"
echo "  echo 'hello' | nc localhost 8082"
echo ""
echo "  # Run automated tests"
echo "  ./tests/test_multi_region.sh"
echo ""
echo -e "${RED}Press Ctrl+C to stop all services${NC}"
echo ""

wait
