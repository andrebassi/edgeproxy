---
sidebar_position: 4
---

# Deploy GCP Compute Engine

Este guia cobre o deploy do edgeProxy como um nó POP (Point of Presence) no Google Cloud Platform na Ásia (região Hong Kong).

:::info Por que Hong Kong?
O GCP não tem data centers na China continental. Hong Kong (`asia-east2`) é a região mais próxima e oferece excelente latência para China, Sudeste Asiático e toda região APAC.
:::

## Pré-requisitos

```bash
# Instalar gcloud CLI
# https://cloud.google.com/sdk/docs/install

# Autenticar
gcloud auth login

# Definir projeto
gcloud config set project SEU_PROJECT_ID

# Habilitar Compute Engine API
gcloud services enable compute.googleapis.com

# Verificar
gcloud config list
```

## Visão Geral da Infraestrutura

![Infraestrutura GCP](/img/gcp-infrastructure.svg)

---

## Criação da Instância VM

### Usando Taskfile

```yaml
version: '3'

vars:
  GCP_PROJECT: seu-projeto-id
  GCP_REGION: asia-east2      # Hong Kong
  GCP_ZONE: asia-east2-a
  MACHINE_TYPE: e2-micro      # Elegível para free tier
  IMAGE_FAMILY: ubuntu-2204-lts
  IMAGE_PROJECT: ubuntu-os-cloud
  INSTANCE_NAME: edgeproxy-pop-hkg

tasks:
  gcp:check:
    desc: Verificar credenciais GCP
    cmds:
      - gcloud config list

  gcp:firewall:create:
    desc: Criar regras de firewall para edgeProxy
    cmds:
      - |
        gcloud compute firewall-rules create edgeproxy-allow-ssh \
          --allow tcp:22 \
          --source-ranges 0.0.0.0/0 \
          --target-tags edgeproxy \
          --description "Permitir SSH ao edgeProxy"

        gcloud compute firewall-rules create edgeproxy-allow-proxy \
          --allow tcp:8080 \
          --source-ranges 0.0.0.0/0 \
          --target-tags edgeproxy \
          --description "Permitir tráfego TCP edgeProxy"

        gcloud compute firewall-rules create edgeproxy-allow-wireguard \
          --allow udp:51820 \
          --source-ranges 0.0.0.0/0 \
          --target-tags edgeproxy \
          --description "Permitir WireGuard VPN"

  gcp:vm:create:
    desc: Criar instância VM para edgeProxy POP
    cmds:
      - |
        gcloud compute instances create {{.INSTANCE_NAME}} \
          --zone={{.GCP_ZONE}} \
          --machine-type={{.MACHINE_TYPE}} \
          --image-family={{.IMAGE_FAMILY}} \
          --image-project={{.IMAGE_PROJECT}} \
          --boot-disk-size=20GB \
          --boot-disk-type=pd-standard \
          --tags=edgeproxy \
          --metadata-from-file=startup-script=startup.sh

        echo "Instância criada. Obtendo IP externo..."
        gcloud compute instances describe {{.INSTANCE_NAME}} \
          --zone={{.GCP_ZONE}} \
          --format='get(networkInterfaces[0].accessConfigs[0].natIP)'

  gcp:vm:ssh:
    desc: SSH na VM
    cmds:
      - gcloud compute ssh {{.INSTANCE_NAME}} --zone={{.GCP_ZONE}}

  gcp:vm:delete:
    desc: Deletar instância VM
    cmds:
      - gcloud compute instances delete {{.INSTANCE_NAME}} --zone={{.GCP_ZONE}} --quiet
```

### Criação Passo a Passo

```bash
# 1. Verificar credenciais GCP
task gcp:check

# 2. Criar regras de firewall
task gcp:firewall:create

# 3. Criar instância VM
task gcp:vm:create

# Output:
# Created [https://www.googleapis.com/compute/v1/projects/.../zones/asia-east2-a/instances/edgeproxy-pop-hkg]
# IP Externo: 34.92.xxx.xxx
```

---

## Compilação e Deploy do edgeProxy

### Cross-Compile para Linux (a partir de macOS/Linux)

Compile o binário localmente usando Docker para deploy mais rápido:

```bash
# Build para Linux amd64 usando Docker
docker run --rm --platform linux/amd64 \
  -v "$(pwd)":/app -w /app \
  rust:latest \
  bash -c "apt-get update && apt-get install -y pkg-config libssl-dev && cargo build --release"

# O binário estará em target/release/edge-proxy (~16MB)
ls -la target/release/edge-proxy
```

### Deploy para VM GCP

```bash
# Copiar binário e banco de dados de roteamento para a VM
gcloud compute scp target/release/edge-proxy edgeproxy-pop-hkg:/tmp/ --zone=asia-east2-a
gcloud compute scp routing.db edgeproxy-pop-hkg:/tmp/ --zone=asia-east2-a

# SSH e configuração na VM
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="
  sudo mkdir -p /opt/edgeproxy
  sudo mv /tmp/edge-proxy /opt/edgeproxy/
  sudo mv /tmp/routing.db /opt/edgeproxy/
  sudo chmod +x /opt/edgeproxy/edge-proxy
"
```

### Criar Serviço systemd

```bash
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="
cat | sudo tee /etc/systemd/system/edgeproxy.service << 'EOF'
[Unit]
Description=edgeProxy TCP Proxy
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/edgeproxy
Environment=EDGEPROXY_REGION=ap
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_DB_PATH=/opt/edgeproxy/routing.db
ExecStart=/opt/edgeproxy/edge-proxy
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable edgeproxy
sudo systemctl start edgeproxy
sudo systemctl status edgeproxy
"
```

### Verificar Deploy

```bash
# Verificar status do serviço
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="sudo systemctl status edgeproxy"

# Verificar logs
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="sudo journalctl -u edgeproxy -n 20"

# Testar conectividade (da máquina local)
nc -zv <IP_EXTERNO> 8080
```

---

## Configuração WireGuard

### Gerar Chaves para POP HKG

```bash
# Gerar chaves para GCP Hong Kong
wg genkey > wireguard/hkg-private.key
cat wireguard/hkg-private.key | wg pubkey > wireguard/hkg-public.key

# Exibir chaves
echo "Private: $(cat wireguard/hkg-private.key)"
echo "Public: $(cat wireguard/hkg-public.key)"
```

### Config do Servidor GCP (Modo Cliente)

A instância GCP conecta ao servidor central EC2:

```ini
# /etc/wireguard/wg0.conf
[Interface]
PrivateKey = <hkg-private-key>
Address = 10.50.5.1/24

[Peer]
# EC2 Irlanda (Servidor Central)
PublicKey = <ec2-public-key>
Endpoint = 54.171.48.207:51820
AllowedIPs = 10.50.0.0/16
PersistentKeepalive = 25
```

### Atualizar Servidor Central EC2

Adicionar o peer HKG na config WireGuard do EC2:

```ini
# Adicionar ao /etc/wireguard/wg0.conf no EC2

# HKG - Hong Kong (Ásia)
[Peer]
PublicKey = <hkg-public-key>
AllowedIPs = 10.50.5.1/32
```

Depois recarregar:

```bash
# No EC2
sudo wg syncconf wg0 <(wg-quick strip wg0)

# Verificar
sudo wg show
```

---

## Topologia de Rede

### Alocação de IPs Atualizada

| Região | Código | IP WireGuard | Localização | Provedor |
|--------|--------|--------------|-------------|----------|
| **Central** | EC2 | 10.50.0.1 | Irlanda | AWS |
| América do Sul | GRU | 10.50.1.1 | São Paulo | Fly.io |
| América do Norte | IAD | 10.50.2.1 | Virginia | Fly.io |
| Europa | LHR | 10.50.3.1 | Londres | Fly.io |
| Ásia Pacífico | NRT | 10.50.4.1 | Tóquio | Fly.io |
| Ásia Pacífico | SIN | 10.50.4.2 | Singapura | Fly.io |
| **Ásia (Novo)** | **HKG** | **10.50.5.1** | **Hong Kong** | **GCP** |

---

## Testando Geo-Routing da China

### Usando VPN para Simular Localização na China

```bash
# Conectar a um servidor VPN na China (ex: Shenzhen, Shanghai, Beijing)

# Testar geo-routing
curl -s http://34.92.xxx.xxx:8080/api/info | jq .

# Resposta esperada:
{
  "region": "hkg",
  "region_name": "Hong Kong",
  "backend": "hkg-node-1",
  "client_country": "CN",
  "latency_ms": 15
}
```

### Teste de Latência

```bash
# Teste rápido de latência via VPN China
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://34.92.xxx.xxx:8080/api/latency
done
```

### Performance Esperada

| Localização do Cliente | Backend Esperado | Latência Esperada |
|------------------------|------------------|-------------------|
| China (Shenzhen) | HKG | 10-30ms |
| China (Beijing) | HKG | 30-50ms |
| Japão (Tóquio) | NRT ou HKG | 40-60ms |
| Singapura | SIN ou HKG | 30-50ms |

---

## Regras de Firewall

| Nome da Regra | Porta | Protocolo | Origem | Descrição |
|---------------|-------|-----------|--------|-----------|
| edgeproxy-allow-ssh | 22 | TCP | Seu IP | Acesso SSH |
| edgeproxy-allow-proxy | 8080 | TCP | 0.0.0.0/0 | edgeProxy TCP |
| edgeproxy-allow-wireguard | 51820 | UDP | 0.0.0.0/0 | WireGuard |

### Restringindo SSH

```bash
# Obter seu IP
MY_IP=$(curl -s ifconfig.me)

# Atualizar regra de firewall
gcloud compute firewall-rules update edgeproxy-allow-ssh \
  --source-ranges ${MY_IP}/32
```

---

## Monitoramento

### Verificar Status do WireGuard

```bash
# SSH na VM
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a

# Mostrar status do WireGuard
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

## Estimativa de Custos

| Recurso | Especificação | Custo Mensal (USD) |
|---------|---------------|-------------------|
| Instância VM | e2-micro (2 vCPU, 1GB) | ~$6.11 |
| Disco de Boot | 20GB Standard | ~$0.80 |
| Egress de Rede | 10GB/mês | ~$1.20 |
| **Total** | | **~$8/mês** |

:::tip Free Tier
O GCP oferece 1 instância e2-micro gratuita por mês em us-west1, us-central1 e us-east1. Hong Kong não está no free tier, mas os custos são mínimos.
:::

---

## Troubleshooting

### WireGuard Não Conecta

```bash
# Verificar interface
ip addr show wg0

# Verificar se a porta está aberta
sudo netstat -ulnp | grep 51820

# Testar conectividade com EC2
ping 10.50.0.1
```

### VM Não Acessível

```bash
# Verificar regras de firewall
gcloud compute firewall-rules list --filter="name~edgeproxy"

# Verificar status da VM
gcloud compute instances describe edgeproxy-pop-hkg --zone=asia-east2-a

# Verificar saída do console serial
gcloud compute instances get-serial-port-output edgeproxy-pop-hkg --zone=asia-east2-a
```

---

## Próximos Passos

- [Deploy AWS EC2](./aws) - POP central na Irlanda
- [Deploy Fly.io](./flyio) - Deploy global de backends
- [Benchmarks](../benchmark) - Testes de performance
