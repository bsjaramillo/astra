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
- [ ] Comandos restantes de sb0t de baja prioridad (greets, scribble admin,
  proxy admin, captcha admin, filter)

## Orden de ejecución

| Orden | Fase | Valor | Esfuerzo |
|---|---|---|---|
| 1 | A (moderación) | Alto — paridad visible para usuarios | Medio |
| 2 | B (cuentas) | Alto — sin esto no hay ops persistentes | Bajo |
| 3 | C (UDP) | Medio — corrige dato falso publicado a la red Ares | Bajo |
| 4 | D (WebSocket) | Medio | Bajo |
| 5 | E (Link) | Medio — solo importa multi-servidor | Alto |
| 6 | F (Tooling) | Bajo | Medio |
