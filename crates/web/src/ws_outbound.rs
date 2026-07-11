//! Traducción de paquetes binarios de broadcast al formato texto ib0t/sb0t.
//!
//! Los clientes web (ib0t/inbizio) hablan un protocolo de texto length-prefixed
//! (ver `protocol.rs`). Cuando el server hace broadcast de un paquete binario
//! Ares, para los usuarios web lo traducimos al mensaje de texto equivalente.

use bytes::Bytes;
use server_core::user_pool::AresUser;

use crate::protocol::{
    build_emote, build_joininfo, build_part, build_pm, build_public, build_userlist_item,
};

/// Traduce un paquete binario de broadcast al formato texto WS, para un
/// `recipient` web concreto (el formato de JOIN difiere entre clientes
/// inbizier y clientes simples).
///
/// `sender` es el usuario que originó el broadcast (para JOIN, el que entra).
/// Retorna `None` si el paquete no tiene equivalente de texto.
pub fn translate_broadcast(pkt: &Bytes, sender: &AresUser, recipient: &AresUser) -> Option<String> {
    if pkt.is_empty() {
        return None;
    }
    let opcode = pkt[0];
    let data = &pkt[1..];

    use proto_ares::TcpMsg;
    match TcpMsg::from_u8(opcode) {
        Some(TcpMsg::Public) => {
            let mut r = proto_ares::PacketReader::new(data);
            let name = r.read_string_nt().ok()?;
            let text = r.read_string_nt().ok()?;
            Some(build_public(&name, &text))
        }
        Some(TcpMsg::Emote) => {
            let mut r = proto_ares::PacketReader::new(data);
            let name = r.read_string_nt().ok()?;
            let text = r.read_string_nt().ok()?;
            Some(build_emote(&name, &text))
        }
        Some(TcpMsg::Pmt) => {
            let mut r = proto_ares::PacketReader::new(data);
            let from = r.read_string_nt().ok()?;
            let text = r.read_string_nt().ok()?;
            Some(build_pm(&from, &text))
        }
        Some(TcpMsg::ServerPart) => {
            let mut r = proto_ares::PacketReader::new(data);
            let name = r.read_string_nt().ok()?;
            Some(build_part(&name))
        }
        Some(TcpMsg::ServerJoin) | Some(TcpMsg::ServerChannelUserList) => {
            // El join se anuncia a los DEMÁS, no al que entra (que ya recibió su
            // estado inicial vía la userlist). Evita el duplicado de sí mismo.
            if recipient.id == sender.id {
                return None;
            }
            // El paquete trae name+level, pero `sender` es el usuario que entra
            // y tiene la info rica (pmsg, id, flags inbizier).
            let name = sender.name.read().clone();
            let level = *sender.level.read() as u8;
            if recipient.inbizier_web || recipient.inbizier_mobile {
                let pmsg = sender.personal_message.lock().clone();
                let avatar = crate::handler::avatar_b64_of(sender);
                Some(build_joininfo(
                    &name,
                    &pmsg,
                    &avatar,
                    sender.id,
                    level,
                    sender.inbizier_web,
                    sender.inbizier_mobile,
                ))
            } else {
                Some(build_userlist_item(&name, level))
            }
        }
        _ => None,
    }
}
