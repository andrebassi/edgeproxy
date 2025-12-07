---
sidebar_position: 8
---

# Benchmark

Este guia documenta a configuração completa e os resultados do benchmark de acesso ao PostgreSQL RDS a partir de 10 regiões globais do Fly.io através da rede overlay WireGuard.

## Visão Geral

O benchmark mede latências de INSERT e SELECT dos nós edge do Fly.io para o AWS RDS PostgreSQL na Irlanda (eu-west-1), roteando através de um hub WireGuard no EC2.

![Arquitetura do Benchmark RDS](/img/rds-benchmark-architecture.svg)

## Resultados do Benchmark

| Bandeira | Região | Localização | Insert (ms) | Read (ms) | Rank |
|:--------:|:------:|-------------|:-----------:|:---------:|:----:|
| 🇬🇧 | lhr | Londres | **25.39** | **11.89** | 1 |
| 🇫🇷 | cdg | Paris | 37.61 | 18.48 | 2 |
| 🇩🇪 | fra | Frankfurt | 54.17 | 26.69 | 3 |
| 🇺🇸 | iad | Virginia | 173.16 | 86.10 | 4 |
| 🇺🇸 | ord | Chicago | 244.20 | 121.68 | 5 |
| 🇺🇸 | lax | Los Angeles | 285.52 | 138.19 | 6 |
| 🇸🇬 | sin | Singapura | 332.65 | 165.96 | 7 |
| 🇧🇷 | gru | São Paulo | 396.09 | 197.72 | 8 |
| 🇯🇵 | nrt | Tóquio | 523.35 | 261.36 | 9 |
| 🇦🇺 | syd | Sydney | 538.58 | 268.85 | 10 |

### Resultados Visuais

![Resultados do Benchmark RDS](/img/rds-benchmark-results.svg)

### Principais Conclusões

- **Londres (LHR)** tem a melhor latência (~25ms INSERT, ~12ms READ) - mais próximo do RDS na Irlanda
- **Regiões europeias** (LHR, CDG, FRA) dominam o top 3 devido à proximidade geográfica
- **Costa Leste dos EUA** (IAD) ~173ms - atravessando o Atlântico
- **Regiões APAC** (NRT, SYD) têm as maiores latências (~520-540ms) - distância geográfica máxima

---

## Passo 1: Configuração do Backend Fly.io

### 1.1 Criar o Backend em Go

Crie `fly-backend/main.go`:

```go
package main

import (
    "database/sql"
    "encoding/json"
    "fmt"
    "net/http"
    "os"
    "strconv"
    "time"

    _ "github.com/lib/pq"
)

var (
    region   string
    hostname string
    db       *sql.DB
)

func main() {
    region = os.Getenv("FLY_REGION")
    if region == "" {
        region = "local"
    }

    hostname = os.Getenv("FLY_ALLOC_ID")
    if hostname == "" {
        hostname, _ = os.Hostname()
    }
    if len(hostname) > 8 {
        hostname = hostname[:8]
    }

    port := os.Getenv("PORT")
    if port == "" {
        port = "8080"
    }

    // Inicializar banco de dados
    initDB()

    // Endpoints do benchmark RDS
    http.HandleFunc("/api/rds/benchmark", handleRDSBenchmark)
    http.HandleFunc("/api/rds/health", handleRDSHealth)
    http.HandleFunc("/api/info", handleInfo)

    fmt.Printf("Backend rodando na região [%s] na porta %s\n", region, port)
    http.ListenAndServe(":"+port, nil)
}

func getEnv(key, defaultValue string) string {
    if value := os.Getenv(key); value != "" {
        return value
    }
    return defaultValue
}

func initDB() {
    dbHost := getEnv("DB_HOST", "")
    if dbHost == "" {
        fmt.Println("DB_HOST não configurado, benchmark RDS desabilitado")
        return
    }

    dbPort := getEnv("DB_PORT", "5432")
    dbUser := getEnv("DB_USER", "postgres")
    dbPassword := getEnv("DB_PASSWORD", "")
    dbName := getEnv("DB_NAME", "contacts")

    connStr := fmt.Sprintf("host=%s port=%s user=%s password=%s dbname=%s sslmode=disable",
        dbHost, dbPort, dbUser, dbPassword, dbName)

    var err error
    db, err = sql.Open("postgres", connStr)
    if err != nil {
        fmt.Printf("Falha ao abrir banco de dados: %v\n", err)
        return
    }

    db.SetMaxOpenConns(10)
    db.SetMaxIdleConns(5)
    db.SetConnMaxLifetime(time.Minute * 5)

    if err := db.Ping(); err != nil {
        fmt.Printf("Falha ao pingar banco de dados: %v\n", err)
        db = nil
        return
    }

    fmt.Printf("Banco de dados conectado: %s\n", dbHost)
}

func handleRDSBenchmark(w http.ResponseWriter, r *http.Request) {
    w.Header().Set("Content-Type", "application/json")
    w.Header().Set("X-Fly-Region", region)

    dbHost := getEnv("DB_HOST", "não configurado")

    if db == nil {
        json.NewEncoder(w).Encode(map[string]interface{}{
            "error":   "Banco de dados não configurado",
            "region":  region,
            "db_host": dbHost,
        })
        return
    }

    iterations := 10
    if iter := r.URL.Query().Get("iterations"); iter != "" {
        if n, err := strconv.Atoi(iter); err == nil && n > 0 && n <= 100 {
            iterations = n
        }
    }

    readLatencies := make([]float64, iterations)
    insertLatencies := make([]float64, iterations)

    // Executar benchmarks de leitura (SELECT COUNT)
    for i := 0; i < iterations; i++ {
        start := time.Now()
        var count int
        db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&count)
        readLatencies[i] = float64(time.Since(start).Microseconds()) / 1000.0
    }

    // Executar benchmarks de inserção
    for i := 0; i < iterations; i++ {
        start := time.Now()
        name := fmt.Sprintf("Bench-%s-%d-%d", region, time.Now().UnixNano(), i)
        email := fmt.Sprintf("bench-%d@test.local", time.Now().UnixNano())
        db.Exec(`INSERT INTO contacts (name, email, notes) VALUES ($1, $2, $3)`,
            name, email, "Benchmark")
        insertLatencies[i] = float64(time.Since(start).Microseconds()) / 1000.0
    }

    // Calcular estatísticas
    calcStats := func(latencies []float64) (avg, min, max float64) {
        if len(latencies) == 0 {
            return 0, 0, 0
        }
        min = latencies[0]
        max = latencies[0]
        var sum float64
        for _, l := range latencies {
            sum += l
            if l < min {
                min = l
            }
            if l > max {
                max = l
            }
        }
        avg = sum / float64(len(latencies))
        return
    }

    readAvg, readMin, readMax := calcStats(readLatencies)
    insertAvg, insertMin, insertMax := calcStats(insertLatencies)

    result := map[string]interface{}{
        "region":           region,
        "db_host":          dbHost,
        "iterations":       iterations,
        "read_avg_ms":      readAvg,
        "read_min_ms":      readMin,
        "read_max_ms":      readMax,
        "insert_avg_ms":    insertAvg,
        "insert_min_ms":    insertMin,
        "insert_max_ms":    insertMax,
        "read_latencies":   readLatencies,
        "insert_latencies": insertLatencies,
        "timestamp":        time.Now().UTC().Format(time.RFC3339),
    }

    json.NewEncoder(w).Encode(result)
}

func handleRDSHealth(w http.ResponseWriter, r *http.Request) {
    w.Header().Set("Content-Type", "application/json")

    result := map[string]interface{}{
        "region":  region,
        "db_host": getEnv("DB_HOST", "não configurado"),
    }

    if db == nil {
        result["status"] = "desabilitado"
    } else if err := db.Ping(); err != nil {
        result["status"] = "erro"
        result["message"] = err.Error()
    } else {
        result["status"] = "conectado"
    }

    json.NewEncoder(w).Encode(result)
}

func handleInfo(w http.ResponseWriter, r *http.Request) {
    w.Header().Set("Content-Type", "application/json")

    json.NewEncoder(w).Encode(map[string]interface{}{
        "region":   region,
        "hostname": hostname,
    })
}
```

### 1.2 Criar go.mod

```go
module fly-backend

go 1.21

require github.com/lib/pq v1.10.9
```

### 1.3 Criar Dockerfile

```dockerfile
FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download
COPY main.go .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o backend main.go

FROM alpine:3.19
RUN apk --no-cache add ca-certificates wireguard-tools iptables ip6tables iproute2 bash
WORKDIR /app
COPY --from=builder /app/backend .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh
CMD ["./entrypoint.sh"]
```

### 1.4 Criar entrypoint.sh (WireGuard + Backend)

```bash
#!/bin/bash
set -e

echo "=== Iniciando WireGuard + Backend ==="
echo "FLY_REGION: ${FLY_REGION}"

# Endpoint e chave pública do EC2 (hub)
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Mapear região para IP WireGuard e chave privada
case "${FLY_REGION}" in
  gru)
    WG_IP="10.50.1.1/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_GRU"
    ;;
  iad)
    WG_IP="10.50.2.1/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_IAD"
    ;;
  ord)
    WG_IP="10.50.2.2/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_ORD"
    ;;
  lax)
    WG_IP="10.50.2.3/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_LAX"
    ;;
  lhr)
    WG_IP="10.50.3.1/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_LHR"
    ;;
  fra)
    WG_IP="10.50.3.2/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_FRA"
    ;;
  cdg)
    WG_IP="10.50.3.3/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_CDG"
    ;;
  nrt)
    WG_IP="10.50.4.1/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_NRT"
    ;;
  sin)
    WG_IP="10.50.4.2/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_SIN"
    ;;
  syd)
    WG_IP="10.50.4.3/32"
    WG_PRIVATE="SUA_CHAVE_PRIVADA_SYD"
    ;;
  *)
    echo "Região desconhecida: ${FLY_REGION}, pulando WireGuard"
    exec ./backend
    ;;
esac

echo "Configurando WireGuard com IP: ${WG_IP}"

# Criar configuração WireGuard
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << WGEOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
# EC2 Irlanda (hub)
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/24, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24, 10.50.4.0/24
PersistentKeepalive = 25
WGEOF

# Iniciar WireGuard
echo "Iniciando interface WireGuard..."
wg-quick up wg0 || echo "WireGuard falhou (pode precisar da capability NET_ADMIN)"

# Mostrar status
wg show || true

echo "Iniciando servidor backend..."
exec ./backend
```

### 1.5 Criar fly.toml

```toml
app = 'edgeproxy-backend'
primary_region = 'gru'

[build]

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

---

## Passo 2: Deploy no Fly.io

### 2.1 Criar o app

```bash
fly apps create edgeproxy-backend
```

### 2.2 Configurar secrets do banco de dados

```bash
fly secrets set \
  DB_HOST=10.50.0.1 \
  DB_PORT=5432 \
  DB_USER=contacts_user \
  DB_PASSWORD=sua_senha \
  DB_NAME=contacts \
  -a edgeproxy-backend
```

### 2.3 Deploy em todas as regiões

```bash
# Fazer deploy do app
fly deploy

# Escalar para todas as 10 regiões
fly scale count 1 --region gru,iad,ord,lax,lhr,fra,cdg,nrt,sin,syd -a edgeproxy-backend
```

---

## Passo 3: Configuração do AWS RDS

### 3.1 Criar Instância RDS

```bash
aws rds create-db-instance \
  --db-instance-identifier edgeproxy-db \
  --db-instance-class db.t3.micro \
  --engine postgres \
  --engine-version 15.4 \
  --master-username postgres \
  --master-user-password SUA_SENHA \
  --allocated-storage 20 \
  --vpc-security-group-ids sg-xxxxxxxx \
  --availability-zone eu-west-1a \
  --publicly-accessible \
  --no-multi-az
```

### 3.2 Desabilitar Requisito de SSL

Criar um grupo de parâmetros personalizado:

```bash
aws rds create-db-parameter-group \
  --db-parameter-group-name edgeproxy-nossl \
  --db-parameter-group-family postgres15 \
  --description "Desabilitar SSL para conexões WireGuard"

aws rds modify-db-parameter-group \
  --db-parameter-group-name edgeproxy-nossl \
  --parameters "ParameterName=rds.force_ssl,ParameterValue=0,ApplyMethod=pending-reboot"

aws rds modify-db-instance \
  --db-instance-identifier edgeproxy-db \
  --db-parameter-group-name edgeproxy-nossl \
  --apply-immediately

aws rds reboot-db-instance --db-instance-identifier edgeproxy-db
```

### 3.3 Criar Banco de Dados e Tabela

```sql
CREATE DATABASE contacts;

\c contacts

CREATE TABLE contacts (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    notes TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE USER contacts_user WITH PASSWORD 'sua_senha';
GRANT ALL PRIVILEGES ON DATABASE contacts TO contacts_user;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO contacts_user;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO contacts_user;
```

---

## Passo 4: Configuração do Hub WireGuard no EC2

### 4.1 User Data do EC2 (cloud-init)

```bash
#!/bin/bash
set -e

# Instalar WireGuard
apt-get update && apt-get install -y wireguard

# Habilitar IP forwarding
echo "net.ipv4.ip_forward = 1" >> /etc/sysctl.conf
sysctl -p

# Gerar chaves WireGuard
wg genkey | tee /etc/wireguard/privatekey | wg pubkey > /etc/wireguard/publickey
PRIVATE_KEY=$(cat /etc/wireguard/privatekey)

# Criar configuração WireGuard
cat > /etc/wireguard/wg0.conf << 'EOF'
[Interface]
PrivateKey = PRIVATE_KEY_AQUI
Address = 10.50.0.1/24
ListenPort = 51820
PostUp = iptables -t nat -A POSTROUTING -o ens5 -j MASQUERADE
PostDown = iptables -t nat -D POSTROUTING -o ens5 -j MASQUERADE

# Peers do Fly.io (adicionar após gerar as chaves deles)
[Peer]
# fly-gru-1
PublicKey = FLY_GRU_PUBKEY
AllowedIPs = 10.50.1.1/32

[Peer]
# fly-iad-1
PublicKey = FLY_IAD_PUBKEY
AllowedIPs = 10.50.2.1/32

# ... adicionar todas as 10 regiões
EOF

# Substituir placeholder
sed -i "s|PRIVATE_KEY_AQUI|$PRIVATE_KEY|" /etc/wireguard/wg0.conf

# Iniciar WireGuard
systemctl enable wg-quick@wg0
systemctl start wg-quick@wg0

# DNAT para acesso ao RDS (rotear 10.50.0.1:5432 para o RDS)
RDS_IP="172.31.x.x"  # IP privado do seu RDS
iptables -t nat -A PREROUTING -d 10.50.0.1 -p tcp --dport 5432 -j DNAT --to-destination $RDS_IP:5432
iptables -t nat -A POSTROUTING -d $RDS_IP -p tcp --dport 5432 -j MASQUERADE
```

### 4.2 Regras do Security Group

**Security Group do EC2:**
- Entrada: UDP 51820 de 0.0.0.0/0 (WireGuard)
- Entrada: TCP 22 do seu IP (SSH)
- Saída: Todo o tráfego

**Security Group do RDS:**
- Entrada: TCP 5432 do Security Group do EC2
- Entrada: TCP 5432 do IP privado do EC2

---

## Passo 5: Executando o Benchmark

### 5.1 Testar a partir do EC2 (via WireGuard)

```bash
# Testar cada backend diretamente
for backend in "gru:10.50.1.1" "iad:10.50.2.1" "lhr:10.50.3.1"; do
  region=$(echo $backend | cut -d: -f1)
  ip=$(echo $backend | cut -d: -f2)
  echo "=== $region ==="
  curl -s http://$ip:8080/api/rds/benchmark | jq '{region, insert_avg_ms, read_avg_ms}'
done
```

### 5.2 Testar via edgeProxy (geo-routing)

```bash
# O edge-proxy irá rotear baseado no IP do cliente
curl -s http://54.171.48.207:8080/api/rds/benchmark | jq .
```

### 5.3 Script Completo de Benchmark

```bash
#!/bin/bash
echo "=== Benchmark RDS: Fly.io → AWS RDS Irlanda ==="
echo ""
printf "| %-8s | %-6s | %-13s | %-11s | %-9s |\n" "Bandeira" "Região" "Localização" "Insert (ms)" "Read (ms)"
echo "|----------|--------|---------------|-------------|-----------|"

for backend in \
  "🇧🇷:gru:10.50.1.1:São Paulo" \
  "🇺🇸:iad:10.50.2.1:Virginia" \
  "🇺🇸:ord:10.50.2.2:Chicago" \
  "🇺🇸:lax:10.50.2.3:Los Angeles" \
  "🇬🇧:lhr:10.50.3.1:Londres" \
  "🇩🇪:fra:10.50.3.2:Frankfurt" \
  "🇫🇷:cdg:10.50.3.3:Paris" \
  "🇯🇵:nrt:10.50.4.1:Tóquio" \
  "🇸🇬:sin:10.50.4.2:Singapura" \
  "🇦🇺:syd:10.50.4.3:Sydney"
do
  flag=$(echo $backend | cut -d: -f1)
  region=$(echo $backend | cut -d: -f2)
  ip=$(echo $backend | cut -d: -f3)
  location=$(echo $backend | cut -d: -f4)

  result=$(curl -s --connect-timeout 10 http://$ip:8080/api/rds/benchmark 2>/dev/null)

  if [ -n "$result" ]; then
    insert=$(echo $result | jq -r '.insert_avg_ms' | xargs printf "%.2f")
    read=$(echo $result | jq -r '.read_min_ms' | xargs printf "%.2f")
    printf "| %-8s | %-6s | %-13s | %11s | %9s |\n" "$flag" "$region" "$location" "$insert" "$read"
  else
    printf "| %-8s | %-6s | %-13s | %11s | %9s |\n" "$flag" "$region" "$location" "TIMEOUT" "TIMEOUT"
  fi
done
```

---

## Referência da API

### GET /api/rds/benchmark

Executa benchmarks de INSERT e SELECT no banco de dados RDS configurado.

**Parâmetros de Query:**
- `iterations` (opcional): Número de iterações (1-100, padrão: 10)

**Resposta:**
```json
{
  "region": "lhr",
  "db_host": "10.50.0.1",
  "iterations": 10,
  "read_avg_ms": 18.72,
  "read_min_ms": 11.89,
  "read_max_ms": 65.45,
  "insert_avg_ms": 25.39,
  "insert_min_ms": 24.60,
  "insert_max_ms": 29.04,
  "read_latencies": [65.45, 12.10, 11.99, ...],
  "insert_latencies": [24.97, 25.62, 24.60, ...],
  "timestamp": "2025-12-07T15:48:02Z"
}
```

### GET /api/rds/health

Retorna o status da conexão com o banco de dados.

**Resposta:**
```json
{
  "region": "lhr",
  "db_host": "10.50.0.1",
  "status": "conectado"
}
```

---

## Solução de Problemas

### Problema: "no pg_hba.conf entry for host"

**Causa:** O RDS exige SSL por padrão.

**Solução:** Desabilitar requisito de SSL:
```bash
aws rds modify-db-parameter-group \
  --db-parameter-group-name edgeproxy-nossl \
  --parameters "ParameterName=rds.force_ssl,ParameterValue=0,ApplyMethod=pending-reboot"
```

### Problema: Timeout de conexão do Fly.io

**Causa:** WireGuard não está conectando ao hub EC2.

**Solução:**
1. Verificar se a chave pública do EC2 está correta no entrypoint.sh
2. Verificar se o security group do EC2 permite UDP 51820
3. Verificar regras NAT no EC2:
```bash
iptables -t nat -L -n -v
```

### Problema: Resposta "Banco de dados não configurado"

**Causa:** Secret DB_HOST não configurado.

**Solução:**
```bash
fly secrets set DB_HOST=10.50.0.1 -a edgeproxy-backend
```

---

## Alocação de IPs WireGuard

| Região | IP WG Fly.io | Propósito |
|--------|-------------|-----------|
| EC2 Hub | 10.50.0.1 | Hub WireGuard + NAT para RDS |
| gru | 10.50.1.1 | América do Sul |
| iad | 10.50.2.1 | EUA Leste |
| ord | 10.50.2.2 | EUA Central |
| lax | 10.50.2.3 | EUA Oeste |
| lhr | 10.50.3.1 | Europa (Reino Unido) |
| fra | 10.50.3.2 | Europa (Alemanha) |
| cdg | 10.50.3.3 | Europa (França) |
| nrt | 10.50.4.1 | Ásia (Japão) |
| sin | 10.50.4.2 | Ásia (Singapura) |
| syd | 10.50.4.3 | Oceania (Austrália) |

---

## Dicas de Otimização de Performance

1. **Use connection pooling**: O backend Go usa `SetMaxOpenConns(10)` e `SetMaxIdleConns(5)`

2. **Conexões persistentes**: WireGuard `PersistentKeepalive = 25` mantém os túneis ativos

3. **Coloque o RDS na mesma região do hub**: EC2 e RDS em eu-west-1 minimiza latência interna

4. **Considere read replicas**: Para workloads com muita leitura, deploy de réplicas de leitura do RDS em outras regiões
