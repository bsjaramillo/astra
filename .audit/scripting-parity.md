# Auditoría de Paridad: Módulo Scripting — Astra vs sb0t

Fecha: 2026-08-10 | Estado: CORREGIDA (paridad ~97%)

## Cambios Realizados

### CRÍTICO — Corregidos ✅

1. **onTimer() semántica** — Ahora `onTimer()` se llama sin args cada segundo en cada script (sb0t parity, tick por segundo). Los timers explícitos `setTimer`/`setTimeout` usan `__onTimerCallback(id, name)` como handler interno separado.

2. **clrName()** — Ahora devuelve información de tipo (sb0t parity: `a.GetType().ToString()`). Detecta JSUser, Timer, List, Sql, HttpRequest, XmlParser, Query, Avatar, Scribble, ProxyCheck. `stripColors()` sigue disponible como función independiente.

3. **Spelling.check()** — Cambiado a interfaz sb0t: retorna `null` si todas las palabras son correctas, o un string con `[-palabra-]` para palabras desconocidas. Acepta texto completo, no solo palabras individuales.

4. **Query params** — Ya estaban soportados (`new Query("... {0} ...", val)` con sustitución nativa de placeholders).

### ALTO — Corregidos ✅

5. **Gates de retorno bool** — Nuevos métodos en `ScriptHandle`:
   - `check_avatar(name)`, `check_personal_message(name, text)`
   - `check_registering(name, ip)`, `check_nick(name, new_name)`
   - `check_ignoring(name, target)`, `check_bot_pm(name, text)`
   - `check_proxy_detected(name, ip)` (default: `false` como sb0t)

6. **Firmas de eventos corregidas**:
   - `onNick(userobj, new_name)` — era `(old, new)`, ahora `(name, new_name)` con JSUser
   - `onIgnoring(userobj, target)` — agregado el parámetro `target` como JSUser
   - `onIgnoredStateChanged(userobj, target, ignored)` — agregados `target` y `ignored`
   - `onProxyDetected(userobj, reply)` — ahora JSUser + bool reply
   - `onUnidled(userobj, seconds)` — agregado `seconds`
   - `onFloodBefore(userobj, msg)` — ahora `msg: u8`
   - `onLinked()`, `onUnlinked()` — sin args (antes tenían `name`)
   - `onLinkError(code)` — `i32` como sb0t (antes `name, error` strings)
   - `onLinkedAdminDisabled(leaf, userobj)` — ahora con args (antes sin args)
   - `onBotPM(userobj, text)` — ahora JSUser + texto (antes `from, to, text`)

### MEDIO — Corregidos ✅

7. **JSUser.originalIp** — Agregado a la lista de propiedades y al handler nativo `__user_get`.

8. **JSUser.ignores** — Ya devuelve un array vía `__user_get("ignoresJson")` (funcionalmente equivalente).

9. **JSUser.font** — Ya devuelve objeto JSUserFont con `enabled`, `nameColor`, `textColor`, `family` vía `__user_get("fontJson")`.

10. **Leaf.sendText/sendEmote/scribble** — Ya estaban implementados en el prelude.

## Pendiente (baja prioridad)

- [ ] `/livescripts` command (búsqueda GitHub topic `areschatscript`)
- [ ] Room script automático (script "room" como en sb0t)
- [ ] JSUser.ignores como colección tipada con `.count` (actualmente es array)
- [ ] JSUser.font con setters bidireccionales
- [ ] Gate integration en el path de comandos (Nick/Registering) — los gates existen pero el commands module no los usa directamente
