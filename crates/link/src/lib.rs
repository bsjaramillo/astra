//! Link Hub/Leaf (multi-servidor) — protocolo compatible con sb0t.
//!
//! Permite conectar servers Astra (y sb0t) para compartir usuarios y
//! mensajes. Cuando un user se une a un server, el otro server lo ve
//! (y viceversa); públicos, emotes y PMs se reenvían entre servers.
//!
//! ## Arquitectura
//!
//! - **LinkServer**: acepta conexiones de otros servers (modo "hub")
//! - **LinkClient**: se conecta a un hub (modo "leaf"), con reconnect
//!   automático (backoff exponencial 1s→60s)
//!
//! ## Handshake (paridad sb0t)
//!
//! 1. Leaf → Hub: `LeafLogin` con credentials `SHA1(reverse(name ++ guid))`
//!    + versión de protocolo (500) + puerto.
//! 2. El hub valida las credentials contra su lista de **trusted leaves**
//!    (configurada en `astra.toml`). Si no hay lista configurada, opera en
//!    modo legacy (acepta cualquier leaf, sesión sin encriptar).
//! 3. Hub → Leaf: `HubAck` con la key AES-256 + IV de la sesión, ofuscados
//!    con [`crypto::e67`] usando `MD5(guid_del_leaf)` (ver [`crypto`]).
//! 4. Post-handshake, los **strings** de los mensajes viajan encriptados
//!    (AES-256-CBC + PKCS7, formato `u16 len + ciphertext`); los campos
//!    binarios van en claro — igual que sb0t.
//!
//! ## Limitaciones (vs el sb0t original)
//!
//! - No hay soporte para múltiples hubs simultáneos

#![warn(missing_docs)]

pub mod client;
pub mod crypto;
pub mod protocol;
pub mod server;

pub use client::LinkClient;
pub use crypto::LinkCrypto;
pub use protocol::{LinkMsg, LinkPacketBuilder, LinkPacketReader, LinkUser};
pub use server::{handle_stream, LinkServer};
