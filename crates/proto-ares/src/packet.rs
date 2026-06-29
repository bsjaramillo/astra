//! Estructura `Packet` que representa un mensaje TCP entrante.

use bytes::Bytes;

use super::messages::TcpMsg;

/// Un paquete TCP Ares recibido por el servidor.
///
/// Contiene el tipo de mensaje y el payload en bruto (sin contar el opcode).
#[derive(Debug, Clone)]
pub struct Packet {
    /// Opcode del mensaje.
    pub msg: TcpMsg,
    /// Payload completo incluyendo el opcode (1 byte) y los datos restantes.
    pub data: Bytes,
}

impl Packet {
    /// Crea un nuevo paquete.
    pub fn new(msg: TcpMsg, data: Bytes) -> Self {
        Self { msg, data }
    }

    /// Devuelve el payload sin el opcode inicial.
    pub fn payload(&self) -> &[u8] {
        if self.data.len() > 1 {
            &self.data[1..]
        } else {
            &[]
        }
    }
}
