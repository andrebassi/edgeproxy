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

:::info GeoIP Incluído
O banco de dados MaxMind GeoLite2 está **embeddado no binário** - nenhum download externo necessário.
:::

## Instalação

### A partir do Código Fonte

```bash
# Clone o repositório
git clone https://github.com/andrebassi/edgeproxy.git
cd edgeproxy

# Build do binário release
task build:release

# Verificar instalação
./target/release/edge-proxy --help
```

### Usando Docker

```bash
# Build da imagem Docker
task docker:build

# Iniciar ambiente multi-região
task docker:up
```

## Estrutura do Projeto

O edgeProxy usa **Arquitetura Hexagonal** (Ports & Adapters):

![Estrutura do Projeto](/img/project-structure.svg)

## Primeira Execução

### 1. Inicializar Banco de Roteamento

```bash
# Criar banco de dados com backends de exemplo
task db:init
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
task run:dev

# Ou com configurações customizadas
EDGEPROXY_REGION=us EDGEPROXY_LISTEN_ADDR=0.0.0.0:9000 task run:dev
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
# Executar todos os testes (485 testes)
task test:all

# Executar com cobertura
task test:coverage
```

### Simulação Local Multi-Região

```bash
# Terminal 1: Iniciar mock backends
task local:env

# Terminal 2: Iniciar proxy
task run:sa

# Terminal 3: Executar testes
task local:test
```

### Testes Docker

```bash
# Suite completa de testes Docker
task docker:build
task docker:up
task docker:test

# Limpeza
task docker:down
```

## Tasks Disponíveis

Execute `task --list` para ver todas as tasks disponíveis. Principais categorias:

### Build

| Task | Descrição |
|------|-----------|
| `task build:release` | Build do binário release |
| `task build:linux` | Cross-compile para Linux AMD64 |
| `task build:all` | Build para todas as plataformas |

### Run

| Task | Descrição |
|------|-----------|
| `task run:dev` | Executar com config padrão |
| `task run:sa` | Executar como POP SA |
| `task run:us` | Executar como POP US |
| `task run:eu` | Executar como POP EU |

### Test

| Task | Descrição |
|------|-----------|
| `task test:all` | Executar todos os testes unitários |
| `task test:coverage` | Executar com relatório de cobertura |

### Database

| Task | Descrição |
|------|-----------|
| `task db:init` | Inicializar routing.db |
| `task db:reset` | Resetar para estado inicial |

### Docker

| Task | Descrição |
|------|-----------|
| `task docker:build` | Build das imagens Docker |
| `task docker:up` | Iniciar ambiente Docker |
| `task docker:down` | Parar ambiente Docker |
| `task docker:test` | Executar suite de testes Docker |

### Documentação

| Task | Descrição |
|------|-----------|
| `task docs:serve` | Build e serve docs (EN + PT-BR) |
| `task docs:dev` | Modo dev (apenas EN, hot reload) |

## Próximos Passos

- [Arquitetura](./architecture) - Entenda como o edgeProxy funciona
- [Configuração](./configuration) - Todas as opções de configuração
- [Deploy com Docker](./deployment/docker) - Deployment em produção
