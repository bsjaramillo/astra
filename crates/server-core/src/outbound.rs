//! Constructores de paquetes salientes (server → client).
//!
//! Equivalente directo de `core/TCPOutbound.cs` del sb0t original.
//! Cada función devuelve `Bytes` listos para enviar al socket del cliente.

use std::net::{IpAddr, Ipv4Addr};

use bytes::Bytes;
use proto_ares::{PacketWriter, TcpMsg};

use crate::user_pool::AresUser;

/// Flags de capabilities de un cliente (byte de features en JOIN/USERLIST).
pub mod features {
    /// Soporta voice chat público
    pub const CLIENT_SUPPORTS_VC: u8 = 1;
    /// Soporta voice chat privado
    pub const CLIENT_SUPPORTS_PM_VC: u8 = 2;
    /// Soporta Opus voice chat público
    pub const CLIENT_SUPPORTS_OPUS_VC: u8 = 4;
    /// Soporta Opus voice chat privado
    pub const CLIENT_SUPPORTS_OPUS_PM_VC: u8 = 8;
    /// Soporta HTML
    pub const CLIENT_SUPPORTS_HTML: u8 = 16;
}

/// Construye el byte de features para un usuario.
pub fn build_features(user: &AresUser) -> u8 {
    let mut f = 0u8;
    if user.voice_chat_public {
        f |= features::CLIENT_SUPPORTS_VC;
    }
    if user.voice_chat_private {
        f |= features::CLIENT_SUPPORTS_PM_VC;
    }
    if user.voice_opus_chat_public {
        f |= features::CLIENT_SUPPORTS_OPUS_VC;
    }
    if user.supports_html {
        f |= features::CLIENT_SUPPORTS_HTML;
    }
    f
}

/// Construye el payload de un JOIN / USERLIST (mismo formato).
///
/// Formato (de `TCPOutbound.cs` `Join` y `Userlist`):
/// ```text
/// u16  file_count
/// u32  (reservado, 0)
/// IPv4 external_ip
/// u16  data_port
/// IPv4 node_ip
/// u16  node_port
/// u8   (reservado, 0)
/// str  name
/// IPv4 local_ip
/// u8   browsable (1/0)
/// u8   level
/// u8   age
/// u8   sex
/// u8   country
/// str  region
/// u8   features
/// ```
pub fn build_join_or_userlist(user: &AresUser) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerJoin);
    w.write_u16_le(user.file_count).ok();
    w.write_u32_le(0).ok(); // reservado
    write_ip(&mut w, &user.external_ip);
    w.write_u16_le(user.data_port).ok();
    write_ip(&mut w, &user.node_ip);
    w.write_u16_le(user.node_port).ok();
    w.write_u8(0).ok(); // reservado
    w.write_string(&user.name.read()).ok();
    write_ip(&mut w, &user.local_ip);
    w.write_u8(user.browsable as u8).ok();
    w.write_u8(level_to_u8(&*user.level.read())).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string(&user.region).ok();
    w.write_u8(build_features(user)).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un PART (un usuario se fue).
///
/// Formato: `str name`
pub fn build_part(user: &AresUser) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerPart);
    w.write_string(&user.name.read()).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un USERLIST (un usuario en la lista).
/// Es el mismo formato que JOIN.
pub fn build_userlist_item(user: &AresUser) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerChannelUserList);
    w.write_u16_le(user.file_count).ok();
    w.write_u32_le(0).ok();
    write_ip(&mut w, &user.external_ip);
    w.write_u16_le(user.data_port).ok();
    write_ip(&mut w, &user.node_ip);
    w.write_u16_le(user.node_port).ok();
    w.write_u8(0).ok();
    w.write_string(&user.name.read()).ok();
    write_ip(&mut w, &user.local_ip);
    w.write_u8(user.browsable as u8).ok();
    w.write_u8(level_to_u8(&*user.level.read())).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string(&user.region).ok();
    w.write_u8(build_features(user)).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Convierte un ILevel a su valor numérico (u8).
fn level_to_u8(level: &crate::types::ILevel) -> u8 {
    match level {
        crate::types::ILevel::Anonymous => 0,
        crate::types::ILevel::Regular => 1,
        crate::types::ILevel::Voice => 2,
        crate::types::ILevel::Moderator => 50,
        crate::types::ILevel::Admin => 80,
        crate::types::ILevel::Owner => 100,
        crate::types::ILevel::System => 255,
    }
}

/// Construye el "bot" que aparece en la lista de usuarios (línea fantasma).
///
/// De `TCPOutbound.cs` `UserlistBot`:
/// ```text
/// u16  file_count = 0
/// u32  (reservado, 0)
/// IPv4 0.0.0.0
/// u16  data_port = 69
/// IPv4 0.0.0.0
/// u16  node_port = 0
/// u8   0
/// str  bot_name (de settings)
/// IPv4 0.0.0.0
/// u8   1 (browsable)
/// u8   3 (level: host-ish)
/// u8   0 (age)
/// u8   0 (sex)
/// u8   0 (country)
/// str  "" (region)
/// ```
pub fn build_userlist_bot(bot_name: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerChannelUserList);
    w.write_u16_le(0).ok();
    w.write_u32_le(0).ok();
    w.write_ipv4(Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u16_le(69).ok();
    w.write_ipv4(Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u16_le(0).ok();
    w.write_u8(0).ok();
    w.write_string(bot_name).ok();
    w.write_ipv4(Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u8(1).ok();
    w.write_u8(3).ok(); // level 3
    w.write_u8(0).ok();
    w.write_u8(0).ok();
    w.write_u8(0).ok();
    w.write_string("").ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Fin de la lista de usuarios.
///
/// Formato: `u8 0`
pub fn build_userlist_end() -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerChannelUserListEnd);
    w.write_u8(0).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Topic (enviado al unirse).
pub fn build_topic_first(text: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerTopicFirst);
    w.write_string(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Topic (broadcast cuando alguien lo cambia).
pub fn build_topic(text: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerTopic);
    w.write_string(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Mensaje público (broadcast).
///
/// Formato: `str name, str text`
pub fn build_public(from_name: &str, text: &str) -> Bytes {
    let text = truncate_message(text, 300);
    let mut w = PacketWriter::with_msg(TcpMsg::Public);
    w.write_string(from_name).ok();
    w.write_string(&text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Emote (broadcast).
///
/// Formato: `str name, str text`
pub fn build_emote(from_name: &str, text: &str) -> Bytes {
    let text = truncate_message(text, 300);
    let mut w = PacketWriter::with_msg(TcpMsg::Emote);
    w.write_string(from_name).ok();
    w.write_string(&text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Mensaje privado (PM).
///
/// Formato: `str from_name, str text`
pub fn build_pvt(from_name: &str, text: &str) -> Bytes {
    let text = truncate_message(text, 300);
    let mut w = PacketWriter::with_msg(TcpMsg::Pmt);
    w.write_string(from_name).ok();
    w.write_string(&text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// OpChange (cambio de nivel del usuario local).
///
/// Formato: `u8 (1 si level > 0, 0 si es regular)`
pub fn build_opchange(is_op: bool) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerOpChange);
    w.write_u8(if is_op { 1 } else { 0 }).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// URL tag de la sala (enviado al unirse).
pub fn build_url(addr: &str, text: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerUrl);
    w.write_string(addr).ok();
    w.write_string(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Trunca un mensaje a `max_chars` chars (o bytes UTF-8, lo que sea menor).
fn truncate_message(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// Helper para escribir una IP (maneja V4 y V6).
fn write_ip(w: &mut PacketWriter, ip: &IpAddr) {
    match ip {
        IpAddr::V4(v4) => w.write_ipv4(*v4).ok(),
        IpAddr::V6(v6) => w.write_bytes(&v6.octets()).ok(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_pool::AresUser;
    use std::net::Ipv4Addr;

    fn make_test_user() -> std::sync::Arc<AresUser> {
        use tokio::sync::mpsc;
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut user = AresUser::new(1, IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)), [1; 16]);
        user.sender = Some(tx);
        *user.name.write() = "Alice".to_string();
        user.age = 25;
        user.sex = 1;
        user.country = 49;
        user.region = "US".to_string();
        std::sync::Arc::new(user)
    }

    #[test]
    fn join_packet_opcode() {
        let u = make_test_user();
        let pkt = build_join_or_userlist(&u);
        assert_eq!(pkt[0], TcpMsg::ServerJoin as u8);
    }

    #[test]
    fn public_uses_public_opcode() {
        let pkt = build_public("Alice", "hi");
        // opcode 10 = Public (shared client/server)
        assert_eq!(pkt[0], 10u8);
    }

    #[test]
    fn part_packet_format() {
        let u = make_test_user();
        let pkt = build_part(&u);
        assert_eq!(pkt[0], TcpMsg::ServerPart as u8);
        // Después del opcode: i32 len(5) + "Alice"
        assert_eq!(&pkt[1..5], &[0x05, 0x00, 0x00, 0x00]);
        assert_eq!(&pkt[5..], b"Alice");
    }

    #[test]
    fn public_packet_format() {
        let pkt = build_public("Alice", "hello");
        assert_eq!(pkt[0], TcpMsg::Public as u8);
        // string 1: len(5) + "Alice"
        assert_eq!(&pkt[1..5], &[0x05, 0x00, 0x00, 0x00]);
        assert_eq!(&pkt[5..10], b"Alice");
        // string 2: len(5) + "hello"
        assert_eq!(&pkt[10..14], &[0x05, 0x00, 0x00, 0x00]);
        assert_eq!(&pkt[14..], b"hello");
    }

    #[test]
    fn truncate_long_message() {
        let long = "a".repeat(1000);
        let pkt = build_public("A", &long);
        // Después del opcode + 1 string, debería haber un i32=300 + 300 'a's
        let text_len_offset = 1 + 4 + 1; // opcode + str1 len + str1
        let len_bytes = &pkt[text_len_offset..text_len_offset + 4];
        let len = i32::from_le_bytes(len_bytes.try_into().unwrap());
        assert_eq!(len, 300);
    }

    #[test]
    fn userlist_end() {
        let pkt = build_userlist_end();
        assert_eq!(pkt[0], TcpMsg::ServerChannelUserListEnd as u8);
        assert_eq!(pkt[1], 0);
    }

    #[test]
    fn bot_userlist() {
        let pkt = build_userlist_bot("MyBot");
        assert_eq!(pkt[0], TcpMsg::ServerChannelUserList as u8);
    }

    #[test]
    fn pvt_packet() {
        let pkt = build_pvt("Bob", "secret");
        assert_eq!(pkt[0], TcpMsg::Pmt as u8);
    }

    #[test]
    fn topic_first() {
        let pkt = build_topic_first("Welcome to Astra");
        assert_eq!(pkt[0], TcpMsg::ServerTopicFirst as u8);
    }
}
