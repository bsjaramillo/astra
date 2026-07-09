//! Filtros de nombre por tipo (`join` / `file`), equivalentes a
//! `commands/JoinFilter.cs` y `commands/FileFilter.cs` de sb0t.
//!
//! - **join**: si el nick de alguien que entra matchea un patrón, se rechaza
//!   el login.
//! - **file**: si el nombre de un archivo compartido matchea, se filtra.
//!
//! Ambos usan el mismo matching con comodines `*`/`?` que el word filter, y
//! se persisten en la tabla `name_filters`.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;
use crate::word_filter::matches_pattern;

/// Manager de un conjunto de filtros de nombre para un `kind` dado.
pub struct NameFilterManager {
    db: Arc<Database>,
    kind: &'static str,
    cache: RwLock<Vec<String>>,
}

impl NameFilterManager {
    /// Crea el manager para un `kind` (`"join"` o `"file"`), cargando desde DB.
    pub fn new(db: Arc<Database>, kind: &'static str) -> Self {
        let cache = db.list_name_filters(kind).unwrap_or_default();
        Self {
            db,
            kind,
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

    /// Agrega un patrón (minúsculas). Retorna `true` si era nuevo.
    pub fn add(&self, pattern: &str) -> bool {
        let pattern = pattern.trim().to_ascii_lowercase();
        if pattern.is_empty() {
            return false;
        }
        let is_new = self.db.add_name_filter(self.kind, &pattern).unwrap_or(false);
        if is_new {
            self.cache.write().push(pattern);
        }
        is_new
    }

    /// Elimina un patrón. Retorna `true` si existía.
    pub fn remove(&self, pattern: &str) -> bool {
        let pattern = pattern.trim().to_ascii_lowercase();
        let mut cache = self.cache.write();
        let before = cache.len();
        cache.retain(|p| *p != pattern);
        if cache.len() != before {
            let _ = self.db.remove_name_filter(self.kind, &pattern);
            true
        } else {
            false
        }
    }

    /// Lista los patrones.
    pub fn list(&self) -> Vec<String> {
        self.cache.read().clone()
    }

    /// ¿El nombre matchea algún patrón?
    pub fn matches(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.cache.read().iter().any(|p| matches_pattern(p, &lower))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn join_filter_matches() {
        let m = NameFilterManager::new(mem_db(), "join");
        assert!(m.add("spam*"));
        assert!(m.matches("SpamBot"));
        assert!(!m.matches("Alice"));
    }

    #[test]
    fn kinds_are_isolated() {
        let db = mem_db();
        let join = NameFilterManager::new(db.clone(), "join");
        let file = NameFilterManager::new(db.clone(), "file");
        join.add("badnick");
        assert!(join.matches("badnick"));
        assert!(!file.matches("badnick"), "el filtro de join no aplica a file");
    }

    #[test]
    fn remove_and_persist() {
        let db = mem_db();
        {
            let m = NameFilterManager::new(db.clone(), "file");
            m.add("*.exe");
        }
        let m2 = NameFilterManager::new(db, "file");
        assert!(m2.matches("virus.exe"));
        assert!(m2.remove("*.exe"));
        assert!(!m2.matches("virus.exe"));
    }
}
