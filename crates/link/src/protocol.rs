//! Protocolo de link entre dos Astra servers (formato idéntico al sb0t original).
//!
//! ## Formato del link packet
//!
//! En una conexión TCP entre dos servers Astra, los mensajes tienen:
//!
//! ```text
//! u16 LE   ← longitud del payload (sin incluir estos 2 bytes)
//! u8  op   ← opcode (LinkMsg)
//! bytes    ← argumentos (formato depende del opcode)
//! ```
//!
//! En el TCP externo (cuando el leaf envía al hub o vice versa), se envuelve
//! en `MSG_LINK_PROTO` (opcode 251 del TCP regular):
//!
//! ```text
//! u16 LE      ← longitud del packet (sin incluir estos 2 bytes)
//! u8  0xFB    ← MSG_LINK_PROTO
//! u16 LE2     ← longitud del link payload
//! u8  op      ← LinkMsg opcode
//! bytes       ← argumentos
//! ```
//!
//! ## Formato de strings en el link
//!
//! - `WriteString(text)` (sin encryption): `bytes` UTF-8 + `\0` (null-terminated)
//! - `WriteString(text)` (con encryption): `u16 LE` + `bytes` encriptados (sin null)
//!
//! ## Opcodes (LinkMsg)
//!
//! Ver `LinkMsg` enum abajo. Los números coinciden EXACTAMENTE con el sb0t
//! original para mantener compatibilidad con servers existentes.

#![allow(dead_code)]

use std::net::IpAddr;

use bytes::{BufMut, BytesMut};

/// Opcodes del protocolo link (idéntico al sb0t original).
///
/// Muchos opcodes son compartidos entre Leaf y Hub (mismo número), pero
/// representan la misma operación desde la perspectiva del receptor.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMsg {
    /// Error en el link (Hub↔Leaf)
    Error = 0,
    /// Leaf → Hub: login inicial
    LeafLogin = 1,
    /// Hub → Leaf: ACK del login
    HubAck = 3,
    /// Hub → Leaf: una nueva leaf se conectó al hub
    HubLeafConnected = 5,
    /// Hub → Leaf: una leaf se desconectó del hub
    HubLeafDisconnected = 6,
    /// Leaf → Hub: ping keep-alive
    LeafPing = 7,
    /// Hub → Leaf: respuesta al ping
    HubPong = 8,

    /// Leaf → Hub: item de la lista de usuarios (parte del envío de userlist)
    /// Hub → Leaf: item de la lista de usuarios
    UserlistItem = 10,
    /// Leaf → Hub: avatar
    /// Hub → Leaf: avatar
    Avatar = 11,
    /// Leaf → Hub: cambio de personal message
    /// Hub → Leaf: cambio de personal message
    PersonalMessage = 12,
    /// Leaf → Hub: fin de la lista de usuarios
    LeafUserlistEnd = 14,
    /// Leaf → Hub: un usuario se unió
    LeafJoin = 15,
    /// Leaf → Hub: un usuario se fue
    /// Hub → Leaf: un usuario se fue
    Part = 16,
    /// Leaf → Hub: usuario actualizado
    /// Hub → Leaf: usuario actualizado
    UserUpdated = 18,
    /// Leaf → Hub: custom name
    /// Hub → Leaf: custom name
    CustomName = 19,

    /// Leaf → Hub: texto público
    /// Hub → Leaf: texto público
    PublicText = 20,
    /// Leaf → Hub: emote
    /// Hub → Leaf: emote
    EmoteText = 21,
    /// Leaf → Hub: PM
    /// Hub → Leaf: PM
    PrivateText = 25,
    /// Leaf → Hub: el receptor del PM nos tiene ignorados
    /// Hub → Leaf: el receptor del PM nos tiene ignorados
    PrivateIgnored = 27,
    /// Leaf → Hub: PM a un usuario específico (cross-leaf)
    /// Hub → Leaf: PM a un usuario específico
    PublicToUser = 28,
    /// Leaf → Hub: emote a un usuario específico
    /// Hub → Leaf: emote a un usuario específico
    EmoteToUser = 29,

    /// Custom data a un usuario
    CustomDataTo = 30,
    /// Custom data a un vroom
    CustomDataAll = 31,
    /// Nudge (empujón)
    Nudge = 32,
    /// Scribble a un usuario
    ScribbleUser = 33,
    /// Scribble a un leaf (sb0t MSG_LINK_LEAF/HUB_SCRIBBLE_LEAF).
    /// Leaf→Hub: `u32 target_ident, str sender, u32 height, bytes img`;
    /// Hub→Leaf: `str sender, u32 height, bytes img`.
    ScribbleLeaf = 34,

    /// Texto público como `sender` en un leaf (sb0t PUBLIC_TO_LEAF).
    /// Leaf→Hub: `u32 target_ident, str sender, str text`; Hub→Leaf: sin ident.
    PublicToLeaf = 90,
    /// Emote como `sender` en un leaf (sb0t EMOTE_TO_LEAF).
    EmoteToLeaf = 91,

    /// Cambio de nick
    NickChanged = 40,
    /// Cambio de vroom
    VroomChanged = 41,
    /// IUSER: invocar un comando en otro server
    IUser = 42,
    /// Admin: comando admin cross-server
    Admin = 43,
    /// IUSER_BIN: comando con args binarios
    IUserBin = 44,
    /// Remover admin
    NoAdmin = 45,

    /// File browse request
    Browse = 50,
    /// File browse data
    BrowseData = 51,

    /// Print a todos los usuarios de un leaf (sb0t PRINT_ALL).
    /// Leaf→Hub: `u32 target_ident, str text`; Hub→Leaf: `str text`.
    PrintAll = 60,
    /// Print a un vroom de un leaf (sb0t PRINT_VROOM).
    /// Leaf→Hub: `u32 target_ident, u16 vroom, str text`; Hub→Leaf: sin ident.
    PrintVroom = 61,
    /// Print a usuarios con nivel > N de un leaf (sb0t PRINT_LEVEL).
    /// Leaf→Hub: `u32 target_ident, u8 level, str text`; Hub→Leaf: sin ident.
    PrintLevel = 62,
}

impl LinkMsg {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Error),
            1 => Some(Self::LeafLogin),
            3 => Some(Self::HubAck),
            5 => Some(Self::HubLeafConnected),
            6 => Some(Self::HubLeafDisconnected),
            7 => Some(Self::LeafPing),
            8 => Some(Self::HubPong),
            10 => Some(Self::UserlistItem),
            11 => Some(Self::Avatar),
            12 => Some(Self::PersonalMessage),
            14 => Some(Self::LeafUserlistEnd),
            15 => Some(Self::LeafJoin),
            16 => Some(Self::Part),
            18 => Some(Self::UserUpdated),
            19 => Some(Self::CustomName),
            20 => Some(Self::PublicText),
            21 => Some(Self::EmoteText),
            25 => Some(Self::PrivateText),
            27 => Some(Self::PrivateIgnored),
            28 => Some(Self::PublicToUser),
            29 => Some(Self::EmoteToUser),
            30 => Some(Self::CustomDataTo),
            31 => Some(Self::CustomDataAll),
            32 => Some(Self::Nudge),
            33 => Some(Self::ScribbleUser),
            34 => Some(Self::ScribbleLeaf),
            40 => Some(Self::NickChanged),
            41 => Some(Self::VroomChanged),
            42 => Some(Self::IUser),
            43 => Some(Self::Admin),
            44 => Some(Self::IUserBin),
            45 => Some(Self::NoAdmin),
            50 => Some(Self::Browse),
            51 => Some(Self::BrowseData),
            60 => Some(Self::PrintAll),
            61 => Some(Self::PrintVroom),
            62 => Some(Self::PrintLevel),
            90 => Some(Self::PublicToLeaf),
            91 => Some(Self::EmoteToLeaf),
            _ => None,
        }
    }
}

/// Representa un usuario del otro server (visto desde el link).
#[derive(Debug, Clone)]
pub struct LinkUser {
    pub org_name: String,
    pub name: String,
    pub version: String,
    pub guid: [u8; 16],
    pub file_count: u16,
    pub external_ip: IpAddr,
    pub local_ip: IpAddr,
    pub port: u16,
    pub dns: String,
    pub browsable: bool,
    pub age: u8,
    pub sex: u8,
    pub country: u8,
    pub region: String,
    pub level: u8,
    pub vroom: u16,
    pub custom_client: bool,
    pub muzzled: bool,
    pub web_client: bool,
    pub encrypted: bool,
    pub registered: bool,
    pub idle: bool,
    pub custom_name: Option<String>,
    pub personal_message: Option<String>,
}

/// Opcode TCP de `MSG_LINK_PROTO` (251). Es el wrapper TCP de los
/// link packets en el canal externo.
pub const MSG_LINK_PROTO: u8 = 0xFB;

/// Builder para construir link packets (formato idéntico al sb0t original).
///
/// Si se construye con [`new_with_crypto`](Self::new_with_crypto), los
/// strings se escriben encriptados (AES-256-CBC, formato sb0t:
/// `u16 len + ciphertext + null`). Los campos binarios van siempre en claro.
pub struct LinkPacketBuilder {
    buf: BytesMut,
    crypto: Option<crate::crypto::LinkCrypto>,
}

impl LinkPacketBuilder {
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
            crypto: None,
        }
    }

    /// Builder cuyas strings van encriptadas con la crypto de sesión.
    /// `None` equivale a [`new`](Self::new) (strings en claro).
    pub fn new_with_crypto(crypto: Option<crate::crypto::LinkCrypto>) -> Self {
        Self {
            buf: BytesMut::new(),
            crypto,
        }
    }

    /// Escribe una string null-terminated (formato Ares para link).
    ///
    /// Con crypto de sesión: `u16 len + AES(utf8) + null` (sb0t
    /// `TCPPacketWriter.WriteString(leaf, text)`).
    pub fn write_string(&mut self, s: &str) {
        match &self.crypto {
            Some(c) => {
                let enc = c.encrypt(s.as_bytes());
                self.buf.put_u16_le(enc.len() as u16);
                self.buf.extend_from_slice(&enc);
                self.buf.put_u8(0);
            }
            None => {
                self.buf.extend_from_slice(s.as_bytes());
                self.buf.put_u8(0);
            }
        }
    }

    /// Escribe una string sin null al final.
    pub fn write_string_no_null(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// Escribe un u16 LE.
    pub fn write_u16(&mut self, v: u16) {
        self.buf.put_u16_le(v);
    }

    /// Escribe un u32 LE.
    pub fn write_u32(&mut self, v: u32) {
        self.buf.put_u32_le(v);
    }

    /// Escribe un u8.
    pub fn write_u8(&mut self, v: u8) {
        self.buf.put_u8(v);
    }

    /// Escribe un GUID de 16 bytes (tal cual, sin MD5).
    pub fn write_guid(&mut self, g: &[u8; 16]) {
        self.buf.extend_from_slice(g);
    }

    /// Escribe una IPv4 (4 bytes big-endian).
    pub fn write_ip(&mut self, ip: IpAddr) {
        match ip {
            IpAddr::V4(v4) => self.buf.extend_from_slice(&v4.octets()),
            IpAddr::V6(v6) => {
                // El original solo soporta IPv4; si nos llega IPv6, escribimos
                // zeros para mantener el tamaño (no debería pasar en la práctica)
                self.buf.extend_from_slice(&[0u8; 4]);
                let _ = v6; // suppress unused
            }
        }
    }

    /// Escribe bytes crudos.
    pub fn write_bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// Construye el link packet con el opcode dado y un prefijo de longitud
    /// u16 LE (formato interno al link).
    pub fn build_link_packet(self, op: LinkMsg) -> Vec<u8> {
        let payload = self.buf.freeze();
        let mut out = Vec::with_capacity(2 + 1 + payload.len());
        let len = (1 + payload.len()) as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.push(op as u8);
        out.extend_from_slice(&payload);
        out
    }
}

impl Default for LinkPacketBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Lee un link packet (formato interno al link) desde bytes.
///
/// El formato esperado (output de `build_link_packet`) es:
/// ```text
/// u16 LE      ← longitud del payload (sin incluir estos 2 bytes)
/// u8  op      ← opcode (LinkMsg)
/// bytes       ← argumentos
/// ```
///
/// `new` skipea automáticamente el prefijo de longitud.
pub struct LinkPacketReader<'a> {
    buf: &'a [u8],
    crypto: Option<crate::crypto::LinkCrypto>,
}

impl<'a> LinkPacketReader<'a> {
    /// Crea un reader desde un packet de `build_link_packet`.
    /// Skipea automáticamente el prefijo de longitud u16.
    pub fn new(data: &'a [u8]) -> Self {
        // Skipear el prefijo de longitud (2 bytes u16 LE)
        let buf = if data.len() >= 2 { &data[2..] } else { data };
        Self { buf, crypto: None }
    }

    /// Crea un reader desde un payload SIN prefijo de longitud.
    /// Usar con los args de `read_link_from_stream` (que ya incluye el op byte).
    pub fn from_payload(data: &'a [u8]) -> Self {
        Self { buf: data, crypto: None }
    }

    /// Como [`from_payload`](Self::from_payload) pero desencriptando los
    /// strings con la crypto de sesión (`None` = strings en claro).
    pub fn from_payload_with_crypto(
        data: &'a [u8],
        crypto: Option<crate::crypto::LinkCrypto>,
    ) -> Self {
        Self { buf: data, crypto }
    }

    /// Lee el opcode.
    pub fn op(&mut self) -> Result<LinkMsg, String> {
        if self.buf.is_empty() {
            return Err("buffer vacío".into());
        }
        let op = self.buf[0];
        self.buf = &self.buf[1..];
        LinkMsg::from_u8(op).ok_or_else(|| format!("opcode desconocido: {}", op))
    }

    /// Lee una string null-terminated.
    ///
    /// Con crypto de sesión: `u16 len + AES + null opcional` (sb0t
    /// `TCPPacketReader.ReadString(leaf)`).
    pub fn read_string(&mut self) -> Result<String, String> {
        if let Some(c) = self.crypto {
            if self.buf.len() < 2 {
                return Err("string encriptada sin length prefix".into());
            }
            let len = u16::from_le_bytes([self.buf[0], self.buf[1]]) as usize;
            if self.buf.len() < 2 + len {
                return Err(format!(
                    "string encriptada truncada: esperaba {} bytes, hay {}",
                    len,
                    self.buf.len() - 2
                ));
            }
            let plain = c
                .decrypt(&self.buf[2..2 + len])
                .ok_or_else(|| "string encriptada inválida (padding)".to_string())?;
            self.buf = &self.buf[2 + len..];
            // sb0t: null terminator opcional después del ciphertext
            if let Some(&0) = self.buf.first() {
                self.buf = &self.buf[1..];
            }
            return String::from_utf8(plain).map_err(|e| format!("string inválida: {}", e));
        }

        let end = self
            .buf
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| "string sin null terminator".to_string())?;
        let s = std::str::from_utf8(&self.buf[..end])
            .map_err(|e| format!("string inválida: {}", e))?
            .to_string();
        self.buf = &self.buf[end + 1..];
        Ok(s)
    }

    /// Lee una string sin null al final.
    pub fn read_string_no_null(&mut self) -> Result<String, String> {
        let s = std::str::from_utf8(self.buf)
            .map_err(|e| format!("string inválida: {}", e))?
            .to_string();
        self.buf = &[];
        Ok(s)
    }

    /// Lee u16 LE.
    pub fn read_u16(&mut self) -> Result<u16, String> {
        if self.buf.len() < 2 {
            return Err("buffer muy corto".into());
        }
        let v = u16::from_le_bytes([self.buf[0], self.buf[1]]);
        self.buf = &self.buf[2..];
        Ok(v)
    }

    /// Lee u32 LE.
    pub fn read_u32(&mut self) -> Result<u32, String> {
        if self.buf.len() < 4 {
            return Err("buffer muy corto".into());
        }
        let v = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
        self.buf = &self.buf[4..];
        Ok(v)
    }

    /// Lee u8.
    pub fn read_u8(&mut self) -> Result<u8, String> {
        if self.buf.is_empty() {
            return Err("buffer vacío".into());
        }
        let v = self.buf[0];
        self.buf = &self.buf[1..];
        Ok(v)
    }

    /// Lee un GUID de 16 bytes.
    pub fn read_guid(&mut self) -> Result<[u8; 16], String> {
        if self.buf.len() < 16 {
            return Err("buffer muy corto".into());
        }
        let mut g = [0u8; 16];
        g.copy_from_slice(&self.buf[..16]);
        self.buf = &self.buf[16..];
        Ok(g)
    }

    /// Lee una IPv4 (4 bytes).
    pub fn read_ip(&mut self) -> Result<IpAddr, String> {
        if self.buf.len() < 4 {
            return Err("buffer muy corto".into());
        }
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(
            self.buf[0], self.buf[1], self.buf[2], self.buf[3],
        ));
        self.buf = &self.buf[4..];
        Ok(ip)
    }

    /// Lee bytes crudos.
    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, String> {
        if self.buf.len() < n {
            return Err("buffer muy corto".into());
        }
        let b = self.buf[..n].to_vec();
        self.buf = &self.buf[n..];
        Ok(b)
    }

    /// Bytes restantes.
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }
}

/// Enmascara un `MSG_LINK_PROTO` packet: lee el header TCP estándar
/// (u16 length + u8 0xFB), extrae el link payload y devuelve el
/// `LinkPacketReader` listo para usar.
pub fn read_link_packet(data: &[u8]) -> Result<LinkPacketReader, String> {
    if data.len() < 2 {
        return Err("buffer muy corto para MSG_LINK_PROTO".into());
    }
    let _len = u16::from_le_bytes([data[0], data[1]]);
    if data.len() < 3 {
        return Err("buffer muy corto".into());
    }
    if data[2] != 0xFB {
        return Err(format!("opcode esperado 0xFB (MSG_LINK_PROTO), recibí {}", data[2]));
    }
    if data.len() < 3 {
        return Err("buffer muy corto".into());
    }
    let _len2 = u16::from_le_bytes([data[3], data[4]]);
    if data.len() < 6 {
        return Err("buffer muy corto".into());
    }
    // El link payload empieza en data[5] (después de MSG_LINK_PROTO + su
    // propio length prefix).
    Ok(LinkPacketReader::new(&data[5..]))
}

/// Envuelve un link packet en un `MSG_LINK_PROTO` packet TCP.
pub fn write_link_packet(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    // u16 length (del TCP packet completo)
    let total = (1 + 2 + payload.len()) as u16;
    out.extend_from_slice(&total.to_le_bytes());
    // u8 MSG_LINK_PROTO
    out.push(0xFB);
    // u16 length (del link packet interno)
    let link_len = payload.len() as u16;
    out.extend_from_slice(&link_len.to_le_bytes());
    // payload
    out.extend_from_slice(payload);
    out
}

/// Lee un `MSG_LINK_PROTO` packet desde un stream TCP, con su length prefix.
///
/// Devuelve `(LinkMsg, args)` donde `args` son los argumentos del paquete
/// SIN el op byte. Para crear un reader sobre los args, usar
/// `LinkPacketReader::from_payload`.
pub async fn read_link_from_stream<Reader>(
    mut reader: Reader,
) -> Result<(LinkMsg, Vec<u8>), String>
where
    Reader: tokio::io::AsyncRead + Unpin + Send,
{
    use tokio::io::AsyncReadExt;

    // Header: u16 (len) + u8 (opcode) + u16 (link_len)
    let mut header = [0u8; 5];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|e| format!("error leyendo header: {}", e))?;
    if header[2] != 0xFB {
        return Err(format!(
            "opcode esperado 0xFB (MSG_LINK_PROTO), recibí {}",
            header[2]
        ));
    }
    let link_len = u16::from_le_bytes([header[3], header[4]]) as usize;
    let mut raw = vec![0u8; link_len];
    reader
        .read_exact(&mut raw)
        .await
        .map_err(|e| format!("error leyendo payload: {}", e))?;

    // raw: [u8 op, args...]
    if raw.is_empty() {
        return Err("payload vacío".into());
    }
    let op = LinkMsg::from_u8(raw[0])
        .ok_or_else(|| format!("opcode desconocido: {}", raw[0]))?;
    Ok((op, raw[1..].to_vec()))
}

/// Escribe un `MSG_LINK_PROTO` packet a un stream TCP.
pub async fn write_link_to_stream<Writer>(
    mut writer: Writer,
    op: LinkMsg,
    payload: &[u8],
) -> Result<(), String>
where
    Writer: tokio::io::AsyncWrite + Unpin + Send,
{
    use tokio::io::AsyncWriteExt;

    // Construye el link packet interno
    let mut inner = Vec::with_capacity(1 + payload.len());
    inner.push(op as u8);
    inner.extend_from_slice(payload);
    let link_len = inner.len() as u16;

    // Header: u16 (total) + u8 (0xFB) + u16 (link_len)
    let total = (1 + 2 + inner.len()) as u16;
    writer
        .write_all(&total.to_le_bytes())
        .await
        .map_err(|e| format!("error escribiendo total len: {}", e))?;
    writer
        .write_all(&[0xFB])
        .await
        .map_err(|e| format!("error escribiendo opcode: {}", e))?;
    writer
        .write_all(&link_len.to_le_bytes())
        .await
        .map_err(|e| format!("error escribiendo link len: {}", e))?;
    writer
        .write_all(&inner)
        .await
        .map_err(|e| format!("error escribiendo payload: {}", e))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("error flushing: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_login_roundtrip() {
        let mut b = LinkPacketBuilder::new();
        b.write_string("TestRoomName");
        b.write_guid(&[0xAA; 16]);
        b.write_u16(251); // LINK_PROTO
        b.write_u16(5009); // port
        let packet = b.build_link_packet(LinkMsg::LeafLogin);

        let mut r = LinkPacketReader::new(&packet);
        assert_eq!(r.op().unwrap(), LinkMsg::LeafLogin);
        assert_eq!(r.read_string().unwrap(), "TestRoomName");
        assert_eq!(r.read_guid().unwrap(), [0xAA; 16]);
        assert_eq!(r.read_u16().unwrap(), 251);
        assert_eq!(r.read_u16().unwrap(), 5009);
    }

    #[test]
    fn userlist_item_roundtrip() {
        let mut b = LinkPacketBuilder::new();
        b.write_string("OriginalName");
        b.write_string("Bob");
        b.write_string("Ares 2.1.0");
        b.write_guid(&[0xBB; 16]);
        b.write_u16(100);
        b.write_ip("1.2.3.4".parse().unwrap());
        b.write_ip("192.168.1.1".parse().unwrap());
        b.write_u16(1234);
        b.write_string("dns.example.com");
        b.write_u8(1); // browsable
        b.write_u8(25); // age
        b.write_u8(1); // sex
        b.write_u8(49); // country
        b.write_string("US");
        b.write_u8(1); // level
        b.write_u16(0); // vroom
        b.write_u8(1); // custom_client
        b.write_u8(0); // muzzled
        b.write_u8(0); // web_client
        b.write_u8(0); // encrypted
        b.write_u8(1); // registered
        b.write_u8(0); // idle
        let packet = b.build_link_packet(LinkMsg::UserlistItem);

        // El packet tiene [u16 length, u8 op, args]. Usamos new (con skip).
        let mut r = LinkPacketReader::new(&packet);
        assert_eq!(r.op().unwrap(), LinkMsg::UserlistItem);
        assert_eq!(r.read_string().unwrap(), "OriginalName");
        assert_eq!(r.read_string().unwrap(), "Bob");
        assert_eq!(r.read_string().unwrap(), "Ares 2.1.0");
        assert_eq!(r.read_guid().unwrap(), [0xBB; 16]);
        assert_eq!(r.read_u16().unwrap(), 100);
        assert_eq!(r.read_ip().unwrap().to_string(), "1.2.3.4");
        assert_eq!(r.read_ip().unwrap().to_string(), "192.168.1.1");
        assert_eq!(r.read_u16().unwrap(), 1234);
        assert_eq!(r.read_string().unwrap(), "dns.example.com");
        assert_eq!(r.read_u8().unwrap(), 1);
        assert_eq!(r.read_u8().unwrap(), 25);
        assert_eq!(r.read_u8().unwrap(), 1);
        assert_eq!(r.read_u8().unwrap(), 49);
        assert_eq!(r.read_string().unwrap(), "US");
        assert_eq!(r.read_u8().unwrap(), 1);
        assert_eq!(r.read_u16().unwrap(), 0);
        assert_eq!(r.read_u8().unwrap(), 1);
        assert_eq!(r.read_u8().unwrap(), 0);
        assert_eq!(r.read_u8().unwrap(), 0);
        assert_eq!(r.read_u8().unwrap(), 0);
        assert_eq!(r.read_u8().unwrap(), 1);
        assert_eq!(r.read_u8().unwrap(), 0);
    }

    #[test]
    fn link_msg_opcodes_match_sb0t() {
        // Verifica que los opcodes EXACTOS coinciden con el sb0t original
        assert_eq!(LinkMsg::Error as u8, 0);
        assert_eq!(LinkMsg::LeafLogin as u8, 1);
        assert_eq!(LinkMsg::HubAck as u8, 3);
        assert_eq!(LinkMsg::HubLeafConnected as u8, 5);
        assert_eq!(LinkMsg::HubLeafDisconnected as u8, 6);
        assert_eq!(LinkMsg::LeafPing as u8, 7);
        assert_eq!(LinkMsg::HubPong as u8, 8);
        assert_eq!(LinkMsg::UserlistItem as u8, 10);
        assert_eq!(LinkMsg::Avatar as u8, 11);
        assert_eq!(LinkMsg::PersonalMessage as u8, 12);
        assert_eq!(LinkMsg::LeafUserlistEnd as u8, 14);
        assert_eq!(LinkMsg::LeafJoin as u8, 15);
        assert_eq!(LinkMsg::Part as u8, 16);
        assert_eq!(LinkMsg::UserUpdated as u8, 18);
        assert_eq!(LinkMsg::CustomName as u8, 19);
        assert_eq!(LinkMsg::PublicText as u8, 20);
        assert_eq!(LinkMsg::EmoteText as u8, 21);
        assert_eq!(LinkMsg::PrivateText as u8, 25);
        assert_eq!(LinkMsg::PrivateIgnored as u8, 27);
        assert_eq!(LinkMsg::PublicToUser as u8, 28);
        assert_eq!(LinkMsg::EmoteToUser as u8, 29);
        assert_eq!(LinkMsg::CustomDataTo as u8, 30);
        assert_eq!(LinkMsg::CustomDataAll as u8, 31);
        assert_eq!(LinkMsg::Nudge as u8, 32);
        assert_eq!(LinkMsg::ScribbleUser as u8, 33);
        assert_eq!(LinkMsg::ScribbleLeaf as u8, 34);
        assert_eq!(LinkMsg::NickChanged as u8, 40);
        assert_eq!(LinkMsg::VroomChanged as u8, 41);
        assert_eq!(LinkMsg::IUser as u8, 42);
        assert_eq!(LinkMsg::Admin as u8, 43);
        assert_eq!(LinkMsg::IUserBin as u8, 44);
        assert_eq!(LinkMsg::NoAdmin as u8, 45);
        assert_eq!(LinkMsg::Browse as u8, 50);
        assert_eq!(LinkMsg::BrowseData as u8, 51);
        assert_eq!(LinkMsg::PrintAll as u8, 60);
        assert_eq!(LinkMsg::PrintVroom as u8, 61);
        assert_eq!(LinkMsg::PrintLevel as u8, 62);
    }
}
