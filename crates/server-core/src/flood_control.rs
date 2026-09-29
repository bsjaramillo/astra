//! Control de flood/anti-spam de texto por usuario (público/emote/PM).
//!
//! Basado en `core/FloodControl.cs` del sb0t original, pero configurable
//! (`AntiSpamConfig`) y con detección de **casi-duplicados**: además del
//! rate-limit, agrupa los últimos mensajes normalizados y marca spam cuando la
//! mayoría son iguales o muy similares (cambios de una letra, mayúsculas,
//! puntuación, letras repetidas...).
//!
//! Los usuarios de nivel superior a `Regular` (Voice/Mod/Admin/Owner) están
//! **exentos**. La acción ante una detección la decide el caller
//! (`astra_commands::check_spam`) según `AntiSpamConfig::action`.

use std::collections::VecDeque;

use parking_lot::Mutex;

use crate::settings::AntiSpamConfig;
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

/// Veredicto del anti-spam para un mensaje.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpamVerdict {
    /// Mensaje permitido.
    Clean,
    /// Demasiados mensajes en la ventana.
    RateExceeded,
    /// Mensaje repetido / casi-duplicado.
    Duplicate,
}

/// Qué debe hacer el caller con el mensaje que disparó el anti-spam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpamOutcome {
    /// Entregar el mensaje con normalidad.
    Clean,
    /// Descartar este mensaje pero mantener la conexión (warn/mute).
    Dropped,
    /// Descartar y desconectar al usuario (kick/ban).
    Disconnected,
}

/// Cap duro de la cola de mensajes recientes por ámbito.
const RECENT_POSTS_MAX: usize = 16;
/// Longitud máxima de texto usada para normalizar/comparar.
const NORM_MAX: usize = 160;

/// Normaliza un texto para comparar duplicados: minúsculas, solo
/// alfanuméricos, colapsa caracteres repetidos y descarta invisibles. Un
/// separador (espacio, puntuación, emoji) corta la colapsación, así
/// `"a a"` y `"aa"` se distinguen.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev: Option<char> = None;
    let mut count = 0usize;
    'outer: for ch in text.chars() {
        for lc in ch.to_lowercase() {
            if !lc.is_alphanumeric() {
                prev = None;
                continue;
            }
            if Some(lc) == prev {
                continue;
            }
            prev = Some(lc);
            out.push(lc);
            count += 1;
            if count >= NORM_MAX {
                break 'outer;
            }
        }
    }
    out
}

/// Similitud 0-100 entre dos textos normalizados, vía distancia de Levenshtein.
/// `100` = idénticos. Los textos vacíos se consideran 0.
pub fn similarity_percent(a: &str, b: &str) -> u8 {
    if a == b {
        return 100;
    }
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let (av, bv) = if av.len() <= bv.len() { (av, bv) } else { (bv, av) };
    let maxlen = bv.len();
    let dist = levenshtein(&av, &bv);
    let ratio = 100.0 * (1.0 - (dist as f64 / maxlen as f64));
    ratio.clamp(0.0, 100.0).round() as u8
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Registro de flood por usuario (interior mutable → funciona con `&AresUser`).
#[derive(Debug, Default)]
pub struct FloodRecord {
    /// Últimos posts públicos/emote normalizados (para duplicados).
    main_posts: Mutex<VecDeque<String>>,
    /// Últimos PM normalizados.
    pm_posts: Mutex<VecDeque<String>>,
    /// Inicio de la ventana de rate (ms epoch).
    last_packet_ms: std::sync::atomic::AtomicU64,
    /// Contador de público/emote en la ventana actual.
    counter_main: std::sync::atomic::AtomicU32,
    /// Contador de PM en la ventana actual.
    counter_pm: std::sync::atomic::AtomicU32,
    /// Contador de misc en la ventana actual.
    counter_misc: std::sync::atomic::AtomicU32,
}

impl FloodRecord {
    /// Crea un registro vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Evalúa un mensaje contra el anti-spam configurado. `now_ms` es el
    /// tiempo actual en milisegundos. No aplica ninguna acción: solo decide.
    pub fn check(
        &self,
        kind: FloodKind,
        text: &str,
        level: ILevel,
        now_ms: u64,
        cfg: &AntiSpamConfig,
    ) -> SpamVerdict {
        // Interruptor maestro y exención de niveles > Regular.
        if !cfg.enabled || level > ILevel::Regular {
            return SpamVerdict::Clean;
        }

        if cfg.duplicates_enabled
            && matches!(kind, FloodKind::Public | FloodKind::Emote | FloodKind::Pm)
            && self.is_duplicate(kind, text, cfg)
        {
            return SpamVerdict::Duplicate;
        }

        if cfg.rate_enabled && self.is_rate_exceeded(kind, now_ms, cfg) {
            return SpamVerdict::RateExceeded;
        }

        SpamVerdict::Clean
    }

    fn is_duplicate(&self, kind: FloodKind, text: &str, cfg: &AntiSpamConfig) -> bool {
        let norm = normalize(text);
        if norm.chars().count() < cfg.min_chars.max(1) {
            return false;
        }
        let count = cfg.duplicate_count.max(2) as usize;
        let threshold = cfg.similarity_percent.clamp(50, 100);
        let queue = match kind {
            FloodKind::Pm => &self.pm_posts,
            _ => &self.main_posts,
        };
        let mut posts = queue.lock();
        let similar = posts
            .iter()
            .filter(|p| {
                if threshold >= 100 {
                    p.as_str() == norm.as_str()
                } else {
                    similarity_percent(p, &norm) >= threshold
                }
            })
            .count();
        let flagged = similar + 1 >= count;
        posts.push_front(norm);
        let cap = count.min(RECENT_POSTS_MAX);
        while posts.len() > cap {
            posts.pop_back();
        }
        flagged
    }

    fn is_rate_exceeded(&self, kind: FloodKind, now_ms: u64, cfg: &AntiSpamConfig) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        let window_ms = cfg.window_secs.max(1) * 1000;
        let last = self.last_packet_ms.load(Relaxed);
        if now_ms > last.saturating_add(window_ms) {
            // Nueva ventana: reinicia los contadores (el mensaje actual SÍ
            // cuenta, para que `max_messages` sea el total permitido).
            self.last_packet_ms.store(now_ms, Relaxed);
            self.counter_main.store(0, Relaxed);
            self.counter_pm.store(0, Relaxed);
            self.counter_misc.store(0, Relaxed);
        }
        match kind {
            FloodKind::Public | FloodKind::Emote => {
                rate_exceeded(&self.counter_main, cfg.max_messages)
            }
            FloodKind::Pm => rate_exceeded(&self.counter_pm, cfg.max_pm),
            FloodKind::Misc => rate_exceeded(
                &self.counter_misc,
                cfg.max_messages.saturating_mul(2).max(1),
            ),
        }
    }
}

fn rate_exceeded(counter: &std::sync::atomic::AtomicU32, max: u32) -> bool {
    if max == 0 {
        return false;
    }
    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 > max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AntiSpamConfig {
        AntiSpamConfig::default()
    }

    #[test]
    fn normalize_case_punct_and_repeats() {
        assert_eq!(normalize("Héllo!!!"), "hélo");
        assert_eq!(normalize("hoooolaaa"), "hola");
        assert_eq!(normalize(" LOL "), "lol");
        // Un separador corta la colapsación.
        assert_eq!(normalize("l o l"), "lol");
        assert_ne!(normalize("a a"), normalize("aa"));
    }

    #[test]
    fn normalize_strips_invisible() {
        assert_eq!(normalize("s\u{200b}p\u{200b}am"), "spam");
    }

    #[test]
    fn similarity_of_near_duplicates() {
        assert_eq!(similarity_percent("hola", "hola"), 100);
        assert!(similarity_percent("holamundo", "holamundx") >= 80);
        assert!(similarity_percent("hola", "adios") < 50);
        assert_eq!(similarity_percent("", "hola"), 0);
    }

    #[test]
    fn near_duplicates_flood() {
        let f = FloodRecord::new();
        let c = cfg(); // duplicate_count = 4
        let mut t = 1_000_000u64;
        // Variaciones mínimas, espaciadas >ventana para aislar del rate.
        for m in ["hola mundo amigo", "hola mundo amigo!", "HOLA MUNDO AMIGO"] {
            assert_eq!(
                f.check(FloodKind::Public, m, ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
            t += 5000;
        }
        // La 4ª variación (1 char cambiado) marca duplicado.
        assert_eq!(
            f.check(FloodKind::Public, "hola mundo amig0", ILevel::Regular, t, &c),
            SpamVerdict::Duplicate
        );
    }

    #[test]
    fn distinct_messages_do_not_flood() {
        let f = FloodRecord::new();
        let c = cfg();
        let mut t = 1_000_000u64;
        let msgs = [
            "alfabeto", "banana", "cocodrilo", "dinosaurio", "elefante",
            "fresa", "guitarra", "hipopotamo", "iglu", "jirafa",
        ];
        for m in msgs {
            t += 5000;
            assert_eq!(
                f.check(FloodKind::Public, m, ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
        }
    }

    #[test]
    fn rate_limit_per_window() {
        let f = FloodRecord::new();
        let c = cfg(); // max_messages = 4
        let t = 2_000_000u64;
        for m in ["a1", "b2", "c3", "d4"] {
            assert_eq!(
                f.check(FloodKind::Public, m, ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
        }
        assert_eq!(
            f.check(FloodKind::Public, "e5", ILevel::Regular, t, &c),
            SpamVerdict::RateExceeded
        );
    }

    #[test]
    fn pm_uses_its_own_limit() {
        let f = FloodRecord::new();
        let c = cfg(); // max_pm = 6
        let t = 3_000_000u64;
        for i in 0..6 {
            assert_eq!(
                f.check(FloodKind::Pm, &format!("privado {i}"), ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
        }
        assert_eq!(
            f.check(FloodKind::Pm, "privado x", ILevel::Regular, t, &c),
            SpamVerdict::RateExceeded
        );
    }

    #[test]
    fn admins_are_exempt() {
        let f = FloodRecord::new();
        let c = cfg();
        let t = 4_000_000u64;
        for _ in 0..20 {
            assert_eq!(
                f.check(FloodKind::Public, "spam", ILevel::Admin, t, &c),
                SpamVerdict::Clean
            );
        }
    }

    #[test]
    fn master_switch_disables_everything() {
        let f = FloodRecord::new();
        let mut c = cfg();
        c.enabled = false;
        let t = 5_000_000u64;
        for _ in 0..20 {
            assert_eq!(
                f.check(FloodKind::Public, "spam", ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
        }
    }

    #[test]
    fn sub_switches_disable_each_technique() {
        // Solo rate: los duplicados ya no marcan.
        let f = FloodRecord::new();
        let mut c = cfg();
        c.duplicates_enabled = false;
        let mut t = 6_000_000u64;
        for _ in 0..6 {
            assert_eq!(
                f.check(FloodKind::Public, "spam igual", ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
            t += 5000;
        }

        // Solo duplicados: el rate ya no marca (mensajes distintos en el
        // mismo instante habrían disparado el rate con el switch activo).
        let f2 = FloodRecord::new();
        let mut c2 = cfg();
        c2.rate_enabled = false;
        let t2 = 7_000_000u64;
        for m in ["uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez"] {
            assert_eq!(
                f2.check(FloodKind::Public, m, ILevel::Regular, t2, &c2),
                SpamVerdict::Clean
            );
        }
    }

    #[test]
    fn short_messages_skip_duplicate_detection() {
        let f = FloodRecord::new();
        let c = cfg(); // min_chars = 4
        let mut t = 8_000_000u64;
        for _ in 0..10 {
            assert_eq!(
                f.check(FloodKind::Public, "ok", ILevel::Regular, t, &c),
                SpamVerdict::Clean
            );
            t += 5000;
        }
    }
}
