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

O edgeProxy é projetado para deploys multi-região com rede mesh WireGuard e replicação SQLite distribuída built-in (SWIM gossip + QUIC transport):

![Arquitetura Multi-Região](/img/multi-region.svg)

## Arquitetura Hexagonal (Ports & Adapters)

O edgeProxy usa **Arquitetura Hexagonal** para separar lógica de negócio de detalhes de infraestrutura.

### Por que Hexagonal?

1. **Testabilidade**: O algoritmo de load balancing é uma função pura - não depende de SQLite, DashMap ou qualquer infraestrutura. Pode ser testado unitariamente com dados mockados.

2. **Flexibilidade**: Quer trocar SQLite por PostgreSQL? Basta criar um novo adapter que implemente `BackendRepository`. O domínio não muda.

3. **Separação de Responsabilidades**:
   - **Domain**: Regras de negócio puras (scoring, affinity logic)
   - **Application**: Orquestração (coordena domain + adapters)
   - **Adapters**: Detalhes de infraestrutura (SQLite, DashMap, MaxMind)

4. **Inversão de Dependência**: O domínio define interfaces (ports/traits), adapters implementam. Domínio nunca importa código de infraestrutura.

### Estrutura do Projeto

```
src/
├── main.rs                 # Composition root
├── config.rs               # Configuração do ambiente
├── domain/                 # Lógica core (sem deps externas)
│   ├── entities.rs         # Backend, Binding, ClientKey, GeoInfo
│   ├── value_objects.rs    # RegionCode
│   ├── ports/              # Interfaces (traits)
│   │   ├── backend_repository.rs
│   │   ├── binding_repository.rs
│   │   ├── geo_resolver.rs
│   │   └── metrics_store.rs
│   └── services/
│       └── load_balancer.rs  # Algoritmo de scoring (puro)
├── application/            # Use cases / orquestração
│   └── proxy_service.rs
└── adapters/               # Implementações de infraestrutura
    ├── inbound/
    │   └── tcp_server.rs     # TCP listener
    └── outbound/
        ├── sqlite_backend_repo.rs    # BackendRepository impl
        ├── dashmap_binding_repo.rs   # BindingRepository impl
        ├── maxmind_geo_resolver.rs   # GeoResolver impl
        └── dashmap_metrics_store.rs  # MetricsStore impl
```

### Diagrama de Camadas

![Camadas da Arquitetura Hexagonal](/img/hexagonal-layers.svg)

### Ports (Traits)

Os **ports** são traits que definem o que o domínio precisa, sem saber COMO será implementado:

```rust
// domain/ports/backend_repository.rs
#[async_trait]
pub trait BackendRepository: Send + Sync {
    async fn get_all(&self) -> Vec<Backend>;
    async fn get_by_id(&self, id: &str) -> Option<Backend>;
    async fn get_healthy(&self) -> Vec<Backend>;
}

// domain/ports/geo_resolver.rs
pub trait GeoResolver: Send + Sync {
    fn resolve(&self, ip: IpAddr) -> Option<GeoInfo>;
}

// domain/ports/metrics_store.rs
pub trait MetricsStore: Send + Sync {
    fn get_connection_count(&self, backend_id: &str) -> usize;
    fn increment_connections(&self, backend_id: &str);
    fn decrement_connections(&self, backend_id: &str);
    fn record_rtt(&self, backend_id: &str, rtt_ms: u64);
}
```

### Adapters (Implementações)

Os **adapters** implementam os ports com tecnologias específicas:

```rust
// adapters/outbound/sqlite_backend_repo.rs
#[async_trait]
impl BackendRepository for SqliteBackendRepository {
    async fn get_healthy(&self) -> Vec<Backend> {
        self.backends.read().await
            .iter()
            .filter(|b| b.healthy)
            .cloned()
            .collect()
    }
}

// adapters/outbound/maxmind_geo_resolver.rs
impl GeoResolver for MaxMindGeoResolver {
    fn resolve(&self, ip: IpAddr) -> Option<GeoInfo> {
        let resp: CountryResp = self.reader.lookup(ip).ok()?;
        let iso = resp.country?.iso_code?;
        let region = RegionCode::from_country(&iso);
        Some(GeoInfo::new(iso, region))
    }
}
```

### Composition Root (main.rs)

O `main.rs` é o único lugar que conhece TODAS as implementações concretas:

```rust
// main.rs - Composition Root
let backend_repo = Arc::new(SqliteBackendRepository::new());
let binding_repo = Arc::new(DashMapBindingRepository::new());
let geo_resolver = Arc::new(MaxMindGeoResolver::embedded()?);
let metrics = Arc::new(DashMapMetricsStore::new());

let proxy_service = Arc::new(ProxyService::new(
    backend_repo,    // trait BackendRepository
    binding_repo,    // trait BindingRepository
    geo_resolver,    // trait GeoResolver
    metrics,         // trait MetricsStore
    RegionCode::from_str(&cfg.region),
));

let server = TcpServer::new(proxy_service, cfg.listen_addr);
server.run().await
```

### Benefícios Práticos

| Cenário | Sem Hexagonal | Com Hexagonal |
|---------|---------------|---------------|
| Testar LoadBalancer | Precisa de SQLite rodando | Mock simples do trait |
| Trocar SQLite→Postgres | Refatorar todo o código | Criar novo adapter |
| Adicionar Redis cache | Modificar state.rs | Criar adapter que implementa port |
| Entender o domínio | Ler código misturado | Olhar só `domain/` |

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

### 2. Banco de Roteamento (`adapters/outbound/sqlite_backend_repo.rs`)

Banco SQLite contendo definições de backends:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Identificador único do backend
    app TEXT,                 -- Nome da aplicação
    region TEXT,              -- Região geográfica (sa, us, eu)
    country TEXT,             -- Código do país (BR, US, FR)
    wg_ip TEXT,               -- IP do overlay WireGuard
    port INTEGER,             -- Porta do backend
    healthy INTEGER,          -- Status de saúde (0/1)
    weight INTEGER,           -- Peso para balanceamento
    soft_limit INTEGER,       -- Máximo preferido de conexões
    hard_limit INTEGER,       -- Máximo absoluto de conexões
    deleted INTEGER           -- Flag de soft delete
);
```

O banco é recarregado periodicamente (padrão: 5 segundos) via task Tokio em background:

```rust
impl SqliteBackendRepository {
    pub fn start_sync(&self, db_path: String, interval_secs: u64) {
        let backends = self.backends.clone();
        tokio::spawn(async move {
            loop {
                let new_backends = Self::load_from_sqlite(&db_path)?;
                *backends.write().await = new_backends;
                sleep(Duration::from_secs(interval_secs)).await;
            }
        });
    }
}
```

### 3. Load Balancer (`domain/services/load_balancer.rs`)

Função pura SEM dependências externas. Recebe uma closure para obter contagem de conexões:

```rust
impl LoadBalancer {
    pub fn pick_backend<F>(
        backends: &[Backend],
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
        get_conn_count: F,  // Closure injetada - não conhece DashMap
    ) -> Option<Backend>
    where
        F: Fn(&str) -> usize,
    {
        // Algoritmo de scoring puro
        // geo_score * 100 + (load_factor / weight)
    }
}
```

**Scoring:**

```
score = geo_score * 100 + (load_factor / weight)

onde:
  geo_score = 0 (mesmo país do cliente - melhor)
            = 1 (mesma região do cliente)
            = 2 (mesma região do POP local)
            = 3 (fallback - cross region)

  load_factor = conexões_atuais / soft_limit
  weight = peso do backend (maior = mais preferido)
```

### 4. Serviço de Aplicação (`application/proxy_service.rs`)

Orquestra a lógica de domínio e coordena adapters:

```rust
pub struct ProxyService {
    backend_repo: Arc<dyn BackendRepository>,
    binding_repo: Arc<dyn BindingRepository>,
    geo_resolver: Option<Arc<dyn GeoResolver>>,
    metrics: Arc<dyn MetricsStore>,
    local_region: RegionCode,
}

impl ProxyService {
    pub async fn resolve_backend(&self, client_ip: IpAddr) -> Option<Backend> {
        // 1. Verificar binding existente
        // 2. Resolver geo do cliente
        // 3. Chamar LoadBalancer com closure de métricas injetada
        // 4. Criar novo binding
    }
}
```

### 5. Servidor TCP (`adapters/inbound/tcp_server.rs`)

Adapter inbound que aceita conexões e chama o serviço de aplicação:

```rust
impl TcpServer {
    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.listen_addr).await?;

        loop {
            let (stream, addr) = listener.accept().await?;
            let service = self.proxy_service.clone();

            tokio::spawn(async move {
                // Resolver backend via ProxyService
                let backend = service.resolve_backend(addr.ip()).await?;

                // Conectar ao backend, registrar métricas
                // Cópia TCP bidirecional
            });
        }
    }
}
```

## Fluxo de Conexão

O fluxo de requisição mostra o ciclo de vida completo de uma conexão TCP através do edgeProxy:

![Request Flow](/img/request-flow.svg)

```
1. Conexão TCP do cliente chega no TcpServer (inbound adapter)
2. TcpServer chama ProxyService.resolve_backend()
3. ProxyService verifica BindingRepository para binding existente
4. Se não há binding: ProxyService resolve geo via GeoResolver
5. ProxyService chama LoadBalancer.pick_backend() com closure de métricas
6. LoadBalancer retorna melhor backend (lógica de domínio pura)
7. ProxyService cria binding via BindingRepository
8. TcpServer conecta ao backend, registra métricas via MetricsStore
9. Cópia TCP bidirecional (L4 passthrough)
10. Na desconexão: TcpServer decrementa contagem de conexões
```

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
- **Replicação**: Sync distribuído built-in via SWIM gossip + QUIC
- **Transações**: Garantias ACID para atualizações de roteamento

### Por que WireGuard?

- **Criptografia**: Overlay seguro entre POPs
- **Performance**: Criptografia a nível de kernel com overhead mínimo
- **Simplicidade**: Configuração point-to-point

### Por que Arquitetura Hexagonal?

- **Testabilidade**: Lógica de domínio pode ser testada sem infraestrutura
- **Flexibilidade**: Fácil trocar implementações (SQLite→PostgreSQL)
- **Manutenibilidade**: Clara separação de responsabilidades
- **Onboarding**: Novos devs podem entender o domínio lendo só `domain/`

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

## Adicionando Novos Adapters

Para adicionar um novo adapter (ex: PostgreSQL para backends):

1. Criar `adapters/outbound/postgres_backend_repo.rs`
2. Implementar trait `BackendRepository`
3. Atualizar composition root no `main.rs`

```rust
// adapters/outbound/postgres_backend_repo.rs
pub struct PostgresBackendRepository {
    pool: PgPool,
}

#[async_trait]
impl BackendRepository for PostgresBackendRepository {
    async fn get_healthy(&self) -> Vec<Backend> {
        sqlx::query_as!(Backend, "SELECT * FROM backends WHERE healthy = true")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default()
    }
}

// main.rs - só muda a composição
let backend_repo = Arc::new(PostgresBackendRepository::new(pool));
// resto do código permanece igual!
```

## Próximos Passos

- [Configuração](./configuration) - Todas as opções disponíveis
- [Internals do Load Balancer](./internals/load-balancer) - Algoritmo de scoring detalhado
- [Deploy com Docker](./deployment/docker) - Setup de containers
