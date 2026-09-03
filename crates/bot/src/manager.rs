//! Gestor del conjunto de bots agente.
//!
//! Implementa [`server_core::bot::BotRegistry`] (crear/actualizar/eliminar
//! bots en caliente) y carga los bots persistidos en `AppContext.bots` al
//! arrancar. El binario lo construye e inyecta en `AppContext.bot_registry`.

use std::sync::Arc;

use server_core::app::AppContext;
use server_core::bot::{Bot, BotRegistry};
use server_core::db::Database;

use crate::config::{BotConfig, BOT_CONFIG_KV_KEY};
use crate::engine::BotEngine;

/// Gestor de bots: crea, actualiza y elimina `BotEngine` en caliente.
pub struct BotManager {
    db: Arc<Database>,
    scripting: astra_scripting::ScriptHandle,
}

impl BotManager {
    /// Crea el gestor con la DB y el handle de scripting del servidor.
    pub fn new(db: Arc<Database>, scripting: astra_scripting::ScriptHandle) -> Arc<Self> {
        Arc::new(Self { db, scripting })
    }

    /// Carga todos los bots persistidos en `ctx.bots` (al arranque).
    ///
    /// Migra la config única antigua (`kv["bot.config"]`) al nuevo esquema la
    /// primera vez: si no hay bots en la tabla `bots` pero existe la clave
    /// legacy, la convierte en el primer registro (sin borrar la clave).
    pub fn load_all(&self, ctx: &Arc<AppContext>) {
        if let Ok(false) = self.db.has_bots() {
            if let Ok(Some(raw)) = self.db.get_kv(BOT_CONFIG_KV_KEY) {
                let _ = self.db.insert_bot(&raw);
            }
        }
        let mut bots: Vec<Arc<dyn Bot>> = Vec::new();
        if let Ok(records) = self.db.list_bots() {
            for rec in records {
                bots.push(BotEngine::new(self.db.clone(), self.scripting.clone(), rec.id));
            }
        }
        *ctx.bots.write() = bots;
    }

    /// Valida nombre (no vacío, distinto del bot del servidor, único entre
    /// bots vivos). `self_id` excluye al bot en edición. La API key solo es
    /// obligatoria si el bot está activo (un bot desactivado es un esqueleto
    /// que se completa antes de activar).
    fn validate(
        &self,
        ctx: &Arc<AppContext>,
        cfg: &BotConfig,
        self_id: Option<i64>,
    ) -> Result<(), String> {
        let name = cfg.name.trim();
        if name.is_empty() {
            return Err("el nombre del bot no puede estar vacío".into());
        }
        if name.eq_ignore_ascii_case(&ctx.settings.bot_name) {
            return Err(format!(
                "el nombre del bot no puede ser igual al del servidor ('{}')",
                ctx.settings.bot_name
            ));
        }
        if cfg.enabled && cfg.llm.api_key.trim().is_empty() {
            return Err("para activar el bot, la api_key del LLM es obligatoria".into());
        }
        for b in ctx.bots.read().iter() {
            if Some(b.bot_id()) == self_id {
                continue;
            }
            if b.bot_name().eq_ignore_ascii_case(name) {
                return Err(format!("ya existe un bot llamado '{}'", name));
            }
        }
        Ok(())
    }
}

impl BotRegistry for BotManager {
    fn create(&self, ctx: &Arc<AppContext>, config_json: &str) -> Result<i64, String> {
        let cfg: BotConfig =
            serde_json::from_str(config_json).map_err(|e| format!("json: {}", e))?;
        self.validate(ctx, &cfg, None)?;
        let id = self
            .db
            .insert_bot(config_json)
            .map_err(|e| format!("db: {}", e))?;
        let engine = BotEngine::new(self.db.clone(), self.scripting.clone(), id);
        ctx.bots.write().push(engine.clone());
        Ok(id)
    }

    fn update(&self, ctx: &Arc<AppContext>, id: i64, config_json: &str) -> Result<String, String> {
        let cfg: BotConfig =
            serde_json::from_str(config_json).map_err(|e| format!("json: {}", e))?;
        self.validate(ctx, &cfg, Some(id))?;
        let engine = ctx
            .bots
            .read()
            .iter()
            .find(|b| b.bot_id() == id)
            .cloned()
            .ok_or_else(|| format!("bot {} no encontrado", id))?;
        engine.set_config_json(config_json)?;
        Ok(engine.config_json())
    }

    fn delete(&self, ctx: &Arc<AppContext>, id: i64) -> Result<(), String> {
        self.db.delete_bot(id).map_err(|e| format!("db: {}", e))?;
        let mut bots = ctx.bots.write();
        if let Some(pos) = bots.iter().position(|b| b.bot_id() == id) {
            bots.remove(pos);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;
    use server_core::settings::Settings;

    fn ctx() -> Arc<AppContext> {
        Arc::new(AppContext::new(Settings::default(), Database::in_memory().unwrap()))
    }

    #[test]
    fn migrate_from_legacy_kv_on_first_load() {
        let db = Database::in_memory().unwrap();
        let mut cfg = BotConfig::default();
        cfg.name = "Nova".into();
        let raw = serde_json::to_string(&cfg).unwrap();
        db.set_kv(BOT_CONFIG_KV_KEY, &raw).unwrap();

        let ctx = ctx();
        let mgr = BotManager::new(db.clone(), astra_scripting::ScriptHandle::dummy());
        mgr.load_all(&ctx);

        assert_eq!(ctx.bots.read().len(), 1);
        assert_eq!(ctx.bots.read()[0].bot_name(), "Nova");
        // La clave legacy se conserva (migración no destructiva).
        assert!(db.get_kv(BOT_CONFIG_KV_KEY).unwrap().is_some());
    }

    #[test]
    fn load_all_empty_is_noop() {
        let ctx = ctx();
        let mgr = BotManager::new(Database::in_memory().unwrap(), astra_scripting::ScriptHandle::dummy());
        mgr.load_all(&ctx);
        assert!(ctx.bots.read().is_empty());
    }

    #[test]
    fn create_adds_bot_and_validates_name_uniqueness() {
        let ctx = ctx();
        let mgr = BotManager::new(
            Database::in_memory().unwrap(),
            astra_scripting::ScriptHandle::dummy(),
        );
        let cfg = serde_json::json!({
            "enabled": true,
            "name": "Nova",
            "llm": {"api_key": "k", "model": "gpt-4o-mini"}
        });
        let id = mgr.create(&ctx, &cfg.to_string()).unwrap();
        assert_eq!(id, 1);
        assert_eq!(ctx.bots.read().len(), 1);
        assert_eq!(ctx.bots.read()[0].bot_name(), "Nova");

        // Nombre duplicado → error.
        let err = mgr.create(&ctx, &cfg.to_string()).unwrap_err();
        assert!(err.contains("ya existe"), "got: {}", err);

        // Nombre igual al bot del servidor → error.
        let server = serde_json::json!({"name": ctx.settings.bot_name, "llm": {"api_key": "k"}});
        let err = mgr.create(&ctx, &server.to_string()).unwrap_err();
        assert!(err.contains("servidor"), "got: {}", err);
    }

    #[test]
    fn update_and_delete_work_by_id() {
        let ctx = ctx();
        let mgr = BotManager::new(
            Database::in_memory().unwrap(),
            astra_scripting::ScriptHandle::dummy(),
        );
        let cfg = serde_json::json!({"name": "Nova", "llm": {"api_key": "k", "model": "gpt-4o-mini"}});
        let id = mgr.create(&ctx, &cfg.to_string()).unwrap();

        let updated = serde_json::json!({"name": "Luna", "llm": {"api_key": "k", "model": "gpt-4o-mini"}});
        let out = mgr.update(&ctx, id, &updated.to_string()).unwrap();
        assert!(out.contains("Luna"));
        assert_eq!(ctx.bots.read()[0].bot_name(), "Luna");

        mgr.delete(&ctx, id).unwrap();
        assert!(ctx.bots.read().is_empty());
        assert!(mgr.update(&ctx, id, &updated.to_string()).is_err());
    }
}