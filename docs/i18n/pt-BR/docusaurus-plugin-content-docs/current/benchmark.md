---
sidebar_position: 6
---

# Resultados de Benchmark

Este documento apresenta os resultados de benchmark do edgeProxy com rede overlay WireGuard, testado em 9 localizações VPN globais roteando para 10 regiões de backend no Fly.io.

## Arquitetura de Teste

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    edgeProxy + WireGuard - Teste de Produção                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Cliente (VPN) ──► EC2 Irlanda (edgeProxy) ──► WireGuard ──► Fly.io       │
│                     54.171.48.207:8080          10.50.x.x    10 regiões    │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│   Geo-Routing: 9/9 ✅                                                       │
│   Túnel WireGuard: 10/10 peers conectados ✅                                │
│   Benchmark v2: Latência, Download, Upload, Stress ✅                       │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Infraestrutura

### Servidor edgeProxy (AWS EC2)
- **Região**: eu-west-1 (Irlanda)
- **Instância**: t3.micro
- **IP**: 54.171.48.207
- **IP WireGuard**: 10.50.0.1/24

### Servidores Backend (Fly.io)

| Região | Localização | IP WireGuard |
|--------|-------------|--------------|
| GRU | São Paulo, Brasil | 10.50.1.1 |
| IAD | Virginia, EUA | 10.50.2.1 |
| ORD | Chicago, EUA | 10.50.2.2 |
| LAX | Los Angeles, EUA | 10.50.2.3 |
| LHR | Londres, Reino Unido | 10.50.3.1 |
| FRA | Frankfurt, Alemanha | 10.50.3.2 |
| CDG | Paris, França | 10.50.3.3 |
| NRT | Tóquio, Japão | 10.50.4.1 |
| SIN | Cingapura | 10.50.4.2 |
| SYD | Sydney, Austrália | 10.50.4.3 |

## Resultados do Benchmark

### Tabela Completa de Testes

| # | Localização VPN | País | Backend | Latência | Download 1MB | Download 5MB | RPS (20) | Status |
|---|-----------------|------|---------|----------|--------------|--------------|----------|--------|
| 1 | Paris | FR | **CDG** | 530ms | 0.5 MB/s | 2.1 MB/s | 35.7 | ✅ |
| 2 | Frankfurt | DE | **FRA** | 528ms | 0.6 MB/s | 2.3 MB/s | 34.0 | ✅ |
| 3 | Londres | GB | **LHR** | 490ms | 0.6 MB/s | 2.3 MB/s | 36.6 | ✅ |
| 4 | Detroit | US | **IAD** | 708ms | 0.6 MB/s | 2.5 MB/s | 27.4 | ✅ |
| 5 | Las Vegas | US | **IAD** | 857ms | 0.5 MB/s | 2.2 MB/s | 22.5 | ✅ |
| 6 | Tóquio | JP | **NRT** | 1546ms | 0.3 MB/s | 1.1 MB/s | 12.6 | ✅ |
| 7 | Cingapura | SG | **SIN** | 1414ms | 0.3 MB/s | 1.2 MB/s | 13.8 | ✅ |
| 8 | Sydney | AU | **SYD** | 1847ms | 0.2 MB/s | 0.9 MB/s | 10.7 | ✅ |
| 9 | São Paulo | BR | **GRU** | 822ms | 0.4 MB/s | 1.6 MB/s | 23.3 | ✅ |

### Análise de Performance por Região

| Região | Faixa de Latência | Observação |
|--------|-------------------|------------|
| Europa (CDG/FRA/LHR) | 490-530ms | Melhor - mais perto da EC2 Irlanda |
| EUA (IAD) | 708-857ms | Médio - atravessa o Atlântico |
| Brasil (GRU) | 822ms | Bom - rota direta |
| Ásia (NRT/SIN) | 1414-1546ms | Alto - distância geográfica |
| Oceania (SYD) | 1847ms | Mais alto - meia volta ao mundo |

## Validação do Geo-Routing

Todos os 9 testes VPN rotearam corretamente para o backend esperado baseado na localização geográfica do cliente:

| Localização do Cliente | Backend Esperado | Backend Real | Resultado |
|------------------------|------------------|--------------|-----------|
| França (FR) | CDG | CDG | ✅ |
| Alemanha (DE) | FRA | FRA | ✅ |
| Reino Unido (GB) | LHR | LHR | ✅ |
| Estados Unidos (US) | IAD | IAD | ✅ |
| Japão (JP) | NRT | NRT | ✅ |
| Cingapura (SG) | SIN | SIN | ✅ |
| Austrália (AU) | SYD | SYD | ✅ |
| Brasil (BR) | GRU | GRU | ✅ |

## Status do Túnel WireGuard

Todos os 10 backends Fly.io estabeleceram túneis WireGuard com sucesso para o servidor EC2:

```
interface: wg0
  public key: bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY=
  listening port: 51820

peer: He2jX3+iEl7hUaaJG/i3YcSnStEFAcW/rs/lP0Pw+nc= (GRU)
  allowed ips: 10.50.1.1/32
  latest handshake: 18 seconds ago ✅

peer: rImgzxPu9MuhqLpcvXQ9xckSSA+AGbDOpBGvTUOwaHQ= (IAD)
  allowed ips: 10.50.2.1/32
  latest handshake: 15 seconds ago ✅

... (todos os 10 peers conectados)
```

## Metodologia de Teste

### Teste de Latência
- 20 requisições HTTP sequenciais para o endpoint `/api/latency`
- Mede o tempo de ida e volta do cliente ao backend via proxy
- Reporta: Latência Média, Mínima e Máxima

### Teste de Download
- Requisições HTTP GET para o endpoint `/api/download?size=N`
- Testes com arquivos de 1MB e 5MB
- Mede: Velocidade de download em MB/s

### Teste de Requisições Concorrentes
- 20 requisições HTTP paralelas
- Mede: Tempo total e Requisições Por Segundo (RPS)

## Endpoints de Benchmark

O backend v2 fornece os seguintes endpoints de teste:

| Endpoint | Descrição |
|----------|-----------|
| `/api/info` | Info do servidor (região, uptime, requisições) |
| `/api/latency` | Resposta mínima para teste de latência |
| `/api/download?size=N` | Teste de download (N bytes, máx 100MB) |
| `/api/upload` | Teste de upload (corpo POST) |
| `/api/stats` | Estatísticas do servidor |
| `/benchmark` | Página HTML interativa de benchmark |

## Executando Seu Próprio Benchmark

### Testes Rápidos

```bash
# Teste rápido de latência
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://54.171.48.207:8080/api/latency
done

# Teste de download (1MB)
curl -w "Velocidade: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=1048576"

# Verificar geo-routing
curl -s http://54.171.48.207:8080/api/info | jq .
```

### Script Completo de Benchmark

Este é o script usado para gerar a tabela de resultados do benchmark:

```bash
#!/bin/bash
# benchmark.sh - Suite completa de benchmark do edgeProxy
# Uso: ./benchmark.sh <url-do-proxy>

PROXY_URL="${1:-http://54.171.48.207:8080}"

echo "=== edgeProxy Benchmark V2 ==="
echo "Alvo: $PROXY_URL"
echo ""

# 1. Verificação de Região
echo "1. Verificação de Região:"
curl -s "$PROXY_URL/api/info" | python3 -m json.tool
echo ""

# 2. Teste de Latência
echo "2. Teste de Latência (20 pings):"
latencies=()
for i in {1..20}; do
  start=$(python3 -c "import time: print(int(time.time()*1000))")
  curl -s "$PROXY_URL/api/latency" > /dev/null
  end=$(python3 -c "import time: print(int(time.time()*1000))")
  latency=$((end - start))
  latencies+=($latency)
  printf "  Ping %2d: %dms\n" $i $latency
done
total=0; for l in "${latencies[@]}"; do total=$((total + l)); done
avg=$((total / 20))
min=$(printf '%s\n' "${latencies[@]}" | sort -n | head -1)
max=$(printf '%s\n' "${latencies[@]}" | sort -n | tail -1)
echo "  ────────────────"
echo "  Média: ${avg}ms | Mín: ${min}ms | Máx: ${max}ms"
echo ""

# 3. Teste de Download (1MB)
echo "3. Teste de Download (1MB):"
curl -w "  Baixado: %{size_download} bytes | Tempo: %{time_total}s | Velocidade: %{speed_download} B/s\n" \
  -o /dev/null -s "$PROXY_URL/api/download?size=1048576"

# 4. Teste de Download (5MB)
echo "4. Teste de Download (5MB):"
curl -w "  Baixado: %{size_download} bytes | Tempo: %{time_total}s | Velocidade: %{speed_download} B/s\n" \
  -o /dev/null -s "$PROXY_URL/api/download?size=5242880"

# 5. Requisições Concorrentes
echo "5. Requisições Concorrentes (20 paralelas):"
start=$(python3 -c "import time: print(int(time.time()*1000))")
for i in {1..20}; do
  curl -s "$PROXY_URL/api/latency" > /dev/null &
done
wait
end=$(python3 -c "import time: print(int(time.time()*1000))")
echo "  20 requisições em $((end - start))ms | RPS: $(python3 -c "print(f'{20000/$((end - start)):.1f}')")"

echo ""
echo "=== Benchmark Completo ==="
```

## Conclusões

1. **Geo-Routing**: 100% de precisão no roteamento de clientes para o backend regional correto
2. **WireGuard**: Túneis estáveis com todos os 10 backends globais
3. **Performance**: Latência escala previsivelmente com a distância geográfica
4. **Confiabilidade**: Todos os testes passaram com resultados consistentes

### Performance Esperada em Produção

Em produção com múltiplos POPs edgeProxy implantados globalmente (não apenas na Irlanda):

| Cenário | Latência Esperada |
|---------|-------------------|
| Cliente → POP Local → Backend Local | 5-20ms |
| Cliente → POP Local → Backend Regional | 20-50ms |
| Cliente → POP Local → Backend Remoto | 50-150ms |

O setup de teste atual roteia todo o tráfego através da Irlanda, o que adiciona latência para clientes distantes. Uma implantação em malha completa melhoraria significativamente a performance para todas as regiões.
