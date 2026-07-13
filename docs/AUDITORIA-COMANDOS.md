# Auditoría de comandos — paridad con sb0t

> **Estado (2026-07-12): Fases 0-2 completas; Fase 3 en gran parte hecha.**
> Los 4 bugs reportados están corregidos. Fase 3 aplicada: niveles por
> comando alineados con los `[CommandLevel]` de sb0t (ban/unban/ban60→Admin,
> cbans/colors/url/pmroom/mtimeout/cloak/customnames/lastseen/status/
> roominfo/listquarantined→Owner, whois/stats/admins→Mod, disableavatar→Mod,
> etc.); `roominfo` y `lastseen` convertidos a toggles Host con efecto real
> (broadcast periódico de 20 min / anuncio "was last seen as..." al entrar);
> anuncios públicos de acciones admin (ban/ban10/ban60/unban/kick/muzzle/
> unmuzzle/cbans) con soporte stealth/cloak; `is_user_command` estricto bajo
> `disableadmins`; `DEFAULT_HELP_LINES` actualizado.
>
> **Pendiente:** `customname` target-based (sb0t: mod fija el nombre de OTRO;
> Astra: self-service), `disableadmins` silencioso, formatos exactos de
> whois/id/admins/trace, `redirect` con hashlink real, verificación manual
> con cliente Ares e inbizio (Fase 4).

> Fuente de verdad: `~/Development/C#/sb0t` — `commands/ServerEvents.cs` (dispatch),
> `commands/Eval.cs` (handlers + niveles `[CommandLevel]`), `core/Events.cs` (built-ins
> de cuenta/sesión), `core/TCPProcessor.cs` / `core/ib0t/WebProcessor.cs` (prefijos y
> semántica de texto).
>
> Regla: **los comandos deben llamarse exactamente igual y comportarse igual que en sb0t.**
> Los extras de Astra se conservan, pero nunca deben pisar/renombrar un comando sb0t.

Estados usados en las tablas:

| Estado | Significado |
|---|---|
| ✅ | nombre y semántica verificadas contra sb0t |
| 🔎 | nombre existe; **semántica pendiente de verificar** (trabajo de esta auditoría) |
| ⚠️ | existe pero con semántica/mapeo distinto a sb0t (confirmado) |
| ❌ | roto (bug reportado y confirmado en código) |
| 🚫 | no existe en Astra |

---

## 1. Bugs confirmados (reportados por el usuario)

### 1.1 `#help` no llega a los scripts (❌ → ✅ corregido en Fase 0)

Dos causas en el path TCP de texto público:

1. **El prefijo `#` no se reconoce.** sb0t acepta `#` **y** `/` como prefijo de comando
   en texto público y emotes (`core/TCPProcessor.cs:343-344,407-408`, ídem WebProcessor).
   En Astra, `parse_command` (`crates/commands/src/lib.rs:151`) solo acepta `/`, y
   `handle_public` (`crates/astra/src/tcp_handler.rs:1013`) depende de él → `#help`
   se difunde como chat público normal.
2. **Los builtins "se comen" el comando.** En `handle_public`
   (`crates/astra/src/tcp_handler.rs:1014-1024`), si `dispatch_builtin` retorna
   `handled=true` se hace `return` sin despachar `ScriptEvent::Command`. En sb0t los
   scripts ven **todos** los comandos (`core/Events.cs` → `js.Command(...)` siempre).
   Los otros dos paths (opcode `ClientCommand` → `route_command_text`, y web
   `handle_ws_command`) ya lo hacen bien; el path de texto público TCP no.

Paridad adicional del flujo sb0t (`core/Events.cs:470-548`) que hoy Astra no respeta:

- `help` se responde **antes** del gate de captcha.
- Con captcha pendiente, el único comando permitido es `login` (Astra hoy deja pasar
  **todos** los slash-commands con captcha pendiente — `tcp_handler.rs:1002`).
- Orden sb0t: `help` → captcha-gate → (`register`/`login` si no registrado) →
  (`unregister`/`logout`/`logoff`/`nick`/`setlevel`/`idle|idles` si registrado) →
  builtins de `commands` → scripts → plugins.

### 1.2 `history` (❌ → ✅ corregido en Fase 1)

- **sb0t**: `#history on|off` es un **toggle de sala** (Host, `Settings.History`,
  `Eval.cs:1792`). El replay ocurre **cuando un usuario entra a la sala**
  (`ServerEvents.cs:186` → `History.Show`): últimos **20** mensajes públicos/emotes,
  reproducidos **como mensajes del nick original** (no del bot), con prefijo
  `[-HH:MM:SS]` = antigüedad del mensaje (`commands/History.cs`), y una línea de
  template de cierre. No se muestra a clientes FastPing.
- **Astra**: `handle_history` (`crates/commands/src/lib.rs:2395`) imprime los últimos
  20 al usuario que ejecuta el comando y exige Moderator+. No hay toggle ni replay
  on-join. **Semántica equivocada de punta a punta.**

### 1.3 `idle` / `idles` (❌ → ✅ corregido en Fase 1)

- **sb0t** tiene dos piezas:
  - `#idle on|off` (Host, `Eval.cs:1411`): toggle `Settings.IdleMonitoring` — habilita
    los **anuncios** de idle/unidle.
  - Marcarse ausente (cualquier usuario **registrado**): comando `idle` o `idles`
    (`core/Events.cs:537`) **o emote cuyo texto empiece con `idles`** —
    `#me idles almorzando` (`TCPProcessor.cs:546`, `WebProcessor.cs:1119`; nótese que
    el emote además se difunde normalmente). Efecto: `IdleManager.Add` + evento
    `Idled` → si el monitoring está on, anuncio `+n is idle (+t)` con la hora
    (`ServerEvents.cs:824`).
  - **Salir de idle**: cualquier texto público o emote posterior → anuncio con el
    tiempo que estuvo ausente (días/horas/min/seg, `ServerEvents.cs:833`).
  - **Cooldown**: no puede volver a marcarse idle hasta 5 minutos después del último
    idle (`IdleManager.CheckIfCanIdle`, `core/IdleManager.cs:76`).
- **Astra**: `idle` está fusionado con `clock` como room-flag
  (`crates/commands/src/lib.rs:641`) — el toggle existe pero `idles` no existe, no hay
  marcado manual (ni por comando ni por emote), no hay cooldown, y el idle automático
  por threshold de 300s (`crates/server-core/src/idle.rs`) **no existe en sb0t** (allí
  el idle es siempre manual).

### 1.4 `info` (❌ → ✅ corregido en Fase 1)

- **sb0t** (`Eval.cs:44-69`): imprime el nombre de la sala y luego **una línea por
  cada usuario** conectado (Ares + web): nombre, vroom, **id** — excluyendo cloaked —
  y, si hay link, repite el listado por cada leaf. Es "la lista de usuarios con sus ids".
- **Astra** (`crates/commands/src/lib.rs:2528`): whois detallado de **un solo usuario**
  (self o el target de args). Semántica equivocada.

---

## 2. Mismatches adicionales detectados (no reportados)

| Comando(s) | sb0t | Astra | Estado |
|---|---|---|---|
| `addkewltext` / `remkewltext` | nombres del dispatch (`ServerEvents.cs:921-924`) | solo `kewltext`/`unkewltext` | ✅ alias añadidos |
| `idles` | alias de `idle` manual para registrados | no existe | ✅ implementado |
| `setlevel <nick> 0-3` | Owner, sube nivel de un registrado (`core/Events.cs:519`) | no existe (Astra usa `grant`/`revoke` propios) | ✅ implementado (escala 0-3) |
| `logout` / `logoff` | cierra sesión de la cuenta (`core/Events.cs:503`) | no existen | ✅ implementados |
| `roomsearch <texto>` | **búsqueda** en la lista de canales Ares, top-5 con hashlinks `arlnk://` (Admin, `Eval.cs:1300`) | mapeado como **toggle** de room-flag (`lib.rs:597`) | ✅ stub honesto (falta channel list) |
| `adminmsg` vs `adminannounce` | `adminmsg` = mensaje a admins (Mod); `adminannounce` = **toggle Host on/off** de anunciar acciones admin (`Eval.cs:1893`) | fusionados en un solo handler (`lib.rs:546`) | ✅ separados (flag `adminannounce` + gate en filtros Announce) |
| `addgreetmsg` vs `pmgreetmsg` | `addgreetmsg` = añade greet (Host); `pmgreetmsg` = **toggle on/off** greet por PM (`Eval.cs:1574`) | fusionados (`lib.rs:718`) | ✅ separados (flags `greetmsg`/`pmgreetmsg`) |
| `viewmotd` vs `loadmotd` | `viewmotd` = mostrar MOTD (user); `loadmotd` = **recargar de disco** (Host, `Eval.cs:751`) | fusionados (`lib.rs:795`) | ✅ separados (`loadmotd` recarga + anuncia) |
| `unregister` vs `rempassword` | `unregister` = auto-baja del propio usuario; `rempassword <n>` = Host borra un password de la lista (`ServerEvents.cs`) | fusionados (`lib.rs:369`) | ✅ separados (`rempassword <índice|nick>` Host) |
| `clock` vs `idle` | toggles independientes (`clock` Admin `Eval.cs:1427`; `idle` Host `Eval.cs:1411`) | fusionados en `handle_room_flag` (`lib.rs:641`) | ✅ separados (`idle` = toggle Host + self-idle) |
| gate `disableadmins` | si activo, los comandos de nivel < Host se ignoran **en silencio** (`ServerEvents.cs:886`) | Astra imprime un aviso (`lib.rs:205`) | ⚠️ menor |

---

## 3. Inventario completo sb0t y estado en Astra

Niveles: los de `[CommandLevel]` en `Eval.cs`. Los comandos sin atributo son de
usuario (o del core de cuentas). `Host` en sb0t ≡ `Owner` en Astra.

### 3.1 Cuenta / sesión (core, `Events.cs`)

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `help` | user | lista de comandos; responde incluso con captcha pendiente; los scripts también lo ven | ✅ (Fase 0; `onHelp(user)`) |
| `register <pw>` | user | registra cuenta | 🔎 |
| `login <pw>` | user | inicia sesión (único comando permitido con captcha pendiente) | 🔎 |
| `unregister` | user (no Owner) | elimina la propia cuenta | ✅ separado |
| `logout` / `logoff` | registrado | cierra sesión | ✅ |
| `nick <nombre>` | registrado | cambia nick (≥2 chars, disponible) | 🔎 |
| `setlevel <nick> <0-3>` | Owner | fija nivel de un registrado | ✅ |
| `idle` / `idles` | registrado | marcarse ausente (cooldown 5 min) | ✅ (Fase 1) |

### 3.2 Información / usuario básico

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `version` | user | versión del server | 🔎 |
| `vroom <n>` | user | cambiarse de vroom | 🔎 |
| `id` | user | imprime `nick: id` propio | 🔎 |
| `info` | user | **listado de todos los usuarios** (nombre, vroom, id; + leaves si hay link) | ✅ (Fase 1; leaves pendiente de link) |
| `locate` | user | país/región propio | 🔎 |
| `viewmotd` | user | muestra el MOTD | ✅ separado |
| `pmblock <nick>` | user | bloquear PMs de alguien | 🔎 |
| `whisper <nick> <txt>` | user | susurro en sala | 🔎 |
| `stats` | Mod | estadísticas del server | 🔎 |
| `whois <nick>` | Mod | datos de un usuario | 🔎 |
| `whowas <nick\|ip>` | Mod | historial de conexiones | 🔎 |
| `urban <término>` | Mod | consulta Urban Dictionary | 🔎 (stub externo) |
| `define <término>` | Mod | consulta diccionario | 🔎 (stub externo) |
| `trace <nick>` | Admin | trace/geoip del usuario | 🔎 (stub) |
| `admins` | Mod | lista admins (online/offline) | 🔎 |
| `stats`/`banstats` | Mod/Admin | contadores | 🔎 |

### 3.3 Moderación directa

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `ban <nick> [motivo]` | Admin | ban permanente + anuncio | 🔎 |
| `ban10 <nick>` | Mod | ban 10 min | 🔎 |
| `ban60 <nick>` | Admin | ban 60 min | 🔎 |
| `unban <nick>` | Admin | quita ban | 🔎 |
| `listbans` | Admin | lista bans | 🔎 |
| `cbans` / `clearbans` | Host | limpia todos los bans | 🔎 |
| `kick <nick>` / `kill <nick>` | Mod | expulsa | 🔎 |
| `muzzle <nick>` / `unmuzzle <nick>` | Mod | silencia / des-silencia | 🔎 |
| `mtimeout <nick> <min>` | Host | muzzle temporal | 🔎 |
| `kiddy <nick>` / `unkiddy <nick>` | Mod | efecto "kiddy" en el texto del target | 🔎 |
| `echo <nick> <txt>` / `unecho <nick>` | Mod | eco en el texto del target / limpiar | 🔎 |
| `paint <nick> <txt>` / `unpaint <nick>` | Mod | efecto paint / limpiar | 🔎 |
| `lower <nick>` / `unlower <nick>` | Mod | fuerza minúsculas | 🔎 |
| `customname <nick> <nuevo>` / `uncustomname` | Mod | nombre custom persistente | 🔎 |
| `customnames [on\|off]` | Host | toggle + listado de custom names | 🔎 |
| `addkewltext <nick>` / `remkewltext <nick>` | Mod | texto arcoíris | ✅ (alias) |
| `move <nick> <vroom>` | Admin | mueve de vroom | 🔎 |
| `changename <nick> <nuevo>` / `oldname <nick>` | Admin | renombra temporal / restaura | 🔎 |
| `changemessage <nick> <txt>` | Mod | cambia el personal message | 🔎 |
| `disableavatar <nick>` | Mod | quita avatar | 🔎 |
| `clone <nick>` | Mod | clona usuario (fantasma) | 🔎 |
| `redirect <nick> <sala>` | Admin | redirige a otra sala (hashlink) | 🔎 |
| `clearscreen` | Mod | limpia pantallas | 🔎 |
| `announce <txt>` | Mod | anuncio popup | 🔎 |
| `adminmsg <txt>` | Mod | mensaje a admins+ | ✅ separado |
| `shout <txt>` | — | grito como server | 🔎 |
| `pmroom <txt>` | Host | PM masivo a la sala | 🔎 (Astra `pmall` es extra) |

### 3.4 Bans avanzados

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `rangeban <ip*>` / `rangeunban` / `listrangebans` | Admin | bans por rango IP | 🔎 |
| `asnban <asn>` / `asnunban` / `listasnbans` | Admin | bans por ASN | 🔎 |
| `hostban` / `hostunban` / `hostkick`·`hostkill` / `hostmuzzle` / `hostunmuzzle` / `hostcban` / `hostclone` | Host | variantes host (ignoran jerarquía normal; viajan por el link) | 🔎 |
| `listquarantined` / `unquarantine <nick>` | Host | cuarentena | 🔎 |
| `banstats` | Admin | ranking de baneadores | 🔎 |

### 3.5 Toggles de sala (on|off)

| Comando | Nivel | Setting sb0t | Astra |
|---|---|---|---|
| `caps` | Admin | anti-CAPS | 🔎 |
| `anon` | Admin | permitir anónimos | 🔎 |
| `general` | Admin | sala general (vrooms libres) | 🔎 |
| `scribbles` | Admin | permitir scribbles | 🔎 |
| `audios` | Admin | permitir audio-emotes | 🔎 |
| `buzzes` | Admin | permitir buzz | 🔎 |
| `colors` | Host | colores en texto | 🔎 |
| `sharefiles` | Host | monitoreo de compartición | 🔎 |
| `stealth` | Admin | acciones admin firmadas como la sala | 🔎 |
| `clock` | Admin | reloj en el topic | ✅ separado |
| `idle` | Host | anuncios de idle/unidle | ✅ (Fase 1) |
| `history` | Host | replay de mensajes al entrar | ✅ (Fase 1) |
| `lastseen` | Host | "última vez visto" al entrar | 🔎 |
| `roominfo` | Host | info de sala al entrar | 🔎 |
| `greetmsg` | Host | greet público on/off | 🔎 |
| `pmgreetmsg` | Host | greet por PM on/off | ✅ separado |
| `adminannounce` | Host | anunciar acciones admin on/off | ✅ separado |
| `url` | Host | mostrar URL de la sala on/off | 🔎 |
| `status <txt>` | Host | mensaje de estado de la sala | 🔎 |
| `disableadmins` / `enableadmins` | Host | apaga/enciende comandos admin (silencioso para el resto) | ⚠️ §2 |
| `cloak on\|off` | Host | invisibilidad del admin | 🔎 |
| `vspy on\|off` | Admin | espía de vrooms | 🔎 (stub) |
| `ipsend` / `bansend` / `logsend` | Mod | feeds por PM al admin | 🔎 |

### 3.6 Greets / topic / MOTD / URLs

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `addgreetmsg <txt>` / `remgreetmsg <n>` / `listgreetmsg` | Host | CRUD de greets | ✅ separado (ver §2) |
| `addtopic <txt>` / `remtopic` | Admin | topic (con persistencia) | 🔎 |
| `loadmotd` | Host | recarga MOTD desde disco | ⚠️ fusionado con `viewmotd` |
| `addurl <url> <título>` / `remurl <n>` / `listurl`·`listurls` | Admin | CRUD de URLs de sala | 🔎 |
| `loadtemplate` | Host | recarga plantillas de textos | 🔎 (stub) |

### 3.7 Filtros

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `filter on\|off` | Admin | toggle word-filter | 🔎 |
| `addwordfilter` / `remwordfilter` / `wordfilters` | Admin | CRUD word-filter | 🔎 |
| `addline <n>, <txt>` / `remline <n>, <m>` / `viewfilter <n>` | Admin | líneas de respuesta de un filtro | 🔎 |
| `addjoinfilter` / `remjoinfilter` / `joinfilters` | Admin | filtros de entrada | 🔎 |
| `addfilefilter` / `remfilefilter` / `filefilters` | Admin | filtros de archivos | 🔎 |

### 3.8 Link / autologin / varios

| Comando | Nivel | Función sb0t | Astra |
|---|---|---|---|
| `link <hub>` / `unlink` | Host | linking entre salas | 🔎 |
| `addautologin <nick>` / `remautologin <nick>` / `autologins` | — | autologin por GUID+IP | 🔎 |
| `listpasswords` / `rempassword <n>` | Host | gestión de passwords | ✅ separado (ver §2) |
| `roomsearch <txt>` | Admin | busca en la lista de canales, imprime top-5 con hashlink | ✅ stub honesto |

### 3.9 Extras de Astra (no existen en sb0t — se conservan, sin pisar nombres sb0t)

`users`, `topic`, `motd`, `pmall`, `opmsg`, `uptime`, `banlist`, `grant`, `revoke`,
`cmdlevel`, `greets`/`addgreet`/`remgreet`/`listgreets`, `addfilter`/`remfilter`/
`listfilters`, `joinfilter`/`filefilter` (subcomandos), `roomflags`, `kewltext`/
`unkewltext`, y la familia de scripts (`listscripts`, `loadscript`, `killscript`,
`livescripts`, `downloadscript`, `errors`).

> Nota: `host` y `jsmsg` aparecen en sb0t solo como prefijos excluidos del logging
> (`ServerEvents.cs:889-893`), no son comandos.

---

## 4. Roadmap de trabajo

### Fase 0 — Paridad del pipeline de dispatch ✅ HECHA
- [x] Aceptar `#` además de `/` como prefijo de comando en texto público y emotes del
      path TCP (paridad `TCPProcessor.cs:343,407`); el web ya lo hace.
- [x] Despachar `ScriptEvent::Command` a los scripts **siempre**, incluso cuando un
      builtin manejó el comando, en `handle_public` de `tcp_handler.rs` (los otros dos
      paths ya lo hacen).
- [x] Gate de captcha: con captcha pendiente solo se permite `login`; `help` se
      responde antes del gate (hoy Astra permite todos los comandos).
- [x] Tests: prefijo `#`, gate de captcha, `onHelp` — nota: además se corrigió `onHelp` para recibir al usuario solicitante (paridad `onHelp(userobj)`); antes recibía los args del comando y un script estilo sb0t no podía responder — esa era la causa del fallo en el cliente web. Falta el test de integración e2e con script real recibe `help`, `foo`, y comandos
      builtin por los tres paths (TCP público, opcode ClientCommand, web).

### Fase 1 — Bugs reportados ✅ HECHA
- [x] **`history`**: convertir a toggle Host `on|off` persistido; replay de los últimos
      20 públicos/emotes **al entrar** un usuario, como mensajes del nick original con
      prefijo `[-HH:MM:SS]` y línea de cierre; excluir FastPing. Quitar el
      comportamiento actual de "imprimir al solicitante".
- [x] **`idle`/`idles`**: separar `idle on|off` (Host, anuncios) de `clock`; añadir
      marcado manual para registrados vía comando `idle`/`idles` y vía emote que
      empiece con `idles` (`#me idles ...`); cooldown de 5 min; anuncio de idle con
      hora y de unidle con duración al volver a hablar; evaluar si el auto-idle de
      300s de Astra se elimina o queda opt-in (sb0t no lo tiene).
- [x] **`info`**: reemplazar por el listado completo de usuarios (nombre, vroom, id),
      excluyendo cloaked, incluyendo web users (y leaves cuando exista link).
- [x] Tests de los tres en `crates/commands`. (Decisión: el auto-idle de 300s se ELIMINÓ — sb0t no lo tiene.)

### Fase 2 — Nombres faltantes y comandos fusionados ✅ HECHA
- [x] `addkewltext`/`remkewltext` como nombres canónicos (mantener `kewltext`/`unkewltext` como alias extra).
- [x] `idles` (con Fase 1), `setlevel`, `logout`/`logoff`.
- [x] Separar: `viewmotd` vs `loadmotd`; `adminmsg` vs `adminannounce` (toggle);
      `addgreetmsg` vs `pmgreetmsg` (toggle); `unregister` vs `rempassword`;
      `clock` vs `idle`.
- [x] `roomsearch`: quitar del grupo de toggles (stub honesto hasta que exista channel list); implementar búsqueda real sobre la
      channel list (o stub honesto "requiere channel list" hasta que exista).
- [ ] `disableadmins`: silencioso (pendiente decidir) para no-Host (hoy imprime aviso) — decidir si se
      mantiene el aviso como mejora consciente o se iguala a sb0t.

### Fase 3 — Verificación funcional por categoría (los 🔎)
Para cada grupo, leer el handler sb0t en `Eval.cs` y contrastar con el de Astra:
argumentos, nivel efectivo (`CommandLevelManager` con los defaults de la tabla §3),
mensajes/plantillas, persistencia y efectos colaterales (anuncios, eventos de script).

- [ ] 3.2 información/usuario (incl. formato exacto de `id`, `locate`, `whois`)
- [ ] 3.3 moderación directa (jerarquía de niveles, anuncios con stealth)
- [ ] 3.4 bans avanzados + host*
- [ ] 3.5 toggles (que cada uno toque el setting correcto y persista)
- [ ] 3.6 greets/topic/MOTD/URLs (placeholders `+n`, `+rn`, etc.)
- [ ] 3.7 filtros (sintaxis `addline <n>, <txt>` con coma, igual que sb0t)
- [ ] 3.8 link/autologin/passwords

### Fase 4 — Regresión y cierre
- [ ] Suite de tests de paridad: un test por comando sb0t (nombre + nivel + efecto
      observable mínimo).
- [ ] Actualizar `DEFAULT_HELP_LINES` para que liste los nombres sb0t canónicos.
- [ ] Verificación manual con cliente Ares y cliente web (inbizio) de los 4 bugs
      originales.
- [ ] Documentar en `docs/ROADMAP-V2.md` el estado final.
