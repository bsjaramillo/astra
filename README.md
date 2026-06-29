# Astra

> Servidor de chat compatible con **Ares Galaxy**, escrito en **Rust**.

Astra es un servidor de chat multiplataforma que implementa el protocolo binario de Ares Galaxy. Es una reescritura moderna de [sb0t](https://github.com/.../sb0t) (C#/.NET Framework), enfocada en:

- **Compatibilidad de protocolo**: cualquier cliente Ares oficial puede conectarse sin cambios.
- **Multiplataforma real**: Linux, Windows, macOS, FreeBSD, Raspberry Pi (ARM).
- **Seguridad por construcción**: memory safety garantizada por el compilador.
- **Rendimiento**: un solo binario estático con miles de conexiones concurrentes.
- **Despliegue simple**: cero dependencias en runtime.

## Estado del proyecto

🚧 **En desarrollo activo** — ver [ROADMAP.md](./ROADMAP.md) para el plan completo.

**Fase actual**: setup inicial completo, primer binario funcional (escucha TCP/UDP y responde logins básicos).

## Compilar y ejecutar

```bash
# Requisitos: Rust 1.75+
cargo --version

# Compilar
cargo build --release

# Ejecutar con configuración por defecto (puerto 5009)
./target/release/astra

# Ejecutar con config custom
./target/release/astra --port 6000 --config mi-sala.toml

# Modo verbose
./target/release/astra --verbose
```

## Tests

```bash
cargo test              # todos los tests
cargo test -p proto-ares  # solo los del protocolo
```

## Estructura

```
astra/
├── crates/
│   ├── proto-ares/    # Protocolo binario (PacketReader, PacketWriter, TcpMsg, UdpMsg)
│   ├── iconnect/      # Traits públicos (IUser, IRoom, IChannel, IStats, ...)
│   ├── server-core/   # UserPool, Room, Stats, Settings, BanSystem, Captcha
│   ├── astra-udp/     # Listener UDP (room search)
│   ├── astra-captcha/ # Generación de captchas
│   ├── astra-commands/# 50+ comandos slash
│   ├── astra-scripting/ # Motor JS (boa_engine)
│   ├── astra-web/     # WebSockets + panel admin
│   └── astra/         # Binario principal (CLI)
├── docs/
├── Cargo.toml         # Workspace
└── ROADMAP.md
```

## Diferencias con el sb0t original

| Aspecto | sb0t (C#) | Astra (Rust) |
|---|---|---|
| Plataforma | Solo Windows | Linux, Windows, macOS, ARM |
| Runtime | .NET Framework 4.7.2 | Ninguno (binario estático) |
| Concurrencia | Thread + Thread.Sleep(25ms) | tokio async |
| Memoria | GC + posibles fugas | Memory-safe por construcción |
| Motor JS | Jurassic (vendoreado) | boa_engine (puro Rust) |
| GUI | WPF + WinForms | Web panel (en desarrollo) |
| Build | msbuild + Visual Studio | cargo |

## Licencia

AGPL-3.0-or-later (igual que el sb0t original).
