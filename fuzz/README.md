# Astra Fuzz Targets

Fuzzing con [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) sobre las
superficies de parsing de bytes del protocolo Ares.

## Requisitos

```bash
rustup install nightly
cargo install cargo-fuzz
```

## Targets disponibles

| Target | Cosa fuzzea |
|---|---|
| `fuzz_reader` | `PacketReader` con bytes random (no debe panicar) |
| `fuzz_writer` | Roundtrip `PacketReader` → `PacketWriter` → `PacketReader` |
| `fuzz_login` | `parse_login` con paquetes de login random |

## Uso

```bash
# Correr un target hasta que encuentre un crash (o Ctrl-C)
cargo +nightly fuzz run fuzz_reader

# Correr por N segundos
cargo +nightly fuzz run fuzz_reader -- -max_total_time=60

# Reproducir un crash encontrado (los crashes se guardan en fuzz/artifacts/)
cargo +nightly fuzz run fuzz_reader fuzz/artifacts/fuzz_reader/crash-<hash>
```

Los targets están **excluidos del workspace** (`Cargo.toml:exclude = ["fuzz"]`)
para que `cargo build` y `cargo test` normales no intenten compilarlos.
Requieren nightly porque `cargo-fuzz` usa sanitizadores (ASan, MSan).
