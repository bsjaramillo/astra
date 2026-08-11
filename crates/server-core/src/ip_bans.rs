//! Range bans (prefijos de IP) y ASN bans, equivalentes a
//! `commands/RangeBans.cs` y `commands/AsnBans.cs` de sb0t.
//!
//! - **Range ban**: una lista de prefijos de string; una IP está baneada si
//!   `ip.to_string().starts_with(prefix)` (idéntico a sb0t).
//! - **ASN ban**: una lista de números de ASN; un usuario está baneado si su
//!   ASN (resuelto externamente, en `AresUser.asn_cache`) está en la lista.
//!
//! Ambos persistidos en SQLite.

use std::net::IpAddr;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Manager de range bans (prefijos de IP).
pub struct RangeBanManager {
    db: Arc<Database>,
    cache: RwLock<Vec<String>>,
}

impl RangeBanManager {
    /// Crea el manager cargando los prefijos desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let cache = db.list_range_bans().unwrap_or_default();
        Self {
            db,
            cache: RwLock::new(cache),
        }
    }

    /// Cantidad de range bans.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// ¿No hay range bans?
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    /// Agrega un prefijo (se normaliza quitando `*` y espacios). Retorna
    /// `true` si era nuevo y no vacío.
    pub fn add(&self, prefix: &str) -> bool {
        let prefix = prefix.replace('*', "").replace('"', "").trim().to_string();
        if prefix.is_empty() {
            return false;
        }
        let is_new = self.db.add_range_ban(&prefix).unwrap_or(false);
        if is_new {
            self.cache.write().push(prefix);
        }
        is_new
    }

    /// Elimina un prefijo exacto. Retorna `true` si existía.
    pub fn remove(&self, prefix: &str) -> bool {
        let prefix = prefix.replace('*', "").replace('"', "").trim().to_string();
        let mut cache = self.cache.write();
        let before = cache.len();
        cache.retain(|p| *p != prefix);
        if cache.len() != before {
            let _ = self.db.remove_range_ban(&prefix);
            true
        } else {
            false
        }
    }

    /// Borra todos los range bans. Retorna cuántos había.
    pub fn clear(&self) -> usize {
        let mut cache = self.cache.write();
        let n = cache.len();
        cache.clear();
        let _ = self.db.clear_range_bans();
        n
    }

    /// Elimina por índice visible. Retorna el prefijo borrado.
    pub fn remove_at(&self, index: usize) -> Option<String> {
        let mut cache = self.cache.write();
        if index >= cache.len() {
            return None;
        }
        let prefix = cache.remove(index);
        let _ = self.db.remove_range_ban(&prefix);
        Some(prefix)
    }

    /// Lista los prefijos.
    pub fn list(&self) -> Vec<String> {
        self.cache.read().clone()
    }

    /// ¿Está la IP dentro de algún prefijo baneado?
    pub fn is_banned(&self, ip: IpAddr) -> bool {
        let s = ip.to_string();
        self.cache.read().iter().any(|p| s.starts_with(p.as_str()))
    }
}

/// Manager de ASN bans.
pub struct AsnBanManager {
    db: Arc<Database>,
    cache: RwLock<Vec<u32>>,
}

impl AsnBanManager {
    /// Crea el manager cargando los ASN desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let cache = db.list_asn_bans().unwrap_or_default();
        Self {
            db,
            cache: RwLock::new(cache),
        }
    }

    /// Cantidad de ASN baneados.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// ¿No hay ASN bans?
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    /// Agrega un ASN. Retorna `true` si era nuevo.
    pub fn add(&self, asn: u32) -> bool {
        if asn == 0 {
            return false;
        }
        let is_new = self.db.add_asn_ban(asn).unwrap_or(false);
        if is_new {
            self.cache.write().push(asn);
        }
        is_new
    }

    /// Elimina un ASN. Retorna `true` si existía.
    pub fn remove(&self, asn: u32) -> bool {
        let mut cache = self.cache.write();
        let before = cache.len();
        cache.retain(|a| *a != asn);
        if cache.len() != before {
            let _ = self.db.remove_asn_ban(asn);
            true
        } else {
            false
        }
    }

    /// Elimina el ASN en la posición de índice. Retorna el ASN eliminado
    /// o `None` si el índice es inválido (paridad sb0t `AsnBans.RemoveIndex`).
    pub fn remove_at(&self, index: usize) -> Option<u32> {
        let mut cache = self.cache.write();
        if index >= cache.len() {
            return None;
        }
        let asn = cache.remove(index);
        let _ = self.db.remove_asn_ban(asn);
        Some(asn)
    }

    /// Lista los ASN.
    pub fn list(&self) -> Vec<u32> {
        self.cache.read().clone()
    }

    /// ¿Está el ASN baneado?
    pub fn is_banned(&self, asn: u32) -> bool {
        asn != 0 && self.cache.read().contains(&asn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn range_ban_prefix_match() {
        let m = RangeBanManager::new(mem_db());
        assert!(m.add("1.2.3"));
        assert!(!m.add("1.2.3"), "duplicado no cuenta");
        assert!(m.is_banned("1.2.3.4".parse().unwrap()));
        assert!(m.is_banned("1.2.3.99".parse().unwrap()));
        assert!(!m.is_banned("1.2.4.4".parse().unwrap()));
    }

    #[test]
    fn range_ban_strips_wildcards() {
        let m = RangeBanManager::new(mem_db());
        m.add("5.6.*");
        assert_eq!(m.list(), vec!["5.6."]);
        assert!(m.is_banned("5.6.7.8".parse().unwrap()));
    }

    #[test]
    fn range_ban_remove() {
        let m = RangeBanManager::new(mem_db());
        m.add("10.0");
        assert!(m.remove("10.0"));
        assert!(!m.remove("10.0"));
        assert!(!m.is_banned("10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn range_ban_persists() {
        let db = mem_db();
        {
            RangeBanManager::new(db.clone()).add("192.168");
        }
        let m2 = RangeBanManager::new(db);
        assert!(m2.is_banned("192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn asn_ban_match() {
        let m = AsnBanManager::new(mem_db());
        assert!(m.add(64500));
        assert!(!m.add(64500));
        assert!(m.is_banned(64500));
        assert!(!m.is_banned(1234));
        assert!(!m.is_banned(0));
        assert!(m.remove(64500));
        assert!(!m.is_banned(64500));
    }

    #[test]
    fn asn_ban_persists() {
        let db = mem_db();
        {
            AsnBanManager::new(db.clone()).add(13335);
        }
        let m2 = AsnBanManager::new(db);
        assert!(m2.is_banned(13335));
    }
}
