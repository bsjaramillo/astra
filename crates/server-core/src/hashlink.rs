//! Hashlinks de sala Ares (`arlnk://...`), paridad `core/Hashlink.cs` de
//! sb0t: cifrado XOR de flujo (`e67`/`d67`, seed 28435) sobre el payload
//! comprimido con zlib/deflate, en base64. También soporta la forma plana
//! `CHATROOM:ip:puerto|nombre`.
//!
//! Usado por `/redirect` (decodificar el destino) y, cuando exista la
//! channel list, por `/roomsearch` (codificar los resultados).

use std::io::{Read, Write};
use std::net::Ipv4Addr;

use base64::Engine as _;

/// Sala descrita por un hashlink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashlinkRoom {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub name: String,
}

const SEED: u32 = 28435;

/// Cifra (variante `e67` de sb0t): XOR con keystream realimentado por el
/// byte CIFRADO.
fn e67(data: &[u8]) -> Vec<u8> {
    let mut b: u32 = SEED;
    let mut out = Vec::with_capacity(data.len());
    for &d in data {
        let c = (d as u32 ^ (b >> 8)) as u8;
        out.push(c);
        b = ((c as u32 + b) * 23219 + 36126) & 0xFFFF;
    }
    out
}

/// Descifra (variante `d67`): igual pero realimentado por el byte CIFRADO
/// de entrada.
fn d67(data: &[u8]) -> Vec<u8> {
    let mut b: u32 = SEED;
    let mut out = Vec::with_capacity(data.len());
    for &d in data {
        out.push((d as u32 ^ (b >> 8)) as u8);
        b = ((b + d as u32) * 23219 + 36126) & 0xFFFF;
    }
    out
}

/// Codifica una sala como hashlink `arlnk://` (sin el prefijo).
pub fn encode(room: &HashlinkRoom) -> String {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0u8; 20]);
    buf.extend_from_slice(b"CHATCHANNEL");
    buf.push(0);
    buf.extend_from_slice(&room.ip.octets());
    buf.extend_from_slice(&room.port.to_le_bytes());
    buf.extend_from_slice(&room.ip.octets());
    buf.extend_from_slice(room.name.as_bytes());
    buf.push(0);
    buf.push(0);

    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = enc.write_all(&buf);
    let compressed = enc.finish().unwrap_or_default();
    base64::engine::general_purpose::STANDARD.encode(e67(&compressed))
}

/// Decodifica un hashlink (con o sin prefijo `arlnk://`; acepta también la
/// forma plana `CHATROOM:ip:puerto|nombre`). `None` si está malformado.
pub fn decode(link: &str) -> Option<HashlinkRoom> {
    let link = link.trim();
    let link = link.strip_prefix("arlnk://").unwrap_or(link);

    // Forma plana sin cifrar.
    if let Some(rest) = link
        .strip_prefix("CHATROOM:")
        .or_else(|| link.strip_prefix("chatroom:"))
    {
        let (ip_str, rest) = rest.split_once(':')?;
        let (port_str, name) = rest.split_once('|')?;
        return Some(HashlinkRoom {
            ip: ip_str.parse().ok()?,
            port: port_str.parse().ok()?,
            name: name.to_string(),
        });
    }

    // Forma cifrada: base64 → d67 → inflate → layout CHATCHANNEL.
    let raw = base64::engine::general_purpose::STANDARD.decode(link).ok()?;
    let plain = d67(&raw);
    let mut dec = flate2::read::ZlibDecoder::new(&plain[..]);
    let mut buf = Vec::new();
    dec.read_to_end(&mut buf).ok()?;

    // 20 bytes de padding + "CHATCHANNEL" + NUL = 32 bytes de encabezado.
    if buf.len() < 32 + 4 + 2 + 4 {
        return None;
    }
    let ip = Ipv4Addr::new(buf[32], buf[33], buf[34], buf[35]);
    let port = u16::from_le_bytes([buf[36], buf[37]]);
    // 4 bytes de IP repetida, luego el nombre terminado en NUL.
    let name_bytes = &buf[42..];
    let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(name_bytes.len());
    let name = String::from_utf8_lossy(&name_bytes[..end]).to_string();
    Some(HashlinkRoom { ip, port, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let room = HashlinkRoom {
            ip: Ipv4Addr::new(203, 0, 113, 7),
            port: 34567,
            name: "Sala de Prueba".to_string(),
        };
        let link = encode(&room);
        let decoded = decode(&link).expect("decode");
        assert_eq!(decoded, room);
        // Con prefijo arlnk:// también.
        let decoded = decode(&format!("arlnk://{}", link)).expect("decode con prefijo");
        assert_eq!(decoded, room);
    }

    #[test]
    fn plain_chatroom_form() {
        let r = decode("arlnk://CHATROOM:10.1.2.3:2300|Mi Sala").expect("plano");
        assert_eq!(r.ip, Ipv4Addr::new(10, 1, 2, 3));
        assert_eq!(r.port, 2300);
        assert_eq!(r.name, "Mi Sala");
    }

    #[test]
    fn garbage_is_none() {
        assert!(decode("arlnk://%%%no-base64%%%").is_none());
        assert!(decode("").is_none());
    }

    #[test]
    fn cipher_is_involutive_pair() {
        let data = b"payload de prueba 1234";
        assert_eq!(d67(&e67(data)), data.to_vec());
    }
}
