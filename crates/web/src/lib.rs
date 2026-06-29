//! # astra-web
//!
//! WebSockets para clientes ib0t (HTML5) + panel de admin web.
//!
//! ## Componentes
//!
//! - [`protocol`]: encode/decode de mensajes de texto (formato `ident:args`)
//! - [`ws`]: WebSocket server + handshake RFC 6455
//! - [`handler`]: maneja una conexión (login, chat, broadcasts)
//! - [`panel`]: HTML simple para testing
//!
//! ## Uso
//!
//! ```no_run
//! use astra_web::protocol::{build, parse_incoming};
//!
//! let msg = build("PUBLIC", "hola mundo");
//! assert_eq!(msg, "PUBLIC:hola mundo");
//!
//! let (ident, args) = parse_incoming(&msg).unwrap();
//! assert_eq!(ident, "PUBLIC");
//! assert_eq!(args, "hola mundo");
//! ```

#![warn(missing_docs)]

pub mod handler;
pub mod panel;
pub mod protocol;
pub mod ws;
pub mod ws_outbound;

pub use handler::handle_connection;
pub use protocol::{build, build_with_lens, parse_incoming, parse_lens_args, LoginArgs};
pub use ws::WsServer;
