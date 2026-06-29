//! Escritor de paquetes binarios Ares.
//!
//! Equivalente directo de `core/TCPPacketWriter.cs`.
//!
//! ```rust
//! use proto_ares::{PacketWriter, TcpMsg};
//!
//! let mut w = PacketWriter::new();
//! w.write_u8(TcpMsg::ClientLogin as u8);
//! w.write_string("Ares");
//! w.write_u16_le(8080);
//! let bytes = w.into_bytes();
//! ```

use std::net::{IpAddr, Ipv4Addr};

use byteorder::{LittleEndian, WriteBytesExt};

use super::messages::TcpMsg;

/// Error al escribir un paquete Ares.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// Fallo al escribir en el buffer subyacente.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type WriteResult<T> = Result<T, WriteError>;

/// Escritor de paquetes con buffer interno.
pub struct PacketWriter {
    buf: Vec<u8>,
}

impl Default for PacketWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketWriter {
    /// Crea un escritor vacío.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Crea un escritor con capacidad pre-reservada.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
        }
    }

    /// Crea un escritor que ya comienza con el opcode del mensaje.
    pub fn with_msg(msg: TcpMsg) -> Self {
        let mut w = Self::new();
        w.write_u8(msg as u8);
        w
    }

    /// Devuelve los bytes escritos hasta ahora.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Consume el escritor y devuelve el buffer final.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Longitud actual del buffer.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// ¿Está vacío?
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Escribe el opcode de un mensaje.
    pub fn write_msg(&mut self, msg: TcpMsg) -> WriteResult<&mut Self> {
        self.write_u8(msg as u8)?;
        Ok(self)
    }

    /// Escribe un `u8`.
    pub fn write_u8(&mut self, v: u8) -> WriteResult<&mut Self> {
        self.buf.push(v);
        Ok(self)
    }

    /// Escribe un `i8`.
    pub fn write_i8(&mut self, v: i8) -> WriteResult<&mut Self> {
        self.write_u8(v as u8)
    }

    /// Escribe un `bool` (1 byte).
    pub fn write_bool(&mut self, v: bool) -> WriteResult<&mut Self> {
        self.write_u8(v as u8)
    }

    /// Escribe un `u16` little-endian.
    pub fn write_u16_le(&mut self, v: u16) -> WriteResult<&mut Self> {
        self.buf.write_u16::<LittleEndian>(v)?;
        Ok(self)
    }

    /// Escribe un `i16` little-endian.
    pub fn write_i16_le(&mut self, v: i16) -> WriteResult<&mut Self> {
        self.buf.write_i16::<LittleEndian>(v)?;
        Ok(self)
    }

    /// Escribe un `u32` little-endian.
    pub fn write_u32_le(&mut self, v: u32) -> WriteResult<&mut Self> {
        self.buf.write_u32::<LittleEndian>(v)?;
        Ok(self)
    }

    /// Escribe un `i32` little-endian.
    pub fn write_i32_le(&mut self, v: i32) -> WriteResult<&mut Self> {
        self.buf.write_i32::<LittleEndian>(v)?;
        Ok(self)
    }

    /// Escribe un `u64` little-endian.
    pub fn write_u64_le(&mut self, v: u64) -> WriteResult<&mut Self> {
        self.buf.write_u64::<LittleEndian>(v)?;
        Ok(self)
    }

    /// Escribe un string Ares: prefijo `i32` con la longitud en bytes, luego UTF-8.
    pub fn write_string(&mut self, s: &str) -> WriteResult<&mut Self> {
        let bytes = s.as_bytes();
        self.write_i32_le(bytes.len() as i32)?;
        self.buf.extend_from_slice(bytes);
        Ok(self)
    }

    /// Escribe 16 bytes de GUID.
    pub fn write_guid(&mut self, guid: &super::Guid) -> WriteResult<&mut Self> {
        self.buf.extend_from_slice(guid.as_bytes());
        Ok(self)
    }

    /// Escribe bytes en bruto.
    pub fn write_bytes(&mut self, bytes: &[u8]) -> WriteResult<&mut Self> {
        self.buf.extend_from_slice(bytes);
        Ok(self)
    }

    /// Escribe una IPv4 en formato big-endian (4 bytes).
    pub fn write_ipv4(&mut self, ip: Ipv4Addr) -> WriteResult<&mut Self> {
        self.write_bytes(&ip.octets())
    }

    /// Escribe una `IpAddr` (siempre como IPv4 si es V4, o como V6 16 bytes si es V6).
    pub fn write_ip(&mut self, ip: IpAddr) -> WriteResult<&mut Self> {
        match ip {
            IpAddr::V4(v4) => self.write_ipv4(v4),
            IpAddr::V6(v6) => self.write_bytes(&v6.octets()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_msg_and_data() {
        let mut w = PacketWriter::new();
        w.write_msg(TcpMsg::ClientLogin).unwrap();
        w.write_string("hola").unwrap();
        let bytes = w.into_bytes();
        assert_eq!(bytes[0], TcpMsg::ClientLogin as u8);
        assert_eq!(&bytes[1..5], &[0x04, 0x00, 0x00, 0x00]);
        assert_eq!(&bytes[5..], b"hola");
    }

    #[test]
    fn write_u16_le() {
        let mut w = PacketWriter::new();
        w.write_u16_le(0x1234).unwrap();
        assert_eq!(w.as_bytes(), &[0x34, 0x12]);
    }

    #[test]
    fn roundtrip_string() {
        let mut w = PacketWriter::new();
        w.write_string("Ares").unwrap();
        let bytes = w.into_bytes();
        let mut r = super::super::PacketReader::new(&bytes);
        assert_eq!(r.read_string().unwrap(), "Ares");
    }

    #[test]
    fn write_ip() {
        use std::net::Ipv4Addr;
        let mut w = PacketWriter::new();
        w.write_ip(std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))).unwrap();
        assert_eq!(w.as_bytes(), &[10, 0, 0, 1]);
    }
}
