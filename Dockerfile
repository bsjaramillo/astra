# =============================================================================
# Astra Chat Server - Multi-stage Dockerfile
# =============================================================================
# Build:  docker build -t astra:local .
# Build: docker buildx build --platform linux/amd64 -t astra .
# Run (bind mount + tu usuario, datos accesibles desde el host en ./data):
#   mkdir -p data
#   docker run -p 5009:5009 -p 5009:5009/udp \
#     --user "$(id -u):$(id -g)" -v "$PWD/data:/app/data" astra
# (o simplemente: docker compose up -d)
# =============================================================================

# -------- Stage 1: build --------
FROM rust:1.96-alpine AS builder

# build-base: gcc/make/musl-dev (rusqlite compila SQLite desde C con `bundled`).
# pkgconfig: para localizar librerías del sistema.
RUN apk add --no-cache build-base pkgconfig

WORKDIR /app

# El build solo necesita los manifests del workspace y el código de los crates.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Compilar únicamente el binario del server.
RUN cargo build --release -p astra --bin astra

# -------- Stage 2: runtime --------
# distroless cc-debian12: chico, seguro, sin shell ni package manager.
FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

# Binario + datos iniciales (scripts de ejemplo, etc.).
# El data dir se copia como `nonroot` (UID 65532) para que el usuario del
# contenedor pueda escribir la DB SQLite. Al montar un volumen vacío encima,
# Docker hereda esta propiedad, así que el volumen queda escribible.
COPY --from=builder /app/target/release/astra /app/astra
COPY --chown=65532:65532 data /app/data

# Astra multiplexa TCP (Ares), WebSocket (web/admin), Link y UDP en un solo
# puerto lógico (5009 por defecto).
EXPOSE 5009 5009/udp

# Volumen para datos persistentes (bans, cuentas, historial, DB SQLite).
VOLUME ["/app/data"]

ENTRYPOINT ["/app/astra"]
CMD ["--port", "5009", "--data-dir", "/app/data"]
