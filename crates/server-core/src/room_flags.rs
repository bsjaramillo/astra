//! Flags de sala (toggles `on|off`), equivalente a los `Settings.*` de sb0t
//! que controlan permisos de la sala: caps, anon, general, audios, buzzes,
//! scribbles, colors, sharefiles, roomsearch, avatars, stealth.
//!
//! Persistidos en la tabla `room_flags`. Cada uno es un booleano con un
//! default; el manager expone get/set/toggle y persiste al cambiar.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Los nombres de flag válidos y su valor por defecto.
///
/// Semántica (paridad sb0t):
/// - `caps`: si `true`, los mensajes TODO-EN-MAYÚSCULAS se pasan a minúsculas.
/// - `anon`: monitoreo de usuarios anónimos (sin archivos).
/// - `general`: chat general habilitado.
/// - `audios`: permite mensajes de voz.
/// - `buzzes`: permite nudges/buzzes.
/// - `scribbles`: permite scribbles (dibujos).
/// - `colors`: permite texto con color.
/// - `sharefiles`: monitoreo de compartición de archivos.
/// - `roomsearch`: la sala se anuncia en el room search UDP.
/// - `avatars`: permite avatares.
/// - `stealth`: oculta la identidad del admin en las acciones.
pub const FLAG_DEFAULTS: &[(&str, bool)] = &[
    ("caps", false),
    ("anon", false),
    ("general", true),
    ("audios", true),
    ("buzzes", true),
    ("scribbles", true),
    ("colors", true),
    ("sharefiles", false),
    ("roomsearch", true),
    ("avatars", true),
    ("stealth", false),
    ("clock", false),
    ("idle", false),
    // `history`: replay de los últimos mensajes al entrar a la sala
    // (paridad sb0t `Settings.History` + `commands/History.Show`).
    ("history", false),
    // `greetmsg`: greet PÚBLICO al entrar (sb0t `Settings.GreetMsg`).
    ("greetmsg", false),
    // `pmgreetmsg`: greet por PM al entrar (sb0t `Settings.PMGreetMsg`).
    // Default on: preserva el comportamiento histórico de Astra (greet=PM).
    ("pmgreetmsg", true),
    // `adminannounce`: los word-filters tipo Announce no disparan para
    // usuarios regulares (sb0t `Settings.AdminAnnounce`, WordFilter.cs:195).
    ("adminannounce", false),
    // `roominfo`: broadcast periódico (20 min) del bloque de info de sala
    // (sb0t `Settings.RoomInfo` + `commands/RoomInfo.Tick`).
    ("roominfo", false),
    // `lastseen`: al entrar un usuario, anuncia con qué nick y cuándo se lo
    // vio por última vez (sb0t `Settings.LastSeen`, ServerEvents.cs:198).
    ("lastseen", false),
    // `customnames`: permite custom names (sb0t `Settings.Get<bool>("customnames")`,
    // toggle `#customnames on|off` Host, expuesto a scripts como `Room.customNames`).
    // sb0t default: false (Settings.Get<bool> sin valor guardado).
    ("customnames", false),
];

/// Manager de flags de sala: cache en memoria + persistencia SQLite.
pub struct RoomFlags {
    db: Arc<Database>,
    flags: RwLock<HashMap<String, bool>>,
}

impl RoomFlags {
    /// Crea el manager con los defaults, sobreescritos por lo persistido.
    pub fn new(db: Arc<Database>) -> Self {
        let mut flags: HashMap<String, bool> =
            FLAG_DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        for (k, v) in db.list_room_flags().unwrap_or_default() {
            flags.insert(k, v);
        }
        Self {
            db,
            flags: RwLock::new(flags),
        }
    }

    /// ¿Es un nombre de flag válido?
    pub fn is_valid(key: &str) -> bool {
        FLAG_DEFAULTS.iter().any(|(k, _)| *k == key)
    }

    /// Valor de un flag (o su default si no existe).
    pub fn get(&self, key: &str) -> bool {
        self.flags
            .read()
            .get(key)
            .copied()
            .unwrap_or_else(|| FLAG_DEFAULTS.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).unwrap_or(false))
    }

    /// Setea un flag (y lo persiste). Retorna `false` si el nombre es inválido.
    pub fn set(&self, key: &str, value: bool) -> bool {
        if !Self::is_valid(key) {
            return false;
        }
        self.flags.write().insert(key.to_string(), value);
        let _ = self.db.set_room_flag(key, value);
        true
    }

    /// Lista todos los flags como `(key, value)` en orden de `FLAG_DEFAULTS`.
    pub fn list(&self) -> Vec<(String, bool)> {
        let flags = self.flags.read();
        FLAG_DEFAULTS
            .iter()
            .map(|(k, def)| (k.to_string(), flags.get(*k).copied().unwrap_or(*def)))
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
    fn defaults_applied() {
        let f = RoomFlags::new(mem_db());
        assert!(!f.get("caps"));
        assert!(f.get("scribbles"));
        assert!(f.get("avatars"));
    }

    #[test]
    fn set_and_persist() {
        let db = mem_db();
        {
            let f = RoomFlags::new(db.clone());
            assert!(f.set("caps", true));
            assert!(!f.set("nonexistent", true));
        }
        let f2 = RoomFlags::new(db);
        assert!(f2.get("caps"));
    }

    #[test]
    fn list_covers_all() {
        let f = RoomFlags::new(mem_db());
        assert_eq!(f.list().len(), FLAG_DEFAULTS.len());
    }

    #[test]
    fn invalid_name_rejected() {
        assert!(!RoomFlags::is_valid("bogus"));
        assert!(RoomFlags::is_valid("scribbles"));
    }
}
