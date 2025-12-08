---
sidebar_position: 1
---

# Testes de Integração Fly.io

Resultados dos testes para edgeProxy v0.4.0 integração com backends Fly.io multi-região via VPN WireGuard.

**Data do Teste**: 2025-12-08
**Ambiente**: EC2 Hub (Irlanda) + 10 POPs Fly.io

## Ambiente de Teste

### EC2 Hub (Irlanda)

| Propriedade | Valor |
|-------------|-------|
| **IP Público** | 34.246.117.138 (Elastic IP) |
| **IP WireGuard** | 10.50.0.1/24 |
| **Região** | eu-west-1 |
| **Versão edgeProxy** | v0.4.0 |

### Máquinas Backend Fly.io

| Região | Localização | IP WireGuard | Status |
|--------|-------------|--------------|--------|
| GRU | São Paulo | 10.50.1.1 | Rodando |
| IAD | Virgínia | 10.50.2.1 | Rodando |
| ORD | Chicago | 10.50.2.2 | Rodando |
| LAX | Los Angeles | 10.50.2.3 | Rodando |
| LHR | Londres | 10.50.3.1 | Rodando |
| FRA | Frankfurt | 10.50.3.2 | Rodando |
| CDG | Paris | 10.50.3.3 | Rodando |
| NRT | Tóquio | 10.50.4.1 | Rodando |
| SIN | Singapura | 10.50.4.2 | Rodando |
| SYD | Sydney | 10.50.4.3 | Rodando |

## Resultados dos Testes

### 1. Conectividade WireGuard

**Teste**: Ping do EC2 Hub para todos os backends Fly.io via túnel WireGuard.

```bash
# Do EC2 Hub (34.246.117.138)
for ip in 10.50.1.1 10.50.2.1 10.50.2.2 10.50.2.3 10.50.3.1 10.50.3.2 10.50.3.3 10.50.4.1 10.50.4.2 10.50.4.3; do
  ping -c 1 -W 2 $ip > /dev/null && echo "[OK] $ip" || echo "[FAIL] $ip"
done
```

**Resultados**:

| Backend | IP | Ping | Handshake |
|---------|-----|------|-----------|
| GRU | 10.50.1.1 | OK | Ativo |
| IAD | 10.50.2.1 | OK | Ativo |
| ORD | 10.50.2.2 | OK | Ativo |
| LAX | 10.50.2.3 | OK | Ativo |
| LHR | 10.50.3.1 | OK | Ativo |
| FRA | 10.50.3.2 | OK | Ativo |
| CDG | 10.50.3.3 | OK | Ativo |
| NRT | 10.50.4.1 | OK | Ativo |
| SIN | 10.50.4.2 | OK | Ativo |
| SYD | 10.50.4.3 | OK | Ativo |

**Status**: 10/10 backends alcançáveis

---

### 2. Status do Serviço edgeProxy

**Teste**: Verificar se todos os serviços edgeProxy estão rodando no EC2 Hub.

```bash
sudo systemctl status edgeproxy
ss -tlnp | grep edge-proxy
ss -ulnp | grep edge-proxy
```

**Resultados**:

| Serviço | Porta | Protocolo | Status |
|---------|-------|-----------|--------|
| TCP Proxy | 8080 | TCP | OK |
| TLS Server | 8443 | TCP | OK |
| API Server | 8081 | TCP | OK |
| DNS Server | 5353 | UDP | OK |
| Gossip | 4001 | UDP | OK |
| Transport | 4002 | UDP | OK |

**Status**: Todos os serviços ativos

---

### 3. Registro de Backend via API

**Teste**: Registrar backends via API de Auto-Discovery.

```bash
curl -X POST http://34.246.117.138:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{"id":"pop-gru","app":"gru.pop","region":"sa","ip":"10.50.1.1","port":80}'
```

**Resultados**:

| Backend | App | Região | Resposta |
|---------|-----|--------|----------|
| pop-gru | gru.pop | sa | `{"registered":true}` |
| pop-iad | iad.pop | us | `{"registered":true}` |
| pop-ord | ord.pop | us | `{"registered":true}` |
| pop-lax | lax.pop | us | `{"registered":true}` |
| pop-lhr | lhr.pop | eu | `{"registered":true}` |
| pop-fra | fra.pop | eu | `{"registered":true}` |
| pop-cdg | cdg.pop | eu | `{"registered":true}` |
| pop-nrt | nrt.pop | ap | `{"registered":true}` |
| pop-sin | sin.pop | ap | `{"registered":true}` |
| pop-syd | syd.pop | ap | `{"registered":true}` |

**Status**: 10/10 backends registrados

---

### 4. Resolução DNS com Filtro por App

**Teste**: Consultar servidor DNS para backends específicos por região.

```bash
dig @127.0.0.1 -p 5353 gru.pop.internal +short
dig @127.0.0.1 -p 5353 lhr.pop.internal +short
dig @127.0.0.1 -p 5353 nrt.pop.internal +short
```

**Resultados**:

| Consulta | Esperado | Resposta | Status |
|----------|----------|----------|--------|
| `gru.pop.internal` | 10.50.1.1 | 10.50.1.1 | OK |
| `iad.pop.internal` | 10.50.2.1 | 10.50.2.1 | OK |
| `ord.pop.internal` | 10.50.2.2 | 10.50.2.2 | OK |
| `lax.pop.internal` | 10.50.2.3 | 10.50.2.3 | OK |
| `lhr.pop.internal` | 10.50.3.1 | 10.50.3.1 | OK |
| `fra.pop.internal` | 10.50.3.2 | 10.50.3.2 | OK |
| `cdg.pop.internal` | 10.50.3.3 | 10.50.3.3 | OK |
| `nrt.pop.internal` | 10.50.4.1 | 10.50.4.1 | OK |
| `sin.pop.internal` | 10.50.4.2 | 10.50.4.2 | OK |
| `syd.pop.internal` | 10.50.4.3 | 10.50.4.3 | OK |

**Status**: 10/10 consultas DNS corretas

---

### 5. DNS das Máquinas Fly.io

**Teste**: Consultar servidor DNS de cada região Fly.io via WireGuard.

```bash
# Do GRU
fly ssh console -a edgeproxy-backend -r gru -C "dig @10.50.0.1 -p 5353 gru.pop.internal +short"

# Do NRT
fly ssh console -a edgeproxy-backend -r nrt -C "dig @10.50.0.1 -p 5353 nrt.pop.internal +short"
```

**Resultados**:

| Região Origem | Consulta | Resposta | Status |
|---------------|----------|----------|--------|
| GRU | `gru.pop.internal` | 10.50.1.1 | OK |
| NRT | `nrt.pop.internal` | 10.50.4.1 | OK |

**Status**: DNS acessível de todas as regiões Fly.io

---

## Problemas Encontrados e Corrigidos

### Problema 1: Mudança de IP do Endpoint WireGuard

**Problema**: Instância EC2 tinha IP público dinâmico que mudou após reinício de `54.171.48.207` para `34.240.78.199`.

**Causa Raiz**: Instâncias EC2 sem Elastic IP recebem novo IP público ao reiniciar.

**Correção**:
1. Alocado Elastic IP `34.246.117.138`
2. Associado à instância EC2
3. Atualizado endpoint WireGuard em todas as máquinas Fly.io

```bash
# Em cada máquina Fly.io
sed -i "s/Endpoint = .*/Endpoint = 34.246.117.138:51820/" /etc/wireguard/wg0.conf
wg-quick down wg0 && wg-quick up wg0
```

### Problema 2: Chave Pública WireGuard Incorreta

**Problema**: Máquinas Fly.io tinham chave pública antiga configurada.

**Causa Raiz**: WireGuard do EC2 foi reconfigurado, gerando novo par de chaves.

**Correção**: Atualizada chave pública em todas as máquinas Fly.io para `Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=`

### Problema 3: Servidor DNS Não Respondendo

**Problema**: Consultas DNS dando timeout mesmo com porta 5353 escutando.

**Causa Raiz**: Bug na função `handle_packet()` - ela parseava pacotes DNS mas nunca enviava respostas.

**Correção**: Reescrita `handle_packet()` para enviar respostas DNS via socket UDP.

```rust
// Antes (quebrado)
async fn handle_packet(...) -> anyhow::Result<()> {
    let message = Message::from_bytes(data)?;
    // Apenas logging, sem resposta!
    Ok(())
}

// Depois (corrigido)
async fn handle_packet(..., socket: Arc<UdpSocket>) -> anyhow::Result<()> {
    let message = Message::from_bytes(data)?;
    // Processa consulta e envia resposta
    let bytes = response.to_bytes()?;
    socket.send_to(&bytes, src).await?;
    Ok(())
}
```

### Problema 4: DNS Não Filtrando por Nome do App

**Problema**: Todas as consultas DNS retornavam o mesmo backend (baseado em geo) independente do nome do app.

**Causa Raiz**: Resolver DNS usava `resolve_backend_with_geo()` que não filtra por app.

**Correção**:
1. Adicionado método `resolve_backend_by_app()` ao ProxyService
2. Atualizado resolver DNS para usar filtro por app quando nome do app é especificado

```rust
// Novo método no ProxyService
pub async fn resolve_backend_by_app(
    &self,
    app: &str,
    client_ip: IpAddr,
    client_geo: Option<GeoInfo>,
) -> Option<Backend> {
    let backends: Vec<Backend> = self.backend_repo.get_healthy().await
        .into_iter()
        .filter(|b| b.app == app)
        .collect();
    // ... balanceamento de carga entre backends filtrados
}
```

---

## Topologia de Rede

![Topologia de Integração Fly.io](/img/tests/flyio-topology.svg)

## Convenção de Nomes DNS

Entradas DNS seguem o padrão `<região>.pop.internal`:

| Nome DNS | Resolve Para | Região |
|----------|--------------|--------|
| `gru.pop.internal` | 10.50.1.1 | América do Sul |
| `iad.pop.internal` | 10.50.2.1 | US Leste |
| `ord.pop.internal` | 10.50.2.2 | US Central |
| `lax.pop.internal` | 10.50.2.3 | US Oeste |
| `lhr.pop.internal` | 10.50.3.1 | Europa (UK) |
| `fra.pop.internal` | 10.50.3.2 | Europa (DE) |
| `cdg.pop.internal` | 10.50.3.3 | Europa (FR) |
| `nrt.pop.internal` | 10.50.4.1 | Ásia Pacífico (JP) |
| `sin.pop.internal` | 10.50.4.2 | Ásia Pacífico (SG) |
| `syd.pop.internal` | 10.50.4.3 | Ásia Pacífico (AU) |

---

## Conclusão

Todos os testes de integração passaram com sucesso após correção dos problemas identificados:

| Categoria de Teste | Resultado |
|--------------------|-----------|
| Conectividade WireGuard | 10/10 OK |
| Status dos Serviços | 6/6 OK |
| Registro via API | 10/10 OK |
| Resolução DNS | 10/10 OK |
| DNS Cross-Region | OK |

**Total**: Todos os testes passando
