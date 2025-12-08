---
sidebar_position: 6
---

# Control Plane Distribuído (Corrosion)

O Corrosion habilita replicação distribuída do SQLite entre todos os POPs.

## Arquitetura

![Arquitetura Corrosion](/img/corrosion-architecture.svg)

## Como Funciona

Quando `EDGEPROXY_CORROSION_ENABLED=true`, o edgeProxy **ignora** o `EDGEPROXY_DB_PATH` local e consulta a API HTTP do Corrosion para obter dados dos backends. O Corrosion gerencia toda a replicação entre POPs automaticamente.

![Fluxo de Dados Corrosion](/img/corrosion-data-flow.svg)

## Instalação

### Opção A: Instalação Nativa (Debian/Ubuntu)

#### Passo 1: Instalar Corrosion

```bash
# Baixar última versão do Corrosion
CORROSION_VERSION="0.5.0"
curl -L -o /tmp/corrosion.tar.gz \
  "https://github.com/superfly/corrosion/releases/download/v${CORROSION_VERSION}/corrosion-x86_64-unknown-linux-gnu.tar.gz"

# Extrair e instalar
sudo tar -xzf /tmp/corrosion.tar.gz -C /usr/local/bin/
sudo chmod +x /usr/local/bin/corrosion

# Verificar instalação
corrosion --version
```

#### Passo 2: Criar Configuração do Corrosion

```bash
# Criar diretórios
sudo mkdir -p /etc/corrosion /var/lib/corrosion

# Criar arquivo de configuração
sudo tee /etc/corrosion/corrosion.toml << 'EOF'
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
# Nós bootstrap (outros POPs) - deixe vazio para o primeiro nó
bootstrap = []

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
EOF
```

#### Passo 3: Criar Serviço Systemd para Corrosion

```bash
sudo tee /etc/systemd/system/corrosion.service << 'EOF'
[Unit]
Description=Corrosion - SQLite Distribuído
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/corrosion agent -c /etc/corrosion/corrosion.toml
Restart=always
RestartSec=5
User=root

[Install]
WantedBy=multi-user.target
EOF

# Habilitar e iniciar
sudo systemctl daemon-reload
sudo systemctl enable corrosion
sudo systemctl start corrosion

# Verificar status
sudo systemctl status corrosion
```

#### Passo 4: Instalar edgeProxy

```bash
# Baixar edgeProxy (ou compilar do código fonte)
curl -L -o /tmp/edgeproxy.tar.gz \
  "https://github.com/andrebassi/edgeproxy/releases/latest/download/edgeproxy-linux-amd64.tar.gz"

# Extrair e instalar
sudo tar -xzf /tmp/edgeproxy.tar.gz -C /usr/local/bin/
sudo chmod +x /usr/local/bin/edgeproxy

# Verificar instalação
edgeproxy --version
```

#### Passo 5: Criar Serviço Systemd para edgeProxy

```bash
sudo tee /etc/systemd/system/edgeproxy.service << 'EOF'
[Unit]
Description=edgeProxy - Proxy TCP Geo-aware
After=network.target corrosion.service
Requires=corrosion.service

[Service]
Type=simple
ExecStart=/usr/local/bin/edgeproxy
Restart=always
RestartSec=5
User=root

# Configuração via variáveis de ambiente
Environment=EDGEPROXY_REGION=sa
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_TLS_LISTEN_ADDR=0.0.0.0:8443
Environment=EDGEPROXY_API_LISTEN_ADDR=0.0.0.0:8081
Environment=EDGEPROXY_CORROSION_ENABLED=true
Environment=EDGEPROXY_CORROSION_API_URL=http://127.0.0.1:8090

[Install]
WantedBy=multi-user.target
EOF

# Habilitar e iniciar
sudo systemctl daemon-reload
sudo systemctl enable edgeproxy
sudo systemctl start edgeproxy

# Verificar status
sudo systemctl status edgeproxy
```

#### Passo 6: Inicializar Schema e Verificar

```bash
# Aguardar Corrosion estar pronto
sleep 2

# Criar tabela backends
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "sql": "CREATE TABLE IF NOT EXISTS backends (id TEXT PRIMARY KEY, app TEXT, region TEXT, wg_ip TEXT, port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER DEFAULT 0)"
  }'

# Verificar se edgeProxy está conectado
curl http://localhost:8081/health
```

#### Configuração de Firewall (UFW)

```bash
# Liberar portas do edgeProxy
sudo ufw allow 8080/tcp   # Proxy TCP
sudo ufw allow 8081/tcp   # Auto-Discovery API
sudo ufw allow 8443/tcp   # Proxy TLS
sudo ufw allow 4001/tcp   # Corrosion gossip (apenas se multi-POP)
```

#### Logs

```bash
# Ver logs do Corrosion
sudo journalctl -u corrosion -f

# Ver logs do edgeProxy
sudo journalctl -u edgeproxy -f
```

---

### Opção B: Docker Compose

O Corrosion roda como um **container sidecar** junto com o edgeProxy. Ambos compartilham a mesma rede, permitindo que o edgeProxy alcance o Corrosion via `http://corrosion:8090`.

### Passo 1: Criar Configuração do Corrosion

```toml
# corrosion.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
# Nós bootstrap (outros POPs) - deixe vazio para o primeiro nó
bootstrap = []

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

### Passo 2: Criar Docker Compose

```yaml
# docker-compose.yml
version: '3.8'

services:
  edgeproxy:
    image: edgeproxy:latest
    ports:
      - "8080:8080"   # TCP proxy
      - "8081:8081"   # Auto-Discovery API
      - "8443:8443"   # TLS proxy
    environment:
      EDGEPROXY_REGION: sa
      EDGEPROXY_LISTEN_ADDR: 0.0.0.0:8080
      # Conectar ao Corrosion sidecar
      EDGEPROXY_CORROSION_ENABLED: "true"
      EDGEPROXY_CORROSION_API_URL: http://corrosion:8090
      EDGEPROXY_CORROSION_POLL_SECS: "5"
    depends_on:
      - corrosion
    networks:
      - edgeproxy-net

  corrosion:
    image: ghcr.io/superfly/corrosion:latest
    volumes:
      - ./corrosion.toml:/etc/corrosion/corrosion.toml:ro
      - corrosion-data:/var/lib/corrosion
    ports:
      - "4001:4001"   # Gossip (para outros POPs)
      - "8090:8090"   # HTTP API (interno)
    command: ["/corrosion", "agent", "-c", "/etc/corrosion/corrosion.toml"]
    networks:
      - edgeproxy-net

networks:
  edgeproxy-net:
    driver: bridge

volumes:
  corrosion-data:
```

### Passo 3: Iniciar a Stack

```bash
# Iniciar edgeProxy + Corrosion
docker-compose up -d

# Verificar se o Corrosion está rodando
curl http://localhost:8090/v1/queries \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT 1"}'

# Verificar logs do edgeProxy
docker-compose logs -f edgeproxy
```

### Passo 4: Inicializar o Schema

```bash
# Criar a tabela backends (uma vez)
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "sql": "CREATE TABLE IF NOT EXISTS backends (id TEXT PRIMARY KEY, app TEXT, region TEXT, wg_ip TEXT, port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER DEFAULT 0)"
  }'
```

## Setup Multi-POP

Para múltiplos POPs, cada POP roda seu próprio par edgeProxy + Corrosion. As instâncias do Corrosion se descobrem via protocolo gossip.

### POP SA (São Paulo) - Primeiro Nó

```toml
# corrosion-sa.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
bootstrap = []  # Primeiro nó, sem bootstrap

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

### POP US (Virginia) - Conecta ao SA

```toml
# corrosion-us.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
bootstrap = ["pop-sa.example.com:4001"]  # Aponta para SA

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

### POP EU (Frankfurt) - Conecta ao SA ou US

```toml
# corrosion-eu.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
bootstrap = ["pop-sa.example.com:4001", "pop-us.example.com:4001"]

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

:::info WireGuard Necessário
A porta gossip (4001) deve estar acessível entre os POPs. Use rede overlay WireGuard para comunicação segura.
:::

## Topologia de Deploy

![Topologia Corrosion](/img/corrosion-topology.svg)

## Referência de Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_CORROSION_ENABLED` | `false` | Habilitar backend Corrosion |
| `EDGEPROXY_CORROSION_API_URL` | `http://localhost:8090` | URL da API HTTP do Corrosion |
| `EDGEPROXY_CORROSION_POLL_SECS` | `5` | Intervalo de polling para sync |

## Benefícios

- **Sync em tempo real**: Mudanças propagam em ~100ms via protocolo gossip
- **Sem intervenção manual**: Replicação automática entre todos os POPs
- **Tolerância a partições**: Funciona durante splits de rede (baseado em CRDT)
- **Fonte única da verdade**: Registre backend uma vez, disponível em todos os lugares

## Registrando Backends

Existem três formas de registrar backends, dependendo da sua configuração:

### Opção 1: Auto-Discovery API (Recomendado para Produção)

:::tip Recomendado
A Auto-Discovery API é o **método mais simples para produção**. Backends se registram automaticamente via HTTP - sem SQL!
:::

```bash
# Backend se registra (a partir do servidor backend)
curl -X POST http://localhost:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{
    "id": "sa-node-1",
    "app": "myapp",
    "region": "sa",
    "ip": "10.50.1.1",
    "port": 9000,
    "weight": 2,
    "soft_limit": 100,
    "hard_limit": 150
  }'

# Backend envia heartbeat periódico para manter-se saudável
curl -X POST http://localhost:8081/api/v1/heartbeat/sa-node-1
```

O backend expira automaticamente se parar de enviar heartbeats. Veja [Auto-Discovery API](./auto-discovery-api) para detalhes.

### Opção 2: Corrosion SQL API (Com Corrosion Habilitado)

Quando usando Corrosion, insira backends via API HTTP do Corrosion. Os dados replicam automaticamente para todos os POPs:

```bash
# Inserir backend (em qualquer POP - replica para todos)
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "sql": "INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit) VALUES (\"sa-node-1\", \"myapp\", \"sa\", \"10.50.1.1\", 9000, 1, 2, 100, 150)"
  }'

# Atualizar saúde do backend
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{"sql": "UPDATE backends SET healthy=0 WHERE id=\"sa-node-1\""}'

# Listar todos os backends
curl -X POST http://localhost:8090/v1/queries \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT * FROM backends WHERE healthy=1"}'
```

### Opção 3: SQLite Local (Modo Standalone)

Sem Corrosion, insira diretamente no `routing.db` em **cada POP manualmente**:

```bash
# Em cada POP (sem replicação automática!)
sqlite3 routing.db "INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 9000, 1, 2, 100, 150);"
```

:::warning
No modo standalone, você deve inserir backends manualmente em cada POP. Isso só é recomendado para desenvolvimento/testes.
:::

## Comparação

| Método | Replicação | Complexidade | Caso de Uso |
|--------|------------|--------------|-------------|
| Auto-Discovery API | Depende do storage | Baixa | Produção (recomendado) |
| Corrosion SQL API | Automática | Média | Produção com Corrosion |
| SQLite Local | Manual | Alta | Desenvolvimento/Testes |

## Endpoints da API Corrosion

O Corrosion expõe uma API REST:

| Endpoint | Método | Descrição |
|----------|--------|-----------|
| `/v1/queries` | POST | Executar query SQL (SELECT) |
| `/v1/transactions` | POST | Executar transação SQL (INSERT/UPDATE/DELETE) |

## Troubleshooting

### Corrosion não acessível

```bash
# Verificar se o Corrosion está rodando
docker-compose ps corrosion

# Verificar logs do Corrosion
docker-compose logs corrosion

# Testar API a partir do container edgeProxy
docker-compose exec edgeproxy curl http://corrosion:8090/v1/queries \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT 1"}'
```

### Dados não replicando entre POPs

```bash
# Verificar conectividade gossip
nc -zv pop-sa.example.com 4001

# Verificar status do cluster Corrosion
curl http://localhost:8090/v1/cluster/status
```
