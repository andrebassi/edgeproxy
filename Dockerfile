# Build stage
FROM rust:1.83-alpine AS builder

RUN apk add --no-cache musl-dev sqlite-dev

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock* ./

# Create dummy src to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Copy actual source
COPY src ./src

# Build the actual binary
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM alpine:3.19

RUN apk add --no-cache sqlite sqlite-libs ca-certificates

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/edge-proxy /app/edge-proxy

# Copy SQL schema
COPY sql ./sql

# Create empty routing.db
RUN sqlite3 /app/routing.db < /app/sql/create_routing_db.sql

# Environment defaults
ENV EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
ENV EDGEPROXY_DB_PATH=/app/routing.db
ENV EDGEPROXY_REGION=sa
ENV EDGEPROXY_DB_RELOAD_SECS=5
ENV EDGEPROXY_BINDING_TTL_SECS=600
ENV EDGEPROXY_BINDING_GC_INTERVAL_SECS=60
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/edge-proxy"]
