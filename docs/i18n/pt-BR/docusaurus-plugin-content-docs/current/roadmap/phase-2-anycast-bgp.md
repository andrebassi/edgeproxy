---
sidebar_position: 2
---

# Fase 2: Anycast BGP

**Objetivo**: Substituir GeoDNS por BGP Anycast para failover instantâneo e roteamento ótimo.

## Estado Atual (GeoDNS)

![Estado Atual - GeoDNS](/img/roadmap/phase-5-geodns.svg)

## Estado Alvo (Anycast BGP)

![Estado Alvo - Anycast BGP](/img/roadmap/phase-5-anycast-bgp.svg)

## Requisitos BGP

| Requisito | Opções |
|-----------|--------|
| **ASN** | Privado (64512-65534) ou Público (do RIR) |
| **Bloco IP** | /24 mínimo (256 IPs) do RIR ou provedor |
| **Trânsito** | Vultr, Packet, AWS Direct Connect |
| **Software** | BIRD, FRRouting, GoBGP |

## Opções de Implementação

**Opção A: BGP de Provedor Cloud**
- Vultr BGP (~$5/mês por localização)
- Packet/Equinix Metal (BGP nativo)
- AWS Global Accelerator (anycast gerenciado)

**Opção B: ASN + Espaço IP Próprio**
- Registrar ASN com RIR (ARIN, RIPE, APNIC)
- Adquirir bloco IP /24
- Estabelecer acordos de peering

## Exemplo de Configuração BIRD

```
# /etc/bird/bird.conf
router id 10.50.5.1;

protocol bgp vultr {
    local as 64512;
    neighbor 169.254.169.254 as 64515;

    ipv4 {
        import none;
        export where net = 198.51.100.0/24;
    };
}

protocol static {
    ipv4;
    route 198.51.100.0/24 blackhole;
}
```

## Benefícios

- **Failover instantâneo**: Sem espera de TTL DNS
- **Roteamento ótimo**: BGP encontra melhor caminho
- **Resiliência DDoS**: Tráfego distribuído globalmente
- **IP único**: Configuração de cliente mais simples

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 1: Rede Privada IPv6](./phase-1-ipv6)
- [Fase 3: Health Checks Ativos](./phase-3-health-checks)
