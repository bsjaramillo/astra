//! Manager de idle MANUAL, paridad `core/IdleManager.cs` de sb0t.
//!
//! En sb0t el idle es siempre una acción del usuario (no hay auto-idle por
//! inactividad): comando `#idle`/`#idles`, o un emote cuyo texto empiece con
//! `idles` (`#me idles almorzando`). Cualquier texto público o emote
//! posterior lo saca de idle. Un usuario no puede volver a marcarse idle
//! hasta 5 minutos después de su último idle (`CheckIfCanIdle`).

use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Instant;

/// Cooldown entre idles (sb0t `IdleManager.CheckIfCanIdle`: 5 minutos desde
/// el inicio del último idle).
const REIDLE_COOLDOWN_SECS: u64 = 5 * 60;

#[derive(Clone, Copy, Default)]
struct IdleState {
    /// ¿Está idle ahora?
    idle: bool,
    /// Momento en que ENTRÓ en idle por última vez (persiste tras el unidle,
    /// para el cooldown — sb0t conserva `IdleStart`).
    last_start: Option<Instant>,
}

/// Manager de idle manual por user_id.
#[derive(Default)]
pub struct IdleManager {
    state: Mutex<HashMap<u16, IdleState>>,
}

impl IdleManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intenta marcar al usuario como idle. Retorna `false` si ya está idle
    /// o si su último idle empezó hace menos de 5 minutos (cooldown sb0t).
    pub fn try_idle(&self, user_id: u16) -> bool {
        let mut state = self.state.lock();
        let entry = state.entry(user_id).or_default();
        if entry.idle {
            return false;
        }
        if let Some(start) = entry.last_start {
            if start.elapsed().as_secs() < REIDLE_COOLDOWN_SECS {
                return false;
            }
        }
        entry.idle = true;
        entry.last_start = Some(Instant::now());
        true
    }

    /// Saca al usuario de idle. Retorna `Some(segundos_ausente)` si estaba
    /// idle (para el anuncio "returned... away time"), `None` si no lo estaba.
    pub fn unidle(&self, user_id: u16) -> Option<u64> {
        let mut state = self.state.lock();
        let entry = state.get_mut(&user_id)?;
        if !entry.idle {
            return None;
        }
        entry.idle = false;
        Some(entry.last_start.map(|s| s.elapsed().as_secs()).unwrap_or(0))
    }

    /// ¿Está idle ahora?
    pub fn is_idle(&self, user_id: u16) -> bool {
        self.state.lock().get(&user_id).map(|s| s.idle).unwrap_or(false)
    }

    /// Elimina el tracking de un usuario (al desconectar).
    pub fn forget(&self, user_id: u16) {
        self.state.lock().remove(&user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_unidle_cycle() {
        let m = IdleManager::new();
        assert!(!m.is_idle(1));
        assert!(m.try_idle(1));
        assert!(m.is_idle(1));
        // Ya idle: no puede volver a idlear.
        assert!(!m.try_idle(1));
        let away = m.unidle(1).expect("estaba idle");
        assert!(away < 5);
        assert!(!m.is_idle(1));
        // Cooldown: el último idle empezó hace <5min.
        assert!(!m.try_idle(1));
    }

    #[test]
    fn unidle_when_not_idle_is_none() {
        let m = IdleManager::new();
        assert!(m.unidle(7).is_none());
        m.forget(7);
    }
}
