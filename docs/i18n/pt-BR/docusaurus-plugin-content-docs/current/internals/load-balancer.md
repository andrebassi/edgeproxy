---
sidebar_position: 1
---

# Load Balancer

Este documento fornece um deep dive técnico no algoritmo de balanceamento de carga do edgeProxy, sistema de pontuação e detalhes de implementação.

## Visão Geral

O edgeProxy usa um **algoritmo de pontuação ponderada** que considera:

1. **Roteamento por país** (match exato de país - maior prioridade)
2. **Roteamento por região** (correspondência de região continental)
3. Carga atual do backend (contagem de conexões)
4. Capacidade do backend (limites soft/hard)
5. Pesos configurados

O objetivo é rotear tráfego para o "melhor" backend onde melhor = menor score.

## Database GeoIP

O database MaxMind GeoLite2-Country está **embutido diretamente no binário** em tempo de compilação usando a macro `include_bytes!` do Rust. Isso significa:

- Nenhum arquivo de database externo necessário em runtime
- Deploy de binário único
- Geo-roteamento automático sem configuração
- Override opcional via variável de ambiente `EDGEPROXY_GEOIP_PATH`

## Algoritmo de Pontuação

### Fórmula

```
score = region_score * 100 + (load_factor / weight)

onde:
  region_score = 0 | 1 | 2 (menor é melhor)
  load_factor = conexões_atuais / soft_limit
  weight = peso do backend (1-10, maior recebe mais tráfego)
```

### Geo Score (País + Região)

O sistema de pontuação geo prioriza **país** primeiro, depois **região**, garantindo que usuários sejam roteados para o backend geograficamente mais próximo:

| Condição | Score | Descrição |
|----------|-------|-----------|
| País do cliente == País do backend | 0 | Melhor match - mesmo país (ex: FR → CDG) |
| Região do cliente == Região do backend | 1 | Bom match - mesma região (ex: FR → qualquer EU) |
| Região do backend == Região do POP local | 2 | Região do POP local |
| Outro | 3 | Fallback - cross-region |

**Exemplo:**

```
Cliente da França (country=FR, region=eu) conectando:
├── fly-cdg-1 (country=FR, region=eu) → geo_score = 0 (match de país!)
├── fly-fra-1 (country=DE, region=eu) → geo_score = 1 (match de região)
├── fly-lhr-1 (country=GB, region=eu) → geo_score = 1 (match de região)
├── fly-iad-1 (country=US, region=us) → geo_score = 3 (fallback)
└── fly-nrt-1 (country=JP, region=ap) → geo_score = 3 (fallback)
```

### Mapeamento País para Região

Os seguintes países são mapeados para regiões:

| Região | Países |
|--------|--------|
| **sa** (América do Sul) | BR, AR, CL, PE, CO, UY, PY, BO, EC |
| **us** (América do Norte) | US, CA, MX |
| **eu** (Europa) | PT, ES, FR, DE, NL, IT, GB, IE, BE, CH, AT, PL, CZ, SE, NO, DK, FI |
| **ap** (Ásia Pacífico) | JP, KR, TW, HK, SG, MY, TH, VN, ID, PH, AU, NZ |
| **us** (Fallback) | Todos os outros países |

### Fator de Carga

```rust
load_factor = conexões_atuais as f64 / soft_limit as f64
```

| Conexões | Soft Limit | Fator de Carga |
|----------|------------|----------------|
| 0 | 50 | 0.0 |
| 25 | 50 | 0.5 |
| 50 | 50 | 1.0 |
| 75 | 50 | 1.5 |

**Nota:** Backends excedendo `hard_limit` são excluídos inteiramente.

### Impacto do Peso

Maior peso = menor score = mais tráfego:

```
contribuição do score = load_factor / weight

weight=1: load_factor contribui 100%
weight=2: load_factor contribui 50%
weight=3: load_factor contribui 33%
```

## Exemplo Completo de Pontuação

O diagrama a seguir mostra como o load balancer pontua e seleciona backends com base na correspondência de região, carga atual e configuração de peso:

![Load Balancer Scoring](/img/load-balancer-scoring.svg)

## Implementação

### Código Fonte (`lb.rs`)

```rust
use crate::model::Backend;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct BackendMetrics {
    pub current_conns: AtomicU64,
    pub last_rtt_ms: AtomicU64,
}

impl BackendMetrics {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn pick_backend(
    backends: &[Backend],
    local_region: &str,
    client_region: Option<&str>,
    client_country: Option<&str>,  // NOVO: roteamento por país
    metrics: &DashMap<String, BackendMetrics>,
) -> Option<Backend> {
    let mut best: Option<(Backend, f64)> = None;

    for b in backends {
        // Pular backends não saudáveis
        if !b.healthy {
            continue;
        }

        // Obter contagem atual de conexões
        let conns = metrics
            .get(&b.id)
            .map(|m| m.current_conns.load(Ordering::Relaxed))
            .unwrap_or(0);

        // Pular backends no limite hard
        if conns >= b.hard_limit as u64 {
            continue;
        }

        // Calcular geo score (país > região > local > fallback)
        let geo_score = if client_country.is_some()
            && Some(b.country.as_str()) == client_country {
            0.0 // Melhor: mesmo país (FR → CDG)
        } else if Some(b.region.as_str()) == client_region {
            1.0 // Bom: mesma região (FR → qualquer EU)
        } else if b.region == local_region {
            2.0 // OK: região do POP local
        } else {
            3.0 // Fallback: cross-region
        };

        // Calcular fator de carga
        let load_factor = conns as f64 / b.soft_limit as f64;

        // Score final (menor é melhor)
        let score = geo_score * 100.0 + (load_factor / b.weight as f64);

        // Atualizar best se este for melhor
        match &best {
            Some((_, best_score)) => {
                if score < *best_score {
                    best = Some((b.clone(), score));
                }
            }
            None => {
                best = Some((b.clone(), score));
            }
        }
    }

    best.map(|(b, _)| b)
}
```

### Decisões de Design

#### 1. Métricas Lock-Free

Usando `DashMap` com contadores atômicos evita contenção:

```rust
// Incrementar na conexão
metrics.entry(backend_id)
    .or_insert_with(BackendMetrics::new)
    .current_conns.fetch_add(1, Ordering::Relaxed);

// Decrementar na desconexão
metrics.get(&backend_id)
    .map(|m| m.current_conns.fetch_sub(1, Ordering::Relaxed));
```

#### 2. Prioridade de Região

O multiplicador `* 100` garante que região sempre domina:

```
Melhor caso (mesma região):     0 + load_factor
Pior caso (diferente):        200 + load_factor

Mesmo um backend local totalmente carregado (load_factor=2.0)
ganha de um backend remoto vazio (200.0)
```

#### 3. Peso como Divisor

Usar peso como divisor fornece escala intuitiva:

```
weight=2 recebe 2x tráfego de weight=1
weight=3 recebe 3x tráfego de weight=1
```

## Casos de Borda

### Sem Backends Saudáveis

```rust
if rt.backends.is_empty() || all_unhealthy {
    tracing::warn!("no healthy backend available");
    return Ok(()); // Conexão descartada
}
```

### Todos os Backends no Limite

Quando todos os backends excedem `hard_limit`, a conexão é descartada:

```rust
if conns >= b.hard_limit as u64 {
    continue; // Pular este backend
}

// Se nenhum backend restar após filtro
best.is_none() → conexão descartada
```

### Região do Cliente Desconhecida

Sem GeoIP, faz fallback para região do POP local:

```rust
let client_region = state.geo
    .as_ref()
    .and_then(|g| g.region_for_ip(client_ip));

// Se None, region_score usa apenas comparação com local_region
```

## Características de Performance

### Complexidade de Tempo

- Iteração de backends: O(n) onde n = contagem de backends
- Lookup de métricas: O(1) médio (DashMap)
- Cálculo de score: O(1)

**Total:** O(n) por conexão

### Complexidade de Espaço

- Mapa de métricas: O(n) onde n = backends únicos
- Por métrica: 16 bytes (dois AtomicU64)

### Benchmarks

| Backends | Tempo Médio de Seleção |
|----------|------------------------|
| 10 | ~100ns |
| 100 | ~1μs |
| 1000 | ~10μs |

## Diretrizes de Tuning

### Distribuição de Peso

| Cenário | Pesos |
|---------|-------|
| Distribuição igual | Todos 1 |
| Preferir primário | Primário: 3, Secundário: 1 |
| Rollout gradual | Novo: 1, Antigo: 9 |

### Limites Soft/Hard

```
soft_limit = conexões_confortáveis
hard_limit = máximo_absoluto

Recomendação:
  soft_limit = 70% do hard_limit
  hard_limit = max_fd / backends_esperados
```

### Configuração de Região

Garanta que backends correspondam à distribuição esperada de clientes:

```
70% tráfego do SA → 70% backends em sa
20% tráfego dos US → 20% backends em us
10% tráfego da EU → 10% backends em eu
```

## Monitoramento

### Métricas Chave

1. **Distribuição de conexões**: Os backends estão balanceados?
2. **Precisão do roteamento por região**: Clientes estão chegando nos backends locais?
3. **Utilização de capacidade**: Hits nos limites soft/hard?

### Debug Logging

```bash
DEBUG=1 ./edge-proxy
```

Saída:

```
DEBUG edge_proxy::proxy: proxying 10.0.0.1 -> sa-node-1 (10.50.1.1:8080)
DEBUG edge_proxy::lb: scores: sa-node-1=0.3, sa-node-2=0.2, selected=sa-node-2
```

## Resultados de Benchmark do Geo-Routing

O seguinte benchmark foi conduzido em **2025-12-07** usando conexões VPN de múltiplos países para validar o algoritmo de geo-routing com 10 backends Fly.io implantados globalmente.

### Ambiente de Teste

- **Versão do edgeProxy**: 0.1.0
- **Database GeoIP**: MaxMind GeoLite2-Country (embutido)
- **Backends**: 10 nodes em 4 regiões (sa, us, eu, ap)
- **Método de teste**: Conexão VPN de cada país, `curl localhost:8080`

### Resultados: 9/9 Testes Aprovados (100%)

| # | Localização VPN | País | Backend Esperado | Resultado | Status |
|---|-----------------|------|------------------|-----------|--------|
| 1 | Paris, França | FR | CDG | CDG | PASS |
| 2 | Frankfurt, Alemanha | DE | FRA | FRA | PASS |
| 3 | Londres, UK | GB | LHR | LHR | PASS |
| 4 | Detroit, USA | US | IAD | IAD | PASS |
| 5 | Las Vegas, USA | US | IAD | IAD | PASS |
| 6 | Tóquio, Japão | JP | NRT | NRT | PASS |
| 7 | Cingapura | SG | SIN | SIN | PASS |
| 8 | Sydney, Austrália | AU | SYD | SYD | PASS |
| 9 | São Paulo, Brasil | BR | GRU | GRU | PASS |

### Observações Principais

1. **Roteamento por país funciona corretamente**: França roteia para CDG (Paris), Alemanha para FRA (Frankfurt), UK para LHR (Londres)
2. **Fallback de região funciona**: Múltiplas localizações US (Detroit, Las Vegas) corretamente fazem fallback para IAD já que todos os backends US têm o mesmo código de país
3. **Detecção de mudança de VPN**: O proxy automaticamente detecta mudanças de VPN/país e limpa bindings de cliente
4. **GeoIP embutido**: Nenhum arquivo de database externo necessário - MaxMind DB está compilado no binário

### Configuração dos Backends

```
| Backend ID   | País | Região | Localização        |
|--------------|------|--------|-------------------|
| fly-gru-1    | BR   | sa     | São Paulo, Brasil |
| fly-iad-1    | US   | us     | Virginia, USA     |
| fly-ord-1    | US   | us     | Chicago, USA      |
| fly-lax-1    | US   | us     | Los Angeles, USA  |
| fly-lhr-1    | GB   | eu     | Londres, UK       |
| fly-fra-1    | DE   | eu     | Frankfurt, Alemanha|
| fly-cdg-1    | FR   | eu     | Paris, França     |
| fly-nrt-1    | JP   | ap     | Tóquio, Japão     |
| fly-sin-1    | SG   | ap     | Cingapura         |
| fly-syd-1    | AU   | ap     | Sydney, Austrália |
```

## Melhorias Futuras

1. **Roteamento baseado em latência**: Incluir RTT no score
2. **Pesos adaptativos**: Auto-ajustar baseado em taxas de erro
3. **Circuit breaker**: Exclusão temporária em falhas
4. **Consistent hashing**: Para backends stateful
5. **Roteamento por cidade/estado**: Para países grandes como US, rotear para backend regional mais próximo

## Próximos Passos

- [Arquitetura](../architecture) - Visão geral do sistema
- [Configuração](../configuration) - Opções de tuning
- [Afinidade de Cliente](./client-affinity) - Sticky sessions
