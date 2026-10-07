//! Perfil de configuración de la sala: export/import en JSON.
//!
//! Es un **perfil de contenido portable**, no un backup completo de la base de
//! datos. Incluye lo que define la "personalidad" y las reglas de la sala
//! editables desde el panel: textos del sistema (templates), MOTD, topic,
//! status, filtros de palabras, filtros de nombre (join/file), greetings,
//! URLs rotadas, flags de sala, niveles de comando, auto-logins por IP y bots
//! agente.
//!
//! **NO** incluye datos operativos: cuentas, bans, historial, nodos/rooms UDP,
//! detecciones anti-VPN, avatares, scripts, logs ni config anti-VPN/GeoIP.
//!
//! El import es **replace-only**: cada sección presente en el archivo se limpia
//! y se vuelve a aplicar desde el perfil; las secciones ausentes se dejan como
//! están (así un archivo parcial no borra lo que no menciona). Todo se aplica
//! en vivo, sin reiniciar el servidor.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use server_core::word_filter::FilterAction;
use server_core::{AppContext, ILevel};

/// Identificador del formato. Se exige al importar para no confundir este
/// archivo con cualquier otro JSON.
pub const PROFILE_KIND: &str = "astra.room.config";
/// Versión del esquema del perfil. Se rechaza cualquier archivo con una
/// versión mayor (no sabríamos interpretarlo).
pub const PROFILE_SCHEMA: u32 = 1;

/// Perfil completo de configuración de sala.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomProfile {
    /// Siempre [`PROFILE_KIND`].
    pub kind: String,
    /// Versión del esquema (ver [`PROFILE_SCHEMA`]).
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// Versión de Astra que generó el archivo (informativa).
    #[serde(default)]
    pub astra_version: String,
    /// Momento de exportación (epoch secs, informativo).
    #[serde(default)]
    pub exported_at: u64,

    /// Nombre de sala/bot (informativo) + topic y status (se aplican).
    #[serde(default)]
    pub room: Option<RoomMeta>,
    /// Overrides de textos del sistema (`clave -> texto`).
    #[serde(default)]
    pub templates: Option<BTreeMap<String, String>>,
    /// MOTD completo (multilínea).
    #[serde(default)]
    pub motd: Option<String>,
    /// Plantillas de greeting.
    #[serde(default)]
    pub greets: Option<Vec<String>>,
    /// Filtros de palabras (con sus líneas de `announce`).
    #[serde(default)]
    pub word_filters: Option<Vec<WordFilterEntry>>,
    /// Estado on/off del filtrado de palabras.
    #[serde(default)]
    pub filter_enabled: Option<bool>,
    /// Filtros de nick en el login.
    #[serde(default)]
    pub join_filters: Option<Vec<String>>,
    /// Filtros de nombres de archivo.
    #[serde(default)]
    pub file_filters: Option<Vec<String>>,
    /// URLs rotadas de la sala.
    #[serde(default)]
    pub urls: Option<Vec<UrlEntry>>,
    /// Flags de sala que difieren de su default.
    #[serde(default)]
    pub room_flags: Option<BTreeMap<String, bool>>,
    /// Overrides de nivel mínimo por comando.
    #[serde(default)]
    pub command_levels: Option<Vec<CommandLevelEntry>>,
    /// Auto-logins por IP/GUID.
    #[serde(default)]
    pub autologins: Option<Vec<AutologinEntry>>,
    /// Configuraciones de bots agente (JSON crudo de `BotConfig`).
    ///
    /// **Contiene secretos**: incluye la API key del LLM de cada bot.
    #[serde(default)]
    pub bots: Option<Vec<serde_json::Value>>,
}

fn default_schema() -> u32 {
    PROFILE_SCHEMA
}

/// Metadata de sala y topic/status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMeta {
    /// Nombre de la sala (solo informativo; no se aplica en el import).
    #[serde(default)]
    pub room_name: String,
    /// Nombre del bot (solo informativo; no se aplica en el import).
    #[serde(default)]
    pub bot_name: String,
    /// Topic de la sala (se aplica).
    #[serde(default)]
    pub topic: String,
    /// Status de la sala (se aplica).
    #[serde(default)]
    pub status: String,
}

/// Una entrada de filtro de palabras con sus líneas de `announce`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordFilterEntry {
    /// Patrón (con comodines `*`/`?`).
    pub pattern: String,
    /// Acción (`block`, `kick`, `ban`, `muzzle`, `announce`, `replace`, ...).
    pub action: String,
    /// Argumento de la acción (vroom de `move`, destino de `redirect`, ...).
    #[serde(default)]
    pub args: String,
    /// Líneas de respuesta (solo `announce`).
    #[serde(default)]
    pub lines: Vec<String>,
}

/// Una URL rotada de la sala.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlEntry {
    /// Dirección (href).
    pub address: String,
    /// Texto visible.
    pub text: String,
}

/// Override de nivel mínimo de un comando.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLevelEntry {
    /// Nombre del comando (sin `/`).
    pub command: String,
    /// Nivel mínimo (byte de [`ILevel`]).
    pub level: u8,
}

/// Auto-login por IP/GUID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutologinEntry {
    /// GUID del cliente (32 hex).
    pub guid: String,
    /// Nombre informativo.
    #[serde(default)]
    pub name: String,
    /// Nivel a auto-otorgar (Moderator/Admin/Owner).
    pub level: u8,
    /// IP asociada.
    pub ip: String,
}

/// Resumen de un import, para que el panel muestre qué se aplicó.
#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    /// Cantidad aplicada por sección.
    pub applied: BTreeMap<String, usize>,
    /// Errores no fatales (p. ej. un bot que no se pudo crear).
    pub errors: Vec<String>,
}

/// Convierte un byte de nivel a [`ILevel`] (mapeo permisivo: el valor mayor
/// conocido que no supere el byte).
fn ilevel_from_u8(v: u8) -> ILevel {
    match v {
        l if l >= ILevel::Owner as u8 => ILevel::Owner,
        l if l >= ILevel::Admin as u8 => ILevel::Admin,
        l if l >= ILevel::Moderator as u8 => ILevel::Moderator,
        l if l >= ILevel::Voice as u8 => ILevel::Voice,
        l if l >= ILevel::Regular as u8 => ILevel::Regular,
        _ => ILevel::Anonymous,
    }
}

/// Parsea un GUID de 32 caracteres hex a `[u8; 16]`.
fn guid_from_hex(s: &str) -> Option<[u8; 16]> {
    let s = s.trim();
    if s.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

/// Construye el perfil a partir del estado en vivo.
fn build_profile(ctx: &AppContext) -> RoomProfile {
    // Templates: solo los overrides (los defaults no aportan).
    let templates: BTreeMap<String, String> = ctx
        .templates
        .list()
        .into_iter()
        .filter(|(_, _, _, is_override)| *is_override)
        .map(|(k, _, cur, _)| (k, cur))
        .collect();

    // Filtros de palabras con sus líneas de announce.
    let word_filters: Vec<WordFilterEntry> = ctx
        .word_filter
        .list()
        .into_iter()
        .map(|(pattern, action, args)| {
            let lines = ctx.word_filter.view(&pattern).unwrap_or_default();
            WordFilterEntry {
                pattern,
                action: action.as_str().to_string(),
                args,
                lines,
            }
        })
        .collect();

    // Flags que difieren del default.
    let room_flags: BTreeMap<String, bool> = ctx
        .room_flags
        .list()
        .into_iter()
        .filter(|(k, v)| {
            server_core::room_flags::FLAG_DEFAULTS
                .iter()
                .find(|(dk, _)| dk == k)
                .map(|(_, dv)| dv != v)
                .unwrap_or(true)
        })
        .collect();

    // Solo overrides de nivel de comando.
    let command_levels: Vec<CommandLevelEntry> = ctx
        .command_levels
        .list()
        .into_iter()
        .filter(|(_, _, is_override)| *is_override)
        .map(|(command, level, _)| CommandLevelEntry {
            command,
            level: level as u8,
        })
        .collect();

    // Auto-logins: la DB expone el guid (que el manager no lista).
    let autologins: Vec<AutologinEntry> = ctx
        .db
        .list_ip_autologins()
        .unwrap_or_default()
        .into_iter()
        .map(|(_id, guid, name, level, ip)| AutologinEntry {
            guid,
            name,
            level,
            ip,
        })
        .collect();

    // Bots: config completa (incluye secretos del LLM).
    let bots: Vec<serde_json::Value> = ctx
        .bots
        .read()
        .iter()
        .map(|b| serde_json::from_str(&b.config_json()).unwrap_or_else(|_| serde_json::json!({})))
        .collect();

    RoomProfile {
        kind: PROFILE_KIND.to_string(),
        schema: PROFILE_SCHEMA,
        astra_version: server_core::VERSION.to_string(),
        exported_at: server_core::time::unix_time(),
        room: Some(RoomMeta {
            room_name: ctx.settings.room_name.clone(),
            bot_name: ctx.settings.bot_name.clone(),
            topic: ctx.current_room_topic(),
            status: ctx.room_status(),
        }),
        templates: Some(templates),
        motd: Some(ctx.motd.text()),
        greets: Some(ctx.greets.list()),
        word_filters: Some(word_filters),
        filter_enabled: Some(ctx.word_filter.is_enabled()),
        join_filters: Some(ctx.join_filters.list()),
        file_filters: Some(ctx.file_filters.list()),
        urls: Some(
            ctx.urls
                .list()
                .into_iter()
                .map(|u| UrlEntry {
                    address: u.address,
                    text: u.text,
                })
                .collect(),
        ),
        room_flags: Some(room_flags),
        command_levels: Some(command_levels),
        autologins: Some(autologins),
        bots: Some(bots),
    }
}

/// Exporta el perfil actual como JSON (pretty).
pub fn export_json(ctx: &AppContext) -> String {
    let profile = build_profile(ctx);
    serde_json::to_string_pretty(&profile).unwrap_or_else(|_| "{}".to_string())
}

/// Aplica un perfil (replace) sobre el estado en vivo.
///
/// Cada sección presente en el archivo se limpia y se re-aplica; las secciones
/// ausentes se dejan intactas. Devuelve un resumen con lo aplicado y los
/// errores no fatales (no aborta el import por un único bot inválido).
pub fn import_json(ctx: &Arc<AppContext>, json: &str) -> Result<ImportReport, String> {
    let profile: RoomProfile =
        serde_json::from_str(json).map_err(|e| format!("invalid profile JSON: {}", e))?;

    if profile.kind != PROFILE_KIND {
        return Err(format!(
            "not an Astra room config (kind = {:?})",
            profile.kind
        ));
    }
    if profile.schema > PROFILE_SCHEMA {
        return Err(format!(
            "unsupported schema {} (this build understands up to {})",
            profile.schema, PROFILE_SCHEMA
        ));
    }

    let mut report = ImportReport::default();

    // ── Room: topic/status en vivo ───────────────────────────────────────
    if let Some(room) = &profile.room {
        ctx.set_room_topic(room.topic.clone());
        ctx.set_room_status(room.status.clone());
        ctx.persist_room_meta(Some(&room.topic), Some(&room.status));
        report.applied.insert("room".to_string(), 1);
    }

    // ── Textos del sistema (templates) ───────────────────────────────────
    if let Some(templates) = &profile.templates {
        for (key, _default, _cur, is_override) in ctx.templates.list() {
            if is_override {
                ctx.templates.reset(&key);
            }
        }
        let mut n = 0;
        for (key, text) in templates {
            if ctx.templates.set(key, text) {
                n += 1;
            }
        }
        report.applied.insert("templates".to_string(), n);
    }

    // ── MOTD ─────────────────────────────────────────────────────────────
    if let Some(motd) = &profile.motd {
        ctx.motd.set(motd);
        report.applied.insert("motd".to_string(), 1);
    }

    // ── Greetings ────────────────────────────────────────────────────────
    if let Some(greets) = &profile.greets {
        while !ctx.greets.is_empty() {
            ctx.greets.remove_at(0);
        }
        for g in greets {
            ctx.greets.add(g);
        }
        report.applied.insert("greets".to_string(), greets.len());
    }

    // ── Filtros de palabras ──────────────────────────────────────────────
    if let Some(filters) = &profile.word_filters {
        for (pattern, _, _) in ctx.word_filter.list() {
            ctx.word_filter.remove(&pattern);
        }
        let mut n = 0;
        for e in filters {
            let action = FilterAction::parse(&e.action);
            ctx.word_filter.add(&e.pattern, action, &e.args);
            if action == FilterAction::Announce {
                for line in &e.lines {
                    if let Err(err) = ctx.word_filter.add_line(&e.pattern, line) {
                        report
                            .errors
                            .push(format!("word filter '{}': {}", e.pattern, err));
                    }
                }
            }
            n += 1;
        }
        report.applied.insert("wordFilters".to_string(), n);
    }
    if let Some(enabled) = profile.filter_enabled {
        ctx.word_filter.set_enabled(enabled);
        report
            .applied
            .insert("filterEnabled".to_string(), usize::from(enabled));
    }

    // ── Filtros de nombre (join / file) ──────────────────────────────────
    if let Some(join) = &profile.join_filters {
        for p in ctx.join_filters.list() {
            ctx.join_filters.remove(&p);
        }
        for p in join {
            ctx.join_filters.add(p);
        }
        report.applied.insert("joinFilters".to_string(), join.len());
    }
    if let Some(file) = &profile.file_filters {
        for p in ctx.file_filters.list() {
            ctx.file_filters.remove(&p);
        }
        for p in file {
            ctx.file_filters.add(p);
        }
        report.applied.insert("fileFilters".to_string(), file.len());
    }

    // ── URLs rotadas ─────────────────────────────────────────────────────
    if let Some(urls) = &profile.urls {
        while !ctx.urls.is_empty() {
            ctx.urls.remove_at(0);
        }
        for u in urls {
            ctx.urls.add(&u.address, &u.text);
        }
        report.applied.insert("urls".to_string(), urls.len());
    }

    // ── Flags de sala ────────────────────────────────────────────────────
    if let Some(flags) = &profile.room_flags {
        // Reponer cada flag a su default y luego aplicar los del perfil.
        for (k, def) in server_core::room_flags::FLAG_DEFAULTS {
            ctx.room_flags.set(k, *def);
        }
        let mut n = 0;
        for (k, v) in flags {
            if ctx.room_flags.set(k, *v) {
                n += 1;
            }
        }
        report.applied.insert("roomFlags".to_string(), n);
    }

    // ── Niveles de comando ───────────────────────────────────────────────
    if let Some(levels) = &profile.command_levels {
        for (name, _level, is_override) in ctx.command_levels.list() {
            if is_override {
                ctx.command_levels.reset(&name);
            }
        }
        let mut n = 0;
        for e in levels {
            if ctx.command_levels.set(&e.command, ilevel_from_u8(e.level)) {
                n += 1;
            } else {
                report
                    .errors
                    .push(format!("command level '{}' is not managed", e.command));
            }
        }
        report.applied.insert("commandLevels".to_string(), n);
    }

    // ── Auto-logins por IP ───────────────────────────────────────────────
    if let Some(autologins) = &profile.autologins {
        let ids: Vec<i64> = ctx
            .ip_autologins
            .list()
            .into_iter()
            .map(|(id, ..)| id)
            .collect();
        for id in ids {
            ctx.ip_autologins.remove(id);
        }
        let mut n = 0;
        for e in autologins {
            let Some(guid) = guid_from_hex(&e.guid) else {
                report
                    .errors
                    .push(format!("autologin '{}': invalid guid", e.name));
                continue;
            };
            let Ok(ip) = e.ip.trim().parse::<IpAddr>() else {
                report
                    .errors
                    .push(format!("autologin '{}': invalid ip '{}'", e.name, e.ip));
                continue;
            };
            match ctx
                .ip_autologins
                .add(&guid, &e.name, ilevel_from_u8(e.level), ip)
            {
                Ok(()) => n += 1,
                Err(err) => report
                    .errors
                    .push(format!("autologin '{}': {}", e.name, err)),
            }
        }
        report.applied.insert("autologins".to_string(), n);
    }

    // ── Bots agente ──────────────────────────────────────────────────────
    if let Some(bots) = &profile.bots {
        let ids: Vec<i64> = ctx.bots.read().iter().map(|b| b.bot_id()).collect();
        for id in ids {
            if let Err(e) = crate::admin::delete_bot(ctx, id) {
                report.errors.push(format!("bot #{}: {}", id, e));
            }
        }
        let mut n = 0;
        for cfg in bots {
            let json = serde_json::to_string(cfg).unwrap_or_else(|_| "{}".to_string());
            match crate::admin::create_bot(ctx, &json) {
                Ok(_) => n += 1,
                Err(e) => report.errors.push(format!("bot: {}", e)),
            }
        }
        report.applied.insert("bots".to_string(), n);
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;
    use server_core::settings::Settings;

    fn ctx() -> Arc<AppContext> {
        Arc::new(AppContext::new(
            Settings::default(),
            Database::in_memory().unwrap(),
        ))
    }

    #[test]
    fn rejects_foreign_json() {
        let ctx = ctx();
        let err = import_json(&ctx, r#"{"kind":"otra.cosa","schema":1}"#).unwrap_err();
        assert!(err.contains("not an Astra room config"), "err: {err}");
        assert!(import_json(&ctx, "not json").is_err());
    }

    #[test]
    fn rejects_future_schema() {
        let ctx = ctx();
        let err = import_json(
            &ctx,
            &format!(r#"{{"kind":"{}","schema":99}}"#, PROFILE_KIND),
        )
        .unwrap_err();
        assert!(err.contains("unsupported schema"), "err: {err}");
    }

    #[test]
    fn roundtrip_restores_sections() {
        let src = ctx();
        src.templates.set("kick.confirm", "Fuera +n");
        src.motd.set("Hola +n");
        src.greets.add("bienvenido +n");
        src.word_filter.add("malo", FilterAction::Ban, "");
        src.word_filter.add("saluda", FilterAction::Announce, "");
        let _ = src.word_filter.add_line("saluda", "hola!");
        src.join_filters.add("spam*");
        src.file_filters.add("*.exe");
        src.urls.add("https://a.com", "A");
        src.room_flags.set("caps", true);
        src.command_levels.set("kick", ILevel::Admin);
        src.set_room_topic("tema nuevo");
        src.set_room_status("estado");

        let json = export_json(&src);

        let dst = ctx();
        let report = import_json(&dst, &json).unwrap();
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);

        assert_eq!(dst.templates.get("kick.confirm"), "Fuera +n");
        assert_eq!(dst.motd.text(), "Hola +n");
        assert_eq!(dst.greets.list(), vec!["bienvenido +n".to_string()]);
        assert!(dst
            .word_filter
            .list()
            .iter()
            .any(|(p, a, _)| { p == "saluda" && *a == FilterAction::Announce }));
        assert_eq!(
            dst.word_filter.view("saluda"),
            Some(vec!["hola!".to_string()])
        );
        assert!(dst.join_filters.matches("SpamBot"));
        assert!(dst.file_filters.matches("virus.exe"));
        assert_eq!(dst.urls.list()[0].address, "https://a.com");
        assert!(dst.room_flags.get("caps"));
        assert_eq!(dst.command_levels.get("kick"), Some(ILevel::Admin));
        assert_eq!(dst.current_room_topic(), "tema nuevo");
        assert_eq!(dst.room_status(), "estado");
    }

    #[test]
    fn replace_clears_previous_entries() {
        let ctx = ctx();
        // Estado previo que NO está en el perfil.
        ctx.greets.add("viejo");
        ctx.word_filter.add("viejo", FilterAction::Ban, "");
        ctx.urls.add("https://viejo.com", "viejo");
        ctx.room_flags.set("caps", true);

        // Perfil mínimo con secciones presentes pero vacías.
        let json = format!(
            r#"{{"kind":"{}","schema":1,"greets":[],"wordFilters":[],"urls":[],"roomFlags":{{}}}}"#,
            PROFILE_KIND
        );
        import_json(&ctx, &json).unwrap();

        assert!(ctx.greets.is_empty());
        assert!(ctx.word_filter.is_empty());
        assert!(ctx.urls.is_empty());
        // roomFlags vacío = todos a default.
        assert!(!ctx.room_flags.get("caps"));
    }

    #[test]
    fn absent_sections_are_left_untouched() {
        let ctx = ctx();
        ctx.greets.add("se queda");
        let json = format!(r#"{{"kind":"{}","schema":1,"motd":"nuevo"}}"#, PROFILE_KIND);
        import_json(&ctx, &json).unwrap();
        assert_eq!(ctx.greets.list(), vec!["se queda".to_string()]);
        assert_eq!(ctx.motd.text(), "nuevo");
    }

    #[test]
    fn guid_and_level_helpers() {
        assert_eq!(
            guid_from_hex("00112233445566778899aabbccddeeff"),
            Some([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff
            ])
        );
        assert_eq!(guid_from_hex("zz"), None);
        assert_eq!(ilevel_from_u8(80), ILevel::Admin);
        assert_eq!(ilevel_from_u8(50), ILevel::Moderator);
        assert_eq!(ilevel_from_u8(3), ILevel::Voice);
    }
}
