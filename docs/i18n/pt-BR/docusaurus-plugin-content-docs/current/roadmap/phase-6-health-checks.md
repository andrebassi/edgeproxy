---
sidebar_position: 6
---

# Fase 6: Health Checks Ativos

**Objetivo**: Monitoramento proativo de saúde ao invés de detecção reativa de falhas.

## Estado Atual (Passivo)

```rust
// Só detecta falha quando conexão falha
match TcpStream::connect(backend).await {
    Ok(stream) => use_backend(stream),
    Err(_) => mark_unhealthy(backend), // Tarde demais!
}
```

## Estado Alvo (Ativo + Passivo)

```rust
// Health checker em background
async fn health_checker(backends: Vec<Backend>) {
    loop {
        for backend in &backends {
            let health = check_health(backend).await;
            update_health_status(backend, health);
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn check_health(backend: &Backend) -> HealthStatus {
    // Verificação TCP
    let tcp_ok = tcp_connect(backend, timeout).await.is_ok();

    // Verificação HTTP (se aplicável)
    let http_ok = http_get(backend, "/health").await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    // Medição de RTT
    let rtt = measure_rtt(backend).await;

    HealthStatus { tcp_ok, http_ok, rtt }
}
```

## Tipos de Health Check

| Tipo | Protocolo | Verificação | Frequência |
|------|-----------|-------------|------------|
| **TCP** | L4 | Porta aberta | 5s |
| **HTTP** | L7 | GET /health retorna 2xx | 10s |
| **gRPC** | L7 | grpc.health.v1.Health | 10s |
| **Custom** | Qualquer | Script definido pelo usuário | Configurável |

## Benefícios

- **Detecção proativa**: Sabe antes dos usuários reclamarem
- **Degradação gradual**: Soft limit antes de hard failure
- **Roteamento baseado em RTT**: Roteia para backend mais rápido
- **Integração com alertas**: Notifica em mudanças de saúde

## Relacionado

- [Visão Geral do Roadmap](../roadmap/)
- [Fase 5: Anycast BGP](./phase-5-anycast-bgp)
