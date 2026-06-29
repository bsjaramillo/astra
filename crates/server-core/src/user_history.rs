//! Historial de usuarios + detección de join-flood.
//!
//! Equivalente a `core/UserHistory.cs`. Mantiene en memoria los últimos
//! joins por IP para detectar ataques de reconexión rápida.
//!
//! ## Join flood
//!
//! Un usuario es "flooder" si hay más de un join desde la misma IP externa
//! en los últimos 15 segundos (umbral del sb0t original).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Historial de usuarios (en memoria + DB).
pub struct UserHistory {
    db: Arc<Database>,
    /// Cache: IP externa -> lista de timestamps (ms epoch) de joins recientes.
    recent: RwLock<HashMap<IpAddr, Vec<u64>>>,
    /// Ventana para detección de flood (15s como en sb0t original)
    flood_window_ms: u64,
    /// Máximo de joins en la ventana antes de considerar flood
    flood_threshold: u32,
}

impl UserHistory {
    /// Crea el historial con la DB.
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            recent: RwLock::new(HashMap::new()),
            flood_window_ms: 15_000,
            // 0 = cualquier join previo en la ventana = flood (compatible con sb0t original)
            flood_threshold: 0,
        }
    }

    /// Registra un join. Persiste en DB y actualiza cache.
    pub fn add_user(
        &self,
        name: &str,
        version: &str,
        guid: &[u8; 16],
        external_ip: IpAddr,
        local_ip: IpAddr,
        port: u16,
        time_ms: u64,
    ) {
        // Cache
        {
            let mut recent = self.recent.write();
            let entry = recent.entry(external_ip).or_default();
            entry.push(time_ms);
            // Limpia entradas fuera de la ventana
            entry.retain(|&t| (time_ms.saturating_sub(t)) < self.flood_window_ms);
        }

        // DB
        if let Err(e) = self.db.add_user_history(
            name, version, guid, external_ip, local_ip, port, time_ms,
        ) {
            tracing::warn!("error guardando user_history: {}", e);
        }
    }

    /// Verifica si la IP está haciendo join-flood.
    pub fn is_join_flooding(&self, external_ip: IpAddr, time_ms: u64) -> bool {
        let recent = self.recent.read();
        if let Some(entries) = recent.get(&external_ip) {
            let count = entries
                .iter()
                .filter(|&&t| (time_ms.saturating_sub(t)) < self.flood_window_ms)
                .count();
            count as u32 > self.flood_threshold
        } else {
            false
        }
    }

    /// Limpia entradas antiguas de la cache.
    pub fn cleanup(&self, now_ms: u64) {
        let mut recent = self.recent.write();
        recent.retain(|_, entries| {
            entries.retain(|&t| (now_ms.saturating_sub(t)) < self.flood_window_ms);
            !entries.is_empty()
        });
    }

    /// Poda entradas de la DB con más de `max_age_secs`.
    pub fn prune(&self, max_age_secs: u64) {
        if let Err(e) = self.db.prune_old_history(max_age_secs) {
            tracing::warn!("error podando user_history: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn first_join_not_flood() {
        let db = Database::in_memory().unwrap();
        let h = UserHistory::new(db);
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let now = crate::time::unix_time();
        // Antes del primer add, no debe ser flood
        assert!(!h.is_join_flooding(ip, now));
        h.add_user("a", "v", &[0; 16], ip, ip, 0, now);
    }

    #[test]
    fn second_join_within_15s_is_flood() {
        let db = Database::in_memory().unwrap();
        let h = UserHistory::new(db);
        let ip = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));
        let now = crate::time::unix_time();
        h.add_user("a", "v", &[0; 16], ip, ip, 0, now);
        // Verificar flood ANTES del segundo add (simulando el flujo de login)
        assert!(h.is_join_flooding(ip, now + 5000));
        h.add_user("a2", "v", &[1; 16], ip, ip, 0, now + 5000);
    }

    #[test]
    fn join_after_window_not_flood() {
        let db = Database::in_memory().unwrap();
        let h = UserHistory::new(db);
        let ip = IpAddr::V4(Ipv4Addr::new(3, 3, 3, 3));
        let now = crate::time::unix_time();
        h.add_user("a", "v", &[0; 16], ip, ip, 0, now);
        // Después de 20s, la entrada vieja ya está fuera de la ventana
        assert!(!h.is_join_flooding(ip, now + 20_000));
    }

    #[test]
    fn cleanup_removes_old() {
        let db = Database::in_memory().unwrap();
        let h = UserHistory::new(db);
        let ip = IpAddr::V4(Ipv4Addr::new(4, 4, 4, 4));
        let now = crate::time::unix_time();
        h.add_user("a", "v", &[0; 16], ip, ip, 0, now);
        h.cleanup(now + 60_000);
        assert!(recent_is_empty(&h, ip));
    }

    fn recent_is_empty(h: &UserHistory, ip: IpAddr) -> bool {
        h.recent.read().get(&ip).map_or(true, |v| v.is_empty())
    }
}
