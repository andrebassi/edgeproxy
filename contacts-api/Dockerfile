FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY go.mod go.sum* ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o contacts-api .

FROM alpine:3.19

RUN apk add --no-cache ca-certificates wireguard-tools iptables

WORKDIR /app
COPY --from=builder /app/contacts-api .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

EXPOSE 8080

CMD ["./entrypoint.sh"]
