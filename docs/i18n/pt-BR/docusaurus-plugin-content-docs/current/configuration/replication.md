---
sidebar_position: 6
---

# Replicação Integrada

O edgeProxy v0.3.0 inclui **replicação SQLite integrada** para sincronização automática do estado entre múltiplos POPs. Este documento fornece um deep-dive em como o sistema de replicação funciona, voltado para desenvolvedores que querem entender os internals.

## Visão Geral

O sistema de replicação integrada permite sincronização automática do `routing.db` entre múltiplos POPs (Points of Presence). Quando um backend é registrado em um POP, ele automaticamente se propaga para todos os outros POPs no cluster.

![Arquitetura de Replicação](/img/replication-architecture.svg)

## Conceitos Fundamentais

### 1. Hybrid Logical Clock (HLC)

O HLC é a fundação para ordenação de eventos entre nós distribuídos. Ele combina:

- **Wall Clock Time**: Timestamp real em milissegundos
- **Contador Lógico**: Incrementado quando eventos acontecem no mesmo milissegundo

```rust
// src/replication/types.rs
pub struct HlcTimestamp {
    pub wall_time: u64,   // milissegundos desde epoch
    pub logical: u32,     // contador lógico
    pub node_id: String,  // qual nó gerou este timestamp
}
```

**Por que HLC?**

Relógios físicos podem divergir entre servidores. Se o relógio do Nó A está 100ms adiantado em relação ao Nó B, eventos no Nó A incorretamente pareceriam mais novos. O HLC resolve isso:

1. Usando o máximo entre tempo local e tempo da mensagem recebida
2. Incrementando contador lógico para empates
3. Incluindo node_id para desempate determinístico

```rust
impl HlcTimestamp {
    pub fn tick(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        if now > self.wall_time {
            self.wall_time = now;
            self.logical = 0;
        } else {
            self.logical += 1;
        }
    }
}
```

### 2. Resolução de Conflitos Last-Write-Wins (LWW)

Quando dois nós modificam o mesmo registro simultaneamente, precisamos de resolução de conflitos determinística. LWW usa o timestamp HLC:

```rust
impl Change {
    pub fn wins_over(&self, other: &Change) -> bool {
        // Compara wall time primeiro
        if self.hlc_timestamp.wall_time != other.hlc_timestamp.wall_time {
            return self.hlc_timestamp.wall_time > other.hlc_timestamp.wall_time;
        }
        // Depois contador lógico
        if self.hlc_timestamp.logical != other.hlc_timestamp.logical {
            return self.hlc_timestamp.logical > other.hlc_timestamp.logical;
        }
        // Finalmente node_id para desempate determinístico
        self.hlc_timestamp.node_id > other.hlc_timestamp.node_id
    }
}
```

**Cenário de exemplo:**

1. Nó SA atualiza backend `b1` em HLC(1000, 0, "sa")
2. Nó US atualiza backend `b1` em HLC(1000, 0, "us")
3. Ambas mudanças chegam no Nó EU
4. EU aplica a mudança de `us` porque "us" > "sa" lexicograficamente

### 3. Detecção de Mudanças

Mudanças são rastreadas via a struct `Change`:

```rust
pub struct Change {
    pub table: String,      // "backends"
    pub row_id: String,     // chave primária
    pub kind: ChangeKind,   // Insert, Update, Delete
    pub data: String,       // dados da linha serializados em JSON
    pub hlc_timestamp: HlcTimestamp,
}

pub enum ChangeKind {
    Insert,
    Update,
    Delete,
}
```

O `SyncService` coleta mudanças pendentes e faz flush como um `ChangeSet`:

```rust
pub struct ChangeSet {
    pub origin_node: String,
    pub changes: Vec<Change>,
    pub checksum: u32,  // CRC32 para integridade
}
```

### 4. Protocolo Gossip tipo SWIM

O protocolo gossip lida com membership do cluster e detecção de falhas. É inspirado no [SWIM](http://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf) (Scalable Weakly-consistent Infection-style Process Group Membership).

```rust
// src/replication/gossip.rs
pub enum GossipMessage {
    // Verifica se nó está vivo
    Ping {
        sender_id: String,
        sender_gossip_addr: SocketAddr,
        sender_transport_addr: SocketAddr,
        incarnation: u64,
    },
    // Resposta ao ping
    Ack {
        sender_id: String,
        sender_gossip_addr: SocketAddr,
        sender_transport_addr: SocketAddr,
        incarnation: u64,
    },
    // Anuncia entrada no cluster
    Join {
        node_id: String,
        gossip_addr: SocketAddr,
        transport_addr: SocketAddr,
    },
    // Compartilha lista de membros
    MemberList {
        members: Vec<(String, SocketAddr, SocketAddr, u64)>,
    },
}
```

**Fluxo de membership:**

1. Novo nó envia `Join` para peers de bootstrap
2. Peer de bootstrap adiciona novo nó à lista de membros
3. Peer de bootstrap responde com `MemberList`
4. Novo nó adiciona todos os membros descobertos
5. `Ping`/`Ack` periódico mantém liveness

**Detecção de falhas:**

- Nós fazem ping em membros aleatórios a cada `gossip_interval` (default: 1s)
- Se nenhum `Ack` recebido em 30s, membro é marcado como `Dead`
- Membros mortos são removidos do roteamento

### 5. Transporte QUIC

Sincronização de dados usa [QUIC](https://quicwg.org/) via biblioteca [Quinn](https://github.com/quinn-rs/quinn):

```rust
// src/replication/transport.rs
pub struct TransportService {
    endpoint: Endpoint,
    peers: RwLock<HashMap<String, Connection>>,
    // ...
}
```

**Por que QUIC?**

- **Streams multiplexados**: Múltiplos ChangeSets podem sincronizar simultaneamente
- **Criptografia integrada**: TLS 1.3 para comunicação segura entre peers
- **Migração de conexão**: Lida com mudanças de IP graciosamente
- **Baixa latência**: Handshakes 0-RTT para peers conhecidos

**Certificados auto-assinados:**

O transport gera certificados auto-assinados para comunicação do cluster:

```rust
fn generate_self_signed_cert() -> (CertificateDer, PrivateKeyDer) {
    let cert = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ]).unwrap();
    // ...
}
```

## Fluxo de Dados: Ponta a Ponta

Vamos rastrear um registro de backend do início ao fim:

### Passo 1: Registro de Backend

```bash
# Backend se registra via Auto-Discovery API
curl -X POST http://pop-sa:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{"id": "sa-node-1", "app": "myapp", "region": "sa", "ip": "10.50.1.1", "port": 9000}'
```

### Passo 2: Escrita no SQLite Local

O `ApiServer` insere no SQLite local:

```rust
// adapters/inbound/api_server.rs
async fn register_backend(State(state): State<AppState>, Json(req): Json<RegisterRequest>) {
    // Insere no SQLite
    sqlx::query("INSERT INTO backends ...")
        .execute(&state.db)
        .await?;
}
```

### Passo 3: Mudança Registrada

O `SyncService` registra a mudança com timestamp HLC:

```rust
// replication/sync.rs
pub fn record_backend_change(&self, id: &str, kind: ChangeKind, data: &str) {
    let mut hlc = self.hlc.write();
    hlc.tick();

    let change = Change {
        table: "backends".to_string(),
        row_id: id.to_string(),
        kind,
        data: data.to_string(),
        hlc_timestamp: hlc.clone(),
    };

    self.pending_changes.write().push(change);
}
```

### Passo 4: Flush para ChangeSet

Periodicamente (default: 5s), mudanças pendentes são flushed:

```rust
pub async fn flush(&self) -> Option<ChangeSet> {
    let changes: Vec<Change> = {
        let mut pending = self.pending_changes.write();
        if pending.is_empty() { return None; }
        pending.drain(..).collect()
    };

    let changeset = ChangeSet::new(&self.node_id, changes);
    let _ = self.event_tx.send(SyncEvent::BroadcastReady(changeset.clone())).await;
    Some(changeset)
}
```

### Passo 5: Broadcast via QUIC

O `ReplicationAgent` recebe o evento e faz broadcast para todos os peers:

```rust
// replication/agent.rs
async fn handle_sync_event(&self, event: SyncEvent) {
    match event {
        SyncEvent::BroadcastReady(changeset) => {
            let transport = self.transport.read().await;
            for member in self.gossip.alive_members() {
                transport.send_changeset(&member.transport_addr, &changeset).await;
            }
        }
    }
}
```

### Passo 6: Nó Remoto Recebe

No POP receptor (ex: POP-US):

```rust
// replication/transport.rs
async fn handle_incoming_stream(&self, stream: RecvStream) {
    let msg: Message = bincode::deserialize(&data)?;
    match msg {
        Message::ChangeBroadcast(changeset) => {
            if changeset.verify_checksum() {
                self.event_tx.send(TransportEvent::ChangeSetReceived(changeset)).await;
            }
        }
    }
}
```

### Passo 7: Aplicar com LWW

O `SyncService` aplica mudanças usando LWW:

```rust
pub async fn apply_changeset(&self, changeset: &ChangeSet) -> anyhow::Result<usize> {
    let mut applied = 0;

    for change in &changeset.changes {
        // Verifica se já temos versão mais nova
        let existing = self.version_vector.read().get(&change.row_id);
        if let Some(existing_hlc) = existing {
            if !change.wins_over_hlc(existing_hlc) {
                continue; // Pula, temos mais novo
            }
        }

        // Aplica a mudança
        match change.kind {
            ChangeKind::Insert => self.apply_insert(&change).await?,
            ChangeKind::Update => self.apply_update(&change).await?,
            ChangeKind::Delete => self.apply_delete(&change).await?,
        }

        // Atualiza version vector
        self.version_vector.write().insert(change.row_id.clone(), change.hlc_timestamp.clone());
        applied += 1;
    }

    Ok(applied)
}
```

### Passo 8: Backend Disponível em Todo Lugar

Agora `sa-node-1` está disponível em todos os POPs:

```bash
# Query do POP-US
curl http://pop-us:8081/api/v1/backends
# Retorna: [{"id": "sa-node-1", "app": "myapp", "region": "sa", ...}]

# Query do POP-EU
curl http://pop-eu:8081/api/v1/backends
# Retorna: [{"id": "sa-node-1", "app": "myapp", "region": "sa", ...}]
```

## Configuração

### Variáveis de Ambiente

| Variável | Default | Descrição |
|----------|---------|-----------|
| `EDGEPROXY_REPLICATION_ENABLED` | `false` | Habilita replicação integrada |
| `EDGEPROXY_REPLICATION_NODE_ID` | hostname | Identificador único do nó |
| `EDGEPROXY_REPLICATION_GOSSIP_ADDR` | `0.0.0.0:4001` | Endereço UDP para gossip |
| `EDGEPROXY_REPLICATION_TRANSPORT_ADDR` | `0.0.0.0:4002` | Endereço QUIC para sync |
| `EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS` | (nenhum) | Endereços de peers separados por vírgula |
| `EDGEPROXY_REPLICATION_GOSSIP_INTERVAL_MS` | `1000` | Intervalo de ping do gossip |
| `EDGEPROXY_REPLICATION_SYNC_INTERVAL_MS` | `5000` | Intervalo de flush do sync |
| `EDGEPROXY_REPLICATION_CLUSTER_NAME` | `edgeproxy` | Nome do cluster para isolamento |

### Exemplo: Cluster com 3 POPs

**POP-SA (Bootstrap)**

```bash
EDGEPROXY_REPLICATION_ENABLED=true
EDGEPROXY_REPLICATION_NODE_ID=pop-sa
EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
# Sem bootstrap peers - este é o primeiro nó
```

**POP-US (Entra no SA)**

```bash
EDGEPROXY_REPLICATION_ENABLED=true
EDGEPROXY_REPLICATION_NODE_ID=pop-us
EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS=10.50.1.1:4001
```

**POP-EU (Entra no SA e US)**

```bash
EDGEPROXY_REPLICATION_ENABLED=true
EDGEPROXY_REPLICATION_NODE_ID=pop-eu
EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS=10.50.1.1:4001,10.50.2.1:4001
```

## Referência de Código Fonte

| Arquivo | Propósito |
|---------|-----------|
| `src/replication/mod.rs` | Exports do módulo |
| `src/replication/config.rs` | Struct ReplicationConfig |
| `src/replication/types.rs` | HlcTimestamp, NodeId, Change, ChangeSet |
| `src/replication/gossip.rs` | GossipService, GossipMessage, Member |
| `src/replication/sync.rs` | SyncService, rastreamento de mudanças |
| `src/replication/transport.rs` | TransportService, comunicação QUIC entre peers |
| `src/replication/agent.rs` | Orquestrador ReplicationAgent |

## Troubleshooting

### Nós não se descobrindo

```bash
# Verifica se porta gossip está aberta
nc -zvu 10.50.1.1 4001

# Verifica se bootstrap peers estão corretos
echo $EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS

# Verifica regras de firewall
sudo ufw status
```

### Mudanças não propagando

```bash
# Verifica conectividade do transport
nc -zv 10.50.1.1 4002

# Verifica membership do cluster (logs)
journalctl -u edgeproxy | grep "member joined"

# Garante que intervalo de sync é razoável
echo $EDGEPROXY_REPLICATION_SYNC_INTERVAL_MS
```

### Warnings de drift do HLC

Se você ver warnings de drift do HLC, garanta que NTP está rodando:

```bash
# Verifica status do NTP
timedatectl status

# Instala e habilita NTP
sudo apt install chrony
sudo systemctl enable chronyd
sudo systemctl start chronyd
```

## Tuning de Performance

### Intervalo de Gossip

- **Menor (500ms)**: Detecção de falha mais rápida, mais tráfego de rede
- **Maior (2000ms)**: Menos tráfego, detecção mais lenta
- **Recomendação**: 1000ms para a maioria dos deploys

### Intervalo de Sync

- **Menor (1000ms)**: Sync quase em tempo real, maior uso de CPU
- **Maior (10000ms)**: Agrupa mais mudanças, possível lag
- **Recomendação**: 5000ms para performance balanceada

### Requisitos de Rede

| Caminho | Protocolo | Porta | Bandwidth |
|---------|----------|-------|-----------|
| Gossip | UDP | 4001 | ~1 KB/s por nó |
| Transport | QUIC/UDP | 4002 | Varia com taxa de mudanças |

## Considerações de Segurança

1. **Isolamento de Rede**: Execute portas de replicação no overlay WireGuard
2. **Firewall**: Permita apenas POPs confiáveis conectar em 4001/4002
3. **TLS**: Transport usa TLS 1.3 (certs auto-assinados para cluster)
4. **Nome do Cluster**: Use nomes únicos para prevenir poluição cross-cluster

```bash
# Exemplo de regras de firewall (UFW)
sudo ufw allow from 10.50.0.0/16 to any port 4001 proto udp
sudo ufw allow from 10.50.0.0/16 to any port 4002 proto udp
```

## Melhorias Futuras

- [ ] Delta sync (enviar apenas campos alterados)
- [ ] Anti-entropia baseada em Merkle tree
- [ ] Descoberta automática de cluster via mDNS
- [ ] Métricas Prometheus para lag de replicação
- [ ] Read replicas para SQLite local
