#!/bin/bash
# benchmark.sh - Complete edgeProxy benchmark suite
# Usage: ./benchmark.sh <proxy-url>

PROXY_URL="${1:-http://54.171.48.207:8080}"

echo "=== edgeProxy Benchmark V2 ==="
echo "Target: $PROXY_URL"
echo ""

# 1. Region Check
echo "1. Region Check:"
curl -s "$PROXY_URL/api/info" | python3 -m json.tool
echo ""

# 2. Latency Test
echo "2. Latency Test (20 pings):"
latencies=()
for i in {1..20}; do
  start=$(python3 -c "import time; print(int(time.time()*1000))")
  curl -s "$PROXY_URL/api/latency" > /dev/null
  end=$(python3 -c "import time; print(int(time.time()*1000))")
  latency=$((end - start))
  latencies+=($latency)
  printf "  Ping %2d: %dms\n" $i $latency
done
total=0; for l in "${latencies[@]}"; do total=$((total + l)); done
avg=$((total / 20))
min=$(printf '%s\n' "${latencies[@]}" | sort -n | head -1)
max=$(printf '%s\n' "${latencies[@]}" | sort -n | tail -1)
echo "  ────────────────"
echo "  Avg: ${avg}ms | Min: ${min}ms | Max: ${max}ms"
echo ""

# 3. Download Test (1MB)
echo "3. Download Test (1MB):"
curl -w "  Downloaded: %{size_download} bytes | Time: %{time_total}s | Speed: %{speed_download} B/s\n" \
  -o /dev/null -s "$PROXY_URL/api/download?size=1048576"

# 4. Download Test (5MB)
echo "4. Download Test (5MB):"
curl -w "  Downloaded: %{size_download} bytes | Time: %{time_total}s | Speed: %{speed_download} B/s\n" \
  -o /dev/null -s "$PROXY_URL/api/download?size=5242880"

# 5. Concurrent Requests
echo "5. Concurrent Requests (20 parallel):"
start=$(python3 -c "import time; print(int(time.time()*1000))")
for i in {1..20}; do
  curl -s "$PROXY_URL/api/latency" > /dev/null &
done
wait
end=$(python3 -c "import time; print(int(time.time()*1000))")
echo "  20 requests in $((end - start))ms | RPS: $(python3 -c "print(f'{20000/$((end - start)):.1f}')")"

echo ""
echo "=== Benchmark Complete ==="
