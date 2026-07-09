//! Mensajes de bienvenida (greets), equivalente a `commands/Greets.cs` de sb0t.
//!
//! Una lista de plantillas que se rotan y se envían al usuario que entra,
//! con sustitución de placeholders. Persistido en SQLite.
//!
//! ## Placeholders soportados (subset del sb0t original)
//!
//! - `+n`  → nick del usuario
//! - `+ip` → IP externa
//! - `+id` → id de sesión
//! - `+f`  → cantidad de archivos compartidos
//! - `+v`  → versión del cliente
//! - `+uc` → usuarios conectados
//! - `+rn` → nombre de la sala
//! - `+ut` → uptime del server (formato `Nd Nh Nm`)
//! - `+l`  → región del usuario (o "unknown")

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Manager de greets: cache en memoria + persistencia SQLite.
pub struct GreetManager {
    db: Arc<Database>,
    /// Cache de `(id, template)` ordenado por id.
    cache: RwLock<Vec<(i64, String)>>,
    /// Índice de rotación (cada join consume el siguiente).
    rotation: AtomicUsize,
    /// ¿Están habilitados los greets?
    enabled: AtomicBool,
}

impl GreetManager {
    /// Crea el manager cargando los greets existentes desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let cache = db.list_greets().unwrap_or_default();
        Self {
            db,
            cache: RwLock::new(cache),
            rotation: AtomicUsize::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    /// ¿Están habilitados los greets?
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Habilita/deshabilita el envío de greets.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Cantidad de greets registrados.
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// ¿No hay greets?
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }

    /// Agrega un greet nuevo. Retorna su id.
    pub fn add(&self, template: &str) -> i64 {
        let id = self.db.add_greet(template).unwrap_or(0);
        if id != 0 {
            self.cache.write().push((id, template.to_string()));
        }
        id
    }

    /// Elimina el greet en la posición visible `index` (la que muestra
    /// `list`). Retorna el template borrado.
    pub fn remove_at(&self, index: usize) -> Option<String> {
        let mut cache = self.cache.write();
        if index >= cache.len() {
            return None;
        }
        let (id, template) = cache.remove(index);
        let _ = self.db.remove_greet(id);
        Some(template)
    }

    /// Devuelve la lista actual de templates (para `/listgreets`).
    pub fn list(&self) -> Vec<String> {
        self.cache.read().iter().map(|(_, t)| t.clone()).collect()
    }

    /// Devuelve el siguiente greet en rotación, o `None` si no hay o están
    /// deshabilitados. Rota el índice.
    pub fn next_template(&self) -> Option<String> {
        if !self.is_enabled() {
            return None;
        }
        let cache = self.cache.read();
        if cache.is_empty() {
            return None;
        }
        let idx = self.rotation.fetch_add(1, Ordering::Relaxed) % cache.len();
        Some(cache[idx].1.clone())
    }
}

/// Contexto para la sustitución de placeholders de un greet.
pub struct GreetContext<'a> {
    /// Nick del usuario.
    pub name: &'a str,
    /// IP externa.
    pub ip: &'a str,
    /// Id de sesión.
    pub id: u16,
    /// Cantidad de archivos.
    pub file_count: u16,
    /// Versión del cliente.
    pub version: &'a str,
    /// Usuarios conectados.
    pub user_count: usize,
    /// Nombre de la sala.
    pub room_name: &'a str,
    /// Uptime en segundos.
    pub uptime_secs: u64,
    /// Región del usuario.
    pub region: &'a str,
}

/// Aplica la sustitución de placeholders de sb0t sobre un template.
pub fn render_greet(template: &str, ctx: &GreetContext) -> String {
    let region = if ctx.region.is_empty() { "unknown" } else { ctx.region };
    // El orden importa: `+uc` antes que `+u`... aquí los tokens no colisionan
    // por prefijo salvo +n/+... todos son distintos, pero `+ip`/`+id` empiezan
    // con `+i`; se reemplazan como literales completos así que no hay conflicto.
    template
        .replace("+n", ctx.name)
        .replace("+ip", ctx.ip)
        .replace("+id", &ctx.id.to_string())
        .replace("+f", &ctx.file_count.to_string())
        .replace("+v", ctx.version)
        .replace("+uc", &ctx.user_count.to_string())
        .replace("+rn", ctx.room_name)
        .replace("+ut", &format_uptime(ctx.uptime_secs))
        .replace("+l", region)
}

fn format_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, (secs / 3600) % 24, (secs / 60) % 60);
    format!("{}d {}h {}m", d, h, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn add_list_remove_roundtrip() {
        let m = GreetManager::new(mem_db());
        assert!(m.is_empty());
        m.add("hola +n");
        m.add("bienvenido +n a +rn");
        assert_eq!(m.len(), 2);
        assert_eq!(m.list(), vec!["hola +n", "bienvenido +n a +rn"]);

        let removed = m.remove_at(0).unwrap();
        assert_eq!(removed, "hola +n");
        assert_eq!(m.list(), vec!["bienvenido +n a +rn"]);
        assert!(m.remove_at(5).is_none());
    }

    #[test]
    fn persists_across_managers() {
        let db = mem_db();
        {
            let m = GreetManager::new(db.clone());
            m.add("saludo +n");
        }
        // Nuevo manager sobre la misma DB debe cargar el greet.
        let m2 = GreetManager::new(db);
        assert_eq!(m2.list(), vec!["saludo +n"]);
    }

    #[test]
    fn rotation_cycles() {
        let m = GreetManager::new(mem_db());
        m.add("A");
        m.add("B");
        assert_eq!(m.next_template().unwrap(), "A");
        assert_eq!(m.next_template().unwrap(), "B");
        assert_eq!(m.next_template().unwrap(), "A");
    }

    #[test]
    fn disabled_returns_none() {
        let m = GreetManager::new(mem_db());
        m.add("A");
        m.set_enabled(false);
        assert!(m.next_template().is_none());
    }

    #[test]
    fn empty_returns_none() {
        let m = GreetManager::new(mem_db());
        assert!(m.next_template().is_none());
    }

    #[test]
    fn render_substitutes_all_placeholders() {
        let ctx = GreetContext {
            name: "Alice",
            ip: "1.2.3.4",
            id: 7,
            file_count: 42,
            version: "Ares 2.5",
            user_count: 3,
            room_name: "MiSala",
            uptime_secs: 90_061, // 1d 1h 1m
            region: "US",
        };
        let out = render_greet("+n (+ip id=+id f=+f v=+v) uc=+uc en +rn up=+ut loc=+l", &ctx);
        assert_eq!(
            out,
            "Alice (1.2.3.4 id=7 f=42 v=Ares 2.5) uc=3 en MiSala up=1d 1h 1m loc=US"
        );
    }

    #[test]
    fn render_empty_region_is_unknown() {
        let ctx = GreetContext {
            name: "Bob",
            ip: "9.9.9.9",
            id: 1,
            file_count: 0,
            version: "v",
            user_count: 1,
            room_name: "R",
            uptime_secs: 0,
            region: "",
        };
        assert_eq!(render_greet("+n de +l", &ctx), "Bob de unknown");
    }
}
