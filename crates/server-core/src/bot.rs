//! Trait del bot agente inteligente integrado.
//!
//! Lo implementa `astra-bot` y lo invoca el binario en cada evento de sala
//! (join, mensaje público, PM). `AppContext` lo cuelga como
//! `Option<Arc<dyn Bot>>` para no acoplar `server-core` al crate del bot.

use std::sync::Arc;

use crate::app::AppContext;

/// Puntos de entrada del bot agente. Todas las invocaciones son "fire and
/// forget": el bot decide internamente si responder (toggles de config,
/// cooldown, triggers) y lanza el trabajo al LLM en background.
///
/// `ctx` se pasa como `&Arc<AppContext>` porque el bot puede necesitar
/// clonarlo para ejecutar trabajo (LLM, comandos) en background.
pub trait Bot: Send + Sync {
    /// Un usuario entró a la sala (para el saludo configurable).
    fn on_join(&self, ctx: &Arc<AppContext>, name: &str);
    /// Mensaje público recibido (para responder cuando lo mencionan).
    fn on_public(&self, ctx: &Arc<AppContext>, from: &str, text: &str);
    /// PM recibido dirigido al nombre del bot.
    fn on_private(&self, ctx: &Arc<AppContext>, from: &str, text: &str);

    /// ¿Bot activo? Controla si aparece en la userlist fantasma.
    fn is_enabled(&self) -> bool;
    /// Nombre actual del bot (para la userlist fantasma).
    fn bot_name(&self) -> String;
    /// Config actual serializada como JSON (para el panel admin).
    fn config_json(&self) -> String;
    /// Reemplaza la config desde JSON (aplica en vivo).
    fn set_config_json(&self, json: &str) -> Result<(), String>;
}