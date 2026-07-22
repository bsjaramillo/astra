//! MOTD (Message of the Day): un texto multilínea que se le muestra a cada
//! usuario cuando entra a la sala. Paridad conceptual con el `motd.txt` de
//! sb0t (`commands/Motd.cs`), pero simplificado: en Astra guardamos el texto
//! completo (multilínea) en el store `kv` de SQLite bajo la clave `motd`, y
//! al entrar se envía línea por línea como PM del bot al usuario.
//!
//! ## Placeholders soportados (mismos que los greets, subset de sb0t)
//!
//! - `+n`  → nick del usuario
//! - `+rn` → nombre de la sala
//! - `+ip` → IP externa
//! - `+uc` → usuarios conectados
//!
//! A diferencia de sb0t, NO interpretamos tags de media (`[youtube=]`,
//! `[image=]`, etc.): Astra manda el MOTD como texto plano por PM. Si en el
//! futuro se quiere HTML para clientes que lo soporten, se puede extender
//! aquí sin tocar los call sites.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

const KV_KEY: &str = "motd";

/// Manager del MOTD: cache en memoria + persistencia en el store `kv`.
pub struct MotdManager {
    db: Arc<Database>,
    /// Texto completo del MOTD (multilínea). Vacío = sin MOTD.
    text: RwLock<String>,
}

impl MotdManager {
    /// Crea el manager cargando el MOTD guardado (si hay).
    pub fn new(db: Arc<Database>) -> Self {
        let text = db.get_kv(KV_KEY).ok().flatten().unwrap_or_default();
        Self {
            db,
            text: RwLock::new(text),
        }
    }

    /// Recarga el MOTD desde la persistencia (paridad `/loadmotd` de sb0t:
    /// útil si el valor se editó por fuera del proceso, p.ej. panel admin).
    pub fn reload(&self) {
        let text = self.db.get_kv(KV_KEY).ok().flatten().unwrap_or_default();
        *self.text.write() = text;
    }

    /// Devuelve el texto completo del MOTD (multilínea, sin sustituir).
    pub fn text(&self) -> String {
        self.text.read().clone()
    }

    /// ¿Hay algún MOTD configurado (no vacío)?
    pub fn is_empty(&self) -> bool {
        self.text.read().trim().is_empty()
    }

    /// Reemplaza el MOTD completo y lo persiste.
    pub fn set(&self, text: &str) {
        *self.text.write() = text.to_string();
        let _ = self.db.set_kv(KV_KEY, text);
    }

    /// Devuelve las líneas del MOTD (sin las vacías) con los placeholders ya
    /// sustituidos, listas para enviarse al usuario que entra. `None`/vacío si
    /// no hay MOTD.
    pub fn rendered_lines(&self, ctx: &MotdContext) -> Vec<String> {
        let text = self.text.read();
        text.lines()
            .map(|l| render_motd(l, ctx))
            .map(|l| l.trim_end().to_string())
            .filter(|l| !l.trim().is_empty())
            .collect()
    }
}

/// Contexto para sustituir los placeholders del MOTD.
pub struct MotdContext<'a> {
    /// Nick del usuario que entra.
    pub name: &'a str,
    /// Nombre de la sala.
    pub room_name: &'a str,
    /// IP externa del usuario.
    pub ip: &'a str,
    /// Usuarios conectados.
    pub user_count: usize,
}

/// Sustituye los placeholders de una línea de MOTD.
pub fn render_motd(line: &str, ctx: &MotdContext) -> String {
    line.replace("+n", ctx.name)
        .replace("+rn", ctx.room_name)
        .replace("+ip", ctx.ip)
        .replace("+uc", &ctx.user_count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn empty_by_default() {
        let m = MotdManager::new(mem_db());
        assert!(m.is_empty());
        assert!(m.text().is_empty());
    }

    #[test]
    fn set_and_persist() {
        let db = mem_db();
        {
            let m = MotdManager::new(db.clone());
            m.set("línea 1\nlínea 2");
            assert!(!m.is_empty());
        }
        let m2 = MotdManager::new(db);
        assert_eq!(m2.text(), "línea 1\nlínea 2");
    }

    #[test]
    fn rendered_lines_substitutes_and_drops_blanks() {
        let m = MotdManager::new(mem_db());
        m.set("¡Hola +n!\n\nBienvenido a +rn (+uc conectados)\n   \n");
        let ctx = MotdContext {
            name: "Ana",
            room_name: "MiSala",
            ip: "1.2.3.4",
            user_count: 5,
        };
        let lines = m.rendered_lines(&ctx);
        assert_eq!(
            lines,
            vec!["¡Hola Ana!".to_string(), "Bienvenido a MiSala (5 conectados)".to_string()]
        );
    }
}
