---
id: performance
title: Performance
sidebar_position: 12
---

# Performance

O edgeProxy foi projetado para lidar com **milhares de conexoes concorrentes** com overhead minimo. Esta pagina explica a arquitetura interna que torna isso possivel.

## Fluxo de Requisicoes de Alta Performance

<p align="center">
  <img src="/img/performance-architecture.svg" alt="Arquitetura de Performance" width="100%" />
</p>

Quando um cliente se conecta ao edgeProxy, a requisicao passa por varios estagios otimizados:

| Estagio | Latencia | Descricao |
|---------|----------|-----------|
| TCP Accept | ~1μs | Kernel passa a conexao para userspace |
| GeoIP Lookup | ~100ns | Consulta ao banco MaxMind em memoria |
| Selecao de Backend | ~10μs | Lookup no DashMap + algoritmo de scoring |
| Tunel WireGuard | ~0.5ms | Overhead de criptografia (ChaCha20-Poly1305) |
| **Overhead Total do Proxy** | **\<1ms** | Latencia end-to-end do proxy |

## Tokio Async Runtime

<p align="center">
  <img src="/img/tokio-runtime.svg" alt="Tokio Async Runtime" width="100%" />
</p>

O edgeProxy usa o **Tokio async runtime** para lidar com milhares de conexoes com threads minimas:

### Como Funciona

1. **Pool de Threads = Nucleos de CPU**
   - Por padrao, Tokio cria uma thread worker por nucleo de CPU
   - Um servidor de 4 nucleos roda 4 threads, lidando com 10.000+ conexoes

2. **Tasks Leves (~200 bytes cada)**
   - Cada conexao e uma "task" Tokio, nao uma thread
   - Tasks sao multiplexadas no pool de threads
   - Sem overhead de troca de contexto entre conexoes

3. **I/O Nao-Bloqueante**
   - Usa `epoll` (Linux) ou `kqueue` (macOS) para polling eficiente
   - Uma task esperando I/O nao bloqueia sua thread

### Eficiencia de Memoria

| Conexoes | Memoria (Apenas Tasks) | Memoria Total (Realista) |
|----------|------------------------|--------------------------|
| 1.000 | ~200KB | ~10MB |
| 10.000 | ~2MB | ~100MB |
| 100.000 | ~20MB | ~1GB |

:::info
A memoria "realista" inclui buffers de socket, entradas DashMap e dados de roteamento. O proxy em si permanece muito eficiente.
:::

## Custo das Operacoes

Entender o custo de cada operacao ajuda a identificar gargalos:

| Operacao | Tempo | Notas |
|----------|-------|-------|
| DashMap read | ~50ns | Hashmap concorrente lock-free |
| DashMap write | ~100ns | Updates atomicos |
| GeoIP lookup | ~100ns | MMDB em memoria |
| Scoring de backend | ~1μs | Iterar e pontuar backends |
| SQLite read | ~10μs | Hot reload do routing.db |
| WireGuard encrypt | ~500ns | Overhead por pacote |
| TCP connect | ~1ms | Depende da distancia de rede |

### Modelo de Concorrencia

```rust
// Bindings de cliente: leituras lock-free
let bindings: DashMap<ClientKey, Binding> = DashMap::new();

// Pool de backends: muita leitura, pouca escrita
let backends: DashMap<String, Backend> = DashMap::new();

// Contagem de conexoes: updates atomicos
let conn_count: AtomicUsize = AtomicUsize::new(0);
```

O uso do `DashMap` permite:
- **Leituras concorrentes** sem bloqueio
- **Locking granular** em escritas (por-shard)
- **Sem lock global** que serializaria requisicoes

## Gargalos do Sistema

<p align="center">
  <img src="/img/system-limits.svg" alt="Limites do Sistema" width="100%" />
</p>

O proxy em si raramente e o gargalo. Estes sao os limites reais:

### Camada de Rede (1-10 Gbps)

| Velocidade NIC | Throughput | Limite Tipico |
|----------------|------------|---------------|
| 1 Gbps | ~125 MB/s | Maioria das VMs cloud |
| 10 Gbps | ~1.25 GB/s | Instancias premium |
| 25 Gbps | ~3.1 GB/s | Bare metal |

**Solucao**: Deploy de multiplos POPs para distribuir carga geograficamente.

### Camada do Kernel (File Descriptors)

Cada conexao TCP consome um file descriptor. Limites padrao frequentemente sao muito baixos:

```bash
# Verificar limite atual
ulimit -n

# Padrao tipico: 1024
# Recomendado para producao: 1.000.000+
```

**Solucao**: Aumentar `ulimit -n` no servico systemd ou `/etc/security/limits.conf`:

```bash
# /etc/security/limits.conf
*    soft    nofile    1048576
*    hard    nofile    1048576
```

### Camada de Backend (Limites de Conexao)

Cada backend tem `soft_limit` e `hard_limit` no `routing.db`:

| Limite | Proposito |
|--------|-----------|
| `soft_limit` | Contagem confortavel de conexoes, usado para scoring |
| `hard_limit` | Maximo de conexoes, rejeita quando atingido |

**Tuning**: Ajuste baseado na capacidade do backend:

```sql
-- Aumentar limites para backends de alta capacidade
UPDATE backends SET soft_limit = 100, hard_limit = 200
WHERE id = 'us-node-1';
```

## Tuning do Kernel

Para deploys de alta performance, ajuste estes parametros do kernel:

```bash
# /etc/sysctl.conf

# Maximo de conexoes em fila para accept
net.core.somaxconn = 65535

# Buffers maximos de socket receive/send
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216

# Tamanhos de buffer TCP (min, default, max)
net.ipv4.tcp_rmem = 4096 87380 16777216
net.ipv4.tcp_wmem = 4096 65536 16777216

# Habilitar TCP Fast Open
net.ipv4.tcp_fastopen = 3

# Aumentar range de portas para conexoes de saida
net.ipv4.ip_local_port_range = 1024 65535

# Reduzir sockets TIME_WAIT
net.ipv4.tcp_fin_timeout = 15
net.ipv4.tcp_tw_reuse = 1
```

Aplicar com:

```bash
sudo sysctl -p
```

## Metricas de Performance

Baseado em benchmarks com uma VM de 4 nucleos:

| Metrica | Valor |
|---------|-------|
| Conexoes/segundo | 50.000+ |
| Conexoes concorrentes | 10.000+ |
| Latencia do proxy | \<1ms |
| Memoria por 1K conexoes | ~10MB |
| Overhead CPU WireGuard | ~3% |
| Tempo de cold start | ~50ms |
| Tamanho do binario | ~5MB |

:::tip
Estes numeros sao conservadores. Performance real depende de condicoes de rede, tempo de resposta dos backends e caracteristicas da carga de trabalho.
:::

## Comparacao com Outros Proxies

| Feature | edgeProxy | HAProxy | Nginx | Envoy |
|---------|-----------|---------|-------|-------|
| Linguagem | Rust | C | C | C++ |
| Modelo Async | Tokio | Multi-processo | Event loop | Multi-thread |
| Memoria por 10K conn | ~100MB | ~50MB | ~30MB | ~200MB |
| Geo-routing | Nativo | Plugin | Modulo | Plugin |
| WireGuard | Nativo | Externo | Externo | Externo |
| Config reload | Hot | Hot | Hot | Hot |

O edgeProxy troca um pouco de throughput bruto por:
- **Geo-routing integrado** sem dependencias externas
- **Integracao WireGuard** para backhaul seguro
- **Seguranca Rust** com latencia previsivel (sem GC)

## Monitorando Performance

Monitore estas metricas em producao:

```bash
# Taxa de conexoes
curl localhost:9090/metrics | grep edge_connections_total

# Conexoes atuais
curl localhost:9090/metrics | grep edge_connections_current

# Latencia do backend
curl localhost:9090/metrics | grep edge_backend_latency_ms
```

:::note
Exportacao de metricas Prometheus esta planejada para uma release futura. Veja o [Roadmap](/docs/roadmap) para detalhes.
:::

## Melhores Praticas

1. **Deploy proximo aos usuarios**: Use POPs em cada regiao principal
2. **Dimensione seus backends**: Configure `soft_limit` para 70% da capacidade real
3. **Monitore file descriptors**: Alerte quando se aproximar do `ulimit`
4. **Use WireGuard**: O overhead de 0.5ms vale pela seguranca
5. **Habilite TCP Fast Open**: Reduz latencia de conexao em 1 RTT
6. **Escale horizontalmente**: Adicione mais POPs, nao VMs maiores
