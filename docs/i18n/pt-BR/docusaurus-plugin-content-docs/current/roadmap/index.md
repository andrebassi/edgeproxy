---
sidebar_position: 0
---

# Roadmap de Arquitetura Futura

Este documento descreve a evolução planejada do edgeProxy em direção a uma plataforma de edge computing totalmente distribuída e auto-recuperável.

:::tip Versão Atual: 0.3.0
O edgeProxy agora inclui **terminação TLS**, **API de Auto-Discovery**, **DNS Interno** e **replicação built-in** (SWIM gossip + QUIC transport). Veja [Configuração](../configuration) para detalhes.
:::

## Princípios de Design

O edgeProxy segue padrões comprovados de plataformas edge em produção:

- **WireGuard como Fundação**: Toda comunicação interna flui sobre mesh WireGuard. Ele fornece o **backhaul** entre POPs - a rede interna que transporta tráfego entre datacenters. Quando um usuário conecta ao servidor edge mais próximo mas seu backend roda em uma região diferente, o proxy roteia transparentemente através de túneis WireGuard de baixa latência ao invés de voltar pela internet pública.

![WireGuard Backhaul](/img/roadmap/backhaul-diagram.svg)

- **Rust + Tokio para Performance**: Componentes de caminho crítico construídos em Rust usando runtime async Tokio para latência previsível sem pausas de GC.
- **6PN (Rede Privada IPv6)**: Conectividade interna usa endereçamento privado IPv6, habilitando espaço de endereços ilimitado para backends e serviços.
- **Rede Global Anycast**: Único endereço IP anunciado de múltiplas localizações, com BGP tratando roteamento ótimo.

---

## Comparação de Arquitetura

### Arquitetura Atual vs Alvo

![Arquitetura Futura](/img/architecture-future.svg)

| Componente | v1 (Atual) | v2 (Alvo) |
|------------|------------|-----------|
| **Roteamento de Tráfego** | GeoDNS | Anycast BGP |
| **Edge Proxy** | edgeProxy (Rust) | edgeProxy (Rust) |
| **Control Plane** | routing.db (local) | Replicação built-in (replicado) |
| **Rede Privada** | WireGuard IPv4 | WireGuard IPv6 (6PN) |
| **Service Discovery** | Estático (manual) | Dinâmico (auto-registro) |
| **DNS Interno** | Nenhum | domínios .internal |
| **Health Checks** | Passivo | Ativo + Passivo |

---

## Funcionalidades Completadas (v0.2.0)

As seguintes funcionalidades foram implementadas e estão documentadas em [Configuração](../configuration):

| Funcionalidade | Descrição | Documentação |
|----------------|-----------|--------------|
| **Terminação TLS** | Suporte HTTPS com certificados auto-gerados ou customizados | [Variáveis de Ambiente](../configuration/environment-variables#configurações-tls) |
| **DNS Interno** | Resolução de domínios `.internal` geo-aware | [Servidor DNS](../configuration/dns-server) |
| **API de Auto-Discovery** | Registro/desregistro dinâmico de backends | [API de Auto-Discovery](../configuration/auto-discovery-api) |
| **Replicação Built-in** | Replicação SQLite distribuída entre POPs (SWIM + QUIC) | [Replicação Built-in](../configuration/replication) |
| **490 Testes Unitários** | Cobertura abrangente de testes | [Testes](../testing) |

---

## Fases de Implementação

| Fase | Descrição | Status |
|------|-----------|--------|
| [Fase 1: IPv6 (6PN)](./phase-1-ipv6) | Rede privada IPv6 | Planejado |
| [Fase 2: Anycast BGP](./phase-2-anycast-bgp) | Roteamento de tráfego baseado em BGP | Planejado |
| [Fase 3: Health Checks](./phase-3-health-checks) | Monitoramento ativo de saúde | Planejado |

---

## Documentação Relacionada

- [Arquitetura](../architecture) - Arquitetura atual
- [Configuração](../configuration) - Todas as variáveis de ambiente e funcionalidades
- [WireGuard](../wireguard) - Detalhes do overlay de rede
- [Benchmarks](../benchmark) - Medições de performance
