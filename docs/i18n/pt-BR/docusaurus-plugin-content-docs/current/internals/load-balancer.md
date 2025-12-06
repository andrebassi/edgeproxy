---
sidebar_position: 1
---

# Internals do Load Balancer

Este documento fornece um deep dive técnico no algoritmo de balanceamento de carga do edgeProxy, sistema de pontuação e detalhes de implementação.

## Visão Geral

O edgeProxy usa um **algoritmo de pontuação ponderada** que considera:

1. Proximidade geográfica (correspondência de região)
2. Carga atual do backend (contagem de conexões)
3. Capacidade do backend (limites soft/hard)
4. Pesos configurados

O objetivo é rotear tráfego para o "melhor" backend onde melhor = menor score.

## Algoritmo de Pontuação

### Fórmula

```
score = region_score * 100 + (load_factor / weight)

onde:
  region_score = 0 | 1 | 2 (menor é melhor)
  load_factor = conexões_atuais / soft_limit
  weight = peso do backend (1-10, maior recebe mais tráfego)
```

### Score de Região

| Condição | Score | Descrição |
|----------|-------|-----------|
| Região do cliente == Região do backend | 0 | Melhor match - mesma região |
| Região do backend == Região do POP local | 1 | Bom match - região local |
| Outro | 2 | Fallback - cross-region |

**Exemplo:**

```
Cliente do Brasil conectando ao POP SA:
├── sa-node-1 (region=sa) → region_score = 0 (match do cliente)
├── sa-node-2 (region=sa) → region_score = 0 (match do cliente)
├── us-node-1 (region=us) → region_score = 2 (fallback)
└── eu-node-1 (region=eu) → region_score = 2 (fallback)
```

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

        // Calcular score de região
        let region_score: u64 = match client_region {
            Some(cr) if cr == b.region => 0,    // Match de região do cliente
            _ if b.region == local_region => 1, // Região do POP local
            _ => 2,                              // Fallback
        };

        // Calcular fator de carga
        let load_factor = conns as f64 / b.soft_limit as f64;

        // Score final (menor é melhor)
        let score = (region_score * 100) as f64 + (load_factor / b.weight as f64);

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

## Próximos Passos

- [Arquitetura](../architecture) - Visão geral do sistema
- [Configuração](../configuration) - Opções de tuning
- [Afinidade de Cliente](./client-affinity) - Sticky sessions
