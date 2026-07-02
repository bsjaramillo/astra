//! LinkServer: acepta conexiones de otros Astra servers (modo hub).
//!
//! Cuando un leaf se conecta:
//! 1. Lee el `LeafLogin` (nombre + SHA1 + LINK_PROTO + port)
//! 2. Responde con `HubAck` (status = 1)
//! 3. Envía la userlist local como `UserlistItem` (uno por user)
//! 4. Cierra con `LeafUserlistEnd`
//! 5. Loop de keep-alive con `HubPong` cada 30s

use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use parking_lot::Mutex;
use proto_ares::{PacketWriter, TcpMsg};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::interval;
use tracing::{error, info, warn};

use server_core::{AppContext, LinkEvent, LinkUserSnapshot};

use crate::protocol::{
    read_link_from_stream, write_link_to_stream, LinkMsg, LinkPacketBuilder, LinkPacketReader,
    LinkUser,
};

const UNSPECIFIED_IPV4: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const LINK_PACKET_HEADER_LEN: usize = 3;

/// Estado del LinkServer.
pub struct LinkServer {
    /// Contexto de la app
    app: Arc<AppContext>,
    /// Indica si el server está activo
    active: Arc<Mutex<bool>>,
}

impl LinkServer {
    /// Crea un nuevo LinkServer.
    pub fn new(app: Arc<AppContext>) -> Self {
        Self {
            app,
            active: Arc::new(Mutex::new(false)),
        }
    }

    /// Inicia el listener TCP. Cada conexión se maneja en su propia task.
    pub async fn start(self: Arc<Self>, port: u16) -> Result<(), String> {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", port)
            .parse()
            .map_err(|e| format!("dirección inválida: {}", e))?;
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|e| format!("error bindeando: {}", e))?;
        info!("link server: escuchando en {}", bind_addr);
        *self.active.lock() = true;

        while *self.active.lock() {
            let (stream, peer) = match listener.accept().await {
                Ok(r) => r,
                Err(e) => {
                    error!("link server: error aceptando: {}", e);
                    continue;
                }
            };
            info!("link server: conexión entrante de {}", peer);
            let app = self.app.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_leaf_connection(app, stream).await {
                    warn!("link server: error con {}: {}", peer, e);
                }
            });
        }
        Ok(())
    }

    /// Cierra el listener.
    pub fn stop(&self) {
        *self.active.lock() = false;
    }
}

/// Maneja una conexión TCP entrante del protocolo Link sobre el listener compartido.
pub async fn handle_stream(app: Arc<AppContext>, stream: TcpStream) -> Result<(), String> {
    handle_leaf_connection(app, stream).await
}

async fn handle_leaf_connection(app: Arc<AppContext>, mut stream: TcpStream) -> Result<(), String> {
    // 1. Leer LeafLogin
    let (op, payload) = read_link_from_stream(&mut stream)
        .await
        .map_err(|e| format!("error leyendo login: {}", e))?;
    if op != LinkMsg::LeafLogin {
        return Err(format!("esperado LeafLogin, recibí {:?}", op));
    }

    let mut r = crate::protocol::LinkPacketReader::from_payload(&payload);

    let leaf_name = r
        .read_string()
        .map_err(|e| format!("error leyendo leaf name: {}", e))?;
    let _leaf_hash = r
        .read_guid()
        .map_err(|e| format!("error leyendo leaf hash: {}", e))?;
    let _link_proto = r
        .read_u16()
        .map_err(|e| format!("error leyendo link_proto: {}", e))?;
    let leaf_port = r
        .read_u16()
        .map_err(|e| format!("error leyendo port: {}", e))?;

    info!(
        "link server: leaf '{}' (port {}) conectado",
        leaf_name, leaf_port
    );

    // 2. Enviar HubAck (status = 1)
    let mut b = LinkPacketBuilder::new();
    b.write_u8(1); // success
    let ack_packet = b.build_link_packet(LinkMsg::HubAck);
    let ack_payload = ack_packet[3..].to_vec();
    write_link_to_stream(&mut stream, LinkMsg::HubAck, &ack_payload)
        .await
        .map_err(|e| format!("error enviando ACK: {}", e))?;

    // 3. Enviar userlist del hub
    let users: Vec<_> = app
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in)
        .collect();
    for user in &users {
        let mut b = LinkPacketBuilder::new();
        b.write_string(&user.name.read()); // org_name
        b.write_string(&user.name.read()); // name
        b.write_string(&user.version);
        b.write_guid(&user.guid);
        b.write_u16(user.file_count);
        b.write_ip(user.external_ip);
        b.write_ip(user.local_ip);
        b.write_u16(user.data_port);
        b.write_string(""); // dns
        b.write_u8(u8::from(user.browsable));
        b.write_u8(user.age);
        b.write_u8(user.sex);
        b.write_u8(user.country);
        b.write_string(&user.region);
        b.write_u8(*user.level.read() as u8);
        b.write_u16(*user.vroom.read());
        b.write_u8(1); // custom_client (simplificado)
        b.write_u8(u8::from(user.muzzled));
        b.write_u8(u8::from(user.web_client));
        b.write_u8(0); // encrypted
        b.write_u8(u8::from(user.registered));
        b.write_u8(u8::from(user.idle));
        let userlist_item = b.build_link_packet(LinkMsg::UserlistItem);
        let userlist_payload = userlist_item[3..].to_vec();
        write_link_to_stream(&mut stream, LinkMsg::UserlistItem, &userlist_payload)
            .await
            .map_err(|e| format!("error enviando userlist item: {}", e))?;
    }

    // 4. Enviar LeafUserlistEnd
    let end = LinkPacketBuilder::new().build_link_packet(LinkMsg::LeafUserlistEnd);
    let end_payload = end[3..].to_vec();
    write_link_to_stream(&mut stream, LinkMsg::LeafUserlistEnd, &end_payload)
        .await
        .map_err(|e| format!("error enviando userlist end: {}", e))?;

    info!(
        "link server: userlist enviada a leaf '{}' ({} users)",
        leaf_name,
        users.len()
    );

    // 5. Loop de keep-alive: enviar HubPong cada 30s
    let mut ping_timer = interval(Duration::from_secs(30));
    ping_timer.tick().await; // skip primer tick inmediato
    let mut link_events = app.subscribe_link_events();

    loop {
        tokio::select! {
            _ = ping_timer.tick() => {
                // Enviar HubPong como keep-alive
                let pong = LinkPacketBuilder::new().build_link_packet(LinkMsg::HubPong);
                let pong_payload = pong[3..].to_vec();
                if write_link_to_stream(&mut stream, LinkMsg::HubPong, &pong_payload)
                    .await
                    .is_err()
                {
                    info!("link server: leaf '{}' desconectado", leaf_name);
                    return Ok(());
                }
            }
            event = link_events.recv() => {
                match event {
                    Ok(event) => {
                        if should_forward_event_to_leaf(&event, &leaf_name) {
                            if send_link_event(&mut stream, &event).await.is_err() {
                                info!("link server: leaf '{}' desconectado", leaf_name);
                                return Ok(());
                            }
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        warn!("link server: leaf '{}' perdió {} eventos Link", leaf_name, skipped);
                    }
                    Err(RecvError::Closed) => return Ok(()),
                }
            }
            read_result = read_link_from_stream(&mut stream) => {
                let (op, payload) = match read_result {
                    Ok(r) => r,
                    Err(_) => {
                        info!("link server: leaf '{}' desconectado", leaf_name);
                        return Ok(());
                    }
                };
                match op {
                    LinkMsg::LeafPing => {}
                    LinkMsg::NickChanged => {
                        if let Some((old_name, user)) = parse_link_nick_changed_payload(&payload) {
                            let part_pkt = build_server_part_for_name(&old_name);
                            let join_pkt = build_server_join_from_link_user(&user);
                            broadcast_to_local_users(&app, part_pkt);
                            broadcast_to_local_users(&app, join_pkt);
                            app.publish_link_event(LinkEvent::NickChanged {
                                origin: Some(leaf_name.clone()),
                                old_name,
                                user: snapshot_from_link_user(&user),
                            });
                            info!("link server: NickChanged recibido: {}", user.name);
                        } else {
                            warn!("link server: NickChanged malformado");
                        }
                    }
                    LinkMsg::VroomChanged => {
                        if let Some(user) = parse_link_user_item(&payload) {
                            let part_pkt = build_server_part_for_name(&user.name);
                            let join_pkt = build_server_join_from_link_user(&user);
                            broadcast_to_local_users(&app, part_pkt);
                            broadcast_to_local_users(&app, join_pkt);
                            app.publish_link_event(LinkEvent::VroomChanged {
                                origin: Some(leaf_name.clone()),
                                user: snapshot_from_link_user(&user),
                            });
                            info!("link server: VroomChanged recibido: {}", user.name);
                        } else {
                            warn!("link server: VroomChanged malformado");
                        }
                    }
                    LinkMsg::Part => {
                        if let Some(name) = parse_link_part_name(&payload) {
                            let part_pkt = build_server_part_for_name(&name);
                            broadcast_to_local_users(&app, part_pkt);
                            app.publish_link_event(LinkEvent::Part {
                                origin: Some(leaf_name.clone()),
                                name: name.clone(),
                            });
                            info!("link server: part recibido desde leaf: {}", name);
                        } else {
                            warn!("link server: Part malformado");
                        }
                    }
                    LinkMsg::LeafJoin => {
                        if let Some(user) = parse_link_user_item(&payload) {
                            let join_pkt = build_server_join_from_link_user(&user);
                            broadcast_to_local_users(&app, join_pkt);
                            app.publish_link_event(LinkEvent::Join {
                                origin: Some(leaf_name.clone()),
                                user: snapshot_from_link_user(&user),
                            });
                            info!("link server: LeafJoin recibido: {}", user.name);
                        } else {
                            warn!("link server: LeafJoin malformado");
                        }
                    }
                    LinkMsg::UserUpdated => {
                        if let Some(user) = parse_link_user_item(&payload) {
                            let join_pkt = build_server_join_from_link_user(&user);
                            broadcast_to_local_users(&app, join_pkt);
                            app.publish_link_event(LinkEvent::UserUpdated {
                                origin: Some(leaf_name.clone()),
                                user: snapshot_from_link_user(&user),
                            });
                            info!("link server: UserUpdated recibido: {}", user.name);
                        } else {
                            warn!("link server: UserUpdated malformado");
                        }
                    }
                    LinkMsg::PublicText => {
                        if let Some((from, text)) = parse_link_chat_payload(&payload) {
                            let pkt = server_core::outbound::build_public(&from, &text);
                            broadcast_to_local_users(&app, pkt);
                            app.publish_link_event(LinkEvent::Public {
                                origin: Some(leaf_name.clone()),
                                from,
                                text,
                            });
                        } else {
                            warn!("link server: PublicText malformado");
                        }
                    }
                    LinkMsg::EmoteText => {
                        if let Some((from, text)) = parse_link_chat_payload(&payload) {
                            let pkt = server_core::outbound::build_emote(&from, &text);
                            broadcast_to_local_users(&app, pkt);
                            app.publish_link_event(LinkEvent::Emote {
                                origin: Some(leaf_name.clone()),
                                from,
                                text,
                            });
                        } else {
                            warn!("link server: EmoteText malformado");
                        }
                    }
                    LinkMsg::PrivateText => {
                        if let Some((from, to, text)) = parse_link_private_payload(&payload) {
                            if let Some(target) = app.user_pool.get_by_name(&to) {
                                if target
                                    .ignore_list
                                    .read()
                                    .iter()
                                    .any(|entry| entry.eq_ignore_ascii_case(&from))
                                {
                                    app.publish_link_event(LinkEvent::PrivateIgnored {
                                        origin: Some(leaf_name.clone()),
                                        from,
                                        to,
                                    });
                                } else {
                                    let _ = target.send(server_core::outbound::build_pvt(&from, &text));
                                    app.publish_link_event(LinkEvent::Private {
                                        origin: Some(leaf_name.clone()),
                                        from,
                                        to,
                                        text,
                                    });
                                }
                            } else {
                                app.publish_link_event(LinkEvent::Private {
                                    origin: Some(leaf_name.clone()),
                                    from,
                                    to,
                                    text,
                                });
                            }
                        } else {
                            warn!("link server: PrivateText malformado");
                        }
                    }
                    LinkMsg::PrivateIgnored => {
                        if let Some((from, to)) = parse_link_private_ignored_payload(&payload) {
                            if let Some(local_from) = app.user_pool.get_by_name(&from) {
                                let mut w = PacketWriter::with_msg(TcpMsg::ServerIsIgnoringYou);
                                w.write_string(&to).ok();
                                let _ = local_from.send(Bytes::copy_from_slice(w.as_bytes()));
                            }
                            app.publish_link_event(LinkEvent::PrivateIgnored {
                                origin: Some(leaf_name.clone()),
                                from,
                                to,
                            });
                        } else {
                            warn!("link server: PrivateIgnored malformado");
                        }
                    }
                    LinkMsg::PublicToUser => {
                        if let Some((from, to, text)) = parse_link_private_payload(&payload) {
                            if let Some(target) = app.user_pool.get_by_name(&to) {
                                let _ = target.send(server_core::outbound::build_public(&from, &text));
                            }
                            app.publish_link_event(LinkEvent::PublicToUser {
                                origin: Some(leaf_name.clone()),
                                from,
                                to,
                                text,
                            });
                        } else {
                            warn!("link server: PublicToUser malformado");
                        }
                    }
                    LinkMsg::EmoteToUser => {
                        if let Some((from, to, text)) = parse_link_private_payload(&payload) {
                            if let Some(target) = app.user_pool.get_by_name(&to) {
                                let _ = target.send(server_core::outbound::build_emote(&from, &text));
                            }
                            app.publish_link_event(LinkEvent::EmoteToUser {
                                origin: Some(leaf_name.clone()),
                                from,
                                to,
                                text,
                            });
                        } else {
                            warn!("link server: EmoteToUser malformado");
                        }
                    }
                    LinkMsg::PersonalMessage => {
                        if let Some((name, text)) = parse_link_chat_payload(&payload) {
                            let mut w = PacketWriter::with_msg(TcpMsg::PersonalMessage);
                            w.write_string(&name).ok();
                            w.write_string(&text).ok();
                            broadcast_to_local_users(&app, Bytes::copy_from_slice(w.as_bytes()));
                            app.publish_link_event(LinkEvent::PersonalMessage {
                                origin: Some(leaf_name.clone()),
                                name,
                                text,
                            });
                        } else {
                            warn!("link server: PersonalMessage malformado");
                        }
                    }
                    LinkMsg::CustomName => {
                        if let Some((name, custom_name)) = parse_link_custom_name_payload(&payload) {
                            app.publish_link_event(LinkEvent::CustomName {
                                origin: Some(leaf_name.clone()),
                                name,
                                custom_name,
                            });
                        } else {
                            warn!("link server: CustomName malformado");
                        }
                    }
                    op if is_passthrough_opcode(op) => {
                        info!("link server: passthrough recibido: {:?}", op);
                        app.publish_link_event(LinkEvent::Raw {
                            origin: Some(leaf_name.clone()),
                            msg: op as u8,
                            payload,
                        });
                    }
                    _ => {
                        warn!("link server: opcode no manejado: {:?}", op);
                    }
                }
            }
        }
    }
}

fn parse_link_user_item(payload: &[u8]) -> Option<LinkUser> {
    let mut r = LinkPacketReader::from_payload(payload);
    let org_name = r.read_string().ok()?;
    let name = r.read_string().ok()?;
    let version = r.read_string().ok()?;
    let guid = r.read_guid().ok()?;
    let file_count = r.read_u16().ok()?;
    let external_ip = r.read_ip().ok()?;
    let local_ip = r.read_ip().ok()?;
    let port = r.read_u16().ok()?;
    let dns = r.read_string().ok()?;
    let browsable = r.read_u8().ok()? != 0;
    let age = r.read_u8().ok()?;
    let sex = r.read_u8().ok()?;
    let country = r.read_u8().ok()?;
    let region = r.read_string().ok()?;
    let level = r.read_u8().ok()?;
    let vroom = r.read_u16().ok()?;
    let custom_client = r.read_u8().ok()? != 0;
    let muzzled = r.read_u8().ok()? != 0;
    let web_client = r.read_u8().ok()? != 0;
    let encrypted = r.read_u8().ok()? != 0;
    let registered = r.read_u8().ok()? != 0;
    let idle = r.read_u8().ok()? != 0;

    Some(LinkUser {
        org_name,
        name,
        version,
        guid,
        file_count,
        external_ip,
        local_ip,
        port,
        dns,
        browsable,
        age,
        sex,
        country,
        region,
        level,
        vroom,
        custom_client,
        muzzled,
        web_client,
        encrypted,
        registered,
        idle,
        custom_name: None,
        personal_message: None,
    })
}

fn parse_link_part_name(payload: &[u8]) -> Option<String> {
    let mut r = LinkPacketReader::from_payload(payload);
    r.read_string().ok()
}

fn parse_link_chat_payload(payload: &[u8]) -> Option<(String, String)> {
    let mut r = LinkPacketReader::from_payload(payload);
    let from = r.read_string().ok()?;
    let text = r.read_string().ok()?;
    Some((from, text))
}

fn parse_link_private_payload(payload: &[u8]) -> Option<(String, String, String)> {
    let mut r = LinkPacketReader::from_payload(payload);
    let from = r.read_string().ok()?;
    let to = r.read_string().ok()?;
    let text = r.read_string().ok()?;
    Some((from, to, text))
}

fn parse_link_private_ignored_payload(payload: &[u8]) -> Option<(String, String)> {
    let mut r = LinkPacketReader::from_payload(payload);
    let from = r.read_string().ok()?;
    let to = r.read_string().ok()?;
    Some((from, to))
}

fn parse_link_nick_changed_payload(payload: &[u8]) -> Option<(String, LinkUser)> {
    let mut r = LinkPacketReader::from_payload(payload);
    let old_name = r.read_string().ok()?;
    let remaining = r.read_bytes(r.remaining()).ok()?;
    let user = parse_link_user_item(&remaining)?;
    Some((old_name, user))
}

fn parse_link_custom_name_payload(payload: &[u8]) -> Option<(String, Option<String>)> {
    let mut r = LinkPacketReader::from_payload(payload);
    let name = r.read_string().ok()?;
    let custom_name = r.read_string().ok()?;
    let custom_name = if custom_name.is_empty() { None } else { Some(custom_name) };
    Some((name, custom_name))
}

fn snapshot_from_link_user(user: &LinkUser) -> LinkUserSnapshot {
    LinkUserSnapshot {
        org_name: user.org_name.clone(),
        name: user.name.clone(),
        version: user.version.clone(),
        guid: user.guid,
        file_count: user.file_count,
        external_ip: user.external_ip,
        local_ip: user.local_ip,
        port: user.port,
        dns: user.dns.clone(),
        browsable: user.browsable,
        age: user.age,
        sex: user.sex,
        country: user.country,
        region: user.region.clone(),
        level: user.level,
        vroom: user.vroom,
        custom_client: user.custom_client,
        muzzled: user.muzzled,
        web_client: user.web_client,
        encrypted: user.encrypted,
        registered: user.registered,
        idle: user.idle,
    }
}

fn should_forward_event_to_leaf(event: &LinkEvent, leaf_name: &str) -> bool {
    match event {
        LinkEvent::Join { origin, .. }
        | LinkEvent::UserUpdated { origin, .. }
        | LinkEvent::NickChanged { origin, .. }
        | LinkEvent::VroomChanged { origin, .. }
        | LinkEvent::CustomName { origin, .. }
        | LinkEvent::Part { origin, .. }
        | LinkEvent::Public { origin, .. }
        | LinkEvent::Emote { origin, .. }
        | LinkEvent::Private { origin, .. }
        | LinkEvent::PublicToUser { origin, .. }
        | LinkEvent::EmoteToUser { origin, .. }
        | LinkEvent::PrivateIgnored { origin, .. }
        | LinkEvent::PersonalMessage { origin, .. }
        | LinkEvent::Raw { origin, .. } => origin.as_deref() != Some(leaf_name),
    }
}

async fn send_link_event(stream: &mut TcpStream, event: &LinkEvent) -> Result<(), String> {
    let (msg, payload) = match event {
        LinkEvent::Join { user, .. } => (LinkMsg::LeafJoin, build_leaf_join_payload_from_snapshot(user, LinkMsg::LeafJoin)),
        LinkEvent::UserUpdated { user, .. } => (LinkMsg::UserUpdated, build_leaf_join_payload_from_snapshot(user, LinkMsg::UserUpdated)),
        LinkEvent::NickChanged { old_name, user, .. } => (LinkMsg::NickChanged, build_nick_changed_payload(old_name, user)),
        LinkEvent::VroomChanged { user, .. } => (LinkMsg::VroomChanged, build_leaf_join_payload_from_snapshot(user, LinkMsg::VroomChanged)),
        LinkEvent::CustomName { name, custom_name, .. } => (LinkMsg::CustomName, build_custom_name_payload(name, custom_name.as_deref())),
        LinkEvent::Part { name, .. } => {
            let mut b = LinkPacketBuilder::new();
            b.write_string(name);
            (LinkMsg::Part, b.build_link_packet(LinkMsg::Part)[LINK_PACKET_HEADER_LEN..].to_vec())
        }
        LinkEvent::Public { from, text, .. } => (LinkMsg::PublicText, build_chat_payload(from, text, LinkMsg::PublicText)),
        LinkEvent::Emote { from, text, .. } => (LinkMsg::EmoteText, build_chat_payload(from, text, LinkMsg::EmoteText)),
        LinkEvent::Private { from, to, text, .. } => (LinkMsg::PrivateText, build_private_payload(from, to, text)),
        LinkEvent::PublicToUser { from, to, text, .. } => (LinkMsg::PublicToUser, build_private_payload(from, to, text)),
        LinkEvent::EmoteToUser { from, to, text, .. } => (LinkMsg::EmoteToUser, build_private_payload(from, to, text)),
        LinkEvent::PrivateIgnored { from, to, .. } => (LinkMsg::PrivateIgnored, build_private_ignored_payload(from, to)),
        LinkEvent::PersonalMessage { name, text, .. } => (LinkMsg::PersonalMessage, build_chat_payload(name, text, LinkMsg::PersonalMessage)),
        LinkEvent::Raw { msg, payload, .. } => {
            let Some(link_msg) = LinkMsg::from_u8(*msg) else {
                return Ok(());
            };
            info!("link server: reenviando passthrough {:?}", link_msg);
            (link_msg, payload.clone())
        }
    };
    write_link_to_stream(stream, msg, &payload).await.map_err(|e| e.to_string())
}

fn build_leaf_join_payload_from_snapshot(user: &LinkUserSnapshot, msg: LinkMsg) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(&user.org_name);
    b.write_string(&user.name);
    b.write_string(&user.version);
    b.write_guid(&user.guid);
    b.write_u16(user.file_count);
    b.write_ip(user.external_ip);
    b.write_ip(user.local_ip);
    b.write_u16(user.port);
    b.write_string(&user.dns);
    b.write_u8(u8::from(user.browsable));
    b.write_u8(user.age);
    b.write_u8(user.sex);
    b.write_u8(user.country);
    b.write_string(&user.region);
    b.write_u8(user.level);
    b.write_u16(user.vroom);
    b.write_u8(u8::from(user.custom_client));
    b.write_u8(u8::from(user.muzzled));
    b.write_u8(u8::from(user.web_client));
    b.write_u8(u8::from(user.encrypted));
    b.write_u8(u8::from(user.registered));
    b.write_u8(u8::from(user.idle));
    b.build_link_packet(msg)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn build_nick_changed_payload(old_name: &str, user: &LinkUserSnapshot) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(old_name);
    b.write_bytes(&build_leaf_join_payload_from_snapshot(user, LinkMsg::LeafJoin));
    b.build_link_packet(LinkMsg::NickChanged)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn build_chat_payload(from: &str, text: &str, msg: LinkMsg) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(from);
    b.write_string(text);
    b.build_link_packet(msg)[LINK_PACKET_HEADER_LEN..].to_vec()
}
fn build_custom_name_payload(name: &str, custom_name: Option<&str>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(name);
    b.write_string(custom_name.unwrap_or(""));
    b.build_link_packet(LinkMsg::CustomName)[3..].to_vec()
}

fn build_chat_payload(from: &str, text: &str, msg: LinkMsg) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(from);
    b.write_string(text);
    b.build_link_packet(msg)[3..].to_vec()
}

fn build_private_payload(from: &str, to: &str, text: &str) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(from);
    b.write_string(to);
    b.write_string(text);
    b.build_link_packet(LinkMsg::PrivateText)[3..].to_vec()
}

fn build_private_ignored_payload(from: &str, to: &str) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new();
    b.write_string(from);
    b.write_string(to);
    b.build_link_packet(LinkMsg::PrivateIgnored)[3..].to_vec()
}

fn build_server_join_from_link_user(user: &LinkUser) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerJoin);
    w.write_u16_le(user.file_count).ok();
    w.write_u32_le(0).ok(); // reservado
    write_ip(&mut w, user.external_ip);
    w.write_u16_le(user.port).ok();
    w.write_ipv4(UNSPECIFIED_IPV4).ok(); // node_ip desconocida para usuarios remotos
    w.write_u16_le(0).ok(); // node_port desconocido
    w.write_u8(0).ok(); // reservado
    w.write_string(&user.name).ok();
    write_ip(&mut w, user.local_ip);
    w.write_u8(user.browsable as u8).ok();
    w.write_u8(user.level).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string(&user.region).ok();
    w.write_u8(0).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

fn build_server_part_for_name(name: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerPart);
    w.write_string(name).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

fn write_ip(w: &mut PacketWriter, ip: IpAddr) {
    match ip {
        IpAddr::V4(v4) => {
            w.write_ipv4(v4).ok();
        }
        IpAddr::V6(_) => {
            w.write_ipv4(UNSPECIFIED_IPV4).ok();
        }
    }
}

fn broadcast_to_local_users(app: &AppContext, pkt: Bytes) {
    for user in app.user_pool.users() {
        if user.logged_in && !user.quarantined {
            let _ = user.send(pkt.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::LinkPacketBuilder;

    #[test]
    fn parse_leaf_join_payload_roundtrip() {
        let mut b = LinkPacketBuilder::new();
        b.write_string("Alice");
        b.write_string("Alice");
        b.write_string("Ares 2.5");
        b.write_guid(&[0xAB; 16]);
        b.write_u16(42);
        b.write_ip("1.2.3.4".parse().unwrap());
        b.write_ip("10.0.0.2".parse().unwrap());
        b.write_u16(5009);
        b.write_string("");
        b.write_u8(1);
        b.write_u8(30);
        b.write_u8(1);
        b.write_u8(49);
        b.write_string("US");
        b.write_u8(1);
        b.write_u16(0);
        b.write_u8(1);
        b.write_u8(0);
        b.write_u8(0);
        b.write_u8(0);
        b.write_u8(1);
        b.write_u8(0);
        let packet = b.build_link_packet(LinkMsg::LeafJoin);

        let parsed = parse_link_user_item(&packet[3..]).expect("payload válido");
        assert_eq!(parsed.name, "Alice");
        assert_eq!(parsed.file_count, 42);
        assert_eq!(parsed.port, 5009);
    }

    #[test]
    fn parse_part_payload_roundtrip() {
        let mut b = LinkPacketBuilder::new();
        b.write_string("Bob");
        let packet = b.build_link_packet(LinkMsg::Part);
        let name = parse_link_part_name(&packet[3..]).expect("part válido");
        assert_eq!(name, "Bob");
    }
}
