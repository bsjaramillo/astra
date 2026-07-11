//! Filtro de palabras del chat público, equivalente a `commands/WordFilter.cs`.
//!
//! Una lista de patrones (con comodines `*` y `?`) y una acción asociada.
//! Cuando un mensaje público de un usuario regular matchea un patrón, se
//! aplica la acción. Persistido en SQLite.

use std::collections::HashMap;
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
    /// NO bloquea el mensaje: además de dejarlo pasar, difunde una o más
    /// líneas de respuesta enlatadas (placeholders `+n`/`+ip`/`+r`).
    /// Paridad `FilterType.Announce` de sb0t — es un mini sistema de
    /// auto-respuesta por keyword, administrado con `/addline`/`/remline`/
    /// `/viewfilter` (a diferencia de Block/Kick/Ban, que no tienen líneas).
    Announce = 3,
}

impl FilterAction {
    /// Convierte desde el byte persistido.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => FilterAction::Kick,
            2 => FilterAction::Ban,
            3 => FilterAction::Announce,
            _ => FilterAction::Block,
        }
    }

    /// Nombre corto para mostrar en `/listfilters`.
    pub fn as_str(&self) -> &'static str {
        match self {
            FilterAction::Block => "block",
            FilterAction::Kick => "kick",
            FilterAction::Ban => "ban",
            FilterAction::Announce => "announce",
        }
    }

    /// Parsea desde el nombre (`block`/`kick`/`ban`/`announce`). Default `block`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "kick" => FilterAction::Kick,
            "ban" => FilterAction::Ban,
            "announce" => FilterAction::Announce,
            _ => FilterAction::Block,
        }
    }
}

/// Resultado de `WordFilterManager::remove_line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveLineResult {
    /// Se borró la línea; el filtro sigue existiendo con las restantes.
    LineRemoved,
    /// Era la última línea: se borró la línea Y el filtro entero (paridad
    /// `WordFilter.RemLine` de sb0t, que borra la entrada completa cuando
    /// se queda sin líneas).
    FilterRemoved,
    /// El pattern no existe, no es de tipo Announce, o el índice de línea
    /// está fuera de rango.
    NotFound,
}

/// Manager de filtros de palabras: cache en memoria + persistencia SQLite.
pub struct WordFilterManager {
    db: Arc<Database>,
    /// Cache de `(pattern, action)`.
    cache: RwLock<Vec<(String, FilterAction)>>,
    /// Líneas de respuesta por pattern, solo relevante para entradas
    /// `Announce`. Vacío (o ausente) para Block/Kick/Ban.
    lines: RwLock<HashMap<String, Vec<String>>>,
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
        let mut lines: HashMap<String, Vec<String>> = HashMap::new();
        for (pattern, _idx, text) in db.list_all_word_filter_lines().unwrap_or_default() {
            lines.entry(pattern).or_default().push(text);
        }
        Self {
            db,
            cache: RwLock::new(cache),
            lines: RwLock::new(lines),
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

    /// Agrega o actualiza un filtro. El patrón se guarda en minúsculas. Si
    /// el pattern ya existía con líneas (Announce) y se lo re-agrega con
    /// otra acción, las líneas viejas se descartan (paridad `WordFilter.Add`
    /// de sb0t: re-agregar un trigger reemplaza la entrada por completo).
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
            cache.push((pattern.clone(), action));
        }
        drop(cache);
        if action != FilterAction::Announce {
            self.lines.write().remove(&pattern);
            let _ = self.db.clear_word_filter_lines(&pattern);
        }
    }

    /// Elimina un filtro por patrón (y sus líneas, si tenía). Retorna
    /// `true` si existía.
    pub fn remove(&self, pattern: &str) -> bool {
        let pattern = pattern.trim().to_ascii_lowercase();
        let mut cache = self.cache.write();
        let before = cache.len();
        cache.retain(|(p, _)| *p != pattern);
        let existed = cache.len() != before;
        drop(cache);
        if existed {
            let _ = self.db.remove_word_filter(&pattern);
            self.lines.write().remove(&pattern);
            let _ = self.db.clear_word_filter_lines(&pattern);
        }
        existed
    }

    /// Lista los filtros como `(pattern, action)`.
    pub fn list(&self) -> Vec<(String, FilterAction)> {
        self.cache.read().clone()
    }

    /// Evalúa un mensaje contra los filtros de censura (Block/Kick/Ban).
    /// Las entradas `Announce` NUNCA se consideran acá — no bloquean el
    /// mensaje, ver [`Self::check_announce`]. Retorna la acción del primer
    /// patrón que matchee, o `None` si ninguno.
    pub fn check(&self, text: &str) -> Option<FilterAction> {
        let lower = text.to_ascii_lowercase();
        for (pattern, action) in self.cache.read().iter() {
            if *action != FilterAction::Announce && matches_pattern(pattern, &lower) {
                return Some(*action);
            }
        }
        None
    }

    /// Evalúa un mensaje contra los filtros `Announce`. Si matchea,
    /// retorna `(pattern, líneas, resto_del_texto)` — `resto_del_texto` es
    /// lo que sigue al pattern si éste es literalmente un prefijo del
    /// mensaje (paridad `+r` de sb0t), o `""` si no (p.ej. el pattern usa
    /// wildcards y no es un prefijo literal). El mensaje NUNCA se bloquea
    /// por esto — a diferencia de `check()`, es responsabilidad del
    /// caller difundir las líneas Y dejar pasar el mensaje normalmente.
    pub fn check_announce(&self, text: &str) -> Option<(String, Vec<String>, String)> {
        let lower = text.to_ascii_lowercase();
        let cache = self.cache.read();
        for (pattern, action) in cache.iter() {
            if *action == FilterAction::Announce && matches_pattern(pattern, &lower) {
                let lines = self.lines.read().get(pattern).cloned().unwrap_or_default();
                if lines.is_empty() {
                    continue;
                }
                let remainder = lower
                    .strip_prefix(pattern.as_str())
                    .map(|r| text[text.len() - r.len()..].trim_start().to_string())
                    .unwrap_or_default();
                return Some((pattern.clone(), lines, remainder));
            }
        }
        None
    }

    /// Agrega una línea de respuesta a un filtro `Announce` existente.
    /// Falla si el pattern no existe o no es de tipo `Announce` (paridad
    /// `WordFilter.AddLine` de sb0t, que no-opea silenciosamente en ese
    /// caso — acá se prefiere devolver un error explícito al admin).
    pub fn add_line(&self, pattern: &str, text: &str) -> Result<(), String> {
        let pattern = pattern.trim().to_ascii_lowercase();
        let cache = self.cache.read();
        let is_announce = cache
            .iter()
            .any(|(p, a)| *p == pattern && *a == FilterAction::Announce);
        drop(cache);
        if !is_announce {
            return Err(format!("'{}' is not an announce-type filter", pattern));
        }
        let mut lines = self.lines.write();
        let entry = lines.entry(pattern.clone()).or_default();
        let index = entry.len() as i64;
        entry.push(text.to_string());
        let _ = self.db.add_word_filter_line(&pattern, index, text);
        Ok(())
    }

    /// Elimina la línea `line_index` de un filtro `Announce`. Si era la
    /// última línea, borra el filtro ENTERO (paridad `WordFilter.RemLine`
    /// de sb0t).
    pub fn remove_line(&self, pattern: &str, line_index: usize) -> RemoveLineResult {
        let pattern = pattern.trim().to_ascii_lowercase();
        let mut lines = self.lines.write();
        let Some(entry) = lines.get_mut(&pattern) else {
            return RemoveLineResult::NotFound;
        };
        if line_index >= entry.len() {
            return RemoveLineResult::NotFound;
        }
        entry.remove(line_index);
        if entry.is_empty() {
            lines.remove(&pattern);
            drop(lines);
            let _ = self.db.clear_word_filter_lines(&pattern);
            self.cache.write().retain(|(p, _)| *p != pattern);
            let _ = self.db.remove_word_filter(&pattern);
            RemoveLineResult::FilterRemoved
        } else {
            let remaining = entry.clone();
            drop(lines);
            let _ = self.db.clear_word_filter_lines(&pattern);
            for (i, line) in remaining.iter().enumerate() {
                let _ = self.db.add_word_filter_line(&pattern, i as i64, line);
            }
            RemoveLineResult::LineRemoved
        }
    }

    /// Líneas de un filtro `Announce` (para `/viewfilter`). `None` si el
    /// pattern no existe o no es `Announce`.
    pub fn view(&self, pattern: &str) -> Option<Vec<String>> {
        let pattern = pattern.trim().to_ascii_lowercase();
        let cache = self.cache.read();
        let is_announce = cache
            .iter()
            .any(|(p, a)| *p == pattern && *a == FilterAction::Announce);
        drop(cache);
        if !is_announce {
            return None;
        }
        Some(self.lines.read().get(&pattern).cloned().unwrap_or_default())
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

    #[test]
    fn announce_does_not_censor() {
        let m = WordFilterManager::new(mem_db());
        m.add("!rules", FilterAction::Announce);
        m.add_line("!rules", "line one").unwrap();
        // check() (censura) nunca debe ver entradas Announce.
        assert!(m.check("!rules please").is_none());
    }

    #[test]
    fn announce_check_returns_lines_and_remainder() {
        let m = WordFilterManager::new(mem_db());
        m.add("!hi", FilterAction::Announce);
        m.add_line("!hi", "hello +n").unwrap();
        m.add_line("!hi", "second line").unwrap();
        let (pattern, lines, remainder) = m.check_announce("!hi there").unwrap();
        assert_eq!(pattern, "!hi");
        assert_eq!(lines, vec!["hello +n".to_string(), "second line".to_string()]);
        assert_eq!(remainder, "there");
    }

    #[test]
    fn add_line_fails_on_non_announce() {
        let m = WordFilterManager::new(mem_db());
        m.add("bad", FilterAction::Block);
        assert!(m.add_line("bad", "won't work").is_err());
        assert!(m.add_line("nonexistent", "won't work").is_err());
    }

    #[test]
    fn remove_line_cascades_to_filter_removal() {
        let m = WordFilterManager::new(mem_db());
        m.add("!x", FilterAction::Announce);
        m.add_line("!x", "only line").unwrap();
        assert_eq!(m.remove_line("!x", 0), RemoveLineResult::FilterRemoved);
        assert_eq!(m.view("!x"), None);
        assert!(m.list().is_empty());
    }

    #[test]
    fn remove_line_keeps_filter_when_lines_remain() {
        let m = WordFilterManager::new(mem_db());
        m.add("!x", FilterAction::Announce);
        m.add_line("!x", "line 0").unwrap();
        m.add_line("!x", "line 1").unwrap();
        assert_eq!(m.remove_line("!x", 0), RemoveLineResult::LineRemoved);
        assert_eq!(m.view("!x"), Some(vec!["line 1".to_string()]));
    }

    #[test]
    fn remove_line_not_found() {
        let m = WordFilterManager::new(mem_db());
        assert_eq!(m.remove_line("nope", 0), RemoveLineResult::NotFound);
        m.add("!x", FilterAction::Announce);
        m.add_line("!x", "line 0").unwrap();
        assert_eq!(m.remove_line("!x", 5), RemoveLineResult::NotFound);
    }

    #[test]
    fn view_only_works_on_announce() {
        let m = WordFilterManager::new(mem_db());
        m.add("bad", FilterAction::Block);
        assert_eq!(m.view("bad"), None);
        m.add("!x", FilterAction::Announce);
        m.add_line("!x", "hi").unwrap();
        assert_eq!(m.view("!x"), Some(vec!["hi".to_string()]));
    }

    #[test]
    fn lines_persist_across_managers() {
        let db = mem_db();
        {
            let m = WordFilterManager::new(db.clone());
            m.add("!p", FilterAction::Announce);
            m.add_line("!p", "one").unwrap();
            m.add_line("!p", "two").unwrap();
        }
        let m2 = WordFilterManager::new(db);
        assert_eq!(m2.view("!p"), Some(vec!["one".to_string(), "two".to_string()]));
    }

    #[test]
    fn re_adding_pattern_discards_stale_lines() {
        let m = WordFilterManager::new(mem_db());
        m.add("!x", FilterAction::Announce);
        m.add_line("!x", "hi").unwrap();
        m.add("!x", FilterAction::Block); // re-add as a different type
        assert_eq!(m.view("!x"), None);
        assert!(m.add_line("!x", "won't work").is_err());
    }
}
