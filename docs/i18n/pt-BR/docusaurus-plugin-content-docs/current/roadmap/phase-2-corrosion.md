---
sidebar_position: 2
---

# Fase 2: Plano de Controle Distribuído (Corrosion)

**Objetivo**: Substituir SQLite local por SQLite replicado para consistência em tempo real entre todos os POPs.

## Estado Atual

![Estado Atual - Sync Manual](/img/roadmap/phase-2-corrosion-current.svg)

## Estado Alvo

![Estado Alvo - Cluster Corrosion](/img/roadmap/phase-2-corrosion-target.svg)

## Integração com Corrosion

```toml
# corrosion.toml
[db]
path = "/var/lib/edgeproxy/routing.db"

[cluster]
name = "edgeproxy"
bootstrap = ["10.50.0.1:4001", "10.50.5.1:4001"]

[gossip]
addr = "0.0.0.0:4001"
```

## Benefícios

- **Sync em tempo real**: Mudanças propagam em ~100ms
- **Sem intervenção manual**: Replicação automática
- **Tolerância a partições**: Funciona durante splits de rede
- **Event-driven**: Inscreva-se em mudanças

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 1: DNS Interno](./phase-1-internal-dns)
- [Fase 3: Auto-Discovery](./phase-3-auto-discovery)
