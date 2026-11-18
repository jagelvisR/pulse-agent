# Etapa 1: Compilación estática con musl
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

# Cachear dependencias primero
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl
RUN rm -rf src

# Copiar código fuente real y compilar
COPY src ./src
RUN touch src/main.rs && cargo build --release --target x86_64-unknown-linux-musl

# Etapa 2: Imagen final ultra-ligera (~6 MB)
FROM scratch

# Certificados raíz para HTTPS (Discord, Telegram, HTTP checks)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/pulse-agent /pulse-agent

ENTRYPOINT ["/pulse-agent"]
CMD ["--config", "/config/pulse.toml"]