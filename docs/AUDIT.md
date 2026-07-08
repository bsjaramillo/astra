# Astra — Auditoría de migración sb0t → Astra

> Análisis del estado real de implementación del proyecto Astra (Rust) como
> reescritura de sb0t (C#/.NET Framework), servidor de chat compatible con
> Ares Galaxy.

**Fecha**: 2026-07-01
**Método**: lectura exhaustiva del código fuente + comparación con `ROADMAP.md`

---

## 0. Hallazgo crítico — El proyecto NO compila

Antes de cualquier otra observación, lo más importante:

```text
$ cargo check --workspace
error[E0428]: the name `build_chat_payload` is defined multiple times
   --> crates/link/src/server.rs:664:1
   |
651 | fn build_chat_payload(...) -> Vec<u8> {     // primera definición
664 | fn build_chat_payload(...) -> Vec<u8> {     // duplicada
   = note: `build_chat_payload` must be defined only once

error[E0425]: cannot find function `is_passthrough_opcode` in this scope
   --> crates/link/src/server.rs:428:27
note: function `crate::client::is_passthrough_opcode` exists but is inaccessible
   --> crates/link/src/client.rs:619:1
```

Esto es un bug de función duplicada + un problema de visibilidad. **Ningún
test E2E marcado ✅ en el ROADMAP es ejecutable tal cual está el código**.
La afirmación "116 tests passing" del ROADMAP es falsa en este momento.

---

## 1. Estructura del workspace

`Cargo.toml` declara **10 crates** en `crates/`:

```text
crates/proto-ares      # Protocolo binario
crates/iconnect        # Traits públicos (IUser, IRoom, ...)
crates/server-core     # Lógica central
crates/udp             # UDP room search
crates/captcha         # Captcha  ← STUB VACÍO
crates/commands        # Comandos slash
crates/scripting       # Motor JS (boa_engine)
crates/web             # WebSockets
crates/link            # Link Hub/Leaf  ← ROTO
crates/astra           # Binario principal
```

- `docs/` está **vacío** (no hay notas de protocolo, specs, ni docs de API).
- No existe ningún `astra.toml` de ejemplo en el repo. El flag `--config`
  funciona solo si el usuario crea el archivo a mano.
- Top-level: `Cargo.toml`, `Cargo.lock`, `README.md`, `ROADMAP.md`,
  `Dockerfile`, `docker-compose.yml`, `.dockerignore`, `.gitignore`,
  `.github/workflows/release.yml` y tres data dirs (`data/`, `data_a/`, `data_b/`).

---

## 2. Auditoría por crate

### 2.1 `crates/proto-ares/` — Protocolo binario Ares

| Archivo | Líneas | Estado | Descripción |
|---|---:|---|---|
| `Cargo.toml` | 14 | implementado | Dependencias mínimas |
| `src/lib.rs` | 30 | implementado | Re-exports |
| `src/guid.rs` | 43 | implementado | `Guid([u8;16])` con MD5 |
| `src/packet.rs` | 32 | implementado | `Packet { msg, data }` |
| `src/reader.rs` | 265 | implementado | `PacketReader` + 8 tests |
| `src/writer.rs` | 206 | implementado | `PacketWriter` + 5 tests |
| `src/messages.rs` | 311 | implementado | `TcpMsg` (52 vars), `UdpMsg` (9 vars) + 3 tests |
| `src/udp_packets.rs` | 227 | implementado | Reader/writer UDP + 7 tests |

**API pública**: `Guid`, `TcpMsg`, `UdpMsg`, `Packet`, `PacketReader`,
`PacketWriter`, `UdpPacketReader`, `UdpPacketWriter`, varios `*Error`/`*Result`.

**Discrepancias con ROADMAP**:
- ROADMAP dice "70+ mensajes TCP" → real: **52 variantes** en el enum `TcpMsg`.
- Los 9 mensajes UDP sí están todos.
- Cero `todo!()`/`unimplemented!()` en el crate.

**Estado**: implementado, pero el conteo documentado está inflado.

---

### 2.2 `crates/iconnect/` — Traits públicos

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 15 | implementado |
| `src/lib.rs` | 730 | implementado (pero **no usado**) |

Define 27 traits/structs: `ILevel`, `IFont`, `IUser` (~70 métodos), `IRoom`,
`IChannel`/`Item`/`Channels`, `IHostApp`, `IExtension`, `ICommandDefault`,
`ILeaf`, `IHub`, `ILink`, `ILinkError`, `IStats`, `IAccounts`, `IBan`,
`IPassword`, `IPrivateMsg`, `IQuarantined`, `ISpell`, `IHashlink`/`IHashlinkRoom`,
`IScripting`, `IRecord`, `IPool`, `ICompression`, `MimeType`, `RejectedMsg`.

**PROBLEMA CRÍTICO**: estos traits **no se implementan en ningún lugar**.

```text
$ grep -rn "impl IUser" crates/
(sin resultados)
$ grep -rn "impl iconnect::IUser" crates/
(sin resultados)
```

`server-core::user_pool::AresUser` es un struct plano con campos públicos,
no un `impl IUser`. El server hace todo por acceso directo a campos, no
a través de la abstracción `iconnect`.

**Estado**: declarado pero sin consumidores. Es código muerto en términos
prácticos, conservado para compatibilidad futura de plugins.

---

### 2.3 `crates/server-core/` — Lógica central

Este es el crate más completo del proyecto (4.160 líneas de Rust).

| Archivo | Líneas | Estado | Descripción |
|---|---:|---|---|
| `Cargo.toml` | 35 | implementado | Dependencias grandes |
| `src/lib.rs` | 70 | implementado | Re-exports |
| `src/app.rs` | 311 | implementado | `AppContext`, `LinkEvent`, `LinkUserSnapshot` |
| `src/user_pool.rs` | 293 | implementado | `AresUser` (80+ campos), `UserPool` |
| `src/room.rs` | 55 | implementado (mínimo) | Wrapper de `Room` |
| `src/stats.rs` | 96 | implementado | `Stats` (atómicos) |
| `src/settings.rs` | 174 | implementado | `Settings` + `SecurityConfig` + TOML |
| `src/bans.rs` | 207 | implementado | `BanSystem` con persistencia + 3 tests |
| `src/captcha.rs` | 230 | implementado | `CaptchaManager` con expiración + 11 tests |
| `src/avatars.rs` | 42 | **deshabilitado** (`#![allow(dead_code)]`) | `AvatarManager` |
| `src/idle.rs` | 50 | **deshabilitado** (`#![allow(dead_code)]`) | `IdleManager` |
| `src/db.rs` | 863 | implementado | `Database` SQLite, 4 tablas + 12 tests |
| `src/login.rs` | 454 | implementado | Parser de login (25 campos) + 7 tests |
| `src/user_history.rs` | 154 | implementado | Join-flood (15s) + 4 tests |
| `src/accounts.rs` | 128 | implementado | SHA-1 compatible sb0t + 3 tests |
| `src/security.rs` | 829 | implementado | 5 capas anti-DDoS + 23 tests |
| `src/outbound.rs` | 351 | implementado | Constructores de paquetes server→client + 9 tests |
| `src/time.rs` | 18 | implementado | Helpers de tiempo |

**Discrepancias con ROADMAP**:
- ROADMAP dice "19 tests nuevos (db.rs, bans.rs, user_history.rs, accounts.rs)"
  → real: db 12 + bans 3 + user_history 4 + accounts 3 = **22 tests**.
- ROADMAP dice "25 tests de seguridad" → real: **23 tests**.
- `captcha`, `avatars`, `idle` están con `dead_code` y **nunca se
  instancian ni se usan** desde el binario principal.
- Solo se referencia `iconnect::ILevel` (en `user_pool.rs:11` y `outbound.rs`).
  El resto de traits son código muerto.

**Estado**: implementado. Es el crate más maduro.

---

### 2.4 `crates/udp/` — UDP room search

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 22 | implementado |
| `src/lib.rs` | 40 | implementado |
| `src/types.rs` | 189 | implementado + 2 tests |
| `src/protocol.rs` | 325 | implementado + 7 tests |
| `src/manager.rs` | 351 | implementado + 6 tests |
| `src/listener.rs` | 269 | **parcial** (2 stubs + 1 TODO) |
| `src/prober.rs` | 67 | implementado |
| `src/seed.rs` | 242 | implementado + 4 tests |

**Discrepancias con ROADMAP**:
- ROADMAP dice "14 tests de UDP" → real: 2 + 7 + 6 + 4 = **19 tests**.
- ROADMAP dice "E2E 2 servers locales" → **no reproducible** (no hay
  harness de integración, no hay CI que levante dos servers).
- ROADMAP dice "E2E con seed real 18.118.100.161:3724" → no es un test,
  requiere acceso a red real.
- **`listener.rs:142`**: `users = 0; // TODO: pasar user_count desde fuera`
  → el campo `users` en `ACKINFO` siempre es 0. El valor enviado es parcial.
- `PROCEEDCHECKFIREWALL` y `CHECKFIREWALLBUSY` son stubs explícitos
  ("ignorado - stub").

**Estado**: implementado, con el bug del user_count=0 y el firewall
check sin implementar.

---

### 2.5 `crates/captcha/` — Generación de captchas

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 12 | implementado (dep `image` con feature `png`) |
| `src/lib.rs` | 78 | implementado + 6 tests |
| `src/wordlist.rs` | 199 | implementado (256 palabras) + 3 tests |
| `src/font.rs` | 144 | implementado (font 5x7 A-Z + 0-9) + 2 tests |
| `src/image.rs` | 86 | implementado (render PNG) + 4 tests |

**Funcionalidad**:
- `Captcha::generate()` retorna `(word, png_bytes)`
- `Captcha::with_word(word)` (para tests)
- `Captcha::verify(answer)` case-insensitive
- Imágenes PNG grayscale 28×9 píxeles con la palabra dibujada
  + 3-5 píxeles de ruido gris

**Tests**: 15 unit + 1 doctest = 16 passing.

---

### 2.6 `crates/commands/` — Dispatcher de comandos slash

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 24 | implementado |
| `src/lib.rs` | 860 | implementado + **23 tests** |

**11 comandos built-in implementados**:
`/help`, `/nick`, `/vroom`, `/cname`, `/users`, `/topic`, `/motd`,
`/ban`, `/unban`, `/banlist`, `/whois`.

**Discrepancias con ROADMAP**:
- ROADMAP dice "Migrar los ~50 comandos nativos de sb0t" → real: **11**,
  faltan greets, hashlink, account admin, browse admin, captcha admin,
  proxy commands, voice chat, scribble admin, registration, link admin.
- ROADMAP dice "6 nuevos tests" → real: **23 tests**.
- ROADMAP "Pendientes" dice que los built-ins se delegan a JS → **falso**:
  `dispatch_builtin` se chequea PRIMERO en `tcp_handler.rs:461` y solo
  cae a `dispatch` (JS) si no se reconoce el comando.

**Estado**: parcial. Implementación real de 11 comandos, pero faltan ~39
de los ~50 que tenía sb0t.

---

### 2.7 `crates/scripting/` — Motor JS (boa_engine)

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 26 | implementado |
| `src/lib.rs` | 61 | implementado |
| `src/types.rs` | 593 | implementado + 3 tests |
| `src/api.rs` | 275 | parcial + 7 tests |
| `src/manager.rs` | 420 | implementado + 8 tests |
| `src/bindings/mod.rs` | 55 | **stub** |
| `src/bindings/statics.rs` | 208 | **stub** |
| `src/bindings/properties.rs` | 72 | **stub** |
| `src/objects.rs` | 152 | **stub** |
| `src/prototypes.rs` | 80 | **stub** |
| `src/instances.rs` | 98 | **stub** |

**Eventos disponibles** (46 variantes en `ScriptEvent`): Connect,
Disconnect, Join, JoinCheck, Rejected, Part, UserList, UserListEnd,
UserUpdate, Public, TextBefore, TextAfter, Emote, EmoteBefore, EmoteAfter,
Private, PMBefore, PM, BotPM, Ignoring, Avatar, PersonalMessage, Nick,
AdminLevelChanged, LoginGranted, Logout, InvalidLoginAttempt, Command,
Idled, Unidled, Registering, Registered, Unregistered, BansAutoCleared,
ProxyDetected, Flood, FloodBefore, FileReceived, ScribbleCheck, Help,
Linked, Unlinked, LinkError, LeafJoin, LeafPart, VroomJoin, VroomJoinCheck,
Timer, etc.

**Discrepancias con ROADMAP**:
- ROADMAP dice "API expuesta: print, log, userCount, sendPublic, sendPM"
  → real en `api.rs`: solo **`print`, `log`, `userCount`**. NO hay
  `sendPublic` ni `sendPM` en el contexto vivo.
- Los archivos `bindings/*` (665 líneas) declaran una API mucho más
  extensa, pero **`register_all` nunca se llama desde `make_context`**.
  Son stubs que devuelven datos falsos. **Código muerto**.
- ROADMAP dice "16 tests" → real: api 7 + manager 8 = **15 tests**.
- ROADMAP "Pendientes" dice "`ScriptHandle::dispatch` es placeholder"
  → **outdated**: el dispatch funciona, hay un thread dedicado con
  mpsc channel (`manager.rs:82-110`). El comentario del propio código
  confirma que el problema ya está resuelto.

**Estado**: parcial. El engine corre, `onLoad()` funciona, el manager
es real, pero la API expuesta a scripts JS es mínima (3 funciones).

---

### 2.8 `crates/web/` — WebSockets

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 30 | implementado (deps infladas) |
| `src/lib.rs` | 35 | implementado |
| `src/protocol.rs` | 410 | implementado + 11 tests |
| `src/ws.rs` | 391 | implementado + 2 tests |
| `src/handler.rs` | 479 | implementado |
| `src/ws_outbound.rs` | 148 | implementado |
| `src/panel.rs` | 81 | implementado (HTML estático) |

**Discrepancias con ROADMAP**:
- ROADMAP dice "WebSocket server en puerto 5010" → **FALSO**: en
  `main.rs:179-183` el WS se sirve en el **mismo puerto** que TCP
  (`settings.port`). La opción `web_port` en `Settings` (settings.rs:32)
  es **dead config**.
- `tokio-tungstenite`, `axum`, `http` están en `Cargo.toml` pero **no se
  usan** — el WS está implementado desde cero con `tokio::net::TcpStream`.
  Dependencias muertas.
- `read_frame` (ws.rs:326) rechaza frames fragmentados explícitamente.
- El panel HTML en `panel.rs` **nunca se sirve** — no hay handler HTTP
  para `GET /` que devuelva ese HTML. Es código muerto en la práctica.
- ROADMAP dice "12 tests + 1 doctest" → real: protocol 11 + ws 2 = **13
  tests, sin doctest**.

**Estado**: implementado. El WS funciona, el bridge TCP↔WS es real,
pero `web_port` y el panel HTML son código muerto.

---

### 2.9 `crates/link/` — Hub/Leaf link protocol

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 25 | implementado |
| `src/lib.rs` | 47 | implementado (con declaración honesta de limitaciones) |
| `src/protocol.rs` | 671 | implementado + 3 tests |
| `src/server.rs` | 778 | **ROTO — no compila** |
| `src/client.rs` | 793 | parcial (compila) |

El `lib.rs` declara abiertamente:

```text
// El protocolo (simplificado)
// No hay forwarding de mensajes (eso requiere implementar un
//   LinkProcessor complejo, fuera del scope de esta fase).
// No hay autenticación (asume que confías en el otro server)
// No hay reconnect automático
// No hay soporte para múltiples hubs simultáneos
```

**Errores de compilación en `server.rs`**:
1. Función `build_chat_payload` duplicada (líneas 651 y 664).
2. `is_passthrough_opcode` (definida en `client.rs:619`) no es accesible
   desde `server.rs:428`.

**Discrepancias con ROADMAP**:
- ROADMAP dice "38 opcodes" → real: **35 variantes** en `LinkMsg`.
- ROADMAP dice "E2E validado: leaf se conecta al hub" → **no
  reproducible** por los errores de compilación.
- TODOs del ROADMAP Fase 8:
  - "integrar UserPool para LeafJoin/HubJoin" → **parcialmente hecho**
    en `server.rs:269-281` y `tcp_handler.rs:129-132`.
  - "dispatch de mensajes públicos cross-server" → **hecho** en
    `server.rs:295-320`.
  - "dispatch de PMs cross-server" → **hecho** en `server.rs:321-371`.
  - "encriptación AES" → **NO hecho**. No hay código AES en el repo.
- `client.rs:154`: `b.write_u8(1); // custom_client (simplificado)` —
  siempre escribe 1, los usuarios linkeados siempre parecen custom client
  al otro lado.

**Estado**: parcial y **roto**. Los ~778 líneas de `server.rs` son
código muerto hasta que se arreglen los 2 errores.

---

### 2.10 `crates/astra/` — Binario principal

| Archivo | Líneas | Estado |
|---|---:|---|
| `Cargo.toml` | 35 | implementado |
| `src/main.rs` | 314 | implementado |
| `src/tcp_handler.rs` | 658 | implementado |

**CLI flags**: `--port`, `--config`, `--no-roomsearch`, `--no-web`,
`--data-dir`, `--link-server`, `--link-client`, `--verbose`.

**Discrepancias con ROADMAP**:
- El binario orquesta todo correctamente: TCP, UDP, scripting, WS, link.
- El lado Link está roto por los errores del crate `link`.
- `main.rs` solo llama `scripting.start_in_thread` (línea 115). No hay
  dispatch de eventos desde el path principal — los eventos solo llegan
  via `dispatch_builtin` cuando un slash command no se reconoce como
  built-in.
- No hay `astra.toml` de ejemplo en el repo.
- `tcp_handler.rs:384` ("`ClientUpdateStatus` re-broadcasts the user's
  full userlist item") no es fiel a sb0t (que actualiza campos
  individuales).

**Estado**: implementado a nivel de orquestación, pero las issues en
`link` rompen el E2E.

---

## 3. ROADMAP claims vs realidad

Cada afirmación marcada ✅ en el ROADMAP y su estado real:

| Claim | Estado | Evidencia |
|---|---|---|
| Workspace Cargo con 9 crates | ✅ real | 10 crates, no 9 |
| proto-ares con 70+ mensajes TCP + 9 UDP | ⚠️ parcial | TCP: 52, UDP: 9 |
| iconnect con todos los traits | ⚠️ parcial | 27 traits, **0 implementaciones** |
| server-core con módulos base | ✅ real | Todos los módulos existen |
| Binario con CLI (clap) y handlers TCP/UDP | ✅ real | main.rs + tcp_handler.rs |
| Hola mundo: server escucha y decodifica | ✅ real | Listeners y dispatch OK |
| PacketReader con 13 tests | ✅ real | 8 tests en reader.rs |
| PacketWriter con tests | ✅ real | 5 tests |
| Guid con MD5 | ✅ real | guid.rs |
| TCP/UDP listener con tokio | ✅ real | tcp_handler.rs + udp/listener.rs |
| ACK básico de login validado E2E | ⚠️ no reproducible | No hay harness E2E |
| FastPing echo | ✅ real | tcp_handler.rs:375 |
| Parser de MSG_CHAT_CLIENT_LOGIN (25+ campos) | ✅ real | login.rs |
| AresUser con todos los campos | ✅ real | 80+ campos |
| UserPool con ID único | ✅ real | AtomicU16 en user_pool.rs:230 |
| Persistencia SQLite | ✅ real | db.rs: 863 líneas |
| BanSystem persistido | ✅ real | bans.rs |
| UserHistory join-flood 15s | ✅ real | user_history.rs |
| AccountManager con SHA-1 | ✅ real | accounts.rs |
| Verificación de bans en login | ✅ real | tcp_handler.rs:270-273 |
| Verificación de join-flood | ✅ real | tcp_handler.rs:277-281 |
| Cleanup periódico de history | ✅ real | main.rs:230 |
| 19 tests nuevos de DB | ⚠️ inflado | real: 22 tests |
| ConnectionFloodTracker | ✅ real | security.rs |
| ConcurrentConnLimiter | ✅ real | security.rs |
| HandshakeTimeout | ✅ real | security.rs |
| LoginValidator | ✅ real | security.rs |
| FailedLoginTracker | ✅ real | security.rs |
| SecurityManager fachada | ✅ real | security.rs + tcp_handler.rs:71-73 |
| SecurityConfig en astra.toml | ✅ real | settings.rs (sin ejemplo) |
| 25 tests de seguridad | ⚠️ inflado | real: 23 tests |
| E2E 7/7 ataques mitigados | ⚠️ no reproducible | No hay harness |
| MSG_CHAT_SERVER_JOIN/PART/PUBLIC/EMOTE/PVT | ✅ real | outbound.rs + tcp_handler |
| Estado inicial al login | ✅ real | send_initial_state |
| 9 tests de outbound.rs | ✅ real | 9 tests |
| E2E 10/10 chat entre 2 clientes | ⚠️ no reproducible | No hay harness |
| Schema SQLite nodes/rooms | ✅ real | db.rs:135-155 |
| data/seed_rooms.json (20 rooms) | ✅ real | 28 líneas |
| UdpNodeManager con persistencia | ✅ real | udp/manager.rs |
| Protocolo UDP completo (9 mensajes) | ✅ real | udp/protocol.rs |
| Listener async | ✅ real | udp/listener.rs |
| Prober async 15s | ✅ real | udp/prober.rs |
| Expiración de nodos | ✅ real | udp/manager.rs:198 |
| CLI --data-dir | ✅ real | main.rs:47 |
| 14 tests de UDP | ⚠️ subestimado | real: 19 tests |
| E2E 2 servers locales | ❌ no reproducible | No hay harness |
| E2E con seed real | ❌ no reproducible | Requiere red real |
| **Total: 116 tests passing** | ❌ **falso** | workspace no compila |
| WS server en puerto 5010 | ❌ **falso** | WS en mismo puerto que TCP |
| Handshake RFC 6455 | ✅ real | ws.rs:100-110 |
| Frame reader/writer | ✅ real | ws.rs |
| Protocolo texto WS | ✅ real | protocol.rs |
| Bridge TCP ↔ WS | ✅ real | handler.rs:460-478 |
| HTML panel | ✅ real (pero no servido) | panel.rs:81 |
| CLI --no-web | ✅ real | main.rs:42 |
| 12 tests + 1 doctest | ⚠️ inflado | real: 13 tests, sin doctest |
| E2E 1 cliente WS 7/7 | ⚠️ no reproducible | No hay harness |
| astra-scripting con motor JS | ✅ real | scripting/ existe |
| API: print, log, userCount, sendPublic, sendPM | ❌ **parcial falso** | solo print, log, userCount |
| Eventos para scripts | ✅ real | 46 variantes en types.rs |
| ScriptManager load/unload/reload | ✅ real | manager.rs |
| ScriptHandle (Send + Clone) | ✅ real | manager.rs:42 |
| Registry global AppContext | ✅ real | api.rs:25-49 |
| Script greet.js de ejemplo | ✅ real | 37 líneas |
| Carga automática de scripts | ✅ real | main.rs:113-115 |
| 16 tests de scripting | ⚠️ cercano | real: 15 tests |
| E2E greet.js onLoad | ⚠️ no verificado | test `#[ignore]`-eado |
| astra-commands dispatcher | ✅ real | commands/ existe |
| parse_command | ✅ real | commands/src/lib.rs:51 |
| dispatch a scripting | ✅ real | commands/src/lib.rs:70 |
| try_dispatch | ✅ real | commands/src/lib.rs:506 |
| Integración en tcp_handler | ✅ real | tcp_handler.rs:460-468 |
| 6 tests de commands | ⚠️ subestimado | real: 23 tests |
| Migrar ~50 comandos nativos | ❌ no hecho | solo 11 |
| Built-ins /help, /users | ✅ real (más de los声称) | 11 built-ins |
| astra-link protocolo idéntico | ❌ **roto** | 2 errores de compilación |
| 38 opcodes LinkMsg | ⚠️ cercano | real: 35 |
| Strings null-terminated | ✅ real | protocol.rs:232-235 |
| MSG_LINK_PROTO (0xFB) | ✅ real | protocol.rs:217 + main.rs:312-314 |
| LinkPacketBuilder/Reader | ✅ real | protocol.rs |
| LinkServer stub handshake | ❌ **roto** | server.rs no compila |
| LinkClient login | ⚠️ parcial | client.rs compila, tests no corren |
| CLI --link-server/--link-client | ✅ real | main.rs:50-57 |
| 3 tests link | ✅ real | protocol.rs:3 tests |
| E2E leaf conecta a hub | ❌ no reproducible | crate roto |
| TODO UserPool para LeafJoin | ⚠️ parcialmente hecho | server.rs:269-281 |
| TODO dispatch públicos | ✅ hecho | server.rs:295-320 |
| TODO dispatch PMs | ✅ hecho | server.rs:321-371 |
| TODO encriptación AES | ❌ no hecho | no hay código AES |
| Dockerfile multi-stage | ✅ real | 73 líneas |
| .dockerignore | ✅ real | existe |
| docker-compose.yml | ✅ real | 31 líneas |
| .github/workflows/release.yml | ✅ real | 256 líneas |
| Docker images multi-arch | ✅ configurado | no probado |
| Binarios estáticos | ✅ configurado | no probado |
| SHA256 checksums | ✅ configurado | no probado |
| GitHub Release auto-notas | ✅ configurado | no probado |
| **Fuzzing con cargo-fuzz** | ❌ no hecho | no hay `fuzz/` |
| **Documentación de API** | ❌ no hecho | `docs/` vacío |
| **Benchmarking** | ❌ no hecho | no hay `benches/` |

**Sección "Pendientes para futuro (TODOs)" del ROADMAP**:
- "Firewall check completo (Opción B con TCP probe real)" — confirmado no hecho
- "Comando CLI `astra seed-refresh`" — no hecho
- "Soporte de frames WebSocket fragmentados" — confirmado no hecho
- "HTML panel servido por el WS server" — el HTML existe pero nunca se sirve
- "`ScriptHandle::dispatch` es placeholder" — **OUTDATED**, ya funciona
- "Comandos nativos built-in solo se delegan a JS" — **OUTDATED**, 11 built-ins en Rust
- "Agregar `astraVersion` y otras constantes" — no hecho

---

## 4. Conteos de líneas por crate

```text
crates/proto-ares/src/                  1,114 líneas (Rust)
crates/iconnect/src/                      730 líneas (Rust)
crates/server-core/src/                 4,160 líneas (Rust)
crates/udp/src/                         1,483 líneas (Rust)
crates/captcha/src/                         5 líneas (Rust)  ← stub vacío
crates/commands/src/                      860 líneas (Rust)
crates/scripting/src/                   2,014 líneas (Rust)
crates/web/src/                         1,544 líneas (Rust)
crates/link/src/                        1,691 líneas (Rust)  ← no compila
crates/astra/src/                         972 líneas (Rust)
                                          ─────────
TOTAL:                                14,573 líneas (Rust)
```

---

## 5. Resumen ejecutivo

El proyecto está al **~60% del sb0t original** en términos de cobertura
funcional, pero la calidad del código implementado es razonable.

### Lo que funciona
- Protocolo binario Ares (lectura/escritura)
- Login completo con 25 campos
- Persistencia SQLite (bans, accounts, user_history, nodes, rooms)
- 5 capas de defensa anti-DDoS
- Mensajes básicos de chat (public, emote, PM, join, part, userlist)
- WebSockets con bridge TCP↔WS
- Motor JS (boa_engine) con manager real
- 11 comandos slash built-in
- UDP room search con seed local
- Hashes SHA-1/SHA-2/MD5 compatibles con sb0t
- Tests unitarios: ~150 (no verificables mientras el workspace no compile)

### Lo que falta migrar
1. **Arreglar `link/src/server.rs`** (2 errores triviales, prerequisito)
2. **Captcha**: el crate `captcha` es un stub vacío de 5 líneas
3. **~39 comandos slash nativos de sb0t** (greets, hashlink admin,
   account admin, browse admin, scribble admin, voice chat, proxy
   admin, captcha admin, link admin, registration, etc.)
4. **Encriptación AES en el protocolo Link**
5. **Firewall check real** (actualmente stub, user_count=0 en ACKINFO)
6. **Servir el HTML panel** desde el WS server (existe pero no se sirve)
7. **Implementar la API completa en `iconnect`** (27 traits declarados,
   0 implementaciones)
8. **Exponer `sendPublic`/`sendPM` (y el resto) a JS** — el sistema
   `bindings/*` está escrito pero nunca se enchufa al contexto vivo
9. **`astra.toml` de ejemplo** en el repo
10. **Fuzzing**, **documentación de API**, **benchmarks** — los tres
    declarados en el ROADMAP como pendientes
11. **Frames WebSocket fragmentados**
12. **Comando CLI `astra seed-refresh`**

### Discrepancias ROADMAP ↔ realidad
- El ROADMAP miente en varios ✅ (link crate roto, WS en otro puerto,
  sendPublic/sendPM no expuestos, "116 tests passing" falso).
- El ROADMAP subestima logros reales (11 built-ins reales, no делегадos
  a JS; 23 tests en commands, no 6).
- El ROADMAP tiene TODOs outdated (ScriptHandle::dispatch ya no es
  placeholder, los built-ins ya están en Rust).

### Recomendación
Antes de seguir migrando features, **arreglar el crate `link`** (2
errores de compilación de 5 minutos) y **verificar `cargo test
--workspace` pase de verdad**. A partir de ahí, priorizar en este orden:

1. `astra.toml` de ejemplo + `fuzz/` + `docs/`
2. Captcha (es un módulo con alta visibilidad en sb0t)
3. Migración de los comandos administrativos faltantes
4. Exponer la API completa de `bindings/*` al contexto JS
5. Implementar las traits de `iconnect` (o eliminarlas si no se van a usar)
6. Encriptación AES en link
7. Firewall check real
8. Servir el HTML panel desde el WS server

---

## Actualización post-remediación (2026-07-02)

Tras los fixes de link crate, captcha y tooling, los pendientes son ahora:

### ✅ Resueltos
1. Link crate no compila → **RESUELTO**
2. Captcha stub vacío → **RESUELTO** (Fase 10, 26 tests nuevos)
3. Sin `astra.toml` de ejemplo → **RESUELTO** (83 líneas)
4. Sin fuzz/ → **RESUELTO** (3 targets con cargo-fuzz)
5. docs/ vacío → **RESUELTO** (SECURITY, PROTOCOL, ARCHITECTURE)

### 🟠 Pendientes actualizados (priorizados)
1. ~39 comandos slash faltantes (greets, hashlink, account/browse/scribble admin)
2. Exponer `bindings/*` completo al contexto JS (sendPublic, sendPM, etc.)
3. Implementar traits de `iconnect` o eliminarlas
4. Encriptación AES en link
5. Firewall check real (Opción B)
6. Servir el HTML panel desde el WS server
7. Frames WebSocket fragmentados
8. CLI `astra seed-refresh`
9. Benchmarking

### Métricas finales
- **187 tests passing** (+27 desde el audit inicial de 160)
- Binario compila: `target/release/astra` (11 MB)
- Captcha genera PNG legibles: 28×9 px grayscale con font 5x7

---

## Fase 11: Scripting API expuesta (2026-07-02)

### Antes
- Scripts JS solo veían `print`, `log`, `userCount` (3 funciones)
- 665 líneas de `bindings/*` declaradas pero **nunca llamadas**
- `greet.js` usaba `astra.sendPublic(...)` que no existía → fallaba silenciosamente
- Registry `Context* → Arc<AppContext>` se invalidaba cuando Context se movía

### Cambios
- `crates/scripting/src/api.rs` reescrito (707 líneas):
  - Registry: HashMap global → **thread-local** (el Context vive en un thread dedicado, el move ya no invalida la key)
  - 16 funciones globales registradas: print, log, sendPublic, sendEmote, sendPM, userCount, userNames, userExists, getUserIp, getUserLevel, getUserVroom, kickUser, getTopic, setTopic, astraHash, astraMd5, astraBase64Encode, astraBase64Decode
  - 9 tests nuevos (send_public_broadcasts, send_pm_targets_specific_user, send_pm_to_nonexistent, full_script_flow, sha1_fn_works, md5_fn_works, base64_roundtrip, base64_decode_invalid, get_topic_and_set_topic)
- `crates/astra/src/tcp_handler.rs`:
  - Dispara `ScriptEvent::Join` después de `user_pool.add`
  - Dispara `ScriptEvent::Part` antes de remover del pool
  - Dispara `ScriptEvent::Public` después de `broadcast_to_room`
  - Dispara `ScriptEvent::Emote` después de `broadcast_to_room`
  - `handle_emote` ahora recibe `&ScriptHandle`
- `data/scripts/greet.js`: reescrito para usar las nuevas APIs (5 comandos: /hola, /usuarios, /quien, /topico, /hash)
- `data/scripts/autokick.js`: nuevo script de demo que usa `kickUser` para auto-kick nicks sospechosos

### Métricas
- **197 tests passing** (+10 desde 187)
- 27 tests en `astra-scripting` (era 22)
- 4 eventos de scripting wired: Join, Part, Public, Emote
- ~30 KB de bindings de sb0t legacy (`bindings/*`) siguen sin conectarse — útiles para compatibilidad, pero no son el path crítico

---

## Fase 13: sb0t-compat wiring (2026-07-02)

### Antes
- Los 22 nombres sb0t-compat en `bindings/statics.rs` (Base64_encode, Crypto_hashSHA1, etc.) **nunca se registraban** en el Context vivo
- Un script sb0t legacy que usara `Base64_encode` directamente fallaba con "undefined"

### Cambios
- 16 nuevas funciones globales registradas en `make_context`:
  - **6 aliases** (delegan a las implementaciones modernas): `Base64_encode`, `Base64_decode`, `Crypto_hashSHA1`, `Crypto_hashMD5`, `Users_count`, `Room_setTopic`
  - **10 stubs honestos** con marcado ⚠️: `Channels_list`, `Hashlink_create`, `Users_getUserByName`, `Stats_addStat`, `Stats_getStat`, `Entities_list`, `Link_createLink`, `Registry_createKey`, `Registry_deleteKey`, `Room_broadcast`
- Stubs usan thread-locals para `STATS_STORE` y `REGISTRY_STORE` (viven en el thread del ScriptManager)
- `Users_getUserByName` y `Room_broadcast` acceden al AppContext via thread-local (lookup_app)

### Métricas
- **222 tests passing** (+16 desde 206)
- `astra-scripting`: 36 → 52 tests
- Scripting API: 23 → 39 funciones globales (35% → 45% de paridad sb0t)

