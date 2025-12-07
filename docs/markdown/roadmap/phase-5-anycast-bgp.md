---
sidebar_position: 5
---

# Phase 5: Anycast BGP

**Goal**: Replace GeoDNS with BGP Anycast for instant failover and optimal routing.

## Current State (GeoDNS)

![Current State - GeoDNS](/img/roadmap/phase-5-geodns.svg)

## Target State (Anycast BGP)

![Target State - Anycast BGP](/img/roadmap/phase-5-anycast-bgp.svg)

## BGP Requirements

| Requirement | Options |
|-------------|---------|
| **ASN** | Private (64512-65534) or Public (from RIR) |
| **IP Block** | /24 minimum (256 IPs) from RIR or provider |
| **Transit** | Vultr, Packet, AWS Direct Connect |
| **Software** | BIRD, FRRouting, GoBGP |

## Implementation Options

**Option A: Cloud Provider BGP**
- Vultr BGP (~$5/month per location)
- Packet/Equinix Metal (native BGP)
- AWS Global Accelerator (managed anycast)

**Option B: Own ASN + IP Space**
- Register ASN with RIR (ARIN, RIPE, APNIC)
- Acquire /24 IP block
- Establish peering agreements

## BIRD Configuration Example

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

## Benefits

- **Instant failover**: No DNS TTL wait
- **Optimal routing**: BGP finds best path
- **DDoS resilience**: Traffic distributed globally
- **Single IP**: Simpler client configuration

## Related

- [Roadmap Overview](../roadmap/)
- [Phase 4: IPv6 Private Network](./phase-4-ipv6)
- [Phase 6: Active Health Checks](./phase-6-health-checks)
