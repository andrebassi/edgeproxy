---
sidebar_position: 1
---

# Fase 1: Rede Privada IPv6 (6PN)

**Objetivo**: Migrar de IPv4 para IPv6 para espaço de endereços ilimitado e networking moderno.

## Estado Atual

```
Rede: 10.50.0.0/16
Endereços: ~65.000
Protocolo: IPv4 sobre WireGuard
```

## Estado Alvo

```
Rede: fd00:edgeproxy::/48
Endereços: 2^80 (ilimitado)
Protocolo: IPv6 sobre WireGuard
```

## Esquema de Endereços

```
fd00:edgeproxy:RRRR:BBBB::1

Onde:
  fd00:edgeproxy = Prefixo ULA (Unique Local Address)
  RRRR          = Código da região (0001=EU, 0002=US, 0003=SA, 0004=AP)
  BBBB          = ID do Backend
  ::1           = Número da instância
```

## Exemplos

| Backend | IPv4 (atual) | IPv6 (futuro) |
|---------|--------------|---------------|
| EC2 Irlanda | 10.50.0.1 | fd00:edgeproxy:0001:0001::1 |
| GRU | 10.50.1.1 | fd00:edgeproxy:0003:0001::1 |
| NRT | 10.50.4.1 | fd00:edgeproxy:0004:0001::1 |
| HKG POP | 10.50.5.1 | fd00:edgeproxy:0004:0100::1 |

## Transição Dual-Stack

```
Fase 1a: Adicionar IPv6 junto com IPv4 (dual-stack)
Fase 1b: Preferir IPv6 para novas conexões
Fase 1c: Depreciar tráfego interno IPv4
```

## Benefícios

- **Escala ilimitada**: Sem exaustão de endereços
- **Padrão moderno**: Aplicações nativas IPv6
- **Roteamento simplificado**: Endereçamento hierárquico
- **Prova de futuro**: Pronto para a próxima década

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 2: Anycast BGP](./phase-2-anycast-bgp)
