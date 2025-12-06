use crate::model::Backend;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Métricas em runtime para cada backend
#[derive(Debug)]
pub struct BackendMetrics {
    pub current_conns: AtomicUsize,
    pub last_rtt_ms: AtomicU64,
}

impl BackendMetrics {
    pub fn new() -> Self {
        Self {
            current_conns: AtomicUsize::new(0),
            last_rtt_ms: AtomicU64::new(0),
        }
    }
}

impl Default for BackendMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Escolha de backend:
/// - Filtra healthy
/// - Respeita hard_limit
/// - Considera região do cliente + região local
/// - Usa weight e soft_limit pra balancear carga
pub fn pick_backend(
    backends: &[Backend],
    local_region: &str,
    client_region: Option<&str>,
    metrics: &DashMap<String, BackendMetrics>,
) -> Option<Backend> {
    let mut best: Option<(Backend, f64)> = None;

    for b in backends.iter().filter(|b| b.healthy) {
        // Métricas
        let m = metrics.get(&b.id);
        let current = m
            .as_ref()
            .map(|mm| mm.current_conns.load(Ordering::Relaxed))
            .unwrap_or(0) as f64;

        let soft = if b.soft_limit == 0 { 1.0 } else { b.soft_limit as f64 };
        let hard = if b.hard_limit == 0 {
            f64::MAX
        } else {
            b.hard_limit as f64
        };

        if current >= hard {
            // estourou hard_limit, ignora
            continue;
        }

        // Região: prioridade
        let region_score = if Some(b.region.as_str()) == client_region {
            0.0 // melhor caso: mesma região do cliente
        } else if b.region == local_region {
            1.0 // mesma região do POP
        } else {
            2.0 // fallback
        };

        // Load factor (quanto da soft_limit está em uso)
        let load_factor = current / soft; // 0.0 ideal; >1 está "acima" do confortável

        // Peso (weight): maior peso => preferido
        let weight = if b.weight == 0 { 1.0 } else { b.weight as f64 };

        // Score final: menor melhor
        let score = region_score * 100.0 + (load_factor / weight);

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
