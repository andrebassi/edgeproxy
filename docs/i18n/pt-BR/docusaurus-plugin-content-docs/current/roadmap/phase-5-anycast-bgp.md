---
sidebar_position: 5
---

# Fase 5: Anycast BGP

**Objetivo**: Substituir GeoDNS por BGP Anycast para failover instantâneo e roteamento ótimo.

## Estado Atual (GeoDNS)

![Estado Atual - GeoDNS](/img/roadmap/phase-5-geodns.svg)

## Estado Alvo (Anycast BGP)

![Estado Alvo - Anycast BGP](/img/roadmap/phase-5-anycast-bgp.svg)

## Requisitos BGP

| Requisito | Opções |
|-----------|--------|
| **ASN** | Privado (64512-65534) ou Público (de RIR) |
| **Bloco IP** | Mínimo /24 (256 IPs) de RIR ou provedor |
| **Trânsito** | Vultr, Packet, AWS Direct Connect |
| **Software** | BIRD, FRRouting, GoBGP |

## Opções de Implementação

**Opção A: BGP via Provedor de Cloud**
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

- **Failover instantâneo**: Sem esperar TTL DNS
- **Roteamento ótimo**: BGP encontra melhor caminho
- **Resiliência DDoS**: Tráfego distribuído globalmente
- **IP único**: Configuração de cliente simplificada

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 4: Rede Privada IPv6](./phase-4-ipv6)
- [Fase 6: Health Checks Ativos](./phase-6-health-checks)
