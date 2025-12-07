---
sidebar_position: 2
---

# Benchmarks

Este documento apresenta os resultados de benchmark do edgeProxy com rede overlay WireGuard em localizações globais.

:::info Setup da Infraestrutura
Para detalhes sobre como configurar a infraestrutura AWS EC2 e WireGuard usada nestes testes, veja [Deploy AWS EC2](./deployment/aws).
:::

---

## Benchmark v2 (Atual)

:::tip Todos os Testes Passaram
- **Geo-Routing:** 9/9
- **WireGuard:** 10/10 peers
- **Status:** Completo
:::

### Resultados dos Testes

| # | Localização VPN | País | Backend | Latência | Download 1MB | Download 5MB | RPS (20) | Status |
|---|-----------------|------|---------|----------|--------------|--------------|----------|--------|
| 1 | 🇫🇷 Paris | FR | **CDG** | 530ms | 0.5 MB/s | 2.1 MB/s | 35.7 | ✅ |
| 2 | 🇩🇪 Frankfurt | DE | **FRA** | 528ms | 0.6 MB/s | 2.3 MB/s | 34.0 | ✅ |
| 3 | 🇬🇧 Londres | GB | **LHR** | 490ms | 0.6 MB/s | 2.3 MB/s | 36.6 | ✅ |
| 4 | 🇺🇸 Detroit | US | **IAD** | 708ms | 0.6 MB/s | 2.5 MB/s | 27.4 | ✅ |
| 5 | 🇺🇸 Las Vegas | US | **IAD** | 857ms | 0.5 MB/s | 2.2 MB/s | 22.5 | ✅ |
| 6 | 🇯🇵 Tóquio | JP | **NRT** | 1546ms | 0.3 MB/s | 1.1 MB/s | 12.6 | ✅ |
| 7 | 🇸🇬 Singapura | SG | **SIN** | 1414ms | 0.3 MB/s | 1.2 MB/s | 13.8 | ✅ |
| 8 | 🇦🇺 Sydney | AU | **SYD** | 1847ms | 0.2 MB/s | 0.9 MB/s | 10.7 | ✅ |
| 9 | 🇧🇷 São Paulo | BR | **GRU** | 822ms | 0.4 MB/s | 1.6 MB/s | 23.3 | ✅ |

### Performance por Região

| Região | Latência | Observação |
|--------|----------|------------|
| 🇪🇺 Europa (CDG/FRA/LHR) | 490-530ms | Melhor - mais próximo do EC2 Irlanda |
| 🇺🇸 EUA (IAD) | 708-857ms | Médio - atravessa Atlântico |
| 🇧🇷 Brasil (GRU) | 822ms | Bom - rota direta |
| 🇯🇵🇸🇬 Ásia (NRT/SIN) | 1414-1546ms | Alto - distância geográfica |
| 🇦🇺 Oceania (SYD) | 1847ms | Mais alto - metade do mundo |

---

## Arquitetura de Teste

![Arquitetura do Benchmark](/img/benchmark-architecture.svg)

---

## Validação do Geo-Routing

Todos os 9 testes VPN rotearam corretamente para o backend esperado:

| Localização do Cliente | Esperado | Atual | Resultado |
|------------------------|----------|-------|-----------|
| 🇫🇷 França | CDG | CDG | ✅ |
| 🇩🇪 Alemanha | FRA | FRA | ✅ |
| 🇬🇧 Reino Unido | LHR | LHR | ✅ |
| 🇺🇸 Estados Unidos | IAD | IAD | ✅ |
| 🇯🇵 Japão | NRT | NRT | ✅ |
| 🇸🇬 Singapura | SIN | SIN | ✅ |
| 🇦🇺 Austrália | SYD | SYD | ✅ |
| 🇧🇷 Brasil | GRU | GRU | ✅ |

---

## Executando Seus Próprios Testes

### Teste Rápido de Latência

```bash
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://54.171.48.207:8080/api/latency
done
```

### Verificar Geo-Routing

```bash
curl -s http://54.171.48.207:8080/api/info | jq .
# Retorna: {"region":"cdg","region_name":"Paris, France",...}
```

### Teste de Velocidade de Download

```bash
# Download de 1MB
curl -w "Velocidade: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=1048576"

# Download de 5MB
curl -w "Velocidade: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=5242880"
```

### Script de Benchmark Completo

Use o script fornecido em `scripts/benchmark.sh`:

```bash
./scripts/benchmark.sh http://54.171.48.207:8080
```

---

## Endpoints de Benchmark

| Endpoint | Descrição |
|----------|-----------|
| `/` | Banner ASCII art com info da região |
| `/api/info` | Info do servidor em JSON (região, uptime, requests) |
| `/api/latency` | Resposta mínima para teste de latência |
| `/api/download?size=N` | Teste de download (N bytes, máx 100MB) |
| `/api/upload` | Teste de upload (corpo POST) |
| `/api/stats` | Estatísticas do servidor |
| `/benchmark` | Página HTML interativa de benchmark |

---

## Conclusões

1. **Geo-Routing**: 100% de precisão roteando clientes para backend regional correto
2. **WireGuard**: Túneis estáveis com todos os 10 backends globais
3. **Performance**: Latência escala previsivelmente com distância geográfica
4. **Confiabilidade**: Todos os testes passaram com resultados consistentes

### Deploy de Produção

Para produção, faça deploy de POPs edgeProxy em múltiplas regiões:

| Cenário | Latência Esperada |
|---------|-------------------|
| Cliente → POP Local → Backend Local | 5-20ms |
| Cliente → POP Local → Backend Regional | 20-50ms |
| Cliente → POP Local → Backend Remoto | 50-150ms |

O setup de teste roteia todo tráfego pela Irlanda. Um deploy full mesh melhoraria significativamente a performance global.

---

## Benchmark v1 (Inicial)

Teste de validação inicial com regiões limitadas para verificar funcionalidade de geo-routing.

:::info Escopo do Teste
- **Regiões testadas:** 3 (foco na Europa)
- **Objetivo:** Validar geo-routing básico e conectividade WireGuard
:::

### Resultados dos Testes

| # | Localização VPN | País | Backend | Latência | Status |
|---|-----------------|------|---------|----------|--------|
| 1 | 🇫🇷 Paris | FR | **CDG** | ~500ms | ✅ |
| 2 | 🇩🇪 Frankfurt | DE | **FRA** | ~520ms | ✅ |
| 3 | 🇬🇧 Londres | GB | **LHR** | ~480ms | ✅ |

### Melhorias v1 → v2

| Aspecto | v1 | v2 |
|---------|----|----|
| Regiões testadas | 3 | 9 |
| Métricas | Apenas latência | Latência, Download, RPS |
| Cobertura global | Apenas Europa | 5 continentes |
| Peers WireGuard | 3 | 10 |

---

## Documentação Relacionada

- [Deploy AWS EC2](./deployment/aws) - Guia de setup da infraestrutura
- [Deploy Fly.io](./deployment/flyio) - Deploy global na edge
- [Deploy Docker](./deployment/docker) - Desenvolvimento local
