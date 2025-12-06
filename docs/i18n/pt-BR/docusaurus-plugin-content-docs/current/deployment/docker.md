---
sidebar_position: 1
---

# Deploy com Docker

Este guia cobre o deployment do edgeProxy usando Docker e Docker Compose para ambientes de desenvolvimento e produção.

## Início Rápido

```bash
# Build e iniciar ambiente multi-região
task docker-build
task docker-up

# Executar testes
task docker-test

# Ver logs
task docker-logs

# Parar ambiente
task docker-down
```

## Dockerfile

O Dockerfile de produção usa multi-stage builds para tamanho mínimo de imagem:

```dockerfile
# Estágio de build
FROM rust:1.83-alpine AS builder
RUN apk add --no-cache musl-dev sqlite-dev
WORKDIR /app

# Cache de dependências
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Build da aplicação
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Estágio de runtime
FROM alpine:3.19
RUN apk add --no-cache sqlite sqlite-libs ca-certificates
WORKDIR /app

COPY --from=builder /app/target/release/edge-proxy /app/edge-proxy
COPY sql ./sql
RUN sqlite3 /app/routing.db < /app/sql/create_routing_db.sql

EXPOSE 8080
CMD ["/app/edge-proxy"]
```

### Tamanho da Imagem

| Estágio | Tamanho |
|---------|---------|
| Builder | ~2.5 GB |
| Runtime | ~25 MB |

## Docker Compose

### Ambiente de Desenvolvimento

Simulação multi-região completa com 3 POPs e 9 backends:

```yaml
version: '3.8'

services:
  # Backends América do Sul
  sa-node-1:
    build:
      context: .
      dockerfile: Dockerfile.backend
    environment:
      - BACKEND_PORT=8080
      - BACKEND_ID=sa-node-1
      - BACKEND_REGION=sa
    networks:
      edgenet:
        ipv4_address: 10.10.1.1

  sa-node-2:
    build:
      context: .
      dockerfile: Dockerfile.backend
    environment:
      - BACKEND_PORT=8080
      - BACKEND_ID=sa-node-2
      - BACKEND_REGION=sa
    networks:
      edgenet:
        ipv4_address: 10.10.1.2

  # POPs edgeProxy
  pop-sa:
    build:
      context: .
      dockerfile: Dockerfile
    environment:
      - EDGEPROXY_REGION=sa
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8080:8080"
    volumes:
      - ./docker/routing-docker.db:/app/routing.db:ro
    networks:
      edgenet:
        ipv4_address: 10.10.0.10

  pop-us:
    build:
      context: .
      dockerfile: Dockerfile
    environment:
      - EDGEPROXY_REGION=us
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8081:8080"
    volumes:
      - ./docker/routing-docker.db:/app/routing.db:ro
    networks:
      edgenet:
        ipv4_address: 10.10.0.11

  pop-eu:
    build:
      context: .
      dockerfile: Dockerfile
    environment:
      - EDGEPROXY_REGION=eu
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8082:8080"
    volumes:
      - ./docker/routing-docker.db:/app/routing.db:ro
    networks:
      edgenet:
        ipv4_address: 10.10.0.12

networks:
  edgenet:
    driver: bridge
    ipam:
      config:
        - subnet: 10.10.0.0/16
```

### Layout de Rede

```
10.10.0.0/16 - Edge Network
├── 10.10.0.10 - POP SA (localhost:8080)
├── 10.10.0.11 - POP US (localhost:8081)
├── 10.10.0.12 - POP EU (localhost:8082)
├── 10.10.1.0/24 - Backends SA
│   ├── 10.10.1.1 - sa-node-1
│   ├── 10.10.1.2 - sa-node-2
│   └── 10.10.1.3 - sa-node-3
├── 10.10.2.0/24 - Backends US
│   ├── 10.10.2.1 - us-node-1
│   └── ...
└── 10.10.3.0/24 - Backends EU
    ├── 10.10.3.1 - eu-node-1
    └── ...
```

## Mock Backend

Para testes, um backend Python mock ecoa dados com identificação:

```python
#!/usr/bin/env python3
import socket
import threading

def handle_client(conn, addr, backend_id, region):
    # Enviar identidade na conexão
    welcome = f"Backend: {backend_id} | Region: {region}\n"
    conn.send(welcome.encode())

    # Loop de echo
    while True:
        data = conn.recv(1024)
        if not data:
            break
        response = f"[{backend_id}] Echo: {data.decode().strip()}\n"
        conn.send(response.encode())

    conn.close()

def start_backend(host, port, backend_id, region):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind((host, port))
    server.listen(5)

    while True:
        conn, addr = server.accept()
        thread = threading.Thread(
            target=handle_client,
            args=(conn, addr, backend_id, region)
        )
        thread.daemon = True
        thread.start()
```

## Testes

### Suite de Testes Docker

```bash
#!/bin/bash
# tests/test_docker.sh

# Teste 1: Conectividade dos POPs
for pop in "pop-sa:10.10.0.10" "pop-us:10.10.0.11" "pop-eu:10.10.0.12"; do
    ip=$(echo $pop | cut -d: -f2)
    nc -z -w 2 $ip 8080 && echo "✓ $pop acessível"
done

# Teste 2: Roteamento Regional
for pop in "SA:10.10.0.10" "US:10.10.0.11" "EU:10.10.0.12"; do
    region=$(echo $pop | cut -d: -f1)
    ip=$(echo $pop | cut -d: -f2)
    response=$(printf '\n' | nc -w 2 $ip 8080 | head -1)
    echo "POP $region -> $response"
done

# Teste 3: Afinidade de Cliente
for i in 1 2 3 4 5; do
    response=$(printf '\n' | nc -w 1 10.10.0.10 8080 | head -1)
    echo "Conexão $i: $response"
done
```

### Executando Testes

```bash
# Suite completa de testes
task docker-test

# Teste manual
docker compose exec pop-sa /bin/sh
nc 10.10.1.1 8080  # Backend direto
nc 10.10.0.10 8080 # Através do proxy
```

## Deploy em Produção

### POP Único

```yaml
# docker-compose.prod.yml
version: '3.8'

services:
  edgeproxy:
    image: edgeproxy:latest
    restart: unless-stopped
    environment:
      - EDGEPROXY_REGION=sa
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/data/routing.db
      - EDGEPROXY_GEOIP_PATH=/data/GeoLite2-Country.mmdb
      - EDGEPROXY_BINDING_TTL_SECS=600
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data:ro
    healthcheck:
      test: ["CMD", "nc", "-z", "localhost", "8080"]
      interval: 30s
      timeout: 5s
      retries: 3
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 512M
```

## Monitoramento

### Health Check

```bash
# Check TCP simples
nc -z localhost 8080 && echo "saudável" || echo "não saudável"
```

### Logs

```bash
# Seguir todos os logs
docker compose logs -f

# Serviço específico
docker compose logs -f pop-sa

# Filtrar por nível
docker compose logs pop-sa 2>&1 | grep -E "(INFO|ERROR)"
```

## Troubleshooting

### Container Não Inicia

```bash
# Verificar logs
docker compose logs edgeproxy

# Verificar se routing.db existe
docker compose exec edgeproxy ls -la /app/routing.db

# Testar banco de dados
docker compose exec edgeproxy sqlite3 /app/routing.db "SELECT * FROM backends"
```

### Conexão Recusada

```bash
# Verificar mapeamento de porta
docker compose ps

# Verificar conectividade interna
docker compose exec edgeproxy nc -z localhost 8080

# Testar acessibilidade do backend
docker compose exec pop-sa nc -z 10.10.1.1 8080
```

### Sem Resposta do Proxy

```bash
# Habilitar logging debug
docker compose down
DEBUG=1 docker compose up

# Verificar saúde do backend
docker compose exec pop-sa sqlite3 /app/routing.db "SELECT id, healthy FROM backends"
```

## Próximos Passos

- [Deploy com Kubernetes](./kubernetes) - Manifests K8s e operators
- [Configuração](../configuration) - Variáveis de ambiente
- [Arquitetura](../architecture) - Design do sistema
