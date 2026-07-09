# Astra — Roadmap V2 (cierre de la migración sb0t → Rust)

> Continuación del [ROADMAP.md](../ROADMAP.md) original (Fases 0-20).
> Basado en la auditoría de [AUDIT.md](AUDIT.md): el proyecto está al ~80-85%
> de paridad funcional con sb0t. Este roadmap cubre el 15-20% restante,
> priorizado por valor/esfuerzo.
>
> **Punto de partida** (2026-07-07): workspace compila limpio, 291 tests
> passing, 11 comandos built-in, scripting API con paridad declarada.

## Fase A — Comandos nativos de moderación e info ✅ (2026-07-07)

El hueco más grande: sb0t trae ~50 comandos nativos, Astra tenía 11.
Esta fase agrega los de moderación e información que no requieren
cambios de esquema:

- [x] `/kick <nick>` — Moderator+: expulsa sin ban (respeta jerarquía de niveles)
- [x] `/muzzle <nick>` / `/unmuzzle <nick>` — Moderator+: silencia en público
  - `AresUser.muzzled: bool → AtomicBool` + gate en `handle_public`/`handle_emote`
    (el muzzleado puede seguir usando comandos) + `LinkEvent::UserUpdated`
- [x] `/pmall <text>` — Admin+: PM a todos los usuarios
- [x] `/opmsg <text>` — Moderator+: mensaje `[ops]` a todos los Moderator+
- [x] `/uptime` (alias `/stats`) — uptime + online/peak/total joins
- [x] `/version` — versión del server (CARGO_PKG_VERSION)
- [x] `DEFAULT_HELP_LINES` actualizado + 16 tests nuevos (35 total en commands)

## Fase B — Comandos de cuentas ✅ (2026-07-07)

La infraestructura (`accounts.rs`, SHA-1 sb0t-compat, tabla `accounts`)
estaba completa desde Fase 2; ahora expuesta como comandos:

- [x] `/register <password>` — registra cuenta propia (gated por `allow_registration`)
- [x] `/unregister` — elimina la cuenta propia
- [x] `/login <password>` — `owner_password` → Owner; si no, verifica cuenta
  strict (nick+GUID+password) con fallback no-strict por password (modo sb0t)
  y restaura el nivel persistido + OpChange
- [x] `/grant <nick> <level>` — Admin+: nivel en vivo + persiste si hay cuenta;
  acepta nombres (`voice|moderator|admin|owner`) o números; no permite otorgar
  un nivel ≥ al propio ni modificar usuarios de nivel ≥
- [x] `/revoke <nick>` — Admin+: resetea a Regular
- [x] `AdminLevelChanged` + `MSG_CHAT_SERVER_OPCHANGE` en cada cambio de nivel

## Fase C — UDP correctness ✅ (2026-07-07)

- [x] `user_count` real en `ACKINFO`: `run_listener` recibe un `UserCountFn`
  inyectado desde `main.rs` (`user_pool.len()`). Validado E2E: login WS →
  `SENDINFO` → `ACKINFO users=1`
- [x] Firewall check real (Opción B): en `PROCEEDCHECKFIREWALL` se hace un
  TCP probe al puerto del solicitante (timeout 5s). Cookies con TTL 60s
  emitidos en `READYTOCHECKFIREWALL` y validados contra la IP origen
  (anti-reflection: nadie puede hacernos probar IPs de terceros); máx 4
  probes simultáneos, por encima responde `CHECKFIREWALLBUSY` con nodos
  alternativos. Tests E2E con sockets reales (flujo completo + cookie
  inválido rechazado)

## Fase D — WebSocket completitud ✅ (2026-07-07)

- [x] Frames fragmentados (RFC 6455 §5.4) en `read_ws_frame` (el path real de
  producción en `handler.rs`, que antes **corrompía** mensajes fragmentados):
  reensambla continuations, consume Ping/Pong intercalados sin perder el
  acumulador, rechaza fragmentación anidada y limita a 1 MiB. 5 tests nuevos
  con TCP real en loopback
- [x] Panel HTML servido: `GET /` sin `Upgrade: websocket` responde 200 con
  `panel::INDEX_HTML` (antes 400). Validado E2E con curl

## Fase E — Link hardening ✅ (2026-07-08)

- [x] Encriptación AES de mensajes link con **paridad exacta sb0t**
  (`crates/link/src/crypto.rs`, verificado contra `core/Crypto.cs`):
  - Cifrado de stream `e67`/`d67` (idéntico al de sb0t), con vector de
    referencia y test de roundtrip
  - Credentials del leaf: `SHA1(reverse(name ++ guid))` (20 bytes)
  - Key AES-256 + IV generados por el hub, enviados en `HubAck` ofuscados
    con `e67` sobre `MD5(guid_del_leaf)` (8 rondas); el leaf los des-ofusca
  - Post-handshake, los **strings** de cada mensaje van AES-256-CBC + PKCS7
    (`u16 len + ciphertext + null`), campos binarios en claro — igual sb0t
  - Vector AES-256-CBC verificado contra `openssl enc`
  - **Dual-mode**: sin `link_trusted_leaves` configurados, el hub opera en
    modo legacy (sin cifrar) para no romper links Astra existentes
  - 9 tests de crypto + 1 de roundtrip cifrado en protocol + 2 E2E
    (handshake cifrado con userlist descifrada; leaf no autorizado rechazado)
- [x] Autenticación de leafs: lista de `link_trusted_leaves` (name+guid) en
  `astra.toml`; el hub valida credentials y rechaza leaves desconocidos
- [x] Reconnect automático del `LinkClient` con backoff — ya existía
  (exponencial 1s→60s en `LinkClient::run`); corregido bug: `peer_users`/
  `peer_name` no se limpiaban al reconectar (duplicaba usuarios del hub)

## Fase F — Tooling y limpieza 🚧

- [x] CLI `astra seed-refresh [--url <URL>]` — descarga el rooms.json
  (default `chatrooms.mywire.org/rooms.json`), lo valida antes de
  sobrescribir `<data_dir>/seed_rooms.json` y fuerza la recarga en DB
  (`load_seed_force`). Validado E2E contra un HTTP server local
- [x] Benchmarks (criterion) de PacketReader/Writer en
  `crates/proto-ares/benches/packets.rs` (`cargo bench -p proto-ares`);
  baseline: writer ~84ns, reader ~56ns por paquete estilo login
- [x] ~~Reemplazar stubs de scripting~~ — ya estaban implementados:
  `Entities_list` lee el snapshot `ctx.udp_nodes`, y `Link_createLink`/
  `Link_disconnect`/`Link_kickHub` publican al bus `LinkRequest` que tiene
  consumer real en `main.rs` (el item venía del audit desactualizado)
- [x] `iconnect` **eliminado** (2026-07-08). En sb0t `iconnect` era el ABI
  de plugins de terceros (los proyectos `commands`/`scripting` dependían
  solo de él), pero Astra no expone plugins binarios —la extensibilidad es
  vía scripting JS embebido— así que los 27 traits nunca se implementaron.
  Decisión del dueño: sin soporte de plugins de terceros. Se movieron los 3
  tipos de datos realmente usados (`ILevel`, `IFont`, `ILink`) a
  `server-core::types` y se borró el crate (−745 líneas, un crate menos en
  el workspace). También se eliminó `BanSystem::to_iban_vec` (código muerto)
- [x] **Greets** (mensajes de bienvenida) — `GreetManager` en server-core
  con persistencia SQLite (tabla `greets`), rotación y sustitución de
  placeholders (`+n +ip +id +f +v +uc +rn +ut +l`, paridad `Greets.cs`).
  Comandos `/greets [on|off]`, `/addgreet`, `/remgreet <i>`, `/listgreets`
  (Admin+). Se envía como PM del bot al entrar, en TCP y WS. Validado E2E
- [x] **Word filter** — `WordFilterManager` en server-core con persistencia
  (tabla `word_filters`), matching con comodines `*`/`?` (paridad
  `WordFilter.cs`) y acciones `block`/`kick`/`ban`. Comandos `/addfilter
  <word> [accion]`, `/remfilter`, `/listfilters` (Admin+). Aplica a
  usuarios regulares (Moderator+ exentos) en público TCP y WS. Validado E2E
- [x] **Paridad TOTAL de comandos sb0t (2026-07-09)**: se migraron los ~95
  comandos del `Eval.cs` de sb0t en 7 tandas. Cobertura verificada: **0
  comandos de sb0t sin cubrir**. 66 built-ins base + aliases con los nombres
  originales de sb0t. Subsistemas nuevos en server-core: `UrlManager`,
  `GreetManager`, `WordFilterManager`, `RangeBanManager`/`AsnBanManager`,
  `RoomFlags` (11 toggles), `NameFilterManager` (join/file), `text_effects`
  (kiddy/lower/kewl/paint), historial de mensajes + ban-log en AppContext.
  - Tanda 1 URLs · Tanda 2 historial/info · Tanda 3 bans avanzados ·
    Tanda 4 moderación · Tanda 5 permisos de sala · Tanda 6 efectos de texto ·
    Tanda 7 cuentas/quarantine/filtros/misc.
  - Enforcement real: range/join filters en login, caps/scribbles/avatars en
    sus paths, muzzle temporal auto-expirante, disableadmins gate global.
  - 407 tests, 0 fallos.
- [x] **Comandos "externos" implementados (2026-07-09)**: tras revisar el
  fuente de sb0t, casi todos los que estaban stubeados eran en realidad
  implementables:
  - `vspy`/`ipsend`/`logsend`/`bansend` → **feeds internos** (suscripción
    per-admin), no push a hub. Implementados con flags en `AresUser` +
    `AppContext::notify_subscribers`.
  - `trace` + `asnban` enforcement → módulo `geoip` (crate `maxminddb`) que
    lee `city.mmdb`/`asn.mmdb` **opcionales** de `data_dir` (GeoLite2 o
    DB-IP Lite). Sin archivos, degradan a mensaje honesto.
  - `define`/`urban` → HTTP async (reqwest) con la **misma URL + api_key
    hardcodeada de sb0t**; el fetch corre en task tokio y PMea el resultado.
  - Único stub restante: `loadtemplate` (necesitaría un subsistema de
    templates/i18n; los mensajes de Astra están hardcodeados).
  - 415 tests, 0 fallos.

## Orden de ejecución

| Orden | Fase | Valor | Esfuerzo |
|---|---|---|---|
| 1 | A (moderación) | Alto — paridad visible para usuarios | Medio |
| 2 | B (cuentas) | Alto — sin esto no hay ops persistentes | Bajo |
| 3 | C (UDP) | Medio — corrige dato falso publicado a la red Ares | Bajo |
| 4 | D (WebSocket) | Medio | Bajo |
| 5 | E (Link) | Medio — solo importa multi-servidor | Alto |
| 6 | F (Tooling) | Bajo | Medio |

---

## Auditoría de paridad sb0t (revisión 2026-07-09)

Revisión exhaustiva sb0t↔Astra + implementación de gaps encontrados.

### Corregido

- **Wire TCP compatible con Ares real** (crítico). Antes Astra usaba framing
  propio (`[op][payload]`, strings i32), incompatible: ningún cliente Ares de
  escritorio podía conectar. Ahora habla el wire real:
  - Framing `[size:u16 LE][op][payload]` (lectura con acumulación de bytes,
    escritura con prefijo en la writer task de TCP).
  - Strings null-terminated (`read_string_nt`/`write_string_nt` en proto-ares)
    para clientes sin cifrar. link/udp mantienen su encoding.
  - Verificado E2E con login Ares framed + público con eco.
- **Protocolo WebSocket ib0t/sb0t** (commit previo): clientes web reales
  (ib0t/inbizio) conectan; secuencia de estado inicial + broadcast traducido.
- **Voice chat relay**: wrapper ADVANCED_FEATURES (250) + VcFirst/Chunk público
  y privado (paridad TCPAdvancedProcessor).
- **Opcodes antes ignorados**: ClientCommand, AUTHLOGIN/AUTHREGISTER (→ /login,
  /register), AUTOLOGIN (auto-login por GUID).
- **Comandos**: kill, ban10/ban60 (bans temporales), whisper, shout, pmblock
  (+ flag pm_blocked), rempassword, unecho, unkiddy, viewfilter, y aliases
  planos de sb0t (addwordfilter/addjoinfilter/addfilefilter + rem*).

### Cifrado del cliente Ares (crypto=250) — IMPLEMENTADO

Handshake AES completo (paridad `Crypto.cs` / `TCPOutbound.CryptoKey`):
- Al login con `crypto=250`, el server genera key AES-256 + IV y los manda en
  `MSG_CHAT_SERVER_CRYPTO_KEY` (op 230, envuelto en ADVANCED_FEATURES 250), con
  `IV++Key` ofuscado con `e67` sobre el GUID (MD5) del cliente.
- Desde ahí **todos los strings** viajan cifrados AES-256-CBC/PKCS7 como
  `u16 len + ciphertext + null`; los campos binarios en claro.
- `proto_ares::AresCrypto` + `PacketWriter::with_msg_crypto` / `read_string_nt`
  crypto-aware; builders `_c` (variante cifrada) + helpers `AresUser::send_pvt/
  send_public/send_emote`. Broadcasts por-destinatario (cada cliente cifrado
  recibe su copia con su key; los sin cifrar y WS comparten el paquete plano).
- Verificado E2E: cliente Python que des-ofusca con `d67`+MD5(guid), descifra
  LoginAck/features/topic y hace round-trip de público cifrado. Sin regresión
  en clientes sin cifrar ni WS.

### Comandos host* + propagación por link — IMPLEMENTADO

- hostban/hostkick/hostkill/hostmuzzle/hostunmuzzle/hostunban/hostclone/hostcban
  con gate Host (= Owner en Astra). Aplican local y se propagan por la red:
  `LinkEvent::AdminAction` → wire `LinkMsg::Admin` (`[kind:u8][target:str]`,
  cifrado AES del link) → cada servidor lo aplica con
  `AppContext::apply_admin_action`; el hub hace fanout a los demás leaves con
  `origin` (sin eco). Verificado E2E con hub+leaf reales: `/hostmuzzle` desde
  el hub silenció a un usuario del leaf.
- hostcban limpia bans + range bans + muzzles + efectos de texto (paridad
  HostCBans). `RangeBanManager::clear()` nuevo.
- **jsmsg**: no era gap — nunca fue built-in en sb0t; rutea al scripting
  (`ScriptEvent::Command` → onCommand), igual que Astra ya hacía.
- **loadtemplate**: mensaje honesto (Astra usa mensajes built-in; no hay
  plantillas que recargar). Era el último stub.

### Diferido (fuera de alcance)

- **File search/sharing**: `ClientBrowse` se relaya al link, pero `ClientSearch`/
  `AddShare`/`RemShare` no se sirven (feature P2P grande, fuera de alcance de un
  servidor de chat). SHARING se sigue anunciando por el browse.
