---
sidebar_position: 3
---

# Deploy Fly.io

Este guia cobre o deploy dos backends do edgeProxy no Fly.io com rede overlay WireGuard para distribuição global.

## Visão Geral

![Fly.io Infrastructure](/img/flyio-infrastructure.svg)

O Fly.io oferece computação de borda com máquinas em mais de 30 regiões mundiais. Usamos para fazer deploy dos servidores backend que conectam ao POP central do edgeProxy via WireGuard.

## Pré-requisitos

```bash
# Instalar Fly CLI
curl -L https://fly.io/install.sh | sh

# Login no Fly.io
fly auth login

# Verificar autenticação
fly auth whoami
```

## Regiões Disponíveis

| Código | Localização | Continente |
|--------|-------------|------------|
| **gru** | São Paulo | América do Sul |
| **iad** | Virginia | América do Norte |
| **ord** | Chicago | América do Norte |
| **lax** | Los Angeles | América do Norte |
| **lhr** | Londres | Europa |
| **fra** | Frankfurt | Europa |
| **cdg** | Paris | Europa |
| **nrt** | Tóquio | Ásia Pacífico |
| **sin** | Singapura | Ásia Pacífico |
| **syd** | Sydney | Oceania |

---

## Estrutura do Projeto

```
fly-backend/
├── fly.toml              # Configuração do Fly.io
├── Dockerfile            # Build multi-stage com WireGuard
├── main.go               # Servidor backend (Go)
├── entrypoint.sh         # Startup do WireGuard + backend
└── wireguard/
    └── keys/             # Chaves WireGuard por região
```

---

## Dockerfile

Build multi-stage com suporte a WireGuard:

```dockerfile
FROM golang:1.21-alpine AS builder
WORKDIR /app
COPY main.go .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o backend main.go

FROM alpine:3.19
RUN apk --no-cache add ca-certificates wireguard-tools iptables ip6tables iproute2 bash
WORKDIR /app
COPY --from=builder /app/backend .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

EXPOSE 8080
EXPOSE 51820/udp

ENTRYPOINT ["./entrypoint.sh"]
```

---

## Script de Entrypoint

O entrypoint configura o WireGuard baseado na região do Fly:

```bash
#!/bin/bash
set -e

# Endpoint central do edgeProxy
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Mapear região para IP WireGuard
case "${FLY_REGION}" in
  gru) WG_IP="10.50.1.1/32"; WG_PRIVATE="${WG_KEY_GRU}" ;;
  iad) WG_IP="10.50.2.1/32"; WG_PRIVATE="${WG_KEY_IAD}" ;;
  ord) WG_IP="10.50.2.2/32"; WG_PRIVATE="${WG_KEY_ORD}" ;;
  lax) WG_IP="10.50.2.3/32"; WG_PRIVATE="${WG_KEY_LAX}" ;;
  lhr) WG_IP="10.50.3.1/32"; WG_PRIVATE="${WG_KEY_LHR}" ;;
  fra) WG_IP="10.50.3.2/32"; WG_PRIVATE="${WG_KEY_FRA}" ;;
  cdg) WG_IP="10.50.3.3/32"; WG_PRIVATE="${WG_KEY_CDG}" ;;
  nrt) WG_IP="10.50.4.1/32"; WG_PRIVATE="${WG_KEY_NRT}" ;;
  sin) WG_IP="10.50.4.2/32"; WG_PRIVATE="${WG_KEY_SIN}" ;;
  syd) WG_IP="10.50.4.3/32"; WG_PRIVATE="${WG_KEY_SYD}" ;;
  *) echo "Região desconhecida: ${FLY_REGION}"; exit 1 ;;
esac

# Criar configuração WireGuard
mkdir -p /etc/wireguard
cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/16
PersistentKeepalive = 25
EOF

# Iniciar WireGuard
wg-quick up wg0

echo "WireGuard conectado: ${FLY_REGION} -> ${WG_IP}"

# Iniciar backend
exec ./backend
```

---

## Configuração fly.toml

```toml
app = "edgeproxy-backend"
primary_region = "gru"

[build]
  dockerfile = "Dockerfile"

[env]
  PORT = "8080"

[http_service]
  internal_port = 8080
  force_https = false
  auto_stop_machines = false
  auto_start_machines = true
  min_machines_running = 1

[[vm]]
  cpu_kind = "shared"
  cpus = 1
  memory_mb = 256
```

---

## Configuração de Chaves WireGuard

### Gerar Chaves

```bash
# Gerar par de chaves para cada região
for region in gru iad ord lax lhr fra cdg nrt sin syd; do
  wg genkey > wireguard/keys/${region}-private.key
  cat wireguard/keys/${region}-private.key | wg pubkey > wireguard/keys/${region}-public.key
done
```

### Configurar Secrets no Fly.io

```bash
# Definir chaves privadas WireGuard como secrets
fly secrets set \
  WG_KEY_GRU="$(cat wireguard/keys/gru-private.key)" \
  WG_KEY_IAD="$(cat wireguard/keys/iad-private.key)" \
  WG_KEY_ORD="$(cat wireguard/keys/ord-private.key)" \
  WG_KEY_LAX="$(cat wireguard/keys/lax-private.key)" \
  WG_KEY_LHR="$(cat wireguard/keys/lhr-private.key)" \
  WG_KEY_FRA="$(cat wireguard/keys/fra-private.key)" \
  WG_KEY_CDG="$(cat wireguard/keys/cdg-private.key)" \
  WG_KEY_NRT="$(cat wireguard/keys/nrt-private.key)" \
  WG_KEY_SIN="$(cat wireguard/keys/sin-private.key)" \
  WG_KEY_SYD="$(cat wireguard/keys/syd-private.key)"
```

---

## Deploy

### Criar App

```bash
cd fly-backend

# Criar novo app
fly apps create edgeproxy-backend

# Ou iniciar interativamente
fly launch --no-deploy
```

### Deploy para Todas as Regiões

```bash
# Deploy da aplicação
fly deploy --remote-only

# Escalar para múltiplas regiões (1 máquina por região)
fly scale count 1 --region gru,iad,ord,lax,lhr,fra,cdg,nrt,sin,syd

# Verificar deploy
fly status
```

### Escalar Regiões Individuais

```bash
# Adicionar mais máquinas em região específica
fly scale count 2 --region gru

# Remover máquinas de região
fly scale count 0 --region lax
```

---

## Monitoramento

### Verificar Status

```bash
# Status da aplicação
fly status

# Listar todas as máquinas
fly machines list

# Ver logs
fly logs

# Logs de região específica
fly logs --region gru
```

### SSH na Máquina

```bash
# SSH para máquina aleatória
fly ssh console

# SSH para região específica
fly ssh console --region gru

# Verificar status WireGuard dentro da máquina
wg show
```

### Verificação de Saúde

```bash
# Testar região específica
curl https://edgeproxy-backend.fly.dev/api/info

# Testar via edgeProxy (deve rotear para backend mais próximo)
curl http://54.171.48.207:8080/api/info
```

---

## Troubleshooting

### WireGuard Não Conecta

```bash
# SSH na máquina
fly ssh console

# Verificar status WireGuard
wg show

# Verificar se interface existe
ip addr show wg0

# Verificar logs
cat /var/log/wireguard.log
```

### Máquina Não Inicia

```bash
# Verificar logs da máquina
fly logs --instance <machine-id>

# Reiniciar máquina
fly machines restart <machine-id>

# Destruir e recriar
fly machines destroy <machine-id>
fly scale count 1 --region <region>
```

### Secrets Não Configurados

```bash
# Listar secrets
fly secrets list

# Definir secret faltante
fly secrets set WG_KEY_GRU="<private-key>"
```

---

## Otimização de Custos

### Máquinas com CPU Compartilhada

```toml
[[vm]]
  cpu_kind = "shared"
  cpus = 1
  memory_mb = 256  # Mínimo para WireGuard
```

### Auto-Stop de Máquinas Ociosas

```toml
[http_service]
  auto_stop_machines = true
  auto_start_machines = true
  min_machines_running = 0  # Escalar para zero quando ocioso
```

---

## Documentação Relacionada

- [Deploy AWS EC2](./aws) - Setup do POP central
- [Deploy Docker](./docker) - Desenvolvimento local
- [Benchmarks](../benchmark) - Testes de performance globais
