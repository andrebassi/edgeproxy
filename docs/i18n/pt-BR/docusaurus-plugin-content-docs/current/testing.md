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
