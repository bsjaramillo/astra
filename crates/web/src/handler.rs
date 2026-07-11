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
    self, build_ack, build_public, build_server_info, build_topic_first, build_userinfo,
    build_userlist_end, build_userlist_item,
};
use crate::ws::{write_close_frame, write_text_frame, WsOpcode};
use crate::ws_outbound::translate_broadcast;

/// Maneja una conexión WebSocket después del handshake.
pub async fn handle_connection(
    ctx: Arc<AppContext>,
    stream: TcpStream,
    peer: SocketAddr,
    resolved_ip: std::net::IpAddr,
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
        ws_handshake_login(&ctx.clone(), &mut read_half, &mut buf, &tx, &ws_text_tx, peer, resolved_ip).await
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

    // Greet de bienvenida (PM del bot al usuario WS que entra)
    send_greet_ws(&ctx, &user, &ws_text_tx);

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

/// Tamaño máximo de un mensaje fragmentado reensamblado (1 MiB).
const MAX_FRAGMENTED_MSG: usize = 1 << 20;

/// Lee un mensaje WebSocket (cliente→servidor, masked).
///
/// Soporta mensajes fragmentados (RFC 6455 §5.4): los fragmentos se
/// acumulan y se retorna el mensaje completo con el opcode del primer
/// fragmento. Frames de control (Ping/Pong) recibidos en medio de una
/// fragmentación se consumen sin interrumpir el reensamblado.
async fn read_ws_frame(
    read_half: &mut OwnedReadHalf,
    buf: &mut BytesMut,
) -> anyhow::Result<Option<(WsOpcode, Vec<u8>)>> {
    // (opcode del primer fragmento, payload acumulado)
    let mut fragmented: Option<(WsOpcode, Vec<u8>)> = None;
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
        let fin = (b1 & 0x80) != 0;
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

        match opcode {
            WsOpcode::Continuation => {
                let Some((first_op, acc)) = fragmented.as_mut() else {
                    return Err(anyhow::anyhow!("continuation sin fragmento inicial"));
                };
                if acc.len() + payload.len() > MAX_FRAGMENTED_MSG {
                    return Err(anyhow::anyhow!("mensaje fragmentado demasiado grande"));
                }
                acc.extend_from_slice(&payload);
                if fin {
                    let op = *first_op;
                    let (_, acc) = fragmented.take().expect("fragmento en curso");
                    return Ok(Some((op, acc)));
                }
            }
            WsOpcode::Text | WsOpcode::Binary => {
                if fragmented.is_some() {
                    return Err(anyhow::anyhow!("fragmentación anidada no permitida"));
                }
                if fin {
                    return Ok(Some((opcode, payload)));
                }
                if payload.len() > MAX_FRAGMENTED_MSG {
                    return Err(anyhow::anyhow!("mensaje fragmentado demasiado grande"));
                }
                fragmented = Some((opcode, payload));
            }
            // Frames de control: no pueden fragmentarse (RFC 6455 §5.5)
            WsOpcode::Close | WsOpcode::Ping | WsOpcode::Pong => {
                if !fin {
                    return Err(anyhow::anyhow!("frame de control fragmentado"));
                }
                // En medio de una fragmentación, consumir Ping/Pong sin
                // perder el acumulador; Close siempre se entrega.
                if fragmented.is_none() || matches!(opcode, WsOpcode::Close) {
                    return Ok(Some((opcode, payload)));
                }
            }
        }
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
    resolved_ip: std::net::IpAddr,
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
            warn!("ws: primer frame no parseable de {}: {:?}", peer, text);
            return Ok(None);
        }
    };

    if !matches!(ident, "LOGIN" | "INBIZIO_LOGIN") {
        warn!("ws: primer frame no es login: ident={:?} de {}: {:?}", ident, peer, text);
        return Ok(None);
    }

    let login = match protocol::parse_login(args) {
        Some(l) => l,
        None => {
            warn!("ws: login malformado de {}: args={:?}", peer, args);
            return Ok(None);
        }
    };

    let external_ip = resolved_ip;
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
    // Nick duplicado: si la sesión existente es de la MISMA IP (ya resuelta
    // vía proxy trust si aplica), es una reconexión — hijack, paridad
    // sb0t. Si es otra IP, se rechaza como antes.
    if let Some(existing) = ctx.user_pool.get_by_name(&login.name) {
        if existing.external_ip == external_ip {
            info!(
                "hijack (misma IP): peer={} nick='{}' reemplaza sesión vieja id={}",
                peer, login.name, existing.id
            );
            astra_commands::force_part_user(ctx, &existing);
        } else {
            warn!("REJECTED (nick en uso): peer={} nick='{}'", peer, login.name);
            let _ = tx.send(Bytes::from_static(b"ERROR:Nickname already in use"));
            return Ok(None);
        }
    }

    let id = ctx.user_pool.next_id();
    let mut user = build_ares_user(id, external_ip, make_login_data(&login));
    user.sender = Some(tx.clone());
    user.ws_text_sender = Some(ws_text_tx.clone());
    user.logged_in = true;
    user.web_client = true;
    user.inbizier_web = login.inbizier_web;
    user.inbizier_mobile = login.inbizier_mobile;
    if !login.pmsg.is_empty() {
        *user.personal_message.lock() = login.pmsg.clone();
    }
    // Avatar del login inbizier (campo 7, base64 de la imagen completa).
    if !login.avatar_b64.is_empty() {
        use base64::Engine as _;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&login.avatar_b64) {
            *user.full_avatar.lock() = Some(bytes.clone());
            *user.avatar.lock() = Some(bytes);
        }
    }

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

/// Envía el estado inicial al cliente WS recién conectado, en el orden que
/// espera el cliente ib0t/inbizio (ver `WebProcessor.cs` de sb0t):
/// PUBLIC de bienvenida → ACK → SERVER_INFO → TOPIC_FIRST → userlist → USERLIST_END.
async fn send_initial_state_ws(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    tx: &mpsc::UnboundedSender<String>,
) {
    use server_core::ILevel;

    let room_name = ctx.settings.room_name.clone();
    let room_topic = ctx.current_room_topic();
    let bot_name = ctx.settings.bot_name.clone();
    let inbizier = user.inbizier_web || user.inbizier_mobile;

    // 1) Bienvenida como PUBLIC del server (nombre vacío).
    let _ = tx.send(build_public(
        "",
        &format!("{} — Astra {}", room_name, env!("CARGO_PKG_VERSION")),
    ));
    // 2) ACK con el nick asignado.
    let _ = tx.send(build_ack(&user.name.read()));
    // 3) SERVER_INFO para clientes inbizier.
    if inbizier {
        let _ = tx.send(build_server_info());
    }
    // 4) Topic inicial.
    let _ = tx.send(build_topic_first(&room_topic));

    // 5) Userlist: bot + usuarios logueados en la misma vroom (incluye self).
    let emit = |name: &str, pmsg: &str, avatar: &str, id: u16, level: u8, iw: bool, im: bool| {
        if inbizier {
            build_userinfo(name, pmsg, avatar, id, level, iw, im)
        } else {
            build_userlist_item(name, level)
        }
    };
    let bot_avatar_b64 = ctx.server_avatar.read().as_ref().map_or_else(String::new, |bytes| {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    });
    let _ = tx.send(emit(&bot_name, "", &bot_avatar_b64, 0, ILevel::Owner as u8, false, false));
    let vroom = *user.vroom.read();
    for other in ctx.user_pool.users() {
        if other.logged_in && *other.vroom.read() == vroom {
            let name = other.name.read().clone();
            let pmsg = other.personal_message.lock().clone();
            let avatar = avatar_b64_of(&other);
            let _ = tx.send(emit(
                &name,
                &pmsg,
                &avatar,
                other.id,
                *other.level.read() as u8,
                other.inbizier_web,
                other.inbizier_mobile,
            ));
        }
    }
    // 6) Fin de la userlist.
    let _ = tx.send(build_userlist_end());
}

/// Base64 del avatar de un usuario para USERINFO/JOININFO (sb0t manda el
/// FullAvatar a clientes inbizier; cae al avatar chico si no hay).
pub(crate) fn avatar_b64_of(user: &AresUser) -> String {
    use base64::Engine as _;
    let full = user.full_avatar.lock();
    if let Some(bytes) = full.as_ref() {
        return base64::engine::general_purpose::STANDARD.encode(bytes);
    }
    drop(full);
    let small = user.avatar.lock();
    match small.as_ref() {
        Some(bytes) => base64::engine::general_purpose::STANDARD.encode(bytes),
        None => String::new(),
    }
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
        "COMMAND" => handle_ws_command(ctx, user, args),
        "PM" => handle_ws_pm(ctx, user, args),
        "PERMSG" => handle_ws_permsg(ctx, user, args),
        "AVATAR" => handle_ws_avatar(ctx, user, args),
        "CUSTOM_DATA_HEAD" => handle_ws_custom_data_head(ctx, user, args, false),
        "CUSTOM_DATA_BODY" => handle_ws_custom_data_body(ctx, args, false),
        "PM_CUSTOM_DATA_HEAD" => handle_ws_custom_data_head(ctx, user, args, true),
        "PM_CUSTOM_DATA_BODY" => handle_ws_custom_data_body(ctx, args, true),
        _ => {
            debug!("ws mensaje {} no procesado de id={}", ident, user.id);
        }
    }
    Ok(())
}

/// Ejecuta un comando recibido por el ident `COMMAND` o por texto público
/// que empieza con `/` o `#`. Acepta el comando con o sin prefijo.
fn handle_ws_command(ctx: &AppContext, user: &Arc<AresUser>, raw: &str) {
    let raw = raw.trim().trim_start_matches(['/', '#']).trim();
    if raw.is_empty() {
        return;
    }
    let (cmd, cargs) = match raw.split_once(' ') {
        Some((c, a)) => (c, a.trim()),
        None => (raw, ""),
    };
    let (handled, _events) = astra_commands::dispatch_builtin(ctx, user, cmd, cargs);
    if !handled {
        let _ = user.print(
            &ctx.settings.bot_name,
            &format!("Unknown command: {}", cmd),
        );
    }
}

fn handle_ws_public(ctx: &AppContext, user: &Arc<AresUser>, text: &str) {
    if text.is_empty() {
        return;
    }
    // Comando: `/cmd` o `#cmd` (paridad WebProcessor.Text de sb0t).
    if text.starts_with('/') || text.starts_with('#') {
        handle_ws_command(ctx, user, text);
        return;
    }
    // Word filter: solo aplica a usuarios regulares (Moderator+ exentos).
    if (*user.level.read() as u8) < server_core::ILevel::Moderator as u8 {
        if let Some(action) = ctx.word_filter.check(text) {
            apply_filter_action_ws(ctx, user, action);
            return;
        }
    }
    let name = user.name.read().clone();
    let pkt = outbound::build_public(&name, text);
    broadcast_to_room(ctx, user, pkt);
    ctx.record_message(&name, text, false);
}

/// PM saliente de un usuario web: `PM:{nameLen},{textLen}:{target}{text}`.
/// Paridad `WebProcessor.PM` de sb0t (incluye `#cmd`//`/cmd` al bot = comando).
fn handle_ws_pm(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let Some(items) = protocol::parse_lens_args(args) else {
        return;
    };
    if items.len() < 2 {
        return;
    }
    let target_name = items[0].trim();
    let mut text = items[1].clone();
    if text.chars().count() > 300 {
        text = text.chars().take(300).collect();
    }
    if target_name.is_empty() || text.is_empty() {
        return;
    }

    // PM al bot: los comandos `/x` o `#x` se ejecutan (paridad sb0t).
    if target_name == ctx.settings.bot_name {
        if text.starts_with('/') || text.starts_with('#') {
            handle_ws_command(ctx, user, &text);
        }
        return;
    }

    let from = user.name.read().clone();
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        let _ = user.print(
            &ctx.settings.bot_name,
            &format!("User '{}' not found", target_name),
        );
        return;
    };
    // Respeta ignore list y /pmblock (regulares).
    let blocked = (target.pm_blocked.load(std::sync::atomic::Ordering::Relaxed)
        && (*user.level.read() as u8) < server_core::ILevel::Moderator as u8)
        || target
            .ignore_list
            .read()
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&from));
    if blocked {
        let _ = user.print(
            &ctx.settings.bot_name,
            &format!("{} is ignoring you.", target_name),
        );
        return;
    }
    let _ = target.send_pvt(&from, &text);
}

/// Cambio de personal message: `PERMSG:{len1},{len2}:{arg0}{texto}`.
/// Se guarda (máx 50 chars) y se difunde como PERSMSG a los inbizier y como
/// PersonalMessage binario a los clientes Ares (paridad sb0t).
fn handle_ws_permsg(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let Some(items) = protocol::parse_lens_args(args) else {
        return;
    };
    // sb0t toma arg_items[1]; si solo vino un campo, usamos ese.
    let mut text = items.last().cloned().unwrap_or_default();
    if text.chars().count() > 50 {
        text = text.chars().take(50).collect();
    }
    {
        let mut pmsg = user.personal_message.lock();
        if *pmsg == text {
            return;
        }
        *pmsg = text.clone();
    }
    let name = user.name.read().clone();
    let vroom = *user.vroom.read();
    let ws_msg = protocol::build_persmsg(&name, &text);
    for u in ctx.user_pool.users() {
        if !u.logged_in
            || *u.vroom.read() != vroom
            || u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            if u.inbizier_web || u.inbizier_mobile {
                let _ = tx.send(ws_msg.clone());
            }
        } else {
            let mut w = proto_ares::PacketWriter::with_msg_crypto(
                proto_ares::TcpMsg::PersonalMessage,
                u.ares_crypto,
            );
            w.write_string_nt(&name).ok();
            w.write_string_nt(&text).ok();
            let _ = u.send(Bytes::copy_from_slice(w.as_bytes()));
        }
    }
}

/// Avatar subido por un usuario web: `AVATAR:{len1},{len2}:{arg0}{base64}`.
/// Se guarda y se re-anuncia a los inbizier de la sala (USERINFO con el
/// avatar nuevo) para que se actualice en vivo.
fn handle_ws_avatar(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    use base64::Engine as _;
    if !ctx.room_flags.get("avatars") {
        return;
    }
    let Some(items) = protocol::parse_lens_args(args) else {
        return;
    };
    let b64 = items.last().cloned().unwrap_or_default();
    if b64.is_empty() || b64 == "/default.png" {
        return;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) else {
        return;
    };
    *user.full_avatar.lock() = Some(bytes.clone());
    *user.avatar.lock() = Some(bytes);
    user.avatar_received.store(true, std::sync::atomic::Ordering::Relaxed);

    // Re-anunciar al resto de los inbizier de la vroom con el avatar nuevo.
    let name = user.name.read().clone();
    let pmsg = user.personal_message.lock().clone();
    let avatar = avatar_b64_of(user);
    let info = protocol::build_userinfo(
        &name,
        &pmsg,
        &avatar,
        user.id,
        *user.level.read() as u8,
        user.inbizier_web,
        user.inbizier_mobile,
    );
    // También difundir a los clientes Ares nativos (paridad del setter
    // `AresClient.Avatar`, que manda `TCPOutbound.Avatar` a `UserPool.AUsers`
    // además de a `WUsers`) — sin esto, un cliente Ares nunca ve el avatar de
    // un usuario web.
    let raw_avatar = user.avatar.lock().clone();
    let vroom = *user.vroom.read();
    for u in ctx.user_pool.users() {
        if !u.logged_in
            || *u.vroom.read() != vroom
            || u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            if u.inbizier_web || u.inbizier_mobile {
                let _ = tx.send(info.clone());
            }
        } else if let Some(bytes) = &raw_avatar {
            let _ = u.send(outbound::build_avatar_c(&name, bytes, u.ares_crypto));
        }
    }
}

/// Tamaño máximo de chunk (paridad `preparePackets`/`Scribble2` del cliente
/// real y de sb0t: ambos cortan en base64 de 30000 chars).
const CUSTOM_DATA_CHUNK: usize = 30_000;
/// Altura fija que sb0t manda en el SCRIBBLE_HEAD público (no la usa este
/// cliente, pero se mantiene por paridad de wire).
const SCRIBBLE_HEIGHT: u16 = 300;

/// `CUSTOM_DATA_HEAD:{userLen},{idLen},{sizeLen}:{user}{id}{size}` (pública) o
/// `PM_CUSTOM_DATA_HEAD:{targetLen},{idLen},{sizeLen}:{target}{id}{size}`
/// (privada, el primer campo es el DESTINATARIO, no el emisor — paridad
/// `WebProcessor.PmCustomDataHead` de sb0t). Inicia el reensamblado; el
/// contenido real (imagen o audio) llega en los `CUSTOM_DATA_BODY` que siguen.
fn handle_ws_custom_data_head(ctx: &AppContext, user: &Arc<AresUser>, args: &str, is_pm: bool) {
    let Some(items) = protocol::parse_lens_args(args) else {
        return;
    };
    if items.len() < 3 {
        return;
    }
    let id = &items[1];
    let Ok(size) = items[2].parse::<u16>() else {
        return;
    };
    if size == 0 {
        return;
    }
    let sender = user.name.read().clone();
    let vroom = *user.vroom.read();
    let target = if is_pm { Some(items[0].clone()) } else { None };
    let store = if is_pm { &ctx.pm_custom_data } else { &ctx.custom_data };
    store.start(id, sender, target, vroom, size);
}

/// `CUSTOM_DATA_BODY:{typeLen},{idLen},{dataLen}:{type}{id}{data}` (o el
/// equivalente `PM_`). `type` es `"SCRIBBLE"` (imagen) o `"AUDIO"`. Cuando se
/// recibe el último chunk, entrega la data completa para difundir.
fn handle_ws_custom_data_body(ctx: &AppContext, args: &str, is_pm: bool) {
    let Some(items) = protocol::parse_lens_args(args) else {
        return;
    };
    if items.len() < 3 {
        return;
    }
    let kind = items[0].as_str();
    let id = &items[1];
    let data = &items[2];
    let store = if is_pm { &ctx.pm_custom_data } else { &ctx.custom_data };
    let Some((sender, target, vroom, full_data)) = store.append(id, data) else {
        return;
    };
    match (kind, target) {
        ("SCRIBBLE", None) => deliver_public_scribble(ctx, &sender, vroom, &full_data),
        ("AUDIO", None) => deliver_public_audio(ctx, &sender, vroom, &full_data),
        ("SCRIBBLE", Some(target)) => deliver_pm_scribble(ctx, &sender, &target, &full_data),
        ("AUDIO", Some(target)) => deliver_pm_audio(ctx, &sender, &target, &full_data),
        _ => {}
    }
}

/// Corta `data` en chunks de a lo sumo `CUSTOM_DATA_CHUNK` chars (mismo
/// tamaño que usa el cliente al armar los BODY, así que en la práctica suele
/// dar un solo chunk de vuelta salvo que la imagen/audio sea grande).
fn chunk_data(data: &str) -> Vec<&str> {
    if data.is_empty() {
        return Vec::new();
    }
    let bytes = data.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let end = (pos + CUSTOM_DATA_CHUNK).min(bytes.len());
        out.push(&data[pos..end]);
        pos = end;
    }
    out
}

/// Difunde una imagen (scribble) reensamblada a todos los clientes web de la
/// vroom, sin filtrar por inbizier (paridad `Scribble2` de sb0t: cualquier
/// cliente web renderiza imágenes igual).
fn deliver_public_scribble(ctx: &AppContext, sender: &str, vroom: u16, data: &str) {
    if !ctx.room_flags.get("scribbles") {
        return;
    }
    let chunks = chunk_data(data);
    let head = protocol::build_scribble_head(sender, chunks.len(), SCRIBBLE_HEIGHT);
    for u in ctx.user_pool.users() {
        if !u.logged_in
            || *u.vroom.read() != vroom
            || u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            let _ = tx.send(head.clone());
            for c in &chunks {
                let _ = tx.send(protocol::build_scribble_block(c));
            }
        }
    }
}

/// Difunde un audio reensamblado a los clientes web inbizier de la vroom
/// (paridad `Audio` de sb0t: solo los clientes inbizio pueden reproducirlo).
fn deliver_public_audio(ctx: &AppContext, sender: &str, vroom: u16, data: &str) {
    if !ctx.room_flags.get("audios") {
        return;
    }
    let chunks = chunk_data(data);
    let head = protocol::build_audio_head(sender, chunks.len());
    for u in ctx.user_pool.users() {
        if !u.logged_in
            || *u.vroom.read() != vroom
            || u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
            || !(u.inbizier_web || u.inbizier_mobile)
        {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            let _ = tx.send(head.clone());
            for c in &chunks {
                let _ = tx.send(protocol::build_audio_block(c));
            }
        }
    }
}

/// Manda una imagen (scribble) por PM a un usuario web inbizier concreto
/// (paridad `PmScribble` de sb0t: ignore list y target no-inbizier bloquean).
fn deliver_pm_scribble(ctx: &AppContext, sender: &str, target_name: &str, data: &str) {
    if !ctx.room_flags.get("scribbles") {
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        return;
    };
    if !target.logged_in || !(target.inbizier_web || target.inbizier_mobile) {
        return;
    }
    if target.ignore_list.read().iter().any(|e| e.eq_ignore_ascii_case(sender)) {
        return;
    }
    let Some(tx) = &target.ws_text_sender else {
        return;
    };
    let chunks = chunk_data(data);
    let _ = tx.send(protocol::build_pm_scribble_head(sender, chunks.len()));
    for c in &chunks {
        let _ = tx.send(protocol::build_pm_scribble_block(sender, c));
    }
}

/// Manda un audio por PM a un usuario web inbizier concreto.
fn deliver_pm_audio(ctx: &AppContext, sender: &str, target_name: &str, data: &str) {
    if !ctx.room_flags.get("audios") {
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        return;
    };
    if !target.logged_in || !(target.inbizier_web || target.inbizier_mobile) {
        return;
    }
    if target.ignore_list.read().iter().any(|e| e.eq_ignore_ascii_case(sender)) {
        return;
    }
    let Some(tx) = &target.ws_text_sender else {
        return;
    };
    let chunks = chunk_data(data);
    let _ = tx.send(protocol::build_pm_audio_head(sender, chunks.len()));
    for c in &chunks {
        let _ = tx.send(protocol::build_pm_audio_block(sender, c));
    }
}

/// Envía el greet de bienvenida como PM del bot al usuario WS que entra.
fn send_greet_ws(
    ctx: &AppContext,
    user: &AresUser,
    ws_text_tx: &mpsc::UnboundedSender<String>,
) {
    let Some(template) = ctx.greets.next_template() else {
        return;
    };
    let gctx = server_core::GreetContext {
        name: &user.name.read(),
        ip: &user.external_ip.to_string(),
        id: user.id,
        file_count: user.file_count,
        version: &user.version,
        user_count: ctx.user_pool.len(),
        room_name: &ctx.settings.room_name,
        uptime_secs: ctx.uptime_secs(),
        region: &user.region,
    };
    let text = server_core::greets::render_greet(&template, &gctx);
    let _ = ws_text_tx.send(crate::protocol::build_pm(&ctx.settings.bot_name, &text));
}

/// Aplica la acción de un word filter a un usuario WS.
fn apply_filter_action_ws(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    action: server_core::FilterAction,
) {
    use server_core::FilterAction;
    if let Some(tx) = &user.ws_text_sender {
        let _ = tx.send(crate::protocol::build_pm(
            &ctx.settings.bot_name,
            "Your message was blocked by a word filter.",
        ));
    }
    match action {
        FilterAction::Block => {}
        FilterAction::Kick => filter_remove_user_ws(ctx, user),
        FilterAction::Ban => {
            let _ = ctx.bans.ban(
                &user.name.read(),
                &user.version,
                &user.guid,
                user.external_ip,
                user.local_ip,
                user.data_port,
            );
            filter_remove_user_ws(ctx, user);
        }
    }
}

fn filter_remove_user_ws(ctx: &AppContext, user: &Arc<AresUser>) {
    let part_pkt = outbound::build_part(user);
    ctx.user_pool.remove(user.id);
    ctx.stats.on_user_part();
    for u in ctx.user_pool.users() {
        if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = u.send(part_pkt.clone());
        }
    }
}

fn handle_ws_emote(ctx: &AppContext, user: &Arc<AresUser>, text: &str) {
    if text.is_empty() {
        return;
    }
    let name = user.name.read().clone();
    let pkt = outbound::build_emote(&name, text);
    broadcast_to_room(ctx, user, pkt);
    ctx.record_message(&name, text, true);
}

/// Broadcast a todos los usuarios en la misma vroom que `sender`.
/// Para usuarios WS, traduce el binario a texto usando `ws_text_sender`.
fn broadcast_to_room(ctx: &AppContext, sender: &AresUser, pkt: Bytes) {
    let vroom = *sender.vroom.read();
    let users = ctx.user_pool.users();
    for u in users {
        if u.logged_in && *u.vroom.read() == vroom && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            if u.web_client {
                // WS user: traducir a texto y enviar por ws_text_sender
                if let Some(text) = translate_broadcast(&pkt, sender, &u) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::tcp::OwnedWriteHalf;
    use tokio::net::{TcpListener, TcpStream};

    /// Construye un frame WS cliente→servidor (masked con key 0 = no-op).
    fn frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(payload.len() + 8);
        v.push(if fin { 0x80 } else { 0x00 } | opcode);
        let len = payload.len();
        if len < 126 {
            v.push(0x80 | len as u8);
        } else {
            v.push(0x80 | 126);
            v.extend((len as u16).to_be_bytes());
        }
        v.extend([0, 0, 0, 0]); // mask key = 0 → XOR no-op
        v.extend(payload);
        v
    }

    async fn tcp_pair() -> (OwnedReadHalf, OwnedWriteHalf, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (read_half, write_half) = server.into_split();
        (read_half, write_half, client)
    }

    #[tokio::test]
    async fn fragmented_text_is_reassembled() {
        let (mut read_half, _wh, mut client) = tcp_pair().await;
        client.write_all(&frame(false, 0x1, b"Hel")).await.unwrap();
        client.write_all(&frame(false, 0x0, b"lo ")).await.unwrap();
        client.write_all(&frame(true, 0x0, b"mundo")).await.unwrap();

        let mut buf = BytesMut::new();
        let (op, payload) = read_ws_frame(&mut read_half, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(op, WsOpcode::Text));
        assert_eq!(payload, b"Hello mundo");
    }

    #[tokio::test]
    async fn ping_between_fragments_is_consumed() {
        let (mut read_half, _wh, mut client) = tcp_pair().await;
        client.write_all(&frame(false, 0x1, b"a")).await.unwrap();
        client.write_all(&frame(true, 0x9, b"")).await.unwrap(); // ping
        client.write_all(&frame(true, 0x0, b"b")).await.unwrap();

        let mut buf = BytesMut::new();
        let (op, payload) = read_ws_frame(&mut read_half, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(op, WsOpcode::Text));
        assert_eq!(payload, b"ab");
    }

    #[tokio::test]
    async fn unfragmented_frame_still_works() {
        let (mut read_half, _wh, mut client) = tcp_pair().await;
        client.write_all(&frame(true, 0x1, b"hola")).await.unwrap();

        let mut buf = BytesMut::new();
        let (op, payload) = read_ws_frame(&mut read_half, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(op, WsOpcode::Text));
        assert_eq!(payload, b"hola");
    }

    #[tokio::test]
    async fn continuation_without_start_is_error() {
        let (mut read_half, _wh, mut client) = tcp_pair().await;
        client.write_all(&frame(true, 0x0, b"x")).await.unwrap();

        let mut buf = BytesMut::new();
        let result = read_ws_frame(&mut read_half, &mut buf).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn nested_fragmentation_is_error() {
        let (mut read_half, _wh, mut client) = tcp_pair().await;
        client.write_all(&frame(false, 0x1, b"a")).await.unwrap();
        client.write_all(&frame(false, 0x1, b"b")).await.unwrap();

        let mut buf = BytesMut::new();
        let result = read_ws_frame(&mut read_half, &mut buf).await;
        assert!(result.is_err());
    }
}
