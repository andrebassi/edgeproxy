---
sidebar_position: 4
---

# Servidor DNS Interno

O servidor DNS fornece resolução de nomes geo-aware para domínios `.internal`.

## Uso

```bash
# Habilitar servidor DNS
export EDGEPROXY_DNS_ENABLED=true
export EDGEPROXY_DNS_LISTEN_ADDR=0.0.0.0:5353
export EDGEPROXY_DNS_DOMAIN=internal

# Consultar melhor IP de backend (geo-aware)
dig @localhost -p 5353 myapp.internal A

# Resposta: Melhor IP de backend baseado na localização do cliente
;; ANSWER SECTION:
myapp.internal.    300    IN    A    10.50.1.5
```

## Schema DNS

| Domínio | Resolve Para | Exemplo |
|---------|--------------|---------|
| `<app>.internal` | Melhor IP de backend | `myapp.internal` → `10.50.1.5` |
| `<region>.backends.internal` | IP WG do backend | `nrt.backends.internal` → `10.50.4.1` |
| `<region>.pops.internal` | IP WG do POP | `hkg.pops.internal` → `10.50.5.1` |

## Configuração

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `EDGEPROXY_DNS_ENABLED` | `false` | Habilitar servidor DNS |
| `EDGEPROXY_DNS_LISTEN_ADDR` | `0.0.0.0:5353` | Endereço DNS |
| `EDGEPROXY_DNS_DOMAIN` | `internal` | Sufixo do domínio DNS |

## Benefícios

- **Abstração**: Mude IPs sem atualizar configs
- **Migração**: Mova backends sem downtime
- **Geo-aware**: Retorna melhor backend baseado na localização do cliente

## Exemplos de Integração

### Docker Compose

```yaml
services:
  app:
    dns: edgeproxy
    environment:
      - API_HOST=backend.internal
```

### Configuração de Aplicação

```bash
# Em vez de hardcode de IPs
export BACKEND_HOST=myapp.internal

# Aplicação resolve via DNS do edgeProxy
curl http://myapp.internal:8080/api
```
