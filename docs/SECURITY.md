# Seguridad

Astra implementa **defensa en 5 capas** contra DDoS y abuso. Cada capa es
independiente y configurable via `astra.toml` bajo `[security]`.

## Capa 1: Rate limit de nuevas conexiones

`ConnectionFloodTracker` cuenta las nuevas conexiones por IP en una
ventana deslizante. Si una IP excede el límite, las nuevas conexiones
se rechazan. Si viola el límite N veces en la misma ventana, **auto-ban**.

```toml
max_new_connections_per_ip = 10       # default
connection_window_secs = 60           # ventana de 1 min
connection_flood_ban_threshold = 3    # 3 violaciones → ban
connection_flood_ban_secs = 300       # ban de 5 min
```

## Capa 2: Límite de conexiones concurrentes

`ConcurrentConnLimiter` limita el número de TCP sockets simultáneos por
IP. Útil contra slowloris y variantes.

```toml
max_concurrent_per_ip = 5             # default
```

## Capa 3: Timeouts

- **Handshake timeout**: 15s para recibir el primer `ClientLogin`.
- **Idle timeout**: 120s entre mensajes (post-login).

```toml
handshake_timeout_secs = 15
idle_timeout_secs = 120
```

## Capa 4: Validación de campos del login

`LoginValidator` rechaza:

- Nicks con caracteres de control o zero-width.
- Versión vacía.
- **Spam bots conocidos**: version `6.6.6.6`, `7.8.7.8`, `6969 files`.
- Perfiles sospechosos (`country=0 + files>0 + age=0`).
- File count absurdo (> 60000).

```toml
min_name_length = 1
max_name_length = 30
reject_spam_bots = true
```

## Capa 5: Auto-ban por logins fallidos

`FailedLoginTracker` cuenta logins fallidos por IP en una ventana. Si
excede el límite, **auto-ban**.

```toml
max_failed_logins = 5
failed_login_window_secs = 3600       # ventana de 1h
failed_login_ban_secs = 3600          # ban de 1h
```

## Bans persistentes

Adicionalmente, `BanSystem` (en `crates/server-core/src/bans.rs`) mantiene
bans persistentes en SQLite, indexados por:
- **GUID** (16 bytes MD5)
- **IP**
- **Ident** (username)

Los bans persisten entre reinicios y se sincronizan opcionalmente con un
backend Supabase (configurable).

## Orden de evaluación

Por cada nueva conexión, las 5 capas se evalúan en orden:

```text
TCP accept
  ↓
Capa 1+2+5  →  rechazado? → ServerError + close
  ↓
Capa 3      →  handshake timeout
  ↓
Capa 4      →  parse login + validate
  ↓
Bans persistentes  →  is_banned(guid, ip)?
  ↓
UserHistory →  join-flood check
  ↓
Aceptar + crear AresUser
```

## Métricas

Todas las capas exponen contadores atómicos via `Stats`:
- `bytes_in`, `bytes_out`
- `peak_users`, `total_users`
- `uptime_secs`

## Tests

- 23 tests unitarios en `crates/server-core/src/security.rs`
- 7 escenarios E2E documentados en el ROADMAP (no automatizados)
