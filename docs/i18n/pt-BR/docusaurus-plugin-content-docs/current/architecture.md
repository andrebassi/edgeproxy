---
sidebar_position: 3
---

# Arquitetura

Este documento fornece um deep dive na arquitetura interna do edgeProxy, fluxo de dados e decisões de design.

## Visão Geral do Sistema

O edgeProxy é projetado como um proxy L4 (TCP) stateless que pode ser implantado em múltiplos Points of Presence (POPs) ao redor do mundo. Cada instância de POP:

1. Aceita conexões TCP de clientes
2. Determina o backend otimal usando pontuação geo + carga
3. Mantém afinidade de cliente (sticky sessions)
4. Faz proxy do tráfego bidirecional de forma transparente

![Visão Geral da Arquitetura](/img/architecture-overview.svg)

## Deploy Multi-Região

O edgeProxy é projetado para deploys multi-região com rede mesh WireGuard e replicação SQLite distribuída via Corrosion:

![Arquitetura Multi-Região](/img/multi-region.svg)

## Componentes Core

### 1. Configuração (`config.rs`)

Carrega todas as configurações de variáveis de ambiente no startup:

```rust
pub struct Config {
    pub listen_addr: String,           // Endereço TCP para escutar
    pub db_path: String,               // Caminho do banco SQLite
    pub region: String,                // Região do POP local
    pub db_reload_secs: u64,           // Intervalo de reload do routing
    pub geoip_path: Option<String>,    // Caminho do banco MaxMind
    pub binding_ttl_secs: u64,         // TTL da afinidade de cliente
    pub binding_gc_interval_secs: u64, // Intervalo de limpeza
    pub debug: bool,                   // Logging verboso
}
```

### 2. Banco de Roteamento (`db.rs`)

Banco SQLite contendo definições de backends:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Identificador único do backend
    app TEXT,                 -- Nome da aplicação
    region TEXT,              -- Região geográfica (sa, us, eu)
    wg_ip TEXT,               -- IP do overlay WireGuard
    port INTEGER,             -- Porta do backend
    healthy INTEGER,          -- Status de saúde (0/1)
    weight INTEGER,           -- Peso para balanceamento
    soft_limit INTEGER,       -- Máximo preferido de conexões
    hard_limit INTEGER,       -- Máximo absoluto de conexões
    deleted INTEGER           -- Flag de soft delete
);
```

O banco é recarregado periodicamente (padrão: 5 segundos) para pegar mudanças sem reiniciar:

```rust
pub async fn start_routing_sync_sqlite(
    routing: Arc<RwLock<RoutingState>>,
    db_path: String,
    interval_secs: u64,
) -> Result<()> {
    loop {
        // Carregar backends do SQLite
        let new_state = load_routing_state_from_sqlite(&db_path)?;

        // Update atômico
        let mut guard = routing.write().await;
        *guard = new_state;

        sleep(Duration::from_secs(interval_secs)).await;
    }
}
```

### 3. Load Balancer (`lb.rs`)

O load balancer usa um sistema de pontuação para selecionar o backend otimal:

```
score = region_score * 100 + (load_factor / weight)

onde:
  region_score = 0 (região do cliente = região do backend)
               = 1 (região do POP local)
               = 2 (fallback/outras regiões)

  load_factor = conexões_atuais / soft_limit

  weight = peso configurado do backend (maior = mais preferido)
```

**Algoritmo:**

1. Filtrar backends: `healthy = true` E `conexões < hard_limit`
2. Calcular score para cada backend
3. Selecionar backend com menor score

### 4. Estado Compartilhado (`state.rs`)

Estado global compartilhado entre todas as conexões:

```rust
pub struct RcProxyState {
    pub local_region: String,
    pub routing: Arc<RwLock<RoutingState>>,
    pub bindings: Arc<DashMap<ClientKey, Binding>>,
    pub metrics: Arc<DashMap<String, BackendMetrics>>,
    pub geo: Option<GeoDb>,
}
```

**Estruturas chave:**

- `RoutingState`: Lista atual de backends (atualizada periodicamente)
- `DashMap<ClientKey, Binding>`: Mapa de afinidade lock-free
- `DashMap<String, BackendMetrics>`: Contagem de conexões e RTT por backend

### 5. Resolução GeoIP (`state.rs`)

Mapeia IPs de cliente para regiões usando MaxMind GeoLite2:

```rust
impl GeoDb {
    pub fn region_for_ip(&self, ip: IpAddr) -> Option<String> {
        let country: geoip2::Country = self.reader.lookup(ip).ok()?;
        let iso_code = country.country?.iso_code?;

        // Mapear país para região
        match iso_code {
            "BR" | "AR" | "CL" | "PE" | "CO" => Some("sa".to_string()),
            "US" | "CA" | "MX" => Some("us".to_string()),
            "PT" | "ES" | "FR" | "DE" | "GB" => Some("eu".to_string()),
            _ => Some("us".to_string()), // Fallback padrão
        }
    }
}
```

### 6. Proxy TCP (`proxy.rs`)

A lógica core do proxy lida com streaming TCP bidirecional:

```rust
async fn handle_connection(
    state: RcProxyState,
    client_stream: TcpStream,
    client_addr: SocketAddr,
) -> Result<()> {
    // 1. Verificar binding existente (afinidade)
    let client_key = ClientKey { client_ip: client_addr.ip() };

    // 2. Resolver backend
    let backend = if let Some(binding) = state.bindings.get(&client_key) {
        // Usar binding existente
        find_backend_by_id(&binding.backend_id)
    } else {
        // Escolher novo backend usando load balancer
        let client_region = state.geo.as_ref()
            .and_then(|g| g.region_for_ip(client_addr.ip()));
        pick_backend(&backends, &state.local_region, client_region.as_deref())
    };

    // 3. Conectar ao backend
    let backend_stream = TcpStream::connect(&backend_addr).await?;

    // 4. Atualizar métricas
    state.metrics.entry(backend.id.clone())
        .or_insert_with(BackendMetrics::new)
        .current_conns.fetch_add(1, Ordering::Relaxed);

    // 5. Cópia bidirecional com half-close adequado
    let (client_read, client_write) = client_stream.into_split();
    let (backend_read, backend_write) = backend_stream.into_split();

    let c2b = tokio::spawn(async move {
        io::copy(&mut client_read, &mut backend_write).await?;
        backend_write.shutdown().await
    });

    let b2c = tokio::spawn(async move {
        io::copy(&mut backend_read, &mut client_write).await
    });

    tokio::join!(c2b, b2c);

    // 6. Limpar métricas
    state.metrics.get(&backend.id)
        .map(|m| m.current_conns.fetch_sub(1, Ordering::Relaxed));

    Ok(())
}
```

## Fluxo de Conexão

O fluxo de requisição mostra o ciclo de vida completo de uma conexão TCP através do edgeProxy:

![Request Flow](/img/request-flow.svg)

## Decisões de Design

### Por que Rust?

- **Latência Previsível**: Sem pausas de garbage collection
- **Segurança de Memória**: Abstrações zero-cost sem overhead de runtime
- **I/O Assíncrono**: Tokio fornece rede event-driven eficiente
- **Performance**: Competitivo com implementações C/C++

### Por que DashMap?

- **Lock-Free**: Leituras concorrentes sem bloqueio
- **Sharded**: Locking distribuído para escritas
- **Drop-in**: API similar ao `HashMap`

### Por que SQLite?

- **Simplicidade**: Arquivo único, sem servidor necessário
- **Replicação**: Funciona com Corrosion para sync distribuído
- **Transações**: Garantias ACID para atualizações de roteamento

### Por que WireGuard?

- **Criptografia**: Overlay seguro entre POPs
- **Performance**: Criptografia a nível de kernel com overhead mínimo
- **Simplicidade**: Configuração point-to-point

## Considerações de Performance

### Tratamento de Conexões

- Cada conexão spawna duas tasks Tokio (cliente→backend, backend→cliente)
- `io::copy` usa splice otimizado do kernel quando disponível
- Half-close tratado adequadamente com `shutdown()`

### Uso de Memória

- Bindings armazenados em DashMap com expiração por TTL
- Garbage collection periódico remove entradas expiradas
- Lista de backends atualizada atomicamente sem picos de memória

### Escalabilidade

- Horizontal: Deploy de múltiplas instâncias edgeProxy atrás de DNS/Anycast
- Vertical: Tokio escala automaticamente para os cores de CPU disponíveis

## Próximos Passos

- [Configuração](./configuration) - Todas as opções disponíveis
- [Internals do Load Balancer](./internals/load-balancer) - Algoritmo de scoring detalhado
- [Deploy com Docker](./deployment/docker) - Setup de containers
