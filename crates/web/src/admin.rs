//! Backend del panel de administración web.
//!
//! Expone tres primitivas que el ruteo HTTP de [`crate::ws`] usa:
//! - [`authenticate`]: valida el owner password y emite un token de sesión.
//! - [`validate`]: chequea un token.
//! - [`run_command`]: ejecuta un comando slash como un Owner sintético y
//!   captura la salida (los PMs del bot) como líneas de texto. Reutiliza
//!   `astra_commands::dispatch_builtin`, así que el panel hereda los ~125
//!   comandos sin reimplementar nada.
//! - [`state_json`]: snapshot en vivo (usuarios, stats, flags, bans, etc.).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use proto_ares::{PacketReader, TcpMsg};
use server_core::{AppContext, AresUser, ILevel};
use tokio::sync::mpsc;

/// Duración de una sesión de admin.
const SESSION_TTL: Duration = Duration::from_secs(2 * 3600);
/// ID de sesión reservado para el Owner sintético del panel.
const ADMIN_USER_ID: u16 = 0xFFFE;
/// Intentos fallidos de login tolerados por IP dentro de [`LOGIN_WINDOW`]
/// antes de responder 429 sin siquiera comparar el password.
const LOGIN_MAX_ATTEMPTS: u32 = 5;
/// Ventana del contador de intentos fallidos de login.
const LOGIN_WINDOW: Duration = Duration::from_secs(300);

fn sessions() -> &'static Mutex<HashMap<String, Instant>> {
    static S: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Intentos fallidos de login por IP: `(contador, fin de la ventana)`.
fn login_attempts() -> &'static Mutex<HashMap<IpAddr, (u32, Instant)>> {
    static A: OnceLock<Mutex<HashMap<IpAddr, (u32, Instant)>>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// Compara dos secretos en tiempo constante respecto al contenido (el
/// largo sí se filtra, como en cualquier comparación de este estilo). Evita
/// que un atacante deduzca el password byte a byte midiendo la latencia de
/// `/admin/login`.
fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Por qué falló un intento de login.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    /// Password incorrecto (o panel sin owner password configurado).
    Invalid,
    /// Demasiados intentos fallidos desde esta IP: hay que esperar.
    Throttled,
}

/// Valida el owner password y, si coincide, emite un token de sesión.
///
/// `ip` es la IP real del cliente (ya resuelta contra los proxies
/// confiables) y se usa para limitar la fuerza bruta: tras
/// [`LOGIN_MAX_ATTEMPTS`] fallos en [`LOGIN_WINDOW`] la IP recibe
/// [`AuthError::Throttled`] sin que se compare el password. Un login exitoso
/// limpia el contador de esa IP.
pub fn authenticate(ctx: &AppContext, ip: IpAddr, password: &str) -> Result<String, AuthError> {
    let now = Instant::now();
    {
        let mut att = login_attempts().lock();
        att.retain(|_, (_, until)| *until > now);
        if let Some((count, _)) = att.get(&ip) {
            if *count >= LOGIN_MAX_ATTEMPTS {
                return Err(AuthError::Throttled);
            }
        }
    }

    let owner = &ctx.settings.owner_password;
    if owner.is_empty() || !secret_eq(password, owner) {
        let mut att = login_attempts().lock();
        let entry = att.entry(ip).or_insert((0, now + LOGIN_WINDOW));
        entry.0 += 1;
        entry.1 = now + LOGIN_WINDOW;
        return Err(AuthError::Invalid);
    }

    login_attempts().lock().remove(&ip);
    let token = random_token();
    sessions().lock().insert(token.clone(), now + SESSION_TTL);
    Ok(token)
}

/// ¿Es `token` una sesión válida y vigente? Purga las expiradas de paso.
pub fn validate(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut s = sessions().lock();
    let now = Instant::now();
    s.retain(|_, exp| *exp > now);
    s.contains_key(token)
}

/// ¿El panel está habilitado? (requiere owner password configurado.)
pub fn is_enabled(ctx: &AppContext) -> bool {
    !ctx.settings.owner_password.is_empty()
}

/// Ejecuta un comando slash como Owner sintético y captura las líneas de
/// respuesta (PMs del bot). `line` puede venir con o sin `/` inicial.
pub fn run_command(ctx: &Arc<AppContext>, line: &str) -> Vec<String> {
    let line = line.trim().trim_start_matches('/').trim();
    if line.is_empty() {
        return vec!["Empty command.".to_string()];
    }
    let (cmd, args) = match line.split_once(char::is_whitespace) {
        Some((c, a)) => (c, a.trim()),
        None => (line, ""),
    };

    // Owner sintético con un canal de captura. No se agrega al UserPool,
    // así que no aparece en las userlists ni recibe broadcasts.
    let (tx, mut rx) = mpsc::unbounded_channel::<bytes::Bytes>();
    let mut u = AresUser::new(ADMIN_USER_ID, IpAddr::V4(Ipv4Addr::LOCALHOST), [0u8; 16]);
    u.logged_in = true;
    *u.name.write() = "admin".to_string();
    *u.level.write() = ILevel::Owner;
    u.sender = Some(tx);
    let u = Arc::new(u);

    let scripting = astra_scripting::ScriptHandle::dummy();
    let (handled, _events) = astra_commands::dispatch_builtin(ctx, &scripting, &u, cmd, args);

    let mut out = Vec::new();
    while let Ok(pkt) = rx.try_recv() {
        if !pkt.is_empty() {
            let op = pkt[0];
            if op == TcpMsg::ServerNosuch as u8 {
                // Respuesta de comando vía `user.print` → ServerNosuch (paridad
                // `client.Print` de sb0t).
                let mut r = PacketReader::new(&pkt[1..]);
                if let Ok(text) = r.read_string_nt() {
                    out.push(text);
                }
            } else if op == TcpMsg::Pmt as u8 {
                // PM real (p.ej. avisos al target).
                let mut r = PacketReader::new(&pkt[1..]);
                let _from = r.read_string_nt().ok();
                if let Ok(text) = r.read_string_nt() {
                    out.push(text);
                }
            }
        }
    }
    if !handled {
        out.push(format!("Unknown command: /{}", cmd));
    }
    if out.is_empty() {
        out.push("(ok)".to_string());
    }
    out
}

/// Lee el contenido del `astra.toml` para editarlo en el panel.
///
/// Si el archivo existe, retorna su texto tal cual. Si no (o no se conoce
/// la ruta), serializa los `Settings` actuales a TOML como punto de partida.
pub fn read_settings(ctx: &AppContext) -> String {
    if let Some(path) = ctx.config_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return text;
        }
    }
    toml::to_string_pretty(&*ctx.settings)
        .unwrap_or_else(|_| "# could not serialize current settings".to_string())
}

/// Valida y persiste el TOML editado en el archivo de config.
///
/// Valida que parsee como `Settings` antes de escribir. Retorna `Err` con un
/// mensaje si el TOML es inválido o no se conoce la ruta del config. **El
/// cambio requiere reiniciar** para aplicarse (los settings vivos ya se
/// tomaron al arranque).
pub fn write_settings(ctx: &AppContext, toml_text: &str) -> Result<(), String> {
    // Validar que sea un Settings válido.
    toml::from_str::<server_core::settings::Settings>(toml_text)
        .map_err(|e| format!("invalid TOML: {}", e))?;
    let path = ctx
        .config_path()
        .ok_or_else(|| "no config file path known (started without --config)".to_string())?;
    // `fs::write` escribe EN SITIO (truncate + write), sin temp+rename: eso es
    // deliberado. El config suele entrar al contenedor como bind mount de UN
    // archivo, y un rename sobre el mountpoint fallaría con EBUSY/EXDEV.
    std::fs::write(&path, toml_text).map_err(|e| config_write_error(&path, &e))?;
    Ok(())
}

/// Traduce un error de escritura del config a un mensaje accionable.
///
/// El caso frecuente es un deployment Docker donde el `astra.toml` entra como
/// bind mount de solo lectura (`- ./astra.toml:/app/astra.toml:ro`): ahí el
/// kernel devuelve EROFS y el error crudo ("Read-only file system") no dice
/// qué hay que cambiar. Los demás casos (permisos del archivo, o `/app` no
/// escribible porque el config no está montado y el WORKDIR es de root)
/// también se explican.
fn config_write_error(path: &std::path::Path, e: &std::io::Error) -> String {
    let p = path.display();
    // EROFS = 30 en Linux/macOS. Se compara el errno crudo en vez de
    // `ErrorKind::ReadOnlyFilesystem` para no exigir un rustc más nuevo que el
    // `rust-version` del workspace.
    if e.raw_os_error() == Some(30) {
        return format!(
            "no se puede guardar en {p}: el archivo está montado como SOLO LECTURA. \
             Si corres en Docker, quita el sufijo `:ro` del bind mount del config \
             en tu docker-compose.yml (debe quedar \
             `- ./astra.toml:/app/astra.toml`) y recrea el contenedor con \
             `docker compose up -d`."
        );
    }
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return format!(
            "no se puede guardar en {p}: permiso denegado. El usuario del proceso \
             no puede escribir ese archivo. En Docker, revisa que el `user:` del \
             servicio sea dueño del archivo en el host \
             (`chown $(id -u):$(id -g) astra.toml`)."
        );
    }
    format!("no se puede guardar en {p}: {e}")
}

/// Escapa una string para incrustarla en JSON.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn level_name(level: u8) -> &'static str {
    match level {
        0 => "anonymous",
        1 => "regular",
        2 => "voice",
        50 => "moderator",
        80 => "admin",
        100 => "owner",
        _ => "system",
    }
}

/// Snapshot JSON del estado del servidor para el panel.
pub fn state_json(ctx: &AppContext) -> String {
    use std::fmt::Write;
    let mut s = String::from("{");

    // server
    let secs = ctx.uptime_secs();
    write!(
        s,
        "\"server\":{{\"room\":\"{}\",\"bot\":\"{}\",\"uptime\":{},\"users\":{},\"peak\":{},\"total\":{},\"bans\":{},\"topic\":\"{}\",\"status\":\"{}\",\"version\":\"{}\",\"update\":{},\"directory\":{}}}",
        esc(&ctx.settings.room_name),
        esc(&ctx.settings.bot_name),
        secs,
        ctx.user_pool.len(),
        ctx.stats.peak_users(),
        ctx.stats.total_users(),
        ctx.bans.len(),
        esc(&ctx.current_room_topic()),
        esc(&ctx.room_status()),
        esc(server_core::VERSION),
        match ctx.available_update() {
            Some(v) => format!("\"{}\"", esc(&v)),
            None => "null".to_string(),
        },
        // URL de la ficha en el directorio, si la sala está publicada. Es la
        // confirmación visible de que el registro funcionó.
        match ctx.directory_listing() {
            Some(v) => format!("\"{}\"", esc(&v)),
            None => "null".to_string(),
        },
    )
    .ok();

    // users
    s.push_str(",\"users\":[");
    let mut first = true;
    for u in ctx.user_pool.users() {
        if !u.logged_in {
            continue;
        }
        if !first {
            s.push(',');
        }
        first = false;
        let level = *u.level.read() as u8;
        write!(
            s,
            "{{\"id\":{},\"name\":\"{}\",\"level\":{},\"levelName\":\"{}\",\"ip\":\"{}\",\"vroom\":{},\"files\":{},\"version\":\"{}\",\"muzzled\":{}}}",
            u.id,
            esc(&u.name.read()),
            level,
            level_name(level),
            u.external_ip,
            *u.vroom.read(),
            u.file_count,
            esc(&u.version),
            u.is_muzzled(),
        )
        .ok();
    }
    s.push(']');

    // room flags
    s.push_str(",\"flags\":[");
    for (i, (name, val)) in ctx.room_flags.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "{{\"name\":\"{}\",\"value\":{}}}", esc(name), val).ok();
    }
    s.push(']');

    // greets
    s.push_str(",\"greets\":[");
    for (i, g) in ctx.greets.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "\"{}\"", esc(g)).ok();
    }
    s.push(']');
    write!(s, ",\"greetsEnabled\":{}", ctx.greets.is_enabled()).ok();

// word filters
    s.push_str(&format!(",\"filtersEnabled\":{}", ctx.word_filter.is_enabled()));
    s.push_str(",\"filters\":[");
    for (i, (pat, act, args)) in ctx.word_filter.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(
            s,
            "{{\"pattern\":\"{}\",\"action\":\"{}\",\"args\":\"{}\"}}",
            esc(pat),
            act.as_str(),
            esc(args)
        )
        .ok();
    }
    s.push(']');

    // bans
    s.push_str(",\"bans\":[");
    let mut bi = 0;
    ctx.bans.for_each(|b| {
        if bi > 0 {
            s.push(',');
        }
        bi += 1;
        let _ = write!(
            s,
            "{{\"ident\":{},\"name\":\"{}\",\"ip\":\"{}\"}}",
            b.ident,
            esc(&b.name),
            b.external_ip
        );
    });
    s.push(']');

    // range/asn bans
    s.push_str(",\"rangeBans\":[");
    for (i, p) in ctx.range_bans.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "\"{}\"", esc(p)).ok();
    }
    s.push_str("],\"asnBans\":[");
    for (i, a) in ctx.asn_bans.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "{}", a).ok();
    }
    s.push(']');

    // accounts
    s.push_str(",\"accounts\":[");
    if let Ok(accts) = ctx.db.list_accounts() {
        for (i, (name, level)) in accts.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            write!(
                s,
                "{{\"name\":\"{}\",\"level\":{},\"levelName\":\"{}\"}}",
                esc(name),
                level,
                level_name(*level)
            )
            .ok();
        }
    }
    s.push(']');

    // command levels
    s.push_str(",\"commandLevels\":[");
    for (i, (name, level, is_override)) in ctx.command_levels.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let lvl = *level as u8;
        write!(
            s,
            "{{\"name\":\"{}\",\"level\":{},\"levelName\":\"{}\",\"isOverride\":{}}}",
            esc(name),
            lvl,
            level_name(lvl),
            is_override
        )
        .ok();
    }
    s.push(']');

    // trusted proxies
    s.push_str(",\"trustedProxies\":[");
    for (i, ip) in ctx.trusted_proxies.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "\"{}\"", esc(ip)).ok();
    }
    s.push(']');

    s.push('}');
    s
}

/// Snapshot JSON de `Settings` completo (para las pestañas estructuradas de
/// configuración: Server/Linking/Advanced). A diferencia de
/// [`read_settings`]/[`write_settings`] (editor TOML crudo), este par usa
/// JSON para que el panel pueda editar campos individuales sin tocar texto
/// libre. Misma fuente de verdad: lee/escribe el mismo archivo via
/// `ctx.config_path()`, reusando `Settings::save`.
pub fn settings_json(ctx: &AppContext) -> String {
    let settings: server_core::settings::Settings = match ctx.config_path() {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|_| (*ctx.settings).clone()),
            Err(_) => (*ctx.settings).clone(),
        },
        None => (*ctx.settings).clone(),
    };
    serde_json::to_string(&settings).unwrap_or_else(|_| "{}".to_string())
}

/// Parsea y persiste un `Settings` completo enviado como JSON. Retorna
/// error de validación (sin escribir nada) si el JSON no calza con
/// `Settings`, o si no hay `config_path` configurado.
pub fn write_settings_json(ctx: &AppContext, json: &str) -> Result<(), String> {
    let settings: server_core::settings::Settings =
        serde_json::from_str(json).map_err(|e| format!("invalid settings: {}", e))?;
    let path = ctx
        .config_path()
        .ok_or_else(|| "no config file path configured (started without --config)".to_string())?;
    settings.save(&path).map_err(|e| config_write_error(&path, &e))
}

/// Agrega una IP a la lista de proxies confiables (panel Proxy). Retorna
/// `false` si la IP no parsea.
pub fn add_trusted_proxy(ctx: &AppContext, ip: &str) -> bool {
    ctx.trusted_proxies.add(ip)
}

/// Quita una IP de la lista de proxies confiables. Retorna `false` si no
/// existía.
pub fn remove_trusted_proxy(ctx: &AppContext, ip: &str) -> bool {
    ctx.trusted_proxies.remove(ip)
}

/// Kinds válidos de avatar administrable (sala/default).
const AVATAR_KINDS: &[&str] = &["server", "default"];
/// Tamaño máximo aceptado para un avatar subido (64 KiB). sb0t reescala a
/// 48x48/JPEG-q69 en el cliente GUI; aquí no reescalamos (evita sumar una
/// dependencia de procesamiento de imágenes), así que en su lugar ponemos
/// un techo de tamaño para no dejar subir archivos gigantes.
const MAX_AVATAR_BYTES: usize = 65_536;

/// ¿Los bytes empiezan con la firma de un formato de imagen que los clientes
/// saben mostrar? Solo se chequea el magic number (no se decodifica la
/// imagen): alcanza para no difundir basura a la sala.
fn is_supported_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xFF\xD8\xFF")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
}

/// Sube (o reemplaza) el avatar de sala (`"server"`) o el avatar default
/// (`"default"`). Persiste en `<data_dir>/avatars/{kind}`, actualiza el
/// estado en memoria, y si es el avatar de sala lo difunde en vivo a todos
/// los conectados (paridad `Avatars.UpdateServerAvatar`, que también
/// empuja de inmediato a todo `AUsers`/`WUsers`).
pub fn set_avatar(ctx: &AppContext, kind: &str, bytes: Vec<u8>) -> Result<(), String> {
    if !AVATAR_KINDS.contains(&kind) {
        return Err(format!("invalid avatar kind: '{}' (expected server|default)", kind));
    }
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(format!("avatar too large ({} bytes, max {})", bytes.len(), MAX_AVATAR_BYTES));
    }
    // El avatar de sala se difunde tal cual a todos los conectados, así que no
    // se aceptan bytes arbitrarios: solo formatos que un cliente Ares/web
    // sepa decodificar (sb0t manda JPEG; PNG/GIF también los renderizan).
    if !is_supported_image(&bytes) {
        return Err("invalid image: expected PNG, JPEG or GIF".to_string());
    }
    let dir = std::path::Path::new(&ctx.settings.data_dir).join("avatars");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir failed: {}", e))?;
    std::fs::write(dir.join(kind), &bytes).map_err(|e| format!("write failed: {}", e))?;

    let lock = if kind == "server" { &ctx.server_avatar } else { &ctx.default_avatar };
    *lock.write() = Some(bytes.clone());

    if kind == "server" {
        broadcast_server_avatar(ctx, &bytes);
    }
    Ok(())
}

/// Difunde el avatar de sala actualizado a todos los usuarios conectados:
/// paquete binario `Avatar` para clientes Ares nativos, ident `AVATAR:` de
/// texto para clientes web/inbizier.
fn broadcast_server_avatar(ctx: &AppContext, bytes: &[u8]) {
    use base64::Engine as _;
    let bot_name = &ctx.settings.bot_name;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    for u in ctx.user_pool.users() {
        if !u.logged_in {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            if u.inbizier_web || u.inbizier_mobile {
                let _ = tx.send(crate::protocol::build_avatar(bot_name, &b64));
            }
        } else {
            let _ = u.send(server_core::outbound::build_avatar_c(bot_name, bytes, u.ares_crypto));
        }
    }
}

/// Lee los bytes actuales del avatar de sala/default (para
/// `GET /admin/avatar/{kind}`). Retorna `None` si el kind es inválido o no
/// hay avatar configurado.
pub fn get_avatar_bytes(ctx: &AppContext, kind: &str) -> Option<Vec<u8>> {
    match kind {
        "server" => ctx.server_avatar.read().clone(),
        "default" => ctx.default_avatar.read().clone(),
        _ => None,
    }
}

// ============================================================================
// Bot agente (`GET/POST /admin/bot`)
// ============================================================================

/// GET /admin/bot → config del bot como JSON (o `{}` si no hay bot).
pub fn get_bot_config(ctx: &AppContext) -> String {
    ctx.bot
        .read()
        .as_ref()
        .map(|b| b.config_json())
        .unwrap_or_else(|| "{}".to_string())
}

/// POST /admin/bot → guarda la config (JSON del cliente) y devuelve la nueva
/// config serializada. Aplica en vivo y, si cambió el estado activo o el
/// nombre, actualiza la presencia del bot en la userlist de toda la sala
/// (JOIN/PART para nativos y web).
pub fn set_bot_config(ctx: &Arc<AppContext>, json: &str) -> Result<String, String> {
    // El bot agente debe ser una identidad DISTINTA del "bot" del servidor
    // (settings.bot_name): comparten el mecanismo de userlist fantasma.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
            if !name.trim().is_empty() && name.eq_ignore_ascii_case(&ctx.settings.bot_name) {
                return Err(format!(
                    "el nombre del bot no puede ser igual al del servidor ('{}')",
                    ctx.settings.bot_name
                ));
            }
        }
    }

    let (old_enabled, old_name) = ctx
        .bot
        .read()
        .as_ref()
        .map(|b| (b.is_enabled(), b.bot_name()))
        .unwrap_or((false, String::new()));

    match ctx.bot.read().as_ref() {
        Some(bot) => bot.set_config_json(json)?,
        None => return Err("bot no disponible".into()),
    }

    let (new_enabled, new_name) = ctx
        .bot
        .read()
        .as_ref()
        .map(|b| (b.is_enabled(), b.bot_name()))
        .unwrap_or((false, String::new()));

    update_bot_presence(ctx, old_enabled, &old_name, new_enabled, &new_name);
    Ok(ctx
        .bot
        .read()
        .as_ref()
        .map(|b| b.config_json())
        .unwrap_or_default())
}

/// Anuncia en vivo la presencia del bot (JOIN/PART) cuando se activa,
/// desactiva o renombra desde el panel.
fn update_bot_presence(
    ctx: &AppContext,
    old_enabled: bool,
    old_name: &str,
    new_enabled: bool,
    new_name: &str,
) {
    match (old_enabled, new_enabled) {
        (true, false) if !old_name.is_empty() => broadcast_bot_part(ctx, old_name),
        (false, true) if !new_name.is_empty() => broadcast_bot_join(ctx, new_name),
        (true, true) if old_name != new_name => {
            if !old_name.is_empty() {
                broadcast_bot_part(ctx, old_name);
            }
            if !new_name.is_empty() {
                broadcast_bot_join(ctx, new_name);
            }
        }
        _ => {}
    }
}

/// AresUser sintético para traducir el JOIN/PART del bot al formato web.
fn bot_dummy_user(name: &str) -> AresUser {
    let mut u = AresUser::new(0, IpAddr::V4(Ipv4Addr::UNSPECIFIED), [0u8; 16]);
    *u.name.write() = name.to_string();
    *u.level.write() = ILevel::Owner;
    u
}

/// JOIN del bot a toda la sala (nativos + web).
fn broadcast_bot_join(ctx: &AppContext, name: &str) {
    use bytes::Bytes;
    let plain: Bytes = server_core::outbound::build_join_bot_c(name, None);
    let dummy = bot_dummy_user(name);
    for u in ctx.user_pool.users() {
        if !u.logged_in {
            continue;
        }
        if u.web_client {
            if let Some(tx) = &u.ws_text_sender {
                if let Some(text) = crate::ws_outbound::translate_broadcast(&plain, &dummy, &u) {
                    let _ = tx.send(text);
                }
            }
        } else if let Some(crypto) = u.ares_crypto {
            let _ = u.send(server_core::outbound::build_join_bot_c(name, Some(crypto)));
        } else {
            let _ = u.send(plain.clone());
        }
    }
}

/// PART del bot a toda la sala (nativos + web).
fn broadcast_bot_part(ctx: &AppContext, name: &str) {
    use bytes::Bytes;
    let plain: Bytes = server_core::outbound::build_part_name_c(name, None);
    let dummy = bot_dummy_user(name);
    for u in ctx.user_pool.users() {
        if !u.logged_in {
            continue;
        }
        if u.web_client {
            if let Some(tx) = &u.ws_text_sender {
                if let Some(text) = crate::ws_outbound::translate_broadcast(&plain, &dummy, &u) {
                    let _ = tx.send(text);
                }
            }
        } else if let Some(crypto) = u.ares_crypto {
            let _ = u.send(server_core::outbound::build_part_name_c(name, Some(crypto)));
        } else {
            let _ = u.send(plain.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;
    use server_core::settings::Settings;

    fn ctx_with_owner(pw: &str) -> Arc<AppContext> {
        let mut settings = Settings::default();
        settings.owner_password = pw.to_string();
        Arc::new(AppContext::new(settings, Database::in_memory().unwrap()))
    }

    /// IP única por test: el contador de intentos fallidos es un estático
    /// global compartido por todos los tests del binario, así que cada uno usa
    /// su propia IP para no interferir con los demás.
    fn test_ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, n))
    }

    #[test]
    fn auth_requires_correct_password() {
        let ctx = ctx_with_owner("secret");
        let ip = test_ip(1);
        assert_eq!(authenticate(&ctx, ip, "wrong"), Err(AuthError::Invalid));
        let token = authenticate(&ctx, ip, "secret").expect("auth ok");
        assert!(validate(&token));
        assert!(!validate("garbage"));
    }

    #[test]
    fn no_owner_password_disables_panel() {
        let ctx = ctx_with_owner("");
        assert!(!is_enabled(&ctx));
        assert_eq!(authenticate(&ctx, test_ip(2), ""), Err(AuthError::Invalid));
    }

    #[test]
    fn auth_throttles_after_repeated_failures() {
        let ctx = ctx_with_owner("secret");
        let ip = test_ip(3);
        for _ in 0..LOGIN_MAX_ATTEMPTS {
            assert_eq!(authenticate(&ctx, ip, "wrong"), Err(AuthError::Invalid));
        }
        // Agotados los intentos, ni el password correcto pasa.
        assert_eq!(authenticate(&ctx, ip, "wrong"), Err(AuthError::Throttled));
        assert_eq!(authenticate(&ctx, ip, "secret"), Err(AuthError::Throttled));
        // Y el bloqueo es por IP: otra IP no queda afectada.
        assert!(authenticate(&ctx, test_ip(4), "secret").is_ok());
    }

    #[test]
    fn auth_success_resets_the_failure_counter() {
        let ctx = ctx_with_owner("secret");
        let ip = test_ip(5);
        assert_eq!(authenticate(&ctx, ip, "wrong"), Err(AuthError::Invalid));
        assert!(authenticate(&ctx, ip, "secret").is_ok());
        // Tras el éxito el contador arranca de cero: quedan todos los intentos.
        for _ in 0..LOGIN_MAX_ATTEMPTS {
            assert_eq!(authenticate(&ctx, ip, "wrong"), Err(AuthError::Invalid));
        }
        assert_eq!(authenticate(&ctx, ip, "wrong"), Err(AuthError::Throttled));
    }

    #[test]
    fn secret_eq_matches_only_identical_strings() {
        assert!(secret_eq("hunter2", "hunter2"));
        assert!(!secret_eq("hunter2", "hunter3"));
        assert!(!secret_eq("hunter2", "hunter22"));
        assert!(!secret_eq("", "x"));
        assert!(secret_eq("", ""));
    }

    #[test]
    fn avatar_rejects_non_images() {
        let ctx = ctx_with_owner("secret");
        let err = set_avatar(&ctx, "server", b"not an image at all".to_vec());
        assert!(err.unwrap_err().contains("invalid image"));
        // Y el kind sigue validándose primero.
        assert!(set_avatar(&ctx, "bogus", vec![]).unwrap_err().contains("invalid avatar kind"));
    }

    #[test]
    fn image_signatures_are_recognized() {
        assert!(is_supported_image(b"\x89PNG\r\n\x1a\n\x00\x00"));
        assert!(is_supported_image(b"\xFF\xD8\xFF\xE0junk"));
        assert!(is_supported_image(b"GIF89a...."));
        assert!(!is_supported_image(b"<html>"));
        assert!(!is_supported_image(b""));
    }

    #[test]
    fn config_write_error_explains_readonly_mount() {
        let path = std::path::Path::new("/app/astra.toml");
        let eros = std::io::Error::from_raw_os_error(30);
        let msg = config_write_error(path, &eros);
        assert!(msg.contains("SOLO LECTURA"), "mensaje: {msg}");
        assert!(msg.contains(":ro"), "mensaje: {msg}");

        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "nope");
        let msg = config_write_error(path, &denied);
        assert!(msg.contains("permiso denegado"), "mensaje: {msg}");
    }

    #[test]
    fn run_command_executes_as_owner() {
        let ctx = ctx_with_owner("secret");
        // /version es un comando de usuario; /caps requiere Admin+ → el Owner
        // sintético debe poder ejecutarlo.
        let out = run_command(&ctx, "/caps on");
        assert!(out.iter().any(|l| l.contains("caps") && l.contains("enabled")));
        assert!(ctx.room_flags.get("caps"));
    }

    #[test]
    fn run_command_reports_unknown() {
        let ctx = ctx_with_owner("secret");
        let out = run_command(&ctx, "/notarealcommand");
        assert!(out.iter().any(|l| l.contains("Unknown command")));
    }

    #[test]
    fn settings_read_serializes_current_when_no_file() {
        let ctx = ctx_with_owner("secret");
        // Sin config_path → serializa los Settings actuales a TOML.
        let toml = read_settings(&ctx);
        assert!(toml.contains("owner_password"));
        assert!(toml.contains("room_name"));
    }

    #[test]
    fn settings_write_validates_and_persists() {
        let dir = std::env::temp_dir().join(format!("astra_admin_settings_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("astra.toml");

        let ctx = ctx_with_owner("secret");
        ctx.set_config_path(path.clone());

        // TOML válido → se escribe.
        let good = toml::to_string_pretty(&*ctx.settings).unwrap();
        assert!(write_settings(&ctx, &good).is_ok());
        assert!(path.exists());

        // TOML inválido → error, no se escribe basura.
        let err = write_settings(&ctx, "this is not valid toml {{{");
        assert!(err.is_err());

        // Ahora read_settings devuelve el archivo escrito.
        let read = read_settings(&ctx);
        assert!(read.contains("room_name"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn settings_write_without_path_errors() {
        let ctx = ctx_with_owner("secret");
        let good = toml::to_string_pretty(&*ctx.settings).unwrap();
        assert!(write_settings(&ctx, &good).is_err());
    }

    #[test]
    fn state_json_is_valid_and_has_sections() {
        let ctx = ctx_with_owner("secret");
        let json = state_json(&ctx);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert!(v.get("server").is_some());
        assert!(v.get("users").is_some());
        assert!(v.get("flags").is_some());
        assert!(v.get("bans").is_some());
    }
}
