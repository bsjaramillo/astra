//! # astra-scripting
//!
//! Motor de scripting JS (boa_engine) para plugins/servlets de sala.
//!
//! ## Componentes
//!
//! - [`types`]: `Script`, `ScriptId`, `ScriptState`, `ScriptEvent`
//! - [`api`]: bindings JS (`print`, `sendPublic`, `sendPM`, etc.)
//! - [`manager`]: `ScriptManager` (load, unload, dispatch)
//!
//! ## Uso
//!
//! ```no_run
//! use std::sync::Arc;
//! use astra_scripting::{ScriptManager, ScriptEvent, ScriptHandle};
//! use server_core::AppContext;
//!
//! # fn run() -> anyhow::Result<()> {
//! # let ctx: Arc<AppContext> = todo!();
//! let mgr = ScriptManager::new(ctx, "scripts".into());
//! let handle: ScriptHandle = mgr.start_in_thread();
//! // Carga todos los .js del directorio y arranca el thread del manager
//! // El handle se puede usar desde otras tasks para dispatchear eventos
//! handle.dispatch(ScriptEvent::Public {
//!     from: "Alice".into(),
//!     text: "hola".into(),
//! });
//! # Ok(())
//! # }
//! ```
//!
//! ## Formato de un script
//!
//! ```javascript
//! // Se ejecuta al cargar
//! function onLoad() {
//!     log("script loaded!");
//! }
//!
//! // Se ejecuta cuando un usuario se une
//! function onJoin(user, ip) {
//!     print(user + " joined from " + ip);
//! }
//!
//! // Se ejecuta cuando hay un mensaje público
//! function onPublic(from, text) {
//!     if (text === "!hello") {
//!         sendPublic("Bot", "Hello " + from + "!");
//!     }
//! }
//! ```

#![warn(missing_docs)]

pub mod api;
pub mod manager;
pub mod types;

pub use api::make_context;
pub use manager::{ScriptHandle, ScriptManager};
pub use types::{Script, ScriptEvent, ScriptId, ScriptState as ScriptLifecycle};
