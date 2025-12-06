---
sidebar_position: 2
---

# Afinidade de Cliente

A afinidade de cliente (sticky sessions) garante que conexões repetidas do mesmo IP de cliente sejam roteadas para o mesmo backend. Isso é crítico para protocolos stateful e aplicações baseadas em sessão.

## Visão Geral

O edgeProxy mantém uma tabela de binding que mapeia IPs de cliente para IDs de backend. O diagrama a seguir mostra o ciclo de vida do binding:

![Client Affinity](/img/client-affinity.svg)

## Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | Tempo de vida do binding (10 minutos) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Intervalo de limpeza |

## Estruturas de Dados

### ClientKey

```rust
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClientKey {
    pub client_ip: IpAddr,
}
```

### Binding

```rust
#[derive(Clone, Debug)]
pub struct Binding {
    pub backend_id: String,
    pub created_at: Instant,
    pub last_seen: Instant,
}
```

### Armazenamento

Bindings são armazenados em um `DashMap` lock-free:

```rust
pub struct RcProxyState {
    // ...
    pub bindings: Arc<DashMap<ClientKey, Binding>>,
    // ...
}
```

## Ciclo de Vida

### 1. Nova Conexão

Quando um cliente conecta pela primeira vez:

```rust
// Sem binding existente - usar load balancer
let backend = pick_backend(&backends, local_region, client_region);

// Criar novo binding
state.bindings.insert(
    ClientKey { client_ip },
    Binding {
        backend_id: backend.id.clone(),
        created_at: Instant::now(),
        last_seen: Instant::now(),
    },
);
```

### 2. Conexões Subsequentes

Quando o mesmo cliente reconecta:

```rust
// Verificar binding existente
if let Some(mut entry) = state.bindings.get_mut(&client_key) {
    // Atualizar timestamp last_seen
    entry.last_seen = Instant::now();

    // Usar backend existente
    chosen_backend_id = Some(entry.backend_id.clone());
}
```

### 3. Expiração do Binding

Bindings expiram após `BINDING_TTL_SECS` de inatividade:

```rust
pub fn start_binding_gc(
    bindings: Arc<DashMap<ClientKey, Binding>>,
    ttl: Duration,
    interval: Duration,
) {
    tokio::spawn(async move {
        loop {
            sleep(interval).await;

            let now = Instant::now();
            bindings.retain(|_, binding| {
                now.duration_since(binding.last_seen) < ttl
            });
        }
    });
}
```

### 4. Falha do Backend

Se o backend vinculado ficar não saudável:

```rust
// Lookup do backend do binding
let backend = rt.backends
    .iter()
    .find(|b| b.id == backend_id && b.healthy)
    .cloned();

// Se não encontrado ou não saudável
if backend.is_none() {
    // Remover binding obsoleto
    state.bindings.remove(&client_key);

    // Fallback para load balancer
    return pick_backend(&backends, ...);
}
```

## Casos de Uso

### 1. Aplicações Stateful

Jogos, servidores de chat ou qualquer aplicação mantendo estado de conexão:

```
Cliente A ──▶ game-server-1 (estado do jogador)
Cliente A ──▶ game-server-1 (mesmo servidor, estado preservado)
```

### 2. Protocolos Baseados em Sessão

Aplicações usando cookies ou tokens de sessão:

```
Cliente B ──▶ web-server-2 (sessão criada)
Cliente B ──▶ web-server-2 (sessão recuperada)
```

### 3. Pool de Conexões

Conexões de banco de dados ou conexões HTTP persistentes:

```
Cliente C ──▶ db-replica-1 (conexão 1)
Cliente C ──▶ db-replica-1 (conexão 2, mesma réplica)
```

## Performance

### Uso de Memória

Cada binding usa aproximadamente:

```
ClientKey: 16 bytes (IPv4) ou 40 bytes (IPv6)
Binding: ~80 bytes (String + 2 Instants)
Overhead DashMap: ~64 bytes por entrada

Total: ~160 bytes por cliente
```

Para 1 milhão de clientes: ~160 MB

### Garbage Collection

GC executa a cada `BINDING_GC_INTERVAL_SECS`:

```rust
// Iterar todos os bindings
bindings.retain(|_, binding| {
    now.duration_since(binding.last_seen) < ttl
});
```

Complexidade de tempo: O(n) onde n = total de bindings

### Concorrência

DashMap fornece leituras lock-free e escritas sharded:

- Leitura (lookup de binding): Sem bloqueio
- Escrita (criar/atualizar binding): Locking por-shard
- GC (retain): Locks breves por-shard

## Tuning

### Conexões de Alta Frequência

Para clientes fazendo muitas conexões curtas:

```bash
# TTL menor para liberar memória mais rápido
export EDGEPROXY_BINDING_TTL_SECS=60

# GC mais frequente
export EDGEPROXY_BINDING_GC_INTERVAL_SECS=10
```

### Sessões de Longa Duração

Para conexões persistentes ou reconexões infrequentes:

```bash
# TTL maior para manter afinidade
export EDGEPROXY_BINDING_TTL_SECS=3600  # 1 hora

# GC menos frequente (menor CPU)
export EDGEPROXY_BINDING_GC_INTERVAL_SECS=300
```

### Alto Volume de Clientes

Para milhões de clientes únicos:

```bash
# TTL agressivo para limitar memória
export EDGEPROXY_BINDING_TTL_SECS=300

# GC frequente
export EDGEPROXY_BINDING_GC_INTERVAL_SECS=30
```

## Limitações

### 1. Apenas Baseado em IP

Afinidade é baseada no IP do cliente, não em:
- Cookies HTTP
- Tickets de sessão TLS
- Tokens de aplicação

**Implicação:** Clientes atrás de NAT compartilham afinidade.

### 2. Sem Sync Cross-POP

Bindings são locais para cada instância de POP:

```
Cliente → POP-SA → sa-node-1 (binding criado)
Cliente → POP-US → us-node-1 (binding diferente!)
```

**Solução:** Usar geo-routing DNS para garantir que clientes atinjam POPs consistentes.

## Próximos Passos

- [Load Balancer](./load-balancer) - Algoritmo de seleção de backend
- [Arquitetura](../architecture) - Visão geral do sistema
- [Configuração](../configuration) - Todas as opções
