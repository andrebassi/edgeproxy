---
sidebar_position: 5
---

# API de Auto-Discovery

A API permite que backends se registrem e desregistrem automaticamente.

## Endpoints

| Método | Endpoint | Descrição |
|--------|----------|-----------|
| GET | `/health` | Health check + versão + contagem de backends |
| POST | `/api/v1/register` | Registrar um novo backend |
| POST | `/api/v1/heartbeat/:id` | Atualizar heartbeat do backend |
| GET | `/api/v1/backends` | Listar todos os backends registrados |
| GET | `/api/v1/backends/:id` | Obter detalhes de um backend específico |
| DELETE | `/api/v1/backends/:id` | Desregistrar um backend |

## Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_API_ENABLED` | `false` | Habilitar API Auto-Discovery |
| `EDGEPROXY_API_LISTEN_ADDR` | `0.0.0.0:8081` | Endereço da API |
| `EDGEPROXY_HEARTBEAT_TTL_SECS` | `60` | TTL do heartbeat do backend |

## Exemplo de Registro

```bash
# Habilitar API
export EDGEPROXY_API_ENABLED=true
export EDGEPROXY_API_LISTEN_ADDR=0.0.0.0:8081
export EDGEPROXY_HEARTBEAT_TTL_SECS=60

# Registrar um backend
curl -X POST http://localhost:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{
    "id": "backend-eu-1",
    "app": "myapp",
    "region": "eu",
    "ip": "10.50.1.1",
    "port": 8080,
    "weight": 2,
    "soft_limit": 100,
    "hard_limit": 150
  }'

# Enviar heartbeat (keep alive)
curl -X POST http://localhost:8081/api/v1/heartbeat/backend-eu-1

# Listar todos os backends
curl http://localhost:8081/api/v1/backends
```

## Payload de Registro

```json
{
  "id": "backend-eu-1",
  "app": "myapp",
  "region": "eu",
  "country": "DE",
  "ip": "10.50.1.1",
  "port": 8080,
  "weight": 2,
  "soft_limit": 100,
  "hard_limit": 150
}
```

| Campo | Obrigatório | Padrão | Descrição |
|-------|-------------|--------|-----------|
| `id` | Sim | - | Identificador único do backend |
| `app` | Sim | - | Nome da aplicação |
| `region` | Sim | - | Código da região (sa, us, eu, ap) |
| `country` | Não | derivado | Código do país (ISO 3166-1) |
| `ip` | Sim | - | Endereço IP do backend |
| `port` | Sim | - | Porta do backend |
| `weight` | Não | 2 | Peso no load balancing |
| `soft_limit` | Não | 100 | Limite soft de conexões |
| `hard_limit` | Não | 150 | Limite hard de conexões |

## Resposta do Health Check

```bash
curl http://localhost:8081/health
```

```json
{
  "status": "ok",
  "version": "0.2.0",
  "backends": 5,
  "uptime_secs": 3600
}
```

## Benefícios

- **Zero configuração**: Backends apenas iniciam e se registram
- **Escala automática**: Novas instâncias aparecem automaticamente
- **Graceful shutdown**: Desregistro limpo
- **Health baseado em TTL**: Não saudável = expirado = desregistrado
