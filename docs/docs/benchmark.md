---
sidebar_position: 6
---

# Benchmark Results

This document presents the benchmark results for edgeProxy with WireGuard overlay network, tested across 9 global VPN locations routing to 10 Fly.io backend regions.

## Test Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    edgeProxy + WireGuard - Production Test                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Client (VPN) ──► EC2 Ireland (edgeProxy) ──► WireGuard ──► Fly.io        │
│                    54.171.48.207:8080          10.50.x.x    10 regions     │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│   Geo-Routing: 9/9 ✅                                                       │
│   WireGuard Tunnel: 10/10 peers connected ✅                                │
│   Benchmark v2: Latency, Download, Upload, Stress ✅                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Infrastructure

### edgeProxy Server (AWS EC2)
- **Region**: eu-west-1 (Ireland)
- **Instance**: t3.micro
- **IP**: 54.171.48.207
- **WireGuard IP**: 10.50.0.1/24

### Backend Servers (Fly.io)

| Region | Location | WireGuard IP |
|--------|----------|--------------|
| GRU | Sao Paulo, Brazil | 10.50.1.1 |
| IAD | Virginia, USA | 10.50.2.1 |
| ORD | Chicago, USA | 10.50.2.2 |
| LAX | Los Angeles, USA | 10.50.2.3 |
| LHR | London, UK | 10.50.3.1 |
| FRA | Frankfurt, Germany | 10.50.3.2 |
| CDG | Paris, France | 10.50.3.3 |
| NRT | Tokyo, Japan | 10.50.4.1 |
| SIN | Singapore | 10.50.4.2 |
| SYD | Sydney, Australia | 10.50.4.3 |

## Benchmark Results

### Complete Test Table

| # | VPN Location | Country | Backend | Latency | Download 1MB | Download 5MB | RPS (20) | Status |
|---|--------------|---------|---------|---------|--------------|--------------|----------|--------|
| 1 | Paris | FR | **CDG** | 530ms | 0.5 MB/s | 2.1 MB/s | 35.7 | ✅ |
| 2 | Frankfurt | DE | **FRA** | 528ms | 0.6 MB/s | 2.3 MB/s | 34.0 | ✅ |
| 3 | London | GB | **LHR** | 490ms | 0.6 MB/s | 2.3 MB/s | 36.6 | ✅ |
| 4 | Detroit | US | **IAD** | 708ms | 0.6 MB/s | 2.5 MB/s | 27.4 | ✅ |
| 5 | Las Vegas | US | **IAD** | 857ms | 0.5 MB/s | 2.2 MB/s | 22.5 | ✅ |
| 6 | Tokyo | JP | **NRT** | 1546ms | 0.3 MB/s | 1.1 MB/s | 12.6 | ✅ |
| 7 | Singapore | SG | **SIN** | 1414ms | 0.3 MB/s | 1.2 MB/s | 13.8 | ✅ |
| 8 | Sydney | AU | **SYD** | 1847ms | 0.2 MB/s | 0.9 MB/s | 10.7 | ✅ |
| 9 | Sao Paulo | BR | **GRU** | 822ms | 0.4 MB/s | 1.6 MB/s | 23.3 | ✅ |

### Performance Analysis by Region

| Region | Latency Range | Observation |
|--------|---------------|-------------|
| Europe (CDG/FRA/LHR) | 490-530ms | Best - closest to EC2 Ireland |
| USA (IAD) | 708-857ms | Medium - crosses Atlantic |
| Brazil (GRU) | 822ms | Good - direct route |
| Asia (NRT/SIN) | 1414-1546ms | High - geographic distance |
| Oceania (SYD) | 1847ms | Highest - half way around the world |

## Geo-Routing Validation

All 9 VPN tests correctly routed to the expected backend based on client geographic location:

| Client Location | Expected Backend | Actual Backend | Result |
|-----------------|------------------|----------------|--------|
| France (FR) | CDG | CDG | ✅ |
| Germany (DE) | FRA | FRA | ✅ |
| United Kingdom (GB) | LHR | LHR | ✅ |
| United States (US) | IAD | IAD | ✅ |
| Japan (JP) | NRT | NRT | ✅ |
| Singapore (SG) | SIN | SIN | ✅ |
| Australia (AU) | SYD | SYD | ✅ |
| Brazil (BR) | GRU | GRU | ✅ |

## WireGuard Tunnel Status

All 10 Fly.io backends successfully established WireGuard tunnels to the EC2 server:

```
interface: wg0
  public key: bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY=
  listening port: 51820

peer: He2jX3+iEl7hUaaJG/i3YcSnStEFAcW/rs/lP0Pw+nc= (GRU)
  allowed ips: 10.50.1.1/32
  latest handshake: 18 seconds ago ✅

peer: rImgzxPu9MuhqLpcvXQ9xckSSA+AGbDOpBGvTUOwaHQ= (IAD)
  allowed ips: 10.50.2.1/32
  latest handshake: 15 seconds ago ✅

... (all 10 peers connected)
```

## Test Methodology

### Latency Test
- 20 sequential HTTP requests to `/api/latency` endpoint
- Measures round-trip time from client to backend via proxy
- Reports: Average, Minimum, Maximum latency

### Download Test
- HTTP GET requests to `/api/download?size=N` endpoint
- Tests with 1MB and 5MB file sizes
- Measures: Download speed in MB/s

### Concurrent Requests Test
- 20 parallel HTTP requests
- Measures: Total time and Requests Per Second (RPS)

## Benchmark Endpoints

The backend v2 provides the following test endpoints:

| Endpoint | Description |
|----------|-------------|
| `/api/info` | Server info (region, uptime, requests) |
| `/api/latency` | Minimal response for latency testing |
| `/api/download?size=N` | Download test (N bytes, max 100MB) |
| `/api/upload` | Upload test (POST body) |
| `/api/stats` | Server statistics |
| `/benchmark` | Interactive HTML benchmark page |

## Running Your Own Benchmark

### Quick Tests

```bash
# Quick latency test
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://54.171.48.207:8080/api/latency
done

# Download test (1MB)
curl -w "Speed: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=1048576"

# Check geo-routing
curl -s http://54.171.48.207:8080/api/info | jq .
```

### Complete Benchmark Script

This is the script used to generate the benchmark results table:

```bash
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
```

## Conclusions

1. **Geo-Routing**: 100% accuracy in routing clients to the correct regional backend
2. **WireGuard**: Stable tunnels with all 10 global backends
3. **Performance**: Latency scales predictably with geographic distance
4. **Reliability**: All tests passed with consistent results

### Expected Production Performance

In production with multiple edgeProxy POPs deployed globally (not just Ireland):

| Scenario | Expected Latency |
|----------|------------------|
| Client → Local POP → Local Backend | 5-20ms |
| Client → Local POP → Regional Backend | 20-50ms |
| Client → Local POP → Remote Backend | 50-150ms |

The current test setup routes all traffic through Ireland, which adds latency for distant clients. A full mesh deployment would significantly improve performance for all regions.
