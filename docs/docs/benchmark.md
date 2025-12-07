---
sidebar_position: 2
---

# Benchmarks

This document presents the benchmark results for edgeProxy with WireGuard overlay network across global locations.

:::info Infrastructure Setup
For details on how to set up the AWS EC2 and WireGuard infrastructure used in these tests, see [AWS EC2 Deployment](./deployment/aws).
:::

---

## Benchmark v3 (Current)

:::info New POP: Hong Kong
Testing the new GCP Hong Kong POP (`35.241.112.61`) for APAC region coverage.
:::

### Test Infrastructure

| Component | Details |
|-----------|---------|
| **New POP** | GCP Hong Kong (asia-east2) |
| **IP** | 35.241.112.61:8080 |
| **Region** | `ap` (Asia Pacific) |
| **Backends** | 10 (via WireGuard mesh) |

### Test Results

| # | VPN Location | Country | Backend | Latency | Status |
|---|--------------|---------|---------|---------|--------|
| 1-3 | 🇨🇳🇭🇰 China/HK | CN/HK | **HKG** | - | ⏭️ (local POP) |
| 4 | 🇯🇵 Tokyo | JP | **NRT** | ~1.79s | ✅ |
| 5 | 🇸🇬 Singapore | SG | **SIN** | ~1.63s | ✅ |
| 6 | 🇹🇼 Taiwan | TW | **NRT** | ~1.64s | ✅ |
| 7 | 🇰🇷 Seoul | KR | **NRT** | ~1.71s | ✅ |
| 8 | 🇮🇳 India | IN | **IAD** | ~1.58s | ✅ |
| 9 | 🇦🇺 Sydney | AU | **SYD** | ~1.94s | ✅ |

**Geo-routing accuracy: 6/6 (100%)**

### WireGuard Mesh Latency (from HKG)

| Backend | WireGuard IP | Ping Latency |
|---------|--------------|--------------|
| EC2 Ireland (Hub) | 10.50.0.1 | 242ms |
| GRU (São Paulo) | 10.50.1.1 | 445ms |
| IAD (Virginia) | 10.50.2.1 | 327ms |
| LHR (London) | 10.50.3.1 | 252ms |
| NRT (Tokyo) | 10.50.4.1 | 492ms |
| SIN (Singapore) | 10.50.4.2 | 408ms |

### Running v3 Tests

```bash
# Test connectivity to HKG POP
nc -zv 35.241.112.61 8080

# Quick latency test
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://35.241.112.61:8080/api/latency
done

# Check geo-routing
curl -s http://35.241.112.61:8080/api/info | jq .
```

### v3 Observations

- All APAC traffic correctly routed to nearest regional backend
- Taiwan and Korea route to NRT (Tokyo) as expected
- India routes to IAD (Virginia) - no local backend available
- High latency due to hub-and-spoke through EC2 Ireland

---

## Benchmark v2

### Test Results

| # | VPN Location | Country | Backend | Latency | Download 1MB | Download 5MB | RPS (20) | Status |
|---|--------------|---------|---------|---------|--------------|--------------|----------|--------|
| 1 | 🇫🇷 Paris | FR | **CDG** | 530ms | 0.5 MB/s | 2.1 MB/s | 35.7 | ✅ |
| 2 | 🇩🇪 Frankfurt | DE | **FRA** | 528ms | 0.6 MB/s | 2.3 MB/s | 34.0 | ✅ |
| 3 | 🇬🇧 London | GB | **LHR** | 490ms | 0.6 MB/s | 2.3 MB/s | 36.6 | ✅ |
| 4 | 🇺🇸 Detroit | US | **IAD** | 708ms | 0.6 MB/s | 2.5 MB/s | 27.4 | ✅ |
| 5 | 🇺🇸 Las Vegas | US | **IAD** | 857ms | 0.5 MB/s | 2.2 MB/s | 22.5 | ✅ |
| 6 | 🇯🇵 Tokyo | JP | **NRT** | 1546ms | 0.3 MB/s | 1.1 MB/s | 12.6 | ✅ |
| 7 | 🇸🇬 Singapore | SG | **SIN** | 1414ms | 0.3 MB/s | 1.2 MB/s | 13.8 | ✅ |
| 8 | 🇦🇺 Sydney | AU | **SYD** | 1847ms | 0.2 MB/s | 0.9 MB/s | 10.7 | ✅ |
| 9 | 🇧🇷 Sao Paulo | BR | **GRU** | 822ms | 0.4 MB/s | 1.6 MB/s | 23.3 | ✅ |

### Performance by Region

| Region | Latency | Observation |
|--------|---------|-------------|
| 🇪🇺 Europe (CDG/FRA/LHR) | 490-530ms | Best - closest to EC2 Ireland |
| 🇺🇸 USA (IAD) | 708-857ms | Medium - crosses Atlantic |
| 🇧🇷 Brazil (GRU) | 822ms | Good - direct route |
| 🇯🇵🇸🇬 Asia (NRT/SIN) | 1414-1546ms | High - geographic distance |
| 🇦🇺 Oceania (SYD) | 1847ms | Highest - half way around the world |

---

## Test Architecture

![Benchmark Architecture](/img/benchmark-architecture.svg)

---

## Geo-Routing Validation

All 9 VPN tests correctly routed to the expected backend:

| Client Location | Expected | Actual | Result |
|-----------------|----------|--------|--------|
| 🇫🇷 France | CDG | CDG | ✅ |
| 🇩🇪 Germany | FRA | FRA | ✅ |
| 🇬🇧 United Kingdom | LHR | LHR | ✅ |
| 🇺🇸 United States | IAD | IAD | ✅ |
| 🇯🇵 Japan | NRT | NRT | ✅ |
| 🇸🇬 Singapore | SIN | SIN | ✅ |
| 🇦🇺 Australia | SYD | SYD | ✅ |
| 🇧🇷 Brazil | GRU | GRU | ✅ |

---

## Running Your Own Tests

### Quick Latency Test

```bash
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://54.171.48.207:8080/api/latency
done
```

### Check Geo-Routing

```bash
curl -s http://54.171.48.207:8080/api/info | jq .
# Returns: {"region":"cdg","region_name":"Paris, France",...}
```

### Download Speed Test

```bash
# 1MB download
curl -w "Speed: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=1048576"

# 5MB download
curl -w "Speed: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=5242880"
```

### Complete Benchmark Script

Use the provided script in `scripts/benchmark.sh`:

```bash
./scripts/benchmark.sh http://54.171.48.207:8080
```

---

## Benchmark Endpoints

| Endpoint | Description |
|----------|-------------|
| `/` | ASCII art banner with region info |
| `/api/info` | JSON server info (region, uptime, requests) |
| `/api/latency` | Minimal response for latency testing |
| `/api/download?size=N` | Download test (N bytes, max 100MB) |
| `/api/upload` | Upload test (POST body) |
| `/api/stats` | Server statistics |
| `/benchmark` | Interactive HTML benchmark page |

---

## Conclusions

1. **Geo-Routing**: 100% accuracy routing clients to correct regional backend
2. **WireGuard**: Stable tunnels with all 10 global backends
3. **Performance**: Latency scales predictably with geographic distance
4. **Reliability**: All tests passed with consistent results

### Production Deployment

For production, deploy edgeProxy POPs in multiple regions:

| Scenario | Expected Latency |
|----------|------------------|
| Client → Local POP → Local Backend | 5-20ms |
| Client → Local POP → Regional Backend | 20-50ms |
| Client → Local POP → Remote Backend | 50-150ms |

The test setup routes all traffic through Ireland. A full mesh deployment would significantly improve global performance.

---

## Benchmark v1 (Initial)

Initial validation test with limited regions to verify geo-routing functionality.

:::info Test Scope
- **Regions tested:** 3 (Europe focus)
- **Purpose:** Validate basic geo-routing and WireGuard connectivity
:::

### Test Results

| # | VPN Location | Country | Backend | Latency | Status |
|---|--------------|---------|---------|---------|--------|
| 1 | 🇫🇷 Paris | FR | **CDG** | ~500ms | ✅ |
| 2 | 🇩🇪 Frankfurt | DE | **FRA** | ~520ms | ✅ |
| 3 | 🇬🇧 London | GB | **LHR** | ~480ms | ✅ |

### v1 → v2 Improvements

| Aspect | v1 | v2 |
|--------|----|----|
| Regions tested | 3 | 9 |
| Metrics | Latency only | Latency, Download, RPS |
| Global coverage | Europe only | 5 continents |
| WireGuard peers | 3 | 10 |

---

## Related Documentation

- [AWS EC2 Deployment](./deployment/aws) - Infrastructure setup guide
- [Fly.io Deployment](./deployment/flyio) - Global edge deployment
- [Docker Deployment](./deployment/docker) - Local development
