//! Handler de una conexión WebSocket (después del handshake).
//!
//! Lee frames de texto, dispatcha los mensajes al server core.
//! El envío de broadcasts se hace a través del `user.sender` (canal
//! `mpsc::UnboundedSender<Bytes>`) — la write task del WS lee de ese
//! mismo canal (tomando la referencia prestada).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use server_core::login::{build_ares_user, LoginData};
use server_core::outbound;
use server_core::AppContext;
use server_core::user_pool::AresUser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::protocol::{
    self, build_ack, build_myfeatures, build_opchange, build_topic, build_user_item,
    build_userlist_bot, build_userlist_end,
};
use crate::ws::{write_close_frame, write_text_frame, WsOpcode};
use crate::ws_outbound::{build_initial_state_ws, build_userlist_item_ws, translate_broadcast};

/// Maneja una conexión WebSocket después del handshake.
pub async fn handle_connection(
    ctx: Arc<AppContext>,
    stream: TcpStream,
    peer: SocketAddr,
) -> anyhow::Result<()> {
    info!("WS conectado: {}", peer);

    // Split del stream
    let (mut read_half, mut write_half) = stream.into_split();

    // Canal mpsc para mensajes de error binarios (poco común)
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();

    // Canal de texto para todos los mensajes al cliente WS:
    // - estado inicial (ACK, MyFeatures, etc.) como strings
    // - broadcasts traducidos a texto
    let (ws_text_tx, mut ws_text_rx) = mpsc::unbounded_channel::<String>();

    // Task de escritura: drena el canal de texto y envía como frames de texto WS.
    // Termina cuando `ws_text_tx` se cierra (drop del user).
    let write_task = tokio::spawn(async move {
        while let Some(text) = ws_text_rx.recv().await {
            if let Err(e) = write_text_frame(&mut write_half, &text).await {
                debug!("ws write error: {}", e);
                break;
            }
            if let Err(e) = write_half.flush().await {
                debug!("ws flush error: {}", e);
                break;
            }
        }
        debug!("ws write task terminado");
    });

    // Esperar el primer mensaje (LOGIN)
    let mut buf = BytesMut::with_capacity(8192);
    let handshake_timeout = Duration::from_secs(15);
    let user = match timeout(handshake_timeout, async {
        ws_handshake_login(&ctx.clone(), &mut read_half, &mut buf, &tx, &ws_text_tx, peer).await
    })
    .await
    {
        Ok(Ok(Some(u))) => u,
        Ok(Ok(None)) => {
            drop(tx);
            let _ = write_task.await;
            return Ok(());
        }
        Ok(Err(e)) => {
            warn!("ws error en handshake de {}: {}", peer, e);
            drop(tx);
            let _ = write_task.await;
            return Ok(());
        }
        Err(_) => {
            warn!("ws handshake timeout de {}", peer);
            drop(tx);
            let _ = write_task.await;
            return Ok(());
        }
    };

    let user_id = user.id;

    // Enviar estado inicial (directo a ws_text_tx como strings)
    send_initial_state_ws(&ctx, &user, &ws_text_tx).await;

    // Broadcast del JOIN a usuarios Ares
    let join_pkt = outbound::build_join_or_userlist(&user);
    broadcast_to_room(&ctx, &user, join_pkt);

    // Loop principal
    let idle_timeout = Duration::from_secs(ctx.settings.security.idle_timeout_secs);
    loop {
        let read_result = timeout(idle_timeout, async {
            read_ws_frame(&mut read_half, &mut buf).await
        })
        .await;
        let frame = match read_result {
            Ok(Ok(Some(frame))) => frame,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => {
                warn!("ws error leyendo de id={}: {}", user_id, e);
                break;
            }
            Err(_) => {
                warn!("ws idle timeout para id={}", user_id);
                break;
            }
        };

        let (opcode, payload) = frame;
        match opcode {
            WsOpcode::Text => {
                let text = match String::from_utf8(payload) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if let Err(e) = dispatch_ws_message(&ctx, &user, &text).await {
                    warn!("ws error dispatch de id={}: {}", user_id, e);
                }
            }
            WsOpcode::Ping => {}
            WsOpcode::Close => {
                debug!("ws close de id={}", user_id);
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    ctx.user_pool.remove(user_id);
    ctx.stats.on_user_part();

    // Broadcast del PART
    let part_pkt = outbound::build_part(&user);
    broadcast_to_room(&ctx, &user, part_pkt);

    // Cerrar: drop del user → drop del sender → write task termina → enviar close
    drop(user);
    drop(tx);
    let _ = write_task.await;

    info!("ws desconectado: id={}", user_id);
    Ok(())
}

/// Lee un frame WebSocket (cliente→servidor, masked).
async fn read_ws_frame(
    read_half: &mut OwnedReadHalf,
    buf: &mut BytesMut,
) -> anyhow::Result<Option<(WsOpcode, Vec<u8>)>> {
    loop {
        while buf.len() < 2 {
            let mut tmp = [0u8; 4096];
            let n = read_half.read(&mut tmp).await?;
            if n == 0 {
                return Ok(None);
            }
            buf.extend_from_slice(&tmp[..n]);
        }

        let b1 = buf[0];
        let b2 = buf[1];
        let opcode = WsOpcode::from_u8(b1 & 0x0F)
            .ok_or_else(|| anyhow::anyhow!("opcode WS desconocido: {}", b1 & 0x0F))?;
        let masked = (b2 & 0x80) != 0;
        let mut len = (b2 & 0x7F) as usize;
        let mut header_len = 2;

        if len == 126 {
            while buf.len() < 4 {
                let mut tmp = [0u8; 4096];
                let n = read_half.read(&mut tmp).await?;
                if n == 0 {
                    return Ok(None);
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
            header_len = 4;
        } else if len == 127 {
            while buf.len() < 10 {
                let mut tmp = [0u8; 4096];
                let n = read_half.read(&mut tmp).await?;
                if n == 0 {
                    return Ok(None);
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&buf[2..10]);
            len = u64::from_be_bytes(bytes) as usize;
            header_len = 10;
        }

        let mask_len = if masked { 4 } else { 0 };
        let total_len = header_len + mask_len + len;

        while buf.len() < total_len {
            let mut tmp = [0u8; 8192];
            let n = read_half.read(&mut tmp).await?;
            if n == 0 {
                return Ok(None);
            }
            buf.extend_from_slice(&tmp[..n]);
        }

        let mut payload = buf[header_len + mask_len..total_len].to_vec();
        if masked {
            let mask = &buf[header_len..header_len + 4];
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        buf.advance(total_len);

        return Ok(Some((opcode, payload)));
    }
}

/// Realiza el login: lee el primer frame de texto, espera `LOGIN:...`,
/// parsea, crea el user y lo registra.
async fn ws_handshake_login(
    ctx: &Arc<AppContext>,
    read_half: &mut OwnedReadHalf,
    buf: &mut BytesMut,
    tx: &mpsc::UnboundedSender<Bytes>,
    ws_text_tx: &mpsc::UnboundedSender<String>,
    peer: SocketAddr,
) -> anyhow::Result<Option<Arc<AresUser>>> {
    let frame = read_ws_frame(read_half, buf).await?;
    let (opcode, payload) = match frame {
        Some(f) => f,
        None => return Ok(None),
    };
    if !matches!(opcode, WsOpcode::Text) {
        return Ok(None);
    }

    let text = String::from_utf8_lossy(&payload);
    let (ident, args) = match protocol::parse_incoming(&text) {
        Some(p) => p,
        None => {
            warn!("ws: primer frame no parseable de {}", peer);
            return Ok(None);
        }
    };

    if !matches!(ident, "LOGIN" | "INBIZIO_LOGIN") {
        warn!("ws: primer frame no es login: {} de {}", ident, peer);
        return Ok(None);
    }

    let login = match protocol::parse_login(args) {
        Some(l) => l,
        None => {
            warn!("ws: login malformado de {}", peer);
            return Ok(None);
        }
    };

    let external_ip = peer.ip();
    let now_ms = server_core::time::unix_time();

    let guid_arr: [u8; 16] = login.guid;
    if ctx.bans.is_banned(&guid_arr, external_ip) {
        warn!("REJECTED (ban persistente): peer={} nick='{}'", peer, login.name);
        let _ = tx.send(Bytes::from_static(b"ERROR:You are banned from this room"));
        return Ok(None);
    }
    if ctx.user_history.is_join_flooding(external_ip, now_ms) {
        warn!("REJECTED (join-flood): peer={} nick='{}'", peer, login.name);
        let _ = tx.send(Bytes::from_static(b"ERROR:Joining too quickly"));
        return Ok(None);
    }

    let id = ctx.user_pool.next_id();
    let mut user = build_ares_user(id, external_ip, make_login_data(&login));
    user.sender = Some(tx.clone());
    user.ws_text_sender = Some(ws_text_tx.clone());
    user.logged_in = true;
    user.web_client = true;

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

    info!(
        "WS LOGIN OK: id={} nick='{}' inbizier={}",
        id,
        user_arc.name.read(),
        login.inbizier_web || login.inbizier_mobile
    );

    Ok(Some(user_arc))
}

/// Convierte los args parseados del LOGIN en `server_core::login::LoginData`.
fn make_login_data(login: &protocol::LoginArgs) -> LoginData {
    use std::net::Ipv4Addr;
    LoginData {
        guid: login.guid,
        file_count: 0,
        crypto: false,
        data_port: 0,
        node_ip: Ipv4Addr::UNSPECIFIED,
        node_port: 0,
        org_name: login.name.clone(),
        version: login.version.clone(),
        is_ares: false,
        is_cbot: false,
        local_ip: Ipv4Addr::UNSPECIFIED,
        browsable: false,
        current_uploads: 0,
        max_uploads: 0,
        current_queued: 0,
        age: 0,
        sex: 0,
        country: 0,
        region: String::new(),
        voice_chat_public: false,
        voice_chat_private: false,
        voice_opus_chat_public: false,
        voice_opus_chat_private: false,
        supports_html: false,
    }
}

/// Envía el estado inicial al cliente WS recién conectado (formato texto directo).
async fn send_initial_state_ws(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    tx: &mpsc::UnboundedSender<String>,
) {
    let room_name = ctx.settings.room_name.clone();
    let room_topic = ctx.settings.room_topic.clone();
    let bot_name = ctx.settings.bot_name.clone();

    // ACK
    let _ = tx.send(build_ack(&user.name.read(), &room_name, &user.version));
    // MyFeatures
    let _ = tx.send(build_myfeatures(&user.version, 0x1F, 0, 0));
    // Topic
    let _ = tx.send(build_topic(&room_topic));
    // Bot
    let _ = tx.send(build_userlist_bot(&bot_name));

    // Userlist
    let user_id = user.id;
    for other in ctx.user_pool.users() {
        if other.id != user_id && other.logged_in {
            let item = build_userlist_item_ws(&other);
            let _ = tx.send(item);
        }
    }

    // End
    let _ = tx.send(build_userlist_end());

    // OpChange
    let level = *user.level.read() as u8;
    let _ = tx.send(build_opchange(level));

    let _ = (room_name, room_topic, bot_name); // silence unused
}

/// Construye un item de userlist completo para un usuario.
fn build_userlist_item_full(user: &Arc<AresUser>) -> String {
    let features = outbound::build_features(user);
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

/// Despacha un mensaje entrante del cliente WS.
async fn dispatch_ws_message(
    ctx: &Arc<AppContext>,
    user: &Arc<AresUser>,
    text: &str,
) -> anyhow::Result<()> {
    let (ident, args) = match protocol::parse_incoming(text) {
        Some(p) => p,
        None => return Ok(()),
    };

    match ident {
        "PUBLIC" => handle_ws_public(ctx, user, args),
        "EMOTE" => handle_ws_emote(ctx, user, args),
        "PING" => {
            debug!("ws PING de id={}", user.id);
        }
        "COMMAND" => {
            debug!("ws COMMAND de id={}: {}", user.id, args);
        }
        _ => {
            debug!("ws mensaje {} no procesado de id={}", ident, user.id);
        }
    }
    Ok(())
}

fn handle_ws_public(ctx: &AppContext, user: &Arc<AresUser>, text: &str) {
    if text.is_empty() {
        return;
    }
    let name = user.name.read().clone();
    let pkt = outbound::build_public(&name, text);
    broadcast_to_room(ctx, user, pkt);
}

fn handle_ws_emote(ctx: &AppContext, user: &Arc<AresUser>, text: &str) {
    if text.is_empty() {
        return;
    }
    let name = user.name.read().clone();
    let pkt = outbound::build_emote(&name, text);
    broadcast_to_room(ctx, user, pkt);
}

/// Broadcast a todos los usuarios en la misma vroom que `sender`.
/// Para usuarios WS, traduce el binario a texto usando `ws_text_sender`.
fn broadcast_to_room(ctx: &AppContext, sender: &AresUser, pkt: Bytes) {
    let vroom = sender.vroom;
    let users = ctx.user_pool.users();
    for u in users {
        if u.logged_in && u.vroom == vroom && !u.quarantined {
            if u.web_client {
                // WS user: traducir a texto y enviar por ws_text_sender
                if let Some(text) = translate_broadcast(&pkt, sender) {
                    if let Some(tx) = &u.ws_text_sender {
                        let _ = tx.send(text);
                    }
                }
                // No enviar por el canal binario (sería basura para el WS)
            } else {
                // TCP user: enviar binario
                let _ = u.send(pkt.clone());
            }
        }
    }
}
