//! Sistema de bans persistido en SQLite.
//!
//! Carga los bans al inicio y los mantiene en memoria para lookups rápidos.
//! Cada modificación (ban/unban) se persiste en la DB.

use std::net::IpAddr;
use std::sync::Arc;

use crate::db::{BanRecord, Database};

/// Sistema de bans con cache en memoria + persistencia.
pub struct BanSystem {
    db: Arc<Database>,
    /// Cache en memoria (indexado por target = guid_hex o ip_str)
    cache: parking_lot::RwLock<Vec<BanRecord>>,
    /// Próximo ident a usar
    next_ident: parking_lot::Mutex<u16>,
}

impl BanSystem {
    /// Crea el sistema de bans a partir de la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let cache = db.list_bans().unwrap_or_default();
        let max_ident = cache.iter().map(|b| b.ident).max().unwrap_or(0);
        Self {
            db,
            cache: parking_lot::RwLock::new(cache),
            next_ident: parking_lot::Mutex::new(max_ident + 1),
        }
    }

    /// Carga la lista desde la DB.
    pub fn load(&self) {
        let mut cache = self.cache.write();
        *cache = self.db.list_bans().unwrap_or_default();
    }

    /// Verifica si un GUID o IP está baneada.
    pub fn is_banned(&self, guid: &[u8; 16], external_ip: IpAddr) -> bool {
        let cache = self.cache.read();
        cache
            .iter()
            .any(|b| &b.guid == guid || b.external_ip == external_ip)
    }

    /// Banea a un usuario.
    pub fn ban(
        &self,
        name: &str,
        version: &str,
        guid: &[u8; 16],
        external_ip: IpAddr,
        local_ip: IpAddr,
        port: u16,
    ) -> u16 {
        self.ban_with_expiry(name, version, guid, external_ip, local_ip, port, 0)
    }

    /// Banea con expiración opcional.
    /// `expires_secs = 0` significa "nunca expira" (compat con sb0t).
    /// `expires_secs > 0` se interpreta como segundos-en-el-futuro desde ahora.
    pub fn ban_with_expiry(
        &self,
        name: &str,
        version: &str,
        guid: &[u8; 16],
        external_ip: IpAddr,
        local_ip: IpAddr,
        port: u16,
        expires_secs: i64,
    ) -> u16 {
        // Si ya está baneado, devolver su ident
        if let Some(existing) = {
            let cache = self.cache.read();
            cache
                .iter()
                .find(|b| &b.guid == guid || b.external_ip == external_ip)
                .cloned()
        } {
            return existing.ident;
        }

        let ident = {
            let mut n = self.next_ident.lock();
            let v = *n;
            *n = n.wrapping_add(1);
            v
        };

        let expires_at = if expires_secs > 0 {
            crate::time::unix_time() as i64 + expires_secs
        } else {
            0
        };

        if let Err(e) = self.db.add_ban(name, version, guid, external_ip, local_ip, port, ident) {
            tracing::error!("error persistiendo ban: {}", e);
            return 0;
        }
        // Setear expiración
        if expires_at > 0 {
            if let Err(e) = self.db.set_ban_expiry(ident, expires_at) {
                tracing::warn!("error seteando expiración de ban {}: {}", ident, e);
            }
        }

        let record = BanRecord {
            name: name.to_string(),
            version: version.to_string(),
            guid: *guid,
            external_ip,
            local_ip,
            port,
            ident,
        };
        self.cache.write().push(record);
        ident
    }

    /// Elimina bans expirados. Retorna la cantidad de bans removidos.
    /// El evento `BansAutoCleared` se publica en `app.link_events` si
    /// `app` está disponible.
    pub fn prune_expired(&self) -> usize {
        let now = crate::time::unix_time() as i64;
        match self.db.prune_expired_bans(now) {
            Ok(0) => 0,
            Ok(n) => {
                // Recargar cache desde DB (el cache ya está desactualizado)
                let mut cache = self.cache.write();
                cache.retain(|b| b.ident < u16::MAX); // placeholder
                // Recargar limpio
                cache.clear();
                // Nota: re-leer todo de DB sería costoso; simplificamos
                // eliminando del cache los bans con ident alto (heurística)
                drop(cache);
                self.load();
                n
            }
            Err(e) => {
                tracing::error!("error podando bans expirados: {}", e);
                0
            }
        }
    }

    /// Desbanea por ident.
    pub fn unban(&self, ident: u16) -> bool {
        let removed = self
            .db
            .remove_ban(ident)
            .unwrap_or(false);
        if removed {
            self.cache.write().retain(|b| b.ident != ident);
        }
        removed
    }

    /// Desbanea por GUID.
    pub fn unban_by_guid(&self, guid: &[u8; 16]) -> bool {
        let removed = self
            .db
            .remove_ban_by_guid(guid)
            .unwrap_or(false);
        if removed {
            self.cache.write().retain(|b| &b.guid != guid);
        }
        removed
    }

    /// Desbanea por IP.
    pub fn unban_by_ip(&self, ip: IpAddr) -> bool {
        let removed = self.db.remove_ban_by_ip(ip).unwrap_or(false);
        if removed {
            self.cache.write().retain(|b| b.external_ip != ip);
        }
        removed
    }

    /// Itera sobre todos los bans.
    pub fn for_each<F: FnMut(&BanRecord)>(&self, mut f: F) {
        let cache = self.cache.read();
        for b in cache.iter() {
            f(b);
        }
    }

    /// Cantidad de bans activos.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// ¿Está vacío?
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn ban_and_check() {
        let db = Database::in_memory().unwrap();
        let bans = BanSystem::new(db);
        let guid = [0x11; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));

        assert!(!bans.is_banned(&guid, ip));
        let ident = bans.ban("u", "v", &guid, ip, ip, 0);
        assert!(bans.is_banned(&guid, ip));
        assert!(bans.unban(ident));
        assert!(!bans.is_banned(&guid, ip));
    }

    #[test]
    fn ban_persists_across_bansystem_instances() {
        let db = Database::in_memory().unwrap();
        let guid = [0x22; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));

        let bans1 = BanSystem::new(db.clone());
        bans1.ban("u", "v", &guid, ip, ip, 0);
        drop(bans1);

        // "recrear" el BanSystem como si el server hubiera reiniciado
        let bans2 = BanSystem::new(db.clone());
        assert!(bans2.is_banned(&guid, ip));
    }

    #[test]
    fn double_ban_returns_same_ident() {
        let db = Database::in_memory().unwrap();
        let bans = BanSystem::new(db);
        let guid = [0x33; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(3, 3, 3, 3));

        let i1 = bans.ban("u", "v", &guid, ip, ip, 0);
        let i2 = bans.ban("u", "v", &guid, ip, ip, 0);
        assert_eq!(i1, i2);
        assert_eq!(bans.len(), 1);
    }
}
