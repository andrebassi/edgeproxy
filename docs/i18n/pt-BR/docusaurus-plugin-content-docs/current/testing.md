---
sidebar_position: 12
---

# Testes

Este guia cobre como testar o edgeProxy localmente e em ambientes de deploy usando o servidor mock backend.

## Mock Backend Server

O diretório `tests/mock-backend/` contém um servidor HTTP leve em Go que simula serviços backend reais para fins de teste.

### Funcionalidades

- **Simulação multi-região**: Configure diferentes regiões por instância
- **Rastreamento de requests**: Conta requisições por backend
- **Múltiplos endpoints**: Root, health, info e latency
- **Respostas JSON**: Respostas estruturadas para fácil parsing
- **Footprint mínimo**: ~8MB de binário, baixo uso de memória

### Compilando o Mock Server

```bash
# Build nativo (para testes locais)
cd tests/mock-backend
go build -o mock-backend main.go

# Cross-compile para Linux AMD64 (para deploy EC2/cloud)
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go
```

### Executando Localmente

Inicie múltiplas instâncias para simular diferentes backends:

```bash
# Terminal 1: Backend EU 1
./mock-backend -port 9001 -region eu -id mock-eu-1

# Terminal 2: Backend EU 2
./mock-backend -port 9002 -region eu -id mock-eu-2

# Terminal 3: Backend US
./mock-backend -port 9003 -region us -id mock-us-1
```

### Opções CLI

| Flag | Padrão | Descrição |
|------|--------|-----------|
| `-port` | `9001` | Porta TCP para escutar |
| `-region` | `eu` | Identificador de região (eu, us, sa, ap) |
| `-id` | `mock-{region}-{port}` | Identificador único do backend |

### Endpoints

| Endpoint | Descrição | Resposta |
|----------|-----------|----------|
| `/` | Root | Texto com info do backend |
| `/health` | Health check | `OK - {id} ({region})` |
| `/api/info` | Info JSON | Detalhes completos do backend |
| `/api/latency` | JSON mínimo | Para testes de latência |

### Exemplo de Resposta (`/api/info`)

```json
{
  "backend_id": "mock-eu-1",
  "region": "eu",
  "hostname": "ip-172-31-29-183",
  "port": "9001",
  "request_count": 42,
  "uptime_secs": 3600,
  "timestamp": "2025-12-08T00:11:43Z",
  "message": "Hello from mock backend!"
}
```

## Setup de Teste Local

### 1. Configurar routing.db

Adicione mock backends ao seu routing.db local:

```sql
-- Limpar backends de teste existentes
DELETE FROM backends WHERE id LIKE 'mock-%';

-- Adicionar mock backends
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150),
  ('mock-eu-2', 'test', 'eu', '127.0.0.1', 9002, 1, 2, 100, 150),
  ('mock-us-1', 'test', 'us', '127.0.0.1', 9003, 1, 2, 100, 150);
```

### 2. Iniciar Mock Backends

```bash
# Iniciar todos os 3 backends
./tests/mock-backend/mock-backend -port 9001 -region eu -id mock-eu-1 &
./tests/mock-backend/mock-backend -port 9002 -region eu -id mock-eu-2 &
./tests/mock-backend/mock-backend -port 9003 -region us -id mock-us-1 &
```

### 3. Executar edgeProxy

```bash
EDGEPROXY_REGION=eu \
EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080 \
cargo run --release
```

### 4. Testar Requisições

```bash
# Teste simples
curl http://localhost:8080/api/info

# Múltiplas requisições (observe o load balancing)
for i in {1..10}; do
  curl -s http://localhost:8080/api/info | grep backend_id
done

# Health check
curl http://localhost:8080/health
```

## Teste de Deploy EC2

### 1. Deploy do Mock Server para EC2

```bash
# Build para Linux
cd tests/mock-backend
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go

# Copiar para EC2
scp -i ~/.ssh/edgeproxy-key.pem mock-backend-linux-amd64 ubuntu@<EC2-IP>:/tmp/

# SSH e setup
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@<EC2-IP>
sudo mv /tmp/mock-backend-linux-amd64 /opt/edgeproxy/mock-backend
sudo chmod +x /opt/edgeproxy/mock-backend
```

### 2. Iniciar Mock Backends na EC2

```bash
# Iniciar 3 instâncias
cd /opt/edgeproxy
nohup ./mock-backend -port 9001 -region eu -id mock-eu-1 > /tmp/mock-9001.log 2>&1 &
nohup ./mock-backend -port 9002 -region eu -id mock-eu-2 > /tmp/mock-9002.log 2>&1 &
nohup ./mock-backend -port 9003 -region us -id mock-us-1 > /tmp/mock-9003.log 2>&1 &

# Verificar
ps aux | grep mock-backend
curl localhost:9001/health
curl localhost:9002/health
curl localhost:9003/health
```

### 3. Configurar routing.db na EC2

```bash
sqlite3 /opt/edgeproxy/routing.db "
DELETE FROM backends WHERE id LIKE 'mock-%';
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150),
  ('mock-eu-2', 'test', 'eu', '127.0.0.1', 9002, 1, 2, 100, 150),
  ('mock-us-1', 'test', 'us', '127.0.0.1', 9003, 1, 2, 100, 150);
SELECT id, region, port, healthy FROM backends WHERE deleted=0;
"
```

#### Campos do Backend Explicados

| Campo | Tipo | Descrição | Exemplo |
|-------|------|-----------|---------|
| `id` | TEXT | Identificador único do backend. Usado em logs e client affinity. | `mock-eu-1` |
| `app` | TEXT | Nome da aplicação. Agrupa backends que servem a mesma app. | `test` |
| `region` | TEXT | Código da região geográfica. Usado para decisões de geo-routing. Válidos: `eu`, `us`, `sa`, `ap`. | `eu` |
| `wg_ip` | TEXT | Endereço IP do backend. Use `127.0.0.1` para testes locais, IPs WireGuard (10.50.x.x) em produção. | `127.0.0.1` |
| `port` | INTEGER | Porta TCP que o backend escuta. | `9001` |
| `healthy` | INTEGER | Status de saúde. `1` = saudável (recebe tráfego), `0` = não saudável (excluído do roteamento). | `1` |
| `weight` | INTEGER | Peso relativo para load balancing. Peso maior = mais tráfego. Range: 1-10. | `2` |
| `soft_limit` | INTEGER | Quantidade confortável de conexões. Acima disso, o backend é considerado "carregado" e menos preferido. | `100` |
| `hard_limit` | INTEGER | Máximo de conexões. Neste limite ou acima, backend é excluído de novas conexões. | `150` |

#### Detalhamento dos Dados de Exemplo

```sql
('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150)
```

| Valor | Campo | Significado |
|-------|-------|-------------|
| `mock-eu-1` | id | Identificador do backend, primeiro mock server EU |
| `test` | app | Nome da aplicação para testes |
| `eu` | region | Localizado na região Europa |
| `127.0.0.1` | wg_ip | Localhost (mesma máquina que o proxy) |
| `9001` | port | Escutando na porta 9001 |
| `1` | healthy | Backend está saudável e ativo |
| `2` | weight | Prioridade média (escala 1-10) |
| `100` | soft_limit | Confortável com até 100 conexões |
| `150` | hard_limit | Máximo de 150 conexões permitidas |

#### Scoring do Load Balancer

O proxy usa esses campos para calcular um score para cada backend:

```
score = geo_score * 100 + (conexões / soft_limit) / weight
```

- **geo_score**: 0 (mesmo país), 1 (mesma região), 2 (região do POP local), 3 (fallback global)
- **conexões**: Conexões ativas atuais (do metrics)
- **soft_limit**: Divide o fator de carga
- **weight**: Peso maior reduz o score (mais preferido)

**Menor score vence.** Backends com `healthy=0` ou no `hard_limit` são excluídos.

### 4. Testar de Cliente Externo

```bash
# Da sua máquina local
curl http://<EC2-PUBLIC-IP>:8080/api/info
curl http://<EC2-PUBLIC-IP>:8080/health

# Múltiplas requisições para ver load balancing
for i in {1..5}; do
  curl -s http://<EC2-PUBLIC-IP>:8080/api/info
  echo ""
done
```

## Cenários de Teste

### Client Affinity

Client affinity (sticky sessions) vincula clientes ao mesmo backend:

```bash
# Todas requisições do mesmo IP vão para o mesmo backend
for i in {1..5}; do
  curl -s http://localhost:8080/api/info | grep backend_id
done
# Esperado: Todos mostram o mesmo backend_id
```

### Distribuição de Carga

Para testar distribuição de carga, simule diferentes clientes:

```bash
# Use IPs de origem diferentes ou aguarde expiração do TTL
# Verifique request_count em cada backend
curl localhost:9001/api/info | grep request_count
curl localhost:9002/api/info | grep request_count
curl localhost:9003/api/info | grep request_count
```

### Health do Backend

Teste roteamento baseado em health parando um backend:

```bash
# Parar mock-eu-1
pkill -f 'mock-backend.*9001'

# Requisições devem ir para backends saudáveis
curl http://localhost:8080/api/info
# Esperado: Roteia para mock-eu-2 ou mock-us-1
```

### Geo-Routing

O proxy roteia clientes para backends em sua região:

1. Configure backends em múltiplas regiões
2. Teste de diferentes localizações geográficas
3. Observe decisões de roteamento nos logs do proxy

## Monitoramento Durante Testes

### Logs do edgeProxy

```bash
# Na EC2
sudo journalctl -u edgeproxy -f

# Procure por:
# - Logs de seleção de backend
# - Contagem de conexões
# - Resolução GeoIP
```

### Logs do Mock Backend

```bash
# Verificar logs individuais dos backends
tail -f /tmp/mock-9001.log
tail -f /tmp/mock-9002.log
tail -f /tmp/mock-9003.log
```

### Distribuição de Requisições

```bash
# Verificação rápida de distribuição
echo "mock-eu-1: $(curl -s localhost:9001/api/info | grep -o '"request_count":[0-9]*')"
echo "mock-eu-2: $(curl -s localhost:9002/api/info | grep -o '"request_count":[0-9]*')"
echo "mock-us-1: $(curl -s localhost:9003/api/info | grep -o '"request_count":[0-9]*')"
```

## Limpeza

### Local

```bash
# Matar todos os mock backends
pkill -f mock-backend
```

### EC2

```bash
# Matar mock backends
sudo pkill -f mock-backend

# Ou matar por porta
sudo fuser -k 9001/tcp 9002/tcp 9003/tcp
```

## Troubleshooting

### Mock Backend Não Inicia

```bash
# Verificar se porta está em uso
sudo ss -tlnp | grep 9001

# Matar processo existente
sudo fuser -k 9001/tcp
```

### Proxy Não Conecta ao Backend

1. Verificar se backend está rodando: `curl localhost:9001/health`
2. Verificar configuração do routing.db
3. Verificar se `wg_ip` está correto (use `127.0.0.1` para testes locais)
4. Verificar regras de firewall na EC2

### Requisições com Timeout

1. Verificar se edgeProxy está rodando: `sudo systemctl status edgeproxy`
2. Verificar health dos backends no routing.db
3. Verificar se limites de conexão não foram excedidos

---

## Testes Unitários

O edgeProxy possui cobertura abrangente de testes unitários seguindo o padrão de Arquitetura Hexagonal. Todos os testes são escritos em Rust usando o framework de testes nativo.

### Resumo dos Testes

| Métrica | Valor |
|---------|-------|
| **Total de Testes** | 875 |
| **Cobertura de Linhas** | **98.71%** |
| **Cobertura de Regiões** | 98.71% |
| **Cobertura de Funções** | 99.58% |
| **Arquivos com 100%** | 22 |

### Evolução da Cobertura

O projeto alcançou melhorias significativas de cobertura através de testes sistemáticos:

| Fase | Cobertura | Testes | Melhorias Principais |
|------|-----------|--------|---------------------|
| Inicial (stable) | 94.43% | 780 | Testes unitários básicos |
| Refatoração | 94.92% | 782 | Adoção do padrão Sans-IO |
| Build nightly | 98.32% | 782 | `coverage(off)` para I/O |
| Testes edge case | 98.50% | 784 | Circuit breaker, métricas |
| TLS & pool | 98.89% | 786 | TLS, connection pool |
| **Replicação v0.4.0** | **98.71%** | **875** | Merkle tree, mDNS, delta sync |

### Benefícios da Arquitetura Sans-IO

O padrão Sans-IO separa lógica de negócio pura das operações de I/O:

```
┌─────────────────────────────────────────────────────────────────────┐
│                     TESTÁVEL (100% coberto)                          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Funções Puras: process_message(), pick_backend(), etc.      │  │
│  │  - Sem chamadas de rede                                       │  │
│  │  - Sem acesso a banco de dados                                │  │
│  │  - Retorna ações para executar                                │  │
│  └──────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────┤
│                     WRAPPERS DE I/O (excluídos)                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Async handlers: start(), run(), handle_connection()         │  │
│  │  - Marcados com #[cfg_attr(coverage_nightly, coverage(off))] │  │
│  │  - Wrappers finos que executam ações                         │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

Esta abordagem garante:
- **Toda lógica de negócio é testável** sem mock de rede
- **100% de cobertura do código de decisão**
- **Separação clara** entre lógica e I/O

### Executando Testes

```bash
# Executar todos os testes
cargo test

# Executar testes com output
cargo test -- --nocapture

# Executar testes de um módulo específico
cargo test domain::services::load_balancer

# Executar apenas testes de infraestrutura
cargo test infrastructure::

# Executar testes em paralelo (padrão)
cargo test -- --test-threads=4

# Executar single-threaded (para debug)
cargo test -- --test-threads=1
```

### Testes por Módulo

#### Adapters Inbound

| Módulo | Testes | Cobertura | Descrição |
|--------|--------|-----------|-----------|
| `adapters::inbound::api_server` | 38 | 99.57% | API de Auto-Discovery, registro, heartbeat |
| `adapters::inbound::dns_server` | 44 | 97.80% | Servidor DNS, resolução geo-routing |
| `adapters::inbound::tcp_server` | 27 | 96.23% | Conexões TCP, lógica de proxy |
| `adapters::inbound::tls_server` | 29 | 94.18% | Terminação TLS, certificados |

#### Adapters Outbound

| Módulo | Testes | Cobertura | Descrição |
|--------|--------|-----------|-----------|
| `adapters::outbound::dashmap_metrics_store` | 20 | 100.00% | Métricas de conexão, tracking RTT |
| `adapters::outbound::dashmap_binding_repo` | 21 | 100.00% | Client affinity, TTL, GC |
| `adapters::outbound::replication_backend_repo` | 28 | 99.85% | Replicação SQLite distribuída |
| `adapters::outbound::sqlite_backend_repo` | 20 | 99.26% | Storage SQLite de backends |
| `adapters::outbound::prometheus_metrics_store` | 19 | 98.70% | Exportação métricas Prometheus |
| `adapters::outbound::maxmind_geo_resolver` | 18 | 95.86% | Resolução GeoIP |
| `adapters::outbound::postgres_backend_repo` | 19 | 88.31% | Backend PostgreSQL (stub) |

#### Camada de Domínio

| Módulo | Testes | Cobertura | Descrição |
|--------|--------|-----------|-----------|
| `domain::entities` | 12 | 100.00% | Backend, Binding, ClientKey |
| `domain::value_objects` | 26 | 96.40% | RegionCode, mapeamento de países |
| `domain::services::load_balancer` | 25 | 98.78% | Algoritmo de scoring, geo-routing |

#### Camada de Aplicação

| Módulo | Testes | Cobertura | Descrição |
|--------|--------|-----------|-----------|
| `application::proxy_service` | 26 | 99.43% | Orquestração de use cases |
| `config` | 24 | 100.00% | Carregamento de configuração |

#### Camada de Infraestrutura

| Módulo | Testes | Cobertura | Descrição |
|--------|--------|-----------|-----------|
| `infrastructure::circuit_breaker` | 22 | 98.30% | Padrão circuit breaker |
| `infrastructure::config_watcher` | 17 | 95.30% | Hot reload de configuração |
| `infrastructure::rate_limiter` | 14 | 93.55% | Rate limiting token bucket |
| `infrastructure::health_checker` | 17 | 92.00% | Health checks ativos |
| `infrastructure::connection_pool` | 17 | 93.71% | Pool de conexões TCP |
| `infrastructure::shutdown` | 11 | 93.65% | Graceful shutdown |

#### Camada de Replicação (v0.4.0)

| Módulo | Testes | Cobertura | Descrição |
|--------|--------|-----------|-----------|
| `replication::types` | 45 | 98.81% | Timestamps HLC, ChangeSet, NodeId |
| `replication::config` | 12 | 99.34% | Configuração de replicação |
| `replication::sync` | 38 | 97.63% | Detecção de mudanças, resolução LWW |
| `replication::gossip` | 42 | 98.80% | Protocolo SWIM, membership do cluster |
| `replication::transport` | 48 | 98.55% | Transporte QUIC, encoding Sans-IO |
| `replication::agent` | 18 | 99.77% | Orquestração de replicação |
| `replication::merkle` | 40 | 98.92% | Anti-entropia Merkle tree |
| `replication::mdns` | 25 | 99.02% | Auto-descoberta mDNS |

### Testes por Camada (Arquitetura Hexagonal)

![Testes por Camada](/img/tests-by-layer.svg)

### Detalhes dos Testes de Infraestrutura

#### Testes do Circuit Breaker (22 testes)

```bash
cargo test infrastructure::circuit_breaker
```

| Teste | Descrição |
|-------|-----------|
| `test_circuit_breaker_new` | Estado inicial é Closed |
| `test_circuit_breaker_default` | Configuração padrão |
| `test_allow_when_closed` | Requisições passam no estado Closed |
| `test_record_success_in_closed` | Rastreamento de sucesso |
| `test_record_failure_in_closed` | Rastreamento de falha |
| `test_transitions_to_open` | Abre após threshold de falhas |
| `test_deny_when_open` | Bloqueia requisições no estado Open |
| `test_circuit_transitions_to_half_open` | Timeout dispara Half-Open |
| `test_half_open_allows_limited` | Requisições limitadas em Half-Open |
| `test_half_open_to_closed` | Recupera para Closed em sucesso |
| `test_half_open_to_open` | Retorna para Open em falha |
| `test_failure_window_resets` | Window reseta em sucesso |
| `test_get_metrics` | Recuperação de métricas |
| `test_concurrent_record` | Operações thread-safe |

#### Testes do Rate Limiter (14 testes)

```bash
cargo test infrastructure::rate_limiter
```

| Teste | Descrição |
|-------|-----------|
| `test_rate_limit_config_default` | Padrão: 100 req/s, burst 10 |
| `test_rate_limiter_new` | Cria com configuração |
| `test_check_allows_initial_burst` | Burst de requisições permitido |
| `test_check_different_clients_isolated` | Isolamento por IP |
| `test_remaining` | Rastreamento de tokens |
| `test_clear_client` | Reset de cliente individual |
| `test_clear_all` | Reset de todos clientes |
| `test_check_with_cost` | Requisições com custo variável |
| `test_cleanup_removes_stale` | GC remove entradas antigas |
| `test_refill_over_time` | Reposição de tokens |
| `test_concurrent_access` | Operações thread-safe |

#### Testes do Health Checker (17 testes)

```bash
cargo test infrastructure::health_checker
```

| Teste | Descrição |
|-------|-----------|
| `test_health_checker_new` | Cria com configuração |
| `test_health_check_config_default` | Intervalos padrão |
| `test_health_status_default` | Estado inicial desconhecido |
| `test_tcp_check_success` | Probe TCP sucesso |
| `test_tcp_check_failure` | Probe TCP falha |
| `test_tcp_check_timeout` | Tratamento de timeout TCP |
| `test_update_status_becomes_healthy` | Transições de threshold |
| `test_update_status_becomes_unhealthy` | Transições de falha |
| `test_on_health_change_callback` | Notificações de mudança |
| `test_check_backend_success` | Check de backend OK |
| `test_check_backend_failure` | Check de backend falha |

#### Testes do Connection Pool (17 testes)

```bash
cargo test infrastructure::connection_pool
```

| Teste | Descrição |
|-------|-----------|
| `test_connection_pool_new` | Criação do pool |
| `test_pool_config_default` | Padrão: 10 max, 60s idle |
| `test_acquire_creates_connection` | Nova conexão em pool vazio |
| `test_release_returns_connection` | Reutilização de conexão |
| `test_pool_exhausted` | Erro de máximo de conexões |
| `test_acquire_timeout` | Timeout de conexão |
| `test_discard_closes_connection` | Descarte explícito |
| `test_stats` | Estatísticas do pool |
| `test_pooled_connection_is_expired` | Verificação de lifetime |
| `test_pooled_connection_is_idle_expired` | Verificação de idle timeout |

#### Testes do Graceful Shutdown (11 testes)

```bash
cargo test infrastructure::shutdown
```

| Teste | Descrição |
|-------|-----------|
| `test_shutdown_controller_new` | Criação do controller |
| `test_connection_guard` | Criação de guard RAII |
| `test_connection_tracking` | Rastreamento de contagem ativa |
| `test_multiple_connection_guards` | Guards concorrentes |
| `test_shutdown_initiates_once` | Shutdown único |
| `test_subscribe_receives_shutdown` | Notificação broadcast |
| `test_wait_for_drain_immediate` | Caso sem conexões |
| `test_wait_for_drain_with_connections` | Aguarda drenagem |
| `test_wait_for_drain_timeout` | Comportamento de timeout |

#### Testes do Config Watcher (17 testes)

```bash
cargo test infrastructure::config_watcher
```

| Teste | Descrição |
|-------|-----------|
| `test_config_watcher_new` | Criação do watcher |
| `test_watch_file` | Monitoramento de arquivo |
| `test_watch_nonexistent_file` | Tratamento de erro |
| `test_unwatch_file` | Remoção do watch |
| `test_set_and_get` | Valores de config |
| `test_get_or` | Valores padrão |
| `test_subscribe_value_change` | Notificações de mudança |
| `test_no_change_on_same_value` | Sem eventos espúrios |
| `test_check_files_detects_modification` | Detecção de mudança |
| `test_hot_value_get_set` | Wrapper HotValue |

---

## Cobertura de Código

### Ferramentas de Cobertura

O edgeProxy usa [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) para medição de cobertura de código com instrumentação LLVM.

### Instalação

```bash
# Instalar cargo-llvm-cov
cargo install cargo-llvm-cov

# Instalar ferramentas LLVM (necessário para cobertura)
rustup component add llvm-tools-preview

# Instalar toolchain nightly (para suporte a coverage(off))
rustup toolchain install nightly
rustup run nightly rustup component add llvm-tools-preview
```

### Executando Cobertura

```bash
# Relatório básico de cobertura (stable Rust - inclui código I/O)
cargo llvm-cov

# Cobertura com nightly (RECOMENDADO - exclui código I/O marcado com coverage(off))
rustup run nightly cargo llvm-cov

# Apenas resumo
rustup run nightly cargo llvm-cov --summary-only

# Cobertura com relatório HTML
rustup run nightly cargo llvm-cov --html

# Cobertura com output LCOV
rustup run nightly cargo llvm-cov --lcov --output-path lcov.info

# Abrir relatório HTML
open target/llvm-cov/html/index.html
```

> **Importante**: Use `rustup run nightly` para habilitar atributos `#[coverage(off)]`. Com Rust stable, código de I/O será incluído nas métricas de cobertura, resultando em ~94% ao invés de ~99%.

### Resultados de Cobertura

**Cobertura Final: 98.71%** (7.159 linhas, 98.98% cobertura de linhas)

> **Nota**: Cobertura medida com `cargo +nightly llvm-cov` para habilitar atributos `coverage(off)` em código de I/O.

#### Cobertura por Camada

| Camada | Regiões | Cobertura | Status |
|--------|---------|-----------|--------|
| **Domínio** | 761 | 99.47% | ✓ Excelente |
| **Aplicação** | 706 | 99.72% | ✓ Excelente |
| **Adapters Inbound** | 2.100 | 98.90% | ✓ Excelente |
| **Adapters Outbound** | 1.450 | 98.62% | ✓ Excelente |
| **Infraestrutura** | 455 | 95.30% | ✓ Muito Bom |
| **Replicação** | 7.159 | 98.98% | ✓ Excelente |
| **Config** | 286 | 100.00% | ✓ Completo |

#### Cobertura Detalhada por Arquivo

##### Componentes Core (100% Cobertura)

| Arquivo | Linhas | Cobertura |
|---------|--------|-----------|
| `config.rs` | 286 | 100.00% |
| `domain/entities.rs` | 130 | 100.00% |
| `adapters/outbound/dashmap_metrics_store.rs` | 224 | 100.00% |
| `adapters/outbound/dashmap_binding_repo.rs` | 287 | 100.00% |

##### Adapters Inbound

| Arquivo | Linhas | Cobertas | Cobertura |
|---------|--------|----------|-----------|
| `adapters/inbound/api_server.rs` | 928 | 924 | 99.57% |
| `adapters/inbound/dns_server.rs` | 774 | 757 | 97.80% |
| `adapters/inbound/tcp_server.rs` | 849 | 817 | 96.23% |
| `adapters/inbound/tls_server.rs` | 996 | 938 | 94.18% |

##### Adapters Outbound

| Arquivo | Linhas | Cobertas | Cobertura |
|---------|--------|----------|-----------|
| `adapters/outbound/replication_backend_repo.rs` | 677 | 676 | 99.85% |
| `adapters/outbound/sqlite_backend_repo.rs` | 404 | 401 | 99.26% |
| `adapters/outbound/prometheus_metrics_store.rs` | 307 | 303 | 98.70% |
| `adapters/outbound/maxmind_geo_resolver.rs` | 145 | 139 | 95.86% |
| `adapters/outbound/postgres_backend_repo.rs` | 231 | 204 | 88.31% |

##### Camada de Infraestrutura

| Arquivo | Regiões | Cobertura |
|---------|---------|-----------|
| `infrastructure/circuit_breaker.rs` | 353 | 98.30% |
| `infrastructure/config_watcher.rs` | 744 | 95.30% |
| `infrastructure/rate_limiter.rs` | 589 | 93.55% |
| `infrastructure/health_checker.rs` | 950 | 92.00% |
| `infrastructure/connection_pool.rs` | 1224 | 93.71% |
| `infrastructure/shutdown.rs` | 488 | 93.65% |

##### Camada de Replicação (v0.4.0)

| Arquivo | Regiões | Cobertura |
|---------|---------|-----------|
| `replication/types.rs` | 1186 | 98.81% |
| `replication/config.rs` | 303 | 99.34% |
| `replication/sync.rs` | 1875 | 97.28% |
| `replication/gossip.rs` | 2350 | 98.80% |
| `replication/transport.rs` | 1668 | 98.55% |
| `replication/agent.rs` | 981 | 99.77% |
| `replication/merkle.rs` | 1030 | 98.92% |
| `replication/mdns.rs` | 619 | 99.02% |

### Exclusões de Cobertura (Padrão Sans-IO)

O padrão Sans-IO separa lógica de negócio pura de operações de I/O. Código que realiza I/O real é excluído da cobertura usando `#[cfg_attr(coverage_nightly, coverage(off))]`:

| Código | Motivo |
|--------|--------|
| `main.rs` | Entry point, composition root |
| `handle_packet()` (dns_server) | Dependente de I/O de rede |
| `proxy_bidirectional()` (tcp_server) | Operações reais de socket TCP |
| `start()`, `run()` (servers) | Event loops async com I/O de rede |
| `start_event_loop()`, `start_flush_loop()` (agent) | Loops async de background |
| `request()` (transport) | Operações de rede QUIC |
| `release()`, `acquire()`, `clear()` (connection_pool) | Gerenciamento async de conexões |
| `handle_connection()` (transport) | Handling de conexões QUIC |
| `start()`, `execute_actions()` (gossip) | I/O de gossip UDP |
| `start()`, `start_discovery()` (mdns) | I/O de rede mDNS |
| `SkipServerVerification` impl | Callback TLS (não pode ser testado unitariamente) |
| Módulos de teste (`#[cfg(test)]`) | Código de teste não é código de produção |

### Regiões Não Cobertas Restantes (162 total)

As 162 regiões não cobertas se enquadram nestas categorias:

| Categoria | Regiões | Motivo |
|-----------|---------|--------|
| **Erros de banco** | 20 | Falhas de conexão DB (caminhos inalcançáveis) |
| **I/O de rede** | 45 | Operações async de rede excluídas |
| **Loops CAS retry** | 25 | Retries de compare-and-swap atômico |
| **Chamadas tracing** | 18 | `tracing::warn!()` em branches de erro |
| **Callbacks TLS/QUIC** | 30 | Callbacks crypto (não pode ser testado unitariamente) |
| **Signal handlers** | 10 | Handling de sinais do SO |
| **Callbacks mDNS** | 14 | Handling de eventos mDNS |

Estes representam edge cases que requerem:
- Falhas de sistemas externos (DB, rede)
- Condições concorrentes específicas (retries CAS)
- Callbacks de handshake TLS/QUIC
- Handling de sinais do sistema operacional

Toda **lógica de negócio está 100% coberta** - apenas wrappers de I/O e caminhos de erro inalcançáveis permanecem.

### Filosofia de Testes

O edgeProxy segue estes princípios de teste:

1. **Lógica de domínio é pura e totalmente testada**: Algoritmo de scoring do `LoadBalancer` não tem dependências externas
2. **Adapters testam através de interfaces**: Implementações mock de traits para testes unitários
3. **Testes de integração usam componentes reais**: Mock backend server para testes E2E
4. **Código de rede tem exclusões de cobertura**: Código I/O-bound é testado via testes de integração
5. **Infraestrutura é modular**: Cada componente pode ser testado isoladamente

### Integração Contínua

```yaml
# Exemplo de configuração CI para cobertura
test:
  script:
    - cargo test
    - rustup run nightly cargo llvm-cov --fail-under-lines 98

coverage:
  script:
    - rustup run nightly cargo llvm-cov --html
  artifacts:
    paths:
      - target/llvm-cov/html/
```

A flag `--fail-under-lines 98` garante que a cobertura não caia abaixo de 98% no CI.

### Novos Testes Adicionados (v0.3.1)

| Módulo | Teste | Descrição |
|--------|-------|-----------|
| `circuit_breaker` | `test_allow_request_when_already_half_open` | Testa transição idempotente HalfOpen |
| `circuit_breaker` | `test_record_success_when_open` | Testa registro de sucesso em estado Open |
| `prometheus_metrics_store` | `test_global_metrics` | Testa métricas globais agregadas |
| `prometheus_metrics_store` | `test_concurrent_decrement` | Testa operações concorrentes de contador |
| `types` | `test_hlc_compare_same_time_different_counter` | Testa desempate por contador HLC |
| `types` | `test_hlc_compare_same_time_same_counter` | Testa caso de igualdade HLC |

### Novos Testes Adicionados (v0.4.0)

#### Testes Merkle Tree (40 testes)

| Teste | Descrição |
|-------|-----------|
| `test_merkle_tree_new` | Criação e inicialização da árvore |
| `test_merkle_tree_insert` | Inserção de linha única |
| `test_merkle_tree_update` | Atualização de linha muda hash |
| `test_merkle_tree_remove` | Remoção de linha |
| `test_merkle_tree_root_hash` | Cálculo do hash raiz |
| `test_merkle_tree_diff` | Detecção de diferenças entre árvores |
| `test_merkle_tree_diff_at_depth` | Traversal de diff multi-nível |
| `test_merkle_tree_get_hash` | Recuperação de hash por profundidade |
| `test_merkle_tree_leaves` | Acesso aos nós folha |
| `test_merkle_message_serialization` | Roundtrip de mensagens |
| `test_merkle_message_range_response` | Mensagem de resposta de range |
| `test_merkle_message_data_request` | Mensagem de requisição de dados |
| `test_hash_to_prefix_at_depth_zero` | Cálculo de prefixo em profundidade 0 |

#### Testes mDNS Discovery (25 testes)

| Teste | Descrição |
|-------|-----------|
| `test_mdns_discovery_new` | Criação do serviço de descoberta |
| `test_mdns_config_service_type` | Configuração de tipo de serviço |
| `test_discovered_peer_debug` | Formato debug de DiscoveredPeer |
| `test_gossip_addr` | Accessor de endereço gossip |
| `test_transport_addr` | Accessor de endereço transport |
| `test_notify_discovered_logs_valid_peer` | Logging de notificação de peer válido |
| `test_notify_discovered_different_cluster` | Filtragem de peer cross-cluster |
| `test_notify_discovered_channel_closed` | Handling de erro de canal |

#### Testes Delta Sync

| Teste | Descrição |
|-------|-----------|
| `test_field_op_serialization` | Roundtrip de enum FieldOp |
| `test_delta_data_new` | Criação de DeltaData |
| `test_change_data_full` | Dados de mudança de linha completa |
| `test_change_data_delta` | Dados de mudança delta |
| `test_apply_delta_change` | Aplicação de delta na linha |

#### Testes Transport Sans-IO (48 testes)

| Teste | Descrição |
|-------|-----------|
| `test_encode_message_*` | Encoding de mensagens para todos os tipos |
| `test_decode_message_*` | Decoding de mensagens com validação |
| `test_validate_broadcast` | Validação de checksum de broadcast |
| `test_create_sync_request` | Criação de mensagem SyncRequest |
| `test_extract_sync_response` | Extração de changesets de resposta |
| `test_message_type_name` | Conversão de tipo de mensagem para string |
| `test_count_broadcast_changes` | Contagem de mudanças |
| `test_get_broadcast_seq` | Extração de número de sequência |

---

## Testes de Configuração (v0.4.0)

Testes de configuração validam todas as combinações de variáveis de ambiente e configurações de serviço.

### Executando Testes de Configuração

```bash
# Executar todos os testes de configuração
task test:config-all

# Executar testes individuais
task test:config-default      # Apenas TCP (config mínima)
task test:config-all-services # Todos os serviços habilitados
task test:config-regions      # Testar regiões sa, us, eu, ap
task test:config-api          # Endpoints da API
task test:config-tls          # TLS com cert auto-assinado
task test:config-replication  # Replicação (gossip + transport)
task test:config-binding      # Configuração de TTL de binding
task test:config-debug        # Modo debug

# Limpeza entre testes
task test:config-cleanup
```

### Cenários de Teste

| Teste | Variáveis de Ambiente | Resultado Esperado |
|-------|----------------------|-------------------|
| **Default** | `LISTEN_ADDR`, `DB_PATH`, `REGION` | TCP proxy na 8080 |
| **All Services** | Todas vars habilitadas | 6 portas listening |
| **Regions** | `REGION=sa/us/eu/ap` | Cada região inicia |
| **API** | `API_ENABLED=true` | 6 endpoints funcionando |
| **TLS** | `TLS_ENABLED=true` | Cert auto-assinado |
| **Replication** | `REPLICATION_ENABLED=true` | Gossip + Transport |
| **Binding** | `BINDING_TTL_SECS=300` | TTL customizado funciona |
| **Debug** | `DEBUG=1` | Logging de debug |

### Testes de Endpoints da API

| Endpoint | Método | Teste | Esperado |
|----------|--------|-------|----------|
| `/health` | GET | Health check | `{"status":"ok"}` |
| `/api/v1/register` | POST | Registrar backend | `{"registered":true}` |
| `/api/v1/backends` | GET | Listar backends | Array de backends |
| `/api/v1/backends/:id` | GET | Obter backend | Detalhes do backend |
| `/api/v1/heartbeat/:id` | POST | Atualizar heartbeat | `{"status":"ok"}` |
| `/api/v1/backends/:id` | DELETE | Remover backend | `{"deregistered":true}` |

---

## Testes de Deploy na AWS (v0.4.0)

Testes de deploy em produção na AWS Irlanda (eu-west-1).

### Detalhes do Deploy

| Propriedade | Valor |
|-------------|-------|
| **Instância** | 34.240.78.199 |
| **Região** | eu-west-1 (Irlanda) |
| **Tipo de Instância** | t3.micro |
| **SO** | Ubuntu 22.04 |
| **Binário** | `/opt/edgeproxy/edge-proxy` |
| **Serviço** | systemd (`edgeproxy.service`) |

### Configuração do Serviço

```ini
[Service]
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_DB_PATH=/opt/edgeproxy/routing.db
Environment=EDGEPROXY_REGION=eu
Environment=EDGEPROXY_DB_RELOAD_SECS=5
Environment=EDGEPROXY_BINDING_TTL_SECS=600
Environment=EDGEPROXY_BINDING_GC_INTERVAL_SECS=60
Environment=EDGEPROXY_TLS_ENABLED=true
Environment=EDGEPROXY_TLS_LISTEN_ADDR=0.0.0.0:8443
Environment=EDGEPROXY_API_ENABLED=true
Environment=EDGEPROXY_API_LISTEN_ADDR=0.0.0.0:8081
Environment=EDGEPROXY_HEARTBEAT_TTL_SECS=60
Environment=EDGEPROXY_DNS_ENABLED=true
Environment=EDGEPROXY_DNS_LISTEN_ADDR=0.0.0.0:5353
Environment=EDGEPROXY_DNS_DOMAIN=internal
Environment=EDGEPROXY_REPLICATION_ENABLED=true
Environment=EDGEPROXY_REPLICATION_NODE_ID=pop-eu-ireland-1
Environment=EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
Environment=EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
Environment=EDGEPROXY_REPLICATION_DB_PATH=/opt/edgeproxy/state.db
Environment=EDGEPROXY_REPLICATION_CLUSTER_NAME=edgeproxy-prod
```

### Resultados dos Testes (2025-12-08)

#### Conectividade de Portas

| Serviço | Porta | Protocolo | Status |
|---------|-------|-----------|--------|
| TCP Proxy | 8080 | TCP | OK |
| TLS Server | 8443 | TCP | OK |
| API Server | 8081 | TCP | OK |
| DNS Server | 5353 | UDP | OK |
| Gossip | 4001 | UDP | OK |
| Transport | 4002 | UDP | OK |

#### Testes de Endpoints da API

| Endpoint | Status | Resposta |
|----------|--------|----------|
| `GET /health` | OK | `{"status":"ok","version":"0.2.0"}` |
| `POST /api/v1/register` | OK | `{"registered":true}` |
| `GET /api/v1/backends` | OK | Lista backends registrados |
| `GET /api/v1/backends/:id` | OK | Retorna detalhes do backend |
| `POST /api/v1/heartbeat/:id` | OK | `{"status":"ok"}` |
| `DELETE /api/v1/backends/:id` | OK | `{"deregistered":true}` |

#### Certificado TLS

```
subject=CN = rcgen self signed cert
issuer=CN = rcgen self signed cert
```

#### Estado da Replicação

- State DB: `/opt/edgeproxy/state.db` (36KB)
- Node ID: `pop-eu-ireland-1`
- Cluster: `edgeproxy-prod`

### Executando Testes na AWS

```bash
# SSH para a instância
ssh -i .keys/edgeproxy-hub.pem ubuntu@34.240.78.199

# Verificar status do serviço
sudo systemctl status edgeproxy

# Ver logs
sudo journalctl -u edgeproxy -f

# Testar API localmente
curl http://127.0.0.1:8081/health | jq .

# Testar de fora (requer regras no Security Group)
curl http://34.240.78.199:8081/health
```

### Regras do Security Group

| Porta | Protocolo | Origem | Descrição |
|-------|-----------|--------|-----------|
| 22 | TCP | Seu IP | SSH |
| 8080 | TCP | 0.0.0.0/0 | TCP Proxy |
| 8081 | TCP | 0.0.0.0/0 | API Server |
| 8443 | TCP | 0.0.0.0/0 | TLS Server |
| 5353 | UDP | 0.0.0.0/0 | DNS Server |
| 4001 | UDP | VPC CIDR | Gossip (interno) |
| 4002 | UDP | VPC CIDR | Transport (interno) |

### Resultados de Latência (Brasil para Irlanda)

| Teste | Latência |
|-------|----------|
| API Health Check | ~408ms |
| 100 requests | 42.8s total (~428ms média) |

> Nota: A latência é esperada devido à distância geográfica (Brasil para Irlanda ~9.000km)
