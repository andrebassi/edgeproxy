FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY main.go .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o backend main.go

FROM alpine:3.19

# Instalar WireGuard e ferramentas
RUN apk --no-cache add ca-certificates wireguard-tools iptables ip6tables iproute2 bash

WORKDIR /app
COPY --from=builder /app/backend .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

EXPOSE 8080
EXPOSE 51820/udp

# Usar entrypoint para configurar WireGuard antes de iniciar o backend
ENTRYPOINT ["./entrypoint.sh"]
