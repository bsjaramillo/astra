//! # astra-bot
//!
//! Bot agente inteligente integrado en Astra: una identidad propia (usuario
//! fantasma) que saluda a quien entra y conversa con los usuarios usando un
//! LLM (OpenAI-compatible o Anthropic).
//!
//! ## Uso
//!
//! El binario construye [`BotEngine`] y lo cuelga en `AppContext.bot`:
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use server_core::db::Database;
//! # let db: Arc<Database> = Database::in_memory().unwrap();
//! let bot = astra_bot::BotEngine::new(db);
//! // ctx.bot = Some(bot);  // AppContext::bot: Option<Arc<dyn Bot>>
//! ```
//!
//! La config se persiste en la DB (tabla `kv`, clave `bot.config`) y se
//! edita en vivo desde el panel admin (`/admin/bot`).

pub mod config;
pub mod engine;
pub mod llm;
pub mod memory;

pub use config::{BotConfig, LlmConfig, LlmProvider, TriggerMode};
pub use engine::BotEngine;
pub use llm::{HttpLlm, LlmClient};
pub use memory::{ChatMessage, ConversationMemory};