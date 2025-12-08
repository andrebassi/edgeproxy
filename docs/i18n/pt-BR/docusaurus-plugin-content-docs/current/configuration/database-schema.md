---
sidebar_position: 3
---

# Schema do Banco de Dados

O banco SQLite `routing.db` contém a configuração dos backends.

## Estrutura da Tabela

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Identificador único (ex: "sa-node-1")
    app TEXT,                 -- Nome da aplicação (ex: "myapp")
    region TEXT,              -- Código da região: "sa", "us", "eu"
    wg_ip TEXT,               -- Endereço IP do backend
    port INTEGER,             -- Porta do backend
    healthy INTEGER,          -- 1 = saudável, 0 = não saudável
    weight INTEGER,           -- Peso no load balancing (maior = mais tráfego)
    soft_limit INTEGER,       -- Limite preferido de conexões
    hard_limit INTEGER,       -- Limite máximo absoluto de conexões
    deleted INTEGER DEFAULT 0 -- Flag de soft delete
);
```

## Dados de Exemplo

```sql
INSERT INTO backends VALUES
    ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 2, 50, 100, 0),
    ('sa-node-2', 'myapp', 'sa', '10.50.1.2', 8080, 1, 1, 50, 100, 0),
    ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 2, 50, 100, 0),
    ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 2, 50, 100, 0);
```

## Descrição dos Campos

### `region`

Identificador da região geográfica. Valores padrão:

| Código | Descrição |
|--------|-----------|
| `sa` | América do Sul (Brasil, Argentina, Chile, etc.) |
| `us` | América do Norte (EUA, Canadá, México) |
| `eu` | Europa (Alemanha, França, Reino Unido, etc.) |
| `ap` | Ásia-Pacífico (Japão, Singapura, Austrália) |

### `weight`

Peso relativo para load balancing. Valores maiores recebem mais tráfego:

- `weight=2`: Recebe 2x mais tráfego que weight=1
- `weight=1`: Participação padrão de tráfego
- `weight=0`: Efetivamente desabilitado (não recomendado, use `healthy=0`)

### `soft_limit` vs `hard_limit`

- **soft_limit**: Contagem alvo de conexões. Acima disso, o backend é considerado "carregado" e recebe uma pontuação maior.
- **hard_limit**: Máximo absoluto. Conexões são recusadas acima deste limite.

```
connections < soft_limit  → Pontuação baixa (preferido)
soft_limit ≤ connections < hard_limit → Pontuação alta (menos preferido)
connections ≥ hard_limit → Backend excluído
```

## Gerenciamento do Banco

### Visualizar Todos os Backends

```bash
sqlite3 routing.db "SELECT * FROM backends WHERE deleted=0"
```

### Adicionar um Backend

```bash
sqlite3 routing.db "INSERT INTO backends VALUES ('eu-node-2', 'myapp', 'eu', '10.50.3.2', 8080, 1, 2, 50, 100, 0)"
```

### Marcar Backend como Não Saudável

```bash
sqlite3 routing.db "UPDATE backends SET healthy=0 WHERE id='sa-node-1'"
```

### Soft Delete de Backend

```bash
sqlite3 routing.db "UPDATE backends SET deleted=1 WHERE id='sa-node-1'"
```

### Ajustar Peso

```bash
sqlite3 routing.db "UPDATE backends SET weight=3 WHERE id='us-node-1'"
```

## Reload Automático

As mudanças são automaticamente detectadas baseado em `EDGEPROXY_DB_RELOAD_SECS` (padrão: 5 segundos). Não é necessário reiniciar.
