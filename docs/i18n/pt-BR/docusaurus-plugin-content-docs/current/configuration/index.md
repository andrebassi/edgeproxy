---
sidebar_position: 1
---

# Configuração

O edgeProxy é configurado inteiramente através de variáveis de ambiente. Esta seção cobre todas as opções disponíveis com exemplos.

## Seções da Documentação

| Seção | Descrição |
|-------|-----------|
| [Variáveis de Ambiente](./environment-variables) | Configurações core, TLS, DNS, API |
| [Schema do Banco de Dados](./database-schema) | Estrutura da tabela de roteamento |
| [DNS Interno](./dns-server) | Resolução geo-aware do domínio `.internal` |
| [API de Auto-Discovery](./auto-discovery-api) | Registro dinâmico de backends |
| [Control Plane Distribuído](./corrosion) | SQLite distribuído com Corrosion |
| [Componentes de Infraestrutura](./infrastructure) | Rate limiting, circuit breaker, métricas |

## Quick Start

### Desenvolvimento

```bash
export EDGEPROXY_LISTEN_ADDR="127.0.0.1:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="./routing.db"
export DEBUG="1"

./target/release/edge-proxy
```

### Produção

```bash
export EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="/data/routing.db"
export EDGEPROXY_BINDING_TTL_SECS="600"

./edge-proxy
```

### Docker Compose

```yaml
services:
  pop-sa:
    image: edgeproxy:latest
    environment:
      - EDGEPROXY_REGION=sa
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
    ports:
      - "8080:8080"
    volumes:
      - ./routing.db:/app/routing.db:ro
```
