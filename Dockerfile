# =============================================================================
# Astra Chat Server - Multi-stage Dockerfile
# =============================================================================
# Build multi-arch con: docker buildx build --platform linux/amd64,linux/arm64 -t astra .
# Run con: docker run -p 5009:5009 -p 5010:5010 -p 5011:5011 -v astra-data:/app/data astra
# =============================================================================

# -------- Stage 1: build --------
FROM rust:1.83-alpine AS builder

# musl-dev es necesario para compilar algunas dependencias C de Rust
# pkgconfig para encontrar librerías del sistema
RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /app

# Layer cache: copiamos solo los manifests primero
# y creamos un dummy main.rs para pre-compilar las dependencias
COPY Cargo.toml Cargo.lock ./
COPY crates/crates ./crates/crates
COPY crates/proto-ares ./crates/proto-ares
COPY crates/iconnect ./crates/iconnect
COPY crates/server-core ./crates/server-core
COPY crates/udp ./crates/udp
COPY crates/captcha ./crates/captcha
COPY crates/commands ./crates/commands
COPY crates/scripting ./crates/scripting
COPY crates/web ./crates/web
COPY crates/link ./crates/link
COPY crates/astra ./crates/astra

# Pre-compilar dependencias (capa de cache)
RUN mkdir -p /tmp/dummy_crate/src && \
    echo "fn main() {}" > /tmp/dummy_crate/src/main.rs && \
    cd /tmp/dummy_crate && \
    echo '[package]' > Cargo.toml && \
    echo 'name = "dummy"' >> Cargo.toml && \
    echo 'version = "0.1.0"' >> Cargo.toml && \
    echo 'edition = "2021"' >> Cargo.toml && \
    echo '[dependencies]' >> Cargo.toml && \
    cargo build --release && \
    rm -rf target/release/deps/dummy*

# Build del binario real
RUN cargo build --release -p astra --bin astra

# -------- Stage 2: runtime --------
# Usamos distroless cc-debian12 (small, secure, no shell, no package manager)
FROM gcr.io/distroless/cc-debian12:nonroot

# Copiar el binario
COPY --from=builder /app/target/release/astra /app/astra

# Copiar los datos iniciales (seed, scripts)
COPY data /app/data

# Crear el directorio de datos (los volúmenes lo montarán encima)
# El usuario puede montar un volumen en /app/data para persistir
WORKDIR /app

# Astra escucha en:
#   5009 TCP (clientes Ares)
#   5010 TCP (clientes web WS)
#   5011 TCP (link hub/leaf)
#   5012 UDP (room search)
EXPOSE 5009 5010 5011 5012/udp

# Volume para datos persistentes
VOLUME ["/app/data"]

# Por defecto, iniciar el server (se puede sobreescribir con CLI args)
ENTRYPOINT ["/app/astra"]
CMD ["--port", "5009", "--data-dir", "/app/data"]
