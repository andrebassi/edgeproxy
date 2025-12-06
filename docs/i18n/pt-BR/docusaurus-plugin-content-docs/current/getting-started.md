---
sidebar_position: 2
---

# Primeiros Passos

Este guia cobre instalação, build a partir do código fonte e execução do edgeProxy localmente.

## Pré-requisitos

### Obrigatórios

- **Rust 1.75+** - [Instalar Rust](https://rustup.rs/)
- **SQLite 3.x** - Geralmente pré-instalado no macOS/Linux
- **Task** - [Instalar Task](https://taskfile.dev/installation/)

### Opcionais

- **Docker & Docker Compose** - Para deployment containerizado
- **MaxMind GeoLite2** - Para geo-roteamento (registro gratuito necessário)

## Instalação

### A partir do Código Fonte

```bash
# Clone o repositório
git clone https://github.com/edge-cloud/edgeproxy.git
cd edgeproxy

# Build do binário release
task build

# Verificar instalação
./target/release/edge-proxy --help
```

### Usando Docker

```bash
# Build da imagem Docker
task docker-build

# Iniciar ambiente multi-região
task docker-up
```

## Estrutura do Projeto

```
edgeproxy/
├── Cargo.toml              # Dependências Rust
├── Taskfile.yaml           # Automação de tasks
├── routing.db              # Banco de dados SQLite de roteamento
├── sql/
│   └── create_routing_db.sql   # Schema + dados iniciais
├── src/
│   ├── main.rs             # Ponto de entrada
│   ├── config.rs           # Carregamento de configuração
│   ├── model.rs            # Estruturas de dados
│   ├── db.rs               # Sync do SQLite
│   ├── lb.rs               # Load balancer
│   ├── state.rs            # Estado compartilhado + GeoIP
│   └── proxy.rs            # Lógica do proxy TCP
├── docker/
│   ├── init-routing.sql    # Config de roteamento Docker
│   └── routing-docker.db   # DB pré-construído para Docker
├── tests/
│   ├── mock_backend.py     # Servidor backend de teste
│   └── test_docker.sh      # Suite de testes Docker
└── docs/                   # Documentação Docusaurus
```

## Primeira Execução

### 1. Inicializar Banco de Roteamento

```bash
# Criar banco de dados com backends de exemplo
task db-init
```

Isso cria `routing.db` com o seguinte schema:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,
    app TEXT,
    region TEXT,          -- "sa", "us", "eu"
    wg_ip TEXT,           -- IP do Backend (WireGuard)
    port INTEGER,
    healthy INTEGER,      -- 0 ou 1
    weight INTEGER,       -- Peso relativo
    soft_limit INTEGER,   -- Conexões confortáveis
    hard_limit INTEGER,   -- Conexões máximas
    deleted INTEGER DEFAULT 0
);
```

### 2. Iniciar edgeProxy

```bash
# Executar com configuração padrão (region=sa, port=8080)
task run

# Ou com configurações customizadas
EDGEPROXY_REGION=us EDGEPROXY_LISTEN_ADDR=0.0.0.0:9000 task run
```

### 3. Testar Conexão

```bash
# Conectar ao proxy
echo "Olá" | nc localhost 8080

# Saída esperada (se backend estiver rodando):
# Backend: sa-node-1 | Region: sa | Your IP: 127.0.0.1:xxxxx
# [sa-node-1] Echo: Olá
```

## Executando Testes

### Testes Unitários

```bash
task test
```

### Simulação Local Multi-Região

```bash
# Terminal 1: Iniciar mock backends
task local-env

# Terminal 2: Iniciar proxy
task run-sa

# Terminal 3: Executar testes
task local-test
```

### Testes Docker

```bash
# Suite completa de testes Docker
task docker-build
task docker-up
task docker-test

# Limpeza
task docker-down
```

## Tasks Disponíveis

| Task | Descrição |
|------|-----------|
| `task build` | Build do binário release |
| `task run` | Executar com config padrão |
| `task run-sa` | Executar como POP SA |
| `task run-us` | Executar como POP US |
| `task run-eu` | Executar como POP EU |
| `task test` | Executar testes unitários |
| `task db-init` | Inicializar routing.db |
| `task docker-build` | Build das imagens Docker |
| `task docker-up` | Iniciar ambiente Docker |
| `task docker-down` | Parar ambiente Docker |
| `task docker-test` | Executar suite de testes Docker |
| `task docker-logs` | Ver logs dos containers |
| `task docs-dev` | Iniciar servidor de documentação |

## Próximos Passos

- [Arquitetura](./architecture) - Entenda como o edgeProxy funciona
- [Configuração](./configuration) - Todas as opções de configuração
- [Deploy com Docker](./deployment/docker) - Deployment em produção
