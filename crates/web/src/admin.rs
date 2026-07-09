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

fn sessions() -> &'static Mutex<HashMap<String, Instant>> {
    static S: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// Valida el owner password y, si coincide, emite un token de sesión.
/// Retorna `None` si el password es incorrecto o no hay owner password
/// configurado (en ese caso el panel queda deshabilitado por seguridad).
pub fn authenticate(ctx: &AppContext, password: &str) -> Option<String> {
    let owner = &ctx.settings.owner_password;
    if owner.is_empty() || password != owner {
        return None;
    }
    let token = random_token();
    sessions()
        .lock()
        .insert(token.clone(), Instant::now() + SESSION_TTL);
    Some(token)
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

    let (handled, _events) = astra_commands::dispatch_builtin(ctx, &u, cmd, args);

    let mut out = Vec::new();
    while let Ok(pkt) = rx.try_recv() {
        if !pkt.is_empty() && pkt[0] == TcpMsg::Pmt as u8 {
            let mut r = PacketReader::new(&pkt[1..]);
            let _from = r.read_string().ok();
            if let Ok(text) = r.read_string() {
                out.push(text);
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
    std::fs::write(&path, toml_text).map_err(|e| format!("write failed: {}", e))?;
    Ok(())
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
        "\"server\":{{\"room\":\"{}\",\"bot\":\"{}\",\"uptime\":{},\"users\":{},\"peak\":{},\"total\":{},\"bans\":{},\"topic\":\"{}\",\"status\":\"{}\"}}",
        esc(&ctx.settings.room_name),
        esc(&ctx.settings.bot_name),
        secs,
        ctx.user_pool.len(),
        ctx.stats.peak_users(),
        ctx.stats.total_users(),
        ctx.bans.len(),
        esc(&ctx.current_room_topic()),
        esc(&ctx.room_status()),
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
    s.push_str(",\"filters\":[");
    for (i, (pat, act)) in ctx.word_filter.list().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "{{\"pattern\":\"{}\",\"action\":\"{}\"}}", esc(pat), act.as_str()).ok();
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

    s.push('}');
    s
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

    #[test]
    fn auth_requires_correct_password() {
        let ctx = ctx_with_owner("secret");
        assert!(authenticate(&ctx, "wrong").is_none());
        let token = authenticate(&ctx, "secret").expect("auth ok");
        assert!(validate(&token));
        assert!(!validate("garbage"));
    }

    #[test]
    fn no_owner_password_disables_panel() {
        let ctx = ctx_with_owner("");
        assert!(!is_enabled(&ctx));
        assert!(authenticate(&ctx, "").is_none());
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
