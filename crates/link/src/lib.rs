//! Link Hub/Leaf (multi-servidor).
//!
//! Permite conectar dos Astra servers para compartir la lista de usuarios.
//! Cuando un user se une a un server, el otro server lo ve (y viceversa).
//!
//! ## Arquitectura
//!
//! - **LinkServer**: acepta conexiones de otros Astra servers (modo "hub")
//! - **LinkClient**: se conecta a otro Astra server (modo "leaf")
//!
//! ## Protocolo (simplificado)
//!
//! Cada 5 segundos, los servers intercambian un paquete `UserListUpdate`
//! con la lista de usuarios conectados. No hay forwarding de mensajes
//! (eso requiere implementar un `LinkProcessor` complejo, fuera del scope
//! de esta fase).
//!
//! Formato del paquete (binario, en una conexión TCP entre servers):
//! - u8  opcode (1 = UserListUpdate)
//! - u32 cantidad de users
//! - Por cada user:
//!   - u16 id
//!   - u16 port
//!   - u16 file_count
//!   - str name
//!   - str external_ip
//!   - u8 level
//!   - u8 country
//!   - str version
//!   - u16 vroom
//!
//! ## Limitaciones (vs el sb0t original)
//!
//! - No hay forwarding de mensajes (solo se comparte la lista de usuarios)
//! - No hay autenticación (asume que confías en el otro server)
//! - No hay reconnect automático
//! - No hay soporte para múltiples hubs simultáneos

#![warn(missing_docs)]

pub mod client;
pub mod protocol;
pub mod server;

pub use client::LinkClient;
pub use protocol::{LinkMsg, LinkPacketBuilder, LinkPacketReader, LinkUser};
pub use server::{handle_stream, LinkServer};
