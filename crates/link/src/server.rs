//! LinkServer: acepta conexiones de otros Astra servers (modo hub).
//!
//! Cuando un leaf se conecta:
//! 1. Lee el `LeafLogin` (nombre + SHA1 + LINK_PROTO + port)
//! 2. Responde con `HubAck` (status = 1)
//! 3. Envía la userlist local como `UserlistItem` (uno por user)
//! 4. Cierra con `LeafUserlistEnd`
//! 5. Loop de keep-alive con `HubPong` cada 30s

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::interval;
use tracing::{error, info, warn};

use server_core::AppContext;

use crate::protocol::{
    read_link_from_stream, write_link_to_stream, LinkMsg, LinkPacketBuilder, MSG_LINK_PROTO,
};

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

    /// Maneja una conexión entrante de un leaf.
async fn handle_leaf_connection(
    app: Arc<AppContext>,
    mut stream: TcpStream,
) -> Result<(), String> {
    // 1. Leer LeafLogin
    let (op, payload) = read_link_from_stream(&mut stream)
        .await
        .map_err(|e| format!("error leyendo login: {}", e))?;
    if op != LinkMsg::LeafLogin {
        return Err(format!("esperado LeafLogin, recibí {:?}", op));
    }

    let mut r = crate::protocol::LinkPacketReader::new(&payload);
    // El payload ya NO incluye el op byte (read_link_from_stream lo
    // extrae), por lo que usamos `from_payload` en vez de `new`.
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
        b.write_u16(user.vroom);
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
            read_result = read_link_from_stream(&mut stream) => {
                let (op, _payload) = match read_result {
                    Ok(r) => r,
                    Err(_) => {
                        info!("link server: leaf '{}' desconectado", leaf_name);
                        return Ok(());
                    }
                };
                match op {
                    LinkMsg::LeafPing => {
                        // HubPong ya se envía periódicamente arriba
                    }
                    LinkMsg::Part => {
                        info!("link server: leaf user se fue");
                    }
                    LinkMsg::LeafJoin => {
                        info!("link server: leaf user se unió");
                    }
                    _ => {
                        warn!("link server: opcode no manejado: {:?}", op);
                    }
                }
            }
        }
    }
}
