---
sidebar_position: 3
---

# Fase 3: Auto-Discovery e Registro

**Objetivo**: Backends se registram/desregistram automaticamente no plano de controle.

## Estado Atual

```sql
-- Insert SQL manual
INSERT INTO backends (id, app, region, wg_ip, port, healthy)
VALUES ('nrt-1', 'echo', 'ap', '10.50.4.1', 8080, 1);
```

## Estado Alvo

```rust
// Backend auto-registra no startup
async fn register_backend(control_plane: &ControlPlane) {
    control_plane.register(Backend {
        id: generate_id(),
        app: env::var("APP_NAME"),
        region: detect_region(),
        wg_ip: get_wireguard_ip(),
        port: env::var("PORT"),
        metadata: collect_metadata(),
    }).await;
}

// Heartbeat mantém registro ativo
loop {
    control_plane.heartbeat().await;
    sleep(Duration::from_secs(10)).await;
}
```

## Fluxo de Registro

![Fluxo de Registro](/img/roadmap/phase-3-auto-discovery.svg)

## Benefícios

- **Zero configuração**: Backends apenas iniciam
- **Scaling automático**: Novas instâncias aparecem automaticamente
- **Shutdown graceful**: Desregistro limpo
- **Integração com health**: Unhealthy = desregistrado

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 2: Plano de Controle Distribuído](./phase-2-corrosion)
- [Fase 4: Rede Privada IPv6](./phase-4-ipv6)
