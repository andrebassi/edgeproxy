#!/bin/bash
#
# Start Local Test Environment for edgeProxy
# Simulates 3 backends (SA, US, EU) and 1 edgeProxy POP
#

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PIDS=()

cleanup() {
    echo ""
    echo "Shutting down..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    echo "Done."
    exit 0
}

trap cleanup SIGINT SIGTERM

echo "=========================================="
echo "  edgeProxy Local Test Environment"
echo "=========================================="
echo ""

# Update routing.db to use localhost
echo "Updating routing.db for local testing..."
cd "$PROJECT_DIR"

sqlite3 routing.db << 'EOF'
DELETE FROM backends;
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('sa-node-1', 'myapp', 'sa', '127.0.0.1', 9001, 1, 1, 100, 200),
  ('us-node-1', 'myapp', 'us', '127.0.0.1', 9002, 1, 1, 100, 200),
  ('eu-node-1', 'myapp', 'eu', '127.0.0.1', 9003, 1, 1, 100, 200);
EOF

echo "Backends configured:"
sqlite3 -header -column routing.db "SELECT id, region, wg_ip, port, healthy FROM backends"
echo ""

# Start mock backends
echo "Starting mock backends..."

python3 "$SCRIPT_DIR/mock_backend.py" 9001 sa-node-1 sa &
PIDS+=($!)
sleep 0.2

python3 "$SCRIPT_DIR/mock_backend.py" 9002 us-node-1 us &
PIDS+=($!)
sleep 0.2

python3 "$SCRIPT_DIR/mock_backend.py" 9003 eu-node-1 eu &
PIDS+=($!)
sleep 0.5

echo ""
echo "Mock backends started:"
echo "  - SA Backend: localhost:9001"
echo "  - US Backend: localhost:9002"
echo "  - EU Backend: localhost:9003"
echo ""

# Start edgeProxy
echo "Starting edgeProxy (region=sa, port=8080)..."
EDGEPROXY_REGION=sa \
EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080" \
EDGEPROXY_DB_PATH="$PROJECT_DIR/routing.db" \
"$PROJECT_DIR/target/release/edge-proxy" &
PIDS+=($!)

sleep 1

echo ""
echo "=========================================="
echo "  Environment Ready!"
echo "=========================================="
echo ""
echo "edgeProxy POP (SA) listening on: localhost:8080"
echo ""
echo "Test commands:"
echo "  nc localhost 8080          # Connect via proxy"
echo "  echo 'hello' | nc localhost 8080"
echo ""
echo "Direct backend test:"
echo "  nc localhost 9001          # SA backend direct"
echo "  nc localhost 9002          # US backend direct"
echo "  nc localhost 9003          # EU backend direct"
echo ""
echo "Press Ctrl+C to stop all services"
echo ""

# Wait for all processes
wait
