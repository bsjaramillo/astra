//! Parser completo de `MSG_CHAT_CLIENT_LOGIN`.
//!
//! Equivalente a `Login()` en `core/TCPProcessor.cs` (línea 643).
//!
//! ## Formato del paquete (después del opcode 2)
//!
//! ```text
//! offset tipo    descripción
//! 0      16B     GUID (MD5-hashed por el cliente)
//! 16     u16     FileCount
//! 18     u8      crypto (250 = encrypted)
//! 19     u16     DataPort
//! 21     4B      NodeIP (IPv4 big-endian)
//! 25     u16     NodePort
//! 27     skip 4  (reservado)
//! 31     string  OrgName
//! ?      string  Version
//! ?      4B      LocalIP (IPv4 big-endian)
//! ?      skip 4  (reservado)
//! ?      u8      Browsable
//! ?      u8      CurrentUploads
//! ?      u8      MaxUploads
//! ?      u8      CurrentQueued
//! ?      u8      Age
//! ?      u8      Sex
//! ?      u8      Country
//! ?      string  Region
//! ?      (opt) u8  VoiceChat capabilities
//! ```

#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr};

use proto_ares::{Guid, PacketReader};

use super::user_pool::AresUser;

/// Resultado del parseo del login.
#[derive(Debug)]
pub struct LoginData {
    /// GUID del usuario
    pub guid: [u8; 16],
    /// Cantidad de archivos compartidos
    pub file_count: u16,
    /// ¿Usa crypto?
    pub crypto: bool,
    /// Puerto de datos
    pub data_port: u16,
    /// IP del supernode
    pub node_ip: Ipv4Addr,
    /// Puerto del supernode
    pub node_port: u16,
    /// Nick original
    pub org_name: String,
    /// Versión del cliente
    pub version: String,
    /// ¿Es Ares oficial?
    pub is_ares: bool,
    /// ¿Es cbot?
    pub is_cbot: bool,
    /// IP local reportada
    pub local_ip: Ipv4Addr,
    /// ¿Browsable?
    pub browsable: bool,
    /// Uploads actuales
    pub current_uploads: u8,
    /// Uploads max
    pub max_uploads: u8,
    /// En cola
    pub current_queued: u8,
    /// Edad declarada
    pub age: u8,
    /// Sexo
    pub sex: u8,
    /// País
    pub country: u8,
    /// Región
    pub region: String,
    /// Voice chat public
    pub voice_chat_public: bool,
    /// Voice chat private
    pub voice_chat_private: bool,
    /// Opus voice chat public
    pub voice_opus_chat_public: bool,
    /// Opus voice chat private
    pub voice_opus_chat_private: bool,
    /// Soporta HTML
    pub supports_html: bool,
}

/// Error al parsear el login.
#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// Buffer muy corto
    #[error("login packet too short: {got} bytes, expected at least {need}")]
    TooShort {
        /// Bytes recibidos
        got: usize,
        /// Bytes necesarios
        need: usize,
    },
    /// Error de lectura
    #[error("read error at field '{field}': {source}")]
    Read {
        /// Campo que se intentaba leer
        field: &'static str,
        /// Error subyacente
        source: proto_ares::reader::ReadError,
    },
    /// Nick inválido (vacío o muy largo)
    #[error("invalid nickname: '{0}'")]
    InvalidName(String),
}

impl From<proto_ares::reader::ReadError> for LoginError {
    fn from(e: proto_ares::reader::ReadError) -> Self {
        Self::Read { field: "unknown", source: e }
    }
}

/// Parsea un paquete `MSG_CHAT_CLIENT_LOGIN` (con opcode incluido).
///
/// `data` debe incluir el byte de opcode al principio.
pub fn parse_login(data: &[u8]) -> Result<LoginData, LoginError> {
    if data.len() < 30 {
        return Err(LoginError::TooShort {
            got: data.len(),
            need: 30,
        });
    }
    // Skipeamos el opcode (1 byte)
    let mut r = PacketReader::new(&data[1..]);

    // 16 bytes GUID
    let guid_obj: Guid = r.read_guid().map_err(|source| LoginError::Read { field: "guid", source })?;
    let guid: [u8; 16] = *guid_obj.as_bytes();

    let file_count: u16 = r.read_u16_le().map_err(|source| LoginError::Read { field: "file_count", source })?;
    let crypto_byte: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "crypto", source })?;
    let crypto = crypto_byte == 250;

    let data_port: u16 = r.read_u16_le().map_err(|source| LoginError::Read { field: "data_port", source })?;
    let node_ip: Ipv4Addr = r.read_ipv4().map_err(|source| LoginError::Read { field: "node_ip", source })?;
    let node_port: u16 = r.read_u16_le().map_err(|source| LoginError::Read { field: "node_port", source })?;

    r.skip(4).map_err(|source| LoginError::Read { field: "skip4", source })?;

    let org_name = r.read_string().map_err(|source| LoginError::Read { field: "org_name", source })?;
    if org_name.is_empty() {
        return Err(LoginError::InvalidName(org_name));
    }

    let version = r.read_string().map_err(|source| LoginError::Read { field: "version", source })?;
    let is_ares = version.starts_with("Ares 2.") || version.starts_with("Ares_2.");
    let is_cbot = version.starts_with("cb0t ");

    let local_ip: Ipv4Addr = r.read_ipv4().map_err(|source| LoginError::Read { field: "local_ip", source })?;
    r.skip(4).map_err(|source| LoginError::Read { field: "skip4b", source })?;

    let browsable_byte: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "browsable", source })?;
    let mut browsable = browsable_byte > 2;
    let current_uploads: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "current_uploads", source })?;
    let max_uploads: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "max_uploads", source })?;
    let current_queued: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "current_queued", source })?;
    let age: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "age", source })?;
    let sex: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "sex", source })?;
    let country: u8 = r.read_u8().map_err(|source| LoginError::Read { field: "country", source })?;
    let mut region = r.read_string().map_err(|source| LoginError::Read { field: "region", source })?;
    if region.len() > 30 {
        region.truncate(30);
    }

    // Si fileCount es 0, browsable se vuelve false
    if file_count == 0 {
        browsable = false;
    }

    // Voice chat capabilities (opcional, puede faltar en clientes viejos)
    let (vc_pub, vc_priv, opus_pub, opus_priv, html) = if r.remaining() > 0 {
        let vc = r.read_u8().map_err(|source| LoginError::Read { field: "vc", source })?;
        (
            (vc & 1) == 1,
            (vc & 2) == 2,
            (vc & 4) == 4,
            (vc & 8) == 8,
            (vc & 16) == 16,
        )
    } else {
        (false, false, false, false, false)
    };

    Ok(LoginData {
        guid,
        file_count,
        crypto,
        data_port,
        node_ip,
        node_port,
        org_name,
        version,
        is_ares,
        is_cbot,
        local_ip,
        browsable,
        current_uploads,
        max_uploads,
        current_queued,
        age,
        sex,
        country,
        region,
        voice_chat_public: vc_pub || opus_pub,
        voice_chat_private: vc_priv || opus_priv,
        voice_opus_chat_public: opus_pub,
        voice_opus_chat_private: opus_priv,
        supports_html: html,
    })
}

/// Crea un `AresUser` a partir de los datos del login + IP externa.
///
/// Devuelve el `AresUser` sin envolver, para que el caller pueda setear
/// campos adicionales (ej. `sender`) antes de envolver en `Arc`.
pub fn build_ares_user(
    id: u16,
    external_ip: IpAddr,
    login: LoginData,
) -> AresUser {
    let mut user = AresUser::new(id, external_ip, login.guid);

    *user.name.write() = login.org_name.clone();
    *user.org_name.write() = login.org_name;
    user.version = login.version;
    user.file_count = login.file_count;
    user.data_port = login.data_port;
    user.node_ip = IpAddr::V4(login.node_ip);
    user.node_port = login.node_port;
    user.local_ip = IpAddr::V4(login.local_ip);
    user.browsable = login.browsable;
    user.age = login.age;
    user.sex = login.sex;
    user.country = login.country;
    user.region = login.region;
    user.voice_chat_public = login.voice_chat_public;
    user.voice_chat_private = login.voice_chat_private;
    user.voice_opus_chat_public = login.voice_opus_chat_public;
    user.voice_opus_chat_private = login.voice_opus_chat_private;
    user.encrypted = login.crypto;
    user.ares = login.is_ares;
    user.cbot = login.is_cbot;
    user.supports_html = login.supports_html;

    user
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto_ares::PacketWriter;

    /// Construye un paquete de login completo y devuelve sus bytes (con opcode).
    fn make_login_bytes(
        name: &str,
        version: &str,
        guid_byte: u8,
        file_count: u16,
        node_ip: Ipv4Addr,
        node_port: u16,
        local_ip: Ipv4Addr,
        voice_byte: Option<u8>,
    ) -> Vec<u8> {
        let mut w = PacketWriter::with_msg(proto_ares::TcpMsg::ClientLogin);
        // 16 bytes GUID (el cliente Ares aplica MD5, nosotros aplicamos MD5 también al parsear)
        // Para el test, escribimos bytes arbitrarios (Guid::from_bytes los hashea con MD5)
        let guid = [guid_byte; 16];
        w.write_bytes(&guid).unwrap();
        w.write_u16_le(file_count).unwrap();
        w.write_u8(0).unwrap(); // crypto byte (0 = no crypto)
        w.write_u16_le(1234).unwrap(); // data_port
        w.write_ipv4(node_ip).unwrap();
        w.write_u16_le(node_port).unwrap();
        w.write_bytes(&[0, 0, 0, 0]).unwrap(); // skip 4
        w.write_string(name).unwrap();
        w.write_string(version).unwrap();
        w.write_ipv4(local_ip).unwrap();
        w.write_bytes(&[0, 0, 0, 0]).unwrap(); // skip 4
        w.write_u8(3).unwrap(); // browsable (> 2)
        w.write_u8(0).unwrap(); // current_uploads
        w.write_u8(3).unwrap(); // max_uploads
        w.write_u8(0).unwrap(); // current_queued
        w.write_u8(25).unwrap(); // age
        w.write_u8(1).unwrap(); // sex (male)
        w.write_u8(49).unwrap(); // country
        w.write_string("US").unwrap(); // region
        if let Some(vc) = voice_byte {
            w.write_u8(vc).unwrap();
        }
        w.into_bytes()
    }

    #[test]
    fn parse_login_basic() {
        let bytes = make_login_bytes(
            "Alice",
            "Ares 2.1.0",
            0x42,
            100,
            Ipv4Addr::new(1, 2, 3, 4),
            5009,
            Ipv4Addr::new(192, 168, 1, 100),
            Some(0x15), // vc public + html
        );
        let login = parse_login(&bytes).expect("parse failed");
        assert_eq!(login.org_name, "Alice");
        assert_eq!(login.version, "Ares 2.1.0");
        assert!(login.is_ares);
        assert!(!login.is_cbot);
        assert_eq!(login.file_count, 100);
        assert_eq!(login.data_port, 1234);
        assert_eq!(login.node_ip, Ipv4Addr::new(1, 2, 3, 4));
        assert_eq!(login.node_port, 5009);
        assert_eq!(login.local_ip, Ipv4Addr::new(192, 168, 1, 100));
        assert!(login.browsable);
        assert_eq!(login.age, 25);
        assert_eq!(login.sex, 1);
        assert_eq!(login.country, 49);
        assert_eq!(login.region, "US");
        assert!(login.voice_chat_public);
        assert!(!login.voice_chat_private);
        assert!(login.voice_opus_chat_public);
        assert!(login.supports_html);
    }

    #[test]
    fn parse_login_cbot() {
        let bytes = make_login_bytes(
            "bot",
            "cb0t 5.43",
            0x01,
            0,
            Ipv4Addr::LOCALHOST,
            5009,
            Ipv4Addr::LOCALHOST,
            None,
        );
        let login = parse_login(&bytes).expect("parse failed");
        assert!(login.is_cbot);
        assert!(!login.is_ares);
        assert_eq!(login.file_count, 0);
        assert!(!login.browsable); // file_count == 0 => no browsable
    }

    #[test]
    fn parse_login_no_voice() {
        let bytes = make_login_bytes(
            "Bob",
            "Ares 2.1.0",
            0x33,
            50,
            Ipv4Addr::new(8, 8, 8, 8),
            5009,
            Ipv4Addr::new(8, 8, 8, 8),
            None,
        );
        let login = parse_login(&bytes).unwrap();
        assert!(!login.voice_chat_public);
        assert!(!login.voice_chat_private);
        assert!(!login.supports_html);
    }

    #[test]
    fn parse_login_encrypted() {
        let mut w = PacketWriter::with_msg(proto_ares::TcpMsg::ClientLogin);
        w.write_bytes(&[0xAA; 16]).unwrap();
        w.write_u16_le(0).unwrap();
        w.write_u8(250).unwrap(); // crypto byte 250 = encrypted
        w.write_u16_le(0).unwrap();
        w.write_ipv4(Ipv4Addr::LOCALHOST).unwrap();
        w.write_u16_le(5009).unwrap();
        w.write_bytes(&[0; 4]).unwrap();
        w.write_string("Eve").unwrap();
        w.write_string("Ares 2.1.0").unwrap();
        w.write_ipv4(Ipv4Addr::LOCALHOST).unwrap();
        w.write_bytes(&[0; 4]).unwrap();
        w.write_u8(1).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_string("").unwrap();
        let bytes = w.into_bytes();
        let login = parse_login(&bytes).unwrap();
        assert!(login.crypto);
    }

    #[test]
    fn parse_login_truncates_long_region() {
        // Construimos el paquete a mano, con una region de 50 chars
        let long_region = "A".repeat(50);
        let mut w = PacketWriter::with_msg(proto_ares::TcpMsg::ClientLogin);
        w.write_bytes(&[0x10; 16]).unwrap(); // guid
        w.write_u16_le(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u16_le(0).unwrap();
        w.write_ipv4(Ipv4Addr::LOCALHOST).unwrap();
        w.write_u16_le(5009).unwrap();
        w.write_bytes(&[0; 4]).unwrap();
        w.write_string("X").unwrap();
        w.write_string("Ares 2.1.0").unwrap();
        w.write_ipv4(Ipv4Addr::LOCALHOST).unwrap();
        w.write_bytes(&[0; 4]).unwrap();
        w.write_u8(1).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_u8(0).unwrap();
        w.write_string(&long_region).unwrap();
        let bytes = w.into_bytes();
        let login = parse_login(&bytes).unwrap();
        assert_eq!(login.region.len(), 30);
    }

    #[test]
    fn parse_login_too_short() {
        let bytes = vec![0x02, 0x01, 0x02, 0x03];
        assert!(parse_login(&bytes).is_err());
    }

    #[test]
    fn build_ares_user_from_login() {
        let bytes = make_login_bytes(
            "TestUser",
            "Ares 2.1.0",
            0x77,
            42,
            Ipv4Addr::new(1, 1, 1, 1),
            5009,
            Ipv4Addr::new(2, 2, 2, 2),
            Some(0x10), // supports_html
        );
        let login = parse_login(&bytes).unwrap();
        let user = build_ares_user(42, IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)), login);
        assert_eq!(user.id, 42);
        assert_eq!(user.name.read().as_str(), "TestUser");
        assert_eq!(user.version, "Ares 2.1.0");
        assert_eq!(user.external_ip, IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)));
        assert!(user.supports_html);
    }
}
