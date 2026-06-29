//! Manager de idle. Equivalente a `core/IdleManager.cs`.

#![allow(dead_code)]

use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::Instant;

/// Manager de idle.
pub struct IdleManager {
    /// Timestamp de última actividad por usuario.
    last_active: RwLock<HashMap<u16, Instant>>,
    /// Threshold en segundos para considerar idle.
    threshold_secs: u64,
}

impl Default for IdleManager {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleManager {
    /// Crea un manager con threshold de 5 minutos.
    pub fn new() -> Self {
        Self {
            last_active: RwLock::new(HashMap::new()),
            threshold_secs: 300,
        }
    }

    /// Registra actividad de un usuario.
    pub fn touch(&self, user_id: u16) {
        self.last_active.write().insert(user_id, Instant::now());
    }

    /// Verifica si un usuario está idle.
    pub fn is_idle(&self, user_id: u16) -> bool {
        if let Some(last) = self.last_active.read().get(&user_id) {
            last.elapsed().as_secs() >= self.threshold_secs
        } else {
            false
        }
    }

    /// Elimina el tracking de un usuario.
    pub fn forget(&self, user_id: u16) {
        self.last_active.write().remove(&user_id);
    }
}
