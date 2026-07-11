//! Auto-nivel por reconocimiento de IP+GUID, sin cuenta ni login (paridad
//! `commands/AutoLogin.cs` de sb0t: `/addautologin`, `/remautologin`,
//! `/autologins`).
//!
//! Distinto del autologin por GUID vía cuentas registradas
//! (`AccountManager`/`dispatch_autologin`): esto es una lista separada,
//! pensada para reconocer "la misma persona reconectando" sin que se haya
//! registrado nunca — un admin la usa para otorgarle un nivel a un usuario
//! conectado, y ese nivel se restaura solo la próxima vez que se conecte
//! desde (aproximadamente) la misma IP.

use std::net::IpAddr;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;
use crate::types::ILevel;

/// Una entrada de auto-nivel por IP.
#[derive(Debug, Clone)]
pub struct IpAutologinEntry {
    /// Id persistido (usado por `/remautologin <id>` y en el listado).
    pub id: i64,
    /// GUID del cliente (hex), tal como lo manda el login.
    pub guid: String,
    /// Nombre del usuario al momento de otorgar el nivel (solo informativo).
    pub name: String,
    /// Nivel a auto-otorgar.
    pub level: ILevel,
    /// Última IP externa vista.
    pub ip: IpAddr,
}

fn guid_hex(guid: &[u8; 16]) -> String {
    guid.iter().map(|b| format!("{:02x}", b)).collect()
}

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

/// ¿Comparten los primeros dos octetos? (aproximación de "misma red /16",
/// paridad del match de sb0t sobre `client_bytes[0]`/`[1]`).
fn same_ip_range(a: IpAddr, b: IpAddr) -> bool {
    match (a, b) {
        (IpAddr::V4(a), IpAddr::V4(b)) => a.octets()[..2] == b.octets()[..2],
        _ => false,
    }
}

/// Manager de auto-logins por IP: cache en memoria + persistencia SQLite.
pub struct IpAutologinManager {
    db: Arc<Database>,
    entries: RwLock<Vec<IpAutologinEntry>>,
}

impl IpAutologinManager {
    /// Crea el manager cargando las entradas existentes desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let entries = db
            .list_ip_autologins()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, guid, name, level, ip)| {
                Some(IpAutologinEntry {
                    id,
                    guid,
                    name,
                    level: ilevel_from_u8(level),
                    ip: ip.parse().ok()?,
                })
            })
            .collect();
        Self {
            db,
            entries: RwLock::new(entries),
        }
    }

    /// Agrega o actualiza (self-healing de guid/ip, paridad `AutoLogin.Add`
    /// de sb0t) una entrada. Rechaza cualquier nivel que no sea
    /// Moderator/Admin — nunca Owner (ni Regular/Voice, que no tendría
    /// sentido auto-otorgar): paridad del rango `byte 1-3` de sb0t, que
    /// deliberadamente no permite auto-otorgar el nivel más alto vía mero
    /// reconocimiento de IP.
    pub fn add(&self, guid: &[u8; 16], name: &str, level: ILevel, ip: IpAddr) -> Result<(), String> {
        if !matches!(level, ILevel::Moderator | ILevel::Admin) {
            return Err("level must be moderator or admin".to_string());
        }
        let guid_str = guid_hex(guid);
        let mut entries = self.entries.write();

        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.guid == guid_str && same_ip_range(e.ip, ip))
        {
            entry.ip = ip;
            entry.level = level;
            entry.name = name.to_string();
            let _ = self
                .db
                .update_ip_autologin(entry.id, &guid_str, name, level as u8, &ip.to_string());
            return Ok(());
        }

        if let Some(entry) = entries.iter_mut().find(|e| e.ip == ip) {
            entry.guid = guid_str.clone();
            entry.level = level;
            entry.name = name.to_string();
            let _ = self
                .db
                .update_ip_autologin(entry.id, &guid_str, name, level as u8, &ip.to_string());
            return Ok(());
        }

        let id = self
            .db
            .add_ip_autologin(&guid_str, name, level as u8, &ip.to_string())
            .map_err(|e| e.to_string())?;
        entries.push(IpAutologinEntry {
            id,
            guid: guid_str,
            name: name.to_string(),
            level,
            ip,
        });
        Ok(())
    }

    /// Busca el nivel auto-otorgado para esta guid+ip (mismo matching de
    /// dos niveles que `add`, con el mismo self-healing de la entrada
    /// encontrada). Retorna `None` si no hay ninguna entrada que matchee.
    pub fn get_level(&self, guid: &[u8; 16], ip: IpAddr) -> Option<ILevel> {
        let guid_str = guid_hex(guid);
        let mut entries = self.entries.write();

        if let Some(entry) = entries
            .iter_mut()
            .find(|e| e.guid == guid_str && same_ip_range(e.ip, ip))
        {
            if entry.ip != ip {
                entry.ip = ip;
                let _ = self.db.update_ip_autologin(
                    entry.id,
                    &guid_str,
                    &entry.name,
                    entry.level as u8,
                    &ip.to_string(),
                );
            }
            return Some(entry.level);
        }

        if let Some(entry) = entries.iter_mut().find(|e| e.ip == ip) {
            if entry.guid != guid_str {
                entry.guid = guid_str.clone();
                let _ = self.db.update_ip_autologin(
                    entry.id,
                    &guid_str,
                    &entry.name,
                    entry.level as u8,
                    &ip.to_string(),
                );
            }
            return Some(entry.level);
        }

        None
    }

    /// Elimina una entrada por id. Retorna `(guid, ip)` (para que el
    /// caller pueda degradar a Regular a cualquier usuario conectado que
    /// matchee, paridad `AutoLogin.Remove` de sb0t).
    pub fn remove(&self, id: i64) -> Option<(String, IpAddr)> {
        let mut entries = self.entries.write();
        let pos = entries.iter().position(|e| e.id == id)?;
        let entry = entries.remove(pos);
        let _ = self.db.remove_ip_autologin(id);
        Some((entry.guid, entry.ip))
    }

    /// Lista `(id, name, ip, level)` en el orden en que fueron creadas.
    pub fn list(&self) -> Vec<(i64, String, IpAddr, ILevel)> {
        self.entries
            .read()
            .iter()
            .map(|e| (e.id, e.name.clone(), e.ip, e.level))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn add_and_get_level_exact_ip() {
        let mgr = IpAutologinManager::new(mem_db());
        let guid = [1u8; 16];
        assert!(mgr.add(&guid, "Alice", ILevel::Moderator, ip("1.2.3.4")).is_ok());
        assert_eq!(mgr.get_level(&guid, ip("1.2.3.4")), Some(ILevel::Moderator));
    }

    #[test]
    fn get_level_matches_by_guid_and_ip_range() {
        let mgr = IpAutologinManager::new(mem_db());
        let guid = [2u8; 16];
        assert!(mgr.add(&guid, "Bob", ILevel::Admin, ip("5.6.7.8")).is_ok());
        // Misma /16, IP distinta, mismo guid -> matchea y se auto-actualiza.
        assert_eq!(mgr.get_level(&guid, ip("5.6.99.1")), Some(ILevel::Admin));
        // IP totalmente distinta con el mismo guid -> ya no matchea la vieja
        // entrada por rango, pero como se auto-actualizó a 5.6.99.1, una IP
        // fuera de esa /16 con el mismo guid no debe matchear.
        assert_eq!(mgr.get_level(&guid, ip("9.9.9.9")), None);
    }

    #[test]
    fn get_level_matches_by_exact_ip_different_guid() {
        let mgr = IpAutologinManager::new(mem_db());
        let guid_a = [3u8; 16];
        let guid_b = [4u8; 16];
        assert!(mgr.add(&guid_a, "Carol", ILevel::Moderator, ip("10.0.0.1")).is_ok());
        // Mismo IP exacto, guid distinto -> matchea por IP y se auto-cura.
        assert_eq!(mgr.get_level(&guid_b, ip("10.0.0.1")), Some(ILevel::Moderator));
    }

    #[test]
    fn get_level_no_match_returns_none() {
        let mgr = IpAutologinManager::new(mem_db());
        assert_eq!(mgr.get_level(&[9u8; 16], ip("8.8.8.8")), None);
    }

    #[test]
    fn owner_and_regular_rejected() {
        let mgr = IpAutologinManager::new(mem_db());
        let guid = [5u8; 16];
        assert!(mgr.add(&guid, "Dave", ILevel::Owner, ip("1.1.1.1")).is_err());
        assert!(mgr.add(&guid, "Dave", ILevel::Regular, ip("1.1.1.1")).is_err());
    }

    #[test]
    fn remove_and_list() {
        let db = mem_db();
        let mgr = IpAutologinManager::new(db.clone());
        let guid = [6u8; 16];
        mgr.add(&guid, "Eve", ILevel::Moderator, ip("2.2.2.2")).unwrap();
        let list = mgr.list();
        assert_eq!(list.len(), 1);
        let id = list[0].0;
        let removed = mgr.remove(id);
        assert!(removed.is_some());
        assert_eq!(mgr.list().len(), 0);
        assert_eq!(mgr.get_level(&guid, ip("2.2.2.2")), None);

        // Persistencia: un manager nuevo desde la misma DB no ve la entrada borrada.
        let mgr2 = IpAutologinManager::new(db);
        assert_eq!(mgr2.list().len(), 0);
    }

    #[test]
    fn persists_across_manager_instances() {
        let db = mem_db();
        {
            let mgr = IpAutologinManager::new(db.clone());
            mgr.add(&[7u8; 16], "Frank", ILevel::Admin, ip("3.3.3.3")).unwrap();
        }
        let mgr2 = IpAutologinManager::new(db);
        assert_eq!(mgr2.get_level(&[7u8; 16], ip("3.3.3.3")), Some(ILevel::Admin));
    }
}
