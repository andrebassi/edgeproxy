---
sidebar_position: 5
---

# Configuração

O edgeProxy é configurado inteiramente através de variáveis de ambiente. Este documento cobre todas as opções disponíveis com exemplos.

## Variáveis de Ambiente

### Configurações Core

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_LISTEN_ADDR` | `0.0.0.0:8080` | Endereço TCP para escutar |
| `EDGEPROXY_DB_PATH` | `routing.db` | Caminho para o banco de roteamento SQLite |
| `EDGEPROXY_REGION` | `sa` | Identificador da região do POP local |

### Sync do Banco

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_DB_RELOAD_SECS` | `5` | Intervalo para recarregar routing.db (segundos) |

### Afinidade de Cliente

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | TTL do binding de cliente (10 minutos) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Intervalo de garbage collection |

### Debugging

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `DEBUG` | *(não definido)* | Habilita logging debug quando definido |

### Configurações TLS

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_TLS_ENABLED` | `false` | Habilita servidor TLS |
| `EDGEPROXY_TLS_LISTEN_ADDR` | `0.0.0.0:8443` | Endereço de escuta TLS |
| `EDGEPROXY_TLS_CERT` | *(nenhum)* | Caminho para certificado TLS (PEM) |
| `EDGEPROXY_TLS_KEY` | *(nenhum)* | Caminho para chave privada TLS (PEM) |

### Configurações do DNS Interno

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_DNS_ENABLED` | `false` | Habilita servidor DNS |
| `EDGEPROXY_DNS_LISTEN_ADDR` | `0.0.0.0:5353` | Endereço de escuta DNS |
| `EDGEPROXY_DNS_DOMAIN` | `internal` | Sufixo de domínio DNS |

### Configurações da API de Auto-Discovery

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_API_ENABLED` | `false` | Habilita API de Auto-Discovery |
| `EDGEPROXY_API_LISTEN_ADDR` | `0.0.0.0:8081` | Endereço de escuta da API |
| `EDGEPROXY_HEARTBEAT_TTL_SECS` | `60` | TTL do heartbeat dos backends |

### Configurações do Corrosion (SQLite Distribuído)

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_CORROSION_ENABLED` | `false` | Habilita backend Corrosion |
| `EDGEPROXY_CORROSION_API_URL` | `http://localhost:8080` | URL da API HTTP do Corrosion |
| `EDGEPROXY_CORROSION_POLL_SECS` | `5` | Intervalo de polling para sync de backends |

## Exemplos de Configuração

### Desenvolvimento

```bash
export EDGEPROXY_LISTEN_ADDR="127.0.0.1:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="./routing.db"
export EDGEPROXY_BINDING_TTL_SECS="60"
export DEBUG="1"

./target/release/edge-proxy
```

### Produção (POP América do Sul)

```bash
export EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="/data/routing.db"
export EDGEPROXY_DB_RELOAD_SECS="5"
export EDGEPROXY_BINDING_TTL_SECS="600"
export EDGEPROXY_BINDING_GC_INTERVAL_SECS="60"

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
      - DEBUG=1
    ports:
      - "8080:8080"
    volumes:
      - ./routing.db:/app/routing.db:ro
```

## Schema do Banco de Roteamento

O banco SQLite `routing.db` contém a configuração dos backends:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Identificador único (ex: "sa-node-1")
    app TEXT,                 -- Nome da aplicação (ex: "myapp")
    region TEXT,              -- Código da região: "sa", "us", "eu"
    wg_ip TEXT,               -- Endereço IP do backend
    port INTEGER,             -- Porta do backend
    healthy INTEGER,          -- 1 = saudável, 0 = não saudável
    weight INTEGER,           -- Peso de balanceamento (maior = mais tráfego)
    soft_limit INTEGER,       -- Máximo preferido de conexões
    hard_limit INTEGER,       -- Máximo absoluto de conexões
    deleted INTEGER DEFAULT 0 -- Flag de soft delete
);
```

### Dados de Exemplo

```sql
INSERT INTO backends VALUES
    ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 2, 50, 100, 0),
    ('sa-node-2', 'myapp', 'sa', '10.50.1.2', 8080, 1, 1, 50, 100, 0),
    ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 2, 50, 100, 0),
    ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 2, 50, 100, 0);
```

### Descrição dos Campos

#### `region`

Identificador de região geográfica. Valores padrão:

| Código | Descrição |
|--------|-----------|
| `sa` | América do Sul (Brasil, Argentina, Chile, etc.) |
| `us` | América do Norte (EUA, Canadá, México) |
| `eu` | Europa (Alemanha, França, UK, etc.) |
| `ap` | Ásia Pacífico (Japão, Singapura, Austrália) |

#### `weight`

Peso relativo para balanceamento de carga. Valores maiores recebem mais tráfego:

- `weight=2`: Recebe 2x mais tráfego que weight=1
- `weight=1`: Parcela padrão de tráfego
- `weight=0`: Efetivamente desabilitado (não recomendado, use `healthy=0`)

#### `soft_limit` vs `hard_limit`

- **soft_limit**: Contagem alvo de conexões. Além disso, o backend é considerado "carregado" e recebe score maior.
- **hard_limit**: Máximo absoluto. Conexões são recusadas além deste limite.

```
conexões < soft_limit  → Score baixo (preferido)
soft_limit ≤ conexões < hard_limit → Score maior (menos preferido)
conexões ≥ hard_limit → Backend excluído
```

## GeoIP

O banco de dados MaxMind GeoLite2 está **embeddado no binário** - nenhum download ou configuração externa necessária.

### Mapeamento de País para Região

Mapeamento padrão em `state.rs`:

```rust
match iso_code {
    // América do Sul
    "BR" | "AR" | "CL" | "PE" | "CO" | "UY" | "PY" | "BO" | "EC" => "sa",

    // América do Norte
    "US" | "CA" | "MX" => "us",

    // Europa
    "PT" | "ES" | "FR" | "DE" | "NL" | "IT" | "GB" | "IE" | "BE" | "CH" => "eu",

    // Fallback padrão
    _ => "us",
}
```

## Hot Reload

O banco de roteamento é automaticamente recarregado a cada `EDGEPROXY_DB_RELOAD_SECS` segundos. Para atualizar a configuração:

1. Modifique o banco SQLite:
   ```bash
   sqlite3 routing.db "UPDATE backends SET healthy=0 WHERE id='sa-node-1'"
   ```

2. Aguarde o reload (verifique os logs):
   ```
   INFO edge_proxy::db: routing reload ok, version=5 backends=9
   ```

Nenhum reinício necessário.

## Logging

### Níveis de Log

- **INFO** (padrão): Mensagens de startup, reloads de roteamento
- **DEBUG** (quando `DEBUG=1`): Detalhes de conexão, seleção de backend

### Exemplo de Saída

```
INFO edge_proxy: starting edgeProxy region=sa listen=0.0.0.0:8080
INFO edge_proxy::proxy: edgeProxy listening on 0.0.0.0:8080
INFO edge_proxy::db: routing reload ok, version=1 backends=9
DEBUG edge_proxy::proxy: proxying 10.10.0.100 -> sa-node-1 (10.10.1.1:8080)
```

---

## Servidor DNS Interno

O servidor DNS fornece resolução de nomes geo-aware para domínios `.internal`.

### Uso

```bash
# Habilitar servidor DNS
export EDGEPROXY_DNS_ENABLED=true
export EDGEPROXY_DNS_LISTEN_ADDR=0.0.0.0:5353
export EDGEPROXY_DNS_DOMAIN=internal

# Consultar melhor IP de backend (geo-aware)
dig @localhost -p 5353 myapp.internal A

# Resposta: Melhor IP de backend baseado na localização do cliente
;; ANSWER SECTION:
myapp.internal.    300    IN    A    10.50.1.5
```

### Schema DNS

| Domínio | Resolve Para | Exemplo |
|---------|--------------|---------|
| `<app>.internal` | Melhor IP de backend | `myapp.internal` → `10.50.1.5` |
| `<region>.backends.internal` | IP WG do backend | `nrt.backends.internal` → `10.50.4.1` |
| `<region>.pops.internal` | IP WG do POP | `hkg.pops.internal` → `10.50.5.1` |

### Benefícios

- **Abstração**: Mude IPs sem atualizar configs
- **Migração**: Mova backends sem downtime
- **Geo-aware**: Retorna melhor backend baseado na localização do cliente

---

## API de Auto-Discovery

A API permite que backends se registrem e desregistrem automaticamente.

### Endpoints

| Método | Endpoint | Descrição |
|--------|----------|-----------|
| GET | `/health` | Health check + versão + contagem de backends |
| POST | `/api/v1/register` | Registrar novo backend |
| POST | `/api/v1/heartbeat/:id` | Atualizar heartbeat do backend |
| GET | `/api/v1/backends` | Listar todos os backends registrados |
| GET | `/api/v1/backends/:id` | Obter detalhes de backend específico |
| DELETE | `/api/v1/backends/:id` | Desregistrar um backend |

### Exemplo de Registro

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

# Enviar heartbeat (manter vivo)
curl -X POST http://localhost:8081/api/v1/heartbeat/backend-eu-1

# Listar todos os backends
curl http://localhost:8081/api/v1/backends
```

### Benefícios

- **Zero configuração**: Backends apenas iniciam e se registram
- **Escalonamento automático**: Novas instâncias aparecem automaticamente
- **Shutdown gracioso**: Desregistro limpo
- **Health baseado em TTL**: Não saudável = expirado = desregistrado

---

## Control Plane Distribuído (Corrosion)

Corrosion habilita replicação distribuída de SQLite em todos os POPs.

### Arquitetura

![Arquitetura Corrosion](/img/corrosion-architecture.svg)

### Como Funciona

Quando `EDGEPROXY_CORROSION_ENABLED=true`, edgeProxy **ignora** o `EDGEPROXY_DB_PATH` local e consulta a API HTTP do Corrosion para dados de backend. O Corrosion gerencia toda a replicação entre POPs automaticamente.

![Fluxo de Dados Corrosion](/img/corrosion-data-flow.svg)

### Configuração

```bash
# Habilitar backend Corrosion (substitui SQLite local)
export EDGEPROXY_CORROSION_ENABLED=true
export EDGEPROXY_CORROSION_API_URL=http://corrosion:8080
export EDGEPROXY_CORROSION_POLL_SECS=5

# Nota: EDGEPROXY_DB_PATH é IGNORADO quando Corrosion está habilitado
./edgeproxy
```

### Configuração do Agente Corrosion

O agente Corrosion roda como sidecar e gerencia seu próprio banco replicado:

```toml
# corrosion.toml (config do agente Corrosion, NÃO do edgeProxy)
[db]
path = "/var/lib/corrosion/state.db"  # Estado interno do Corrosion

[cluster]
name = "edgeproxy"
bootstrap = ["10.50.0.1:4001", "10.50.5.1:4001"]

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8080"  # edgeProxy conecta aqui
```

### Benefícios

- **Sync em tempo real**: Mudanças propagam em ~100ms via protocolo gossip
- **Sem intervenção manual**: Replicação automática entre todos os POPs
- **Tolerância a partições**: Funciona durante splits de rede (baseado em CRDT)
- **Fonte única de verdade**: Registre backend uma vez, disponível em todos os lugares

---

## Próximos Passos

- [Deploy com Docker](./deployment/docker) - Configuração de container
- [Deploy com Fly.io](./deployment/flyio) - Deploy global no edge
- [Internals do Load Balancer](./internals/load-balancer) - Detalhes de scoring
