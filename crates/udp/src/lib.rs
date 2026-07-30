//! # astra-udp
//!
//! Sistema de UDP room search (descubrimiento de salas Ares).
//!
//! ## Componentes
//!
//! Siempre disponibles (codec puro, sin estado ni runtime async):
//!
//! - [`types`]: tipos de datos (`UdpNode`, `UdpChannelItem`, `UdpStats`, etc.)
//! - [`protocol`]: encode/decode de los 9 mensajes UDP del protocolo Ares
//!
//! Bajo la feature `node-manager` (activa por defecto), que es la que arrastra
//! `server-core` y `tokio`:
//!
//! - [`manager`]: `UdpNodeManager` (in-memory + DB)
//! - [`seed`]: carga del seed JSON (`data/seed_rooms.json`)
//! - [`listener`]: task async que recibe/envía paquetes UDP
//! - [`prober`]: task async que publica (ADDIPS) nuestra existencia a nodos periódicamente
//!
//! Compilar con `--no-default-features` deja solo el codec. Sirve para hablar
//! el protocolo desde otro proceso —por ejemplo un rastreador que recorre la
//! red para catalogarla— sin montar el núcleo del servidor.

#![warn(missing_docs)]

pub mod protocol;
pub mod types;

#[cfg(feature = "node-manager")]
pub mod listener;
#[cfg(feature = "node-manager")]
pub mod manager;
#[cfg(feature = "node-manager")]
pub mod prober;
#[cfg(feature = "node-manager")]
pub mod seed;

pub use types::{NodeAddr, UdpChannelItem, UdpNode, UdpStats};

#[cfg(feature = "node-manager")]
pub use listener::{run_listener, RoomInfoFn, UserCountFn};
#[cfg(feature = "node-manager")]
pub use manager::{NodeChangeCallback, NodeSnapshot, UdpNodeManager};
#[cfg(feature = "node-manager")]
pub use prober::{push_once, run_prober};
#[cfg(feature = "node-manager")]
pub use seed::{load_seed, load_seed_force, validate_seed};
