//! Enlaces rotados de la sala (URLs), equivalente a `commands/Urls.cs` de sb0t.
//!
//! Una lista de `(address, text)` que se rota y se difunde periódicamente a
//! todos los usuarios como un `MSG_CHAT_SERVER_URL` (banner clicable en el
//! cliente Ares). Persistido en SQLite.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Una URL de la sala.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlItem {
    /// Id en la DB.
    pub id: i64,
    /// Dirección (href).
    pub address: String,
    /// Texto visible.
    pub text: String,
}

/// Manager de URLs: cache en memoria + persistencia SQLite.
pub struct UrlManager {
    db: Arc<Database>,
    cache: RwLock<Vec<UrlItem>>,
    rotation: AtomicUsize,
    enabled: AtomicBool,
}

impl UrlManager {
    /// Crea el manager cargando las URLs existentes desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let cache = db
            .list_urls()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, address, text)| UrlItem { id, address, text })
            .collect();
        Self {
            db,
            cache: RwLock::new(cache),
            rotation: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    /// ¿Está habilitada la rotación de URLs?
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Habilita/deshabilita la rotación.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Cantidad de URLs.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// ¿No hay URLs?
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    /// Agrega una URL. Retorna su id.
    pub fn add(&self, address: &str, text: &str) -> i64 {
        let id = self.db.add_url(address, text).unwrap_or(0);
        if id != 0 {
            self.cache.write().push(UrlItem {
                id,
                address: address.to_string(),
                text: text.to_string(),
            });
        }
        id
    }

    /// Elimina la URL en la posición visible `index`. Retorna el item borrado.
    pub fn remove_at(&self, index: usize) -> Option<UrlItem> {
        let mut cache = self.cache.write();
        if index >= cache.len() {
            return None;
        }
        let item = cache.remove(index);
        let _ = self.db.remove_url(item.id);
        Some(item)
    }

    /// Lista las URLs actuales.
    pub fn list(&self) -> Vec<UrlItem> {
        self.cache.read().clone()
    }

    /// Devuelve la siguiente URL en rotación, o `None` si está deshabilitada
    /// o no hay ninguna. Rota el índice.
    pub fn next_url(&self) -> Option<UrlItem> {
        if !self.is_enabled() {
            return None;
        }
        let cache = self.cache.read();
        if cache.is_empty() {
            return None;
        }
        let idx = self.rotation.fetch_add(1, Ordering::Relaxed) % cache.len();
        Some(cache[idx].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn add_list_remove_roundtrip() {
        let m = UrlManager::new(mem_db());
        assert!(m.is_empty());
        m.add("https://a.com", "Site A");
        m.add("https://b.com", "Site B");
        assert_eq!(m.len(), 2);
        let list = m.list();
        assert_eq!(list[0].address, "https://a.com");
        assert_eq!(list[1].text, "Site B");

        let removed = m.remove_at(0).unwrap();
        assert_eq!(removed.address, "https://a.com");
        assert_eq!(m.len(), 1);
        assert!(m.remove_at(9).is_none());
    }

    #[test]
    fn rotation_cycles() {
        let m = UrlManager::new(mem_db());
        m.add("u1", "t1");
        m.add("u2", "t2");
        assert_eq!(m.next_url().unwrap().address, "u1");
        assert_eq!(m.next_url().unwrap().address, "u2");
        assert_eq!(m.next_url().unwrap().address, "u1");
    }

    #[test]
    fn disabled_and_empty_return_none() {
        let m = UrlManager::new(mem_db());
        assert!(m.next_url().is_none());
        m.add("u", "t");
        m.set_enabled(false);
        assert!(m.next_url().is_none());
    }

    #[test]
    fn persists_across_managers() {
        let db = mem_db();
        {
            let m = UrlManager::new(db.clone());
            m.add("https://x.com", "X");
        }
        let m2 = UrlManager::new(db);
        assert_eq!(m2.list()[0].text, "X");
    }
}
