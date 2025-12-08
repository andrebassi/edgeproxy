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
| **Total de Testes** | 358 |
| **Cobertura de Linhas** | **99.38%** |
| **Linhas Cobertas** | 1.441 / 1.450 |
| **Linhas Não Cobertas** | 9 |

### Executando Testes

```bash
# Executar todos os testes
cargo test

# Executar testes com output
cargo test -- --nocapture

# Executar testes de um módulo específico
cargo test domain::services::load_balancer

# Executar testes em paralelo (padrão)
cargo test -- --test-threads=4

# Executar single-threaded (para debug)
cargo test -- --test-threads=1
```

### Testes por Módulo

| Módulo | Testes | Descrição |
|--------|--------|-----------|
| `adapters::inbound::dns_server` | 44 | Servidor DNS, handling de pacotes, resolução geo-routing |
| `adapters::inbound::api_server` | 38 | API de Auto-Discovery, registro, heartbeat, lifecycle |
| `adapters::inbound::tls_server` | 29 | Terminação TLS, handling de certificados, conexões |
| `adapters::outbound::corrosion_backend_repo` | 28 | Sync distribuído SQLite via Corrosion |
| `adapters::inbound::tcp_server` | 27 | Conexões TCP, lógica de proxy, handling de clientes |
| `domain::value_objects` | 26 | RegionCode, mapeamento de países, parsing |
| `application::proxy_service` | 26 | Orquestração de use cases, resolução de backend |
| `domain::services::load_balancer` | 25 | Algoritmo de scoring, geo-routing, pesos |
| `config` | 24 | Carregamento de configuração, variáveis de ambiente |
| `adapters::outbound::dashmap_binding_repo` | 21 | Client affinity, TTL, garbage collection |
| `adapters::outbound::sqlite_backend_repo` | 20 | Storage SQLite de backends, reload |
| `adapters::outbound::dashmap_metrics_store` | 20 | Métricas de conexão, tracking de RTT |
| `adapters::outbound::maxmind_geo_resolver` | 18 | Resolução GeoIP, mapeamento país/região |
| `domain::entities` | 12 | Entidades Backend, Binding, ClientKey |

### Testes por Camada (Arquitetura Hexagonal)

![Testes por Camada](/img/tests-by-layer.svg)

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
```

### Executando Cobertura

```bash
# Relatório básico de cobertura (exclui main.rs)
cargo +nightly llvm-cov --ignore-filename-regex "main.rs"

# Cobertura com relatório HTML
cargo +nightly llvm-cov --html --ignore-filename-regex "main.rs"

# Cobertura com output LCOV
cargo +nightly llvm-cov --lcov --output-path lcov.info --ignore-filename-regex "main.rs"

# Abrir relatório HTML
open target/llvm-cov/html/index.html
```

### Resultados de Cobertura

**Cobertura Final: 99.38%** (1.441 de 1.450 linhas cobertas)

| Arquivo | Cobertura | Linhas | Faltando | Notas |
|---------|-----------|--------|----------|-------|
| `config.rs` | 100% | 92 | 0 | Todos os caminhos de config testados |
| `domain/entities.rs` | 100% | 58 | 0 | Todos os métodos de entidade testados |
| `domain/value_objects.rs` | 100% | 106 | 0 | Mapeamento completo país/região |
| `domain/services/load_balancer.rs` | 98.78% | 82 | 5 | Branch coverage em edge cases |
| `application/proxy_service.rs` | 100% | 80 | 0 | Cobertura completa de use cases |
| `adapters/inbound/api_server.rs` | 100% | 295 | 0 | Cobertura completa da API |
| `adapters/inbound/dns_server.rs` | 100% | 138 | 0 | Resolução DNS testada |
| `adapters/inbound/tcp_server.rs` | 97.92% | 96 | 1 | Exclusão de I/O de rede |
| `adapters/inbound/tls_server.rs` | 97.30% | 111 | 3 | Edge cases de TLS handshake |
| `adapters/outbound/sqlite_backend_repo.rs` | 100% | 67 | 0 | Cobertura completa SQLite |
| `adapters/outbound/corrosion_backend_repo.rs` | 100% | 127 | 0 | Sync distribuído testado |
| `adapters/outbound/dashmap_binding_repo.rs` | 100% | 78 | 0 | Client affinity completo |
| `adapters/outbound/dashmap_metrics_store.rs` | 100% | 68 | 0 | Tracking de métricas coberto |
| `adapters/outbound/maxmind_geo_resolver.rs` | 100% | 52 | 0 | Resolução GeoIP coberta |

### Exclusões de Cobertura

Alguns códigos são intencionalmente excluídos da cobertura usando `#[cfg_attr(coverage_nightly, coverage(off))]`:

| Código | Motivo |
|--------|--------|
| `main.rs` | Entry point, composition root |
| `handle_packet()` (dns_server) | Dependente de I/O de rede |
| `proxy_bidirectional()` (tcp_server) | Operações reais de socket TCP |
| `start_sync()` (sqlite_backend_repo) | Thread de background com I/O |
| Módulos de teste (`#[cfg(test)]`) | Código de teste não é código de produção |

### Por que não 100%?

As 9 linhas restantes não cobertas são **artefatos de branch coverage**, não código verdadeiramente não testado:

1. **Condições de branch com valores `0`**: Linhas como `if soft_limit == 0` ou `if weight == 0` são testadas, mas o LLVM conta branches de forma diferente
2. **Edge cases de I/O de rede**: Alguns caminhos de erro em código async de rede não podem ser disparados em testes unitários
3. **Todas as linhas executam**: Análise LCOV mostra `0` linhas com contagem zero - as linhas "faltando" são contadores internos de branch

### Filosofia de Testes

O edgeProxy segue estes princípios de teste:

1. **Lógica de domínio é pura e totalmente testada**: Algoritmo de scoring do `LoadBalancer` não tem dependências externas
2. **Adapters testam através de interfaces**: Implementações mock de traits para testes unitários
3. **Testes de integração usam componentes reais**: Mock backend server para testes E2E
4. **Código de rede tem exclusões de cobertura**: Código I/O-bound é testado via testes de integração, não unitários

### Integração Contínua

```yaml
# Exemplo de configuração CI para cobertura
test:
  script:
    - cargo test
    - cargo +nightly llvm-cov --ignore-filename-regex "main.rs" --fail-under-lines 95
```

A flag `--fail-under-lines 95` garante que a cobertura não caia abaixo de 95% no CI.
