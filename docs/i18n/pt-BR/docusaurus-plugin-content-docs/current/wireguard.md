---
sidebar_position: 4
---

# Rede Overlay WireGuard

Este documento cobre a rede overlay WireGuard que conecta todos os POPs e backends do edgeProxy globalmente.

:::info Por que WireGuard?
O WireGuard fornece uma rede overlay segura e de alta performance que permite aos POPs do edgeProxy rotear tráfego para backends independentemente de sua localização física ou topologia de rede.
:::

---

## Arquitetura de Rede

### Visão Geral

![Rede WireGuard Full Mesh](/img/wireguard-full-mesh.svg)

### Esquema de Alocação de IPs

| Subnet | Região | Descrição |
|--------|--------|-----------|
| `10.50.0.0/24` | Central | EC2 Irlanda (Hub) |
| `10.50.1.0/24` | América do Sul | GRU (São Paulo) |
| `10.50.2.0/24` | América do Norte | IAD, ORD, LAX |
| `10.50.3.0/24` | Europa | LHR, FRA, CDG |
| `10.50.4.0/24` | Ásia Pacífico | NRT, SIN, SYD |
| `10.50.5.0/24` | Ásia Pacífico | HKG POP (GCP) |

### Atribuições de IP dos Backends

| Backend | Código | IP WireGuard | Localização |
|---------|--------|--------------|-------------|
| EC2 Irlanda | - | 10.50.0.1 | eu-west-1 |
| São Paulo | GRU | 10.50.1.1 | gru |
| Virginia | IAD | 10.50.2.1 | iad |
| Chicago | ORD | 10.50.2.2 | ord |
| Los Angeles | LAX | 10.50.2.3 | lax |
| Londres | LHR | 10.50.3.1 | lhr |
| Frankfurt | FRA | 10.50.3.2 | fra |
| Paris | CDG | 10.50.3.3 | cdg |
| Tóquio | NRT | 10.50.4.1 | nrt |
| Singapura | SIN | 10.50.4.2 | sin |
| Sydney | SYD | 10.50.4.3 | syd |
| Hong Kong | HKG | 10.50.5.1 | asia-east2 |

---

## Topologias

### Hub-and-Spoke (Legado)

Na configuração inicial, todo tráfego passava por um hub central (EC2 Irlanda):

![Topologia Hub-and-Spoke](/img/wireguard-hub-spoke.svg)

**Problemas:**
- Alta latência para backends geograficamente distantes
- Ponto único de falha
- Todo tráfego cruza a Irlanda independente do destino

**Exemplo de latências (do POP HKG):**
| Backend | Latência via Hub |
|---------|-----------------|
| NRT (Tóquio) | 492ms |
| SIN (Singapura) | 408ms |
| SYD (Sydney) | ~500ms |

### Full Mesh (Atual)

POPs conectam diretamente aos seus backends regionais:

![HKG Full Mesh](/img/wireguard-hkg-mesh.svg)

**Benefícios:**
- ~10x menor latência para tráfego regional
- Sem ponto único de falha para roteamento regional
- Tráfego permanece dentro da região geográfica

**Exemplo de latências (do POP HKG com full mesh):**
| Backend | Latência Hub | Latência Mesh | Melhoria |
|---------|--------------|---------------|----------|
| NRT (Tóquio) | 492ms | **49ms** | **10x** |
| SIN (Singapura) | 408ms | **38ms** | **10.7x** |
| SYD (Sydney) | ~500ms | **122ms** | **~4x** |

---

## Configuração

### Geração de Chaves

Gere um par de chaves para cada nó:

```bash
# Gerar chave privada
wg genkey > private.key

# Derivar chave pública
cat private.key | wg pubkey > public.key

# Gerar chaves de todos os backends de uma vez
for region in gru iad ord lax lhr fra cdg nrt sin syd hkg; do
  wg genkey > wireguard/${region}-private.key
  cat wireguard/${region}-private.key | wg pubkey > wireguard/${region}-public.key
  echo "${region}: $(cat wireguard/${region}-public.key)"
done
```

### Configuração do Hub EC2

A instância EC2 Irlanda atua como hub central para tráfego não-regional:

```ini
# /etc/wireguard/wg0.conf no EC2 Irlanda
[Interface]
PrivateKey = <ec2-private-key>
Address = 10.50.0.1/24
ListenPort = 51820

# Habilitar IP forwarding para roteamento
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT

# GRU - São Paulo
[Peer]
PublicKey = He2jX3+iEl7hUaaJG/i3YcSnStEFAcW/rs/lP0Pw+nc=
AllowedIPs = 10.50.1.1/32
PersistentKeepalive = 25

# IAD - Virginia
[Peer]
PublicKey = rImgzxPu9MuhqLpcvXQ9xckSSA+AGbDOpBGvTUOwaHQ=
AllowedIPs = 10.50.2.1/32
PersistentKeepalive = 25

# ORD - Chicago
[Peer]
PublicKey = SIh+oa2J6k4rYA+N1SzskwztVVR/1Hx3ef/yLyyh+VU=
AllowedIPs = 10.50.2.2/32
PersistentKeepalive = 25

# LAX - Los Angeles
[Peer]
PublicKey = z7JmcJguquFBQiphSSmYBsttr6BoRs8MkCev9o5JkAU=
AllowedIPs = 10.50.2.3/32
PersistentKeepalive = 25

# LHR - Londres
[Peer]
PublicKey = w+XApd9CmhlyweQr8Fp7YPMbjd6RAk/cmXA6OET9/H0=
AllowedIPs = 10.50.3.1/32
PersistentKeepalive = 25

# FRA - Frankfurt
[Peer]
PublicKey = g5IzaRpt1hkvFhGTfy5LC0HLwPxVTC5dQb3if5sds24=
AllowedIPs = 10.50.3.2/32
PersistentKeepalive = 25

# CDG - Paris
[Peer]
PublicKey = C1My7suqoLuYchPIaVLbsB5A/dX21h7wICqa7yL2oX4=
AllowedIPs = 10.50.3.3/32
PersistentKeepalive = 25

# NRT - Tóquio
[Peer]
PublicKey = 9ZK9FzSzihxrRX9gktc99Oj0WFSJMa0mf33pP2LJ/lU=
AllowedIPs = 10.50.4.1/32
PersistentKeepalive = 25

# SIN - Singapura
[Peer]
PublicKey = gcwoqaT950PGW1ZaUEV75VEV7HOdyYT5rwdYOUBQzR0=
AllowedIPs = 10.50.4.2/32
PersistentKeepalive = 25

# SYD - Sydney
[Peer]
PublicKey = 9yHQmzbLKEyM+F1x7obbX0WNaR25XhAcUOdU9SLBeEo=
AllowedIPs = 10.50.4.3/32
PersistentKeepalive = 25

# HKG POP - Hong Kong
[Peer]
PublicKey = GxuSsvO9/raKe5WctZQfX5tkHOrTf0PLJWmHEzrw1Go=
AllowedIPs = 10.50.5.0/24
PersistentKeepalive = 25
```

### Configuração do POP GCP HKG (Full Mesh)

O POP HKG usa full mesh para backends APAC:

```ini
# /etc/wireguard/wg0.conf no GCP HKG
[Interface]
PrivateKey = <hkg-private-key>
Address = 10.50.5.1/24
ListenPort = 51820

# Habilitar IP forwarding
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT

# EC2 Irlanda (para backends não-APAC: SA, NA, EU)
[Peer]
PublicKey = bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY=
Endpoint = 54.171.48.207:51820
AllowedIPs = 10.50.0.1/32, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24
PersistentKeepalive = 25

# NRT - Tóquio (MESH DIRETO)
[Peer]
PublicKey = 9ZK9FzSzihxrRX9gktc99Oj0WFSJMa0mf33pP2LJ/lU=
AllowedIPs = 10.50.4.1/32
PersistentKeepalive = 25

# SIN - Singapura (MESH DIRETO)
[Peer]
PublicKey = gcwoqaT950PGW1ZaUEV75VEV7HOdyYT5rwdYOUBQzR0=
AllowedIPs = 10.50.4.2/32
PersistentKeepalive = 25

# SYD - Sydney (MESH DIRETO)
[Peer]
PublicKey = 9yHQmzbLKEyM+F1x7obbX0WNaR25XhAcUOdU9SLBeEo=
AllowedIPs = 10.50.4.3/32
PersistentKeepalive = 25
```

### Configuração de Backend (APAC)

Backends APAC conectam tanto ao hub EC2 quanto ao POP HKG:

```bash
#!/bin/bash
# entrypoint.sh para backends APAC

# Endpoint EC2 (hub)
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Endpoint HKG (mesh direto)
HKG_ENDPOINT="35.241.112.61:51820"
HKG_PUBKEY="GxuSsvO9/raKe5WctZQfX5tkHOrTf0PLJWmHEzrw1Go="

# Config base com hub EC2
cat > /etc/wireguard/wg0.conf << WGEOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
# EC2 Irlanda (hub para tráfego não-APAC)
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/24, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24
PersistentKeepalive = 25
WGEOF

# Adicionar peer HKG direto para regiões APAC
case "${REGION}" in
  nrt|sin|syd)
    cat >> /etc/wireguard/wg0.conf << WGEOF

[Peer]
# GCP HKG (mesh direto para APAC)
PublicKey = ${HKG_PUBKEY}
Endpoint = ${HKG_ENDPOINT}
AllowedIPs = 10.50.5.0/24
PersistentKeepalive = 25
WGEOF
    ;;
esac

wg-quick up wg0
```

---

## Script User Data

Use este script unificado para provisionar POPs em qualquer cloud (AWS, GCP, Azure):

```bash
#!/bin/bash
# =============================================================================
# edgeProxy POP - Script User Data / Cloud Init
# =============================================================================
# Funciona em: AWS EC2, GCP Compute Engine, Azure VM, qualquer Ubuntu 22.04+
#
# Variáveis obrigatórias:
#   POP_REGION      - Código da região (eu, ap, us, sa)
#   WG_PRIVATE_KEY  - Chave privada WireGuard deste POP
#   WG_ADDRESS      - Endereço IP WireGuard (ex: 10.50.5.1/24)
# =============================================================================

set -e
exec > >(tee /var/log/userdata.log) 2>&1
echo "=== Setup edgeProxy POP Iniciado: $(date) ==="

# Configuração
POP_REGION="${POP_REGION:-ap}"
WG_PRIVATE_KEY="${WG_PRIVATE_KEY}"
WG_ADDRESS="${WG_ADDRESS:-10.50.5.1/24}"
WG_LISTEN_PORT="${WG_LISTEN_PORT:-51820}"

# Todos os peers backend (full mesh)
declare -a WG_PEERS=(
  # EC2 Irlanda (hub central)
  "bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY=|54.171.48.207:51820|10.50.0.1/32|ec2-ireland"

  # América do Sul
  "He2jX3+iEl7hUaaJG/i3YcSnStEFAcW/rs/lP0Pw+nc=||10.50.1.1/32|gru"

  # América do Norte
  "rImgzxPu9MuhqLpcvXQ9xckSSA+AGbDOpBGvTUOwaHQ=||10.50.2.1/32|iad"
  "SIh+oa2J6k4rYA+N1SzskwztVVR/1Hx3ef/yLyyh+VU=||10.50.2.2/32|ord"
  "z7JmcJguquFBQiphSSmYBsttr6BoRs8MkCev9o5JkAU=||10.50.2.3/32|lax"

  # Europa
  "w+XApd9CmhlyweQr8Fp7YPMbjd6RAk/cmXA6OET9/H0=||10.50.3.1/32|lhr"
  "g5IzaRpt1hkvFhGTfy5LC0HLwPxVTC5dQb3if5sds24=||10.50.3.2/32|fra"
  "C1My7suqoLuYchPIaVLbsB5A/dX21h7wICqa7yL2oX4=||10.50.3.3/32|cdg"

  # Ásia Pacífico
  "9ZK9FzSzihxrRX9gktc99Oj0WFSJMa0mf33pP2LJ/lU=||10.50.4.1/32|nrt"
  "gcwoqaT950PGW1ZaUEV75VEV7HOdyYT5rwdYOUBQzR0=||10.50.4.2/32|sin"
  "9yHQmzbLKEyM+F1x7obbX0WNaR25XhAcUOdU9SLBeEo=||10.50.4.3/32|syd"
)

# Instalar pacotes
apt-get update
apt-get install -y wireguard curl jq

# Criar config WireGuard
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE_KEY}
Address = ${WG_ADDRESS}
ListenPort = ${WG_LISTEN_PORT}

PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT
EOF

# Adicionar todos os peers
for peer in "${WG_PEERS[@]}"; do
  IFS='|' read -r pubkey endpoint allowed_ips name <<< "$peer"

  cat >> /etc/wireguard/wg0.conf << EOF

# ${name}
[Peer]
PublicKey = ${pubkey}
AllowedIPs = ${allowed_ips}
PersistentKeepalive = 25
EOF

  if [ -n "$endpoint" ]; then
    sed -i "/PublicKey = ${pubkey}/a Endpoint = ${endpoint}" /etc/wireguard/wg0.conf
  fi
done

chmod 600 /etc/wireguard/wg0.conf

# Iniciar WireGuard
wg-quick up wg0
systemctl enable wg-quick@wg0

echo "=== Status WireGuard ==="
wg show
```

---

## Operações

### Iniciando WireGuard

```bash
# Iniciar interface
sudo wg-quick up wg0

# Habilitar no boot
sudo systemctl enable wg-quick@wg0

# Verificar status
sudo wg show
```

### Verificando Conectividade

```bash
# Mostrar todos os peers e seu status
sudo wg show

# Ping em backend específico
ping 10.50.4.1  # NRT
ping 10.50.4.2  # SIN
ping 10.50.4.3  # SYD

# Verificar tempos de handshake
sudo wg show wg0 latest-handshakes
```

### Adicionando Novo Peer

```bash
# Gerar chaves para novo peer
wg genkey > new-peer-private.key
cat new-peer-private.key | wg pubkey > new-peer-public.key

# Adicionar peer dinamicamente (sem restart)
sudo wg set wg0 peer $(cat new-peer-public.key) allowed-ips 10.50.6.1/32

# Salvar no arquivo de config
sudo wg-quick save wg0
```

### Removendo Peer

```bash
# Remover peer dinamicamente
sudo wg set wg0 peer <public-key> remove

# Ou editar config e reiniciar
sudo vim /etc/wireguard/wg0.conf
sudo wg-quick down wg0 && sudo wg-quick up wg0
```

---

## Troubleshooting

### Peer Não Conectando

```bash
# Verificar se interface existe
ip addr show wg0

# Verificar se peer está configurado
sudo wg show wg0 peers

# Verificar firewall
sudo iptables -L -n | grep 51820

# Verificar se porta UDP está aberta
nc -zvu <peer-ip> 51820
```

### Alta Latência

```bash
# Medir latência para cada peer
for ip in 10.50.4.1 10.50.4.2 10.50.4.3; do
  echo "Ping para $ip:"
  ping -c 3 $ip | tail -1
done

# Se latência estiver alta, verifique se tráfego está passando pelo hub
# Adicione endpoints diretos de peer para tráfego regional
```

### Handshake Não Acontecendo

```bash
# Verificar último tempo de handshake
sudo wg show wg0 latest-handshakes

# Se handshake estiver antigo (>2 minutos), peer pode estar inacessível
# Tente forçar novo handshake
ping -c 1 <peer-wg-ip>
```

### Tráfego Não Roteando

```bash
# Verificar tabela de roteamento
ip route | grep wg0

# Verificar se AllowedIPs inclui destino
sudo wg show wg0 allowed-ips

# Verificar se IP forwarding está habilitado
sysctl net.ipv4.ip_forward
```

---

## Boas Práticas de Segurança

### Gerenciamento de Chaves

1. **Nunca compartilhe chaves privadas** - Cada nó deve ter chaves únicas
2. **Rotacione chaves periodicamente** - Regenere chaves a cada 6-12 meses
3. **Armazene chaves com segurança** - Use gerenciamento de secrets (AWS Secrets Manager, Vault)

### Regras de Firewall

```bash
# Permitir apenas WireGuard UDP
ufw allow 51820/udp

# Restringir SSH a IPs conhecidos
ufw allow from 1.2.3.4 to any port 22
```

### Segmentação de Rede

- Use subnets separadas para cada região
- Limite AllowedIPs ao mínimo necessário
- Não use 0.0.0.0/0 a menos que absolutamente necessário

---

## Otimização de Performance

### Ajuste de MTU

```ini
[Interface]
MTU = 1420  # Ideal para maioria dos cenários
```

### Persistent Keepalive

```ini
[Peer]
PersistentKeepalive = 25  # Manter mapeamentos NAT ativos
```

### Seleção de Endpoint

- Use endpoints estáticos para servidores com IPs públicos
- Omita endpoints para clientes atrás de NAT (eles conectarão a nós)
- Use nomes DNS para IPs dinâmicos

---

## Documentação Relacionada

- [Deploy AWS EC2](./deployment/aws) - Setup do hub EC2
- [Deploy GCP](./deployment/gcp) - Setup do POP GCP
- [Deploy Docker](./deployment/docker) - Desenvolvimento local
- [Benchmarks](./benchmark) - Resultados de performance com WireGuard mesh
