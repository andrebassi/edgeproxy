---
sidebar_position: 2
---

# Testes da API de Auto-Discovery

Resultados dos testes para a API de Auto-Discovery que permite backends se registrarem dinamicamente.

**Data do Teste**: 2025-12-08
**Porta da API**: 8081

## Endpoints da API

| Endpoint | Método | Descrição |
|----------|--------|-----------|
| `/health` | GET | Health check com versão e contagem de backends |
| `/api/v1/register` | POST | Registrar um novo backend |
| `/api/v1/heartbeat/:id` | POST | Atualizar heartbeat do backend |
| `/api/v1/backends` | GET | Listar todos os backends registrados |
| `/api/v1/backends/:id` | GET | Obter detalhes de um backend específico |
| `/api/v1/backends/:id` | DELETE | Desregistrar um backend |

---

## Resultados dos Testes

### 1. Health Check

**Requisição**:
```bash
curl -s http://34.246.117.138:8081/health
```

**Resposta**:
```json
{
  "status": "ok",
  "version": "0.2.0",
  "registered_backends": 10
}
```

**Status**: OK

---

### 2. Registro de Backend

**Requisição**:
```bash
curl -s -X POST http://34.246.117.138:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{
    "id": "pop-gru",
    "app": "gru.pop",
    "region": "sa",
    "ip": "10.50.1.1",
    "port": 80
  }'
```

**Resposta**:
```json
{
  "id": "pop-gru",
  "registered": true,
  "message": "Backend registered successfully"
}
```

**Todos os Registros**:

| ID | App | Região | IP | Porta | Resultado |
|----|-----|--------|-----|-------|-----------|
| pop-gru | gru.pop | sa | 10.50.1.1 | 80 | OK |
| pop-iad | iad.pop | us | 10.50.2.1 | 80 | OK |
| pop-ord | ord.pop | us | 10.50.2.2 | 80 | OK |
| pop-lax | lax.pop | us | 10.50.2.3 | 80 | OK |
| pop-lhr | lhr.pop | eu | 10.50.3.1 | 80 | OK |
| pop-fra | fra.pop | eu | 10.50.3.2 | 80 | OK |
| pop-cdg | cdg.pop | eu | 10.50.3.3 | 80 | OK |
| pop-nrt | nrt.pop | ap | 10.50.4.1 | 80 | OK |
| pop-sin | sin.pop | ap | 10.50.4.2 | 80 | OK |
| pop-syd | syd.pop | ap | 10.50.4.3 | 80 | OK |

**Status**: 10/10 registros bem-sucedidos

---

### 3. Listar Backends

**Requisição**:
```bash
curl -s http://34.246.117.138:8081/api/v1/backends
```

**Resposta**:
```json
{
  "backends": [
    {
      "id": "pop-gru",
      "app": "gru.pop",
      "region": "sa",
      "ip": "10.50.1.1",
      "port": 80,
      "healthy": true,
      "last_heartbeat_secs": 5,
      "registered_secs": 120
    },
    {
      "id": "pop-iad",
      "app": "iad.pop",
      "region": "us",
      "ip": "10.50.2.1",
      "port": 80,
      "healthy": true,
      "last_heartbeat_secs": 5,
      "registered_secs": 118
    }
    // ... mais backends
  ],
  "total": 10
}
```

**Status**: OK - Todos os 10 backends listados

---

### 4. Obter Backend Específico

**Requisição**:
```bash
curl -s http://34.246.117.138:8081/api/v1/backends/pop-gru
```

**Resposta**:
```json
{
  "id": "pop-gru",
  "app": "gru.pop",
  "region": "sa",
  "ip": "10.50.1.1",
  "port": 80,
  "healthy": true,
  "last_heartbeat_secs": 10,
  "registered_secs": 125
}
```

**Status**: OK

---

### 5. Atualização de Heartbeat

**Requisição**:
```bash
curl -s -X POST http://34.246.117.138:8081/api/v1/heartbeat/pop-gru
```

**Resposta**:
```json
{
  "id": "pop-gru",
  "status": "ok"
}
```

**Status**: OK - Heartbeat atualizado, `last_heartbeat_secs` resetado para 0

---

### 6. Desregistro de Backend

**Requisição**:
```bash
curl -s -X DELETE http://34.246.117.138:8081/api/v1/backends/test-backend
```

**Resposta**:
```json
{
  "deregistered": true,
  "id": "test-backend"
}
```

**Status**: OK

---

## Schema do Payload de Registro

### Campos Obrigatórios

| Campo | Tipo | Descrição | Exemplo |
|-------|------|-----------|---------|
| `id` | string | Identificador único do backend | `"pop-gru"` |
| `app` | string | Nome da aplicação (usado para DNS) | `"gru.pop"` |
| `region` | string | Código da região (sa, us, eu, ap) | `"sa"` |
| `ip` | string | Endereço IP do backend | `"10.50.1.1"` |
| `port` | number | Porta do backend | `80` |

### Campos Opcionais (com valores padrão)

| Campo | Tipo | Padrão | Descrição |
|-------|------|--------|-----------|
| `country` | string | derivado | Código ISO do país |
| `weight` | number | `2` | Peso no balanceamento de carga (1-10) |
| `soft_limit` | number | `100` | Número confortável de conexões |
| `hard_limit` | number | `150` | Máximo de conexões |

### Exemplo de Payload Completo

```json
{
  "id": "my-backend-1",
  "app": "myapp",
  "region": "eu",
  "country": "DE",
  "ip": "10.50.3.1",
  "port": 8080,
  "weight": 5,
  "soft_limit": 200,
  "hard_limit": 300
}
```

---

## Gerenciamento de Saúde

### TTL do Heartbeat

Backends são marcados como não saudáveis se não enviarem heartbeat dentro do período de TTL.

| Configuração | Padrão | Variável de Ambiente |
|--------------|--------|---------------------|
| TTL do Heartbeat | 60 segundos | `EDGEPROXY_HEARTBEAT_TTL_SECS` |

### Limpeza Automática

- Backends que perdem heartbeat são marcados `healthy: false`
- Tarefa em background remove backends obsoletos periodicamente
- Backends não saudáveis são excluídos do balanceamento de carga

### Mantendo Backends Saudáveis

```bash
# Loop de heartbeat simples (a cada 30 segundos)
while true; do
  curl -s -X POST http://hub:8081/api/v1/heartbeat/my-backend-1
  sleep 30
done
```

---

## Integração com DNS

Backends registrados via API podem ser resolvidos através do DNS:

```bash
# Registrar backend com nome do app
curl -X POST http://hub:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{"id":"my-eu-1","app":"myservice","region":"eu","ip":"10.50.3.1","port":8080}'

# Resolver via DNS
dig @hub -p 5353 myservice.internal +short
# Retorna: 10.50.3.1
```

**Nota**: Backends registrados via API são armazenados em memória (DashMap). Para resolução DNS, backends também devem estar no routing.db.

---

## Respostas de Erro

### 400 Bad Request

```json
{
  "error": "Missing required field: id"
}
```

### 404 Not Found

```json
{
  "error": "Backend not found",
  "id": "unknown-backend"
}
```

### 409 Conflict

```json
{
  "error": "Backend already exists",
  "id": "existing-backend"
}
```

---

## Monitorando Backends Registrados

### Via API

```bash
# Contar backends
curl -s http://hub:8081/api/v1/backends | jq '.total'

# Listar backends saudáveis
curl -s http://hub:8081/api/v1/backends | jq '.backends[] | select(.healthy==true) | .id'

# Encontrar backends por região
curl -s http://hub:8081/api/v1/backends | jq '.backends[] | select(.region=="eu")'
```

### Via Logs

```bash
# Observar eventos de registro
sudo journalctl -u edgeproxy -f | grep -i "register\|heartbeat"
```

---

## Resumo dos Testes

| Teste | Resultado |
|-------|-----------|
| Health Check | OK |
| Registro (10 backends) | OK |
| Listar Backends | OK |
| Obter Backend Específico | OK |
| Atualização de Heartbeat | OK |
| Desregistro | OK |

**Total**: Todos os testes da API passando
