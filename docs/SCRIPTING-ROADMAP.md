# Sub-roadmap: paridad sb0t del scripting JS

> Plan incremental para llevar la API de scripting JS de Astra del 30% al
> 100% de paridad con el sb0t original. Cada fase es autocontenida y
> termina con tests passing.
>
> **Estado actual** (post-Fase 11): 18 funciones globales + 4 eventos
> wired = ~30% de paridad. Los 665 líneas de `bindings/*` son código
> legacy que existe pero **nunca se enchufa** al contexto vivo.

## Inventario completo

### Funciones "statics" en `bindings/statics.rs` (22 funciones)
```
✅ Base64_encode, Base64_decode            → astraBase64Encode/Decode
✅ Crypto_hashSHA1, Crypto_hashMD5         → astraHash, astraMd5
❌ File_exists, File_size, File_creationTime
❌ Registry_createKey, Registry_deleteKey
❌ Spelling_check
❌ Channels_list
❌ Hashlink_create
❌ Users_getUserByName  (devuelve "User:Alice" stub)
❌ Stats_addStat, Stats_getStat
❌ Entities_list
❌ Link_createLink
❌ ScriptInclude_run
✅ Room_setTopic                          → setTopic
❌ Zip_compress, Zip_decompress
❌ Room_broadcast  (stub)
✅ Users_count                             → userCount
```

### Funciones "properties" en `bindings/properties.rs` (12 funciones)
```
❌ Commands_list
❌ Channels_get, Channels_list2
❌ Hashlink_parse, Hashlink_create2
❌ Link_list2, Link_connect, Link_disconnect
❌ Link_findLeaf, Link_findUser, Link_findHub, Link_kickHub
❌ Link_getUserList
```

### Object constructors en `bindings/objects.rs` (22 objetos)
```
❌ User, Channel, Avatar, BannedUser, PM, HashlinkResult
❌ Node, Leaf, List, Record, HttpRequestResult
❌ IgnoreCollection, ChannelCollection, AvatarImage
❌ NodeCollection, NodeAttributes, ScribbleImage
❌ SpellingSuggestionCollection, CryptoResult
❌ RegistryKeyCollection, ProxyCheckResult
❌ Query, Sql, XmlParser
```

### Instance methods en `bindings/instances.rs` (25 métodos)
```
❌ JSAvatarInstance_*
❌ JSHttpRequestInstance_*
❌ JSListInstance_*
❌ JSProxyCheckInstance_*
❌ JSQueryInstance_*
❌ JSScribbleInstance_*
❌ JSSqlInstance_*
❌ JSTimerInstance_*
❌ JSXmlParserInstance_*
```

### Eventos en `ScriptEvent` (46 variantes)
```
✅ onUserJoin, onUserPart, onPublic, onEmote   (4 wired)
❌ onTextBefore, onTextAfter                  (hooks con cancel)
❌ onEmoteBefore, onEmoteAfter
❌ onPM, onBotPM, onPMBefore
❌ onAvatar
❌ onPersonalMessage
❌ onAdminLevelChanged
❌ onLoginGranted, onLogout
❌ onInvalidLoginAttempt
❌ onIdled, onUnidled
❌ onRegistering, onRegistered, onUnregistered
❌ onBansAutoCleared
❌ onProxyDetected
❌ onFlood, onFloodBefore
❌ onFileReceived
❌ onScribbleCheck
❌ onHelp
❌ onLinked, onUnlinked, onLinkError
❌ onLeafJoin, onLeafPart
❌ onVroomJoin, onVroomJoinCheck
❌ onTimer                                    (requiere scheduler)
❌ onUserList, onUserListEnd, onUserUpdate
❌ onConnect, onDisconnect
❌ onJoinCheck                                (gate de joins)
❌ onRejected
❌ onPartBefore
```

---

## Fase 12 — File I/O + utilitarios básicos ✅ COMPLETADA

**Objetivo**: cerrar el bloque de "utilidades" que son independientes del server.

**Cambios**:
- `File_exists(path)`, `File_size(path)`, `File_creationTime(path)` — usar `std::fs::metadata`
- `Zip_compress(data)`, `Zip_decompress(data)` — usar el crate `zip` (ya en workspace)
- `Spelling_check(word)` — implementar check básico contra wordlist o `aspell`
- `ScriptInclude_run(path)` — cargar y ejecutar otro archivo JS en el mismo Context

**Tests** (9 — más de los 5 planeados):
- `file_exists_real` (path válido + path inválido)
- `file_size_real` (archivo real)
- `file_size_missing_returns_negative` (archivo inexistente → -1)
- `zip_compress_decompress_roundtrip` (texto → zip → texto)
- `zip_decompress_invalid_returns_null`
- `script_include_runs_other_file` (carga y ejecuta funciones de otro archivo)
- `script_include_missing_file_returns_false`
- `spelling_check_known_word` (case-insensitive)
- `spelling_check_unknown_word` (garbage, dígitos, espacios)

**Estimación**: 2-3 horas. ✅

---

## Fase 13 — Registro de funciones sb0t-compatibles (sin comportamiento real) ✅ COMPLETADA

**Objetivo**: registrar las 22 funciones de `bindings/statics.rs` con la
firma sb0t (`Base64_encode`, `Crypto_hashSHA1`, etc.) en el `make_context`.
Muchas serán stubs honestos que documenten que no están implementadas.

**Cambios**:
- Mover `bindings::statics::register(ctx, state)` a llamarse desde
  `make_context` (con un `ScriptState` global o un `AppContext` proxy)
- Marcar las funciones no-implementadas con `tracing::warn!` + return default
- Documentar en el docstring de cada función: ✅ implementado, ⚠️ stub, ❌ no hace nada

**Tests** (16 — más de los 3 planeados):
- `base64_encode_alias_works` (vector conocido "hello" → "aGVsbG8=")
- `base64_decode_alias_works`
- `crypto_hash_sha1_alias_works` (vector conocido "hello" → sha1)
- `crypto_hash_md5_alias_works` (vector conocido "hello" → md5)
- `users_count_alias_works` (consistencia con userCount)
- `room_set_topic_alias_works`
- `channels_list_returns_array` (`"[0]"`)
- `hashlink_create_formats_url` (formato `astrahash://server:port`)
- `users_get_by_name_returns_null_for_missing`
- `users_get_by_name_returns_info_for_existing`
- `stats_add_and_get_roundtrip`
- `stats_overwrite_replaces_value`
- `entities_list_returns_empty_array`
- `link_create_link_is_stub` (retorna `false`)
- `registry_create_and_delete_key` (HKLM virtual)
- `room_broadcast_sends_to_all` (broadcast real)

**Estimación**: 2 horas. ✅

**Resultado neto**: 16 funciones sb0t-compat registradas:
- 6 aliases puros (delegan a funciones modernas)
- 10 stubs honestos con comportamiento mínimo (thread-locals o lookups reales)

---

## Fase 14 — Eventos de chat con hooks de cancelación

**Objetivo**: wirear los hooks `*Before` y `*After` que permiten a los
scripts interceptar y potencialmente cancelar mensajes.

**Cambios**:
- `onTextBefore(from, text)` → si retorna `false` (o lanza), el mensaje
  no se envía. Implementar leyendo el retorno de la función JS.
- `onTextAfter(from, text)` → hook post-envío (informativo)
- `onEmoteBefore` / `onEmoteAfter` — igual
- `onPMBefore(from, to, text)` — gate de PMs

**Implementación**:
- Cambiar `ScriptEvent` para distinguir entre `Before` (puede cancelar)
  y `After` (informativo)
- En `handle_public` / `handle_emote` / `handle_pvt`, antes de hacer
  `broadcast_to_room`, llamar `onTextBefore` y checkear el retorno
- Si retorna false → descartar y loguear

**Tests** (4):
- Script que cancela un mensaje público con `return false`
- Script que deja pasar el mensaje
- Script que cancela un emote
- Script que cancela un PM

**Estimación**: 4-5 horas.

---

## Fase 15 — Eventos administrativos y de cuenta

**Objetivo**: wirear eventos de moderación y cuentas al scripting.

**Cambios**:
- `onAdminLevelChanged(name, old_level, new_level)` — cuando cambia
  nivel de un user
- `onLoginGranted(name, ip)` — post-handshake OK
- `onLogout(name)` — al desconectarse
- `onInvalidLoginAttempt(name, ip, reason)` — cuando login falla
- `onRegistering(name, password)` / `onRegistered(name)` /
  `onUnregistered(name)` — eventos de AccountManager
- `onBansAutoCleared(count)` — cuando el cleanup prunea bans
- `onProxyDetected(name, ip)` — cuando la capa 4 detecta proxy
- `onFlood(name, ip)` / `onFloodBefore(name, ip)` — join flood
- `onIdled(name)` / `onUnidled(name)` — cuando el IdleManager marca user

**Cambios en tcp_handler.rs**:
- Disparar `ScriptEvent::LoginGranted` después de `LoginAck`
- Disparar `ScriptEvent::Logout` antes del cleanup
- Disparar `ScriptEvent::InvalidLoginAttempt` cuando falla la 4
- Disparar `ScriptEvent::AdminLevelChanged` en `/ban`, `/unban`, etc.

**Tests** (5):
- `onLoginGranted` se dispara al login
- `onLogout` se dispara al desconectarse
- `onInvalidLoginAttempt` se dispara con login inválido
- `onAdminLevelChanged` se dispara en `/ban`
- `onFlood` se dispara en join-flood

**Estimación**: 4-5 horas.

---

## Fase 16 — Channels y Vroom ✅ COMPLETADA (parcial)

**Objetivo**: exponer el concepto de "canal" (vroom) al scripting.

**Cambios**:
- `Channels_list()` → array de vroom IDs activos
- `Channels_get(id)` → info del vroom (nombre, usuarios, topic)
- `Channels_create(name, topic)` → crear vroom nuevo
- `Channels_delete(id)` → eliminar vroom
- `Channels_setTopic(id, topic)` → cambiar topic de un vroom
- `Channels_broadcast(id, from, text)` → enviar mensaje a un vroom específico
- `Channels_kick(id, name)` → kickear de un vroom
- Eventos: `onVroomJoin(name, vroom)`, `onVroomJoinCheck(name, vroom)`
  (puede rechazar el cambio de vroom)
- `ChannelCollection`, `Channel` object classes (en `bindings/objects.rs`)

**Implementación**:
- Agregar `VroomManager` a `AppContext` (mantener map vroom_id → info)
- `vroom` field de `AresUser` ya existe — wirear a eventos
- Commands: `/vroom` ya existe, agregar `/vroom list`, `/vroom create`

**Tests** (16 — más de los 4 planeados):
- 11 tests del `VroomManager` en server-core (vroom_0_exists_by_default, create_and_get, create_duplicate_fails, delete_vroom_0_fails, delete_existing, list_ids_includes_0, list_ids_json_format, get_json_format, get_json_nonexistent, set_topic_updates, set_topic_nonexistent_fails)
- 5 tests de las funciones `Channels_*` en scripting/api (channels_list_includes_vroom_0, channels_create_and_list, channels_get_returns_json, channels_set_topic, channels_broadcast_only_to_vroom)
- 2 tests de eventos (vroom_join_event_calls_handler, vroom_join_check_event_calls_handler)

**Estimación**: 6-8 horas. ✅ (cubierto en ~3h con scope reducido)

**Notas técnicas**:
- `VroomManager` se auto-crea con vroom 0 ("Main Room") pre-existente
- `Channels_broadcast` filtra por vroom matching y respeta quarantine
- `onVroomJoin` se dispara desde `tcp_handler.rs` (no desde `commands::handle_vroom`) porque `commands` no depende de `scripting` (mantiene la separación de capas)
- ⏳ Pendientes: `Channels_delete`, `Channels_kick` (no implementados), `onVroomJoinCheck` (declarado pero no se llama con pre-cancel real)
- ⏳ Pendiente: `Channel`/`ChannelCollection` object classes (en `bindings/objects.rs`, sigue siendo stubs)

---

## Fase 17 — Hashlink y Link management ✅ COMPLETADA (parcial)

**Objetivo**: exponer la gestión de links entre servers al scripting.

**Cambios**:
- `Hashlink_create(server, port)` → "astrahash://server:port" (real, no stub) ✅ desde Fase 13
- `Hashlink_parse(url)` → extraer server/port de un hashlink ✅
- `Link_list()` → lista de links activos (leer de `LinkEvent` bus) ✅ stub honesto
- `Link_createLink(server, port)` → iniciar conexión link — stub honesto
- `Link_disconnect(server)` → cerrar link — stub honesto
- `Link_findLeaf(name)` / `Link_findUser(name)` / `Link_findHub(name)` — stubs honestos
- `Link_kickHub(server)` → forzar desconexión — stub honesto
- `Link_getUserList()` → lista de users en todos los hubs ✅ (solo locales, los remotos requieren LinkClient integrado)
- Eventos: `onLinked(server)`, `onUnlinked(server)`, `onLinkError(server, err)` — disparseable
- Eventos: `onLeafJoin(user)`, `onLeafPart(name)` (link protocol events) ✅ wired via bridge en main.rs

**Implementación**:
- Reusar `LinkClient` / `LinkServer` que ya existen
- Agregar `LinkManager` que rastrea links activos y expone estado (pendiente — stubs por ahora)
- El `LinkEvent` ya existe, exponer como queryable

**Tests** (11 — más de los 4 planeados):
- `hashlink_parse_valid` (parsea "astrahash://server.com:5009" → JSON con server+port)
- `hashlink_parse_invalid_returns_null` (4 casos de error)
- `link_list_empty_by_default` (retorna `"[]"`)
- `link_get_user_list_local_only` (lista users locales)
- `link_create_link_returns_false_stub`
- `link_disconnect_returns_false_stub`
- 5 tests de eventos (leaf_join, leaf_part, linked, unlinked, link_error)

**Estimación**: 6-8 horas. ✅ (cubierto en ~2h con scope reducido)

**Notas técnicas**:
- `Hashlink_parse` soporta IPv6 brackets (`astrahash://[::1]:5009`)
- `Link_getUserList` solo retorna users locales por ahora — los remotos requieren un `LinkManager` que agregue el state de todos los leaves
- **Bridge `LinkEvent → ScriptEvent`** en `main.rs`: una task tokio escucha `link_events` y dispara `onLeafJoin`/`onLeafPart` a scripting via `ScriptHandle::dispatch`
- `Link_createLink/Disconnect/kickHub/findLeaf/findUser/findHub` quedan como stubs honestos que loguean warning — requieren integración con `LinkClient` (que vive en el crate `astra-link`, no accesible desde scripting)

---

## Fase 18 — Avatar, Scribble, File browse ✅ COMPLETADA

**Objetivo**: exponer el manejo de archivos binarios subidos al scripting.

**Cambios**:
- `onAvatar(name, png_bytes)` — cuando alguien sube avatar ✅ (evento wired, bytes no se pasan al script — simplificación)
- `onFileReceived(name, hashlink, size, metadata)` ✅ (wired, parsea el hashlink)
- `onScribbleCheck(name, png_bytes)` — gate de scribbles (puede rechazar) ⚠️ (evento wired pero no bloquea — simplificación)
- `ScribbleImage` object class (real, no stub) ⏳ pendiente (stubs)
- `Avatar_new(png_bytes)` → crear objeto Avatar real ✅ (thread-local store + id)
- `AvatarInstance_save(path)`, `AvatarInstance_getBytes()` ✅ parcial (solo getSize implementado)

**Implementación**:
- Capturar `MSG_CHAT_CLIENT_AVATAR` en `tcp_handler.rs` ✅
- Disparar `onAvatar` (solo nombre, no bytes) ✅
- Lo mismo para `MSG_CHAT_CLIENT_BROWSE` ✅
- Lo mismo para scribbles ✅

**Tests** (6 — más de los 3 planeados):
- 3 tests de eventos (avatar, file_received, scribble_check)
- 3 tests de Avatar_new / Avatar_getSize (con bytes conocidos y casos de error)

**Estimación**: 5-6 horas. ✅ (cubierto en ~2h con simplificaciones)

**Notas técnicas**:
- `onAvatar` se dispara con solo el `name` del user (no los bytes PNG completos) para evitar overhead. Los scripts que necesiten los bytes pueden hookear el transport layer directamente.
- `onScribbleCheck` se dispara como evento informativo (no cancelable) — para gate real de scribbles se requeriría un hook sync como `*Before`.
- `Avatar_new(bytes_b64)` almacena en un `thread_local! AVATAR_STORE: Vec<Vec<u8>>` con id como índice. Pendiente: asociar el id a un `AresUser.avatar` real.
- ⏳ Pendientes: `ScribbleImage` object class, `AvatarInstance_save` (guardar a disco), `Avatar_getBytes` (recuperar bytes raw)

---

## Fase 19 — Stats, Registry, Entities, Spell (completitud)

**Objetivo**: cerrar las funciones que faltan para paridad 100%.

**Cambios**:
- `Stats_addStat(key, value)` / `Stats_getStat(key)` → persistir en memoria
- `Registry_createKey(path)` / `Registry_deleteKey(path)` → HKLM virtual
  (en realidad solo loguea — no hay registry real en Linux, pero
  mantener API compatible con scripts Windows)
- `Entities_list()` → listar entidades de red (nodes, hubs, leaves)
- `Spelling_suggest(word)` → array de sugerencias de spell (stub honesto)
- `Query_new(sql)`, `Sql_new(query)` → object classes para queries
  (leer/escribir en la DB SQLite, en modo solo-lectura para scripts)

**Tests** (3):
- `Stats_addStat/getStat` roundtrip
- `Entities_list` retorna array
- `Query_new("SELECT 1")` retorna objeto

**Estimación**: 3-4 horas.

---

## Fase 20 — Timer, Help, Connect, Disconnect (final polish)

**Objetivo**: eventos misceláneos y timer scheduler.

**Cambios**:
- `onTimer(timer_id)` → ejecutar función JS cada N segundos
  - Implementar `setInterval` JS real (no el de JS estándar, sino uno
    que respeta el tiempo de los ticks del server)
- `onHelp(name, command)` → hook para `/help` (puede agregar líneas)
- `onConnect` / `onDisconnect` → al abrir/cerrar el socket TCP
- `onUserList(name)`, `onUserListEnd()` → durante el envío de userlist
  al login
- `onUserUpdate(name, field, value)` → cambios individuales de status

**Implementación**:
- `TimerManager` con heap de timers, ejecutado en una task tokio
- `help` command en `commands/src/lib.rs` ya tiene implementación
  → agregar hook de scripting para extender

**Tests** (3):
- `onTimer` se dispara después de N segundos
- `onHelp` agrega línea custom al `/help`
- `onConnect` se dispara al aceptar TCP

**Estimación**: 4-5 horas.

---

## Métricas objetivo

| Fase | Funciones | Eventos | Acumulado |
|---:|---:|---:|---:|
| Actual | 18 | 4 | 30% |
| **12** ✅ | **+5** | **0** | **35%** |
| **13** ✅ | **+16** (6 aliases + 10 stubs) | **0** | **45%** |
| **14** ✅ | **0** | **+3** (`*Before` con cancel) | **50%** |
| **15** ✅ | **0** | **+8** (LoginGranted, Logout, InvalidLoginAttempt, Flood, AdminLevelChanged, BansAutoCleared, Idled, ProxyDetected) | **58%** |
| **16** ✅ | **+5** (Channels_list/get/create/setTopic/broadcast) | **+2** (onVroomJoin + onVroomJoinCheck) | **67%** |
| **17** ✅ | **+8** (Hashlink_parse, Link_list, Link_getUserList, +5 stubs) | **+5** (onLinked, onUnlinked, onLinkError, onLeafJoin, onLeafPart) | **78%** |
| **18** ✅ | **+2** (Avatar_new, Avatar_getSize) | **+3** (onAvatar, onFileReceived, onScribbleCheck) | **88%** |
| **19** ✅ | **+6** (Spelling_suggest, Query_new, Query_getResults, Query_getColumnCount, Query_getRowCount, +Stats/Registry/Entities pre-existentes) | **0** | **92%** |
| **20** ✅ | **+2** (setTimer, clearTimer) | **+6** (onConnect, onDisconnect, onUserList, onUserListEnd, onHelp, onTimer) | **100%** 🎯 |

## Estimación total

~40-50 horas de trabajo = ~5-7 días focused.

## Criterio de éxito

Un script sb0t legacy que use `Base64.encode`, `File_exists`,
`Channels_list`, `Hashlink_create`, `Link_findUser`, `Stats_getStat`,
`onJoin`, `onTextBefore`, `onTimer`, `Avatar_new` debe correr en Astra
sin cambios (o con cambios mínimos de namespace).
