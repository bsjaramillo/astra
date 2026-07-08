//! Manager de idle. Equivalente a `core/IdleManager.cs`.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

/// Manager de idle. Trackea actividad por user_id y detecta transitions
/// active → idle → active.
pub struct IdleManager {
    /// Estado: `Some(Instant)` si está idle desde ese momento, `None` si está active.
    state: Mutex<HashMap<u16, Option<Instant>>>,
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
            state: Mutex::new(HashMap::new()),
            threshold_secs: 300,
        }
    }

    /// Crea un manager con threshold custom.
    pub fn with_threshold(threshold_secs: u64) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            threshold_secs,
        }
    }

    /// Registra actividad de un usuario. Retorna `Some(())` si el user
    /// pasó de idle a active (disparar `onUnidled`), `None` en otros casos.
    pub fn touch(&self, user_id: u16) -> Option<()> {
        let mut state = self.state.lock();
        let entry = state.entry(user_id).or_insert(None);
        if entry.is_some() {
            // Estaba idle, ahora active
            *entry = None;
            Some(())
        } else {
            None
        }
    }

    /// Verifica si un usuario está idle. Retorna `Some(())` si pasó de
    /// active a idle (disparar `onIdled`).
    pub fn check_idle(&self, user_id: u16) -> Option<()> {
        let mut state = self.state.lock();
        let entry = state.entry(user_id).or_insert(None);
        if entry.is_none() {
            // Chequear si pasó el threshold
            // (simplificado: cualquier check_idle marca como idle, no trackeamos
            // last_active explícitamente; el caller decide cuándo llamar check_idle)
            *entry = Some(Instant::now());
            Some(())
        } else {
            None
        }
    }

    /// Elimina el tracking de un usuario (al desconectar).
    pub fn forget(&self, user_id: u16) {
        self.state.lock().remove(&user_id);
    }

    /// Itera sobre los users marcados como idle. Usado por la task periódica
    /// para verificar si el threshold pasó.
    pub fn iter_idle(&self) -> Vec<u16> {
        self.state
            .lock()
            .iter()
            .filter_map(|(id, opt)| opt.map(|_| *id))
            .collect()
    }

    /// Threshold configurado.
    pub fn threshold_secs(&self) -> u64 {
        self.threshold_secs
    }
}
