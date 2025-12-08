---
sidebar_position: 7
---

# Componentes de Infraestrutura

O edgeProxy inclui componentes de infraestrutura prontos para produção para confiabilidade e observabilidade.

## Graceful Shutdown

Trata sinais SIGTERM e Ctrl+C, permitindo que conexões em andamento completem antes do shutdown.

```rust
use edgeproxy::infrastructure::{ShutdownController, shutdown_signal};

// Criar controller
let shutdown = ShutdownController::new();

// Rastrear conexões ativas com guards RAII
let _guard = shutdown.connection_guard();
// Conexão é automaticamente decrementada quando guard é dropado

// Aguardar sinal de shutdown
shutdown_signal().await;

// Drenar conexões com timeout
shutdown.wait_for_drain(Duration::from_secs(30)).await;
```

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_SHUTDOWN_TIMEOUT_SECS` | `30` | Tempo máximo para aguardar drenagem de conexões |

### Como Funciona

![Fluxo de Graceful Shutdown](/img/graceful-shutdown.svg)

1. Sinal recebido (SIGTERM/Ctrl+C)
2. Para de aceitar novas conexões
3. Aguarda conexões ativas completarem (até timeout)
4. Força fechamento das conexões restantes
5. Sai de forma limpa

---

## Rate Limiting

Rate limiting por token bucket por IP do cliente para prevenir abuso.

```rust
use edgeproxy::infrastructure::{RateLimiter, RateLimitConfig};

let limiter = RateLimiter::new(RateLimitConfig {
    max_requests: 100,        // Requisições por janela
    window: Duration::from_secs(1),
    burst_size: 10,           // Burst inicial permitido
});

// Verificar se requisição é permitida
if limiter.check(client_ip) {
    // Processar requisição
} else {
    // Retornar 429 Too Many Requests
}

// Verificar tokens restantes
let remaining = limiter.remaining(client_ip);
```

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_RATE_LIMIT_ENABLED` | `false` | Habilitar rate limiting |
| `EDGEPROXY_RATE_LIMIT_MAX_REQUESTS` | `100` | Máximo de requisições por janela |
| `EDGEPROXY_RATE_LIMIT_WINDOW_SECS` | `1` | Janela de tempo em segundos |
| `EDGEPROXY_RATE_LIMIT_BURST` | `10` | Tamanho do burst (token bucket) |

### Algoritmo

```
Algoritmo Token Bucket:
- Cada cliente começa com `burst_size` tokens
- Tokens são reabastecidos na taxa `max_requests / window`
- Cada requisição consome 1 token
- Requisição negada se não houver tokens disponíveis
```

---

## Circuit Breaker

Previne falhas em cascata bloqueando temporariamente requisições para backends com falha.

```rust
use edgeproxy::infrastructure::{CircuitBreaker, CircuitBreakerConfig};

let breaker = CircuitBreaker::new(CircuitBreakerConfig {
    failure_threshold: 5,     // Falhas antes de abrir
    success_threshold: 3,     // Sucessos para fechar
    timeout: Duration::from_secs(30),  // Tempo no estado aberto
});

// Verificar se requisição é permitida
if breaker.allow() {
    match backend_request().await {
        Ok(_) => breaker.record_success(),
        Err(_) => breaker.record_failure(),
    }
} else {
    // Circuito está aberto, falha rápida
}
```

### Estados

| Estado | Descrição | Comportamento |
|--------|-----------|---------------|
| **Closed** | Operação normal | Todas requisições passam |
| **Open** | Backend falhando | Todas requisições falham rápido |
| **Half-Open** | Testando recuperação | Requisições limitadas para testar backend |

### Transições de Estado

```
         falhas >= threshold
Closed ─────────────────────────► Open
   ▲                                │
   │ sucessos >= threshold          │ timeout expira
   │                                ▼
   └──────────────────────────── Half-Open
                                    │
                                    │ falha
                                    ▼
                                  Open
```

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_CIRCUIT_BREAKER_ENABLED` | `false` | Habilitar circuit breaker |
| `EDGEPROXY_CIRCUIT_FAILURE_THRESHOLD` | `5` | Falhas para abrir circuito |
| `EDGEPROXY_CIRCUIT_SUCCESS_THRESHOLD` | `3` | Sucessos para fechar circuito |
| `EDGEPROXY_CIRCUIT_TIMEOUT_SECS` | `30` | Timeout no estado aberto |

---

## Health Checks Ativos

Monitora proativamente a saúde dos backends com probes TCP ou HTTP.

```rust
use edgeproxy::infrastructure::{HealthChecker, HealthCheckConfig, HealthCheckType};

let checker = HealthChecker::new(
    "backend-1".to_string(),
    "10.50.1.1:8080".to_string(),
    HealthCheckConfig {
        check_type: HealthCheckType::Http {
            path: "/health".to_string(),
            expected_status: 200,
        },
        interval: Duration::from_secs(5),
        timeout: Duration::from_secs(2),
        healthy_threshold: 2,
        unhealthy_threshold: 3,
    },
);

// Iniciar health checks em background
checker.start();

// Obter status atual
let status = checker.status();
println!("Backend saudável: {}", status.is_healthy);
```

### Tipos de Check

| Tipo | Descrição | Caso de Uso |
|------|-----------|-------------|
| **TCP** | Verificação simples de conexão | Conectividade básica |
| **HTTP** | HTTP GET com verificação de status | Saúde da aplicação |

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_HEALTH_CHECK_ENABLED` | `false` | Habilitar health checks ativos |
| `EDGEPROXY_HEALTH_CHECK_INTERVAL_SECS` | `5` | Intervalo de check |
| `EDGEPROXY_HEALTH_CHECK_TIMEOUT_SECS` | `2` | Timeout do check |
| `EDGEPROXY_HEALTH_CHECK_TYPE` | `tcp` | Tipo de check: `tcp` ou `http` |
| `EDGEPROXY_HEALTH_CHECK_PATH` | `/health` | Path do check HTTP |
| `EDGEPROXY_HEALTH_HEALTHY_THRESHOLD` | `2` | Sucessos para se tornar saudável |
| `EDGEPROXY_HEALTH_UNHEALTHY_THRESHOLD` | `3` | Falhas para se tornar não saudável |

---

## Connection Pooling

Reutiliza conexões TCP para backends para melhorar performance.

```rust
use edgeproxy::infrastructure::{ConnectionPool, PoolConfig};

let pool = ConnectionPool::new(PoolConfig {
    max_connections_per_backend: 10,
    idle_timeout: Duration::from_secs(60),
    max_lifetime: Duration::from_secs(300),
    connect_timeout: Duration::from_secs(5),
});

// Adquirir uma conexão (reutiliza existente ou cria nova)
let conn = pool.acquire("backend-1", "10.50.1.1:8080").await?;

// Usar conexão...
// Conexão é retornada ao pool quando dropada
```

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_POOL_ENABLED` | `false` | Habilitar connection pooling |
| `EDGEPROXY_POOL_MAX_PER_BACKEND` | `10` | Máximo de conexões por backend |
| `EDGEPROXY_POOL_IDLE_TIMEOUT_SECS` | `60` | Timeout de conexão ociosa |
| `EDGEPROXY_POOL_MAX_LIFETIME_SECS` | `300` | Tempo máximo de vida da conexão |
| `EDGEPROXY_POOL_CONNECT_TIMEOUT_SECS` | `5` | Timeout de conexão |

### Benefícios

- Latência reduzida (sem handshake TCP)
- Carga reduzida no backend (menos conexões)
- Melhor utilização de recursos

---

## Métricas Prometheus

Exporta métricas em formato Prometheus para monitoramento e alertas.

```rust
use edgeproxy::adapters::outbound::PrometheusMetricsStore;

let metrics = PrometheusMetricsStore::new();

// Registrar conexão
metrics.record_connection("backend-1");

// Registrar bytes
metrics.record_bytes_sent("backend-1", 1024);
metrics.record_bytes_received("backend-1", 2048);

// Exportar formato Prometheus
let output = metrics.export_prometheus();
```

### Métricas Expostas

| Métrica | Tipo | Descrição |
|---------|------|-----------|
| `edgeproxy_connections_total` | Counter | Total de conexões |
| `edgeproxy_connections_active` | Gauge | Conexões ativas |
| `edgeproxy_bytes_sent_total` | Counter | Total de bytes enviados |
| `edgeproxy_bytes_received_total` | Counter | Total de bytes recebidos |
| `edgeproxy_backend_connections_total` | Counter | Conexões por backend |
| `edgeproxy_backend_connections_active` | Gauge | Conexões ativas por backend |
| `edgeproxy_backend_errors_total` | Counter | Erros por backend |
| `edgeproxy_backend_rtt_seconds` | Histogram | RTT por backend |

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_METRICS_ENABLED` | `false` | Habilitar métricas Prometheus |
| `EDGEPROXY_METRICS_LISTEN_ADDR` | `0.0.0.0:9090` | Endereço do endpoint de métricas |
| `EDGEPROXY_METRICS_PATH` | `/metrics` | Path do endpoint de métricas |

### Config de Scrape Prometheus

```yaml
scrape_configs:
  - job_name: 'edgeproxy'
    static_configs:
      - targets: ['edgeproxy:9090']
    metrics_path: '/metrics'
```

---

## Hot Reload de Configuração

Monitora arquivos de configuração por mudanças e recarrega sem reiniciar.

```rust
use edgeproxy::infrastructure::{ConfigWatcher, ConfigChange};

let watcher = ConfigWatcher::new(Duration::from_secs(5));

// Monitorar um arquivo de configuração
watcher.watch_file("/etc/edgeproxy/config.toml").await?;

// Se inscrever para mudanças
let mut rx = watcher.subscribe();

// Reagir a mudanças
tokio::spawn(async move {
    while let Ok(change) = rx.recv().await {
        match change {
            ConfigChange::FileModified(path) => {
                println!("Arquivo de config alterado: {:?}", path);
                // Recarregar configuração
            }
            ConfigChange::ValueChanged { key, new_value, .. } => {
                println!("Config {} alterada para {}", key, new_value);
            }
            ConfigChange::FullReload => {
                println!("Reload completo de config");
            }
        }
    }
});
```

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_CONFIG_WATCH_ENABLED` | `false` | Habilitar monitoramento de arquivo de config |
| `EDGEPROXY_CONFIG_WATCH_INTERVAL_SECS` | `5` | Intervalo de verificação de arquivo |

### Configurações Recarregáveis

O seguinte pode ser alterado sem reiniciar:

- Pesos e limites de backends
- Parâmetros de health check
- Thresholds de rate limit
- Configurações de circuit breaker

---

## Repositório PostgreSQL de Backends

Armazenamento de backends pronto para produção usando PostgreSQL.

```rust
use edgeproxy::adapters::outbound::{PostgresBackendRepository, PostgresConfig};

let repo = PostgresBackendRepository::new(PostgresConfig {
    url: "postgres://user:pass@localhost:5432/edgeproxy".to_string(),
    max_connections: 10,
    min_connections: 2,
    connect_timeout: Duration::from_secs(5),
    query_timeout: Duration::from_secs(10),
    reload_interval: Duration::from_secs(5),
});

// Inicializar (cria tabelas se necessário)
repo.initialize().await?;

// Iniciar sync em background
repo.start_sync();

// Usar como trait BackendRepository
let backends = repo.get_healthy().await;
```

### Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_POSTGRES_ENABLED` | `false` | Usar PostgreSQL para backends |
| `EDGEPROXY_POSTGRES_URL` | *(obrigatório)* | URL de conexão PostgreSQL |
| `EDGEPROXY_POSTGRES_MAX_CONNECTIONS` | `10` | Máximo de conexões no pool |
| `EDGEPROXY_POSTGRES_MIN_CONNECTIONS` | `2` | Mínimo de conexões no pool |
| `EDGEPROXY_POSTGRES_CONNECT_TIMEOUT_SECS` | `5` | Timeout de conexão |
| `EDGEPROXY_POSTGRES_QUERY_TIMEOUT_SECS` | `10` | Timeout de query |
| `EDGEPROXY_POSTGRES_RELOAD_SECS` | `5` | Intervalo de reload do cache |

### Schema

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,
    app TEXT NOT NULL,
    region TEXT NOT NULL,
    country TEXT NOT NULL,
    wg_ip TEXT NOT NULL,
    port INTEGER NOT NULL,
    healthy INTEGER NOT NULL DEFAULT 1,
    weight INTEGER NOT NULL DEFAULT 1,
    soft_limit INTEGER NOT NULL DEFAULT 100,
    hard_limit INTEGER NOT NULL DEFAULT 150,
    deleted INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_backends_healthy ON backends(healthy) WHERE deleted = 0;
CREATE INDEX idx_backends_region ON backends(region) WHERE deleted = 0;
```

---

## Exemplo de Configuração de Produção

Exemplo completo com todos os componentes de infraestrutura habilitados:

```bash
# Core
export EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080"
export EDGEPROXY_REGION="sa"

# PostgreSQL Backend
export EDGEPROXY_POSTGRES_ENABLED=true
export EDGEPROXY_POSTGRES_URL="postgres://edgeproxy:secret@postgres:5432/edgeproxy"

# TLS
export EDGEPROXY_TLS_ENABLED=true
export EDGEPROXY_TLS_LISTEN_ADDR="0.0.0.0:8443"
export EDGEPROXY_TLS_CERT="/etc/ssl/edgeproxy.crt"
export EDGEPROXY_TLS_KEY="/etc/ssl/edgeproxy.key"

# API
export EDGEPROXY_API_ENABLED=true
export EDGEPROXY_API_LISTEN_ADDR="0.0.0.0:8081"

# Rate Limiting
export EDGEPROXY_RATE_LIMIT_ENABLED=true
export EDGEPROXY_RATE_LIMIT_MAX_REQUESTS=1000
export EDGEPROXY_RATE_LIMIT_BURST=50

# Circuit Breaker
export EDGEPROXY_CIRCUIT_BREAKER_ENABLED=true
export EDGEPROXY_CIRCUIT_FAILURE_THRESHOLD=5
export EDGEPROXY_CIRCUIT_TIMEOUT_SECS=30

# Health Checks
export EDGEPROXY_HEALTH_CHECK_ENABLED=true
export EDGEPROXY_HEALTH_CHECK_TYPE=http
export EDGEPROXY_HEALTH_CHECK_PATH=/health

# Connection Pooling
export EDGEPROXY_POOL_ENABLED=true
export EDGEPROXY_POOL_MAX_PER_BACKEND=20

# Prometheus Metrics
export EDGEPROXY_METRICS_ENABLED=true
export EDGEPROXY_METRICS_LISTEN_ADDR="0.0.0.0:9090"

# Graceful Shutdown
export EDGEPROXY_SHUTDOWN_TIMEOUT_SECS=60

./edge-proxy
```
