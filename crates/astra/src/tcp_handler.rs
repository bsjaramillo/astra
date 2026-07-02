//! Handler de clientes TCP (Ares Galaxy) con defensa en capas anti-DDoS.
//!
//! ## Arquitectura
//!
//! Por cada cliente se spawnen 2 tasks:
//! - **reader**: lee paquetes del socket y los dispatcha a los handlers
//! - **writer**: drena el mpsc channel y escribe al socket
//!
//! El `AresUser` tiene un `mpsc::UnboundedSender<Bytes>`. Cualquier código
//! puede enviar paquetes (broadcast, PM, JOIN, PART) llamando a
//! `user.send(bytes)`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use proto_ares::{PacketReader, PacketWriter, TcpMsg};
use server_core::login::parse_login;
use server_core::outbound;
use server_core::{AppContext, LinkEvent, LinkUserSnapshot};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use astra_commands;
use astra_scripting::ScriptHandle;

const LINK_MSG_AVATAR: u8 = 11;
const LINK_MSG_CUSTOM_DATA_TO: u8 = 30;
const LINK_MSG_CUSTOM_DATA_ALL: u8 = 31;
const LINK_MSG_SCRIBBLE_LEAF: u8 = 34;
const LINK_MSG_BROWSE: u8 = 50;

/// Server features flags (de `TCPOutbound.cs` `MyFeatures`).
mod server_features {
    pub const SERVER_SUPPORTS_PVT: u8 = 1;
    pub const SERVER_SUPPORTS_SHARING: u8 = 2;
    pub const SERVER_SUPPORTS_COMPRESSION: u8 = 4;
    pub const SERVER_SUPPORTS_VC: u8 = 8;
    pub const SERVER_SUPPORTS_OPUS_VC: u8 = 16;
    pub const SERVER_SUPPORTS_PM_SCRIBBLES: u8 = 64;
    pub const SERVER_SUPPORTS_HTML: u8 = 128;
}

/// Maneja una conexión TCP entrante. Hace el setup de canales y despacha
/// la conexión a las tasks de reader/writer.
pub async fn handle_tcp_client(
    ctx: Arc<AppContext>,
    stream: TcpStream,
    peer: SocketAddr,
    scripting: ScriptHandle,
) -> anyhow::Result<()> {
    let ip = peer.ip();
    info!("nueva conexión TCP desde {}", peer);

    // ============================================================
    // CAPA 1+2+5: rate-limit, concurrent limit, failed-login ban
    // ============================================================
    if let Some(reason) = ctx.security.check_new_connection(ip) {
        warn!("REJECTED (capa 1/2/5): peer={} razón={:?}", peer, reason);
        let _ = send_server_error_to_stream(stream, reason.message()).await;
        return Ok(());
    }

    // Asegurar release al salir
    let security = ctx.security.clone();
    let _guard = scopeguard::guard(ip, move |ip| {
        security.on_disconnect(ip);
    });

    // Split del socket (owned, para que el writer task pueda ser 'static)
    let (read_half, write_half) = stream.into_split();

    // Canal mpsc para enviar al cliente
    let (tx, rx) = mpsc::unbounded_channel::<Bytes>();

    // Spawn writer task
    let writer_handle = tokio::spawn(writer_task(write_half, rx));

    // ============================================================
    // CAPA 3: Handshake timeout
    // ============================================================
    let handshake_timeout = Duration::from_secs(ctx.settings.security.handshake_timeout_secs);
    let mut reader = PacketReaderStream::new(read_half);

    let user = match timeout(handshake_timeout, async {
        process_handshake(&ctx, &mut reader, peer, tx.clone()).await
    })
    .await
    {
        Ok(Ok(Some(u))) => u,
        Ok(Ok(None)) => {
            debug!("handshake abortado por {}", peer);
            drop(tx);
            let _ = writer_handle.await;
            return Ok(());
        }
        Ok(Err(e)) => {
            warn!("error en handshake de {}: {}", peer, e);
            ctx.security.failed_logins.record_failure(ip);
            drop(tx);
            let _ = writer_handle.await;
            return Ok(());
        }
        Err(_) => {
            warn!("REJECTED (capa 3 - handshake timeout): {}", peer);
            ctx.security.failed_logins.record_failure(ip);
            drop(tx);
            let _ = writer_handle.await;
            return Ok(());
        }
    };

    let user_arc = user.clone();
    let user_id = user.id;

    // ============================================================
    // Post-login: enviar JOIN a los demás y USERLIST al nuevo
    // ============================================================
    send_initial_state(&ctx, &user).await;

    // Broadcast del JOIN a los usuarios existentes
    let join_pkt = outbound::build_join_or_userlist(&user);
    broadcast_to_room(&ctx, &user, join_pkt);
    ctx.publish_link_event(LinkEvent::Join {
        origin: None,
        user: LinkUserSnapshot::from_user(&user),
    });

    // ============================================================
    // Loop de lectura de mensajes
    // ============================================================
    let idle_timeout = Duration::from_secs(ctx.settings.security.idle_timeout_secs);
    loop {
        let pkt = match timeout(idle_timeout, reader.read_packet()).await {
            Ok(Ok(p)) if p.data.is_empty() => break, // EOF
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                warn!("error leyendo de id={}: {}", user_id, e);
                break;
            }
            Err(_) => {
                warn!("idle timeout para id={}, cerrando", user_id);
                break;
            }
        };

        ctx.stats.add_bytes_in(pkt.data.len() as u64);
        if let Err(e) = dispatch_message(&ctx, &scripting, &user_arc, &pkt).await {
            warn!("error procesando msg {:?} de id={}: {}", pkt.msg, user_id, e);
        }
    }

    // ============================================================
    // Cleanup
    // ============================================================
    ctx.user_pool.remove(user_id);
    ctx.stats.on_user_part();

    // Broadcast del PART
    let part_pkt = outbound::build_part(&user_arc);
    broadcast_to_room(&ctx, &user_arc, part_pkt);
    ctx.publish_link_event(LinkEvent::Part {
        origin: None,
        name: user_arc.name.read().clone(),
    });

    // Cerrar el canal para que el writer termine
    drop(tx);
    let _ = writer_handle.await;

    info!("usuario id={} '{}' desconectado", user_id, user.name.read());
    Ok(())
}

/// Task que drena el mpsc y escribe al socket.
async fn writer_task(mut write_half: OwnedWriteHalf, mut rx: mpsc::UnboundedReceiver<Bytes>) {
    while let Some(data) = rx.recv().await {
        if let Err(e) = write_half.write_all(&data).await {
            debug!("writer: error escribiendo: {}", e);
            break;
        }
    }
    debug!("writer: terminado");
}

/// Wrapper sobre `OwnedReadHalf` que lee paquetes con framing implícito de Ares.
struct PacketReaderStream {
    inner: OwnedReadHalf,
    buf: Vec<u8>,
}

impl PacketReaderStream {
    fn new(inner: OwnedReadHalf) -> Self {
        Self {
            inner,
            buf: vec![0u8; 8192],
        }
    }

    /// Lee un paquete. Retorna `None` en EOF.
    async fn read_packet(&mut self) -> std::io::Result<RawPacket> {
        let n = self.inner.read(&mut self.buf).await?;
        if n == 0 {
            return Ok(RawPacket::eof());
        }
        let opcode = self.buf[0];
        let msg = TcpMsg::from_u8(opcode);
        let data = self.buf[..n].to_vec();
        Ok(RawPacket { msg, data })
    }
}

/// Un paquete crudo leído del socket.
#[derive(Debug)]
struct RawPacket {
    msg: Option<TcpMsg>,
    data: Vec<u8>,
}

impl RawPacket {
    fn eof() -> Self {
        Self { msg: None, data: Vec::new() }
    }
}

/// Procesa el handshake: lee el primer paquete, parsea el login,
/// valida, y registra al usuario. Retorna `Some(Arc<AresUser>)` en éxito.
async fn process_handshake(
    ctx: &Arc<AppContext>,
    reader: &mut PacketReaderStream,
    peer: SocketAddr,
    tx: mpsc::UnboundedSender<Bytes>,
) -> anyhow::Result<Option<Arc<server_core::user_pool::AresUser>>> {
    let pkt = reader.read_packet().await?;
    if pkt.data.is_empty() {
        return Ok(None);
    }
    let opcode = pkt.data[0];
    let msg = match TcpMsg::from_u8(opcode) {
        Some(m) => m,
        None => {
            warn!("REJECTED (opcode desconocido {}): {}", opcode, peer);
            ctx.security.failed_logins.record_failure(peer.ip());
            let _ = tx.send(server_error_packet("Unknown protocol opcode"));
            return Ok(None);
        }
    };

    match msg {
        TcpMsg::ClientLogin | TcpMsg::ClientRelogin => {
            match parse_login(&pkt.data) {
                Ok(login) => {
                    // CAPA 4: validación
                    if let Err(reason) = ctx.security.login_validator.validate(&login) {
                        warn!("REJECTED (capa 4): peer={} nick='{}' razón={:?}", peer, login.org_name, reason);
                        ctx.security.failed_logins.record_failure(peer.ip());
                        let _ = tx.send(server_error_packet(reason.message()));
                        return Ok(None);
                    }

                    let external_ip = peer.ip();
                    let now_ms = server_core::time::unix_time();

                    // Ban persistente
                    if ctx.bans.is_banned(&login.guid, external_ip) {
                        warn!("REJECTED (ban persistente): peer={}", peer);
                        let _ = tx.send(server_error_packet("You are banned from this room"));
                        return Ok(None);
                    }

                    // Join-flood
                    if ctx.user_history.is_join_flooding(external_ip, now_ms) {
                        warn!("REJECTED (join-flood): peer={}", peer);
                        let _ = tx.send(server_error_packet("Joining too quickly. Please wait 15 seconds."));
                        return Ok(None);
                    }

                    // Construir usuario
                    let id = ctx.user_pool.next_id();
                    let mut user = server_core::login::build_ares_user(id, external_ip, login);
                    user.sender = Some(tx.clone());
                    user.logged_in = true;  // marcar ANTES de envolver en Arc
                    let user_arc = Arc::new(user);
                    ctx.user_pool.add(user_arc.clone());
                    ctx.stats.on_user_join(ctx.user_pool.len() as u32);

                    ctx.user_history.add_user(
                        &user_arc.name.read(),
                        &user_arc.version,
                        &user_arc.guid,
                        user_arc.external_ip,
                        user_arc.local_ip,
                        user_arc.data_port,
                        now_ms,
                    );

                    ctx.security.failed_logins.record_success(peer.ip());

                    info!(
                        "LOGIN OK: id={} nick='{}' ver='{}' ares={} cbot={} guid={}",
                        id, user_arc.name.read(), user_arc.version, user_arc.ares, user_arc.cbot,
                        user_arc.guid.iter().map(|b| format!("{:02x}", b)).collect::<String>()
                    );

                    // Paquetes de bienvenida (LoginAck + MyFeatures)
                    tx.send(build_login_ack(&user_arc, &ctx.settings.room_name))?;
                    tx.send(build_my_features(&user_arc))?;

                    Ok(Some(user_arc))
                }
                Err(e) => {
                    warn!("login inválido desde {}: {}", peer, e);
                    ctx.security.failed_logins.record_failure(peer.ip());
                    let _ = tx.send(server_error_packet(&format!("login: {}", e)));
                    Ok(None)
                }
            }
        }
        _ => {
            warn!("primer paquete no es login: {:?} de {}", msg, peer);
            Ok(None)
        }
    }
}

/// Envía el estado inicial al usuario recién conectado:
/// topic, userlist del bot, lista de usuarios, op change, etc.
async fn send_initial_state(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>) {
    let user_id = user.id;

    // Topic
    let _ = user.send(outbound::build_topic_first(&ctx.current_room_topic()));

    // Bot fantasma
    let _ = user.send(outbound::build_userlist_bot(&ctx.settings.bot_name));

    // Userlist de todos los usuarios conectados
    let others = ctx.user_pool.users();
    for other in others {
        if other.id != user_id && other.logged_in {
            let _ = user.send(outbound::build_userlist_item(&other));
        }
    }

    // End of userlist
    let _ = user.send(outbound::build_userlist_end());

    // OpChange (este usuario)
    let level = *user.level.read();
    let is_op = level as u8 >= iconnect::ILevel::Moderator as u8;
    let _ = user.send(outbound::build_opchange(is_op));
}

/// Despacha un paquete al handler correspondiente.
async fn dispatch_message(
    ctx: &Arc<AppContext>,
    scripting: &ScriptHandle,
    user: &Arc<server_core::user_pool::AresUser>,
    pkt: &RawPacket,
) -> anyhow::Result<()> {
    let msg = match pkt.msg {
        Some(m) => m,
        None => {
            warn!("opcode desconocido {} de id={}", pkt.data[0], user.id);
            return Ok(());
        }
    };

    match msg {
        TcpMsg::FastPing => {
            // No-op: keep-alive del cliente. El MyFeatures ya lo configuró.
            debug!("fastping de id={}", user.id);
        }
        TcpMsg::ClientDummy => {
            // Dummy keep-alive
        }
        TcpMsg::ClientUpdateStatus => {
            debug!("update status de id={}", user.id);
            // Por ahora: reenvía el status (broadcast del join refresh)
            let join_pkt = outbound::build_join_or_userlist(user);
            broadcast_to_room(ctx, user, join_pkt);
            ctx.publish_link_event(LinkEvent::UserUpdated {
                origin: None,
                user: LinkUserSnapshot::from_user(user),
            });
        }
        TcpMsg::ClientIgnorelist => {
            handle_ignore_list(user, &pkt.data[1..]);
        }
        TcpMsg::Avatar => {
            publish_raw_link(ctx, LINK_MSG_AVATAR, &pkt.data[1..]);
        }
        TcpMsg::Public => {
            handle_public(ctx, user, &pkt.data[1..], &scripting).await;
        }
        TcpMsg::Emote => {
            handle_emote(ctx, user, &pkt.data[1..]).await;
        }
        TcpMsg::Pmt => {
            handle_pvt(ctx, user, &pkt.data[1..]).await;
        }
        TcpMsg::PersonalMessage => {
            handle_personal_message(ctx, user, &pkt.data[1..]).await;
        }
        TcpMsg::ClientBrowse => {
            publish_raw_link(ctx, LINK_MSG_BROWSE, &pkt.data[1..]);
        }
        TcpMsg::CustomData => {
            publish_raw_link(ctx, LINK_MSG_CUSTOM_DATA_TO, &pkt.data[1..]);
        }
        TcpMsg::CustomDataAll => {
            publish_raw_link(ctx, LINK_MSG_CUSTOM_DATA_ALL, &pkt.data[1..]);
        }
        TcpMsg::ClientScribbleRoomFirst | TcpMsg::ClientScribbleRoomChunk => {
            publish_raw_link(ctx, LINK_MSG_SCRIBBLE_LEAF, &pkt.data[1..]);
        }
        _ => {
            debug!("mensaje {:?} de id={} (no procesado en esta fase)", msg, user.id);
        }
    }
    Ok(())
}

fn publish_raw_link(ctx: &AppContext, msg: u8, payload: &[u8]) {
    if ctx.link_receiver_count() == 0 {
        return;
    }
    ctx.publish_link_event(LinkEvent::Raw {
        origin: None,
        msg,
        payload: payload.to_vec(),
    });
}

/// Maneja MSG_CHAT_CLIENT_PUBLIC (10).
/// Formato: `str text`
async fn handle_public(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    data: &[u8],
    scripting: &ScriptHandle,
) {
    let mut r = PacketReader::new(data);
    let text = match r.read_string() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }

    let name = user.name.read().clone();

    // Si el mensaje empieza con '/', es un comando slash
    if let Some((cmd, args)) = astra_commands::parse_command(&text) {
        if astra_commands::dispatch_builtin(ctx, user, cmd, args) {
            debug!("comando built-in de '{}': /{} {}", name, cmd, args);
            return;
        }
        astra_commands::dispatch(ctx, scripting, &name, cmd, args);
        debug!("comando slash de '{}': /{} {}", name, cmd, args);
        return;
    }

    let pkt = outbound::build_public(&name, &text);
    broadcast_to_room(ctx, user, pkt);
    ctx.publish_link_event(LinkEvent::Public {
        origin: None,
        from: name.clone(),
        text: text.clone(),
    });
    debug!("public de '{}': {}", name, text);
}

/// Maneja MSG_CHAT_CLIENT_EMOTE (11).
async fn handle_emote(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>, data: &[u8]) {
    let mut r = PacketReader::new(data);
    let text = match r.read_string() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }
    let name = user.name.read().clone();
    let pkt = outbound::build_emote(&name, &text);
    broadcast_to_room(ctx, user, pkt);
    ctx.publish_link_event(LinkEvent::Emote {
        origin: None,
        from: name,
        text,
    });
}

/// Maneja MSG_CHAT_CLIENT_PVT (25).
/// Formato: `str target_name, str text`
async fn handle_pvt(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>, data: &[u8]) {
    let mut r = PacketReader::new(data);
    let target_name = match r.read_string() {
        Ok(s) => s,
        Err(_) => return,
    };
    let text = match r.read_string() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }

    // Buscar al destinatario
    if let Some(target) = ctx.user_pool.get_by_name(&target_name) {
        let from = user.name.read().clone();
        if target
            .ignore_list
            .read()
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(&from))
        {
            let mut w = PacketWriter::with_msg(TcpMsg::ServerIsIgnoringYou);
            w.write_string(&target_name).ok();
            let _ = user.send(Bytes::copy_from_slice(w.as_bytes()));
            ctx.publish_link_event(LinkEvent::PrivateIgnored {
                origin: None,
                from,
                to: target_name,
            });
        } else {
            let pkt = outbound::build_pvt(&from, &text);
            if !target.send(pkt) {
                warn!("no se pudo enviar PM de '{}' a '{}'", from, target_name);
            }
        }
    } else if ctx.link_receiver_count() > 0 {
        ctx.publish_link_event(LinkEvent::Private {
            origin: None,
            from: user.name.read().clone(),
            to: target_name,
            text,
        });
    } else {
        // NoSuch
        let mut w = PacketWriter::with_msg(TcpMsg::ServerNosuch);
        w.write_string(&format!("User '{}' not found", target_name)).ok();
        user.send(Bytes::copy_from_slice(w.as_bytes()));
    }
}

/// Maneja MSG_CHAT_CLIENT_IGNORELIST (45).
/// Formato: lista de strings Ares consecutivas.
fn handle_ignore_list(user: &Arc<server_core::user_pool::AresUser>, data: &[u8]) {
    let mut r = PacketReader::new(data);
    let mut list = Vec::new();
    while r.remaining() > 0 {
        let Ok(entry) = r.read_string() else {
            break;
        };
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !list.iter().any(|e: &String| e.eq_ignore_ascii_case(trimmed)) {
            list.push(trimmed.to_string());
        }
    }
    *user.ignore_list.write() = list;
}

/// Maneja MSG_CHAT_CLIENT_PERSONAL_MESSAGE (13).
async fn handle_personal_message(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>, data: &[u8]) {
    let mut r = PacketReader::new(data);
    let text = match r.read_string() {
        Ok(s) => s,
        Err(_) => return,
    };
    *user.personal_message.lock() = text.clone();

    // Broadcast a la sala: cada uno recibe un "MSG_CHAT_SERVER_PERSONAL_MESSAGE"
    // con el nuevo PM. Lo simplificamos: el server reenvía a cada user.
    let mut w = PacketWriter::with_msg(TcpMsg::PersonalMessage);
    w.write_string(&user.name.read()).ok();
    w.write_string(&text).ok();
    let pkt = Bytes::copy_from_slice(w.as_bytes());
    broadcast_to_room(ctx, user, pkt);
    ctx.publish_link_event(LinkEvent::PersonalMessage {
        origin: None,
        name: user.name.read().clone(),
        text,
    });
}

/// Broadcast a todos los usuarios en la misma vroom que `sender`.
/// `sender` también lo recibe (compat con sb0t original).
fn broadcast_to_room(ctx: &AppContext, sender: &server_core::user_pool::AresUser, pkt: Bytes) {
    let sender_id = sender.id;
    let vroom = *sender.vroom.read();
    let users = ctx.user_pool.users();
    for u in users {
        if u.logged_in && *u.vroom.read() == vroom && !u.quarantined {
            // En el sb0t original el sender también recibe el broadcast
            // (excepto en algunos casos). Aquí también.
            let _ = u.send(pkt.clone());
            let _ = sender_id; // unused but indicates intent
        }
    }
}

// ============================================================================
// Constructores de paquetes de bienvenida
// ============================================================================

fn server_error_packet(text: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerError);
    w.write_string(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

async fn send_server_error_to_stream(mut stream: TcpStream, text: &str) -> anyhow::Result<()> {
    stream.write_all(&server_error_packet(text)).await?;
    Ok(())
}

fn build_login_ack(user: &server_core::user_pool::AresUser, room_name: &str) -> Bytes {
    let mut w = PacketWriter::with_msg(TcpMsg::ServerLoginAck);
    w.write_string(&user.name.read()).ok();
    w.write_string(room_name).ok();
    w.write_string(&user.version).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

fn build_my_features(user: &server_core::user_pool::AresUser) -> Bytes {
    use server_features::*;

    let mut flag: u8 = SERVER_SUPPORTS_PVT
        | SERVER_SUPPORTS_SHARING
        | SERVER_SUPPORTS_COMPRESSION
        | SERVER_SUPPORTS_VC
        | SERVER_SUPPORTS_OPUS_VC
        | SERVER_SUPPORTS_PM_SCRIBBLES;

    if user.supports_html {
        flag |= SERVER_SUPPORTS_HTML;
    }

    let mut w = PacketWriter::with_msg(TcpMsg::ServerMyFeatures);
    w.write_string(&format!("Astra {} - chat server", env!("CARGO_PKG_VERSION"))).ok();
    w.write_u8(flag).ok();
    w.write_u8(63).ok();
    w.write_u8(0).ok();
    w.write_u32_le(0).ok();
    w.write_u8(1).ok();
    Bytes::copy_from_slice(w.as_bytes())
}
