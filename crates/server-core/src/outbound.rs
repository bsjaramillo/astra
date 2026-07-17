//! Constructores de paquetes salientes (server → client).
//!
//! Equivalente directo de `core/TCPOutbound.cs` del sb0t original.
//! Cada función devuelve `Bytes` listos para enviar al socket del cliente.

use std::net::{IpAddr, Ipv4Addr};

use bytes::Bytes;
use proto_ares::{AresCrypto, PacketWriter, TcpMsg};

use crate::user_pool::AresUser;

/// Crypto del destinatario para cifrar los strings de un paquete. `None` =
/// cliente sin cifrar (o WS): strings null-terminated en claro.
pub type Crypto = Option<AresCrypto>;

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
    build_join_or_userlist_c(user, None)
}

/// Variante con cifrado para el destinatario (`crypto`).
pub fn build_join_or_userlist_c(user: &AresUser, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerJoin, crypto);
    w.write_u16_le(user.file_count).ok();
    w.write_u32_le(0).ok(); // reservado
    write_ip(&mut w, &user.external_ip);
    w.write_u16_le(user.data_port).ok();
    write_ip(&mut w, &user.node_ip);
    w.write_u16_le(user.node_port).ok();
    w.write_u8(0).ok(); // reservado
    w.write_string_nt(&user.name.read()).ok();
    write_ip(&mut w, &user.local_ip);
    w.write_u8(user.browsable as u8).ok();
    w.write_u8(level_to_u8(&*user.level.read())).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string_nt(&user.region).ok();
    w.write_u8(build_features(user)).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye la respuesta a `ClientUpdateStatus`: paridad exacta de
/// `TCPOutbound.UpdateUserStatus` de sb0t. A diferencia de
/// `build_join_or_userlist_c` (que usa otro opcode y SÍ se difunde a toda la
/// sala), este paquete se manda ÚNICAMENTE de vuelta al cliente que lo pidió
/// — sb0t nunca lo broadcastea (`client.SendPacket(...)`, no
/// `Server.Users.SendAll`). Reutilizar el opcode de JOIN para esto (como
/// hacía una versión anterior de Astra) hacía que cualquier cliente que
/// manda `ClientUpdateStatus` periódicamente (varios bots cb0t lo hacen como
/// keep-alive) disparara un "X has joined" fantasma en cada cliente web,
/// sin que el usuario se hubiera ido ni vuelto a entrar.
///
/// Formato (de `TCPOutbound.cs` `UpdateUserStatus`):
/// ```text
/// str  name
/// u16  file_count
/// u8   browsable (1/0)
/// IPv4 node_ip
/// u16  node_port
/// IPv4 external_ip (0.0.0.0 si el cliente no es Ares nativo)
/// u8   level
/// u8   age
/// u8   sex
/// u8   country
/// str  region
/// ```
pub fn build_update_user_status_c(user: &AresUser, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerUpdateUserStatus, crypto);
    w.write_string_nt(&user.name.read()).ok();
    w.write_u16_le(user.file_count).ok();
    w.write_u8(user.browsable as u8).ok();
    write_ip(&mut w, &user.node_ip);
    w.write_u16_le(user.node_port).ok();
    let reported_ip = if user.ares {
        user.external_ip
    } else {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    };
    write_ip(&mut w, &reported_ip);
    w.write_u8(level_to_u8(&*user.level.read())).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string_nt(&user.region).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un PART (un usuario se fue).
///
/// Formato: `str name`
pub fn build_part(user: &AresUser) -> Bytes {
    build_part_c(user, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_part_c(user: &AresUser, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerPart, crypto);
    w.write_string_nt(&user.name.read()).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un AVATAR (server → cliente Ares): notifica el avatar de
/// `target_name` a un destinatario (paridad `TCPOutbound.Avatar`).
///
/// Formato: `str target_name` + `bytes avatar_png` (sin largo explícito:
/// el resto del paquete es el PNG).
pub fn build_avatar_c(target_name: &str, avatar: &[u8], crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::Avatar, crypto);
    w.write_string_nt(target_name).ok();
    w.write_bytes(avatar).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un AVATAR vacío (limpia el avatar de `target_name` en el
/// cliente, paridad `TCPOutbound.AvatarCleared`).
pub fn build_avatar_cleared_c(target_name: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::Avatar, crypto);
    w.write_string_nt(target_name).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un PERSONAL_MESSAGE (server → cliente Ares): notifica el
/// mensaje personal de `target_name` (paridad `TCPOutbound.PersonalMessage`).
pub fn build_personal_message_c(target_name: &str, text: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::PersonalMessage, crypto);
    w.write_string_nt(target_name).ok();
    w.write_string_nt(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Construye un USERLIST (un usuario en la lista).
/// Es el mismo formato que JOIN.
pub fn build_userlist_item(user: &AresUser) -> Bytes {
    build_userlist_item_c(user, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_userlist_item_c(user: &AresUser, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerChannelUserList, crypto);
    w.write_u16_le(user.file_count).ok();
    w.write_u32_le(0).ok();
    write_ip(&mut w, &user.external_ip);
    w.write_u16_le(user.data_port).ok();
    write_ip(&mut w, &user.node_ip);
    w.write_u16_le(user.node_port).ok();
    w.write_u8(0).ok();
    w.write_string_nt(&user.name.read()).ok();
    write_ip(&mut w, &user.local_ip);
    w.write_u8(user.browsable as u8).ok();
    w.write_u8(level_to_u8(&*user.level.read())).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string_nt(&user.region).ok();
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
    build_userlist_bot_c(bot_name, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_userlist_bot_c(bot_name: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerChannelUserList, crypto);
    w.write_u16_le(0).ok();
    w.write_u32_le(0).ok();
    w.write_ipv4(Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u16_le(69).ok();
    w.write_ipv4(Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u16_le(0).ok();
    w.write_u8(0).ok();
    w.write_string_nt(bot_name).ok();
    w.write_ipv4(Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u8(1).ok();
    w.write_u8(3).ok(); // level 3
    w.write_u8(0).ok();
    w.write_u8(0).ok();
    w.write_u8(0).ok();
    w.write_string_nt("").ok();
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
    build_topic_first_c(text, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_topic_first_c(text: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerTopicFirst, crypto);
    w.write_string_nt(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Topic (broadcast cuando alguien lo cambia).
pub fn build_topic(text: &str) -> Bytes {
    build_topic_c(text, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_topic_c(text: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerTopic, crypto);
    w.write_string_nt(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Custom data a un cliente custom (`MSG_CHAT_SERVER_CUSTOM_DATA`, op 200).
/// Formato sb0t `TCPOutbound.CustomData`: `str ident, str sender, bytes`.
/// Lo usan el nudge (`cb0t_nudge`) y el scribble dirigido
/// (`cb0t_scribble_once/first/chunk/last`).
pub fn build_custom_data_c(ident: &str, sender: &str, data: &[u8], crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::CustomData, crypto);
    w.write_string_nt(ident).ok();
    w.write_string_nt(sender).ok();
    w.write_bytes(data).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Error fatal del server (`ServerError`) — aviso antes de expulsar.
pub fn build_server_error_c(text: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerError, crypto);
    w.write_string_nt(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Redirect a otro servidor (`MSG_CHAT_SERVER_REDIRECT`, opcode 6).
///
/// Formato sb0t: `ip, u16 port, ip, str room_name, str "Redirecting..."`.
/// El cliente Ares cierra y se reconecta al `ip:port` indicado.
pub fn build_redirect(ip: std::net::IpAddr, port: u16, room_name: &str) -> Bytes {
    build_redirect_c(ip, port, room_name, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_redirect_c(ip: std::net::IpAddr, port: u16, room_name: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerRedirect, crypto);
    write_ip(&mut w, &ip);
    w.write_u16_le(port).ok();
    write_ip(&mut w, &ip);
    w.write_string_nt(room_name).ok();
    w.write_string_nt("Redirecting...").ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Mensaje público (broadcast).
///
/// Formato: `str name, str text`
pub fn build_public(from_name: &str, text: &str) -> Bytes {
    build_public_c(from_name, text, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_public_c(from_name: &str, text: &str, crypto: Crypto) -> Bytes {
    let text = truncate_message(text, 300);
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::Public, crypto);
    w.write_string_nt(from_name).ok();
    w.write_string_nt(&text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Emote (broadcast).
///
/// Formato: `str name, str text`
pub fn build_emote(from_name: &str, text: &str) -> Bytes {
    build_emote_c(from_name, text, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_emote_c(from_name: &str, text: &str, crypto: Crypto) -> Bytes {
    let text = truncate_message(text, 300);
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::Emote, crypto);
    w.write_string_nt(from_name).ok();
    w.write_string_nt(&text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

/// Mensaje privado (PM).
///
/// Formato: `str from_name, str text`
pub fn build_pvt(from_name: &str, text: &str) -> Bytes {
    build_pvt_c(from_name, text, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_pvt_c(from_name: &str, text: &str, crypto: Crypto) -> Bytes {
    let text = truncate_message(text, 300);
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::Pmt, crypto);
    w.write_string_nt(from_name).ok();
    w.write_string_nt(&text).ok();
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
    build_url_c(addr, text, None)
}

/// Variante con cifrado para el destinatario.
pub fn build_url_c(addr: &str, text: &str, crypto: Crypto) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerUrl, crypto);
    w.write_string_nt(addr).ok();
    w.write_string_nt(text).ok();
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
        // Strings null-terminated (formato Ares sin cifrar): "Alice\0"
        assert_eq!(&pkt[1..], b"Alice\x00");
    }

    #[test]
    fn public_packet_format() {
        let pkt = build_public("Alice", "hello");
        assert_eq!(pkt[0], TcpMsg::Public as u8);
        // Dos strings null-terminated consecutivas: "Alice\0hello\0"
        assert_eq!(&pkt[1..], b"Alice\x00hello\x00");
    }

    #[test]
    fn truncate_long_message() {
        let long = "a".repeat(1000);
        let pkt = build_public("A", &long);
        // Tras opcode + "A\0", el texto se trunca a 300 chars, luego null.
        // pkt = [op]"A\0"("a"*300)"\0"
        let text = &pkt[1 + 2..]; // salta opcode + "A\0"
        let nul = text.iter().position(|&b| b == 0).unwrap();
        assert_eq!(nul, 300);
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
