# =============================================================================
# Astra Chat Server - Multi-stage Dockerfile
# =============================================================================
# Build:  docker build -t astra:local .
# Multi-arch: docker buildx build --platform linux/amd64,linux/arm64 -t astra .
# Run:    docker run -p 5009:5009 -p 5009:5009/udp -v astra-data:/app/data astra
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

# Se crea un directorio vacío en builder para pasarlo al runtime
RUN mkdir -p /app/data

# -------- Stage 2: runtime --------
# distroless cc-debian12: chico, seguro, sin shell ni package manager.
FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app

# Binario + directorio data vacío
# Se copia el dir vacío desde el builder como `nonroot` (UID 65532) para que
# el usuario pueda escribir la DB SQLite. Al montar un volumen vacío, Docker hereda
# estos permisos.
COPY --from=builder /app/target/release/astra /app/astra
COPY --from=builder --chown=65532:65532 /app/data /app/data

# Astra multiplexa TCP (Ares), WebSocket (web/admin), Link y UDP en un solo
# puerto lógico (5009 por defecto).
EXPOSE 5009 5009/udp

# Volumen para datos persistentes (bans, cuentas, historial, DB SQLite).
VOLUME ["/app/data"]

ENTRYPOINT ["/app/astra"]
CMD ["--port", "5009", "--data-dir", "/app/data"]
