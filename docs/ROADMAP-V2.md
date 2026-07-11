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
- [x] **(2026-07-10) Auto-publicación real hacia la red**: el prober mandaba
  `SENDINFO` (consulta: "¿sos una room?") a los nodos conocidos en vez de
  `ADDIPS` (anuncio: "acá estoy, agregame"). SENDINFO no hace que nadie nos
  agregue a su lista de nodos — por eso la sala respondía bien si alguien la
  consultaba directo, pero nunca llegaba a aparecer en los clientes reales
  (nadie se enteraba de que existía). Fix en `crates/udp/src/prober.rs`
  (`push_once`, antes `probe_once`): manda `ADDIPS` con `build_addips`,
  paridad `UdpListener.Push()` de sb0t. Nuevo `active_nodes_excluding` en el
  manager (paridad `GetServers(target_ip,...)`). Verificado E2E con un nodo
  UDP simulado.

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

### TCP: keepalive y desconexión silenciosa — IMPLEMENTADO (2026-07-10)

Reportado: un cliente Ares real dejaba de recibir mensajes tras un rato de
inactividad, sin que la app mostrara "desconectado". Causa raíz (paridad
`ServerCore.cs` de sb0t): Astra nunca implementó el `FASTPING` que sb0t manda
a CADA cliente logueado **cada 2 segundos** — ese ping es lo que mantiene viva
la conexión contra NAT/firewalls que reciclan mappings TCP ociosos. Sin él,
Astra además tenía un timeout propio de solo 120s sin lectura del cliente
(sb0t no tiene un timeout así: se apoya enteramente en el FASTPING + en que un
`send()` fallido revela una conexión muerta). Resultado: cualquier usuario que
solo leyera sin escribir por más de 2 minutos era desconectado por el propio
server, sin aviso, y si además el NAT ya había reciclado el mapping antes de
eso, ni el cliente ni el server se enteraban (silencio total).

- Nueva task periódica en `main.rs` (cada 2s): manda `FASTPING` (opcode 14,
  paquete vacío, cifrado-invariante) a todos los clientes Ares TCP logueados.
- `idle_timeout_secs` default 120 → 1800 (30 min): pasa de ser el mecanismo
  principal de liveness a una red de seguridad para conexiones realmente
  colgadas.
- `writer_task` ahora avisa por un `oneshot` cuando una escritura falla; el
  loop de lectura lo corre en `select!` junto al read normal, así una
  conexión muerta se detecta por el lado de ESCRITURA (mucho más rápido) sin
  depender de que el lado de lectura también falle.
- Bonus: varios broadcasts periódicos/de scripting (rotación de URLs, reloj
  de sala, `sendPublic`/`sendEmote`/`setTopic`/`Room_broadcast`/
  `Channels_broadcast` de la API JS) armaban el paquete UNA vez y lo mandaban
  igual a todos, ignorando el cifrado AES de cada cliente — un cliente
  cifrado recibía ahí un string en claro que no podía decodificar. Ahora se
  arman por-destinatario (`build_*_c` + `user.ares_crypto`), igual que ya
  se hacía para el chat normal.

Verificado E2E: conexión TCP real, login, mensaje público, 8s de "inactividad"
(el cliente no manda nada) recibiendo FASTPINGs cada 2s, y un mensaje después
de esa espera llega y hace eco normalmente — reproduce exactamente el flujo
reportado. 18 suites de tests en verde.

### Bugs de cliente real: nombre/topic, /login, imágenes y audio — IMPLEMENTADO (2026-07-11)

Reportados probando con un cliente inbizio real ya en producción (sala visible
en la red tras los fixes UDP anteriores):

- **CLI: `--port` clobbereaba silenciosamente el `port` del `--config`**.
  `settings.port = cli.port;` corría siempre, y `--port` tenía
  `default_value_t = 5009` — así que correr `astra --config astra.toml` SIN
  pasar `--port` explícito ignoraba el puerto del toml y bindeaba 5009 sin
  ningún error/warning. `port` ahora es `Option<u16>`; solo pisa el valor del
  toml si se pasa explícitamente. (Encontrado mientras se armaba el E2E de
  esta misma tanda: cualquier invocación `--config`-only pisaba 5009.)
- **La sala aparecía con nombre/topic genéricos** ("Astra Chat"/"Welcome to
  Astra"): `handle_send_info` (listener.rs) leía `ASTRA_ROOM_NAME`/
  `ASTRA_ROOM_TOPIC` de variables de entorno que nadie seteaba, en vez de
  `ctx.settings.room_name`/`ctx.current_room_topic()`. Nuevo `RoomInfoFn`
  (mismo patrón que `UserCountFn`) inyectado desde `main.rs`, así el ACKINFO
  siempre refleja la config real (y el topic en vivo, no un valor fijo).
- **`/login` no reflejaba el nivel actualizado**: `apply_level` solo mandaba
  `OPCHANGE` binario (no-op para clientes web, cuyo `sender` es `None`) y
  nunca el ib0t `UPDATE:{name},1:{name}{level}` que el cliente real usa para
  refrescar el badge/crown del userlist (paridad `ib0tClient.Level` setter de
  sb0t). Ahora se difunde `UPDATE` a todos los web clients de la vroom, y un
  refresh de join/userlist a los clientes Ares TCP — a **todos** los que
  cambien de nivel (grant/revoke/login/register), no solo `/login`.
- **Imágenes y audio no se mostraban** (aun con `scribbles on`/`audios on`):
  Astra nunca manejaba los idents `CUSTOM_DATA_HEAD`/`CUSTOM_DATA_BODY` (ni
  sus variantes `PM_`) — el mecanismo real que un cliente inbizio moderno usa
  para mandar imágenes/audio en chunks de ≤30000 chars de base64 (paridad
  `WebProcessor.CustomDataHead/Body` + `CustomData.cs` de sb0t). Caían al
  catch-all y se perdían en silencio. Implementado:
  - `server_core::custom_data::CustomDataStore`: reensamblado por `id`
    (HEAD abre la transferencia con `sender`+`size`; cada BODY agrega un
    chunk; al completarse `size` chunks, entrega `(sender, target, vroom,
    data)`). Dos instancias en `AppContext` (pública y PM).
  - `crates/web/src/handler.rs`: al completarse una transferencia pública,
    re-chunkea y difunde `SCRIBBLE_HEAD/BLOCK` (imágenes, a todo web client de
    la vroom, gate `room_flags.scribbles`) o `AUDIO_HEAD/BLOCK` (audio, solo a
    clientes inbizier, gate `room_flags.audios`). Las privadas van a un solo
    destinatario inbizier respetando su ignore list (`PM_SCRIBBLE_*`/
    `PM_AUDIO_*`).
  - Nuevos builders en `crates/web/src/protocol.rs` (formato exacto extraído
    del cliente real en `~/Development/Javascript/ReactJS/inbizio-web-ios/`
    y de `WebOutbound.cs`/`ib0tClient.cs` de sb0t).

Verificado E2E con dos clientes WS reales (login inbizio v6000): ACKINFO UDP
con nombre/topic correctos, `/login` propaga UPDATE a ambos usuarios, imagen
pública llega con SCRIBBLE_HEAD+BLOCK y el base64 exacto, audio público llega
con AUDIO_HEAD+BLOCK. 18 suites de tests en verde, clippy limpio.

### Nicks duplicados y largos UTF-16 (emoji/unicode) — IMPLEMENTADO (2026-07-11)

Dos bugs más de la misma tanda de pruebas contra clientes reales:

- **Nicks duplicados no se rechazaban**: ni el login TCP nativo ni el WS
  verificaban si el nick ya estaba en uso por otra sesión conectada — dos
  usuarios podían coexistir con el mismo nombre, dejando ambiguo a quién
  apunta `get_by_name` (PMs, kicks, bans por nick, etc. solo afectaban a una
  de las dos sesiones). Fix: si `ctx.user_pool.get_by_name(nick)` ya
  encuentra un usuario logueado, se rechaza el nuevo login ("Nickname
  already in use") antes de crear el `AresUser`, en `tcp_handler.rs` y
  `web/handler.rs`. Es una paridad *simplificada* de sb0t: sb0t además
  soporta "hijack" cuando el reconectante viene de la misma IP; Astra solo
  rechaza (lo pedido explícitamente), sin ese caso especial.
- **Nicks/mensajes/topics con unicode o emoji rompían el parseo del
  protocolo de texto ib0t** (`ws: login malformado`): todo el esquema de
  largos-declarados (`IDENT:len1,len2,...:val1val2...`) usaba
  `.chars().count()` (valores escalares Unicode), pero el cliente real es
  JavaScript y calcula los largos con `String.length`, que cuenta *code
  units UTF-16* — un emoji o carácter astral (fuera del BMP) cuenta 2, no 1.
  Cualquier nick/mensaje con esos caracteres desalineaba el parseo. Fix:
  nueva `utf16_len()`/`ws_len()` (`s.encode_utf16().count()`) reemplazando
  `.chars().count()` en TODOS los puntos donde se declaran o parsean largos:
  `crates/web/src/protocol.rs` (`clen`, `build_with_lens`,
  `parse_lens_args` — reescrito para avanzar por code units UTF-16, no
  chars, y detectar cortes a mitad de un surrogate pair), y
  `crates/server-core/src/user_pool.rs` (`send_pvt`/`send_public`/
  `send_emote`/`print`), y 3 puntos en `crates/commands/src/lib.rs`
  (mensajes `UPDATE`/`PART`/`TOPIC`).
- **De paso, se corrigió la confusión `PART` vs `OFFLINE`**: al salir de la
  sala, Astra mandaba `OFFLINE:` (el ident real de sb0t para "el
  destinatario de tu PM no está conectado") en vez de `PART:` (el ident real
  de "un usuario salió de la sala", que el cliente usa para mostrar "X ha
  salido" y borrarlo de su lista). Nuevo `build_part()` en `protocol.rs`
  (se mantiene `build_offline()` intacto para su uso real); corregido en
  `ws_outbound.rs` (`translate_broadcast`) y `commands/lib.rs`
  (`force_part_user`).

Verificado E2E: login con nick `✮ ℓυηα ❥luna💖✨` (emoji + caracteres
astrales) exitoso; segundo login con el mismo nick rechazado; al
desconectarse un usuario, el resto recibe `PART:` (no `OFFLINE:`). 18 suites
en verde, clippy limpio.

### Niveles de permiso configurables por comando + `/help` filtrado — IMPLEMENTADO (2026-07-11)

Reportado también contra el cliente real: `#help`/`/help` mostraba
literalmente **todos** los comandos sin filtrar por nivel del usuario, y
además preguntaba si los comandos se gatean por nivel al ejecutarse (sí,
pero estaba hardcodeado a 3 umbrales: `can_edit_topic` = Moderator+,
`has_level(Admin)`, `has_level(Owner)`) y si eso era configurable como en
sb0t (sb0t sí lo permite, vía `[CommandLevel]` + registro de Windows +
GUI `gui/CommandManager.cs`).

Implementado el equivalente sin GUI:

- **`server_core::command_levels::CommandLevelManager`**: tabla
  `DEFAULT_COMMAND_LEVELS` con el nivel default de ~141 nombres de comando
  (incluyendo cada alias por separado, ej. `kick`/`kill`), reflejando
  exactamente el gate que cada handler ya tenía hardcodeado (para no
  cambiar comportamiento por defecto). Overrides persistidos en SQLite
  (tabla `command_levels`), con `get`/`set`/`reset`/`list`. Nuevo campo
  `AppContext::command_levels`.
- **Gate centralizado en `dispatch_builtin`** (`crates/commands/src/lib.rs`):
  antes del `match cmd.as_str()`, si el comando está en la tabla y el
  usuario no alcanza el nivel requerido (efectivo = override o default), se
  rechaza sin llegar al handler. Los checks internos de cada handler
  (`can_edit_topic`, `has_level`, `require_host`) se mantienen intactos como
  defensa en profundidad — ahora son redundantes en el camino feliz, pero no
  estorban.
- **`/help` filtrado por nivel**: cada línea de `DEFAULT_HELP_LINES` se
  mapea a su nombre de comando y se omite si el nivel del usuario no
  alcanza el requerido.
- **`/cmdlevel <comando> [nivel|reset]`** (Owner-only — a propósito más
  restrictivo que Admin, porque permite reconfigurar los demás gates y un
  Admin no debe poder auto-escalarse): sin nivel, muestra el efectivo y el
  default; con `reset`, revierte al default; si no, lo persiste.
- **Fix colateral necesario**: `has_level()` ahora trata a todo usuario
  conectado como mínimo `Regular`, aunque su `level` en memoria siga en
  `Anonymous` (el default real de `AresUser::new` — ningún path de login
  seteaba `Regular` explícitamente). Antes no importaba porque ningún gate
  comparaba contra exactamente `Regular`; con comandos de autoservicio
  (`/topic`, `/whois`, `/users`, etc.) ahora gateados a `Regular`, sin este
  piso quedaban inaccesibles para cualquier usuario sin nivel explícito.

**Nombres de comando**: ya casi todos los nombres originales de sb0t existen
como alias en Astra (sección "Aliases con los nombres originales de sb0t").
Lo que sigue diferente, documentado pero **no cambiado por defecto** (para
no alterar comportamiento sin pedido explícito — reconfigurable con
`/cmdlevel`):

- **`/whois` no tiene ningún gate en Astra** (cualquiera puede ver IP/GUID
  de cualquier usuario), mientras sb0t lo requiere Moderator+. Vale la pena
  revisar si esto es intencional.
- **Varios comandos Host-only en sb0t están en Admin (o Moderator) en
  Astra**, porque Astra no tiene un tier "Host" separado de Owner: todo el
  subsistema de greets (`greets`/`addgreet`/.../`greetmsg`/...), `url`,
  `customnames`, `history`, `lastseen`, `mtimeout`, `idle`,
  `listquarantined`/`unquarantine`, `clearbans`/`cbans`, `link`/`unlink`
  (estos dos sí quedaron en Owner).
- **`ban`/`unban`/`banstats`/`oldname`/`trace` son Moderator+ en Astra pero
  Administrator+ en sb0t.**
- **`/cname`/`customname` tienen semántica distinta**: en Astra es
  autoservicio (cada usuario setea SU PROPIO nombre custom, sin gate); en
  sb0t `customname` es un comando de Moderator+ que asigna un nombre custom
  A OTRO usuario. No se tocó por ser un cambio de diseño, no un bug.

Verificado E2E: un usuario Regular no ve `/ban` en `/help` y lo recibe
rechazado ("Access denied. Moderator+ required."); el Owner ve la lista
completa incluyendo `/cmdlevel`; `/cmdlevel ban admin` sube el requisito en
caliente y un Moderator queda bloqueado hasta el `reset`. 18 suites en
verde (130 tests en `astra-commands`, 5 nuevos en `command_levels`),
clippy limpio.

### Diferido (fuera de alcance)

- **File search/sharing**: `ClientBrowse` se relaya al link, pero `ClientSearch`/
  `AddShare`/`RemShare` no se sirven (feature P2P grande, fuera de alcance de un
  servidor de chat). SHARING se sigue anunciando por el browse.
- **PM_SCRIBBLE/PM_AUDIO**: implementados en el server (ver arriba) pero sin
  E2E dedicado (el reporte era sobre el chat público); la lógica es la misma
  que la pública con destinatario único, debería funcionar igual.
