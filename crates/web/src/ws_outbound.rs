//! Constructores de mensajes en formato WebSocket (texto).
//!
//! Equivalente a `server_core::outbound` pero produciendo strings
//! en lugar de bytes binarios, para clientes ib0t/HTML5.

use bytes::Bytes;
use server_core::outbound as bin;
use server_core::user_pool::AresUser;

use crate::protocol::{
    build_emote as build_emote_text, build_opchange, build_part as build_part_text,
    build_pm as build_pm_text, build_public as build_public_text,
    build_userlist as build_userlist_text, build_userlist_bot, build_userlist_end,
    build_user_item, build_topic, build_ack, build_myfeatures,
};

/// Traduce un paquete binario de broadcast al formato texto WS.
///
/// Retorna `Some(texto)` si el paquete es un broadcast conocido (PUBLIC/EMOTE/PM/JOIN/PART/USERLIST).
/// Retorna `None` si no se puede traducir (paquete desconocido).
pub fn translate_broadcast(pkt: &Bytes, sender: &AresUser) -> Option<String> {
    if pkt.is_empty() {
        return None;
    }
    let opcode = pkt[0];
    let data = &pkt[1..];

    use proto_ares::TcpMsg;
    match TcpMsg::from_u8(opcode) {
        Some(TcpMsg::Public) => {
            let mut r = proto_ares::PacketReader::new(data);
            let name = r.read_string().ok()?;
            let text = r.read_string().ok()?;
            Some(build_public_text(&name, &text))
        }
        Some(TcpMsg::Emote) => {
            let mut r = proto_ares::PacketReader::new(data);
            let name = r.read_string().ok()?;
            let text = r.read_string().ok()?;
            Some(build_emote_text(&name, &text))
        }
        Some(TcpMsg::Pmt) => {
            let mut r = proto_ares::PacketReader::new(data);
            let from = r.read_string().ok()?;
            let text = r.read_string().ok()?;
            Some(build_pm_text(&from, &text))
        }
        Some(TcpMsg::ServerPart) => {
            let mut r = proto_ares::PacketReader::new(data);
            let name = r.read_string().ok()?;
            Some(build_part_text(&name))
        }
        Some(TcpMsg::ServerJoin) => {
            // JOIN:port,users,filecount,extip,dataport,nodeip,nodeport,reserved,name,localip,browsable,level,age,sex,country,region,features
            // Para WS, podemos mandar un JOIN simplificado con la info esencial
            let mut r = proto_ares::PacketReader::new(data);
            let port = r.read_u16_le().ok()?;
            let users = r.read_u16_le().ok()?;
            let _file_count = r.read_u16_le().ok()?;
            let _ext_ip = r.read_ip().ok()?;
            let _data_port = r.read_u16_le().ok()?;
            let _node_ip = r.read_ip().ok()?;
            let _node_port = r.read_u16_le().ok()?;
            let _reserved = r.read_u8().ok()?;
            let name = r.read_string().ok()?;
            let _local_ip = r.read_ip().ok()?;
            let _browsable = r.read_u8().ok()?;
            let level = r.read_u8().ok()?;
            let _age = r.read_u8().ok()?;
            let _sex = r.read_u8().ok()?;
            let _country = r.read_u8().ok()?;
            let _region = r.read_string().ok()?;
            let features = r.read_u8().ok()?;
            // Formato WS: JOIN:name,port,users,level,features
            Some(format!("JOIN:{},{},{},{},{}", name, port, users, level, features))
        }
        Some(TcpMsg::ServerChannelUserList) => {
            // Mismo formato que JOIN, pero opcode 30
            let mut r = proto_ares::PacketReader::new(data);
            let _port = r.read_u16_le().ok()?;
            let users = r.read_u16_le().ok()?;
            let _file_count = r.read_u16_le().ok()?;
            let _ext_ip = r.read_ip().ok()?;
            let _data_port = r.read_u16_le().ok()?;
            let _node_ip = r.read_ip().ok()?;
            let _node_port = r.read_u16_le().ok()?;
            let _reserved = r.read_u8().ok()?;
            let name = r.read_string().ok()?;
            let _local_ip = r.read_ip().ok()?;
            let _browsable = r.read_u8().ok()?;
            let level = r.read_u8().ok()?;
            let _age = r.read_u8().ok()?;
            let _sex = r.read_u8().ok()?;
            let _country = r.read_u8().ok()?;
            let _region = r.read_string().ok()?;
            let features = r.read_u8().ok()?;
            // USERLIST:port,users,...,name,...,level,features
            Some(format!("USERLIST:{},{},{},{},{}", 0, users, name, level, features))
        }
        _ => None,
    }
}

/// Construye el estado inicial en formato WS para un user.
pub fn build_initial_state_ws(
    user: &AresUser,
    room_name: &str,
    room_topic: &str,
    bot_name: &str,
) -> Vec<String> {
    let mut msgs = Vec::new();
    msgs.push(build_ack(&user.name.read(), room_name, &user.version));
    msgs.push(build_myfeatures(&user.version, 0x1F, 0, 0));
    msgs.push(build_topic(room_topic));
    msgs.push(build_userlist_bot(bot_name));
    // Los userlist items se agregan aparte
    let _ = user; // evitar warning
    msgs.push(build_userlist_end());
    let level = *user.level.read() as u8;
    msgs.push(build_opchange(level));
    msgs
}

/// Construye un item de userlist en formato WS para un user.
pub fn build_userlist_item_ws(user: &AresUser) -> String {
    let features = bin::build_features(user);
    build_user_item(
        0,
        0,
        user.file_count,
        user.external_ip,
        user.data_port,
        user.node_ip,
        user.node_port,
        &user.name.read(),
        user.local_ip,
        user.browsable,
        *user.level.read() as u8,
        user.age,
        user.sex,
        user.country,
        &user.region,
        features,
    )
}

// Re-exports para tests
pub use crate::protocol::build_userlist as _build_userlist_rexport;
