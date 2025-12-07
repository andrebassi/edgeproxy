---
sidebar_position: 0
---

# Roadmap de Arquitetura Futura

Este documento descreve a evolução planejada do edgeProxy rumo a uma plataforma de edge computing totalmente distribuída e auto-recuperável.

:::info Estado Atual
edgeProxy v1 é um proxy TCP geo-aware funcional com overlay WireGuard. Este roadmap descreve o caminho para v2 e além.
:::

## Princípios de Design

edgeProxy segue padrões comprovados de plataformas edge em produção:

- **WireGuard como Fundação**: Toda comunicação interna flui sobre a malha WireGuard. Ele fornece o **backhaul** entre POPs - a rede interna que transporta tráfego entre datacenters. Quando um usuário conecta ao servidor edge mais próximo mas seu backend roda em outra região, o proxy roteia transparentemente através de túneis WireGuard de baixa latência em vez de voltar pela internet pública.

![WireGuard Backhaul](/img/roadmap/backhaul-diagram.svg)

- **Rust + Tokio para Performance**: Componentes críticos construídos em Rust usando runtime assíncrono Tokio para latência previsível sem pausas de GC.
- **6PN (Rede Privada IPv6)**: Conectividade interna usa endereçamento IPv6 privado, habilitando espaço de endereços ilimitado para backends e serviços.
- **Rede Global Anycast**: Endereço IP único anunciado de múltiplas localizações, com BGP gerenciando roteamento ótimo.

---

## Comparação de Arquiteturas

### Arquitetura Atual vs Alvo

![Arquitetura Futura](/img/architecture-future.svg)

| Componente | v1 (Atual) | v2 (Alvo) |
|------------|------------|-----------|
| **Roteamento de Tráfego** | GeoDNS | Anycast BGP |
| **Edge Proxy** | edgeProxy (Rust) | edgeProxy (Rust) |
| **Plano de Controle** | routing.db (local) | Corrosion (replicado) |
| **Rede Privada** | WireGuard IPv4 | WireGuard IPv6 (6PN) |
| **Service Discovery** | Estático (manual) | Dinâmico (auto-registro) |
| **DNS Interno** | Nenhum | Domínios .internal |
| **Health Checks** | Passivo | Ativo + Passivo |

---

## Fases de Implementação

| Fase | Descrição | Status |
|------|-----------|--------|
| [Fase 1: DNS Interno](./phase-1-internal-dns) | Abstrair IPs de backend com nomes DNS | Planejado |
| [Fase 2: Corrosion](./phase-2-corrosion) | Plano de controle distribuído com replicação SQLite | Planejado |
| [Fase 3: Auto-Discovery](./phase-3-auto-discovery) | Registro automático de backends | Planejado |
| [Fase 4: IPv6 (6PN)](./phase-4-ipv6) | Rede privada IPv6 | Planejado |
| [Fase 5: Anycast BGP](./phase-5-anycast-bgp) | Roteamento de tráfego baseado em BGP | Planejado |
| [Fase 6: Health Checks](./phase-6-health-checks) | Monitoramento ativo de saúde | Planejado |

---

## Documentação Relacionada

- [Arquitetura](../architecture) - Arquitetura atual
- [WireGuard](../wireguard) - Detalhes do overlay de rede
- [Benchmarks](../benchmark) - Medições de performance
