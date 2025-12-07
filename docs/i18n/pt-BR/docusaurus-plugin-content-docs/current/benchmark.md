---
sidebar_position: 2
---

# Testes Globais de Benchmark

Este documento apresenta os resultados completos de benchmark do edgeProxy com rede overlay WireGuard, incluindo configuração da infraestrutura e resultados de testes em 9 localizações VPN globais.

## Resumo dos Resultados

:::tip Todos os Testes Passaram
**Geo-Routing: 9/9 ✅** | **WireGuard: 10/10 peers ✅** | **Benchmark v2: Completo ✅**
:::

### Tabela Completa de Testes

| # | Localização VPN | País | Backend | Latência | Download 1MB | Download 5MB | RPS (20) | Status |
|---|-----------------|------|---------|----------|--------------|--------------|----------|--------|
| 1 | 🇫🇷 Paris | FR | **CDG** | 530ms | 0.5 MB/s | 2.1 MB/s | 35.7 | ✅ |
| 2 | 🇩🇪 Frankfurt | DE | **FRA** | 528ms | 0.6 MB/s | 2.3 MB/s | 34.0 | ✅ |
| 3 | 🇬🇧 Londres | GB | **LHR** | 490ms | 0.6 MB/s | 2.3 MB/s | 36.6 | ✅ |
| 4 | 🇺🇸 Detroit | US | **IAD** | 708ms | 0.6 MB/s | 2.5 MB/s | 27.4 | ✅ |
| 5 | 🇺🇸 Las Vegas | US | **IAD** | 857ms | 0.5 MB/s | 2.2 MB/s | 22.5 | ✅ |
| 6 | 🇯🇵 Tóquio | JP | **NRT** | 1546ms | 0.3 MB/s | 1.1 MB/s | 12.6 | ✅ |
| 7 | 🇸🇬 Cingapura | SG | **SIN** | 1414ms | 0.3 MB/s | 1.2 MB/s | 13.8 | ✅ |
| 8 | 🇦🇺 Sydney | AU | **SYD** | 1847ms | 0.2 MB/s | 0.9 MB/s | 10.7 | ✅ |
| 9 | 🇧🇷 São Paulo | BR | **GRU** | 822ms | 0.4 MB/s | 1.6 MB/s | 23.3 | ✅ |

### Performance por Região

| Região | Latência | Observação |
|--------|----------|------------|
| 🇪🇺 Europa (CDG/FRA/LHR) | 490-530ms | Melhor - mais próximo da EC2 Irlanda |
| 🇺🇸 EUA (IAD) | 708-857ms | Médio - atravessa o Atlântico |
| 🇧🇷 Brasil (GRU) | 822ms | Bom - rota direta |
| 🇯🇵🇸🇬 Ásia (NRT/SIN) | 1414-1546ms | Alto - distância geográfica |
| 🇦🇺 Oceania (SYD) | 1847ms | Mais alto - meia volta ao mundo |

---

## Arquitetura de Teste

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    edgeProxy + WireGuard - Teste de Produção                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Cliente (VPN) ──► EC2 Irlanda (edgeProxy) ──► WireGuard ──► Fly.io       │
│                     54.171.48.207:8080          10.50.x.x    10 regiões    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Configuração da Infraestrutura

### Criação do Nodo AWS EC2

O nodo POP do edgeProxy foi criado na AWS EC2 usando automação via Taskfile:

#### Pré-requisitos

```bash
# AWS CLI configurado com credenciais
export AWS_ACCESS_KEY_ID="sua-access-key"
export AWS_SECRET_ACCESS_KEY="sua-secret-key"
export AWS_DEFAULT_REGION="eu-west-1"
```

#### Configuração do Taskfile

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
```

#### Criando a Instância EC2

```bash
# Navegar para o diretório fly-backend
cd fly-backend

# 1. Verificar credenciais AWS
task aws:check

# 2. Criar Security Group
task aws:sg:create

# 3. Criar Par de Chaves SSH
task aws:key:create

# 4. Criar Instância EC2
task aws:ec2:create

# Saída:
# Instance ID: i-0813ee3c789b40e51
# Public IP: 54.171.48.207
# SSH: ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@54.171.48.207
```

#### Script de User Data (Auto-Instalação)

A instância EC2 auto-instala WireGuard e dependências via user data:

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

# Criar diretório do edgeProxy
mkdir -p /opt/edgeproxy

# Instalar Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

---

### Configuração do WireGuard

#### Gerando Chaves

```bash
# Gerar chaves para EC2 (servidor central)
wg genkey > wireguard/ec2-private.key
cat wireguard/ec2-private.key | wg pubkey > wireguard/ec2-public.key

# Gerar chaves para cada região do Fly.io
for region in gru iad ord lax lhr fra cdg nrt sin syd; do
  wg genkey > wireguard/${region}-private.key
  cat wireguard/${region}-private.key | wg pubkey > wireguard/${region}-public.key
done
```

#### Configuração do Servidor WireGuard EC2

```ini
[Interface]
PrivateKey = <chave-privada-ec2>
Address = 10.50.0.1/24
ListenPort = 51820
PostUp = iptables -A FORWARD -i wg0 -j ACCEPT; iptables -t nat -A POSTROUTING -o ens5 -j MASQUERADE
PostDown = iptables -D FORWARD -i wg0 -j ACCEPT; iptables -t nat -D POSTROUTING -o ens5 -j MASQUERADE

# GRU - São Paulo (América do Sul)
[Peer]
PublicKey = <chave-publica-gru>
AllowedIPs = 10.50.1.1/32

# IAD - Virginia (América do Norte)
[Peer]
PublicKey = <chave-publica-iad>
AllowedIPs = 10.50.2.1/32

# ... (todos os 10 peers)
```

#### Iniciando o WireGuard

```bash
# Na EC2
sudo cp wg0.conf /etc/wireguard/
sudo wg-quick up wg0

# Verificar
sudo wg show
```

---

### Deploy do Backend Fly.io

#### Dockerfile com WireGuard

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

#### Script de Entrypoint

O script de entrypoint configura o WireGuard baseado na região do Fly.io:

```bash
#!/bin/bash
set -e

EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Mapear região para IP WireGuard
case "${FLY_REGION}" in
  gru) WG_IP="10.50.1.1/32"; WG_PRIVATE="<chave>" ;;
  iad) WG_IP="10.50.2.1/32"; WG_PRIVATE="<chave>" ;;
  # ... outras regiões
esac

# Criar configuração WireGuard
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

# Iniciar backend
exec ./backend
```

#### Deploy para Fly.io

```bash
cd fly-backend
fly deploy --remote-only

# Saída: 10/10 máquinas implantadas e saudáveis
```

---

### Topologia da Rede WireGuard

```
                           Malha WireGuard (10.50.x.x)
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐          ┌───────────────┐          ┌───────────────┐
│  EC2 Irlanda  │          │  Fly.io GRU   │          │  Fly.io NRT   │
│  10.50.0.1    │◄────────►│  10.50.1.1    │          │  10.50.4.1    │
│  (edgeProxy)  │          │  (backend)    │          │  (backend)    │
└───────────────┘          └───────────────┘          └───────────────┘
        │
        │ Todos os backends Fly.io conectam à EC2
        │
        ├──► 10.50.2.1 (IAD) ──► 10.50.2.2 (ORD) ──► 10.50.2.3 (LAX)
        ├──► 10.50.3.1 (LHR) ──► 10.50.3.2 (FRA) ──► 10.50.3.3 (CDG)
        └──► 10.50.4.2 (SIN) ──► 10.50.4.3 (SYD)
```

| Região | Código | IP WireGuard | Localização |
|--------|--------|--------------|-------------|
| América do Sul | GRU | 10.50.1.1 | São Paulo, Brasil |
| América do Norte | IAD | 10.50.2.1 | Virginia, EUA |
| América do Norte | ORD | 10.50.2.2 | Chicago, EUA |
| América do Norte | LAX | 10.50.2.3 | Los Angeles, EUA |
| Europa | LHR | 10.50.3.1 | Londres, Reino Unido |
| Europa | FRA | 10.50.3.2 | Frankfurt, Alemanha |
| Europa | CDG | 10.50.3.3 | Paris, França |
| Ásia Pacífico | NRT | 10.50.4.1 | Tóquio, Japão |
| Ásia Pacífico | SIN | 10.50.4.2 | Cingapura |
| Ásia Pacífico | SYD | 10.50.4.3 | Sydney, Austrália |

---

## Validação do Geo-Routing

Todos os 9 testes VPN rotearam corretamente para o backend esperado:

| Localização do Cliente | Esperado | Real | Resultado |
|------------------------|----------|------|-----------|
| 🇫🇷 França | CDG | CDG | ✅ |
| 🇩🇪 Alemanha | FRA | FRA | ✅ |
| 🇬🇧 Reino Unido | LHR | LHR | ✅ |
| 🇺🇸 Estados Unidos | IAD | IAD | ✅ |
| 🇯🇵 Japão | NRT | NRT | ✅ |
| 🇸🇬 Cingapura | SIN | SIN | ✅ |
| 🇦🇺 Austrália | SYD | SYD | ✅ |
| 🇧🇷 Brasil | GRU | GRU | ✅ |

---

## Executando Seus Próprios Testes

### Teste Rápido de Latência

```bash
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://54.171.48.207:8080/api/latency
done
```

### Verificar Geo-Routing

```bash
curl -s http://54.171.48.207:8080/api/info | jq .
# Retorna: {"region":"cdg","region_name":"Paris, France",...}
```

### Teste de Velocidade de Download

```bash
# Download de 1MB
curl -w "Velocidade: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=1048576"

# Download de 5MB
curl -w "Velocidade: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=5242880"
```

### Script Completo de Benchmark

Use o script fornecido em `scripts/benchmark.sh`:

```bash
./scripts/benchmark.sh http://54.171.48.207:8080
```

---

## Endpoints de Benchmark

| Endpoint | Descrição |
|----------|-----------|
| `/` | Banner ASCII art com info da região |
| `/api/info` | Info JSON do servidor (região, uptime, requisições) |
| `/api/latency` | Resposta mínima para teste de latência |
| `/api/download?size=N` | Teste de download (N bytes, máx 100MB) |
| `/api/upload` | Teste de upload (corpo POST) |
| `/api/stats` | Estatísticas do servidor |
| `/benchmark` | Página HTML interativa de benchmark |

---

## Conclusões

1. **Geo-Routing**: 100% de precisão roteando clientes para o backend regional correto
2. **WireGuard**: Túneis estáveis com todos os 10 backends globais
3. **Performance**: Latência escala previsivelmente com distância geográfica
4. **Confiabilidade**: Todos os testes passaram com resultados consistentes

### Deploy de Produção

Para produção, faça deploy de POPs edgeProxy em múltiplas regiões:

| Cenário | Latência Esperada |
|---------|-------------------|
| Cliente → POP Local → Backend Local | 5-20ms |
| Cliente → POP Local → Backend Regional | 20-50ms |
| Cliente → POP Local → Backend Remoto | 50-150ms |

A configuração de teste roteia todo o tráfego pela Irlanda. Um deploy em malha completa melhoraria significativamente a performance global.
