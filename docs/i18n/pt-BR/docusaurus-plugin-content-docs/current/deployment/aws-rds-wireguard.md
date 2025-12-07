---
sidebar_position: 2
---

# RDS via WireGuard

Este guia cobre a implantação de um banco de dados PostgreSQL no AWS RDS e o acesso seguro através de uma rede overlay WireGuard a partir de aplicações edge no Fly.io.

:::info Visão Geral
Esta arquitetura permite que aplicações edge no Fly.io acessem de forma segura um banco de dados PostgreSQL centralizado no AWS RDS através de um túnel WireGuard criptografado, usando uma instância EC2 como gateway NAT.
:::

---

## Arquitetura

![Arquitetura RDS WireGuard](/img/rds-wireguard-architecture.svg)

### Componentes

| Componente | Tipo | IP WireGuard | IP Público/Privado | Função |
|------------|------|--------------|-------------------|--------|
| **Fly.io App** | Container | 10.50.3.10/32 | dinâmico | Aplicação Go (contacts-api) |
| **EC2 Hub** | t3.micro | 10.50.0.1/24 | 34.240.78.199 | Gateway WireGuard + NAT |
| **RDS PostgreSQL** | db.t3.micro | - | 172.31.3.134 | Banco de dados |

### Portas

| Serviço | Porta | Protocolo | Descrição |
|---------|-------|-----------|-----------|
| WireGuard | 51820 | UDP | Túnel VPN criptografado |
| PostgreSQL | 5432 | TCP | Conexão com banco (via NAT) |
| HTTP API | 8080 | TCP | REST API da aplicação |

---

## Fluxo de Tráfego

![Fluxo de Tráfego](/img/rds-wireguard-traffic-flow.svg)

### Passo a Passo

1. **App conecta em `10.50.0.1:5432`** - Aplicação Go usa `DB_HOST=10.50.0.1`
2. **Kernel roteia via wg0** - Pacotes para `10.50.0.0/24` vão pela interface WireGuard
3. **Túnel UDP criptografado** - WireGuard encapsula e envia para EC2 (`34.240.78.199:51820`)
4. **EC2 recebe e descriptografa** - Interface wg0 recebe o pacote original
5. **iptables DNAT** - Reescreve destino de `10.50.0.1:5432` para `172.31.3.134:5432`
6. **iptables MASQUERADE** - Reescreve origem de `10.50.3.10` para `172.31.18.19` (IP do EC2)
7. **RDS processa query** - Banco vê requisição vindo do EC2
8. **Resposta retorna** - Caminho reverso através do NAT e WireGuard

---

## Roteamento NAT com iptables

![iptables NAT](/img/rds-wireguard-iptables.svg)

### Como Funciona o NAT

O EC2 Hub atua como gateway entre a rede WireGuard (10.50.x.x) e a VPC AWS (172.31.x.x). Isso é feito através de duas regras iptables:

#### 1. DNAT (Destination NAT) - PREROUTING

```bash
iptables -t nat -A PREROUTING -i wg0 -p tcp --dport 5432 \
  -j DNAT --to-destination 172.31.3.134:5432
```

**O que faz:**
- Intercepta pacotes TCP chegando na interface `wg0` destinados à porta 5432
- Reescreve endereço de destino de `10.50.0.1` para `172.31.3.134` (IP do RDS)
- Pacote agora pode ser roteado para o RDS na VPC

#### 2. SNAT (Source NAT) - POSTROUTING com MASQUERADE

```bash
iptables -t nat -A POSTROUTING -d 172.31.3.134 -p tcp --dport 5432 \
  -j MASQUERADE
```

**O que faz:**
- Intercepta pacotes indo para o RDS (172.31.3.134:5432)
- Reescreve endereço de origem de `10.50.3.10` para `172.31.18.19` (IP privado do EC2)
- RDS vê a requisição como vindo do EC2, não do Fly.io
- Respostas retornam ao EC2, que repassa via WireGuard

#### 3. IP Forwarding

```bash
sysctl -w net.ipv4.ip_forward=1
```

**Pré-requisito:** Habilita o kernel Linux a rotear pacotes entre interfaces (wg0 ↔ eth0).

#### 4. FORWARD Chain

```bash
iptables -A FORWARD -i wg0 -j ACCEPT
iptables -A FORWARD -o wg0 -j ACCEPT
```

**O que faz:** Permite que pacotes sejam encaminhados de/para a interface WireGuard.

### Transformação do Pacote

![Transformação do Pacote](/img/rds-wireguard-packet-transformation.svg)

---

## Implantação Passo a Passo

### Passo 1: Criar Security Group para RDS

```bash
# Criar security group
aws ec2 create-security-group \
  --region eu-west-1 \
  --group-name edgeproxy-rds-sg \
  --description "Security group for edgeProxy RDS" \
  --vpc-id vpc-0af2bf5af1b4460f7

# Permitir PostgreSQL (restringir em produção)
aws ec2 authorize-security-group-ingress \
  --region eu-west-1 \
  --group-id sg-06ad37f4e3ef49d7c \
  --protocol tcp \
  --port 5432 \
  --cidr 0.0.0.0/0
```

### Passo 2: Criar DB Subnet Group

```bash
aws rds create-db-subnet-group \
  --region eu-west-1 \
  --db-subnet-group-name edgeproxy-subnet-group \
  --db-subnet-group-description "Subnet group for edgeProxy RDS" \
  --subnet-ids subnet-0e5a3518878e1e16d subnet-0ae5feb18dd1f0bb7 subnet-0c8b89f0384c4c3f8
```

### Passo 3: Criar RDS PostgreSQL

```bash
aws rds create-db-instance \
  --region eu-west-1 \
  --db-instance-identifier edgeproxy-contacts-db \
  --db-instance-class db.t3.micro \
  --engine postgres \
  --engine-version 15 \
  --master-username postgres \
  --master-user-password EdgeProxy2024 \
  --allocated-storage 20 \
  --storage-type gp2 \
  --db-name contacts \
  --vpc-security-group-ids sg-06ad37f4e3ef49d7c \
  --db-subnet-group-name edgeproxy-subnet-group \
  --publicly-accessible \
  --backup-retention-period 1 \
  --no-multi-az
```

### Passo 4: Aguardar RDS Ficar Disponível

```bash
# Verificar status (leva ~5-10 minutos)
aws rds describe-db-instances \
  --region eu-west-1 \
  --db-instance-identifier edgeproxy-contacts-db \
  --query 'DBInstances[0].[DBInstanceStatus,Endpoint.Address]' \
  --output text

# Saída quando pronto:
# available    edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com
```

### Passo 5: Gerar Chaves WireGuard

```bash
# Chaves do EC2 Hub
wg genkey | tee ec2-wg-private.key | wg pubkey > ec2-wg-public.key
# Private: EHToyBXWXGOdh8dSngJnE9h6TGZ+VU6FLJDLnwq8Q2g=
# Public:  Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=

# Chaves do Fly.io App
wg genkey | tee fly-wg-private.key | wg pubkey > fly-wg-public.key
# Private: QHgup1SNdoXT2X1SH8OoKbIhQfayX/7+lGCDNcmyPHY=
# Public:  92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
```

### Passo 6: Criar Security Group para EC2

```bash
# Criar security group
aws ec2 create-security-group \
  --region eu-west-1 \
  --group-name edgeproxy-hub-sg \
  --description "Security group for edgeProxy WireGuard Hub" \
  --vpc-id vpc-0af2bf5af1b4460f7

# Permitir SSH
aws ec2 authorize-security-group-ingress \
  --region eu-west-1 \
  --group-id sg-06b10b1222b9f530f \
  --protocol tcp \
  --port 22 \
  --cidr 0.0.0.0/0

# Permitir WireGuard UDP
aws ec2 authorize-security-group-ingress \
  --region eu-west-1 \
  --group-id sg-06b10b1222b9f530f \
  --protocol udp \
  --port 51820 \
  --cidr 0.0.0.0/0
```

### Passo 7: Criar Key Pair SSH

```bash
aws ec2 create-key-pair \
  --region eu-west-1 \
  --key-name edgeproxy-hub \
  --query 'KeyMaterial' \
  --output text > edgeproxy-hub.pem

chmod 400 edgeproxy-hub.pem
```

### Passo 8: Script User Data (Cloud-Init)

Este script executa automaticamente quando o EC2 inicia, configurando WireGuard e NAT:

```bash
#!/bin/bash
# =============================================================================
# edgeProxy Hub - EC2 Ireland - WireGuard + NAT to RDS
# Executado via cloud-init (User Data) - 100% não-interativo
# =============================================================================
set -e
exec > >(tee /var/log/userdata.log) 2>&1
echo "=== edgeProxy Hub Setup Started: $(date) ==="

# Desabilitar prompts interativos
export DEBIAN_FRONTEND=noninteractive

# ============================================================================
# INSTALAÇÃO DE PACOTES
# ============================================================================
echo "=== Instalando pacotes ==="
apt-get update -qq
apt-get install -y -qq wireguard dnsutils net-tools

# ============================================================================
# CONFIGURAÇÃO WIREGUARD
# ============================================================================
echo "=== Criando configuração WireGuard ==="
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << 'WGEOF'
[Interface]
PrivateKey = EHToyBXWXGOdh8dSngJnE9h6TGZ+VU6FLJDLnwq8Q2g=
Address = 10.50.0.1/24
ListenPort = 51820

# Habilitar IP forwarding e regras FORWARD
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -j ACCEPT
PostUp = iptables -A FORWARD -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -j ACCEPT
PostDown = iptables -D FORWARD -o wg0 -j ACCEPT

# Peer: Fly.io contacts-api (região LHR)
[Peer]
PublicKey = 92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
AllowedIPs = 10.50.3.10/32
PersistentKeepalive = 25
WGEOF

chmod 600 /etc/wireguard/wg0.conf

echo "=== Iniciando WireGuard ==="
wg-quick up wg0
systemctl enable wg-quick@wg0

echo "=== Status WireGuard ==="
wg show

# ============================================================================
# CONFIGURAÇÃO NAT (iptables)
# ============================================================================
echo "=== Configurando NAT para RDS ==="

# Resolver IP do RDS (seguir CNAME e obter registro A)
RDS_IP=$(host edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com \
  | grep "has address" | awk '{print $4}' | head -1)
echo "IP do RDS resolvido: $RDS_IP"
echo "$RDS_IP" > /tmp/rds_ip.txt

if [ -z "$RDS_IP" ]; then
    echo "ERRO: Não foi possível resolver IP do RDS"
    exit 1
fi

# DNAT: Tráfego do WireGuard para 10.50.0.1:5432 → RDS
# Pacotes chegando na wg0 destinados à porta 5432 são redirecionados para o RDS
iptables -t nat -A PREROUTING -i wg0 -p tcp --dport 5432 \
  -j DNAT --to-destination ${RDS_IP}:5432

# SNAT/MASQUERADE: Garante que resposta retorne pelo EC2
# Pacotes indo para o RDS têm origem reescrita para IP do EC2
iptables -t nat -A POSTROUTING -d ${RDS_IP} -p tcp --dport 5432 \
  -j MASQUERADE

# ============================================================================
# PERSISTIR REGRAS
# ============================================================================
mkdir -p /etc/iptables
iptables-save > /etc/iptables/rules.v4

# Criar serviço systemd para restaurar regras no boot
cat > /etc/systemd/system/iptables-restore.service << 'SVCEOF'
[Unit]
Description=Restore iptables rules
After=network.target

[Service]
Type=oneshot
ExecStart=/sbin/iptables-restore /etc/iptables/rules.v4
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
SVCEOF

systemctl daemon-reload
systemctl enable iptables-restore.service

# ============================================================================
# VERIFICAÇÃO
# ============================================================================
echo "=== Testando conectividade com RDS ==="
nc -zv ${RDS_IP} 5432 && echo "Conexão RDS OK" || echo "Conexão RDS falhou"

echo "=== Status Final ==="
echo "EC2 WireGuard Public Key: Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14="
echo "EC2 WireGuard IP: 10.50.0.1"
echo "EC2 Public IP: $(curl -s http://169.254.169.254/latest/meta-data/public-ipv4)"
echo "RDS NAT Target: ${RDS_IP}:5432"
echo ""
echo "Regras NAT:"
iptables -t nat -L -n
echo ""
wg show
echo "=== Setup Completo: $(date) ==="
```

### Passo 9: Criar EC2

```bash
# Obter AMI Ubuntu 22.04 mais recente
AMI_ID=$(aws ec2 describe-images \
  --region eu-west-1 \
  --owners 099720109477 \
  --filters "Name=name,Values=ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*" \
  --query 'sort_by(Images, &CreationDate)[-1].ImageId' \
  --output text)

# Criar instância com user-data
aws ec2 run-instances \
  --region eu-west-1 \
  --image-id $AMI_ID \
  --instance-type t3.micro \
  --key-name edgeproxy-hub \
  --security-group-ids sg-06b10b1222b9f530f \
  --subnet-id subnet-0e5a3518878e1e16d \
  --associate-public-ip-address \
  --user-data file://ec2-userdata.sh \
  --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=edgeproxy-hub}]'
```

### Passo 10: Verificar Setup

```bash
# Obter IP público
aws ec2 describe-instances \
  --region eu-west-1 \
  --instance-ids i-079799a933a21ae5c \
  --query 'Reservations[0].Instances[0].PublicIpAddress' \
  --output text
# Saída: 34.240.78.199

# Aguardar ~90s e verificar logs
ssh -i edgeproxy-hub.pem ubuntu@34.240.78.199 \
  "sudo tail -30 /var/log/userdata.log"
```

**Saída esperada:**

```
=== Status WireGuard ===
interface: wg0
  public key: Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=
  private key: (hidden)
  listening port: 51820

peer: 92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
  allowed ips: 10.50.3.10/32
  persistent keepalive: every 25 seconds

=== Configurando NAT para RDS ===
IP do RDS resolvido: 172.31.3.134
Connection to 172.31.3.134 5432 port [tcp/postgresql] succeeded!
Conexão RDS OK
=== Setup Completo ===
```

---

## Aplicação Go (contacts-api)

### Estrutura do Projeto

```
contacts-api/
├── main.go           # Servidor REST API
├── seed.go           # Seeder de dados de teste
├── go.mod            # Módulo Go
├── go.sum            # Checksum de dependências
├── Dockerfile        # Build multi-stage com WireGuard
├── entrypoint.sh     # Setup WireGuard + iniciar app
└── fly.toml          # Configuração Fly.io
```

### main.go

API REST completa com PostgreSQL:

```go
package main

import (
    "database/sql"
    "encoding/json"
    "fmt"
    "log"
    "net/http"
    "os"
    "time"

    _ "github.com/lib/pq"
)

type Contact struct {
    ID        int       `json:"id"`
    Name      string    `json:"name"`
    Email     string    `json:"email"`
    Phone     *string   `json:"phone,omitempty"`
    Company   *string   `json:"company,omitempty"`
    Notes     *string   `json:"notes,omitempty"`
    CreatedAt time.Time `json:"created_at"`
    UpdatedAt time.Time `json:"updated_at"`
}

var db *sql.DB

func getEnv(key, defaultValue string) string {
    if value := os.Getenv(key); value != "" {
        return value
    }
    return defaultValue
}

func initDB() error {
    dbHost := getEnv("DB_HOST", "localhost")
    dbPort := getEnv("DB_PORT", "5432")
    dbUser := getEnv("DB_USER", "postgres")
    dbPassword := getEnv("DB_PASSWORD", "")
    dbName := getEnv("DB_NAME", "contacts")

    connStr := fmt.Sprintf(
        "host=%s port=%s user=%s password=%s dbname=%s sslmode=require",
        dbHost, dbPort, dbUser, dbPassword, dbName)

    var err error
    db, err = sql.Open("postgres", connStr)
    if err != nil {
        return err
    }

    db.SetMaxOpenConns(10)
    db.SetMaxIdleConns(5)
    db.SetConnMaxLifetime(time.Minute * 5)

    return db.Ping()
}

func healthHandler(w http.ResponseWriter, r *http.Request) {
    resp := map[string]string{
        "status":   "healthy",
        "database": "connected",
        "region":   getEnv("FLY_REGION", "local"),
        "db_host":  getEnv("DB_HOST", "localhost"),
    }

    if err := db.Ping(); err != nil {
        resp["status"] = "unhealthy"
        resp["database"] = err.Error()
    }

    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(resp)
}

// ... handlers completos no código fonte
```

### Endpoints da API

| Método | Endpoint | Descrição |
|--------|----------|-----------|
| GET | `/` | Informações do serviço |
| GET | `/health` | Health check com status do DB |
| GET | `/stats` | Estatísticas do banco |
| GET | `/contacts` | Listar contatos (paginado) |
| GET | `/contacts/:id` | Obter contato por ID |
| POST | `/contacts` | Criar contato |
| PUT | `/contacts/:id` | Atualizar contato |
| DELETE | `/contacts/:id` | Deletar contato |
| GET | `/contacts/search/:query` | Buscar contatos |

### Dockerfile

```dockerfile
FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY go.mod go.sum* ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o contacts-api .

FROM alpine:3.19

# Instalar WireGuard e iptables
RUN apk add --no-cache ca-certificates wireguard-tools iptables

WORKDIR /app
COPY --from=builder /app/contacts-api .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

EXPOSE 8080

CMD ["./entrypoint.sh"]
```

### entrypoint.sh

```bash
#!/bin/sh
set -e

echo "=== Iniciando WireGuard ==="

mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE_KEY}
Address = ${WG_ADDRESS:-10.50.3.10/32}

[Peer]
PublicKey = ${WG_PEER_PUBLIC_KEY}
Endpoint = ${WG_PEER_ENDPOINT}
AllowedIPs = 10.50.0.0/24
PersistentKeepalive = 25
EOF

chmod 600 /etc/wireguard/wg0.conf

wg-quick up wg0

echo "=== Status WireGuard ==="
wg show

echo "=== Testando conectividade com EC2 Hub ==="
ping -c 2 10.50.0.1 || echo "Ping falhou"

echo "=== Iniciando contacts-api ==="
exec ./contacts-api
```

### fly.toml

```toml
app = 'edgeproxy-contacts-api'
primary_region = 'lhr'

[build]

[env]
  PORT = "8080"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = 'stop'
  auto_start_machines = true
  min_machines_running = 0
  processes = ['app']

[[vm]]
  memory = '256mb'
  cpu_kind = 'shared'
  cpus = 1
```

### Deploy no Fly.io

```bash
# Configurar secrets
fly secrets set \
  WG_PRIVATE_KEY="QHgup1SNdoXT2X1SH8OoKbIhQfayX/7+lGCDNcmyPHY=" \
  WG_ADDRESS="10.50.3.10/32" \
  WG_PEER_PUBLIC_KEY="Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=" \
  WG_PEER_ENDPOINT="34.240.78.199:51820" \
  DB_HOST="10.50.0.1" \
  DB_PORT="5432" \
  DB_USER="postgres" \
  DB_PASSWORD="EdgeProxy2024" \
  DB_NAME="contacts" \
  -a edgeproxy-contacts-api

# Deploy
fly deploy -a edgeproxy-contacts-api
```

---

## Verificação

### Logs WireGuard no Fly.io

```bash
fly logs -a edgeproxy-contacts-api
```

**Saída esperada:**

```
=== Iniciando WireGuard ===
[#] ip link add wg0 type wireguard
[#] wg setconf wg0 /dev/fd/63
[#] ip -4 address add 10.50.3.10/32 dev wg0
[#] ip link set mtu 1340 up dev wg0
[#] ip -4 route add 10.50.0.0/24 dev wg0
=== Status WireGuard ===
interface: wg0
  public key: 92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
  private key: (hidden)
  listening port: 46637

peer: Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=
  endpoint: 34.240.78.199:51820
  allowed ips: 10.50.0.0/24
  latest handshake: Now
  transfer: 92 B received, 180 B sent
  persistent keepalive: every 25 seconds

=== Testando conectividade com EC2 Hub ===
PING 10.50.0.1 (10.50.0.1): 56 data bytes
64 bytes from 10.50.0.1: seq=0 ttl=64 time=10.491 ms
64 bytes from 10.50.0.1: seq=1 ttl=64 time=10.704 ms

=== Iniciando contacts-api ===
2025/12/07 13:22:20 Initializing Contacts API...
2025/12/07 13:22:21 Database connected
2025/12/07 13:22:21 Schema initialized
2025/12/07 13:22:21 Server starting on port 8080
```

### Testar Endpoints

```bash
# Health check
curl -s https://edgeproxy-contacts-api.fly.dev/health | jq .
```

```json
{
  "status": "healthy",
  "database": "connected",
  "region": "lhr",
  "db_host": "10.50.0.1"
}
```

```bash
# Estatísticas
curl -s https://edgeproxy-contacts-api.fly.dev/stats | jq .
```

```json
{
  "total_contacts": 500,
  "unique_companies": 33,
  "latest_contact": "2025-12-07T12:54:31.629798Z",
  "served_by": "lhr",
  "db_host": "10.50.0.1"
}
```

```bash
# Listar contatos
curl -s "https://edgeproxy-contacts-api.fly.dev/contacts?limit=3" | jq .
```

```json
{
  "contacts": [
    {
      "id": 115,
      "name": "Amanda Araujo",
      "email": "Amanda.Araujo@corporativo.com",
      "phone": "+55 11 93049-2680",
      "company": "Microservices Ltd",
      "notes": "Aguardando proposta comercial"
    }
  ],
  "limit": 3,
  "offset": 0,
  "served_by": "lhr",
  "total": 500
}
```

---

## Seeding do Banco

### seed.go

```go
// +build ignore

package main

import (
    "database/sql"
    "fmt"
    "log"
    "math/rand"
    "os"

    _ "github.com/lib/pq"
)

var firstNames = []string{
    "Ana", "Pedro", "Maria", "John", "Carla", "Lucas",
    "James", "Emma", "Hans", "François", "Marie",
}

var lastNames = []string{
    "Silva", "Santos", "Oliveira", "Smith", "Mueller", "Dubois",
}

var companies = []string{
    "TechCorp Brasil", "Cloud Nine Tech", "Kubernetes Masters",
    "AWS Partners", "DevSecOps Group",
}

func main() {
    connStr := fmt.Sprintf(
        "host=%s port=%s user=%s password=%s dbname=%s sslmode=require",
        os.Getenv("DB_HOST"), os.Getenv("DB_PORT"),
        os.Getenv("DB_USER"), os.Getenv("DB_PASSWORD"),
        os.Getenv("DB_NAME"))

    db, _ := sql.Open("postgres", connStr)
    defer db.Close()

    log.Println("Inserindo 500 contatos...")

    for i := 0; i < 500; i++ {
        firstName := firstNames[rand.Intn(len(firstNames))]
        lastName := lastNames[rand.Intn(len(lastNames))]

        db.Exec(`INSERT INTO contacts (name, email, company) VALUES ($1, $2, $3)`,
            firstName+" "+lastName,
            fmt.Sprintf("%s.%s@email.com", firstName, lastName),
            companies[rand.Intn(len(companies))])
    }

    log.Println("Concluído!")
}
```

### Executar Seeder

```bash
export DB_HOST="edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com"
export DB_PORT="5432"
export DB_USER="postgres"
export DB_PASSWORD="EdgeProxy2024"
export DB_NAME="contacts"

go run seed.go
```

---

## Segurança

### Recomendações para Produção

1. **Security Group do RDS**: Restringir apenas ao EC2 Hub
   ```bash
   aws ec2 authorize-security-group-ingress \
     --group-id sg-06ad37f4e3ef49d7c \
     --protocol tcp --port 5432 \
     --source-group sg-06b10b1222b9f530f
   ```

2. **Chaves WireGuard**: Armazenar no AWS Secrets Manager

3. **Criptografia RDS**: Habilitar criptografia em repouso
   ```bash
   --storage-encrypted --kms-key-id alias/aws/rds
   ```

4. **RDS Privado**: Desabilitar acesso público
   ```bash
   --no-publicly-accessible
   ```

---

## Estimativa de Custos (eu-west-1)

| Recurso | Tipo | Custo Mensal (USD) |
|---------|------|-------------------|
| RDS PostgreSQL | db.t3.micro | ~$15 |
| EC2 Hub | t3.micro | ~$8 |
| EBS Storage | 20GB gp2 | ~$2 |
| Transferência de Dados | ~10GB | ~$1 |
| **Total** | | **~$26/mês** |

---

## Troubleshooting

### Handshake WireGuard Não Acontece

```bash
# No EC2 Hub
sudo wg show

# Verificar:
# 1. Security group permite UDP 51820
# 2. App Fly.io está rodando
# 3. Chaves públicas correspondem em ambos os lados
```

### Conexão com Banco Falha

```bash
# No EC2 Hub
nc -zv 172.31.3.134 5432

# Verificar regras NAT
sudo iptables -t nat -L -n

# Verificar security group do RDS permite EC2
```

### App Fly.io Crasha

```bash
fly logs -a edgeproxy-contacts-api

# Problemas comuns:
# - Secrets faltando (WG_PRIVATE_KEY, DB_HOST, etc.)
# - Configuração WireGuard inválida
# - RDS não alcançável (verificar NAT)
```

---

## Documentação Relacionada

- [Rede Overlay WireGuard](../wireguard.md)
- [Deploy AWS EC2](./aws.md)
- [Deploy Fly.io](./flyio.md)
- [Visão Geral da Arquitetura](../architecture.md)

---

## Resumo

Esta arquitetura fornece:

- **Acesso Seguro**: Tráfego do banco criptografado via WireGuard
- **Performance Edge**: App roda próximo aos usuários (Fly.io LHR)
- **Dados Centralizados**: Única instância RDS na AWS Ireland
- **Auto-scaling**: Máquinas Fly.io escalam para zero quando ociosas
- **Baixo Custo**: ~$26/mês para infraestrutura completa

O túnel WireGuard garante que todo o tráfego do banco seja criptografado e roteado através de um caminho controlado, enquanto o gateway NAT no EC2 fornece conectividade transparente com a instância RDS privada.
