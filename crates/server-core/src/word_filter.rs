//! Filtro de palabras del chat público, equivalente a `commands/WordFilter.cs`.
//!
//! Una lista de patrones (con comodines `*` y `?`) y una acción asociada.
//! Cuando un mensaje público de un usuario regular matchea un patrón, se
//! aplica la acción. Persistido en SQLite.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Acción a aplicar cuando un mensaje matchea un filtro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAction {
    /// Solo bloquear el mensaje (no se difunde).
    Block = 0,
    /// Bloquear y expulsar al usuario.
    Kick = 1,
    /// Bloquear y banear al usuario.
    Ban = 2,
}

impl FilterAction {
    /// Convierte desde el byte persistido.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => FilterAction::Kick,
            2 => FilterAction::Ban,
            _ => FilterAction::Block,
        }
    }

    /// Nombre corto para mostrar en `/listfilters`.
    pub fn as_str(&self) -> &'static str {
        match self {
            FilterAction::Block => "block",
            FilterAction::Kick => "kick",
            FilterAction::Ban => "ban",
        }
    }

    /// Parsea desde el nombre (`block`/`kick`/`ban`). Default `block`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "kick" => FilterAction::Kick,
            "ban" => FilterAction::Ban,
            _ => FilterAction::Block,
        }
    }
}

/// Manager de filtros de palabras: cache en memoria + persistencia SQLite.
pub struct WordFilterManager {
    db: Arc<Database>,
    /// Cache de `(pattern, action)`.
    cache: RwLock<Vec<(String, FilterAction)>>,
}

impl WordFilterManager {
    /// Crea el manager cargando los filtros existentes desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let cache = db
            .list_word_filters()
            .unwrap_or_default()
            .into_iter()
            .map(|(p, a)| (p, FilterAction::from_u8(a)))
            .collect();
        Self {
            db,
            cache: RwLock::new(cache),
        }
    }

    /// Cantidad de filtros.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// ¿No hay filtros?
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    /// Agrega o actualiza un filtro. El patrón se guarda en minúsculas.
    pub fn add(&self, pattern: &str, action: FilterAction) {
        let pattern = pattern.trim().to_ascii_lowercase();
        if pattern.is_empty() {
            return;
        }
        let _ = self.db.add_word_filter(&pattern, action as u8);
        let mut cache = self.cache.write();
        if let Some(entry) = cache.iter_mut().find(|(p, _)| *p == pattern) {
            entry.1 = action;
        } else {
            cache.push((pattern, action));
        }
    }

    /// Elimina un filtro por patrón. Retorna `true` si existía.
    pub fn remove(&self, pattern: &str) -> bool {
        let pattern = pattern.trim().to_ascii_lowercase();
        let mut cache = self.cache.write();
        let before = cache.len();
        cache.retain(|(p, _)| *p != pattern);
        if cache.len() != before {
            let _ = self.db.remove_word_filter(&pattern);
            true
        } else {
            false
        }
    }

    /// Lista los filtros como `(pattern, action)`.
    pub fn list(&self) -> Vec<(String, FilterAction)> {
        self.cache.read().clone()
    }

    /// Evalúa un mensaje contra los filtros. Retorna la acción del primer
    /// patrón que matchee, o `None` si ninguno.
    pub fn check(&self, text: &str) -> Option<FilterAction> {
        let lower = text.to_ascii_lowercase();
        for (pattern, action) in self.cache.read().iter() {
            if matches_pattern(pattern, &lower) {
                return Some(*action);
            }
        }
        None
    }
}

/// Matchea un patrón con comodines (`*` = cualquier secuencia, `?` = un char)
/// contra `text` (búsqueda de subcadena, ambos ya en minúsculas).
///
/// Un patrón sin comodines matchea si aparece como subcadena. Con comodines,
/// el patrón se ancla al inicio de cada posición y `*` consume cualquier cosa.
pub(crate) fn matches_pattern(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Probar el patrón anclado en cada posición inicial del texto.
    for start in 0..=t.len() {
        if glob_at(&p, &t[start..]) {
            return true;
        }
    }
    false
}

/// Match tipo glob desde el inicio de `text` (permite que sobre texto al final).
fn glob_at(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => true, // patrón consumido → match (subcadena)
        Some((&'*', rest)) => {
            // `*` consume 0..n chars: probar cada corte.
            for i in 0..=text.len() {
                if glob_at(rest, &text[i..]) {
                    return true;
                }
            }
            false
        }
        Some((&'?', rest)) => {
            !text.is_empty() && glob_at(rest, &text[1..])
        }
        Some((&c, rest)) => {
            !text.is_empty() && text[0] == c && glob_at(rest, &text[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn substring_match() {
        assert!(matches_pattern("bad", "you are bad indeed"));
        assert!(!matches_pattern("bad", "good vibes"));
    }

    #[test]
    fn wildcard_star() {
        assert!(matches_pattern("ba*rd", "baaaastard here"));
        assert!(matches_pattern("f*ck", "what the fck"));
        assert!(matches_pattern("f*ck", "what the fuuuuck"));
        assert!(!matches_pattern("f*ck", "fine"));
    }

    #[test]
    fn wildcard_question() {
        assert!(matches_pattern("b?d", "this is bad"));
        assert!(matches_pattern("b?d", "bud light"));
        assert!(!matches_pattern("b?d", "bd"));
    }

    #[test]
    fn action_parse_roundtrip() {
        assert_eq!(FilterAction::parse("ban"), FilterAction::Ban);
        assert_eq!(FilterAction::parse("KICK"), FilterAction::Kick);
        assert_eq!(FilterAction::parse("otro"), FilterAction::Block);
        assert_eq!(FilterAction::from_u8(2), FilterAction::Ban);
        assert_eq!(FilterAction::Ban.as_str(), "ban");
    }

    #[test]
    fn manager_add_check_remove() {
        let m = WordFilterManager::new(mem_db());
        assert!(m.check("hello world").is_none());
        m.add("Spam", FilterAction::Ban);
        // case-insensitive
        assert_eq!(m.check("this is SPAM").unwrap(), FilterAction::Ban);
        assert!(m.remove("spam"));
        assert!(m.check("this is spam").is_none());
        assert!(!m.remove("spam"));
    }

    #[test]
    fn manager_update_action() {
        let m = WordFilterManager::new(mem_db());
        m.add("x", FilterAction::Block);
        m.add("x", FilterAction::Kick);
        assert_eq!(m.len(), 1);
        assert_eq!(m.check("xyz").unwrap(), FilterAction::Kick);
    }

    #[test]
    fn persists_across_managers() {
        let db = mem_db();
        {
            let m = WordFilterManager::new(db.clone());
            m.add("badword", FilterAction::Kick);
        }
        let m2 = WordFilterManager::new(db);
        assert_eq!(m2.check("a badword here").unwrap(), FilterAction::Kick);
    }
}
