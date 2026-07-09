//! # proto-ares
//!
//! Implementación del protocolo binario de Ares Galaxy (cliente de chat P2P).
//!
//! Este crate provee:
//! - [`TcpMsg`]: enum de los ~70 mensajes del protocolo TCP
//! - [`UdpMsg`]: enum de los mensajes UDP (room search / firewall check)
//! - [`Packet`]: estructura de un paquete TCP entrante
//! - [`PacketReader`]: deserializador binario con conversión a `u8`, `u16`, `u32`, `String`, `Guid`, etc.
//! - [`PacketWriter`]: serializador binario
//!
//! Compatible con clientes Ares Galaxy legacy (cliente original + extensiones sb0t/ib0t).

#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod crypto;
pub mod messages;
pub mod packet;
pub mod reader;
pub mod writer;
pub mod udp_packets;

mod guid;

pub use crypto::{d67, e67, AresCrypto};
pub use guid::Guid;
pub use messages::{TcpMsg, UdpMsg};
pub use packet::Packet;
pub use reader::PacketReader;
pub use udp_packets::{UdpPacketReader, UdpPacketWriter, UdpReadError, UdpReadResult};
pub use writer::PacketWriter;
