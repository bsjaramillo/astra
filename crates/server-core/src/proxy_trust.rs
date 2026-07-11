//! Lista de proxies reversos confiables, para resolver la IP real de un
//! cliente WS detrás de un reverse proxy (paridad `TrustedProxyManager`/
//! `ib0tClient.ApplyForwardedIP` de sb0t).
//!
//! sb0t guarda esta lista en el registro de Windows y la lee en vivo en
//! cada conexión (sin reiniciar el server); acá el equivalente es una tabla
//! SQLite + cache en memoria, mismo patrón que [`crate::room_flags::RoomFlags`].
//!
//! El chequeo real (qué headers confiar y en qué orden) vive en el crate
//! `web` (necesita los headers HTTP del handshake WS, que no existen en el
//! path TCP nativo); este manager solo responde "¿esta IP directa está en
//! la lista de confianza?" — loopback siempre cuenta como confiable.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Manager de proxies confiables: cache en memoria + persistencia SQLite.
pub struct TrustedProxyManager {
    db: Arc<Database>,
    ips: RwLock<HashSet<IpAddr>>,
}

impl TrustedProxyManager {
    /// Crea el manager cargando la lista existente desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let ips = db
            .list_trusted_proxies()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| s.parse::<IpAddr>().ok())
            .collect();
        Self {
            db,
            ips: RwLock::new(ips),
        }
    }

    /// ¿Se debe confiar en los headers `X-Forwarded-For`/`X-Real-IP` que
    /// manda esta IP directa? Loopback siempre es confiable (paridad
    /// `IPAddress.IsLoopback` de sb0t), además de lo persistido.
    pub fn is_trusted(&self, ip: IpAddr) -> bool {
        ip.is_loopback() || self.ips.read().contains(&ip)
    }

    /// Agrega una IP a la lista de confianza. Retorna `false` si el string
    /// no parsea como IP válida.
    pub fn add(&self, ip: &str) -> bool {
        let Ok(parsed) = ip.trim().parse::<IpAddr>() else {
            return false;
        };
        self.ips.write().insert(parsed);
        let _ = self.db.add_trusted_proxy(&parsed.to_string());
        true
    }

    /// Quita una IP de la lista de confianza. Retorna `false` si no
    /// parseaba o no estaba en la lista.
    pub fn remove(&self, ip: &str) -> bool {
        let Ok(parsed) = ip.trim().parse::<IpAddr>() else {
            return false;
        };
        let removed = self.ips.write().remove(&parsed);
        if removed {
            let _ = self.db.remove_trusted_proxy(&parsed.to_string());
        }
        removed
    }

    /// Lista las IPs de confianza persistidas (no incluye loopback, que es
    /// implícito y no se guarda).
    pub fn list(&self) -> Vec<String> {
        let mut out: Vec<String> = self.ips.read().iter().map(|ip| ip.to_string()).collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn loopback_always_trusted() {
        let mgr = TrustedProxyManager::new(mem_db());
        assert!(mgr.is_trusted("127.0.0.1".parse().unwrap()));
        assert!(mgr.is_trusted("::1".parse().unwrap()));
        assert!(!mgr.is_trusted("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn add_and_persist() {
        let db = mem_db();
        {
            let mgr = TrustedProxyManager::new(db.clone());
            assert!(mgr.add("10.0.0.5"));
            assert!(!mgr.add("not-an-ip"));
            assert!(mgr.is_trusted("10.0.0.5".parse().unwrap()));
        }
        let mgr2 = TrustedProxyManager::new(db);
        assert!(mgr2.is_trusted("10.0.0.5".parse().unwrap()));
        assert_eq!(mgr2.list(), vec!["10.0.0.5".to_string()]);
    }

    #[test]
    fn remove_works() {
        let mgr = TrustedProxyManager::new(mem_db());
        assert!(mgr.add("10.0.0.5"));
        assert!(mgr.remove("10.0.0.5"));
        assert!(!mgr.is_trusted("10.0.0.5".parse().unwrap()));
        assert!(!mgr.remove("10.0.0.5"));
    }
}
