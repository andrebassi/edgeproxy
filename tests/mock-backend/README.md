# Mock Backend Server

A lightweight Go HTTP server that simulates real backend services for testing edgeProxy.

## Quick Start

```bash
# Build
go build -o mock-backend main.go

# Run
./mock-backend -port 9001 -region eu -id mock-eu-1
```

## CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `-port` | `9001` | TCP port |
| `-region` | `eu` | Region (eu, us, sa, ap) |
| `-id` | `mock-{region}-{port}` | Backend ID |

## Endpoints

- `/` - Text response with backend info
- `/health` - Health check
- `/api/info` - JSON with full details
- `/api/latency` - Minimal JSON for latency testing

## Cross-Compile for Linux

```bash
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go
```

## Example Multi-Backend Setup

```bash
./mock-backend -port 9001 -region eu -id mock-eu-1 &
./mock-backend -port 9002 -region eu -id mock-eu-2 &
./mock-backend -port 9003 -region us -id mock-us-1 &
```

See [Testing Documentation](../../docs/markdown/testing.md) for full usage guide.
