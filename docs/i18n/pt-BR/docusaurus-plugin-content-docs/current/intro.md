---
sidebar_position: 1
sidebar_label: "O que é?"
slug: /
---

# O que é o edgeProxy?

**edgeProxy** é um proxy TCP distribuído de alta performance escrito em Rust, projetado para operar em Points of Presence (POPs) ao redor do mundo. Ele roteia conexões de clientes para backends otimais baseado em proximidade geográfica, saúde do backend, carga atual e limites de capacidade.

## Funcionalidades Principais

- **Roteamento Geo-Aware**: Direciona clientes para o backend regional mais próximo usando MaxMind GeoIP
- **Afinidade de Cliente**: Sessões persistentes com TTL configurável garantem atribuição consistente de backend
- **Balanceamento de Carga Ponderado**: Pontuação inteligente baseada em região, carga e peso do backend
- **Limites Soft/Hard**: Degradação graciosa com limites de conexão por backend
- **Configuração Dinâmica**: Hot-reload do banco de roteamento sem reinicialização
- **Proxy Zero-Copy**: Cópia TCP bidirecional eficiente com Tokio
- **Pronto para WireGuard**: Projetado para conectividade via rede overlay entre POPs

## Casos de Uso

| Cenário | Descrição |
|---------|-----------|
| **CDN/Edge Computing** | POPs globais servindo conteúdo da origem mais próxima |
| **Servidores de Jogos** | Afinidade de sessão para conexões de jogos stateful |
| **APIs Multi-Região** | Failover automático e geo-roteamento |
| **Proxies de Banco de Dados** | Roteamento de réplicas de leitura baseado na localização do cliente |

## Visão Geral da Arquitetura

![Visão Geral da Arquitetura](/img/architecture-overview.svg)

## Início Rápido

```bash
# Clone o repositório
git clone https://github.com/andrebassi/edgeproxy.git
cd edgeproxy

# Build e execução
task build
task run

# Ou com Docker
task docker-build
task docker-up
task docker-test
```

## Stack Tecnológica

| Componente | Tecnologia |
|------------|------------|
| Linguagem | Rust 2021 Edition |
| Runtime Async | Tokio (full features) |
| Banco de Dados | SQLite (rusqlite) |
| Concorrência | DashMap (lock-free) |
| GeoIP | MaxMind GeoLite2 |
| Rede | WireGuard overlay |

## Próximos Passos

- [Primeiros Passos](./getting-started) - Instalação e primeira execução
- [Arquitetura](./architecture) - Deep dive no design do sistema
- [Configuração](./configuration) - Variáveis de ambiente e opções
- [Deploy com Docker](./deployment/docker) - Deployment baseado em containers
