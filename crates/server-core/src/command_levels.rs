//! Niveles de permiso configurables por comando, equivalente al sistema de
//! `[CommandLevel]` + registro de sb0t (`gui/CommandManager.cs`).
//!
//! sb0t define un nivel *default* por comando vía atributos en `Eval.cs`, y
//! permite al operador sobreescribirlo en runtime desde la GUI (persistido en
//! el registro de Windows). Astra no tiene GUI, así que el equivalente es:
//! una tabla de defaults ([`DEFAULT_COMMAND_LEVELS`], reflejando el gate que
//! cada comando ya tenía hardcodeado) + un override persistido en SQLite,
//! configurable en runtime vía `/cmdlevel` (ver `astra-commands`).
//!
//! Solo los comandos listados en `DEFAULT_COMMAND_LEVELS` son gateados por
//! este manager; cualquier otro nombre (p.ej. comandos registrados por
//! scripts) no es tocado por el gate centralizado.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;
use crate::types::ILevel;

/// Nivel mínimo default de cada comando built-in, reflejando el gate que el
/// handler ya aplicaba en el código antes de existir este manager. Cuando un
/// mismo handler sirve varios alias (p.ej. `kick`/`kill`), cada alias tiene su
/// propia entrada para poder configurarse independientemente, tal como sb0t
/// trata cada string de comando por separado.
pub const DEFAULT_COMMAND_LEVELS: &[(&str, ILevel)] = &[
    // Autoservicio / sin gate.
    ("help", ILevel::Regular),
    ("nick", ILevel::Regular),
    ("vroom", ILevel::Regular),
    ("customname", ILevel::Regular),
    ("uncustomname", ILevel::Regular),
    ("users", ILevel::Regular),
    ("topic", ILevel::Regular),
    ("motd", ILevel::Regular),
    ("viewmotd", ILevel::Regular),
    ("loadmotd", ILevel::Regular),
    ("whois", ILevel::Regular),
    ("uptime", ILevel::Regular),
    ("stats", ILevel::Regular),
    ("version", ILevel::Regular),
    ("register", ILevel::Regular),
    ("unregister", ILevel::Regular),
    ("rempassword", ILevel::Regular),
    ("whisper", ILevel::Regular),
    ("pmblock", ILevel::Regular),
    ("login", ILevel::Regular),
    ("id", ILevel::Regular),
    ("info", ILevel::Regular),
    ("roominfo", ILevel::Regular),
    ("status", ILevel::Regular),
    ("admins", ILevel::Regular),
    // Moderator+.
    ("ban", ILevel::Moderator),
    ("unban", ILevel::Moderator),
    ("ban10", ILevel::Moderator),
    ("ban60", ILevel::Moderator),
    ("banlist", ILevel::Moderator),
    ("listbans", ILevel::Moderator),
    ("kick", ILevel::Moderator),
    ("kill", ILevel::Moderator),
    ("muzzle", ILevel::Moderator),
    ("unmuzzle", ILevel::Moderator),
    ("history", ILevel::Moderator),
    ("whowas", ILevel::Moderator),
    ("lastseen", ILevel::Moderator),
    ("banstats", ILevel::Moderator),
    ("oldname", ILevel::Moderator),
    ("changemessage", ILevel::Moderator),
    ("announce", ILevel::Moderator),
    ("shout", ILevel::Moderator),
    ("opmsg", ILevel::Moderator),
    ("adminmsg", ILevel::Moderator),
    ("adminannounce", ILevel::Moderator),
    ("echo", ILevel::Moderator),
    ("unecho", ILevel::Moderator),
    ("clone", ILevel::Moderator),
    ("kiddy", ILevel::Moderator),
    ("unkiddy", ILevel::Moderator),
    ("mtimeout", ILevel::Moderator),
    ("clearscreen", ILevel::Moderator),
    ("locate", ILevel::Moderator),
    ("customnames", ILevel::Moderator),
    ("roomflags", ILevel::Moderator),
    ("cloak", ILevel::Moderator),
    ("lower", ILevel::Moderator),
    ("unlower", ILevel::Moderator),
    ("kewltext", ILevel::Moderator),
    ("unkewltext", ILevel::Moderator),
    ("paint", ILevel::Moderator),
    ("unpaint", ILevel::Moderator),
    ("addtopic", ILevel::Moderator),
    ("remtopic", ILevel::Moderator),
    ("define", ILevel::Moderator),
    ("urban", ILevel::Moderator),
    ("trace", ILevel::Moderator),
    ("vspy", ILevel::Moderator),
    ("ipsend", ILevel::Moderator),
    ("logsend", ILevel::Moderator),
    ("bansend", ILevel::Moderator),
    ("loadtemplate", ILevel::Moderator),
    // Admin+.
    ("pmall", ILevel::Admin),
    ("pmroom", ILevel::Admin),
    ("grant", ILevel::Admin),
    ("revoke", ILevel::Admin),
    ("addfilter", ILevel::Admin),
    ("remfilter", ILevel::Admin),
    ("listfilters", ILevel::Admin),
    ("wordfilters", ILevel::Admin),
    ("viewfilter", ILevel::Admin),
    ("addwordfilter", ILevel::Admin),
    ("remwordfilter", ILevel::Admin),
    ("filter", ILevel::Admin),
    ("url", ILevel::Admin),
    ("addurl", ILevel::Admin),
    ("remurl", ILevel::Admin),
    ("listurl", ILevel::Admin),
    ("listurls", ILevel::Admin),
    ("rangeban", ILevel::Admin),
    ("rangeunban", ILevel::Admin),
    ("listrangebans", ILevel::Admin),
    ("asnban", ILevel::Admin),
    ("asnunban", ILevel::Admin),
    ("listasnbans", ILevel::Admin),
    ("clearbans", ILevel::Admin),
    ("cbans", ILevel::Admin),
    ("move", ILevel::Admin),
    ("changename", ILevel::Admin),
    ("redirect", ILevel::Admin),
    ("disableavatar", ILevel::Admin),
    ("caps", ILevel::Admin),
    ("anon", ILevel::Admin),
    ("general", ILevel::Admin),
    ("audios", ILevel::Admin),
    ("buzzes", ILevel::Admin),
    ("scribbles", ILevel::Admin),
    ("colors", ILevel::Admin),
    ("sharefiles", ILevel::Admin),
    ("roomsearch", ILevel::Admin),
    ("avatars", ILevel::Admin),
    ("stealth", ILevel::Admin),
    ("clock", ILevel::Admin),
    ("idle", ILevel::Admin),
    ("listquarantined", ILevel::Admin),
    ("unquarantine", ILevel::Admin),
    ("joinfilter", ILevel::Admin),
    ("joinfilters", ILevel::Admin),
    ("filefilter", ILevel::Admin),
    ("filefilters", ILevel::Admin),
    ("addjoinfilter", ILevel::Admin),
    ("remjoinfilter", ILevel::Admin),
    ("addfilefilter", ILevel::Admin),
    ("remfilefilter", ILevel::Admin),
    ("greets", ILevel::Admin),
    ("addgreet", ILevel::Admin),
    ("remgreet", ILevel::Admin),
    ("listgreets", ILevel::Admin),
    ("greetmsg", ILevel::Admin),
    ("addgreetmsg", ILevel::Admin),
    ("pmgreetmsg", ILevel::Admin),
    ("remgreetmsg", ILevel::Admin),
    ("listgreetmsg", ILevel::Admin),
    // Owner (equivalente a "Host" en sb0t: no hay tier intermedio en Astra).
    // `cmdlevel` es Owner-only: permite reconfigurar los demás gates, así
    // que un Admin no debe poder usarlo para auto-escalar sus privilegios.
    ("cmdlevel", ILevel::Owner),
    ("hostban", ILevel::Owner),
    ("hostkick", ILevel::Owner),
    ("hostkill", ILevel::Owner),
    ("hostmuzzle", ILevel::Owner),
    ("hostunmuzzle", ILevel::Owner),
    ("hostunban", ILevel::Owner),
    ("hostcban", ILevel::Owner),
    ("hostclone", ILevel::Owner),
    ("disableadmins", ILevel::Owner),
    ("enableadmins", ILevel::Owner),
    ("listpasswords", ILevel::Owner),
    ("autologins", ILevel::Owner),
    ("link", ILevel::Owner),
    ("unlink", ILevel::Owner),
];

fn ilevel_from_u8(v: u8) -> Option<ILevel> {
    match v {
        0 => Some(ILevel::Anonymous),
        1 => Some(ILevel::Regular),
        2 => Some(ILevel::Voice),
        50 => Some(ILevel::Moderator),
        80 => Some(ILevel::Admin),
        100 => Some(ILevel::Owner),
        255 => Some(ILevel::System),
        _ => None,
    }
}

/// Manager de niveles de comando: defaults en código + overrides en SQLite.
pub struct CommandLevelManager {
    db: Arc<Database>,
    overrides: RwLock<HashMap<String, ILevel>>,
}

impl CommandLevelManager {
    /// Crea el manager cargando los overrides persistidos desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let overrides = db
            .list_command_levels()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, level)| ilevel_from_u8(level).map(|l| (name, l)))
            .collect();
        Self {
            db,
            overrides: RwLock::new(overrides),
        }
    }

    /// ¿Es un nombre de comando gestionado por este sistema?
    pub fn is_managed(name: &str) -> bool {
        DEFAULT_COMMAND_LEVELS.iter().any(|(n, _)| *n == name)
    }

    /// Nivel default (hardcodeado) de un comando gestionado.
    pub fn default_level(name: &str) -> Option<ILevel> {
        DEFAULT_COMMAND_LEVELS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
    }

    /// Nivel requerido efectivo (override si existe, si no el default).
    ///
    /// Retorna `None` si `name` no es un comando gestionado (p.ej. un
    /// comando registrado por un script), en cuyo caso el llamador no debe
    /// aplicar ningún gate.
    pub fn get(&self, name: &str) -> Option<ILevel> {
        if let Some(level) = self.overrides.read().get(name) {
            return Some(*level);
        }
        Self::default_level(name)
    }

    /// Sobreescribe el nivel de un comando gestionado. Retorna `false` si el
    /// nombre no es gestionado.
    pub fn set(&self, name: &str, level: ILevel) -> bool {
        if !Self::is_managed(name) {
            return false;
        }
        self.overrides.write().insert(name.to_string(), level);
        let _ = self.db.set_command_level(name, level as u8);
        true
    }

    /// Elimina el override de un comando, revirtiendo a su default. Retorna
    /// `false` si el nombre no es gestionado o no tenía override.
    pub fn reset(&self, name: &str) -> bool {
        if !Self::is_managed(name) {
            return false;
        }
        let had = self.overrides.write().remove(name).is_some();
        if had {
            let _ = self.db.remove_command_level(name);
        }
        had
    }

    /// Lista todos los comandos gestionados: `(nombre, nivel efectivo,
    /// tiene_override)`, en el orden de [`DEFAULT_COMMAND_LEVELS`].
    pub fn list(&self) -> Vec<(String, ILevel, bool)> {
        let overrides = self.overrides.read();
        DEFAULT_COMMAND_LEVELS
            .iter()
            .map(|(name, default)| match overrides.get(*name) {
                Some(level) => (name.to_string(), *level, true),
                None => (name.to_string(), *default, false),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn defaults_match_table() {
        let mgr = CommandLevelManager::new(mem_db());
        assert_eq!(mgr.get("ban"), Some(ILevel::Moderator));
        assert_eq!(mgr.get("hostban"), Some(ILevel::Owner));
        assert_eq!(mgr.get("help"), Some(ILevel::Regular));
        assert_eq!(mgr.get("not_a_real_command"), None);
    }

    #[test]
    fn set_overrides_and_persists() {
        let db = mem_db();
        {
            let mgr = CommandLevelManager::new(db.clone());
            assert!(mgr.set("ban", ILevel::Admin));
            assert_eq!(mgr.get("ban"), Some(ILevel::Admin));
            assert!(!mgr.set("not_a_real_command", ILevel::Owner));
        }
        let mgr2 = CommandLevelManager::new(db);
        assert_eq!(mgr2.get("ban"), Some(ILevel::Admin));
    }

    #[test]
    fn reset_reverts_to_default() {
        let mgr = CommandLevelManager::new(mem_db());
        assert!(mgr.set("ban", ILevel::Admin));
        assert!(mgr.reset("ban"));
        assert_eq!(mgr.get("ban"), Some(ILevel::Moderator));
        assert!(!mgr.reset("ban"));
    }

    #[test]
    fn list_covers_all_defaults() {
        let mgr = CommandLevelManager::new(mem_db());
        assert_eq!(mgr.list().len(), DEFAULT_COMMAND_LEVELS.len());
        assert!(mgr.list().iter().all(|(_, _, overridden)| !overridden));
    }

    #[test]
    fn no_duplicate_names_in_table() {
        let mut names: Vec<&str> = DEFAULT_COMMAND_LEVELS.iter().map(|(n, _)| *n).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "hay nombres de comando duplicados en DEFAULT_COMMAND_LEVELS");
    }
}
