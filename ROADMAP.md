# Astra — Roadmap

> Servidor de chat compatible con Ares Galaxy, escrito en Rust.

## Estado actual

**Fase 0 ✅ Setup del workspace**
- [x] Workspace Cargo con 9 crates definidos
- [x] `.gitignore` configurado
- [x] Manifiestos de cada crate
- [x] `proto-ares` completo (70+ mensajes TCP + 9 mensajes UDP)
- [x] `iconnect` con todos los traits del sb0t original
- [x] `server-core` con módulos base (app, time, user_pool, room, stats, settings, bans, captcha, avatars, idle)
- [x] Binario `astra` con CLI (clap) y handlers TCP/UDP
- [x] Hola mundo: el server escucha conexiones TCP y UDP, loguea paquetes, decodifica login básico

**Fase 1 ✅ Protocolo Ares + setup mínimo**
- [x] `PacketReader` con lectura de u8/u16/u32/i32/string/guid/bool + tests (13 tests passing)
- [x] `PacketWriter` con las inversas + tests
- [x] `Guid` con MD5 (compatible con Ares)
- [x] TCP listener con tokio
- [x] UDP listener con tokio
- [x] ACK básico de login (validado end-to-end con cliente Python)
- [x] FastPing echo (validado)
- [x] `cargo check --workspace` ✅
- [x] `cargo test --workspace` ✅ (13/13)
- [x] `cargo build --bin astra` ✅

**Fase 2 ✅ Login completo + UserPool + Persistencia**
- [x] Parser completo de `MSG_CHAT_CLIENT_LOGIN` con 25+ campos
- [x] Soporte de voice chat capabilities (vc, opus, html flags)
- [x] Detección automática Ares vs cbot vs custom
- [x] Encriptación (crypto byte 250)
- [x] Truncado de region a 30 chars
- [x] Creación de `AresUser` con todos los campos
- [x] Registro en `UserPool` con ID único auto-asignado
- [x] Cleanup automático al desconectar
- [x] LoginAck con nick + room name + version
- [x] MyFeatures con flags correctos (VC, opus, sharing, html, etc.)
- [x] Stats tracking (peak/total users)
- [x] Tests del parser (7 tests)
- [x] Validado con 3 clientes en paralelo (Ares, Ares, cbot) — IDs 1, 2, 3 únicos
- [x] **Persistencia SQLite** (db.rs con bans, accounts, user_history)
- [x] **BanSystem** persistido (carga al arranque, cache en memoria)
- [x] **UserHistory** con detección de join-flood (15s, compatible con sb0t)
- [x] **AccountManager** con SHA-1 (compatible con sb0t original)
- [x] Verificación de bans en login → ServerError "You are banned from this room"
- [x] Verificación de join-flood en login → ServerError "Joining too quickly"
- [x] Task periódico de cleanup (prune history > 30 días)
- [x] Tests de DB: 19 tests nuevos (db.rs, bans.rs, user_history.rs, accounts.rs)
- [x] Validado E2E: 3 clientes, flood detection, persistencia entre reinicios

**Fase 2.5 ✅ Defensa en capas anti-DDoS (5 capas)**
- [x] **Capa 1: ConnectionFloodTracker** — Rate limit per-IP de nuevas conexiones (sliding window 60s, default 10/min, auto-ban después de 3 violaciones)
- [x] **Capa 2: ConcurrentConnLimiter** — Máx conexiones TCP simultáneas por IP (default 5)
- [x] **Capa 3: HandshakeTimeout** — Timeout 15s para recibir el primer login (anti-slowloris)
- [x] **Capa 4: LoginValidator** — Anti-fake/anti-spam:
  - Nombre: longitud 1-30, sin chars de control, sin zero-width
  - Versión: requerida, no vacía
  - Spam bots: 6.6.6.6, 7.8.7.8, 6969 files (del sb0t original)
  - Perfil sospechoso: country=0 + files>0 + age=0
  - File count absurdo (>60000)
- [x] **Capa 5: FailedLoginTracker** — Auto-ban después de 5 logins fallidos en 1h
- [x] **SecurityManager** (fachada) + scopeguard para release automático
- [x] **SecurityConfig** en `astra.toml` (todos los valores ajustables)
- [x] Tests: 25 nuevos tests de seguridad (4 capas + fachada)
- [x] E2E: 7/7 ataques mitigados:
  - Spam bot 6.6.6.6 → rechazado
  - Spam bot 6969 → rechazado
  - Login normal → OK
  - Control char en name → rechazado
  - Slowloris → timeout 15s
  - 15 conexiones rápidas → 10/15 rechazadas + auto-ban
  - Post-ban → IP bloqueada

**Fase 3 ✅ Mensajes básicos del chat (protocolo Ares)**
- [x] **Refactor arquitectónico**: cada cliente tiene un `mpsc::UnboundedSender<Bytes>` para envío async
- [x] **Split TCP handler**: `reader_task` (lee del socket) + `writer_task` (drena mpsc y escribe)
- [x] **Módulo `outbound.rs`**: constructores de todos los paquetes server→client (12 funciones)
- [x] **MSG_CHAT_SERVER_JOIN (20)**: broadcast al resto cuando alguien entra
- [x] **MSG_CHAT_SERVER_PART (22)**: broadcast al resto cuando alguien sale
- [x] **MSG_CHAT_SERVER_PUBLIC (10)**: broadcast de mensaje público
- [x] **MSG_CHAT_SERVER_EMOTE (11)**: broadcast de emote
- [x] **MSG_CHAT_SERVER_PVT (25)**: mensaje privado
- [x] **MSG_CHAT_SERVER_PERSONAL_MESSAGE (13)**: cambio de PM
- [x] **MSG_CHAT_SERVER_CHANNEL_USER_LIST (30)**: lista de usuarios al login
- [x] **MSG_CHAT_SERVER_CHANNEL_USER_LIST_END (35)**: fin de la lista
- [x] **MSG_CHAT_SERVER_TOPIC_FIRST (32)**: topic al login
- [x] **MSG_CHAT_SERVER_OPCHANGE (75)**: nivel de op
- [x] **MSG_CHAT_SERVER_NOSUCH (44)**: user no encontrado (PM)
- [x] **Estado inicial al login**: LoginAck + MyFeatures + TopicFirst + Bot fantasma + Userlist + UserListEnd + OpChange
- [x] Tests: 9 nuevos tests de `outbound.rs` (formato de cada paquete)
- [x] E2E: 10/10 tests de chat entre 2 clientes:
  - Login ambos
  - JOIN broadcast (Alice↔Bob)
  - Public broadcast
  - Emote broadcast
  - PM
  - Topic al login
  - Cleanup al desconectar

**Fase 3.5 ✅ UDP Room Search (sin Supabase)**
- [x] **Schema SQLite**: tablas `nodes` y `rooms` con índices
- [x] **Seed local**: `data/seed_rooms.json` (subset de 20 rooms de `chatrooms.mywire.org/rooms.json`)
- [x] **`UdpNodeManager`** con cache en memoria + persistencia
- [x] **Protocolo UDP completo** (9 mensajes):
  - `SENDINFO` (2): "¿estás vivo?"
  - `ACKINFO` (3): info del server + lista de nodos
  - `ADDIPS` (11) / `ACKIPS` (12): compartir listas de nodos
  - `SENDNODES` (21) / `ACKNODES` (22): nodos Ares 2.x
  - `WANTCHECKFIREWALL` (31) / `READYTOCHECKFIREWALL` (32) / `PROCEEDCHECKFIREWALL` (33) / `CHECKFIREWALLBUSY` (34): stub
- [x] **`UdpNode` / `UdpChannelItem` / `UdpStats`** en `types.rs`
- [x] **Encode/decode** de los 9 mensajes en `protocol.rs`
- [x] **Listener async** que recibe paquetes y dispatcha (`SENDINFO` → `ACKINFO`, etc.)
- [x] **Prober async** que envía `SENDINFO` cada 15s al nodo más viejo
- [x] **Expiración** de nodos muertos (try > 4 y last_connect > 1h)
- [x] **CLI flag** `--data-dir` para tests con DBs separadas
- [x] Tests: 14 nuevos tests de UDP (types, protocol, manager, seed)
- [x] E2E 2 servers locales: se descubren mutuamente via UDP (ack=1, rooms intercambiadas)
- [x] E2E con seed real: recibe ACKINFO del server `18.118.100.161:3724` con 6 nodos nuevos
- [x] **Total: 116 tests passing**

**Nota**: el UDP room search estaba contemplado en la **Fase 4** del ROADMAP original. La etiqueta "3.5" que se le puso es engañosa — en realidad completa la Fase 4 (con el cambio de Supabase → BD local que se hizo durante la fase de planeación).

**Fase 7 ✅ WebSockets para clientes ib0t (HTML5)**
- [x] **`astra-web` crate**: WebSocket server en puerto 5010
- [x] **Handshake RFC 6455** (HTTP/1.1 → 101 Switching Protocols → WebSocket)
- [x] **Frame reader/writer** con soporte para client-mask (per RFC)
- [x] **Protocolo texto WS** (formato `IDENT:args`):
  - `LOGIN`, `PUBLIC`, `EMOTE`, `PM`, `PING`, `COMMAND`
  - Outgoing: `ACK`, `MYFEATURES`, `TOPIC`, `JOIN`, `PART`, `USERLIST`, `USERLIST_END`, `PUBLIC`, `EMOTE`, `PM`, `OPCHANGE`, `NOSUCH`
- [x] **Args de longitud variable** (`4,32,5,5:arg1arg2...`)
- [x] **Estado inicial al login**: ACK + MyFeatures + Topic + Bot fantasma + Userlist + UserListEnd + OpChange
- [x] **Bridge TCP ↔ WS**: usuarios WS comparten el `UserPool` y reciben broadcasts
- [x] **`ws_text_sender`** en `AresUser`: canal de texto pre-formateado para WS
- [x] **`translate_broadcast`**: convierte paquetes binarios TCP a texto WS (Public, Emote, PM, Join, Part, UserList)
- [x] **HTML panel** (`panel.rs`): página de prueba con chat JS
- [x] **CLI flag** `--no-web` para desactivar WS
- [x] Tests: 12 nuevos tests de `protocol` + 1 doctest
- [x] E2E 1 cliente WS: 7/7 (handshake, login, estado inicial, PUBLIC broadcast)

**Fase 5 ✅ Scripting con boa_engine (Rust-native JS engine)**
- [x] **`astra-scripting` crate**: motor de scripting JS para plugins de sala
- [x] **API expuesta a JS** (boilerplate del sb0t original):
  - `print(msg)`, `log(msg)` — log a tracing
  - `userCount()` — número de usuarios conectados (real)
  - `sendPublic(from, text)` — broadcast público (real)
  - `sendPM(from, to, text)` — PM (real)
- [x] **Eventos** que los scripts pueden manejar:
  - `onLoad()` — al cargar
  - `onUserJoin(name, ip)`, `onUserPart(name)`
  - `onPublic(from, text)`, `onEmote(from, text)`
  - `onPrivate(from, to, text)`
  - `onCommand(from, command, args)`
- [x] **ScriptManager**: load, unload, reload, load_all
- [x] **ScriptHandle** (Send + Clone) para dispatchear eventos desde otras tasks
- [x] **Registry global** de `Arc<AppContext>` por Context (solución al problema
      del `Context` no-Send de boa_engine 0.20)
- [x] **Args de longitud variable** en mensajes (formato `4,32,5,5:arg1arg2...`)
- [x] **Script de ejemplo** (`data/scripts/greet.js`): bienvenida a usuarios + comando /hola
- [x] Integración en `main.rs`: carga automática de scripts en `data/scripts/`
- [x] Tests: 16 nuevos (api + manager)
- [x] E2E: script greet.js se carga al iniciar, ejecuta `onLoad()`, imprime "greet.js cargado!"

**Fase 6 ✅ Comandos slash (dispatcher)**
- [x] **`astra-commands` crate**: dispatcher de comandos slash
- [x] **`parse_command(text)`**: parsea `/hola mundo` → `("hola", "mundo")`
- [x] **`dispatch(ctx, scripting, from, cmd, args)`**: despacha el evento a los scripts
- [x] **`try_dispatch(...)`**: helper que parsea + dispatcha en un solo paso
- [x] Integración en `tcp_handler`: detecta `/` y dispatcha como comando
- [x] Tests: 6 nuevos de `astra-commands`
- [ ] Migrar los ~50 comandos nativos de sb0t (ban, motd, greets, hashlink, etc.)
- [ ] Registrar handlers built-in para `/help`, `/users`, etc.

**Fase 8 ✅ Link Hub/Leaf (multi-servidor)**
- [x] **`astra-link` crate**: protocolo link idéntico al sb0t
- [x] **Protocolo completo** con opcodes exactos (38 opcodes del enum `LinkMsg`):
  - `Error(0)`, `LeafLogin(1)`, `HubAck(3)`, `HubLeafConnected(5)`, `HubLeafDisconnected(6)`
  - `LeafPing(7)`, `HubPong(8)`, `UserlistItem(10)`, `Avatar(11)`, `PersonalMessage(12)`
  - `LeafUserlistEnd(14)`, `LeafJoin(15)`, `Part(16)`, `UserUpdated(18)`, `CustomName(19)`
  - `PublicText(20)`, `EmoteText(21)`, `PrivateText(25)`, `PrivateIgnored(27)`, `PublicToUser(28)`
  - `EmoteToUser(29)`, `CustomDataTo(30)`, `CustomDataAll(31)`, `Nudge(32)`, `ScribbleUser(33)`
  - `ScribbleLeaf(34)`, `NickChanged(40)`, `VroomChanged(41)`, `IUser(42)`, `Admin(43)`
  - `IUserBin(44)`, `NoAdmin(45)`, `Browse(50)`, `BrowseData(51)`, `PrintAll(60)`
  - `PrintVroom(61)`, `PrintLevel(62)`
- [x] **Formato de strings**: null-terminated, idéntico al sb0t
- [x] **MSG_LINK_PROTO** (0xFB) wrapper TCP con su propio length prefix
- [x] **LinkPacketBuilder** y **LinkPacketReader** con todos los métodos
- [x] **LinkServer**: stub que acepta conexiones y maneja handshake (login → ack → userlist)
- [x] **LinkClient**: se conecta, hace login, lee userlist, manda keep-alive (E2E validado)
- [x] **LinkServer**: acepta conexiones, responde con userlist local, envía HubPong keep-alive
- [x] **CLI flags**: `--link-server` y `--link-client <addr>`
- [x] Tests: 3 tests (protocolo + opcodes exactos)
- [x] **E2E validado**: leaf se conecta al hub, hace handshake completo (login → ACK → userlist → end)
- [ ] **TODO**: integrar `UserPool` para que cuando un user se une en un
  server, el otro lo vea via `LeafJoin`/`HubJoin`
- [ ] **TODO**: dispatch de mensajes públicos cross-server (PublicText/EmoteText)
- [ ] **TODO**: dispatch de PMs cross-server (PrivateText/PrivateIgnored)
- [ ] **TODO**: encriptación AES (el original soporta mensajes encriptados)

**Fase 9 ✅ Release y cross-compile**
- [x] **`Dockerfile`** multi-stage (rust:1.83-alpine → gcr.io/distroless/cc-debian12)
- [x] **`.dockerignore`** optimizado
- [x] **`docker-compose.yml`** para testing local
- [x] **`.github/workflows/release.yml`**: automatiza build al pushear tag `v*`:
  - [x] Docker images multi-arch (linux/amd64, linux/arm64) → `ghcr.io/$OWNER/astra:$VERSION`
  - [x] Binarios estáticos:
    - [x] `x86_64-unknown-linux-musl` (musl static)
    - [x] `aarch64-unknown-linux-musl` (musl static)
    - [x] `x86_64-pc-windows-gnu` (Windows)
    - [x] `x8664-apple-darwin` y `aarch64-apple-darwin` (macOS)
  - [x] SHA256 checksums para todos los binarios
  - [x] GitHub Release con notas auto-generadas
- [ ] Fuzzing del protocolo binario con `cargo-fuzz`
- [ ] Documentación de API
- [ ] Benchmarking

## Distribución e instalación

### Opción 1: Docker (recomendado para producción)

```bash
docker pull ghcr.io/<owner>/astra:v0.1.0

docker run -d \
  --name astra \
  -p 5009:5009 \
  -p 5010:5010 \
  -p 5011:5011 \
  -p 5012:5012/udp \
  -v $(pwd)/astra-data:/app/data \
  ghcr.io/<owner>/astra:v0.1.0
```

O usando `docker compose`:

```bash
curl -O https://raw.githubusercontent.com/<owner>/astra/v0.1.0/docker-compose.yml
docker compose up -d
```

### Opción 2: Binario estático (testing local)

```bash
wget https://github.com/<owner>/astra/releases/download/v0.1.0/astra-linux-x86_64
chmod +x astra-linux-x86_64
./astra-linux-x86_64 --port 5009 --data-dir ./data
```

Plataformas soportadas: Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64, Windows x86_64.

## Pendientes para futuro (TODOs)
- Firewall check completo (Opción B con TCP probe real)
- Comando CLI `astra seed-refresh` para actualizar la lista
- Soporte de frames WebSocket fragmentados
- HTML panel servido por el WS server (actualmente solo el HTML estático en el código)
- `ScriptHandle::dispatch` es placeholder: el dispatch real requiere un
  mecanismo de eventos entre threads (el Context de boa_engine no es Send).
  Solución: usar un LocalSet o un thread dedicado con un canal. Por ahora,
  los eventos se loguean pero no se ejecutan en los scripts.
- Comandos nativos built-in (`/help`, `/users`, `/ban`, etc.) — actualmente
  solo se delegan a scripts JS
- Agregar `astraVersion` y otras constantes (limitado por la API de boa_engine 0.20)

## Convenciones del proyecto

- **Versión de Rust**: MSRV 1.75
- **Edición**: 2021
- **Async runtime**: `tokio` (full features)
- **Logging**: `tracing` + `tracing-subscriber`
- **Errores**: `thiserror` para libs, `anyhow` para binario
- **Tests**: `#[cfg(test)]` en cada módulo
- **Formato**: `cargo fmt` antes de commit
- **Lints**: `#![warn(missing_docs)]` en cada `lib.rs`
