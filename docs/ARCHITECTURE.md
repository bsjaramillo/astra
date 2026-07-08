# Arquitectura

Visión de alto nivel del workspace Astra.

## Diagrama de crates

```
                    ┌─────────────────┐
                    │      astra      │  Binario CLI
                    │   (main.rs)     │
                    └────────┬────────┘
                             │
       ┌────────────┬────────┼────────┬─────────────┬──────────────┐
       │            │        │        │             │              │
       ▼            ▼        ▼        ▼             ▼              ▼
   ┌───────┐   ┌──────┐ ┌──────┐ ┌──────┐   ┌──────────┐    ┌──────────┐
   │ proto │   │ web  │ │ link │ │ udp  │   │ commands │    │ scripting│
   │ -ares │   │  WS  │ │Hub/Lf│ │room  │   │  slash   │    │   JS     │
   └───┬───┘   └──┬───┘ └──┬───┘ │search│   └────┬─────┘    └────┬─────┘
       │          │        │     └──────┘        │               │
       └──────────┴────────┼─────────────────────┘               │
                           │                                     │
                    ┌──────▼──────┐                       ┌──────▼──────┐
                    │ server-core │ ◄─────────────────────│  captcha    │
                    │  app+db+    │                       │  (palabras  │
                    │  userpool   │                       │   + PNG)    │
                    │  +security  │                       └─────────────┘
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │  iconnect   │  Traits públicos (IUser, IRoom...)
                    │             │  (referenciado pero no implementado)
                    └─────────────┘
```

## Flujo de una conexión TCP

```
1. accept() → TcpStream
2. security.check_new_connection(ip)
   ├─ reject → ServerError + close
   └─ OK
3. split stream → reader_task + writer_task (mpsc)
4. handshake timeout (15s) — leer primer paquete
5. parse_login (25+ campos)
6. login_validator.validate()
   ├─ reject → ServerError + close
   └─ OK
7. bans.is_banned(guid, ip)?
   ├─ banned → ServerError + close
   └─ OK
8. user_history.is_join_flooding(ip)?
   ├─ flood → ServerError + close
   └─ OK
9. Crear AresUser + UserPool.add()
10. Enviar LoginAck + MyFeatures
11. send_initial_state (topic, userlist, opchange)
12. Loop principal:
    - leer paquetes
    - dispatch por opcode (Public → broadcast, PM → target, etc.)
    - slash commands → commands crate (built-ins) o scripting (JS)
```

## Flujo de Link Hub ↔ Leaf

```
Leaf                          Hub
 │                            │
 │──── LeafLogin ────────────▶│  (name + sha1 + port)
 │                            │
 │◀─── HubAck ────────────────│  (status = 1)
 │                            │
 │◀─ UserlistItem (×N) ───────│  (uno por user local)
 │◀─ LeafUserlistEnd ─────────│
 │                            │
 │◀═══ Loop: HubPong cada 30s │
 │═════ Loop: LeafPing cada 30s
 │                            │
 │◀─── LeafJoin ──────────────│  (otro server: nuevo user)
 │──── Part ─────────────────▶│
 │◀── PublicText ─────────────│
 │──── EmoteText ────────────▶│
 │ ...                        │
```

Todos los eventos se serializan como `LinkEvent` en `AppContext::link_events`
(broadcast channel). Hub y Leaf filtran por `origin` para evitar loops.

## Persistencia

Todo el estado persistente va a SQLite (`data/astra.db`):

- `bans` — bans por GUID/IP/ident
- `accounts` — cuentas registradas (SHA-1 hash)
- `user_history` — historial de joins por IP/GUID (join-flood + cleanup)
- `nodes` — nodos UDP descubiertos
- `rooms` — rooms UDP cacheadas (seed inicial)

## Concurrencia

- **tokio multi-thread runtime** (default)
- Por conexión TCP: 2 tasks (reader + writer)
- Por conexión UDP: 1 task (listener) + 1 task (prober cada 15s)
- Por Link: 1 task (server) o 1 task (client con reconnect)
- Estado compartido via `Arc<AppContext>` (con `parking_lot::Mutex`/`RwLock` para interior mutability)

## Logging

`tracing` + `tracing-subscriber`. Niveles: `error`, `warn`, `info`, `debug`, `trace`.

```bash
RUST_LOG=debug astra --port 5009
RUST_LOG=astra_link=trace,astra::tcp_handler=debug astra
```
