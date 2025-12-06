---
sidebar_position: 3
---

# Gerenciamento de Nodes

Este documento descreve como gerenciar nodes backend no edgeProxy usando os comandos do Taskfile.

## Visão Geral

O edgeProxy descobre backends dinamicamente a partir do banco de dados SQLite `routing.db`. O banco é tipicamente replicado em todos os POPs via Corrosion (ou sistema similar de estado distribuído). Você pode gerenciar nodes usando os comandos do Taskfile.

## Schema do Node

Cada node backend possui os seguintes atributos:

| Campo | Tipo | Descrição |
|-------|------|-----------|
| `id` | TEXT | Identificador único (ex: `sa-node-1`) |
| `app` | TEXT | Nome da aplicação (padrão: `myapp`) |
| `region` | TEXT | Região geográfica (`sa`, `us`, `eu`, `ap`) |
| `wg_ip` | TEXT | Endereço IP WireGuard |
| `port` | INTEGER | Porta do backend (padrão: 8080) |
| `healthy` | INTEGER | Status de saúde (0=não saudável, 1=saudável) |
| `weight` | INTEGER | Peso no balanceamento (maior = mais tráfego) |
| `soft_limit` | INTEGER | Máximo preferido de conexões |
| `hard_limit` | INTEGER | Máximo absoluto de conexões |
| `deleted` | INTEGER | Flag de soft delete (0=ativo, 1=deletado) |

## Comandos

### Adicionar um Node

```bash
task node-add -- <id> <region> <wg_ip> [port] [weight]
```

**Exemplos:**

```bash
# Adicionar node com valores padrão (port=8080, weight=1)
task node-add -- sa-node-2 sa 10.50.1.2

# Adicionar node com porta customizada
task node-add -- us-node-2 us 10.50.2.2 9000

# Adicionar node com porta e peso customizados
task node-add -- eu-node-2 eu 10.50.3.2 8080 3
```

### Remover um Node (Soft Delete)

Marca o node como deletado sem removê-lo do banco. O node pode ser restaurado depois.

```bash
task node-remove -- <id>
```

**Exemplo:**

```bash
task node-remove -- sa-node-2
```

### Deletar um Node (Permanente)

Remove permanentemente o node do banco de dados.

```bash
task node-delete -- <id>
```

**Exemplo:**

```bash
task node-delete -- sa-node-2
```

### Habilitar um Node

Define `healthy=1` e `deleted=0`, tornando o node disponível para tráfego.

```bash
task node-enable -- <id>
```

**Exemplo:**

```bash
task node-enable -- sa-node-1
```

### Desabilitar um Node

Define `healthy=0`, impedindo que tráfego seja roteado para este node.

```bash
task node-disable -- <id>
```

**Exemplo:**

```bash
task node-disable -- sa-node-1
```

### Definir Peso do Node

Ajusta o peso no balanceamento de carga. Peso maior significa que o node recebe mais tráfego em relação aos outros.

```bash
task node-weight -- <id> <weight>
```

**Exemplo:**

```bash
# Dar a este node 3x mais tráfego
task node-weight -- sa-node-1 3
```

### Definir Limites de Conexão

Configura limites soft e hard de conexões para um node.

- **soft_limit**: Quando atingido, o balanceador começa a preferir outros nodes
- **hard_limit**: Máximo absoluto de conexões; novas conexões são rejeitadas

```bash
task node-limits -- <id> <soft_limit> <hard_limit>
```

**Exemplo:**

```bash
task node-limits -- sa-node-1 200 500
```

## Comandos de Visualização

### Mostrar Todos os Nodes

```bash
task db-show
```

### Mostrar Apenas Nodes Saudáveis

```bash
task db-healthy
```

## Algoritmo de Balanceamento

Ao selecionar um backend, o edgeProxy usa um sistema de pontuação:

```
score = region_score * 100 + (load_factor / weight)

onde:
  region_score = 0 (região do cliente = região do backend)
               = 1 (backend na mesma região do POP)
               = 2 (fallback para outras regiões)

  load_factor = conexões_atuais / soft_limit

  weight = peso configurado do backend (maior = preferido)
```

O backend com menor pontuação é selecionado, respeitando:
- Apenas backends saudáveis (`healthy = 1`)
- Backends abaixo do hard_limit
- Backends com soft delete são excluídos (`deleted = 0`)

## Atualizações Dinâmicas

O edgeProxy recarrega o `routing.db` periodicamente (padrão: a cada 5 segundos). Mudanças nos nodes entram em efeito automaticamente sem reiniciar o proxy.

Para alterar o intervalo de reload:

```bash
export EDGEPROXY_DB_RELOAD_SECS=10
```

## Boas Práticas

1. **Use soft delete** (`node-remove`) antes de deletar permanentemente para permitir recuperação
2. **Desabilite nodes** durante manutenção ao invés de removê-los
3. **Defina pesos apropriados** baseado na capacidade do node
4. **Configure limites** para prevenir sobrecarga durante picos de tráfego
5. **Monitore nodes saudáveis** com `task db-healthy` antes de deployments
