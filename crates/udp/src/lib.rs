//! # astra-udp
//!
//! Sistema de UDP room search (descubrimiento de salas Ares).
//!
//! ## Componentes
//!
//! - [`types`]: tipos de datos (`UdpNode`, `UdpChannelItem`, `UdpStats`, etc.)
//! - [`protocol`]: encode/decode de los 9 mensajes UDP del protocolo Ares
//! - [`manager`]: `UdpNodeManager` (in-memory + DB)
//! - [`seed`]: carga del seed JSON (`data/seed_rooms.json`)
//! - [`listener`]: task async que recibe/envía paquetes UDP
//! - [`prober`]: task async que proa nodos periódicamente
//!
//! ## Uso
//!
//! ```no_run
//! use std::sync::Arc;
//! use astra_udp::UdpNodeManager;
//! use server_core::db::Database;
//!
//! let db = Database::open("data/astra.db").unwrap();
//! let manager = UdpNodeManager::new(db, 5009);
//! let stats = astra_udp::load_seed(&manager.db_arc(), std::path::Path::new("data/seed_rooms.json")).unwrap();
//! println!("seed: {} nodos cargados", stats.nodes_added);
//! ```

#![warn(missing_docs)]

pub mod listener;
pub mod manager;
pub mod prober;
pub mod protocol;
pub mod seed;
pub mod types;

pub use listener::run_listener;
pub use manager::UdpNodeManager;
pub use prober::{probe_once, run_prober};
pub use seed::load_seed;
pub use types::{NodeAddr, UdpChannelItem, UdpNode, UdpStats};
