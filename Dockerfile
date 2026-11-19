# --- Etapa 1: Compilación estática (musl) ---
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev ca-certificates

WORKDIR /app

# Cachear dependencias primero para acelerar compilaciones posteriores
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copiar código fuente real y compilar binario definitivo
COPY src ./src
RUN touch src/main.rs && \
    cargo build --release && \
    strip target/release/pulse-agent

# --- Etapa 2: Imagen final minimalista (scratch) ---
FROM scratch

LABEL org.opencontainers.image.title="pulse-agent"
LABEL org.opencontainers.image.description="Agente ultraligero de telemetría, monitoreo TUI y alertas en Rust"
LABEL org.opencontainers.image.source="https://github.com/jagelvisr/pulse-agent"
LABEL org.opencontainers.image.licenses="MIT"

# Certificados raíz CA necesarios para HTTPS (Discord, Telegram, Heartbeat, SSL checks)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Binario estático y configuración de ejemplo por defecto
COPY --from=builder /app/target/release/pulse-agent /usr/local/bin/pulse-agent
COPY pulse.example.toml /config/pulse.toml

ENTRYPOINT ["/usr/local/bin/pulse-agent"]
CMD ["--config", "/config/pulse.toml"]