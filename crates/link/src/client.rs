//! LinkClient: se conecta a otro Astra server (modo leaf).
//!
//! Handshake:
//! 1. Envía `LeafLogin` (nombre + SHA1(name+guid_reverse) + LINK_PROTO + port)
//! 2. Espera `HubAck` con status byte
//! 3. Lee la userlist del hub
//! 4. Envía un ping cada 30s
//!
//! Para esta implementación mínima, no reenvía broadcasts — solo mantiene
//! la lista de usuarios del otro server en una estructura interna.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::interval;
use tracing::{info, warn};

use server_core::{AppContext, LinkEvent, LinkUserSnapshot};

use crate::crypto::{self, LinkCrypto};
use crate::protocol::{
    read_link_from_stream, write_link_to_stream, LinkMsg, LinkPacketBuilder, LinkUser,
    MSG_LINK_PROTO,
};

const LINK_PACKET_HEADER_LEN: usize = 3;
/// Versión de protocolo Link (sb0t `Settings.LINK_PROTO`).
const LINK_PROTO: u16 = 500;

/// Estado de un LinkClient.
pub struct LinkClient {
    /// Contexto de la app
    app: Arc<AppContext>,
    /// Indica si la conexión está activa
    active: Arc<Mutex<bool>>,
    /// El nombre del otro server (hub)
    peer_name: Mutex<Option<String>>,
    /// Usuarios del hub (leídos del handshake)
    peer_users: Mutex<Vec<LinkUser>>,
}

impl LinkClient {
    /// Crea un nuevo LinkClient.
    pub fn new(app: Arc<AppContext>) -> Self {
        Self {
            app,
            active: Arc::new(Mutex::new(true)),
            peer_name: Mutex::new(None),
            peer_users: Mutex::new(Vec::new()),
        }
    }

    /// ¿Está conectado?
    pub fn is_active(&self) -> bool {
        *self.active.lock()
    }

    /// Nombre del peer (hub).
    pub fn peer_name(&self) -> Option<String> {
        self.peer_name.lock().clone()
    }

    /// Usuarios del peer (leídos del handshake).
    pub fn peer_users(&self) -> Vec<LinkUser> {
        self.peer_users.lock().clone()
    }

    /// Inicia el loop de conexión: conecta, hace handshake, mantiene
    /// la conexión. Se debe llamar desde un `tokio::spawn`.
    pub async fn run(self: Arc<Self>, addr: SocketAddr) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if !*self.active.lock() {
                break;
            }
            match self.connect_and_run(addr).await {
                Ok(_) => {
                    info!("link client: desconectado limpiamente de {}", addr);
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    warn!("link client: error: {} — reintentando en {:?}", e, backoff);
                }
            }
            if !*self.active.lock() {
                break;
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }

    /// Conecta, hace handshake, loop principal. Retorna cuando la
    /// conexión se cierra limpiamente.
    async fn connect_and_run(&self, addr: SocketAddr) -> Result<(), String> {
        // Limpiar estado de la conexión anterior (en reconnect los users
        // del hub se vuelven a recibir completos en el handshake).
        self.peer_users.lock().clear();
        *self.peer_name.lock() = None;
        self.app.link_leaves.write().clear();

        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("error conectando: {}", e))?;

        // Credentials = SHA1(reverse(name ++ guid)) — 20 bytes, paridad
        // exacta con sb0t LeafOutbound.LeafLogin.
        let name = self.app.settings.room_name.clone();
        let my_guid = crypto::guid_bytes_from_string(&self.app.settings.guid);
        let credentials = crypto::credentials(&name, &my_guid);

        // Enviar LeafLogin: credentials(20) + LINK_PROTO(u16) + port(u16).
        // El login nunca va encriptado.
        let mut b = LinkPacketBuilder::new();
        b.write_bytes(&credentials);
        b.write_u16(LINK_PROTO);
        b.write_u16(self.app.settings.port);
        let login_payload = {
            let packet = b.build_link_packet(LinkMsg::LeafLogin);
            packet[LINK_PACKET_HEADER_LEN..].to_vec()
        };

        let mut stream = stream;
        write_link_to_stream(&mut stream, LinkMsg::LeafLogin, &login_payload)
            .await
            .map_err(|e| format!("error enviando login: {}", e))?;

        // Leer HubAck. sb0t/cifrado: 48 bytes de key+IV ofuscados. Legacy
        // Astra: 1 byte de status. Distinguimos por longitud del payload.
        let (op, payload) = read_link_from_stream(&mut stream)
            .await
            .map_err(|e| format!("error leyendo ACK: {}", e))?;
        if op != LinkMsg::HubAck {
            return Err(format!("esperado HubAck, recibí {:?}", op));
        }
        let crypto: Option<LinkCrypto> = if payload.len() >= 48 {
            let mut obf = [0u8; 48];
            obf.copy_from_slice(&payload[..48]);
            let c = LinkCrypto::from_obfuscated(&obf, &my_guid);
            info!("link client: ACK cifrado recibido, sesión AES-256 establecida");
            Some(c)
        } else {
            let status = payload.first().copied().unwrap_or(0);
            if status != 1 {
                return Err(format!("login rechazado: status={}", status));
            }
            info!("link client: ACK legacy recibido (sin cifrado)");
            None
        };

        // Leer userlist
        loop {
            let (op, payload) = read_link_from_stream(&mut stream)
                .await
                .map_err(|e| format!("error leyendo userlist: {}", e))?;
            match op {
                LinkMsg::UserlistItem => {
                    if let Some(user) = parse_userlist_item(&payload, crypto) {
                        info!("link client: user del hub: {}", user.name);
                        self.peer_users.lock().push(user);
                    }
                }
                LinkMsg::LeafUserlistEnd => {
                    info!(
                        "link client: userlist completa ({} users)",
                        self.peer_users.lock().len()
                    );
                    break;
                }
                _ => {
                    warn!("link client: opcode inesperado en userlist: {:?}", op);
                }
            }
        }

        // Loop de keep-alive: enviar ping cada 30s, esperar pong
        let mut ping_timer = interval(Duration::from_secs(30));
        ping_timer.tick().await; // primer tick inmediato
        sync_local_users_to_hub(&self.app, &mut stream, crypto).await?;
        let mut link_events = self.app.subscribe_link_events();

        loop {
            tokio::select! {
                _ = ping_timer.tick() => {
                    let ping = LinkPacketBuilder::new().build_link_packet(LinkMsg::LeafPing);
                    let ping_payload = ping[LINK_PACKET_HEADER_LEN..].to_vec();
                    if write_link_to_stream(&mut stream, LinkMsg::LeafPing, &ping_payload).await.is_err() {
                        info!("link client: conexión cerrada");
                        return Ok(());
                    }
                }
                read_result = read_link_from_stream(&mut stream) => {
                    let (op, payload) = match read_result {
                        Ok(r) => r,
                        Err(_) => {
                            info!("link client: conexión cerrada");
                            return Ok(());
                        }
                    };
                    if op == LinkMsg::HubPong {
                        // OK, pong recibido
                    } else if handle_incoming_link_message(&self.app, self, op, &payload, crypto) {
                        // mensaje aplicado localmente
                    } else {
                        warn!("link client: opcode no manejado: {:?}", op);
                    }
                }
                event = link_events.recv() => {
                    match event {
                        Ok(event) => {
                            if let Err(e) = send_link_event(&mut stream, &event, crypto).await {
                                return Err(format!("error reenviando evento Link al hub: {}", e));
                            }
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            warn!("link client: perdió {} eventos Link", skipped);
                        }
                        Err(RecvError::Closed) => return Ok(()),
                    }
                }
            }
        }
    }

    /// Cierra la conexión.
    pub fn close(&self) {
        *self.active.lock() = false;
    }
}

fn build_leaf_join_payload(user: &server_core::user_pool::AresUser, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
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
    // El protocolo Link es de sb0t: el nivel viaja en la escala Ares (0..3).
    b.write_u8(server_core::outbound::ares_level(*user.level.read() as u8));
    b.write_u16(*user.vroom.read());
    b.write_u8(u8::from(user.custom_client));
    b.write_u8(u8::from(
        user.muzzled.load(std::sync::atomic::Ordering::Relaxed),
    ));
    b.write_u8(u8::from(user.web_client));
    b.write_u8(0); // encrypted
    b.write_u8(u8::from(user.registered));
    b.write_u8(u8::from(user.idle));
    let packet = b.build_link_packet(LinkMsg::LeafJoin);
    packet[LINK_PACKET_HEADER_LEN..].to_vec()
}

async fn sync_local_users_to_hub(
    app: &AppContext,
    stream: &mut TcpStream,
    crypto: Option<LinkCrypto>,
) -> Result<(), String> {
    let current_users: Vec<std::sync::Arc<server_core::user_pool::AresUser>> = app
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in)
        .collect();

    for user in &current_users {
        let payload = build_leaf_join_payload(user, crypto);
        write_link_to_stream(&mut *stream, LinkMsg::LeafJoin, &payload)
            .await
            .map_err(|e| format!("error enviando LeafJoin inicial: {}", e))?;
    }

    Ok(())
}

fn parse_userlist_item(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<LinkUser> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    let org_name = r.read_string().ok()?;
    let name = r.read_string().ok()?;
    let version = r.read_string().ok()?;
    let guid_bytes: [u8; 16] = r.read_guid().ok()?;
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
    let level = server_core::outbound::ares_level_from_wire(r.read_u8().ok()?);
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
        guid: guid_bytes,
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

fn handle_incoming_link_message(
    app: &AppContext,
    client: &LinkClient,
    op: LinkMsg,
    payload: &[u8],
    crypto: Option<LinkCrypto>,
) -> bool {
    match op {
        LinkMsg::LeafJoin => {
            if let Some(user) = parse_userlist_item(payload, crypto) {
                client.peer_users.lock().push(user.clone());
                broadcast_to_local_users(app, |c| build_server_join_from_link_user(&user, c));
                true
            } else {
                false
            }
        }
        LinkMsg::NickChanged => {
            if let Some((old_name, user)) = parse_link_nick_changed_payload(payload, crypto) {
                {
                    let mut peer_users = client.peer_users.lock();
                    peer_users.retain(|item| !item.name.eq_ignore_ascii_case(&old_name));
                    peer_users.push(user.clone());
                }
                broadcast_to_local_users(app, |c| build_server_part_for_name(&old_name, c));
                broadcast_to_local_users(app, |c| build_server_join_from_link_user(&user, c));
                true
            } else {
                false
            }
        }
        LinkMsg::VroomChanged => {
            if let Some(user) = parse_userlist_item(payload, crypto) {
                {
                    let mut peer_users = client.peer_users.lock();
                    peer_users.retain(|item| !item.name.eq_ignore_ascii_case(&user.name));
                    peer_users.push(user.clone());
                }
                broadcast_to_local_users(app, |c| build_server_part_for_name(&user.name, c));
                broadcast_to_local_users(app, |c| build_server_join_from_link_user(&user, c));
                true
            } else {
                false
            }
        }
        LinkMsg::CustomName => {
            if let Some((name, custom_name)) = parse_link_custom_name_payload(payload, crypto) {
                let mut peer_users = client.peer_users.lock();
                if let Some(existing) = peer_users.iter_mut().find(|item| item.name.eq_ignore_ascii_case(&name)) {
                    existing.custom_name = custom_name;
                }
                true
            } else {
                false
            }
        }
        LinkMsg::UserUpdated => {
            if let Some(user) = parse_userlist_item(payload, crypto) {
                {
                    let mut peer_users = client.peer_users.lock();
                    if let Some(existing) = peer_users.iter_mut().find(|item| item.name.eq_ignore_ascii_case(&user.name)) {
                        *existing = user.clone();
                    } else {
                        peer_users.push(user.clone());
                    }
                }
                broadcast_to_local_users(app, |c| build_server_join_from_link_user(&user, c));
                true
            } else {
                false
            }
        }
        LinkMsg::Part => {
            if let Some(name) = parse_link_part_name(payload, crypto) {
                client.peer_users.lock().retain(|user| !user.name.eq_ignore_ascii_case(&name));
                broadcast_to_local_users(app, |c| build_server_part_for_name(&name, c));
                true
            } else {
                false
            }
        }
        LinkMsg::PublicText => {
            if let Some((from, text)) = parse_link_chat_payload(payload, crypto) {
                // Web-aware: send_public elige el formato por cliente (los
                // users web tienen sender binario = None y lo descartarían).
                for u in app.user_pool.users() {
                    if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = u.send_public(&from, &text);
                    }
                }
                true
            } else {
                false
            }
        }
        LinkMsg::EmoteText => {
            if let Some((from, text)) = parse_link_chat_payload(payload, crypto) {
                for u in app.user_pool.users() {
                    if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = u.send_emote(&from, &text);
                    }
                }
                true
            } else {
                false
            }
        }
        // ---- Leaf peers (paridad LinkLeaf.LeafProcessor de sb0t) ----
        LinkMsg::HubLeafConnected => {
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match (r.read_u32(), r.read_string(), r.read_ip(), r.read_u16()) {
                (Ok(ident), Ok(name), Ok(ip), Ok(port)) => {
                    let mut leaves = app.link_leaves.write();
                    leaves.retain(|l| l.ident != ident);
                    leaves.push(server_core::LinkLeafInfo { ident, name, ip, port });
                    true
                }
                _ => false,
            }
        }
        LinkMsg::HubLeafDisconnected => {
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match r.read_u32() {
                Ok(ident) => {
                    app.link_leaves.write().retain(|l| l.ident != ident);
                    true
                }
                _ => false,
            }
        }
        // ---- Dirigidos a ESTE leaf (paridad HubPrint*/HubPublicToLeaf/...) ----
        LinkMsg::PrintAll => {
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match r.read_string_no_null() {
                Ok(text) => {
                    app.broadcast_print(&text);
                    true
                }
                _ => false,
            }
        }
        LinkMsg::PrintVroom => {
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match (r.read_u16(), r.read_string_no_null()) {
                (Ok(vroom), Ok(text)) => {
                    let bot = app.settings.bot_name.clone();
                    for u in app.user_pool.users() {
                        if u.logged_in
                            && *u.vroom.read() == vroom
                            && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            let _ = u.print(&bot, &text);
                        }
                    }
                    true
                }
                _ => false,
            }
        }
        LinkMsg::PrintLevel => {
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match (r.read_u8(), r.read_string_no_null()) {
                (Ok(level), Ok(text)) => {
                    // sb0t HubPrintLevel: usuarios con Level > level (estricto).
                    // El nivel del wire está en la escala Ares (0..3), así que
                    // el local hay que traducirlo antes de comparar — si no,
                    // se comparan escalas distintas y cualquier Moderator de
                    // Astra (50) supera a un Host (3) del peer.
                    let level = server_core::outbound::ares_level_from_wire(level);
                    let bot = app.settings.bot_name.clone();
                    for u in app.user_pool.users() {
                        if u.logged_in
                            && server_core::outbound::ares_level(*u.level.read() as u8) > level
                            && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            let _ = u.print(&bot, &text);
                        }
                    }
                    true
                }
                _ => false,
            }
        }
        LinkMsg::PublicToLeaf | LinkMsg::EmoteToLeaf => {
            // Wire: `str from` (null-terminated) + `str text` (SIN null, es
            // el último campo — mismo formato que arma el hub).
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match (r.read_string(), r.read_string_no_null()) {
                (Ok(from), Ok(text)) => {
                    let emote = op == LinkMsg::EmoteToLeaf;
                    for u in app.user_pool.users() {
                        if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                            let _ = if emote { u.send_emote(&from, &text) } else { u.send_public(&from, &text) };
                        }
                    }
                    true
                }
                _ => false,
            }
        }
        LinkMsg::ScribbleLeaf => {
            let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
            match (r.read_string(), r.read_u32()) {
                (Ok(sender), Ok(_height)) => {
                    let img = r.read_bytes(r.remaining()).unwrap_or_default();
                    // A los custom clients (paridad HubScribbleLeaf), como
                    // CustomData cb0t_scribble_* chunked.
                    for u in app.user_pool.users() {
                        if u.logged_in
                            && u.custom_client
                            && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            send_scribble_custom_data(&u, &sender, &img);
                        }
                    }
                    true
                }
                _ => false,
            }
        }
        LinkMsg::PrivateText => {
            if let Some((from, to, text)) = parse_link_private_payload(payload, crypto) {
                if let Some(target) = app.user_pool.get_by_name(&to) {
                    if target
                        .ignore_list
                        .read()
                        .iter()
                        .any(|entry| entry.eq_ignore_ascii_case(&from))
                    {
                        app.publish_link_event(LinkEvent::PrivateIgnored {
                            origin: None,
                            from,
                            to,
                        });
                    } else {
                        let _ = target.send_pvt(&from, &text);
                    }
                }
                true
            } else {
                false
            }
        }
        LinkMsg::PrivateIgnored => {
            if let Some((from, to)) = parse_link_private_ignored_payload(payload, crypto) {
                if let Some(local_from) = app.user_pool.get_by_name(&from) {
                    let mut w = proto_ares::PacketWriter::with_msg_crypto(
                        proto_ares::TcpMsg::ServerIsIgnoringYou,
                        local_from.ares_crypto,
                    );
                    w.write_string(&to).ok();
                    let _ = local_from.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
                }
                true
            } else {
                false
            }
        }
        LinkMsg::PublicToUser => {
            if let Some((from, to, text)) = parse_link_private_payload(payload, crypto) {
                if let Some(target) = app.user_pool.get_by_name(&to) {
                    let _ = target.send_public(&from, &text);
                }
                true
            } else {
                false
            }
        }
        LinkMsg::EmoteToUser => {
            if let Some((from, to, text)) = parse_link_private_payload(payload, crypto) {
                if let Some(target) = app.user_pool.get_by_name(&to) {
                    let _ = target.send_emote(&from, &text);
                }
                true
            } else {
                false
            }
        }
        LinkMsg::PersonalMessage => {
            if let Some((name, text)) = parse_link_chat_payload(payload, crypto) {
                broadcast_to_local_users(app, |c| {
                    let mut w = proto_ares::PacketWriter::with_msg_crypto(
                        proto_ares::TcpMsg::PersonalMessage,
                        c,
                    );
                    w.write_string(&name).ok();
                    w.write_string(&text).ok();
                    bytes::Bytes::copy_from_slice(w.as_bytes())
                });
                true
            } else {
                false
            }
        }
        LinkMsg::Admin => {
            // Acción host* propagada desde otro servidor de la red: aplicarla
            // al pool local (ban/kick/muzzle/unmuzzle del target si está aquí).
            if let Some((kind, target)) = crate::server::parse_admin_payload(payload, crypto) {
                app.apply_admin_action(kind, &target);
                true
            } else {
                false
            }
        }
        op if is_passthrough_opcode(op) => {
            app.publish_link_event(LinkEvent::Raw {
                origin: Some("hub".to_string()),
                msg: op as u8,
                payload: payload.to_vec(),
            });
            true
        }
        _ => false,
    }
}

async fn send_link_event(stream: &mut TcpStream, event: &LinkEvent, crypto: Option<LinkCrypto>) -> Result<(), String> {
    let (msg, payload) = match event {
        LinkEvent::Join { origin, user } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::LeafJoin, build_leaf_join_payload_from_snapshot(user, LinkMsg::LeafJoin, crypto))
        }
        LinkEvent::UserUpdated { origin, user } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::UserUpdated, build_leaf_join_payload_from_snapshot(user, LinkMsg::UserUpdated, crypto))
        }
        LinkEvent::NickChanged { origin, old_name, user } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::NickChanged, build_nick_changed_payload(old_name, user, crypto))
        }
        LinkEvent::VroomChanged { origin, user } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::VroomChanged, build_leaf_join_payload_from_snapshot(user, LinkMsg::VroomChanged, crypto))
        }
        LinkEvent::CustomName { origin, name, custom_name } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::CustomName, build_custom_name_payload(name, custom_name.as_deref(), crypto))
        }
        LinkEvent::Part { origin, name } => {
            if origin.is_some() {
                return Ok(());
            }
            let mut b = LinkPacketBuilder::new_with_crypto(crypto);
            b.write_string(name);
            (LinkMsg::Part, b.build_link_packet(LinkMsg::Part)[LINK_PACKET_HEADER_LEN..].to_vec())
        }
        LinkEvent::Public { origin, from, text } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::PublicText, build_chat_payload(from, text, LinkMsg::PublicText, crypto))
        }
        LinkEvent::Emote { origin, from, text } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::EmoteText, build_chat_payload(from, text, LinkMsg::EmoteText, crypto))
        }
        LinkEvent::Private { origin, from, to, text } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::PrivateText, build_private_payload(from, to, text, crypto))
        }
        LinkEvent::PublicToUser { origin, from, to, text } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::PublicToUser, build_private_payload(from, to, text, crypto))
        }
        LinkEvent::EmoteToUser { origin, from, to, text } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::EmoteToUser, build_private_payload(from, to, text, crypto))
        }
        LinkEvent::PrivateIgnored { origin, from, to } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::PrivateIgnored, build_private_ignored_payload(from, to, crypto))
        }
        LinkEvent::PersonalMessage { origin, name, text } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::PersonalMessage, build_chat_payload(name, text, LinkMsg::PersonalMessage, crypto))
        }
        LinkEvent::AdminAction { origin, kind, target } => {
            if origin.is_some() {
                return Ok(());
            }
            (LinkMsg::Admin, crate::server::build_admin_payload(*kind, target, crypto))
        }
        LinkEvent::ToLeaf { origin, target_ident, payload } => {
            if origin.is_some() {
                return Ok(());
            }
            // Leaf→Hub: el wire lleva el ident destino adelante.
            build_leaf_directed_wire(*target_ident, payload, crypto)
        }
        LinkEvent::LeafAnnounce { .. } => return Ok(()),
        LinkEvent::Raw { origin, msg, payload } => {
            if origin.is_some() {
                return Ok(());
            }
            let Some(link_msg) = LinkMsg::from_u8(*msg) else {
                return Ok(());
            };
            (link_msg, payload.clone())
        }
    };
    write_link_to_stream(stream, msg, &payload).await.map_err(|e| e.to_string())
}

pub(crate) fn is_passthrough_opcode(op: LinkMsg) -> bool {
    matches!(
        op,
        LinkMsg::Error
            | LinkMsg::HubLeafConnected
            | LinkMsg::HubLeafDisconnected
            | LinkMsg::Avatar
            | LinkMsg::CustomDataTo
            | LinkMsg::CustomDataAll
            | LinkMsg::Nudge
            | LinkMsg::ScribbleUser
            | LinkMsg::ScribbleLeaf
            | LinkMsg::IUser
            | LinkMsg::IUserBin
            | LinkMsg::NoAdmin
            | LinkMsg::Browse
            | LinkMsg::BrowseData
            | LinkMsg::PrintAll
            | LinkMsg::PrintVroom
            | LinkMsg::PrintLevel
    )
}

fn build_leaf_join_payload_from_snapshot(user: &LinkUserSnapshot, msg: LinkMsg, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
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

fn build_chat_payload(from: &str, text: &str, msg: LinkMsg, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
    b.write_string(from);
    b.write_string(text);
    b.build_link_packet(msg)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn build_nick_changed_payload(old_name: &str, user: &LinkUserSnapshot, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
    b.write_string(old_name);
    b.write_bytes(&build_leaf_join_payload_from_snapshot(user, LinkMsg::LeafJoin, crypto));
    b.build_link_packet(LinkMsg::NickChanged)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn build_custom_name_payload(name: &str, custom_name: Option<&str>, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
    b.write_string(name);
    b.write_string(custom_name.unwrap_or(""));
    b.build_link_packet(LinkMsg::CustomName)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn build_private_payload(from: &str, to: &str, text: &str, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
    b.write_string(from);
    b.write_string(to);
    b.write_string(text);
    b.build_link_packet(LinkMsg::PrivateText)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn build_private_ignored_payload(from: &str, to: &str, crypto: Option<LinkCrypto>) -> Vec<u8> {
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
    b.write_string(from);
    b.write_string(to);
    b.build_link_packet(LinkMsg::PrivateIgnored)[LINK_PACKET_HEADER_LEN..].to_vec()
}

fn parse_link_chat_payload(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<(String, String)> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    let from = r.read_string().ok()?;
    let text = r.read_string().ok()?;
    Some((from, text))
}

fn parse_link_private_payload(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<(String, String, String)> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    let from = r.read_string().ok()?;
    let to = r.read_string().ok()?;
    let text = r.read_string().ok()?;
    Some((from, to, text))
}

fn parse_link_private_ignored_payload(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<(String, String)> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    let from = r.read_string().ok()?;
    let to = r.read_string().ok()?;
    Some((from, to))
}

fn parse_link_part_name(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<String> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    r.read_string().ok()
}

fn parse_link_nick_changed_payload(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<(String, LinkUser)> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    let old_name = r.read_string().ok()?;
    let remaining = r.read_bytes(r.remaining()).ok()?;
    let user = parse_userlist_item(&remaining, crypto)?;
    Some((old_name, user))
}

fn parse_link_custom_name_payload(payload: &[u8], crypto: Option<LinkCrypto>) -> Option<(String, Option<String>)> {
    let mut r = crate::protocol::LinkPacketReader::from_payload_with_crypto(payload, crypto);
    let name = r.read_string().ok()?;
    let custom_name = r.read_string().ok()?;
    let custom_name = if custom_name.is_empty() { None } else { Some(custom_name) };
    Some((name, custom_name))
}

fn build_server_join_from_link_user(
    user: &LinkUser,
    crypto: server_core::outbound::Crypto,
) -> bytes::Bytes {
    let mut w = proto_ares::PacketWriter::with_msg_crypto(proto_ares::TcpMsg::ServerJoin, crypto);
    w.write_u16_le(user.file_count).ok();
    w.write_u32_le(0).ok();
    write_ip(&mut w, user.external_ip);
    w.write_u16_le(user.port).ok();
    w.write_ipv4(std::net::Ipv4Addr::new(0, 0, 0, 0)).ok();
    w.write_u16_le(0).ok();
    w.write_u8(0).ok();
    w.write_string(&user.name).ok();
    write_ip(&mut w, user.local_ip);
    w.write_u8(user.browsable as u8).ok();
    // `LinkUser.level` ya está normalizado a la escala Ares al parsearlo.
    w.write_u8(user.level).ok();
    w.write_u8(user.age).ok();
    w.write_u8(user.sex).ok();
    w.write_u8(user.country).ok();
    w.write_string(&user.region).ok();
    w.write_u8(0).ok();
    bytes::Bytes::copy_from_slice(w.as_bytes())
}

fn build_server_part_for_name(
    name: &str,
    crypto: server_core::outbound::Crypto,
) -> bytes::Bytes {
    let mut w = proto_ares::PacketWriter::with_msg_crypto(proto_ares::TcpMsg::ServerPart, crypto);
    w.write_string(name).ok();
    bytes::Bytes::copy_from_slice(w.as_bytes())
}

fn write_ip(writer: &mut proto_ares::PacketWriter, ip: std::net::IpAddr) {
    match ip {
        std::net::IpAddr::V4(v4) => {
            writer.write_ipv4(v4).ok();
        }
        std::net::IpAddr::V6(_) => {
            writer.write_ipv4(std::net::Ipv4Addr::new(0, 0, 0, 0)).ok();
        }
    }
}

/// Construye el wire leaf→hub de un envío dirigido: `u32 target_ident` +
/// payload del opcode (paridad LeafOutbound.LeafPrint*/LeafPublicTextToLeaf/
/// LeafScribbleLeaf de sb0t).
fn build_leaf_directed_wire(
    target_ident: u32,
    directed: &server_core::LeafDirected,
    crypto: Option<LinkCrypto>,
) -> (LinkMsg, Vec<u8>) {
    use server_core::LeafDirected;
    let mut b = LinkPacketBuilder::new_with_crypto(crypto);
    b.write_u32(target_ident);
    let msg = match directed {
        LeafDirected::PrintAll { text } => {
            b.write_string_no_null(text);
            LinkMsg::PrintAll
        }
        LeafDirected::PrintVroom { vroom, text } => {
            b.write_u16(*vroom);
            b.write_string_no_null(text);
            LinkMsg::PrintVroom
        }
        LeafDirected::PrintLevel { level, text } => {
            b.write_u8(*level);
            b.write_string_no_null(text);
            LinkMsg::PrintLevel
        }
        LeafDirected::Public { from, text } => {
            b.write_string(from);
            b.write_string_no_null(text);
            LinkMsg::PublicToLeaf
        }
        LeafDirected::Emote { from, text } => {
            b.write_string(from);
            b.write_string_no_null(text);
            LinkMsg::EmoteToLeaf
        }
        LeafDirected::Scribble { from, height, data } => {
            b.write_string(from);
            b.write_u32(*height);
            b.write_bytes(data);
            LinkMsg::ScribbleLeaf
        }
    };
    (msg, b.build_link_packet(msg)[LINK_PACKET_HEADER_LEN..].to_vec())
}

/// Manda un scribble entrante del link a un custom client local como
/// CustomData `cb0t_scribble_*` (chunks de 4000, paridad AresClient.Scribble).
fn send_scribble_custom_data(u: &server_core::user_pool::AresUser, sender: &str, img: &[u8]) {
    if img.is_empty() {
        return;
    }
    if img.len() <= 4000 {
        let _ = u.send(server_core::outbound::build_custom_data_c(
            "cb0t_scribble_once", sender, img, u.ares_crypto,
        ));
        return;
    }
    let chunks: Vec<&[u8]> = img.chunks(4000).collect();
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.iter().enumerate() {
        let ident = if i == 0 { "cb0t_scribble_first" }
            else if i == last { "cb0t_scribble_last" }
            else { "cb0t_scribble_chunk" };
        let _ = u.send(server_core::outbound::build_custom_data_c(
            ident, sender, chunk, u.ares_crypto,
        ));
    }
}

/// Difunde un paquete a los usuarios locales, cifrando por destinatario.
///
/// `build` se llama con `None` (paquete plano compartido) y una vez por cada
/// cliente Ares que negoció cifrado: mandarle el plano hace que lo descarte
/// en silencio (no vería a los usuarios ni el chat del otro server enlazado).
fn broadcast_to_local_users<F>(app: &AppContext, build: F)
where
    F: Fn(server_core::outbound::Crypto) -> bytes::Bytes,
{
    let plain = build(None);
    for user in app.user_pool.users() {
        if user.logged_in && !user.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            if user.ares_crypto.is_some() {
                let _ = user.send(build(user.ares_crypto));
            } else {
                let _ = user.send(plain.clone());
            }
        }
    }
}
