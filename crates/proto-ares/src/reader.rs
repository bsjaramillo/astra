//! Lector de paquetes binarios Ares.
//!
//! Equivalente directo de `core/TCPPacketReader.cs`.
//!
//! ```rust
//! use proto_ares::PacketReader;
//!
//! let mut r = PacketReader::new(&[0x05, 0x12, 0x34, 0x78, 0x56, 0x34, 0x12]);
//! assert_eq!(r.read_u8().unwrap(), 0x05);
//! assert_eq!(r.read_u16_le().unwrap(), 0x3412);
//! assert_eq!(r.read_u32_le().unwrap(), 0x12345678);
//! ```

use std::string::FromUtf8Error;

use byteorder::ByteOrder;

use super::crypto::AresCrypto;
use super::Guid;

/// Error al leer un paquete Ares.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// No hay suficientes bytes para satisfacer la lectura.
    #[error("buffer underflow: tried to read {needed} bytes at position {pos} but only {remaining} remain")]
    Underflow {
        /// Bytes que se querían leer
        needed: usize,
        /// Posición actual del cursor
        pos: usize,
        /// Bytes restantes en el buffer
        remaining: usize,
    },
    /// String UTF-8 inválido.
    #[error("invalid utf-8: {0}")]
    InvalidUtf8(#[from] FromUtf8Error),
    /// String Ares inválido (longitud negativa o excesiva).
    #[error("invalid string length: {0}")]
    InvalidStringLength(i32),
}

pub type ReadResult<T> = Result<T, ReadError>;

/// Lector binario con posición interna.
///
/// A diferencia del `C#` original, en Rust usamos conversiones explícitas
/// (`read_*`) en vez de conversiones implícitas, para mantener seguridad de tipos.
pub struct PacketReader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Si está seteado, `read_string_nt` desencripta el string.
    crypto: Option<AresCrypto>,
}

impl<'a> PacketReader<'a> {
    /// Crea un nuevo lector sobre el slice de bytes.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            crypto: None,
        }
    }

    /// Como [`new`](Self::new) pero con desencriptado de strings si `crypto`
    /// es `Some` (cliente Ares que negoció cifrado).
    pub fn new_crypto(data: &'a [u8], crypto: Option<AresCrypto>) -> Self {
        Self {
            data,
            pos: 0,
            crypto,
        }
    }

    /// Bytes que quedan por leer.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Posición actual del cursor.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Salta `n` bytes.
    pub fn skip(&mut self, n: usize) -> ReadResult<()> {
        if self.remaining() < n {
            return Err(ReadError::Underflow {
                needed: n,
                pos: self.pos,
                remaining: self.remaining(),
            });
        }
        self.pos += n;
        Ok(())
    }

    /// Salta 1 byte.
    pub fn skip_byte(&mut self) -> ReadResult<()> {
        self.skip(1)
    }

    /// Lee un `u8`.
    pub fn read_u8(&mut self) -> ReadResult<u8> {
        self.check(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    /// Lee un `i8`.
    pub fn read_i8(&mut self) -> ReadResult<i8> {
        self.read_u8().map(|v| v as i8)
    }

    /// Lee un `bool` (1 byte, 0 = false).
    pub fn read_bool(&mut self) -> ReadResult<bool> {
        Ok(self.read_u8()? != 0)
    }

    /// Lee un `u16` little-endian.
    pub fn read_u16_le(&mut self) -> ReadResult<u16> {
        self.check(2)?;
        let v = byteorder::LittleEndian::read_u16(&self.data[self.pos..self.pos + 2]);
        self.pos += 2;
        Ok(v)
    }

    /// Lee un `i16` little-endian.
    pub fn read_i16_le(&mut self) -> ReadResult<i16> {
        self.read_u16_le().map(|v| v as i16)
    }

    /// Lee un `u32` little-endian.
    pub fn read_u32_le(&mut self) -> ReadResult<u32> {
        self.check(4)?;
        let v = byteorder::LittleEndian::read_u32(&self.data[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(v)
    }

    /// Lee un `i32` little-endian.
    pub fn read_i32_le(&mut self) -> ReadResult<i32> {
        self.read_u32_le().map(|v| v as i32)
    }

    /// Lee un `u64` little-endian.
    pub fn read_u64_le(&mut self) -> ReadResult<u64> {
        self.check(8)?;
        let v = byteorder::LittleEndian::read_u64(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(v)
    }

    /// Lee un string Ares: prefijo `u16` con la longitud en bytes, luego UTF-8.
    pub fn read_string(&mut self) -> ReadResult<String> {
        let len = self.read_i32_le()?;
        if len < 0 || (len as usize) > self.remaining() {
            return Err(ReadError::InvalidStringLength(len));
        }
        let bytes = &self.data[self.pos..self.pos + len as usize];
        self.pos += len as usize;
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    /// Lee una string null-terminated: bytes UTF-8 hasta el `0x00` (que se
    /// consume). Si no hay null, lee hasta el final. Es el formato de un
    /// cliente Ares TCP **sin cifrar** (ver `TCPPacketReader.ReadString(IClient)`
    /// rama no cifrada de sb0t).
    pub fn read_string_nt(&mut self) -> ReadResult<String> {
        // Cliente cifrado: [u16 len][AES(cipher)][opcional 0x00].
        if let Some(crypto) = self.crypto {
            let len = self.read_u16_le()? as usize;
            if len > self.remaining() {
                return Err(ReadError::Underflow {
                    needed: len,
                    pos: self.pos,
                    remaining: self.remaining(),
                });
            }
            let cipher = &self.data[self.pos..self.pos + len];
            self.pos += len;
            // sb0t consume un null final si lo hay.
            if self.pos < self.data.len() && self.data[self.pos] == 0 {
                self.pos += 1;
            }
            let plain = crypto
                .decrypt(cipher)
                .ok_or(ReadError::InvalidStringLength(len as i32))?;
            return Ok(String::from_utf8(plain)?);
        }
        let start = self.pos;
        let end = self.data[start..]
            .iter()
            .position(|&b| b == 0)
            .map(|rel| start + rel);
        match end {
            Some(nul) => {
                let s = String::from_utf8(self.data[start..nul].to_vec())?;
                self.pos = nul + 1; // consume el null
                Ok(s)
            }
            None => {
                let s = String::from_utf8(self.data[start..].to_vec())?;
                self.pos = self.data.len();
                Ok(s)
            }
        }
    }

    /// Lee 16 bytes en bruto y los devuelve como un `Guid` Ares (aplica MD5).
    pub fn read_guid(&mut self) -> ReadResult<Guid> {
        self.check(16)?;
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 16]);
        self.pos += 16;
        Ok(Guid::from_bytes(bytes))
    }

    /// Lee 16 bytes como GUID sin hashear (MD5 ya aplicado upstream).
    pub fn read_guid_hashed(&mut self) -> ReadResult<Guid> {
        self.check(16)?;
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + 16]);
        self.pos += 16;
        Ok(Guid::from_raw_hashed(bytes))
    }

    /// Lee N bytes en bruto.
    pub fn read_bytes(&mut self, n: usize) -> ReadResult<Vec<u8>> {
        self.check(n)?;
        let v = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(v)
    }

    /// Lee el resto del buffer.
    pub fn read_remaining(&mut self) -> Vec<u8> {
        let v = self.data[self.pos..].to_vec();
        self.pos = self.data.len();
        v
    }

    /// Lee una IPv4 (4 bytes big-endian) como `Ipv4Addr`.
    pub fn read_ipv4(&mut self) -> ReadResult<std::net::Ipv4Addr> {
        self.check(4)?;
        let b = &self.data[self.pos..self.pos + 4];
        self.pos += 4;
        Ok(std::net::Ipv4Addr::new(b[0], b[1], b[2], b[3]))
    }

    /// Lee una IP (4 bytes big-endian) como `IpAddr`.
    pub fn read_ip(&mut self) -> ReadResult<std::net::IpAddr> {
        Ok(std::net::IpAddr::V4(self.read_ipv4()?))
    }

    fn check(&self, n: usize) -> ReadResult<()> {
        if self.remaining() < n {
            Err(ReadError::Underflow {
                needed: n,
                pos: self.pos,
                remaining: self.remaining(),
            })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_u8() {
        let mut r = PacketReader::new(&[0x42, 0x99]);
        assert_eq!(r.read_u8().unwrap(), 0x42);
        assert_eq!(r.read_u8().unwrap(), 0x99);
        assert!(r.read_u8().is_err());
    }

    #[test]
    fn read_u16_le() {
        let mut r = PacketReader::new(&[0x34, 0x12]);
        assert_eq!(r.read_u16_le().unwrap(), 0x1234);
    }

    #[test]
    fn read_string_nt_basic() {
        // "Alice\0Bob\0" + un byte binario suelto
        let mut r = PacketReader::new(b"Alice\x00Bob\x00\x07");
        assert_eq!(r.read_string_nt().unwrap(), "Alice");
        assert_eq!(r.read_string_nt().unwrap(), "Bob");
        assert_eq!(r.read_u8().unwrap(), 0x07);
    }

    #[test]
    fn read_string_nt_no_terminator_reads_to_end() {
        let mut r = PacketReader::new(b"tail");
        assert_eq!(r.read_string_nt().unwrap(), "tail");
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn encrypted_string_roundtrip() {
        use crate::crypto::AresCrypto;
        use crate::writer::PacketWriter;
        let crypto = AresCrypto::generate();
        // Escribe [op][str1 cifrado][u8=7][str2 cifrado] con cifrado activo.
        let mut w = PacketWriter::with_msg_crypto(crate::TcpMsg::Public, Some(crypto));
        w.write_string_nt("hola mundo").unwrap();
        w.write_u8(7).unwrap();
        w.write_string_nt("otro").unwrap();
        let bytes = w.into_bytes();

        let mut r = PacketReader::new_crypto(&bytes[1..], Some(crypto));
        assert_eq!(r.read_string_nt().unwrap(), "hola mundo");
        assert_eq!(r.read_u8().unwrap(), 7);
        assert_eq!(r.read_string_nt().unwrap(), "otro");
    }

    #[test]
    fn read_u32_le() {
        let mut r = PacketReader::new(&[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(r.read_u32_le().unwrap(), 0x12345678);
    }

    #[test]
    fn read_string_ares() {
        let mut r = PacketReader::new(&[0x04, 0x00, 0x00, 0x00, b'h', b'o', b'l', b'a']);
        assert_eq!(r.read_string().unwrap(), "hola");
    }

    #[test]
    fn read_bool() {
        let mut r = PacketReader::new(&[0x01, 0x00]);
        assert!(r.read_bool().unwrap());
        assert!(!r.read_bool().unwrap());
    }

    #[test]
    fn underflow() {
        let mut r = PacketReader::new(&[0x01]);
        assert!(r.read_u16_le().is_err());
    }

    #[test]
    fn remaining() {
        let mut r = PacketReader::new(&[1, 2, 3, 4, 5]);
        assert_eq!(r.remaining(), 5);
        r.read_u16_le().unwrap();
        assert_eq!(r.remaining(), 3);
    }

    #[test]
    fn read_ip() {
        let mut r = PacketReader::new(&[192, 168, 1, 100]);
        let ip = r.read_ip().unwrap();
        assert_eq!(ip.to_string(), "192.168.1.100");
    }
}
