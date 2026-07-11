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

### `/cname` removido (no existe en sb0t) + avatar/personal message rotos para clientes Ares nativos — IMPLEMENTADO (2026-07-11)

Dos hallazgos más probando con un cliente Ares real (nativo TCP) además del
cliente web inbizio:

- **`/cname` no es un comando de sb0t** (correcto, señalado por el dueño del
  proyecto): era un alias inventado sin equivalente real. sb0t solo tiene
  `customname`/`uncustomname` (`[CommandLevel("customname", ILevel.Moderator)]`
  en `Eval.cs`) — ya existían como alias de `handle_cname` en Astra. Se quitó
  el arm `"cname"` del dispatcher (y de `DEFAULT_HELP_LINES`,
  `is_user_command`, `DEFAULT_COMMAND_LEVELS`), dejando `customname`/
  `uncustomname` como los únicos nombres reales. **Nota de diseño no
  tocada**: en sb0t `customname` tiene una semántica dual — auto-asignación
  (gateada a `nivel > Regular` o el flag de sala `general`) O asignar el
  nombre custom a OTRO usuario (gateada a Moderator+); Astra solo implementa
  la auto-asignación, sin gate. Es un cambio de diseño más grande, no
  aplicado por ahora.
- **Avatar y personal message nunca llegaban a clientes Ares nativos**
  (reportado como "¿los usuarios TCP reciben avatar/pmsg?"). Comparado
  contra `TCPProcessor.cs`/`AresClient.cs` de sb0t, se encontraron 3 gaps
  reales:
  1. `send_initial_state` (tcp_handler.rs, lo que se manda a un cliente Ares
     recién conectado) nunca incluía el avatar ni el personal message de los
     usuarios YA conectados — solo el item de userlist (sin esos campos, que
     van en paquetes `Avatar`/`PersonalMessage` separados en el protocolo
     real). Un cliente nativo que se unía tarde nunca veía el avatar/pmsg de
     nadie. Fix: tras cada `USERLIST` item, si el otro usuario tiene avatar u
     pmsg, se le manda también (paridad `TCPProcessor.Login`, líneas
     908-920 de sb0t).
  2. El handler `TcpMsg::Avatar` (avatar subido por un cliente Ares) NUNCA
     difundía el cambio a nadie — solo lo guardaba en `user.avatar` y
     notificaba a scripts. Nadie más (ni Ares ni web) veía nunca un avatar
     actualizado en vivo. Fix: ahora difunde por `broadcast_to_room`
     (paridad del setter `AresClient.Avatar`, que manda a `AUsers` Y
     `WUsers`). También: payloads `< 10 bytes` ahora se tratan como "avatar
     limpiado" (antes se guardaban tal cual, incluso vacíos).
  3. El avatar subido por un usuario WEB (`handle_ws_avatar`) solo se
     reanunciaba a otros clientes web/inbizier — nunca llegaba a los
     clientes Ares nativos. Fix: ahora también manda el paquete binario
     `Avatar` a los peers TCP de la vroom.
  - Nuevos builders reutilizables en `server_core::outbound`:
    `build_avatar_c`/`build_avatar_cleared_c`/`build_personal_message_c`.
  - Nuevo caso `TcpMsg::Avatar` y `TcpMsg::PersonalMessage` en
    `ws_outbound::translate_broadcast` (traduce el broadcast binario TCP al
    `AVATAR:`/`PERSMSG:` de texto para clientes web inbizier — antes
    `PersonalMessage` tampoco tenía traducción, así que un cambio de pmsg
    originado en un cliente Ares nunca llegaba a los clientes web).

Verificado E2E con clientes Ares nativos crudos (framing binario real) +
cliente WS inbizier: un usuario nativo que se conecta tarde recibe el
avatar/pmsg de un usuario nativo ya conectado; un update de avatar en vivo
(nativo→nativo, nativo→web, web→nativo) llega a todos sin reconectar. 18
suites en verde, clippy limpio.

### Panel de administración: paridad con las pantallas de sb0t — IMPLEMENTADO (2026-07-11)

Pedido explícito: el panel web (`/admin`) solo tenía Dashboard/Users/Bans/
Room/Filters/Accounts/Settings(TOML crudo)/Console, mientras que la GUI WPF
de sb0t (`gui/MainWindow.xaml`) tenía 7 tabs (Main, Admin, Linking, Advanced,
Proxy, Avatars, Plugins). Se pidió paridad completa, incluyendo dos
funcionalidades que **no existían en absoluto** en Astra (proxy trust,
avatares de sala/default) — confirmado explícitamente "todo junto, incluyendo
backend nuevo". "Plugins/Extensions" y "Start/stop server" quedaron fuera:
no aplican a la arquitectura de Astra (scripting JS embebido en archivos, no
un instalador de plugins; no es una app de escritorio con botón de arranque).

Seis pestañas nuevas, cada una con su propio nivel de "vivo" vs "requiere
restart":

- **Command Levels** (vivo, sin backend nuevo — ya existía de la tanda
  anterior): tabla de todos los comandos gestionados por
  `CommandLevelManager` con su nivel efectivo y un `<select>`/botón reset
  por fila que reusa `/cmdlevel` (sin rutas HTTP nuevas). `state_json` ahora
  incluye `commandLevels`.
- **Server / Linking / Advanced** (requieren restart, igual que la pestaña
  "Settings" de TOML crudo ya existente — son campos de `Settings`, cargado
  una sola vez al arrancar): vistas estructuradas (inputs, no textarea) del
  mismo `Settings`, vía dos funciones nuevas en `admin.rs`
  (`settings_json`/`write_settings_json`, JSON en vez de TOML pero mismo
  archivo/mismo `Settings::save`) y dos rutas nuevas
  (`GET`/`POST /admin/config`). La pestaña TOML cruda queda intacta como
  escape hatch — ambas leen/escriben el mismo archivo, así que son
  consistentes entre sí.
- **Proxy trust** (backend nuevo, 100% vivo — sin restart, paridad con sb0t
  que también lee su lista en caliente desde el registro): nueva tabla
  SQLite `trusted_proxies` + manager `server_core::proxy_trust::
  TrustedProxyManager` (mismo patrón que `RoomFlags`), nuevo campo
  `AppContext::trusted_proxies`. Confirmado en el C# de referencia
  (`ib0tClient.ApplyForwardedIP`, `core/ib0t/ib0tClient.cs:949-970`) que el
  trust de `X-Forwarded-For`/`X-Real-IP` **solo aplica al handshake WS/
  ib0t** — el TCP Ares nativo no tiene headers HTTP y nunca se toca. Nueva
  `resolve_client_ip` en `crates/web/src/ws.rs` (peer directo debe estar en
  la lista, o ser loopback — siempre confiable — para que los headers
  cuenten; `X-Real-IP` gana sobre el primer valor de `X-Forwarded-For`),
  enhebrada como parámetro nuevo (`resolved_ip: IpAddr`) por
  `handle_connection`/`ws_handshake_login` en `handler.rs`, reemplazando el
  antiguo `let external_ip = peer.ip();`. Nuevas rutas
  `/admin/proxy/add`/`/admin/proxy/remove`.
- **Avatares** (backend nuevo, mayormente vivo): dos campos nuevos en
  `AppContext` (`server_avatar`/`default_avatar`, cargados al arrancar desde
  `<data_dir>/avatars/{server,default}` si existen — Astra no tiene GUI que
  los persista como sb0t, así que se cargan de archivo en vez de en el
  registro/GUI). Confirmado en `core/Avatars.cs` de sb0t: el avatar de sala
  se manda a todo Ares nativo en cada login (`TCPProcessor.cs:902/959`) y se
  empuja en vivo a todos cuando se actualiza (mismo patrón que ya usábamos
  para avatares de usuario); el avatar default es un timer que cada ~2s
  asigna el avatar default a cualquier Ares nativo con >10s conectado sin
  haber mandado el suyo (`Avatars.CheckAvatars`), solo para clientes nativos
  (no web). Portado: nuevo campo `AresUser::avatar_received` (reusa
  `join_time`, que ya existía, para el chequeo de los 10s — no hizo falta
  agregar otro timestamp), nueva task periódica en `main.rs` (mismo patrón
  que el FastPing existente), `send_initial_state` (tcp_handler.rs) y el
  estado inicial WS (`handler.rs`) ahora mandan el avatar del bot si hay
  uno configurado. Simplificación deliberada: sin reescalar a 48x48/JPEG-69
  como sb0t (evita sumar la dependencia `image`); en su lugar, un tope de
  64 KiB en la subida. Nuevas rutas `POST /admin/avatar` y
  `GET /admin/avatar/{server,default}` (bytes crudos, Content-Type
  adivinado por magic bytes).

Verificado E2E contra un binario real: cambiar un nivel de comando desde el
panel se refleja al instante; `GET`/`POST /admin/config` mantiene
consistencia con el editor TOML crudo; agregar una IP a la lista de proxies
confiables y mandar `X-Forwarded-For` con IPs distintas en conexiones WS
consecutivas desde el mismo loopback demuestra que cada una se trata como
un cliente distinto (sin choque de anti-join-flood); subir un avatar de
sala se ve reflejado tanto en el `USERINFO` del bot para clientes web como
en un paquete `Avatar` binario para un cliente Ares nativo crudo en su
login; un cliente nativo que no manda su propio avatar recibe el avatar
default a los ~10s y se difunde en vivo. 18 suites en verde (133 tests en
`server-core`, +5 nuevos: 3 de `TrustedProxyManager` + validaciones ya
cubiertas), clippy limpio.

### Nick "pegado" tras una desconexión — hijack por misma IP (2026-07-11) — IMPLEMENTADO

Reportado en producción: un usuario perdió la conexión a internet (el
server nunca se enteró — la sesión murió sin un FIN/RST limpio, algo
común con desconexiones reales) y al volver a intentar entrar con el mismo
nick, quedaba rechazado con "Nickname already in use" indefinidamente
(hasta que la sesión vieja expirara sola, hasta 30 min con el
`idle_timeout_secs` actual). Esto era una consecuencia directa de la
simplificación deliberada del fix de nicks duplicados de una tanda
anterior (rechazar siempre, documentado ahí mismo como más simple que sb0t
pero con este trade-off).

Portado el comportamiento real de sb0t (`TCPProcessor.cs:738-756`): al
encontrar un nick ya en uso, en vez de rechazar siempre, se compara la IP
externa de la sesión existente con la de la conexión nueva. Si es la
**misma IP**, es una reconexión — se saca la sesión vieja (`force_part_user`,
ahora `pub` en `astra_commands`, reusado tal cual: mismo camino que
`/kick`) y se deja pasar la nueva. Si es una IP **distinta**, se sigue
rechazando como antes ("name in use" real, no es la misma persona).
Aplicado en ambos paths de login (`tcp_handler.rs` y `web/handler.rs`) —
funciona cross-protocol también (ej. reconectar por WS tras perder una
sesión TCP desde la misma IP).

Verificado E2E con clientes reales (nativo Ares crudo y WS): una segunda
conexión con el mismo nick desde la misma IP recibe el login exitoso
(hijack) en vez del rechazo; con una IP distinta, se sigue rechazando. 18
suites en verde, clippy limpio.

### WS: fuga de conexiones/tasks en TODO desconexión (rechazo o normal) — IMPLEMENTADO (2026-07-11)

Investigando un reporte de "un usuario entra a cada rato, pero en los logs
solo aparece una vez" (visto desde otro cliente como muchos "X has joined"
repetidos) se encontró un bug de fondo mucho más grave que el síntoma
reportado: **ninguna desconexión WS cerraba realmente la conexión**, ni la
de un login rechazado (ban/join-flood/nick duplicado) ni la de un usuario
que se va normalmente.

Causa raíz en `crates/web/src/handler.rs::handle_connection`: existían DOS
canales — uno de `Bytes` (`tx`/`rx`, para "mensajes de error binarios") y
uno de `String` (`ws_text_tx`/`ws_text_rx`, el que la write task realmente
drena para escribir frames de texto). El canal de `Bytes` nunca tenía un
lector (`rx` se creaba y no se leía en ningún lado), así que:

1. Los 3 mensajes de rechazo del handshake (`ERROR:You are banned...`,
   `ERROR:Joining too quickly`, `ERROR:Nickname already in use`) se
   mandaban por el canal muerto — el cliente **nunca los recibía**, y su
   conexión simplemente se cortaba sin ninguna explicación.
2. Peor: la limpieza en los 4 puntos de salida (3 del handshake + 1 del
   loop principal, éste último tras una desconexión NORMAL) hacía
   `drop(tx)` (el canal muerto) en vez de `drop(ws_text_tx)` (el canal
   real que la write task espera con `recv()`). Como el canal real nunca
   se dropeaba, `write_task.await` quedaba esperando para siempre — cada
   desconexión (rechazada O normal) filtraba la write task Y la propia
   task de `handle_connection` (bloqueada en ese `.await`) indefinidamente,
   sin cerrar jamás el socket subyacente (ni `read_half` ni `write_half` se
   dropeaban nunca). En un server de larga duración esto acumula sockets
   medio-cerrados y tasks colgadas sin límite — memoria/fds crecientes, y
   consistente con el síntoma original de "nicks pegados" (una sesión
   vieja podía nunca cerrarse a nivel de socket, aunque el usuario ya
   estuviera desconectado hace rato).

Fix: se eliminó el canal de `Bytes` (dead code — nada más lo necesitaba;
`user.sender` para usuarios web queda en `None`, correcto porque los
usuarios web solo usan `ws_text_sender`); los 3 mensajes de rechazo ahora
se mandan por `ws_text_tx` (el canal real); los 4 puntos de limpieza ahora
dropean `ws_text_tx` (no `tx`), lo que efectivamente cierra el canal, deja
salir el loop de la write task, y dropea `write_half` (cerrando el socket
de verdad). De paso: `write_close_frame` estaba importado pero nunca
llamado (otro cabo suelto) — ahora la write task manda un close frame de
verdad antes de terminar, para que clientes bien portados distingan un
cierre limpio de un corte abrupto (que muchos clientes reintentan de forma
agresiva — posible explicación adicional del síntoma de reconexión
repetida).

Verificado E2E: un login rechazado por nick duplicado (IP distinta, sin
hijack) ahora SÍ le llega el mensaje `ERROR:...` al cliente, y su socket se
cierra solo (antes quedaba colgado); confirmado en los logs que
`handle_connection` llega a su `info!("ws desconectado...")` final tanto
para la sesión rechazada como para una desconexión normal (antes, con el
bug, esa línea nunca se alcanzaba — la función quedaba bloqueada para
siempre en `write_task.await`). 18 suites en verde, clippy limpio.

### Auditoría sistemática sb0t vs Astra: scripting management, autologin por IP, filtros multi-línea — IMPLEMENTADO (2026-07-11)

El usuario pidió una auditoría sistemática de qué comandos de sb0t faltan en
Astra. Un agente de investigación confirmó tres gaps reales (más los ya
reportados sueltos antes), y se implementaron los tres por completo:

**1. Comandos de gestión de scripts** (no existían en absoluto):
`/listscripts`, `/loadscript <name>`, `/killscript <name>`,
`/livescripts`, `/downloadscript <owner/repo>`, `/errors [on|off]`.

- Plumbing nuevo: `AppContext` no puede depender de `astra_scripting`
  (circular), así que se usó el mismo patrón de closures inyectadas que
  `crates/udp` (`RoomInfoFn`/`UserCountFn`) — nuevo
  `AppContext::scripting_hooks: RwLock<Option<ScriptingHooks>>`, seteado
  en `main.rs` tras `start_in_thread()`. Nuevas variantes
  `ScriptRequest::ListScripts/LoadScript/KillScript` +
  `ScriptHandle::list_scripts/load_script/kill_script` (mismo patrón
  sync-request-con-reply que ya usaban los hooks `*Before`).
- `/livescripts`/`/downloadscript`: paridad con `LiveScript.cs` de sb0t —
  buscan repos de GitHub con el topic `areschatscript`, y descargan+cargan
  el último release. **Simplificación deliberada**: sb0t renombra el
  directorio raíz extraído a `<filename>.js` (su modelo permite que un
  "script" sea una carpeta); Astra busca el primer `.js` dentro del zip y
  lo carga como archivo individual, consistente con su modelo de scripts.
  Nuevo campo `Settings.live_scripts_endpoint` (default
  `https://api.github.com`, paridad con el default real de sb0t).
- `/errors [on|off]`: reusa el patrón `Subscription`/`handle_subscription`
  ya existente (mismo que `/vspy`, `/ipsend`, etc.) — nuevo
  `AresUser::sub_errors`. A diferencia de sb0t (~90 call sites de
  `ErrorDispatcher.SendError`), Astra centraliza los errores de script en
  solo 2 lugares (`manager.rs::load_source`/`dispatch`), así que alcanzó
  con notificar ahí.
- **Bug real encontrado al probar `/killscript` en vivo**: la primera vez
  que se descargaba CUALQUIER script (incluso uno cargado al arrancar el
  server) el thread del scripting manager **paniqueaba** (`attempt to
  subtract with overflow` dentro de `boa_engine`). Causa raíz:
  `start_in_thread()` cargaba los scripts (`load_all_inner()`, creando sus
  `boa_engine::Context`) ANTES de mover el manager al thread dedicado — los
  Context se creaban en el thread llamante pero se destruían en el thread
  dedicado, y `boa_engine` mantiene un contador `thread_local`
  (`CANNOT_BLOCK_COUNTER`) que se incrementa al crear un Context y se
  decrementa al dropearlo: crear en un thread y destruir en otro descuenta
  un contador que nunca se incrementó ahí. Este bug existía desde que se
  construyó el scripting subsystem, pero nada en producción llamaba
  `unload()`/`reload()` hasta `/killscript`. Fix: mover
  `load_all_inner()` a DENTRO del thread dedicado (después de
  `Box::from_raw`), para que TODA la carga de scripts (inicial y en
  caliente) pase por el mismo thread consistentemente.
- **Bug adicional encontrado**: el autologin (tanto el existente por
  cuenta/GUID como el nuevo por IP) nunca se ejecutaba para usuarios
  web/WS — `dispatch_autologin` solo se llamaba desde el path TCP nativo,
  gateado detrás del opcode `ClientAutologin` que el cliente debe mandar
  explícitamente (el protocolo de texto ib0t no tiene un opcode
  equivalente). Fix: en `web/handler.rs`, llamar `dispatch_autologin`
  automáticamente en cada join web, antes de mandar el estado inicial
  (paridad más fiel del `Joined()` incondicional de sb0t). El path TCP
  nativo queda como estaba (opt-in vía el opcode, sin cambios).

**2. `/addautologin`/`/remautologin`/`/autologins`** (auto-nivel por
IP+GUID sin cuenta, paridad `commands/AutoLogin.cs` de sb0t): nuevo
`server_core::ip_autologin::IpAutologinManager` (tabla `ip_autologins`,
mismo patrón que `RoomFlags`/`TrustedProxyManager`) con el matching de dos
niveles de sb0t (GUID + mismos primeros 2 octetos de IP, o IP exacta,
self-healing en ambos casos). Restringido a Moderator/Admin — nunca Owner
(paridad del rango `byte 1-3` de sb0t, que deliberadamente no permite
auto-otorgar el nivel más alto vía reconocimiento de IP). **Corrección de
nombre necesaria**: `"autologins"` estaba aliaseado a `listpasswords`
(cuentas registradas) — se desaliaseó, ya que en sb0t `/autologins` lista
las entradas de IP-autologin, un concepto distinto.

**3. `/addline`/`/remline`/`/viewfilter`** (líneas de respuesta múltiples
en un filtro, paridad `WordFilter.cs` de sb0t): nueva variante
`FilterAction::Announce` — un filtro que, a diferencia de Block/Kick/Ban,
**no bloquea el mensaje**: además de dejarlo pasar, difunde una o más
líneas enlatadas con placeholders `+n`/`+ip`/`+r` (mini sistema de
auto-respuesta por keyword). `WordFilterManager` gana un cache paralelo de
líneas por pattern (tabla `word_filter_lines`) + `add_line`/`remove_line`
(con borrado en cascada del filtro si era la última línea, paridad sb0t)/
`view`/`check_announce` (separado de `check()`, que sigue siendo solo para
censura). Nuevo hook en `handle_public` (TCP y WS): si `check()` no
matcheó, se prueba `check_announce()` y se difunden las líneas sin
bloquear el mensaje original. **Corrección de nombre necesaria**:
`"viewfilter"` estaba aliaseado a `wordfilters` (lista plana) — se
desaliaseó, ya que en sb0t `/viewfilter <índice>` es el visor per-entrada
de líneas, un comando distinto de `/wordfilters` (que ahora además
muestra el índice de cada filtro, necesario para poder referenciarlos
desde `/addline`/`/remline`/`/viewfilter`).

Verificado E2E contra un binario real conectado a la API real de GitHub:
`/livescripts` devolvió resultados reales (repos con el topic
`areschatscript`); `/downloadscript owner/repo` descargó, extrajo y cargó
un script real sin crashear; `/killscript`+`/loadscript` en ciclo repetido
confirmaron el fix del panic; `/addautologin` + reconexión sin login
restauró el nivel automáticamente (verificado tanto el mensaje de sistema
como el `UPDATE` de nivel y el propio `USERINFO` del usuario); un filtro
`announce` con 2 líneas disparó ambas con `+n`/`+ip`/`+r` sustituidos SIN
bloquear el mensaje original, y `/remline` en cascada borró el filtro al
vaciarse. 18 suites en verde (149 tests en `astra-commands`, 16 en
`word_filter`, 7 nuevos en `ip_autologin`), clippy limpio.

### "Has joined" fantasma en clientes web por `ClientUpdateStatus` mal manejado — IMPLEMENTADO (2026-07-11)

Reportado en producción por el usuario con una captura real: dos bots cb0t
(`Host`, `ARANA`, ambos `cbot=true` en el login) aparecían repitiendo
"has joined" en la sala decenas de veces sin ningún desconecte real de por
medio (los logs mostraban un único `LOGIN OK` para cada uno). Esto NO tenía
relación con el hijack-por-IP ni con la fuga de canales WS arregladas
antes en esta misma sesión (ver secciones arriba) — era un bug distinto,
específico de clientes TCP nativos.

Causa raíz, confirmada leyendo `TCPProcessor.cs` de sb0t: el opcode
`MSG_CHAT_CLIENT_UPDATE_STATUS` (4) — que los clientes cb0t mandan
periódicamente como refresh de su estado de compartición de archivos
(file_count/browsable/etc, no tiene nada que ver con join/leave) — en sb0t
se responde ÚNICAMENTE al mismo cliente que lo mandó
(`client.SendPacket(TCPOutbound.UpdateUserStatus(client, client))`, opcode
`MSG_CHAT_SERVER_UPDATE_USER_STATUS` = 5). Astra, en cambio, reusaba el
opcode de JOIN (`build_join_or_userlist_c`, `ServerJoin`/
`ServerChannelUserList`) y lo DIFUNDÍA A TODA LA SALA
(`tcp_handler.rs::dispatch_message`, arm `TcpMsg::ClientUpdateStatus`).
Como `ws_outbound::translate_broadcast` traduce cualquier paquete con esos
dos opcodes a un mensaje de texto "ha entrado" para clientes web sin
distinguir "join real" de "refresh reusando el mismo opcode", cada
`ClientUpdateStatus` periódico de un bot cb0t generaba un "X has joined"
fantasma en todos los clientes web de la sala, indefinidamente, sin que el
usuario se moviera.

Fix: agregado `build_update_user_status_c` en
`server-core/src/outbound.rs` (paridad exacta de `TCPOutbound.cs
UpdateUserStatus`: name, file_count, browsable, node_ip, node_port,
external_ip —oculta a `0.0.0.0` si el cliente no es Ares nativo, paridad
`client.Ares`—, level, age, sex, country, region; opcode
`ServerUpdateUserStatus`, ya existía en `proto_ares::TcpMsg` pero nunca se
usaba). El handler de `ClientUpdateStatus` ahora hace
`user.send(outbound::build_update_user_status_c(user, user.ares_crypto))`
(reply directo al socket del propio cliente) en vez de `broadcast_to_room`,
y se quitó el `ctx.publish_link_event(LinkEvent::UserUpdated {...})`
asociado (sb0t tampoco lo propaga a links; era un efecto colateral de la
implementación anterior, no algo real de sb0t).

De paso, mientras se investigaba este reporte con logs reales del server en
producción, se encontró un segundo bug menor: el arm `_` (paquete
desconocido/no-login como primer paquete, ej. `LinkProto` de un peer que
prueba el protocolo de link sin éxito) en `tcp_handler.rs::process_handshake`
era la ÚNICA rama de rechazo que NO llamaba a
`ctx.security.failed_logins.record_failure(peer.ip())` — permitía que una
IP reintentara indefinidamente sin nunca activar el ban automático de CAPA
5 (se observó una IP externa reintentando cada ~10s durante 40000+
segundos de uptime sin ser jamás baneada). Corregido agregando la llamada
faltante, igual que las otras 5 ramas de rechazo.

Verificado E2E con un binario real: cliente TCP nativo logueado + 5
`ClientUpdateStatus` seguidos → 5 respuestas de opcode `5`
(`ServerUpdateUserStatus`) recibidas únicamente por el propio cliente,
con el payload byte-a-byte coincidente con el formato de sb0t (node_ip,
node_port, IP oculta por ser `cbot`, level/age/sex/country/region). Antes
del fix, esos mismos 5 paquetes habrían sido opcode `20`/`30` difundidos a
toda la sala. `cargo build/test/clippy --workspace` en verde.

### Rediseño UX del panel de administración (más intuitivo, no-técnico, móvil) — IMPLEMENTADO (2026-07-11)

Pedido del usuario: "ponele más cariño al panel admin, tiene que ser más
intuitivo para usuarios que no son técnicos y una mejor experiencia de
usuario". El panel anterior era funcional pero denso, en inglés, con tablas
apretadas de 13px que se ven mal en el teléfono (la mayoría de los admins lo
usan desde el móvil — de hecho todos los reportes de bugs de esta sesión
vinieron con capturas de celular). Rediseño completo de `ADMIN_HTML` en
`crates/web/src/panel.rs` (la capa de presentación; el contrato con el
backend —endpoints `/admin/*`, campos del STATE, comandos slash— quedó
idéntico, no se tocó nada de `admin.rs`/`ws.rs`):

- **Todo en español**, con lenguaje para no-técnicos: "Expulsar"/"Banear"/
  "Silenciar" en vez de kick/ban/muzzle, "rangos" en vez de levels, cada
  sección con un texto explicativo de qué hace en criollo (ej. las room
  flags ahora tienen nombre + descripción legible: `sharefiles` →
  "Vigilar archivos / Monitorea la compartición de archivos").
- **Mobile-first**: navegación agrupada en 4 secciones (Principal /
  Moderación / Sala / Avanzado) dentro de un cajón lateral (drawer) que en
  desktop queda fijo como sidebar y en móvil se abre con un botón ☰ +
  backdrop. Áreas táctiles de 40px+, `env(safe-area-inset-*)` para el
  notch.
- **Tarjetas en vez de tablas** para las listas interactivas (usuarios,
  con badge de rango + acciones táctiles grandes); toggles tipo switch para
  las room flags (aplican al instante); tiles para las estadísticas del
  inicio.
- **Feedback claro**: notificaciones tipo toast en cada acción (en vez del
  texto inline chiquito de antes), confirmaciones en español para lo
  destructivo (banear, vaciar baneos).
- **Color de marca**: acento naranja para matchear la app de inbizio/Astra
  que se ve en las capturas del cliente móvil.
- Detalles UX: buscador de comandos en la pestaña de Permisos (159
  comandos), las pestañas con formularios (Servidor/Enlace/Seguridad/
  Config/Permisos/Proxies/Avatares) quedan excluidas del auto-refresh de
  5s para no borrar lo que el admin está escribiendo.

Verificado E2E contra un binario real: panel servido OK (48KB), sintaxis JS
validada con `node --check`, las 15 funciones `render*` ejecutadas en un DOM
simulado contra un STATE realista sin un solo error de referencia, y el
flujo de endpoints (login → token → state → comando `/topic` reflejado en el
state) funcionando. `cargo build -p astra-web` limpio.

### Panel de administración bilingüe (español / inglés) — IMPLEMENTADO (2026-07-11)

Continuación del rediseño UX: el usuario pidió hacer el panel `/admin`
bilingüe ES/EN (solo el panel, no el cliente de chat de prueba ni los
mensajes del backend, que siguen viniendo en inglés desde el crate
`commands`). Refactor de `ADMIN_HTML` en `crates/web/src/panel.rs`:

- Diccionario `I18N = { es, en }` (~210 claves por idioma, paridad exacta
  verificada) + función `t(key, ...args)` con interpolación `{0}`/`{1}`
  para conteos y nombres. Los mapas de rangos (`LVL`), acciones de filtro
  (`ACT`) y room flags (`FLAG`, label + descripción) también son
  bilingües.
- Selector de idioma: botón `ES`/`EN` en el header (y un link en la
  pantalla de login), persistido en `sessionStorage`. Al cambiar,
  `setLang()` reconstruye la navegación, re-renderiza la pestaña actual y
  actualiza el header sin recargar.
- Detección inicial por `navigator.language` (empieza en inglés si el
  navegador está en inglés, español en cualquier otro caso), overrideable
  y recordado.
- `applyChrome()` traduce también los textos "estáticos" que viven fuera
  de las funciones `render*` (login, títulos de botones del header).

Verificado: `cargo build -p astra-web` limpio; panel servido crece a ~67KB;
`node --check` OK; las 15 funciones `render*` ejecutadas en un DOM simulado
en **ambos** idiomas (30 renders) sin un solo error ni `undefined` filtrado;
paridad de claves ES/EN comprobada programáticamente (210 = 210, cero
faltantes); login → state contra un binario real OK.

### MOTD real + textos del sistema editables ("templates") — IMPLEMENTADO (2026-07-11)

Pedido del usuario: dos textareas nuevas en el panel para editar el "MOTD" y
el "template". Tras confirmar la semántica sb0t con el usuario (respondió por
AskUserQuestion): **MOTD = mensaje multilínea mostrado al entrar** (real, no
alias del topic), y **template = los textos del sistema estilo sb0t**
(`commands/Template.cs`), hecho **por fases** (el usuario lo aceptó
explícitamente).

**MOTD (completo):**
- Nuevo `server-core/src/motd.rs` (`MotdManager`): texto multilínea
  persistido en una tabla `kv` nueva (clave `motd`), con `rendered_lines()`
  que sustituye `+n`/`+rn`/`+ip`/`+uc` y descarta líneas vacías. A diferencia
  de sb0t, texto plano (sin tags `[youtube=]`/`[image=]`).
- Se envía al entrar, línea por línea como PM del bot, tras el greet, en TCP
  nativo (`tcp_handler.rs::send_motd`) y web (`handler.rs::send_motd_ws`).
- `/motd` (sin args) ahora muestra el MOTD real (antes estaba aliaseado al
  topic — se corrigió esa conflación); `/motd <texto>` lo setea (una línea;
  el editor multilínea es el panel). Ya no toca el topic.

**Template (Fase 1 — moderación y control de acceso):**
- Nuevo `server-core/src/templates.rs` (`TemplateManager`): catálogo de 17
  claves con default en inglés (`TEMPLATE_DEFAULTS`), overrides persistidos
  en la tabla `message_templates`, `render(key, subs)` con sustitución
  `+n`/`+a`/`+l`/`+i`, y `apply_bulk`/`export_text` para el editor `key =
  valor` del panel. Setear un texto igual al default borra el override.
- Se rutearon por el manager los mensajes de los handlers de moderación:
  `kick`, `ban`/`unban`, `muzzle`/`unmuzzle`, `grant`/`revoke`, más
  "Access denied" (moderator/admin) y "User not found". El resto de los
  ~400 mensajes del server siguen fijos — se documentó como Fase 1 en el
  propio panel; la infra queda lista para sumar claves (agregar entrada a
  `TEMPLATE_DEFAULTS` + usar `templates.get/render` en el call site).
- **Por qué no todo de una**: sb0t difunde a la sala casi toda acción de
  admin y su template tiene ~200 strings; Astra notifica en privado y tiene
  ~409 `send_system_line`, la mayoría errores de uso/resultados que no son
  material de "template". Rutear los 409 de una sola pasada sería un
  refactor gigante y riesgoso.

**Panel:** dos pestañas nuevas — "Mensaje de entrada" (MOTD, en el grupo
Sala) y "Textos del sistema" (en Avanzado) — cada una una textarea, bilingües
(ES/EN, +18 claves i18n nuevas), excluidas del auto-refresh. Rutas
`GET`/`POST /admin/motd` y `/admin/template` en `ws.rs` (protegidas por token
como el resto).

Verificado E2E contra un binario real: `cargo build/test/clippy` en verde
(160 tests en server-core incl. los nuevos de motd/templates, 79 en
commands); panel servido ~71KB, `node --check` OK, las 17 pestañas
renderizadas en ES y EN sin errores (paridad 222=222 claves); roundtrip de
ambos endpoints; y lo importante — un usuario WS recibió el MOTD al entrar
con `+n`/`+rn`/`+uc` sustituidos, y el override de `kick.confirm` cambió el
mensaje real al kickearlo ("Chau Watcher, te fuiste!").

### Diferido (fuera de alcance)

- **File search/sharing**: `ClientBrowse` se relaya al link, pero `ClientSearch`/
  `AddShare`/`RemShare` no se sirven (feature P2P grande, fuera de alcance de un
  servidor de chat). SHARING se sigue anunciando por el browse.
- **PM_SCRIBBLE/PM_AUDIO**: implementados en el server (ver arriba) pero sin
  E2E dedicado (el reporte era sobre el chat público); la lógica es la misma
  que la pública con destinatario único, debería funcionar igual.
