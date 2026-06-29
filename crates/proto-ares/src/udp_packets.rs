//! Reader/Writer para paquetes UDP del protocolo Ares (room search).
//!
//! A diferencia del TCP reader, el UDP reader es más simple:
//! no tiene reordering ni features especiales del b0t original.
//! Usa u16 little-endian para el prefijo de longitud de strings
//! (igual que el TCP), y 4 bytes big-endian para IPv4 (igual que el TCP).

use std::net::{IpAddr, Ipv4Addr};
use std::string::FromUtf8Error;

use byteorder::{ByteOrder, LittleEndian};

use super::messages::UdpMsg;

/// Error al leer un paquete UDP.
#[derive(Debug, thiserror::Error)]
pub enum UdpReadError {
    /// Buffer underflow
    #[error("UDP packet underflow: tried to read at pos {pos}, remaining {remaining}")]
    Underflow {
        /// Posición actual
        pos: usize,
        /// Bytes restantes
        remaining: usize,
    },
    /// String UTF-8 inválido
    #[error("invalid utf-8 in UDP string: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),
    /// Longitud de string inválida
    #[error("invalid UDP string length: {0}")]
    InvalidStringLength(i32),
}

pub type UdpReadResult<T> = Result<T, UdpReadError>;

/// Reader de paquetes UDP.
pub struct UdpPacketReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> UdpPacketReader<'a> {
    /// Crea un reader sobre los bytes dados.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bytes restantes por leer.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Posición actual.
    pub fn position(&self) -> usize {
        self.pos
    }

    fn check(&self, n: usize) -> UdpReadResult<()> {
        if self.remaining() < n {
            Err(UdpReadError::Underflow {
                pos: self.pos,
                remaining: self.remaining(),
            })
        } else {
            Ok(())
        }
    }

    /// Lee un u8.
    pub fn read_u8(&mut self) -> u8 {
        let v = self.data[self.pos];
        self.pos += 1;
        v
    }

    /// Lee un u16 little-endian.
    pub fn read_u16_le(&mut self) -> u16 {
        let v = LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]);
        self.pos += 2;
        v
    }

    /// Lee un u32 little-endian.
    pub fn read_u32_le(&mut self) -> u32 {
        let v = LittleEndian::read_u32(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        v
    }

    /// Lee una IPv4 (4 bytes big-endian, como el protocolo Ares).
    pub fn read_ipv4(&mut self) -> UdpReadResult<IpAddr> {
        self.check(4)?;
        let o = &self.data[self.pos..self.pos + 4];
        self.pos += 4;
        Ok(IpAddr::V4(Ipv4Addr::new(o[0], o[1], o[2], o[3])))
    }

    /// Lee un string Ares: prefijo u16 LE con la longitud, luego UTF-8.
    pub fn read_string(&mut self) -> UdpReadResult<String> {
        let len = self.read_u16_le() as usize;
        self.check(len)?;
        let s = String::from_utf8(self.data[self.pos..self.pos + len].to_vec())?;
        self.pos += len;
        Ok(s)
    }
}

/// Writer de paquetes UDP.
pub struct UdpPacketWriter {
    buf: Vec<u8>,
}

impl Default for UdpPacketWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl UdpPacketWriter {
    /// Crea un writer vacío.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Escribe un u8.
    pub fn write_u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    /// Escribe un u16 LE.
    pub fn write_u16_le(&mut self, v: u16) -> &mut Self {
        let mut b = [0u8; 2];
        LittleEndian::write_u16(&mut b, v);
        self.buf.extend_from_slice(&b);
        self
    }

    /// Escribe un u32 LE.
    pub fn write_u32_le(&mut self, v: u32) -> &mut Self {
        let mut b = [0u8; 4];
        LittleEndian::write_u32(&mut b, v);
        self.buf.extend_from_slice(&b);
        self
    }

    /// Escribe una IPv4 (4 bytes big-endian).
    pub fn write_ipv4(&mut self, ip: IpAddr) -> &mut Self {
        let v4 = match ip {
            IpAddr::V4(v) => v,
            IpAddr::V6(_) => Ipv4Addr::UNSPECIFIED,
        };
        self.buf.extend_from_slice(&v4.octets());
        self
    }

    /// Escribe un string Ares: prefijo u16 LE + bytes UTF-8.
    pub fn write_string(&mut self, s: &str) -> &mut Self {
        let bytes = s.as_bytes();
        self.write_u16_le(bytes.len() as u16);
        self.buf.extend_from_slice(bytes);
        self
    }

    /// Convierte en un paquete Ares con opcode antepuesto.
    pub fn to_ares_packet(&self, msg: UdpMsg) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.buf.len() + 1);
        out.push(msg as u8);
        out.extend_from_slice(&self.buf);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_u8() {
        let mut r = UdpPacketReader::new(&[0x42, 0x99]);
        assert_eq!(r.read_u8(), 0x42);
        assert_eq!(r.read_u8(), 0x99);
    }

    #[test]
    fn read_u16_le() {
        let mut r = UdpPacketReader::new(&[0x34, 0x12]);
        assert_eq!(r.read_u16_le(), 0x1234);
    }

    #[test]
    fn read_u32_le() {
        let mut r = UdpPacketReader::new(&[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(r.read_u32_le(), 0x12345678);
    }

    #[test]
    fn read_string_ares() {
        let mut r = UdpPacketReader::new(&[0x04, 0x00, b'h', b'o', b'l', b'a']);
        assert_eq!(r.read_string().unwrap(), "hola");
    }

    #[test]
    fn read_ipv4() {
        let mut r = UdpPacketReader::new(&[192, 168, 1, 100]);
        assert_eq!(r.read_ipv4().unwrap().to_string(), "192.168.1.100");
    }

    #[test]
    fn write_to_ares_packet() {
        let mut w = UdpPacketWriter::new();
        w.write_u16_le(5009);
        w.write_string("hello");
        let pkt = w.to_ares_packet(UdpMsg::ServerListAckInfo);
        assert_eq!(pkt[0], UdpMsg::ServerListAckInfo as u8);
        assert_eq!(pkt.len(), 1 + 2 + 2 + 5);
    }

    #[test]
    fn roundtrip_string() {
        let mut w = UdpPacketWriter::new();
        w.write_string("Ares");
        let bytes = w.to_ares_packet(UdpMsg::ServerListSendInfo);
        let mut r = UdpPacketReader::new(&bytes[1..]);
        assert_eq!(r.read_string().unwrap(), "Ares");
    }
}
