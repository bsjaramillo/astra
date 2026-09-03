//! # astra-bot
//!
//! Bot agente inteligente integrado en Astra: una identidad propia (usuario
//! fantasma) que saluda a quien entra y conversa con los usuarios usando un
//! LLM (OpenAI, DeepSeek o Anthropic) vía Rig (`rig-core`).
//!
//! ## Uso
//!
//! El binario construye un [`BotManager`], carga los bots persistidos en
//! `AppContext.bots` y lo inyecta en `AppContext.bot_registry` para el CRUD
//! del panel admin:
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use server_core::app::AppContext;
//! # use server_core::db::Database;
//! # use server_core::settings::Settings;
//! # let db: Arc<Database> = Database::in_memory().unwrap();
//! # let ctx = Arc::new(AppContext::new(Settings::default(), db.clone()));
//! let scripting = astra_scripting::ScriptHandle::dummy();
//! let manager = astra_bot::BotManager::new(db, scripting);
//! manager.load_all(&ctx);
//! // *ctx.bot_registry.write() = Some(manager);  // AppContext::bot_registry
//! // ctx.bots: Vec<Arc<dyn Bot>> (uno por bot persistido en la tabla `bots`)
//! ```
//!
//! La config de cada bot se persiste en la DB (tabla `bots`, un registro por
//! bot con su `id`) y se edita en vivo desde el panel admin (`/admin/bots`).

pub mod config;
pub mod engine;
pub mod llm;
pub mod manager;
pub mod memory;

pub use config::{BotConfig, LlmConfig, LlmProvider, TriggerMode};
pub use engine::BotEngine;
pub use llm::{LlmClient, RigLlm};
pub use manager::BotManager;
pub use memory::{ChatMessage, ConversationMemory};