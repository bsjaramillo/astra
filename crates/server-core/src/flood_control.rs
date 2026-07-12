//! Control de flood de texto por usuario (público/emote/PM).
//!
//! Espeja `core/FloodControl.cs` del sb0t original: los usuarios de nivel
//! `Regular` o menor que envían texto demasiado rápido, o el mismo mensaje
//! repetido, se consideran flooding y el server los desconecta. Los usuarios
//! de nivel superior a `Regular` (Voice/Mod/Admin/Owner) están **exentos**.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

use crate::types::ILevel;

/// Tipo de paquete a efectos del control de flood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloodKind {
    /// Mensaje público a la sala.
    Public,
    /// Emote (acción) a la sala.
    Emote,
    /// Mensaje privado.
    Pm,
    /// Otros paquetes (contados aparte, límite más laxo).
    Misc,
}

/// Máximo de posts recientes que se guardan para detectar duplicados.
const RECENT_POSTS_MAX: usize = 5;
/// Umbral público/emote por ventana de 1s (sb0t: `> 3`).
const MAX_MAIN_PER_SEC: u32 = 3;
/// Umbral PM por ventana de 1s (sb0t: `> 5`).
const MAX_PM_PER_SEC: u32 = 5;
/// Umbral misc por ventana de 1s (sb0t: `> 8`).
const MAX_MISC_PER_SEC: u32 = 8;

/// Registro de flood por usuario (interior mutable → funciona con `&AresUser`).
#[derive(Debug, Default)]
pub struct FloodRecord {
    /// Últimos posts públicos/emote (para detectar mensajes repetidos).
    recent_posts: parking_lot::Mutex<std::collections::VecDeque<String>>,
    /// Inicio de la ventana de rate-limit de 1s (ms epoch).
    last_packet_ms: AtomicU64,
    /// Contador de público/emote en la ventana actual.
    counter_main: AtomicU32,
    /// Contador de PM en la ventana actual.
    counter_pm: AtomicU32,
    /// Contador de misc en la ventana actual.
    counter_misc: AtomicU32,
}

impl FloodRecord {
    /// Crea un registro vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Devuelve `true` si el usuario está flooding y debe desconectarse.
    /// `level` exime a niveles superiores a `Regular`. `now_ms` es el tiempo
    /// actual en milisegundos. Espeja `FloodControl.IsFlooding` de sb0t.
    pub fn is_flooding(&self, kind: FloodKind, text: &str, level: ILevel, now_ms: u64) -> bool {
        // Detección de mensajes repetidos (solo público/emote, solo Regular-).
        if matches!(kind, FloodKind::Public | FloodKind::Emote) {
            if level > ILevel::Regular {
                return false; // niveles superiores: exentos de todo
            }
            let mut posts = self.recent_posts.lock();
            posts.push_front(text.to_string());
            if posts.len() >= RECENT_POSTS_MAX {
                posts.pop_back();
                // Si todos los posts recientes son idénticos → flood.
                let first = &posts[0];
                if posts.iter().all(|p| p == first) {
                    posts.clear();
                    return true;
                }
            }
        }

        if level > ILevel::Regular {
            return false;
        }

        // Rate-limit por ventana de 1 segundo.
        let last = self.last_packet_ms.load(Relaxed);
        if now_ms > last.saturating_add(1000) {
            self.last_packet_ms.store(now_ms, Relaxed);
            self.counter_main.store(0, Relaxed);
            self.counter_pm.store(0, Relaxed);
            self.counter_misc.store(0, Relaxed);
            return false;
        }

        match kind {
            FloodKind::Public | FloodKind::Emote => {
                self.counter_main.fetch_add(1, Relaxed) + 1 > MAX_MAIN_PER_SEC
            }
            FloodKind::Pm => self.counter_pm.fetch_add(1, Relaxed) + 1 > MAX_PM_PER_SEC,
            FloodKind::Misc => self.counter_misc.fetch_add(1, Relaxed) + 1 > MAX_MISC_PER_SEC,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_messages_flood() {
        let f = FloodRecord::new();
        let mut t = 1_000_000u64;
        // Espaciados >1s para aislar el duplicado del rate-limit: 4 idénticos
        // pasan, el 5º idéntico dispara la detección de mensajes repetidos.
        for _ in 0..4 {
            assert!(!f.is_flooding(FloodKind::Public, "hi", ILevel::Regular, t));
            t += 1100;
        }
        assert!(f.is_flooding(FloodKind::Public, "hi", ILevel::Regular, t));
    }

    #[test]
    fn distinct_messages_do_not_dup_flood() {
        let f = FloodRecord::new();
        let mut t = 1_000_000u64;
        // mensajes distintos, espaciados >1s: nunca duplicado ni rate.
        for i in 0..10 {
            t += 1100;
            assert!(!f.is_flooding(FloodKind::Public, &format!("msg{i}"), ILevel::Regular, t));
        }
    }

    #[test]
    fn rate_limit_per_second() {
        let f = FloodRecord::new();
        let t = 2_000_000u64;
        // Textos distintos en el mismo instante: 4 permitidos (1 abre la
        // ventana + 3), el 5º dispara el rate-limit.
        assert!(!f.is_flooding(FloodKind::Public, "a", ILevel::Regular, t));
        assert!(!f.is_flooding(FloodKind::Public, "b", ILevel::Regular, t));
        assert!(!f.is_flooding(FloodKind::Public, "c", ILevel::Regular, t));
        assert!(!f.is_flooding(FloodKind::Public, "d", ILevel::Regular, t));
        assert!(f.is_flooding(FloodKind::Public, "e", ILevel::Regular, t));
    }

    #[test]
    fn admins_are_exempt() {
        let f = FloodRecord::new();
        let t = 3_000_000u64;
        // Mismo mensaje 10 veces, mismo instante: Admin nunca floodea.
        for _ in 0..10 {
            assert!(!f.is_flooding(FloodKind::Public, "spam", ILevel::Admin, t));
        }
    }

    #[test]
    fn pm_rate_limit() {
        let f = FloodRecord::new();
        let t = 4_000_000u64;
        // PM: 6 permitidos (1 abre ventana + 5), el 7º floodea.
        for _ in 0..6 {
            assert!(!f.is_flooding(FloodKind::Pm, "x", ILevel::Regular, t));
        }
        assert!(f.is_flooding(FloodKind::Pm, "x", ILevel::Regular, t));
    }
}
