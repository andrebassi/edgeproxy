---
sidebar_position: 2
---

# Variáveis de Ambiente

Todas as configurações do edgeProxy são feitas via variáveis de ambiente.

## Configurações Core

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_LISTEN_ADDR` | `0.0.0.0:8080` | Endereço TCP para escutar |
| `EDGEPROXY_DB_PATH` | `routing.db` | Caminho para o banco SQLite |
| `EDGEPROXY_REGION` | `sa` | Identificador da região do POP |

## Sincronização do Banco

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_DB_RELOAD_SECS` | `5` | Intervalo para recarregar routing.db (segundos) |

## Afinidade de Cliente

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | TTL do binding do cliente (10 minutos) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Intervalo de garbage collection |

## Debug

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `DEBUG` | *(não definido)* | Habilita logs de debug quando definido |

## Configurações TLS

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_TLS_ENABLED` | `false` | Habilitar servidor TLS |
| `EDGEPROXY_TLS_LISTEN_ADDR` | `0.0.0.0:8443` | Endereço TLS |
| `EDGEPROXY_TLS_CERT` | *(nenhum)* | Caminho para certificado TLS (PEM) |
| `EDGEPROXY_TLS_KEY` | *(nenhum)* | Caminho para chave privada TLS (PEM) |

## Configurações DNS Interno

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_DNS_ENABLED` | `false` | Habilitar servidor DNS |
| `EDGEPROXY_DNS_LISTEN_ADDR` | `0.0.0.0:5353` | Endereço DNS |
| `EDGEPROXY_DNS_DOMAIN` | `internal` | Sufixo do domínio DNS |

## Configurações da API Auto-Discovery

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_API_ENABLED` | `false` | Habilitar API Auto-Discovery |
| `EDGEPROXY_API_LISTEN_ADDR` | `0.0.0.0:8081` | Endereço da API |
| `EDGEPROXY_HEARTBEAT_TTL_SECS` | `60` | TTL do heartbeat do backend |

## Configurações de Replicação Built-in

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_REPLICATION_ENABLED` | `false` | Habilitar replicação built-in |
| `EDGEPROXY_REPLICATION_NODE_ID` | (hostname) | Identificador único do nó |
| `EDGEPROXY_REPLICATION_GOSSIP_ADDR` | `0.0.0.0:4001` | Endereço UDP para protocolo gossip |
| `EDGEPROXY_REPLICATION_TRANSPORT_ADDR` | `0.0.0.0:4002` | Endereço QUIC para sync de dados |
| `EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS` | (nenhum) | Lista de peers separados por vírgula |
| `EDGEPROXY_REPLICATION_GOSSIP_INTERVAL_MS` | `1000` | Intervalo de ping gossip |
| `EDGEPROXY_REPLICATION_SYNC_INTERVAL_MS` | `5000` | Intervalo de flush do sync |
| `EDGEPROXY_REPLICATION_CLUSTER_NAME` | `edgeproxy` | Nome do cluster para isolamento |

Veja [Replicação Built-in](./replication) para documentação detalhada.

## GeoIP

O banco de dados MaxMind GeoLite2 está **embutido no binário** - não requer download ou configuração externa.

### Mapeamento País para Região

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

Não é necessário reiniciar.

## Logging

### Níveis de Log

- **INFO** (padrão): Mensagens de inicialização, reloads de roteamento
- **DEBUG** (quando `DEBUG=1`): Detalhes de conexão, seleção de backend

### Exemplo de Saída

```
INFO edge_proxy: starting edgeProxy region=sa listen=0.0.0.0:8080
INFO edge_proxy::proxy: edgeProxy listening on 0.0.0.0:8080
INFO edge_proxy::db: routing reload ok, version=1 backends=9
DEBUG edge_proxy::proxy: proxying 10.10.0.100 -> sa-node-1 (10.10.1.1:8080)
```
