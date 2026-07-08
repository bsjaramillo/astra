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
- [x] Fuzzing del protocolo binario con `cargo-fuzz` (3 targets: reader, writer, login)
- [x] `astra.toml.example` con todos los campos documentados
- [x] Documentación: `docs/SECURITY.md`, `docs/PROTOCOL.md`, `docs/ARCHITECTURE.md`
- [ ] Benchmarking

**Fase 10 ✅ Captcha (protección anti-bot para IPs nuevas)**
- [x] **`astra-captcha` crate**: genera palabras de 4 letras (wordlist 256) + imagen PNG (font 5x7 bitmap)
- [x] **`CaptchaManager`** en server-core: tracking por user_id, expiración, máx intentos
- [x] **Config en `SecurityConfig`**: `captcha_enabled`, `captcha_expiration_secs`, `captcha_max_attempts`
- [x] **Integración en `process_handshake`**: si `captcha_enabled` y la IP no tiene historial previo, generar challenge
- [x] **Prompt**: PM del bot con código obfuscado (case-mix + ruido)
- [x] **Verificación**: PM al bot con la respuesta correcta → un-quarantine
- [x] **Gate en `handle_public`**: users con captcha pendiente no pueden hablar en público
- [x] **`UserHistory::has_prior_join`**: query a DB para detectar IPs nuevas
- [x] Tests: 14 en `astra-captcha` + 11 en `server-core::captcha`

**Fase 11 ✅ Scripting API completa (expuesta a JS)**
- [x] **Refactor del registry**: `Context*` → `Arc<AppContext>` reemplazado por **thread-local** (Context no-Send vive en un thread dedicado, evita invalidación de punteros)
- [x] **Mensajería**: `sendPublic(from, text)`, `sendEmote(from, text)`, `sendPM(from, to, text)`
- [x] **Usuarios**: `userCount()`, `userNames()`, `userExists(name)`, `getUserIp(name)`, `getUserLevel(name)`, `getUserVroom(name)`, `kickUser(name)`
- [x] **Sala**: `getTopic()`, `setTopic(text)`
- [x] **Hashing**: `astraHash(s)` (SHA-1), `astraMd5(s)`, `astraBase64Encode(s)`, `astraBase64Decode(s)`
- [x] **Eventos disparados al script**: `onUserJoin`, `onUserPart`, `onPublic`, `onEmote` (wiring en `tcp_handler.rs`)
- [x] **Demo scripts**: `greet.js` (mejorado con todas las APIs), `autokick.js` (nuevo, usa `kickUser`)
- [x] Tests: 27 en scripting/api (5 nuevos: sha1, md5, base64 roundtrip, base64 invalid, get/set topic, full_script_flow, send_public_broadcasts, send_pm_targets_specific_user, send_pm_to_nonexistent)

**Fase 12 ✅ File I/O + Zip + ScriptInclude + Spell (Fase 1 del SCRIPTING-ROADMAP)**
- [x] **`File_exists(path)`** — `std::path::Path::exists()`
- [x] **`File_size(path)`** — `std::fs::metadata().len()` (o -1 si no existe)
- [x] **`File_creationTime(path)`** — `metadata.created()` como unix epoch secs
- [x] **`Zip_compress(data)`** — `zip` crate con `CompressionMethod::Deflated`, retorna base64
- [x] **`Zip_decompress(b64_data)`** — lee zip y extrae el primer entry
- [x] **`ScriptInclude_run(path)`** — lee archivo JS y lo evalúa en el mismo Context (funciones quedan disponibles)
- [x] **`Spelling_check(word)`** — verifica contra wordlist de 100 palabras comunes
- [x] Tests: 9 nuevos (file_exists_real, file_size_real, file_size_missing_returns_negative, zip_compress_decompress_roundtrip, zip_decompress_invalid_returns_null, script_include_runs_other_file, script_include_missing_file_returns_false, spelling_check_known_word, spelling_check_unknown_word)
- [x] **Bug fix**: `base64_decode_bytes` separado de `base64_decode` (este último asume UTF-8, el primero devuelve `Vec<u8>` para datos binarios)

**Fase 13 ✅ sb0t-compat wiring (Fase 2 del SCRIPTING-ROADMAP)**
- [x] **6 aliases** registrados con nombres sb0t originales (delegan a las funciones modernas):
  - `Base64_encode`, `Base64_decode`, `Crypto_hashSHA1`, `Crypto_hashMD5`, `Users_count`, `Room_setTopic`
- [x] **10 stubs honestos** (⚠️ comportamiento parcial o default):
  - `Channels_list` → `"[0]"` (solo vroom 0 por ahora)
  - `Hashlink_create(server, port)` → `astrahash://server:port` (formato URL real)
  - `Users_getUserByName(name)` → `"User:name:ip:level"` o `null` (consulta el pool)
  - `Stats_addStat(key, value)` / `Stats_getStat(key)` → thread-local HashMap
  - `Entities_list` → `"[]"` (no hay integración con UdpNodeManager aún)
  - `Link_createLink(server, port)` → `false` + warning (no implementado)
  - `Registry_createKey(name)` / `Registry_deleteKey(name)` → HKLM virtual thread-local
  - `Room_broadcast(text)` → alias de `sendPublic("Bot", text)` (broadcast real)
- [x] Tests: 16 nuevos (cubren cada alias con vector conocido + cada stub con su default esperado)

**Fase 14 ✅ Hooks *Before con cancelación (Fase 3 del SCRIPTING-ROADMAP)**
- [x] **Nuevo módulo `ScriptRequest`**: enum con 3 variantes (`TextBefore`, `EmoteBefore`, `PMBefore`) cada una con `std::sync::mpsc::SyncSender<bool>` para reply
- [x] **`ScriptHandle` extendido** con 3 métodos sync:
  - `check_text_before(from, text) -> bool` (bloquea 100ms, default allow)
  - `check_emote_before(from, text) -> bool`
  - `check_pm_before(from, to, text) -> bool`
- [x] **`Manager::dispatch_request`**: ejecuta la función JS en cada script activo, retorna `false` si ALGUNO retorna `false`
- [x] **`call_handler_with_return`**: nueva helper que captura el return value de la función JS
- [x] **Doble canal en el manager**: events (async) + requests (sync con reply)
- [x] **Wireado en tcp_handler.rs**: los 3 hooks se llaman antes de broadcast
- [x] Tests: 8 nuevos (cancel public, dejar pasar, return non-bool ignorado, cancel emote, cancel PM, multi-script any-cancel-wins, no-handler default allow, handle dead default)
- [x] **Bug encontrado y aislado**: `boa_engine::Context` es `!Send`, no se puede usar desde un thread distinto al que lo creó. Los tests llaman a `dispatch_request` directamente sobre el manager (en el mismo thread) en vez de via `start_in_thread`

**Fase 15 ✅ Eventos administrativos y de cuenta (Fase 4 del SCRIPTING-ROADMAP)**
- [x] **LoginGranted** — disparado después de enviar `LoginAck` en `process_handshake`
- [x] **Logout** — disparado antes del cleanup en `handle_tcp_client`
- [x] **InvalidLoginAttempt** — disparado cuando falla la capa 4 de validación
- [x] **Flood** — disparado cuando se detecta join-flood (15s window)
- [x] Tests: 11 nuevos (login_granted, logout, invalid_login_attempt, flood, admin_level_changed, bans_auto_cleared, idled/unidled, proxy_detected, multiple_handlers, dispatch_passes_correct_args, error_in_one_script_doesnt_affect_others)
- [ ] Pendiente: AdminLevelChanged wired a /ban, /unban (requiere refactor de `dispatch_builtin` para tomar `&ScriptHandle`)
- [ ] Pendiente: BansAutoCleared wired a cleanup de bans (no hay `ban.prune` aún — la tabla de bans no tiene `expires_at`)
- [ ] Pendiente: ProxyDetected wired a `LoginValidator` (capa 4)
- [ ] Pendiente: Idled/Unidled wired a `IdleManager` (no está en uso actualmente)

**Fase 16 ✅ Channels + Vroom (Fase 5 del SCRIPTING-ROADMAP)**
- [x] **`VroomManager`** en server-core: HashMap<u16, VroomInfo> con `RwLock`
  - Vroom 0 (Main Room) pre-creado
  - `create/delete/get/list_ids/set_topic/count`
  - Auto-creación de vrooms al hacer `/vroom <nuevo_id>`
- [x] **`AppContext.vrooms: Arc<VroomManager>`**
- [x] **`Channels_*` funciones en JS** (5 nuevas):
  - `Channels_list()` → `"[0,1,2,...]"` (JSON array de IDs)
  - `Channels_get(id)` → `{"id":N,"name":"...","topic":"..."}` o `"null"`
  - `Channels_create(id, name)` → bool
  - `Channels_setTopic(id, topic)` → bool
  - `Channels_broadcast(id, from, text)` → envía solo a users en ese vroom
- [x] **`onVroomJoin(name, vroom)`** — disparado en `tcp_handler.rs` después de `/vroom`
- [x] **Auto-creación de vroom en `/vroom`** — si el ID no existe, se crea con nombre default
- [x] Tests: 11 nuevos (vroom_0_exists_by_default, create_and_get, create_duplicate_fails, delete_vroom_0_fails, delete_existing, list_ids_includes_0, list_ids_json_format, get_json_format, get_json_nonexistent, set_topic_updates, set_topic_nonexistent_fails + channels_list, channels_create_and_list, channels_get_returns_json, channels_set_topic, channels_broadcast_only_to_vroom)

**Fase 17 ✅ Hashlink + Link management (Fase 6 del SCRIPTING-ROADMAP)**
- [x] **`Hashlink_create(server, port)`** ✅ desde Fase 13 — genera URL `astrahash://server:port`
- [x] **`Hashlink_parse(url)`** — extrae `{"server":"x.com","port":5009}` (soporta IPv6 brackets)
- [x] **`Link_list()`** → `"[]"` (stubs honestos — no hay LinkManager expuesto aún)
- [x] **`Link_getUserList()`** → JSON array de users locales (los remotos requieren integración LinkClient)
- [x] **Stubs honestos**: `Link_disconnect`, `Link_findLeaf`, `Link_findUser`, `Link_findHub`, `Link_kickHub` (log warning + return default)
- [x] **Bridge `LinkEvent → ScriptEvent`** en `main.rs`: task tokio que escucha `link_events` y dispara `onLeafJoin`/`onLeafPart` a scripting
- [x] Tests: 11 nuevos (hashlink_parse_valid, hashlink_parse_invalid, link_list_empty, link_get_user_list_local, link_create_link_stub, link_disconnect_stub + 5 tests de eventos Link en el manager)

**Fase 18 ✅ Avatar, Scribble, File browse (Fase 7 del SCRIPTING-ROADMAP)**
- [x] **`onAvatar(name)`** — disparado en `tcp_handler.rs` cuando se recibe `MSG_CHAT_CLIENT_AVATAR` (opcode 9)
- [x] **`onFileReceived(name, hashlink)`** — disparado cuando se recibe `MSG_CHAT_CLIENT_BROWSE` (opcode 50), parsea el hashlink
- [x] **`onScribbleCheck(name, is_pm)`** — disparado en `MSG_CHAT_CLIENT_SCRIBBLE_ROOM_FIRST/CHUNK` (no bloquea, solo audit)
- [x] **`Avatar_new(b64_bytes)`** — crea avatar desde base64 en thread-local store, retorna id (índice)
- [x] **`Avatar_getSize(id)`** — retorna tamaño en bytes del avatar (o -1 si no existe)
- [x] Tests: 6 nuevos (avatar_event_calls_handler, file_received_event_calls_handler, scribble_check_event_calls_handler + avatar_new_returns_id, avatar_new_invalid_base64, avatar_get_size_returns_correct_value)

**Fase 19 ✅ Stats, Registry, Entities, Query (Fase 8 del SCRIPTING-ROADMAP)**
- [x] **`Stats_addStat/getStat`** ✅ desde Fase 13 (thread-local HashMap)
- [x] **`Registry_createKey/deleteKey`** ✅ desde Fase 13 (HKLM virtual)
- [x] **`Entities_list`** ✅ desde Fase 13 (`"[]"` stub)
- [x] **`Spelling_suggest(word)`** — array JSON de sugerencias por prefijo de 2+ chars
- [x] **`Query_new(sql)`** — ejecuta SELECT (solo lectura) sobre la DB SQLite, retorna id
- [x] **`Query_getResults(id)`** — JSON array con `{col:val, col:val}` por fila
- [x] **`Query_getColumnCount(id)`** — cantidad de columnas
- [x] **`Query_getRowCount(id)`** — cantidad de filas
- [x] **Validación SQL**: solo permite `SELECT`/`WITH`/`EXPLAIN` (bloquea DELETE/DROP/INSERT)
- [x] **Nuevo método `Database::execute()`** (público) para INSERT/UPDATE/DELETE interno
- [x] Tests: 5 nuevos (spelling_suggest_returns_array, spelling_suggest_no_match, query_new_select_works, query_new_blocks_writes, query_get_results_nonexistent_returns_null)

**Fase 20 ✅ Timer, Help, Connect/Disconnect (Fase 9 del SCRIPTING-ROADMAP) — 🎯 100% PARITY**
- [x] **`onConnect(ip)`** — disparado al abrir el socket TCP (en `handle_tcp_client`)
- [x] **`onDisconnect(ip)`** — disparado al cerrar el socket
- [x] **`onUserList(name, users_csv)`** — disparado por cada user en la userlist inicial
- [x] **`onUserListEnd(name)`** — disparado al final del envío de userlist
- [x] **`onHelp(command)`** — disparado en `/help` (puede agregar líneas)
- [x] **`Help_addLine(cmd, line)`** — agrega línea custom al `/help` desde script
- [x] **`setTimer(secs, fn_name)`** — agenda una función JS (one-shot) — usa `pop_due_timers` en el loop del manager
- [x] **`clearTimer(id)`** — cancela un timer
- [x] **`onTimer(id, fn_name)`** — handler llamado cuando el timer expira
- [x] Tests: 10 nuevos (6 de eventos + 4 de Help_addLine/setTimer)

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
