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

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::time::interval;
use tracing::{info, warn};

use server_core::AppContext;

use crate::protocol::{
    read_link_from_stream, write_link_to_stream, LinkMsg, LinkPacketBuilder, LinkUser,
    MSG_LINK_PROTO,
};

const LINK_PACKET_HEADER_LEN: usize = 3;

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
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("error conectando: {}", e))?;

        // Calcular SHA1(name + guid.reverse()) — igual que el sb0t original
        let name = self.app.settings.room_name.clone();
        let guid = self.app.settings.guid.clone();

        // Si el guid es corto, lo hasheamos a 16 bytes (sb0t original)
        let guid_bytes: [u8; 16] = if guid.len() >= 16 {
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&guid.as_bytes()[..16]);
            arr
        } else {
            use sha1::{Digest, Sha1};
            let mut hasher = Sha1::new();
            hasher.update(guid.as_bytes());
            let result = hasher.finalize();
            // El sb0t usa solo los primeros 16 bytes del SHA1
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&result[..16]);
            arr
        };

        let mut guid_rev = guid_bytes;
        guid_rev.reverse();
        let mut combined = Vec::new();
        combined.extend_from_slice(name.as_bytes());
        combined.extend_from_slice(&guid_rev);
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(&combined);
        let sha1_digest = hasher.finalize();
        // El sb0t original usa solo los primeros 16 bytes del SHA1
        let mut login_hash = [0u8; 16];
        login_hash.copy_from_slice(&sha1_digest[..16]);

        // Enviar LeafLogin envuelto en MSG_LINK_PROTO
        let mut b = LinkPacketBuilder::new();
        b.write_string(&name);
        b.write_guid(&login_hash);
        b.write_u16(MSG_LINK_PROTO as u16);
        b.write_u16(self.app.settings.port);
        let login_payload = {
            let mut tmp = Vec::new();
            let packet = b.build_link_packet(LinkMsg::LeafLogin);
            // El packet tiene: u16 len + u8 op + args
            // Necesitamos solo los args (sin u16 len + u8 op)
            tmp.extend_from_slice(&packet[LINK_PACKET_HEADER_LEN..]);
            tmp
        };

        let mut stream = stream;
        write_link_to_stream(&mut stream, LinkMsg::LeafLogin, &login_payload)
            .await
            .map_err(|e| format!("error enviando login: {}", e))?;

        // Leer HubAck
        let (op, payload) = read_link_from_stream(&mut stream)
            .await
            .map_err(|e| format!("error leyendo ACK: {}", e))?;
        if op != LinkMsg::HubAck {
            return Err(format!("esperado HubAck, recibí {:?}", op));
        }
        let mut r = crate::protocol::LinkPacketReader::from_payload(&payload);
        let ack_status = r.read_u8().map_err(|e| format!("ACK malformado: {}", e))?;
        if ack_status != 1 {
            return Err(format!("login rechazado: status={}", ack_status));
        }
        info!("link client: ACK recibido, leyendo userlist...");

        // Leer userlist
        loop {
            let (op, payload) = read_link_from_stream(&mut stream)
                .await
                .map_err(|e| format!("error leyendo userlist: {}", e))?;
            match op {
                LinkMsg::UserlistItem => {
                    if let Some(user) = parse_userlist_item(&payload) {
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
        let mut sync_timer = interval(Duration::from_secs(60));
        ping_timer.tick().await; // primer tick inmediato
        let mut synced_users: HashMap<u16, String> = HashMap::new();
        sync_local_users_to_hub(&self.app, &mut stream, &mut synced_users).await?;

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
                    let (op, _payload) = match read_result {
                        Ok(r) => r,
                        Err(_) => {
                            info!("link client: conexión cerrada");
                            return Ok(());
                        }
                    };
                    if op == LinkMsg::HubPong {
                        // OK, pong recibido
                    } else {
                        warn!("link client: opcode no manejado: {:?}", op);
                    }
                }
                _ = sync_timer.tick() => {
                    sync_local_users_to_hub(&self.app, &mut stream, &mut synced_users).await?;
                }
            }
        }
    }

    /// Cierra la conexión.
    pub fn close(&self) {
        *self.active.lock() = false;
    }
}

fn build_leaf_join_payload(user: &server_core::user_pool::AresUser) -> Vec<u8> {
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
    b.write_u16(user.vroom);
    b.write_u8(u8::from(user.custom_client));
    b.write_u8(u8::from(user.muzzled));
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
    synced_users: &mut HashMap<u16, String>,
) -> Result<(), String> {
    let current_users: HashMap<u16, std::sync::Arc<server_core::user_pool::AresUser>> = app
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in)
        .map(|u| (u.id, u))
        .collect();

    // Nuevos users: enviar LeafJoin
    for (id, user) in &current_users {
        if !synced_users.contains_key(id) {
            let payload = build_leaf_join_payload(user);
            write_link_to_stream(&mut *stream, LinkMsg::LeafJoin, &payload)
                .await
                .map_err(|e| format!("error enviando LeafJoin: {}", e))?;
            synced_users.insert(*id, user.name.read().clone());
        }
    }

    // Users que salieron: enviar Part
    let departed_ids: Vec<u16> = synced_users
        .keys()
        .copied()
        .filter(|id| !current_users.contains_key(id))
        .collect();

    for id in departed_ids {
        if let Some(name) = synced_users.remove(&id) {
            let mut b = LinkPacketBuilder::new();
            b.write_string(&name);
            let packet = b.build_link_packet(LinkMsg::Part);
            write_link_to_stream(&mut *stream, LinkMsg::Part, &packet[LINK_PACKET_HEADER_LEN..])
                .await
                .map_err(|e| format!("error enviando Part: {}", e))?;
        }
    }

    Ok(())
}

fn parse_userlist_item(payload: &[u8]) -> Option<LinkUser> {
    let mut r = crate::protocol::LinkPacketReader::from_payload(payload);
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
