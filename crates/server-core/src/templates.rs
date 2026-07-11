//! Textos del sistema editables ("templates"), paridad conceptual con
//! `commands/Template.cs` de sb0t: un catálogo de mensajes con claves y un
//! texto por defecto, que el admin puede reescribir (por ejemplo, para
//! traducirlos o adaptarlos al tono de su sala).
//!
//! ## Diferencias con sb0t (deliberadas)
//!
//! - sb0t tiene ~200 textos porque difunde a la sala casi toda acción de
//!   admin (`+n was banned by +a`). Astra, en cambio, notifica en privado al
//!   que ejecuta y al afectado, y muchos de sus ~400 mensajes son errores de
//!   uso o resultados de comandos que no tiene sentido "templatear". Esta es
//!   la **Fase 1**: cubre el grupo más valioso y coherente — los avisos de
//!   moderación y control de acceso. La infraestructura queda lista para
//!   sumar más claves en fases siguientes (agregar una entrada a
//!   [`TEMPLATE_DEFAULTS`] y usar `render`/`get` en el call site).
//!
//! ## Placeholders
//!
//! El call site pasa los valores concretos vía `render(key, &[(ph, val)])`.
//! Convención de placeholders (subset sb0t): `+n` = nombre del sujeto,
//! `+a` = nombre del admin que ejecuta, `+l` = nivel, `+i` = ident/valor
//! extra. Cada default documenta cuáles usa.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Catálogo de textos del sistema: `(clave, texto por defecto)`.
///
/// Las claves son estables (se persisten los overrides por clave), así que no
/// se renombran una vez publicadas.
pub const TEMPLATE_DEFAULTS: &[(&str, &str)] = &[
    // Control de acceso / errores comunes
    ("error.access_moderator", "Access denied. Moderator+ required."),
    ("error.access_admin", "Access denied. Admin+ required."),
    ("error.user_not_found", "User not found."),
    // Kick
    ("kick.target", "You have been kicked from this room."),
    ("kick.confirm", "Kicked '+n'."),
    // Ban  (+i = ident)
    ("ban.target", "You have been banned from this room."),
    ("ban.confirm", "Banned '+n' (ident +i)."),
    ("unban.success", "Unban successful."),
    ("unban.none", "No matching ban found."),
    // Muzzle
    ("muzzle.target", "You have been muzzled."),
    ("muzzle.confirm", "Muzzled '+n'."),
    ("unmuzzle.target", "You have been unmuzzled."),
    ("unmuzzle.confirm", "Unmuzzled '+n'."),
    // Grant / revoke  (+l = nivel, ej. "80 (admin)")
    ("grant.target", "Your level is now +l."),
    ("grant.confirm", "'+n' is now level +l."),
    ("revoke.target", "Your level has been reset to regular."),
    ("revoke.confirm", "'+n' is now a regular user."),
];

/// Manager de textos del sistema: defaults en el binario + overrides en SQLite.
pub struct TemplateManager {
    db: Arc<Database>,
    /// Overrides por clave (solo las que el admin cambió).
    overrides: RwLock<HashMap<String, String>>,
}

impl TemplateManager {
    /// Crea el manager cargando los overrides guardados.
    pub fn new(db: Arc<Database>) -> Self {
        let mut overrides = HashMap::new();
        for (k, v) in db.list_templates().unwrap_or_default() {
            // Ignorar overrides de claves que ya no existen en el catálogo.
            if is_valid_key(&k) {
                overrides.insert(k, v);
            }
        }
        Self {
            db,
            overrides: RwLock::new(overrides),
        }
    }

    /// Texto actual de una clave (override o default). Si la clave no existe
    /// en el catálogo, devuelve la clave misma (nunca debería pasar en
    /// call sites correctos, pero evita panics).
    pub fn get(&self, key: &str) -> String {
        if let Some(v) = self.overrides.read().get(key) {
            return v.clone();
        }
        default_for(key).unwrap_or(key).to_string()
    }

    /// Texto de una clave con los placeholders sustituidos.
    pub fn render(&self, key: &str, subs: &[(&str, &str)]) -> String {
        let mut s = self.get(key);
        for (ph, val) in subs {
            s = s.replace(ph, val);
        }
        s
    }

    /// Setea (o borra, si `text` coincide con el default) el override de una
    /// clave. Retorna `false` si la clave no existe en el catálogo.
    pub fn set(&self, key: &str, text: &str) -> bool {
        let Some(def) = default_for(key) else {
            return false;
        };
        if text == def {
            // Igual al default → no guardamos override (o borramos el que había).
            self.overrides.write().remove(key);
            let _ = self.db.remove_template(key);
        } else {
            self.overrides.write().insert(key.to_string(), text.to_string());
            let _ = self.db.set_template(key, text);
        }
        true
    }

    /// Restaura una clave a su default (borra el override).
    pub fn reset(&self, key: &str) {
        self.overrides.write().remove(key);
        let _ = self.db.remove_template(key);
    }

    /// Lista el catálogo completo para el panel:
    /// `(key, default, current, is_override)`, en el orden de
    /// [`TEMPLATE_DEFAULTS`].
    pub fn list(&self) -> Vec<(String, String, String, bool)> {
        let ov = self.overrides.read();
        TEMPLATE_DEFAULTS
            .iter()
            .map(|(k, def)| {
                let cur = ov.get(*k).cloned();
                let is_ov = cur.is_some();
                (
                    k.to_string(),
                    def.to_string(),
                    cur.unwrap_or_else(|| def.to_string()),
                    is_ov,
                )
            })
            .collect()
    }

    /// Aplica en bloque un texto con líneas `key = valor` (el formato que
    /// edita el panel). Las claves desconocidas o líneas vacías/comentario
    /// (`#`) se ignoran. Las claves del catálogo que NO aparezcan en el
    /// texto se dejan como están (no se resetean), para que el panel pueda
    /// mandar solo lo que cambió si quisiera. Retorna cuántas se aplicaron.
    pub fn apply_bulk(&self, text: &str) -> usize {
        let mut n = 0;
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim_start(); // preservar espacios finales intencionales igual no aporta; sí quitamos el de después del '='
            if self.set(key, val.trim_end()) {
                n += 1;
            }
        }
        n
    }

    /// Exporta el catálogo como texto editable (`key = valor` por línea, con
    /// el valor actual), para precargar el textarea del panel.
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        for (k, _def, cur, _ov) in self.list() {
            out.push_str(&k);
            out.push_str(" = ");
            out.push_str(&cur);
            out.push('\n');
        }
        out
    }
}

fn is_valid_key(key: &str) -> bool {
    TEMPLATE_DEFAULTS.iter().any(|(k, _)| *k == key)
}

fn default_for(key: &str) -> Option<&'static str> {
    TEMPLATE_DEFAULTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn default_when_no_override() {
        let m = TemplateManager::new(mem_db());
        assert_eq!(m.get("kick.target"), "You have been kicked from this room.");
    }

    #[test]
    fn render_substitutes() {
        let m = TemplateManager::new(mem_db());
        assert_eq!(m.render("kick.confirm", &[("+n", "Bob")]), "Kicked 'Bob'.");
        assert_eq!(
            m.render("ban.confirm", &[("+n", "Bob"), ("+i", "7")]),
            "Banned 'Bob' (ident 7)."
        );
    }

    #[test]
    fn set_override_and_persist() {
        let db = mem_db();
        {
            let m = TemplateManager::new(db.clone());
            assert!(m.set("kick.confirm", "Expulsé a +n."));
            assert_eq!(m.render("kick.confirm", &[("+n", "Ana")]), "Expulsé a Ana.");
        }
        let m2 = TemplateManager::new(db);
        assert_eq!(m2.get("kick.confirm"), "Expulsé a +n.");
    }

    #[test]
    fn set_unknown_key_fails() {
        let m = TemplateManager::new(mem_db());
        assert!(!m.set("nope.nope", "x"));
    }

    #[test]
    fn set_to_default_clears_override() {
        let db = mem_db();
        let m = TemplateManager::new(db.clone());
        m.set("kick.target", "Otro texto");
        assert!(m.list().iter().find(|e| e.0 == "kick.target").unwrap().3); // is_override
        m.set("kick.target", "You have been kicked from this room.");
        assert!(!m.list().iter().find(|e| e.0 == "kick.target").unwrap().3);
        // y no quedó en la DB
        let m2 = TemplateManager::new(db);
        assert!(!m2.list().iter().find(|e| e.0 == "kick.target").unwrap().3);
    }

    #[test]
    fn reset_restores_default() {
        let m = TemplateManager::new(mem_db());
        m.set("muzzle.target", "silenciado");
        m.reset("muzzle.target");
        assert_eq!(m.get("muzzle.target"), "You have been muzzled.");
    }

    #[test]
    fn apply_bulk_and_export_roundtrip() {
        let m = TemplateManager::new(mem_db());
        let n = m.apply_bulk("kick.confirm = Fuera +n\n# comentario\n\nban.target = Estás baneado\ndesconocida = x");
        assert_eq!(n, 2); // dos claves válidas (la desconocida no cuenta)
        assert_eq!(m.get("kick.confirm"), "Fuera +n");
        assert_eq!(m.get("ban.target"), "Estás baneado");
        // export contiene las líneas actualizadas
        let txt = m.export_text();
        assert!(txt.contains("kick.confirm = Fuera +n"));
        assert!(txt.contains("ban.target = Estás baneado"));
    }

    #[test]
    fn list_covers_full_catalog() {
        let m = TemplateManager::new(mem_db());
        assert_eq!(m.list().len(), TEMPLATE_DEFAULTS.len());
    }
}
