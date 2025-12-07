---
sidebar_position: 1
---

# Fase 1: DNS Interno (.internal)

**Objetivo**: Abstrair IPs de backend com nomes DNS para facilitar gerenciamento e migração.

## Estado Atual

```rust
// IPs hardcoded no routing.db
backend.wg_ip = "10.50.4.1"  // NRT
backend.wg_ip = "10.50.4.2"  // SIN
```

## Estado Alvo

```rust
// Resolução DNS
backend.address = "nrt.backends.internal"  // Resolve para 10.50.4.1
backend.address = "sin.backends.internal"  // Resolve para 10.50.4.2
```

## Implementação

![Serviço DNS Interno](/img/roadmap/phase-1-internal-dns.svg)

## Schema DNS

| Domínio | Resolve Para | Exemplo |
|---------|--------------|---------|
| `<region>.backends.internal` | IP WG do Backend | `nrt.backends.internal` → `10.50.4.1` |
| `<region>.pops.internal` | IP WG do POP | `hkg.pops.internal` → `10.50.5.1` |
| `<app>.<region>.services.internal` | Endpoint da App | `api.nrt.services.internal` → `10.50.4.1:8080` |

## Benefícios

- **Abstração**: Muda IPs sem atualizar configs
- **Migração**: Move backends sem downtime
- **Multi-tenancy**: Namespace por organização

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 2: Plano de Controle Distribuído](./phase-2-corrosion)
