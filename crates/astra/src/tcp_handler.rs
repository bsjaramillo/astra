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
    scripting.dispatch(astra_scripting::ScriptEvent::Connect {
        ip: peer.ip().to_string(),
    });

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
        process_handshake(&ctx, &mut reader, peer, tx.clone(), &scripting).await
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
    send_initial_state(&ctx, &user, &scripting).await;

    // Broadcast del JOIN a los usuarios existentes
    let join_pkt = outbound::build_join_or_userlist(&user);
    broadcast_to_room(&ctx, &user, join_pkt);
    ctx.publish_link_event(LinkEvent::Join {
        origin: None,
        user: LinkUserSnapshot::from_user(&user),
    });
    // Disparar evento de scripting
    scripting.dispatch(astra_scripting::ScriptEvent::Join {
        name: user.name.read().clone(),
        ip: user.external_ip.to_string(),
    });

    // Greet de bienvenida (PM del bot al usuario que entra)
    send_greet(&ctx, &user);

    // Feeds de admin: ipsend (IP del que entra) y logsend (log de join).
    {
        let jname = user.name.read().clone();
        let ipsend_line = format!(
            "IPSEND: {} {} {} {}",
            jname, user.external_ip, user.local_ip, user.data_port
        );
        let logsend_line = format!("LOG: join {} [{}]", jname, user.external_ip);
        let self_id = user.id;
        ctx.notify_subscribers(&ipsend_line, |u| {
            u.id != self_id && u.sub_ipsend.load(std::sync::atomic::Ordering::Relaxed)
        });
        ctx.notify_subscribers(&logsend_line, |u| {
            u.id != self_id && u.sub_logsend.load(std::sync::atomic::Ordering::Relaxed)
        });
    }

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
        // Touch idle: registrar actividad del user
        let was_idle = ctx.idle.touch(user_arc.id).is_some();
        if was_idle {
            let name = user_arc.name.read().clone();
            scripting.dispatch(astra_scripting::ScriptEvent::Unidled { name });
        }

        if let Err(e) = dispatch_message(&ctx, &scripting, &user_arc, &pkt).await {
            warn!("error procesando msg {:?} de id={}: {}", pkt.msg, user_id, e);
        }
    }

    // ============================================================
    // Cleanup
    // ============================================================
    let user_name = user_arc.name.read().clone();
    ctx.user_pool.remove(user_id);
    ctx.stats.on_user_part();
    // Forget idle tracking
    ctx.idle.forget(user_id);

    // Broadcast del PART
    let part_pkt = outbound::build_part(&user_arc);
    broadcast_to_room(&ctx, &user_arc, part_pkt);
    ctx.publish_link_event(LinkEvent::Part {
        origin: None,
        name: user_name.clone(),
    });
    // Disparar evento de scripting
    scripting.dispatch(astra_scripting::ScriptEvent::Part {
        name: user_name.clone(),
    });
    scripting.dispatch(astra_scripting::ScriptEvent::Logout { name: user_name });

    // Cerrar el canal para que el writer termine
    drop(tx);
    let _ = writer_handle.await;

    info!("usuario id={} '{}' desconectado", user_id, user.name.read());
    scripting.dispatch(astra_scripting::ScriptEvent::Disconnect {
        ip: user.external_ip.to_string(),
    });
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
    scripting: &astra_scripting::ScriptHandle,
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
                    let (validation_result, issues) = ctx.security.login_validator.validate(&login);
                    // Disparar eventos de scripting para issues detectados (no rechazos)
                    for issue in &issues {
                        match issue {
                            server_core::security::DetectedIssue::Proxy => {
                                scripting.dispatch(astra_scripting::ScriptEvent::ProxyDetected {
                                    ip: peer.ip().to_string(),
                                });
                            }
                        }
                    }
                    if let Err(reason) = validation_result {
                        warn!("REJECTED (capa 4): peer={} nick='{}' razón={:?}", peer, login.org_name, reason);
                        ctx.security.failed_logins.record_failure(peer.ip());
                        let _ = tx.send(server_error_packet(reason.message()));
                        // Disparar evento de scripting
                        scripting.dispatch(astra_scripting::ScriptEvent::InvalidLoginAttempt {
                            name: login.org_name.clone(),
                            ip: peer.ip().to_string(),
                        });
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

                    // Range ban (prefijo de IP)
                    if ctx.range_bans.is_banned(external_ip) {
                        warn!("REJECTED (range ban): peer={}", peer);
                        let _ = tx.send(server_error_packet("You are banned from this room"));
                        return Ok(None);
                    }

                    // Join filter (patrón de nick)
                    if ctx.join_filters.matches(&login.org_name) {
                        warn!("REJECTED (join filter): peer={} name='{}'", peer, login.org_name);
                        let _ = tx.send(server_error_packet("Your nickname is not allowed here"));
                        return Ok(None);
                    }

                    // ASN ban (requiere base GeoIP-ASN cargada; si no, no-op)
                    if !ctx.asn_bans.is_empty() {
                        if let Some(asn) = ctx.geoip.lookup_asn(external_ip) {
                            if ctx.asn_bans.is_banned(asn) {
                                warn!("REJECTED (ASN ban {}): peer={}", asn, peer);
                                let _ = tx.send(server_error_packet("You are banned from this room"));
                                return Ok(None);
                            }
                        }
                    }

                    // Join-flood
                    if ctx.user_history.is_join_flooding(external_ip, now_ms) {
                        warn!("REJECTED (join-flood): peer={}", peer);
                        let _ = tx.send(server_error_packet("Joining too quickly. Please wait 15 seconds."));
                        // Disparar evento de scripting
                        scripting.dispatch(astra_scripting::ScriptEvent::Flood {
                            name: login.org_name.clone(),
                        });
                        return Ok(None);
                    }

                    // Construir usuario
                    let id = ctx.user_pool.next_id();
                    let mut user = server_core::login::build_ares_user(id, external_ip, login);
                    user.sender = Some(tx.clone());
                    user.logged_in = true;  // marcar ANTES de envolver en Arc
                    let user_arc = Arc::new(user);

                    // Captcha gate (chequear ANTES de add_user, para que la
                    // primera conexión de una IP sea considerada "nueva").
                    let needs_captcha_now = ctx.settings.security.captcha_enabled
                        && !ctx.user_history.has_prior_join(external_ip);

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

                    // Disparar evento LoginGranted al scripting
                    scripting.dispatch(astra_scripting::ScriptEvent::LoginGranted {
                        name: user_arc.name.read().clone(),
                    });

                    if needs_captcha_now {
                        let user_id = user_arc.id.to_string();
                        let challenge = ctx.captcha.create(user_id.clone());
                        user_arc.needs_captcha.store(true, std::sync::atomic::Ordering::Relaxed);
                        user_arc.quarantined.store(true, std::sync::atomic::Ordering::Relaxed);
                        let visual = obfuscate_captcha_word(&challenge.word);
                        let prompt = format!(
                            "Welcome! Please type this code to enter: {}  (PM it back to {})",
                            visual, ctx.settings.bot_name
                        );
                        let pkt = outbound::build_pvt(&ctx.settings.bot_name, &prompt);
                        let _ = user_arc.send(pkt);
                        info!(
                            "CAPTCHA issued: id={} ip={} word={}",
                            user_arc.id, peer.ip(), challenge.word
                        );
                    }

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
async fn send_initial_state(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    scripting: &astra_scripting::ScriptHandle,
) {
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
            // Evento de scripting por cada user en la userlist
            scripting.dispatch(astra_scripting::ScriptEvent::UserList {
                name: other.name.read().clone(),
                users_csv: String::new(),
            });
        }
    }

    // End of userlist
    let _ = user.send(outbound::build_userlist_end());
    scripting.dispatch(astra_scripting::ScriptEvent::UserListEnd {
        name: user.name.read().clone(),
    });

    // OpChange (este usuario)
    let level = *user.level.read();
    let is_op = level as u8 >= server_core::ILevel::Moderator as u8;
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
            // Gate de sala: si los avatares están deshabilitados, se ignora.
            if !ctx.room_flags.get("avatars") {
                return Ok(());
            }
            publish_raw_link(ctx, LINK_MSG_AVATAR, &pkt.data[1..]);
            // Avatar: el payload completo (sin opcode) son los bytes PNG.
            // Guardar en user.avatar (set real, no solo notificar).
            let png = pkt.data[1..].to_vec();
            *user.avatar.lock() = Some(png.clone());
            scripting.dispatch(astra_scripting::ScriptEvent::Avatar {
                name: user.name.read().clone(),
                png,
            });
        }
        TcpMsg::Public => {
            handle_public(ctx, user, &pkt.data[1..], &scripting).await;
        }
        TcpMsg::Emote => {
            handle_emote(ctx, user, &pkt.data[1..], scripting).await;
        }
        TcpMsg::Pmt => {
            handle_pvt(ctx, user, &pkt.data[1..], scripting).await;
        }
        TcpMsg::PersonalMessage => {
            handle_personal_message(ctx, user, &pkt.data[1..]).await;
        }
        TcpMsg::ClientBrowse => {
            publish_raw_link(ctx, LINK_MSG_BROWSE, &pkt.data[1..]);
            // El payload de ClientBrowse es un string Ares (null-terminated) con
            // un hashlink del archivo que se está compartiendo.
            let mut r = PacketReader::new(&pkt.data[1..]);
            if let Ok(hashlink) = r.read_string() {
                scripting.dispatch(astra_scripting::ScriptEvent::FileReceived {
                    name: user.name.read().clone(),
                    filename: hashlink,
                });
            }
        }
        TcpMsg::CustomData => {
            publish_raw_link(ctx, LINK_MSG_CUSTOM_DATA_TO, &pkt.data[1..]);
        }
        TcpMsg::CustomDataAll => {
            publish_raw_link(ctx, LINK_MSG_CUSTOM_DATA_ALL, &pkt.data[1..]);
        }
        TcpMsg::ClientScribbleRoomFirst => {
            // Gate de sala: si los scribbles están deshabilitados, se descarta.
            if !ctx.room_flags.get("scribbles") {
                debug!("scribble de '{}' bloqueado (flag de sala)", user.name.read());
                return Ok(());
            }
            // Gate real: el script puede cancelar el scribble retornando false
            // desde onScribbleCheck. Si cancela, no se reenvía al link ni a la sala.
            let allow = scripting.check_scribble(&user.name.read(), false);
            if allow {
                publish_raw_link(ctx, LINK_MSG_SCRIBBLE_LEAF, &pkt.data[1..]);
            } else {
                tracing::info!("scribble de '{}' bloqueado por scripting", user.name.read());
            }
        }
        TcpMsg::ClientScribbleRoomChunk => {
            if !ctx.room_flags.get("scribbles") {
                return Ok(());
            }
            // Chunks siempre se reenvían (asumiendo que First pasó el gate).
            // Para gate por-chunk, se necesitaría trackear el state de cada scribble.
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

    // Si tiene captcha pendiente, no puede hablar en público.
    // (Sí puede ejecutar /help y otros built-ins, así que permitimos esos.)
    if user.needs_captcha.load(std::sync::atomic::Ordering::Relaxed) && !text.trim_start().starts_with('/') {
        let pkt = outbound::build_pvt(
            &ctx.settings.bot_name,
            "Please solve the captcha before chatting. Check your PMs.",
        );
        let _ = user.send(pkt);
        return;
    }

    let name = user.name.read().clone();

    // Si el mensaje empieza con '/', es un comando slash
    if let Some((cmd, args)) = astra_commands::parse_command(&text) {
        let (handled, events) = astra_commands::dispatch_builtin(ctx, user, cmd, args);
        if handled {
            debug!("comando built-in de '{}': /{} {}", name, cmd, args);
            // Disparar los side-effects de scripting que el comando generó
            for ev in events {
                scripting.dispatch(ev);
            }
            // Post-dispatch hooks: disparan eventos de scripting
            // que commands no puede emitir (no depende de scripting).
            if cmd.eq_ignore_ascii_case("vroom") {
                if let Ok(new_vroom) = args.trim().parse::<u16>() {
                    scripting.dispatch(astra_scripting::ScriptEvent::VroomJoin {
                        name: name.clone(),
                        vroom: new_vroom,
                    });
                }
            }
            return;
        }
        astra_commands::dispatch(ctx, scripting, &name, cmd, args);
        debug!("comando slash de '{}': /{} {}", name, cmd, args);
        return;
    }

    // Muzzled: puede ejecutar comandos pero no hablar en público.
    // (is_muzzled auto-expira los muzzles temporales de /mtimeout)
    if user.is_muzzled() {
        let pkt = outbound::build_pvt(
            &ctx.settings.bot_name,
            "You are muzzled and cannot chat in public.",
        );
        let _ = user.send(pkt);
        return;
    }

    // Word filter: solo aplica a usuarios regulares (Moderator+ exentos).
    if (*user.level.read() as u8) < server_core::ILevel::Moderator as u8 {
        if let Some(action) = ctx.word_filter.check(&text) {
            apply_filter_action(ctx, user, action, &name);
            return;
        }
    }

    // Echo heckle (/echo): reenvía el texto configurado solo a este usuario.
    if let Some(echo) = user.echo_text.read().clone() {
        let _ = user.send(outbound::build_pvt(&ctx.settings.bot_name, &echo));
    }

    // Efectos de castigo por-usuario (/kiddy, /lower).
    let mut text = server_core::text_effects::apply_punish_effects(user, &text);

    // Caps monitoring de sala: los mensajes TODO-EN-MAYÚSCULAS se bajan a
    // minúsculas (paridad sb0t CapsMonitoring).
    if ctx.room_flags.get("caps") && server_core::text_effects::is_shouting(&text) {
        text = text.to_lowercase();
    }

    // Hook onTextBefore: si algún script retorna false, cancelar el broadcast
    if !scripting.check_text_before(&name, &text) {
        debug!("onTextBefore canceló mensaje de '{}'", name);
        return;
    }

    let pkt = outbound::build_public(&name, &text);
    broadcast_to_room(ctx, user, pkt);
    ctx.record_message(&name, &text, false);
    vspy_copy(ctx, user, &name, &text);
    ctx.publish_link_event(LinkEvent::Public {
        origin: None,
        from: name.clone(),
        text: text.clone(),
    });
    // Disparar evento de scripting
    scripting.dispatch(astra_scripting::ScriptEvent::Public {
        from: name.clone(),
        text: text.clone(),
    });
    debug!("public de '{}': {}", name, text);
}

/// Emite una variante visual del captcha: cada letra tiene 50% de
/// probabilidad de estar en mayúscula o minúscula, y se añaden 1-2
/// caracteres "0Oo1l" como ruido para OCRs simples.
fn obfuscate_captcha_word(word: &str) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut out = String::with_capacity(word.len() + 3);
    for c in word.chars() {
        let mapped = if rng.gen_bool(0.5) { c.to_ascii_lowercase() } else { c };
        out.push(mapped);
    }
    // Añade 0-2 caracteres de ruido
    let noise_count = rng.gen_range(0..=2);
    let noise_chars = ['0', 'o', '1', 'l', 'I'];
    for _ in 0..noise_count {
        let nc = noise_chars[rng.gen_range(0..noise_chars.len())];
        let pos = rng.gen_range(0..=out.len());
        out.insert(pos, nc);
    }
    out
}

/// Maneja MSG_CHAT_CLIENT_EMOTE (11).
async fn handle_emote(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    data: &[u8],
    scripting: &astra_scripting::ScriptHandle,
) {
    let mut r = PacketReader::new(data);
    let text = match r.read_string() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }
    // Muzzled: sin voz en público (aplica también a emotes).
    if user.is_muzzled() {
        let pkt = outbound::build_pvt(
            &ctx.settings.bot_name,
            "You are muzzled and cannot chat in public.",
        );
        let _ = user.send(pkt);
        return;
    }

    let name = user.name.read().clone();
    // Hook onEmoteBefore
    if !scripting.check_emote_before(&name, &text) {
        debug!("onEmoteBefore canceló emote de '{}'", name);
        return;
    }
    let pkt = outbound::build_emote(&name, &text);
    broadcast_to_room(ctx, user, pkt);
    ctx.record_message(&name, &text, true);
    ctx.publish_link_event(LinkEvent::Emote {
        origin: None,
        from: name.clone(),
        text: text.clone(),
    });
    scripting.dispatch(astra_scripting::ScriptEvent::Emote {
        from: name,
        text,
    });
}

/// Maneja MSG_CHAT_CLIENT_PVT (25).
/// Formato: `str target_name, str text`
async fn handle_pvt(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    data: &[u8],
    scripting: &astra_scripting::ScriptHandle,
) {
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
        // Hook onPMBefore: si algún script retorna false, cancelar el PM
        if !scripting.check_pm_before(&from, &target_name, &text) {
            debug!("onPMBefore canceló PM de '{}' a '{}'", from, target_name);
            return;
        }
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
        if u.logged_in && *u.vroom.read() == vroom && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            // En el sb0t original el sender también recibe el broadcast
            // (excepto en algunos casos). Aquí también.
            let _ = u.send(pkt.clone());
            let _ = sender_id; // unused but indicates intent
        }
    }
}

/// Envía una copia de un mensaje público/emote a los admins suscritos a
/// `/vspy` que estén en un vroom DISTINTO al del emisor (monitoreo
/// cross-vroom, paridad sb0t VSpy).
fn vspy_copy(ctx: &AppContext, sender: &server_core::user_pool::AresUser, name: &str, text: &str) {
    let sender_vroom = *sender.vroom.read();
    let line = format!("[vroom {}] {}: {}", sender_vroom, name, text);
    let pkt = outbound::build_pvt(&ctx.settings.bot_name, &line);
    for u in ctx.user_pool.users() {
        if u.logged_in
            && u.sub_vspy.load(std::sync::atomic::Ordering::Relaxed)
            && *u.vroom.read() != sender_vroom
            && (*u.level.read() as u8) >= server_core::ILevel::Moderator as u8
        {
            let _ = u.send(pkt.clone());
        }
    }
}

/// Envía el greet de bienvenida (rotado) como PM del bot al usuario que
/// acaba de entrar, con los placeholders sustituidos. No-op si los greets
/// están deshabilitados o no hay ninguno configurado.
fn send_greet(ctx: &AppContext, user: &server_core::user_pool::AresUser) {
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
    let _ = user.send(outbound::build_pvt(&ctx.settings.bot_name, &text));
}

/// Aplica la acción de un word filter a un mensaje bloqueado: notifica al
/// emisor y, según la acción, lo expulsa (remueve del pool) o lo banea.
fn apply_filter_action(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    action: server_core::FilterAction,
    name: &str,
) {
    use server_core::FilterAction;
    // Aviso al emisor de que su mensaje fue bloqueado.
    let _ = user.send(outbound::build_pvt(
        &ctx.settings.bot_name,
        "Your message was blocked by a word filter.",
    ));

    match action {
        FilterAction::Block => {
            debug!("word filter bloqueó mensaje de '{}'", name);
        }
        FilterAction::Kick => {
            info!("word filter: kick de '{}'", name);
            filter_remove_user(ctx, user);
        }
        FilterAction::Ban => {
            info!("word filter: ban de '{}'", name);
            let _ = ctx.bans.ban(
                &user.name.read(),
                &user.version,
                &user.guid,
                user.external_ip,
                user.local_ip,
                user.data_port,
            );
            filter_remove_user(ctx, user);
        }
    }
}

/// Remueve un usuario del pool y difunde su PART (mismo patrón que `/kick`).
fn filter_remove_user(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>) {
    let part_pkt = outbound::build_part(user);
    ctx.user_pool.remove(user.id);
    ctx.stats.on_user_part();
    for u in ctx.user_pool.users() {
        if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = u.send(part_pkt.clone());
        }
    }
    ctx.publish_link_event(LinkEvent::Part {
        origin: None,
        name: user.name.read().clone(),
    });
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
