---
sidebar_position: 4
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

## Próximos Passos

- [Deploy com Docker](./deployment/docker) - Configuração de container
- [Deploy com Kubernetes](./deployment/kubernetes) - Manifests K8s
- [Internals do Load Balancer](./internals/load-balancer) - Detalhes de scoring
