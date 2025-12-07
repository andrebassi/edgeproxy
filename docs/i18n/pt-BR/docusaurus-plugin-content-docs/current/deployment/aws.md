---
sidebar_position: 3
---

# Deploy AWS EC2

Este guia cobre o deploy do edgeProxy como nó POP (Point of Presence) no AWS EC2 com rede overlay WireGuard.

## Pré-requisitos

```bash
# AWS CLI configurado com credenciais
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
export AWS_DEFAULT_REGION="eu-west-1"

# Verificar credenciais
aws sts get-caller-identity
```

## Visão Geral da Infraestrutura

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    edgeProxy + WireGuard - Setup de Produção                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Clientes ──► EC2 (edgeProxy POP) ──► Túnel WireGuard ──► Backends        │
│               54.171.48.207:8080       10.50.x.x            Fly.io/K8s     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Criação da Instância EC2

### Usando Taskfile

O `fly-backend/Taskfile.yaml` contém todas as tasks para infraestrutura AWS:

```yaml
version: '3'

vars:
  AWS_REGION: eu-west-1
  INSTANCE_TYPE: t3.micro
  AMI_ID: ami-0d940f23d527c3ab1  # Ubuntu 22.04 LTS
  KEY_NAME: edgeproxy-key
  SG_NAME: edgeproxy-sg
  INSTANCE_NAME: edgeproxy-pop-eu

tasks:
  aws:check:
    desc: Verificar credenciais AWS
    cmds:
      - aws sts get-caller-identity

  aws:sg:create:
    desc: Criar Security Group para edgeProxy
    cmds:
      - |
        VPC_ID=$(aws ec2 describe-vpcs --filters "Name=is-default,Values=true" \
          --query 'Vpcs[0].VpcId' --output text)

        SG_ID=$(aws ec2 create-security-group \
          --group-name {{.SG_NAME}} \
          --description "EdgeProxy - TCP proxy com WireGuard" \
          --vpc-id $VPC_ID --query 'GroupId' --output text)

        # SSH, edgeProxy, WireGuard
        aws ec2 authorize-security-group-ingress --group-id $SG_ID \
          --protocol tcp --port 22 --cidr 0.0.0.0/0
        aws ec2 authorize-security-group-ingress --group-id $SG_ID \
          --protocol tcp --port 8080 --cidr 0.0.0.0/0
        aws ec2 authorize-security-group-ingress --group-id $SG_ID \
          --protocol udp --port 51820 --cidr 0.0.0.0/0

  aws:key:create:
    desc: Criar Par de Chaves SSH
    cmds:
      - |
        aws ec2 create-key-pair --key-name {{.KEY_NAME}} \
          --query 'KeyMaterial' --output text > ~/.ssh/{{.KEY_NAME}}.pem
        chmod 400 ~/.ssh/{{.KEY_NAME}}.pem

  aws:ec2:create:
    desc: Criar instância EC2 para edgeProxy POP
    cmds:
      - |
        INSTANCE_ID=$(aws ec2 run-instances \
          --image-id {{.AMI_ID}} \
          --instance-type {{.INSTANCE_TYPE}} \
          --key-name {{.KEY_NAME}} \
          --security-group-ids $SG_ID \
          --user-data file://userdata.sh \
          --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value={{.INSTANCE_NAME}}}]' \
          --query 'Instances[0].InstanceId' --output text)

        aws ec2 wait instance-running --instance-ids $INSTANCE_ID

        PUBLIC_IP=$(aws ec2 describe-instances --instance-ids $INSTANCE_ID \
          --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)

        echo "Instance: $INSTANCE_ID"
        echo "Public IP: $PUBLIC_IP"
        echo "SSH: ssh -i ~/.ssh/{{.KEY_NAME}}.pem ubuntu@$PUBLIC_IP"
```

### Criação Passo a Passo

```bash
cd fly-backend

# 1. Verificar credenciais AWS
task aws:check

# 2. Criar Security Group
task aws:sg:create

# 3. Criar Par de Chaves SSH
task aws:key:create

# 4. Criar Instância EC2
task aws:ec2:create

# Output:
# Instance ID: i-0813ee3c789b40e51
# Public IP: 54.171.48.207
# SSH: ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@54.171.48.207
```

---

## Script User Data

A instância EC2 instala todas as dependências automaticamente via user data:

```bash
#!/bin/bash
set -ex

# Atualizar sistema
apt-get update && apt-get upgrade -y

# Instalar WireGuard
apt-get install -y wireguard wireguard-tools

# Instalar ferramentas de build
apt-get install -y curl wget git build-essential pkg-config libssl-dev

# Habilitar IP forwarding
echo "net.ipv4.ip_forward=1" >> /etc/sysctl.conf
echo "net.ipv6.conf.all.forwarding=1" >> /etc/sysctl.conf
sysctl -p

# Criar diretório edgeProxy
mkdir -p /opt/edgeproxy

# Instalar Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# Clonar e compilar edgeProxy
cd /opt/edgeproxy
git clone https://github.com/edge-cloud/edgeproxy.git .
cargo build --release

# Criar serviço systemd
cat > /etc/systemd/system/edgeproxy.service << 'EOF'
[Unit]
Description=edgeProxy TCP Proxy
After=network.target wireguard.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/edgeproxy
Environment=EDGEPROXY_REGION=eu
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_DB_PATH=/opt/edgeproxy/routing.db
ExecStart=/opt/edgeproxy/target/release/edge-proxy
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable edgeproxy
```

---

## Configuração WireGuard

### Gerar Chaves

```bash
# Gerar chaves para EC2 (servidor central)
wg genkey > wireguard/ec2-private.key
cat wireguard/ec2-private.key | wg pubkey > wireguard/ec2-public.key

# Gerar chaves para cada região de backend
for region in gru iad ord lax lhr fra cdg nrt sin syd; do
  wg genkey > wireguard/${region}-private.key
  cat wireguard/${region}-private.key | wg pubkey > wireguard/${region}-public.key
done
```

### Config do Servidor EC2

```ini
# /etc/wireguard/wg0.conf
[Interface]
PrivateKey = <ec2-private-key>
Address = 10.50.0.1/24
ListenPort = 51820
PostUp = iptables -A FORWARD -i wg0 -j ACCEPT; iptables -t nat -A POSTROUTING -o ens5 -j MASQUERADE
PostDown = iptables -D FORWARD -i wg0 -j ACCEPT; iptables -t nat -D POSTROUTING -o ens5 -j MASQUERADE

# GRU - São Paulo (América do Sul)
[Peer]
PublicKey = <gru-public-key>
AllowedIPs = 10.50.1.1/32

# IAD - Virginia (América do Norte)
[Peer]
PublicKey = <iad-public-key>
AllowedIPs = 10.50.2.1/32

# ... (todos os 10 peers)
```

### Iniciar WireGuard

```bash
# Copiar config
sudo cp wg0.conf /etc/wireguard/

# Iniciar WireGuard
sudo wg-quick up wg0

# Habilitar no boot
sudo systemctl enable wg-quick@wg0

# Verificar conexões
sudo wg show
```

---

## Topologia de Rede

```
                           Mesh WireGuard (10.50.x.x)
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐          ┌───────────────┐          ┌───────────────┐
│  EC2 Ireland  │          │  Fly.io GRU   │          │  Fly.io NRT   │
│  10.50.0.1    │◄────────►│  10.50.1.1    │          │  10.50.4.1    │
│  (edgeProxy)  │          │  (backend)    │          │  (backend)    │
└───────────────┘          └───────────────┘          └───────────────┘
        │
        │ Todos os backends conectam ao EC2 via WireGuard
        │
        ├──► 10.50.2.1 (IAD) ──► 10.50.2.2 (ORD) ──► 10.50.2.3 (LAX)
        ├──► 10.50.3.1 (LHR) ──► 10.50.3.2 (FRA) ──► 10.50.3.3 (CDG)
        └──► 10.50.4.2 (SIN) ──► 10.50.4.3 (SYD)
```

### Alocação de IPs

| Região | Código | IP WireGuard | Localização |
|--------|--------|--------------|-------------|
| **Central** | EC2 | 10.50.0.1 | Irlanda (eu-west-1) |
| América do Sul | GRU | 10.50.1.1 | São Paulo, Brasil |
| América do Norte | IAD | 10.50.2.1 | Virginia, EUA |
| América do Norte | ORD | 10.50.2.2 | Chicago, EUA |
| América do Norte | LAX | 10.50.2.3 | Los Angeles, EUA |
| Europa | LHR | 10.50.3.1 | Londres, UK |
| Europa | FRA | 10.50.3.2 | Frankfurt, Alemanha |
| Europa | CDG | 10.50.3.3 | Paris, França |
| Ásia Pacífico | NRT | 10.50.4.1 | Tóquio, Japão |
| Ásia Pacífico | SIN | 10.50.4.2 | Singapura |
| Ásia Pacífico | SYD | 10.50.4.3 | Sydney, Austrália |

---

## Setup Backend Fly.io

### Dockerfile com WireGuard

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

### Deploy no Fly.io

```bash
cd fly-backend

# Criar app
fly apps create edgeproxy-backend

# Deploy para todas as regiões
fly deploy --remote-only

# Escalar para múltiplas regiões
fly scale count 1 --region gru,iad,ord,lax,lhr,fra,cdg,nrt,sin,syd

# Verificar deploy
fly status
```

---

## Regras do Security Group

| Porta | Protocolo | Origem | Descrição |
|-------|-----------|--------|-----------|
| 22 | TCP | Seu IP | Acesso SSH |
| 8080 | TCP | 0.0.0.0/0 | edgeProxy TCP |
| 51820 | UDP | 0.0.0.0/0 | WireGuard |

---

## Monitoramento

### Verificar Status WireGuard

```bash
# Mostrar todos os peers
sudo wg show

# Verificar handshakes
sudo wg show wg0 latest-handshakes
```

### Verificar edgeProxy

```bash
# Status do serviço
sudo systemctl status edgeproxy

# Logs
sudo journalctl -u edgeproxy -f

# Testar conexão
curl http://localhost:8080/api/info
```

---

## Próximos Passos

- [Testes de Benchmark Globais](../benchmark) - Resultados dos testes com este setup
- [Deploy Docker](./docker) - Desenvolvimento local
- [Deploy Kubernetes](./kubernetes) - Deploy em K8s
