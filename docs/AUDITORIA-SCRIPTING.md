# Auditoría del módulo de Scripting — paridad con sb0t

> Fuente de verdad: `~/Development/C#/sb0t/scripting/` — `JSScript.cs` (stubs de
> eventos), `ServerEvents.cs` (dispatch a JS + semántica de retorno),
> `JSGlobal.cs` (globales), `Objects/JSUser.cs` (prototipo user),
> `Statics/*` (Room/Users/Channels/File/Registry/...), `Instances/*`
> (Avatar/HttpRequest/List/ProxyCheck/Query/Scribble/Sql/Timer/XmlParser).
>
> Lado Astra: `crates/scripting/` — `api.rs` (~95 globales + prelude de
> compatibilidad sb0t), `types.rs` (54 eventos), `manager.rs` (motor Boa,
> hilo dedicado, hooks bloqueantes).
>
> **Nota:** `docs/SCRIPTING-ROADMAP.md` está obsoleto (dice "30% de paridad";
> el estado real es mucho mejor). Este documento lo reemplaza como fuente de
> estado.

## 0. Resumen ejecutivo

> **Estado (2026-07-13): Fases S0-S4 IMPLEMENTADAS** (suite completa verde).
> - **S-A ✅**: onTextBefore/onEmoteBefore/onPMBefore ahora retornan el texto
>   reescrito encadenado entre scripts (string reemplaza; false/null/"" cancela;
>   true/undefined no toca). Call sites TCP y web usan el texto resultante.
> - **S-B ✅**: onJoinCheck (login TCP y web — rechaza con error + Rejected),
>   onFloodBefore (perdona el castigo) y onVroomJoinCheck (wired en /vroom
>   vía closure `AppContext::set_vroom_check` inyectada desde main — mismo
>   patrón que ScriptingHooks).
> - **S-C ✅**: user.sendText/sendEmote difunden público/emote COMO el usuario
>   a su vroom (respetando ignore lists); sendPM/sendHTML quedan al usuario.
> - **S-D ✅**: user.kick/disconnect/ban usan AppContext::force_part_user
>   (PART broadcast Ares+web, movido a server-core) y ban registra en banstats.
> - **S-E ✅**: props captcha/idle/visible/ghost/localEP/linked/leaf
>   + ignores(); setters writable customName/vroom (con PART/JOIN broadcast)/
>   level (con persistencia y UPDATE)/muzzled; `avatar` get/set (base64,
>   sobre Avatar_getForUser/set:avatar) y `font` get (objeto {name,size,
>   color,bold,italic,underline}; set no-op — la fija el cliente); métodos
>   redirect (hashlink), setTopic (a ese usuario), nudge (aprox. texto);
>   stubs honestos setUrl/scribble/restoreAvatar/getASN.
> - **S-F ✅**: onCommand(userobj, command, target, args) — target = JSUser
>   del primer token de args resuelto contra el pool, o null.
> - **S-G ✅**: `@código` en chat público (TCP y web) evalúa JS en el primer
>   script activo con `userobj` preseteado — SOLO Owner, sin difundir.
> - **S-H ✅**: Users.banned() real sobre ctx.bans (con unban()); JSUserFont
>   cubierto por la prop `font`; HttpRequest con `header(nombre, valor)` y
>   POST. Única divergencia deliberada: `Channels.enabled()` fijo en true
>   (los "channels" de Astra son vrooms — siempre disponibles).
>
> **Además:** `docs/SCRIPTING-ROADMAP.md` marcado como obsoleto; los scripts
> de ejemplo (`data/scripts/`) actualizados a las firmas reales (usaban
> `onUserJoin` y `onCommand` de 3 args — este último se habría ROTO con el
> nuevo `target`); nuevo `data/scripts/paridad.js` para verificación manual;
> y un test (`bundled_example_scripts_load_and_run`) que carga cada script de
> `data/scripts/` e **invoca sus handlers verificando el Result**, para que
> un cambio de API no los deje rotos en silencio.

Lo que está **bien** (verificado):

- **Eventos**: los 47 callbacks de sb0t existen en Astra (54 handlers, con 7
  extras propios: onConnect/onDisconnect/onPublic/onEmote/onPrivate/
  onUserList/onUserUpdate/onHttpComplete). `onLoad` corre al cargar.
- **Objeto user en los handlers**: el mecanismo `ArgKind::User` +
  `__mkUser` del prelude entrega objetos user (props vía `__user_get`,
  métodos vía `__user_do`, `toString` = nombre para compat).
- **Globales sb0t**: print, user(), include/includeAll, sendText/sendEmote/
  sendPM (forma de 3 args), byteLength, clrName, escapeUtf, stripColors,
  tickCount, scriptName — todos presentes.
- **Statics**: Room, Users, Channels, Base64, Zip, Hashlink, Entities, File,
  Registry, Spelling, Stats, Link, ScriptInclude, Crypto — mapeados en el
  prelude sobre funciones planas.
- **Extras Astra**: Help_addLine, setTimer/clearTimer/setTimeout, feed de
  errores (`/errors`), descarga de scripts (`/downloadscript`).

Los hallazgos (§1) son **7 puntos concretos** (S-H quedó reducido a stubs
menores tras verificar que las instances sí existen): hooks que no
reescriben texto, métodos del user object con efectos equivocados o
incompletos, y el @eval de chat sin portar.

---

## 1. Hallazgos (ordenados por severidad)

### S-A. Los hooks `*Before` no permiten MODIFICAR el texto (❌ crítico de paridad)

- **sb0t** (`scripting/ServerEvents.cs:396-470`): `onTextBefore(userobj, text)`
  retorna un **string**: el texto (posiblemente reescrito) se **encadena**
  entre scripts (`result = CallGlobalFunction<String>(...)`); retorno
  vacío/null cancela el mensaje. Ídem `onEmoteBefore` y el `pm.Text` mutable
  de `onPMBefore` (el script edita `pm.Text` y puede `pm.cancel`).
- **Astra** (`manager.rs:173-205`): `check_text_before/check_emote_before/
  check_pm_before` retornan **bool** — un script puede cancelar pero NO
  censurar/reescribir. Los word-filter-scripts de sb0t no funcionan.
- **Fix**: los `ScriptRequest` deben devolver `Option<String>` (None=cancel,
  Some(texto)=continuar con ese texto), encadenado entre scripts, y los call
  sites (tcp_handler/web handler) deben usar el texto resultante.

### S-B. `onJoinCheck` / `onVroomJoinCheck` / `onFloodBefore` no son cancelables (❌)

- **sb0t**: retornan bool — un script puede **rechazar el join** (o el cambio
  de vroom, o dejar pasar un flood).
- **Astra**: son `ScriptEvent` fire-and-forget (no `ScriptRequest`); el valor
  de retorno del script se ignora. Solo TextBefore/EmoteBefore/PMBefore/
  ScribbleCheck son bloqueantes.
- **Fix**: promover los tres a `ScriptRequest` bool y wirear en el login/
  vroom/flood path (TCP y web).

### S-C. `user.sendText()` / `user.sendEmote()` hacen lo incorrecto (❌)

- **sb0t** (`core/AresClient.cs:345`): `user.sendText(t)` hace que **el
  usuario "diga" `t` en público** a todo su vroom (respetando ignores y
  custom name); `sendEmote` ídem con emote. Es lo que usa `#clone`.
- **Astra** (`api.rs user_do_fn`): ambos mandan un **PM del bot al usuario**.
  Además `sendEmote` ni siquiera usa paquete de emote.
- **Fix**: `sendText` → broadcast público como el usuario al vroom;
  `sendEmote` → broadcast emote como el usuario; `sendHTML` → NOSUCH/HTML a
  ese usuario (paridad `SendHTML`). El PM del bot ya existe vía `sendPM`.

### S-D. `user.kick()` / `user.disconnect()` / `user.ban()` dejan fantasmas (❌)

- **Astra** (`api.rs user_do_fn`): mandan el error y hacen
  `user_pool.remove()` — **sin broadcast de PART** al resto de la sala (los
  demás clientes siguen viendo al usuario) y sin el cierre forzado del writer
  (gotcha conocido: los cierres iniciados por el server cuelgan si no se
  fuerza el writer). `ban()` tampoco registra en banstats ni anuncia.
- **sb0t**: `IUser.Ban()/Disconnect()` pasan por el pipeline normal del core.
- **Fix**: reutilizar el plumbing de `astra-commands` (`force_part_user`,
  ban con registro) — probablemente moviendo esos helpers a `server-core`
  para que `api.rs` pueda llamarlos sin ciclo de dependencias.

### S-E. JSUser incompleto: props/métodos/setters faltantes (⚠️)

Comparación con `Objects/JSUser.cs` (520 líneas):

- **Props faltantes en `__USER_PROPS`**: `avatar`, `font`, `captcha`, `idle`,
  `ignores`, `leaf`, `linked`, `localEP`, `visible`, `ghost`.
- **Métodos faltantes en `__mkUser`**: `redirect(hashlink)` (¡ya existe el
  códec en `server-core/hashlink.rs`!), `nudge([sender])`, `scribble(img)`,
  `setTopic(t)`, `setUrl(u)`, `restoreAvatar()`, `getASN()`.
- **Setters writable de sb0t** (`font=`, `avatar=`, `customName=`, `level=`,
  `vroom=`): en Astra todas las props son read-only. Asignar `u.vroom = 2` o
  `u.customName = "X"` es la forma sb0t de mover/renombrar desde scripts.
- **Fix**: extender `__user_get`/`__user_do` + `Object.defineProperty` con
  setters que llamen `__user_do(name, "set:<prop>", valor)`.

### S-F. `onCommand` sin el argumento `target` (⚠️)

- **sb0t**: `onCommand(userobj, command, target, args)` — `target` es el
  JSUser resuelto del primer token de args (o null).
- **Astra**: `onCommand(from, command, args)` — 3 args. Scripts sb0t que
  destructuran el 4º arg leen `undefined` y los que usan `target` rompen.
- **Fix**: `ScriptEvent::Command { from, command, target: Option<String>, args }`
  con `arg_kind(2) = User` cuando hay target resuelto contra el pool.

### S-G. Falta el eval inline `@` del chat (⚠️ feature notable de sb0t)

- **sb0t** (`ServerEvents.cs:405-441`): si el texto público empieza con `@`,
  se evalúa como JS en el script principal (con `userobj` preseteado al
  emisor) — y una expresión JS "suelta" de un usuario con permiso también se
  evalúa y su resultado se imprime. Gated por `ScriptInRoom` +
  `Server.CanScript(client)` (nivel configurable, `ScriptCanLevel`).
- **Astra**: no existe.
- **Fix propuesto**: `@<código>` en `handle_public` (TCP y web) → eval en el
  primer script cargado, SOLO para Owner por defecto (configurable). No
  portar el eval de "expresiones sueltas" (demasiado mágico/peligroso; sb0t
  lo hace con cualquier texto del privilegiado).

### S-H. Stubs silenciosos y verificaciones menores (🔎)

- **CORRECCIÓN tras revisar el historial**: `List`, `Sql` (rusqlite nativo),
  `XmlParser` (DOM JS puro), `ProxyCheck` y `HttpRequest` SÍ están
  implementados (prelude `api.rs:661-849`, fases 1-3b, verificados E2E).
  Las instances de sb0t están completas.
- **Stubs silenciosos detectados en el prelude**: `Users.banned()` retorna
  `[]` (sb0t: lista de JSBannedUser con unban) — Astra ya tiene la data en
  `ctx.bans`; `Channels.enabled()` retorna `true` fijo; `Leaf.sendText()`
  retorna `false` (limitación conocida del snapshot de link).
- **Verificar**: `JSHttpRequest` (¿POST/headers?), `JSUserFont`
  (colores/fuente del user), LiveScript API (`/livescripts`,
  `/downloadscript` vs `LiveScript.cs`).

---

## 2. Inventario de eventos (sb0t → Astra)

Los 47 de sb0t presentes. Firmas: ✅ = igual (con user object donde sb0t pasa
userobj), ⚠️ = difiere.

| Evento | Firma | Nota |
|---|---|---|
| onTextReceived/onTextAfter, onEmoteReceived/onEmoteAfter | ✅ | |
| onTextBefore, onEmoteBefore | ⚠️ | §S-A: no reescriben |
| onPMBefore(user,target,pm), onPM(user,target) | ⚠️/✅ | pm.cancel existe; pm.Text no reescribe (§S-A) |
| onJoinCheck, onVroomJoinCheck, onFloodBefore | ⚠️ | §S-B: no cancelan |
| onJoin, onPartBefore, onPart, onRejected | ✅ | |
| onCommand | ⚠️ | §S-F: falta `target` |
| onHelp | ✅ | corregido en la auditoría de comandos |
| onAvatar, onPersonalMessage, onFileReceived | ✅ | |
| onFlood, onBotPM, onNick, onIgnoring, onIgnoredStateChanged | ✅ | |
| onInvalidLoginAttempt, onLoginGranted, onAdminLevelChanged | ✅ | |
| onRegistering, onRegistered, onUnregistered, onLogout | ✅ | |
| onProxyDetected(user, reply) | 🔎 | verificar 2º arg `reply` |
| onIdled, onUnidled(user, seconds) | ✅ | wired en Fase 1 de comandos |
| onBansAutoCleared, onLinkError, onLinked, onUnlinked | ✅ | |
| onLeafJoin, onLeafPart, onLinkedAdminDisabled | ✅ | leaf = objeto Leaf del prelude |
| onTimer, onLoad, onScribbleCheck | ✅ | |
| *(extras Astra)* onConnect/onDisconnect/onPublic/onEmote/onPrivate/onUserList(End)/onUserUpdate/onHttpComplete | — | conservar |

## 3. Inventario de API global

- **Globales sb0t** (13): todos presentes (`sendText/sendEmote/sendPM` en la
  forma de 3 args `(user, sender, text)`).
- **Statics**: Room ✅, Users ✅ (salvo `banned()` stub — §S-H), Channels ✅
  (con `enabled()` stub), File ✅, Registry ✅, Spelling ✅, Stats ✅, Link ✅
  (verificar profundidad), Base64/Zip/Crypto/Hashlink/Entities/ScriptInclude ✅.
- **Instances**: Avatar ✅, ScribbleImage ✅, Query ✅, Sql ✅, Timer ✅,
  List ✅, XmlParser ✅, ProxyCheck ✅, HttpRequest ✅ (verificar POST/headers).
- **JSUser**: ~29/39 props, 8/15 métodos, 0/5 setters (§S-E).
- **JSPM**: ✅ (contains/remove/replace/isScribble/cancel).

## 4. Roadmap de trabajo

### Fase S0 — Bugs del user object (los scripts actuales los pisan)
- [ ] `user.sendText`/`sendEmote` = hablar/emotear COMO el usuario a su vroom (§S-C).
- [ ] `user.kick`/`disconnect`/`ban` con PART broadcast + cierre forzado del
      writer + registro de ban (§S-D). Mover `force_part_user` (o un
      equivalente) a `server-core`.
- [ ] Tests: kick desde script → los demás reciben PART; sendText → público.

### Fase S1 — Hooks con retorno (reescritura y rechazo)
- [ ] `TextBefore`/`EmoteBefore`/`PMBefore` retornan `Option<String>`
      encadenado entre scripts; call sites TCP y web usan el texto (§S-A).
- [ ] `JoinCheck`/`VroomJoinCheck`/`FloodBefore` como requests cancelables (§S-B).
- [ ] Tests de reescritura, cancelación y rechazo de join.

### Fase S2 — JSUser completo
- [ ] Props faltantes + setters writable (font/avatar/customName/level/vroom) (§S-E).
- [ ] Métodos: redirect (usa `server-core/hashlink`), nudge, setTopic, setUrl,
      restoreAvatar, scribble, getASN (stub honesto si no hay ASN), ignores.
- [ ] `onCommand` con `target` (§S-F).

### Fase S3 — @eval de chat (gated)
- [ ] `@<código>` en texto público → eval con `userobj` preseteado, Owner-only
      por defecto, nivel configurable (§S-G). Sin el eval de expresiones sueltas.

### Fase S4 — Instances y stubs
- [ ] `Users.banned()` real sobre `ctx.bans` (con `unban()`).
- [ ] Verificar JSHttpRequest (POST/headers), JSUserFont, LiveScript API.

### Fase S5 — Cierre
- [ ] Reescribir/retirar `docs/SCRIPTING-ROADMAP.md` (obsoleto).
- [ ] Suite de paridad: un script de prueba que ejercite cada evento y cada
      método del user object.
- [ ] Verificación manual con los scripts reales de las salas.

---

## Barrido exhaustivo de superficie (2026-07-16) — post "100% paridad"

Tras un bug real (hangman: `sql.connected` faltante → el script tomaba su rama
de error en silencio), se hizo el barrido MECÁNICO que la auditoría original
no hizo: extracción de TODOS los `[JSProperty]`/`[JSFunction]` del código
fuente de sb0t (272 nombres en 98 archivos) cruzados contra el prelude de
Astra. Resultado: **17 nombres ausentes + un error sistemático de semántica**
(los estáticos de sb0t son PROPIEDADES, Astra los exponía como FUNCIONES).
Todo implementado y verificado; el scan cierra en 0 faltantes.

Implementado en esta pasada:
- `Sql.connected` (JSSqlInstance) — el disparador de todo.
- `Room.*` → getters/setters reales (`Room.name`, `Room.topic` con setter,
  etc.) + `customNames` (get/set, flag de sala nuevo `customnames`, default
  false como sb0t, con gate en el self-service de `#customname` y toggle
  `#customnames on|off` Host) + `hashlink` ("" honesto) + `setUrl`/`clearUrl`
  (reemplazan la lista de URLs y anuncian web-aware).
- `Stats.*` → 11 propiedades (antes funciones).
- `Link.linked/name/externalIp/port/hashlink` → propiedades.
- `Channels.available/enabled` → propiedades; `Channels.search(texto)` REAL
  sobre la tabla `rooms` del room-search UDP (native `__channels_search`),
  devuelve JSChannel con `language` y `hashlink` incluidos.
- `Crypto.md5hash/sha1hash` → devuelven CryptoResult {toHex,toBase64,toArray}.
- `user.font` → props sb0t {enabled, family, nameColor, textColor} además de
  los campos crudos de Astra.
- `user.avatar` → JSAvatarImage: String OBJECT del base64 (compat Astra) con
  {arg, exists, save(name), toScribble()}; scribble equivalente con toAvatar().
  `Avatar_save`/`ScribbleImage_save` ahora escriben en `<script>/data/`.
- `HttpRequest.oncomplete` → recibe JSHttpRequestResult {arg, page} (String
  OBJECT del body para compat); `download(arg)` acepta el arg de sb0t.
- `ProxyCheck.query` → callback con {error, proxy, type, provider}.
- `XmlNode.attributes` → JSNodeAttributes {getValue, setValue, removeValue,
  length}, indexable como diccionario para compat.

Tests: `sb0t_surface_audit_regressions` (api.rs) cubre todo lo anterior;
`sql_connected_property_like_sb0t` y `file_api_resolves_in_data_subfolder_like_sb0t`
(manager.rs) cubren los disparadores. E2E verde con script de superficie por
WebSocket.

Divergencias honestas que quedan (documentadas, no silenciosas):
- `Room.externalIp`/`Room.hashlink`/`Link.name/externalIp/port/hashlink`: ""
  (Astra no autodetecta su IP externa ni corre link multi-servidor completo).
- `Channels.enabled`: true fijo (no hay off-switch para el room-search local).
- `user.font.nameColor`: "" (Astra no trackea el color de nick por separado).
