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
use server_core::flood_control::FloodKind;
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
    // CAPA 1+2+5: rate-limit, concurrent limit (nativo), failed-login ban.
    // (El cap anti-Slowloris de conexiones crudas por IP es aparte y ya se
    //  aplicó antes del peek, en `handle_muxed_connection`.)
    // ============================================================
    if let Some(reason) = ctx.security.check_new_connection(ip) {
        warn!("REJECTED (capa 1/2/5): peer={} razón={:?}", peer, reason);
        let _ = send_server_error_to_stream(stream, reason.message()).await;
        return Ok(());
    }

    // Asegurar release del slot de la CAPA 2 al salir.
    let security = ctx.security.clone();
    let _guard = scopeguard::guard(ip, move |ip| {
        security.on_disconnect(ip);
    });

    // Split del socket (owned, para que el writer task pueda ser 'static)
    let (read_half, write_half) = stream.into_split();

    // Canal mpsc para enviar al cliente
    let (tx, rx) = mpsc::unbounded_channel::<Bytes>();

    // Señal de "el writer detectó un error de escritura": el socket está
    // muerto (peer desapareció sin FIN/RST, o el write directamente falló).
    // El loop de lectura la corre en `select!` junto al read normal, para no
    // depender únicamente de que el lado de lectura también falle (que en un
    // half-dead connection puede tardar mucho más, hasta que el SO agote sus
    // propios reintentos).
    let (write_failed_tx, mut write_failed_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn writer task
    let mut writer_handle = tokio::spawn(writer_task(write_half, rx, write_failed_tx));

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

    // Gate de scripts (onJoinCheck, paridad sb0t `Joining`): un script puede
    // rechazar el join ANTES de anunciar al usuario a la sala.
    {
        let jname = user.name.read().clone();
        let jip = user.external_ip.to_string();
        if !scripting.check_join(&jname, &jip) {
            info!("join de '{}' rechazado por script (onJoinCheck)", jname);
            let mut w = proto_ares::PacketWriter::with_msg_crypto(
                proto_ares::TcpMsg::ServerError,
                user.ares_crypto,
            );
            w.write_string_nt("You have been rejected from this room.").ok();
            let _ = user.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
            scripting.dispatch(astra_scripting::ScriptEvent::Rejected {
                name: jname,
                ip: jip,
                reason: "script".to_string(),
            });
            ctx.user_pool.remove(user_id);
            ctx.stats.on_user_part();
            return Ok(());
        }
    }

    // ============================================================
    // Post-login: enviar JOIN a los demás y USERLIST al nuevo
    // ============================================================
    // Autologin ANTES del estado inicial (paridad sb0t `Joined()`: corre
    // `AutoLogin.GetLevel` para todos los que entran). DEBE correr antes de
    // `send_initial_state` y del broadcast del JOIN, para que el nivel ya
    // esté aplicado cuando se manda el USERLIST/JOIN — si no, los clientes
    // ven al usuario como Regular y nunca reciben su nivel real (el path web
    // ya lo hacía en este orden; el TCP nativo lo aplicaba DESPUÉS y por eso
    // el nivel del autologin no llegaba en el paquete de entrada).
    let _ = astra_commands::dispatch_autologin(&ctx, &user);

    send_initial_state(&ctx, &user, &scripting).await;

    // Broadcast del JOIN a los usuarios existentes (ya con el nivel aplicado).
    broadcast_to_room(&ctx, &user, |c| outbound::build_join_or_userlist_c(&user, c));
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
    // MOTD (message of the day), tras el greet.
    send_motd(&ctx, &user);
    // Replay del historial de chat si el flag `history` está activo
    // (paridad sb0t ServerEvents.cs:186 → History.Show al entrar).
    ctx.replay_history(&user);
    // Anuncio "was last seen as..." si el flag `lastseen` está activo.
    ctx.announce_last_seen(&user);

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
    // `idle_timeout` es una red de seguridad (conexiones realmente colgadas),
    // no el mecanismo principal de detección: para eso está el FastPing
    // periódico (main.rs) + `write_failed_rx`, que corta de inmediato en
    // cuanto el writer nota que el socket está muerto, sin esperar a que el
    // lado de lectura también lo note (podría tardar mucho más).
    let idle_timeout = Duration::from_secs(ctx.settings.security.idle_timeout_secs);
    loop {
        let pkt = tokio::select! {
            biased;
            _ = &mut write_failed_rx => {
                debug!("writer detectó conexión rota para id={}, cerrando lector", user_id);
                break;
            }
            read_result = timeout(idle_timeout, reader.read_packet()) => {
                match read_result {
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
                }
            }
        };

        ctx.stats.add_bytes_in(pkt.data.len() as u64);
        // NOTA: el unidle es manual como en sb0t — solo hablar en público o
        // emotear saca del idle (handle_public/handle_emote), no cualquier
        // paquete (un ping no debe des-idlear).

        match dispatch_message(&ctx, &scripting, &user_arc, &pkt).await {
            Ok(true) => {
                info!("desconectando id={} por flood de texto", user_id);
                break;
            }
            Ok(false) => {}
            Err(e) => warn!("error procesando msg {:?} de id={}: {}", pkt.msg, user_id, e),
        }
    }

    // ============================================================
    // Cleanup
    // ============================================================
    let user_name = user_arc.name.read().clone();
    // GHOST check (paridad sb0t Disconnect(true)): si esta sesión ya fue
    // sacada del pool por un hijack de login, o si hay OTRA sesión viva con
    // el mismo nick (el usuario ya reconectó — p.ej. cambio de red), la
    // salida es SILENCIOSA: sin PART a la sala (borraría de las userlists a
    // la sesión NUEVA, que los clientes indexan por nombre) y sin Part al
    // link. Los eventos de scripting se disparan igual (sb0t también los
    // dispara en el ghost).
    let ghost = ctx.is_ghost_departure(user_id, &user_name);
    ctx.record_departure(&user_arc);
    ctx.user_pool.remove(user_id);
    ctx.stats.on_user_part();
    // Forget idle tracking
    ctx.idle.forget(user_id);

    if !ghost {
        // Broadcast del PART
        broadcast_to_room(&ctx, &user_arc, |c| outbound::build_part_c(&user_arc, c));
        ctx.publish_link_event(LinkEvent::Part {
            origin: None,
            name: user_name.clone(),
        });
    } else {
        info!(
            "ghost departure: id={} '{}' (hay una sesión nueva con ese nick o ya fue hijackeado) — sin PART",
            user_id, user_name
        );
    }
    // Disparar evento de scripting
    scripting.dispatch(astra_scripting::ScriptEvent::Part {
        name: user_name.clone(),
    });
    scripting.dispatch(astra_scripting::ScriptEvent::Logout { name: user_name });

    // Cerrar el socket. `drop(tx)` no basta: el `sender` del user (un clone
    // del mismo canal, en el pool/Arc) sigue vivo, así que el writer no ve
    // cerrado su `rx` y `await` colgaría indefinidamente — dejando el socket
    // ABIERTO aunque ya difundimos el PART/onPart (bug: "el usuario sigue
    // conectado tras aparecer 'has parted'"). Le damos un margen para vaciar
    // lo pendiente (p.ej. el ServerError de flood) y forzamos el cierre
    // abortando el writer (dropea el write_half → el cliente recibe el FIN).
    drop(tx);
    tokio::select! {
        _ = &mut writer_handle => {}
        _ = tokio::time::sleep(Duration::from_millis(300)) => writer_handle.abort(),
    }

    info!("usuario id={} '{}' desconectado", user_id, user.name.read());
    scripting.dispatch(astra_scripting::ScriptEvent::Disconnect {
        ip: user.external_ip.to_string(),
    });
    Ok(())
}

/// Task que drena el mpsc y escribe al socket. Si una escritura falla, avisa
/// por `write_failed` para que el loop de lectura corte de inmediato en vez
/// de esperar a que el lado de lectura también note la conexión muerta (que
/// en una conexión half-dead puede tardar mucho más).
async fn writer_task(
    mut write_half: OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<Bytes>,
    write_failed: tokio::sync::oneshot::Sender<()>,
) {
    while let Some(data) = rx.recv().await {
        if data.is_empty() {
            continue;
        }
        // Framing Ares: [size:u16 LE][op][payload], size = largo de op+payload menos 1
        // (= largo del payload), igual que `ToAresPacket` de sb0t.
        let size = (data.len() - 1) as u16;
        let header = size.to_le_bytes();
        if let Err(e) = write_half.write_all(&header).await {
            debug!("writer: error escribiendo header: {}", e);
            let _ = write_failed.send(());
            return;
        }
        if let Err(e) = write_half.write_all(&data).await {
            debug!("writer: error escribiendo: {}", e);
            let _ = write_failed.send(());
            return;
        }
    }
    debug!("writer: terminado");
}

/// Wrapper sobre `OwnedReadHalf` que lee paquetes con el framing de Ares:
/// `[size:u16 LE][op:u8][payload:size]`, donde `size` es la longitud de
/// `op+payload` menos 1 (es decir, el largo del payload). Acumula bytes para
/// manejar paquetes partidos o coalescidos por TCP.
struct PacketReaderStream {
    inner: OwnedReadHalf,
    acc: Vec<u8>,
    scratch: Vec<u8>,
}

impl PacketReaderStream {
    fn new(inner: OwnedReadHalf) -> Self {
        Self {
            inner,
            acc: Vec::with_capacity(8192),
            scratch: vec![0u8; 8192],
        }
    }

    /// Lee un paquete completo. Retorna un `RawPacket` con `data.is_empty()`
    /// (EOF) cuando el socket se cierra sin un paquete pendiente.
    ///
    /// `data` se entrega como `[op][payload]` (el opcode en `data[0]`), para
    /// que el resto del handler siga usando `&data[1..]` sin cambios.
    async fn read_packet(&mut self) -> std::io::Result<RawPacket> {
        loop {
            // ¿Hay un paquete completo en el buffer acumulado?
            if self.acc.len() >= 3 {
                let size = u16::from_le_bytes([self.acc[0], self.acc[1]]) as usize;
                if self.acc.len() >= size + 3 {
                    let op = self.acc[2];
                    let msg = TcpMsg::from_u8(op);
                    let mut data = Vec::with_capacity(size + 1);
                    data.push(op);
                    data.extend_from_slice(&self.acc[3..3 + size]);
                    self.acc.drain(0..3 + size);
                    return Ok(RawPacket { msg, data });
                }
            }
            let n = self.inner.read(&mut self.scratch).await?;
            if n == 0 {
                return Ok(RawPacket::eof());
            }
            self.acc.extend_from_slice(&self.scratch[..n]);
        }
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
                        ctx.stats.on_invalid_login();
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

                    // Nick duplicado: si la sesión existente es de la MISMA IP
                    // externa, es una reconexión (el cliente perdió la conexión
                    // sin que el server se enterara todavía) — la tratamos como
                    // un hijack, paridad `TCPProcessor.cs` de sb0t (busca por
                    // Name/OrgName, compara `OriginalIP`): se saca la sesión
                    // vieja y se deja pasar la nueva. Si es una IP DISTINTA, se
                    // rechaza como antes ("name in use" real).
                    if let Some(existing) = ctx.user_pool.get_by_name(&login.org_name) {
                        if existing.external_ip == external_ip {
                            info!(
                                "hijack (misma IP): peer={} nick='{}' reemplaza sesión vieja id={}",
                                peer, login.org_name, existing.id
                            );
                            // GHOST (paridad sb0t Disconnect(true)): sin PART
                            // ni anuncio — la sesión nueva reemplaza el nombre
                            // en las userlists sin que la sala vea "has parted".
                            ctx.ghost_part_user(&existing);
                        } else {
                            warn!("REJECTED (nick en uso): peer={} nick='{}'", peer, login.org_name);
                            let _ = tx.send(server_error_packet("Nickname already in use"));
                            return Ok(None);
                        }
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
                        ctx.stats.on_flood();
                        scripting.dispatch(astra_scripting::ScriptEvent::Flood {
                            name: login.org_name.clone(),
                        });
                        return Ok(None);
                    }

                    // Construir usuario
                    let wants_crypto = login.crypto; // crypto == 250 en el login
                    let id = ctx.user_pool.next_id();
                    let mut user = server_core::login::build_ares_user(id, external_ip, login);
                    user.sender = Some(tx.clone());
                    user.logged_in = true;  // marcar ANTES de envolver en Arc
                    // Cifrado del cliente Ares: generamos key/IV; se los mandamos
                    // ofuscados en el CryptoKey antes del LoginAck.
                    if wants_crypto {
                        user.ares_crypto = Some(proto_ares::AresCrypto::generate());
                    }
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

                    // Cliente cifrado: primero el CryptoKey (ofuscado con el
                    // GUID); a partir de acá todos los strings van cifrados.
                    if let Some(crypto) = user_arc.ares_crypto {
                        tx.send(build_crypto_key(&crypto, &user_arc.guid))?;
                    }

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
                        let _ = user_arc.send_pvt(&ctx.settings.bot_name, &prompt);
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
            ctx.security.failed_logins.record_failure(peer.ip());
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
    let crypto = user.ares_crypto; // cifra los strings si el cliente lo negoció

    // Topic
    let _ = user.send(outbound::build_topic_first_c(&ctx.current_room_topic(), crypto));

    // Bot fantasma
    let _ = user.send(outbound::build_userlist_bot_c(&ctx.settings.bot_name, crypto));
    // Avatar de sala (bot), si hay uno configurado (paridad `Avatars.Server`,
    // mandado en cada login en `TCPProcessor.cs`).
    if let Some(avatar) = ctx.server_avatar.read().clone() {
        let _ = user.send(outbound::build_avatar_c(&ctx.settings.bot_name, &avatar, crypto));
    }

    // Userlist de todos los usuarios conectados
    let others = ctx.user_pool.users();
    for other in others {
        if other.id != user_id && other.logged_in {
            let _ = user.send(outbound::build_userlist_item_c(&other, crypto));
            // Evento de scripting por cada user en la userlist
            scripting.dispatch(astra_scripting::ScriptEvent::UserList {
                name: other.name.read().clone(),
                users_csv: String::new(),
            });
            // Avatar y mensaje personal ya existentes (paridad
            // `TCPProcessor.Login`: sb0t manda el avatar/pmsg de cada user ya
            // conectado al recién llegado; sin esto, un cliente Ares nativo
            // nunca ve avatares/pmsg de nadie que se conectó antes que él).
            let other_name = other.name.read().clone();
            if let Some(avatar) = other.avatar.lock().clone() {
                let _ = user.send(outbound::build_avatar_c(&other_name, &avatar, crypto));
            }
            let pmsg = other.personal_message.lock().clone();
            if !pmsg.is_empty() {
                let _ = user.send(outbound::build_personal_message_c(&other_name, &pmsg, crypto));
            }
        }
    }

    // End of userlist (sin strings: cifrado-invariante)
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
/// Lee un string null-terminated (descifrado) del payload de un paquete.
fn read_crypto_text(
    user: &Arc<server_core::user_pool::AresUser>,
    data: &[u8],
) -> Option<String> {
    let mut r = PacketReader::new_crypto(data, user.ares_crypto);
    r.read_string_nt().ok()
}

/// Chequea el flood de texto de sb0t (rate-limit + duplicados). Si el usuario
/// está flooding, le manda un ServerError y devuelve `true` (el caller debe
/// desconectarlo). Niveles > Regular están exentos.
fn is_text_flooding(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    scripting: &ScriptHandle,
    kind: server_core::flood_control::FloodKind,
    text: &str,
) -> bool {
    let level = *user.level.read();
    let now = server_core::time::unix_time();
    if user.flood.is_flooding(kind, text, level, now) {
        // Gate de scripts (onFloodBefore, paridad sb0t `Flooding`): si algún
        // script retorna false, se perdona el flood (no se castiga).
        let fname = user.name.read().clone();
        if !scripting.check_flood(&fname, text) {
            debug!("flood de '{}' perdonado por script (onFloodBefore)", fname);
            return false;
        }
        ctx.stats.on_flood();
        let mut w = proto_ares::PacketWriter::with_msg_crypto(
            proto_ares::TcpMsg::ServerError,
            user.ares_crypto,
        );
        w.write_string_nt("You are flooding the room and have been disconnected.")
            .ok();
        let _ = user.send(bytes::Bytes::copy_from_slice(w.as_bytes()));
        info!(
            "flood de texto de '{}' (id={}) → desconectando",
            user.name.read(),
            user.id
        );
        return true;
    }
    false
}

/// Procesa un paquete del cliente. Devuelve `Ok(true)` si el cliente debe
/// desconectarse (p.ej. por flood de texto).
async fn dispatch_message(
    ctx: &Arc<AppContext>,
    scripting: &ScriptHandle,
    user: &Arc<server_core::user_pool::AresUser>,
    pkt: &RawPacket,
) -> anyhow::Result<bool> {
    let msg = match pkt.msg {
        Some(m) => m,
        None => {
            warn!("opcode desconocido {} de id={}", pkt.data[0], user.id);
            return Ok(false);
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
            // Paridad `TCPProcessor.cs` MSG_CHAT_CLIENT_UPDATE_STATUS: solo
            // responde al mismo cliente (echo de su propio status de
            // file-sharing), NUNCA se difunde a la sala. Difundirlo (como
            // hacía una versión anterior, reusando el opcode de JOIN) hacía
            // que cualquier bot que manda esto periódicamente disparara un
            // "has joined" fantasma en los clientes web.
            let _ = user.send(outbound::build_update_user_status_c(user, user.ares_crypto));
        }
        TcpMsg::ClientIgnorelist => {
            handle_ignore_list(user, &pkt.data[1..]);
        }
        TcpMsg::Avatar => {
            // Gate de sala: si los avatares están deshabilitados, se ignora.
            if !ctx.room_flags.get("avatars") {
                return Ok(false);
            }
            publish_raw_link(ctx, LINK_MSG_AVATAR, &pkt.data[1..]);
            // Avatar: el payload completo (sin opcode) son los bytes PNG.
            // < 10 bytes = "sin avatar" (paridad `AresClient.Avatar` setter).
            let png = pkt.data[1..].to_vec();
            let cleared = png.len() < 10;
            *user.avatar.lock() = if cleared { None } else { Some(png.clone()) };
            // Avatar propio del cliente → stash para user.restoreAvatar().
            *user.org_avatar.lock() = if cleared { None } else { Some(png.clone()) };
            *user.full_avatar.lock() = None;
            user.avatar_received.store(true, std::sync::atomic::Ordering::Relaxed);
            scripting.dispatch(astra_scripting::ScriptEvent::Avatar {
                name: user.name.read().clone(),
                png: png.clone(),
            });
            // Difundir a la sala (Ares nativos + web/inbizier), paridad del
            // setter de `AresClient.Avatar`: sin esto, nadie más ve el avatar
            // hasta que se reconecten (nunca, en la práctica).
            let name = user.name.read().clone();
            broadcast_to_room(ctx, user, |c| {
                if cleared {
                    outbound::build_avatar_cleared_c(&name, c)
                } else {
                    outbound::build_avatar_c(&name, &png, c)
                }
            });
        }
        TcpMsg::Public => {
            // Control de flood de texto (rate-limit + duplicados) antes de
            // procesar. Los comandos slash se eximen (no son chat). Nivel >
            // Regular está exento (lo maneja is_flooding).
            let text = read_crypto_text(user, &pkt.data[1..]);
            let is_cmd = text.as_deref().map(|t| t.trim_start().starts_with('/')).unwrap_or(false);
            if !is_cmd
                && is_text_flooding(ctx, user, scripting, FloodKind::Public, text.as_deref().unwrap_or(""))
            {
                return Ok(true);
            }
            handle_public(ctx, user, &pkt.data[1..], &scripting).await;
        }
        TcpMsg::Emote => {
            let text = read_crypto_text(user, &pkt.data[1..]);
            if is_text_flooding(ctx, user, scripting, FloodKind::Emote, text.as_deref().unwrap_or("")) {
                return Ok(true);
            }
            handle_emote(ctx, user, &pkt.data[1..], scripting).await;
        }
        TcpMsg::Pmt => {
            if is_text_flooding(ctx, user, scripting, FloodKind::Pm, "") {
                return Ok(true);
            }
            handle_pvt(ctx, user, &pkt.data[1..], scripting).await;
        }
        TcpMsg::PersonalMessage => {
            handle_personal_message(ctx, user, &pkt.data[1..], scripting).await;
        }
        TcpMsg::ClientCommand => {
            // Canal de comandos de Ares (sin '/'). Se rutea a los built-ins.
            let mut r = PacketReader::new_crypto(&pkt.data[1..], user.ares_crypto);
            if let Ok(cmd_text) = r.read_string_nt() {
                route_command_text(ctx, user, &scripting, &cmd_text);
            }
        }
        TcpMsg::ClientAuthLogin => {
            // Atajo de protocolo para `/login <password>` (sb0t AUTHLOGIN).
            let mut r = PacketReader::new_crypto(&pkt.data[1..], user.ares_crypto);
            if let Ok(pw) = r.read_string_nt() {
                route_command_text(ctx, user, &scripting, &format!("login {}", pw));
            }
        }
        TcpMsg::ClientAuthRegister => {
            // Atajo de protocolo para `/register <password>` (sb0t AUTHREGISTER).
            let mut r = PacketReader::new_crypto(&pkt.data[1..], user.ares_crypto);
            if let Ok(pw) = r.read_string_nt() {
                route_command_text(ctx, user, &scripting, &format!("register {}", pw));
            }
        }
        TcpMsg::ClientAutologin => {
            // Auto-login por GUID: restaura el nivel de la cuenta asociada.
            handle_autologin(ctx, user);
        }
        TcpMsg::ClientBrowse => {
            publish_raw_link(ctx, LINK_MSG_BROWSE, &pkt.data[1..]);
            // El payload de ClientBrowse es un string Ares (null-terminated) con
            // un hashlink del archivo que se está compartiendo.
            let mut r = PacketReader::new_crypto(&pkt.data[1..], user.ares_crypto);
            if let Ok(hashlink) = r.read_string_nt() {
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
                return Ok(false);
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
                return Ok(false);
            }
            // Chunks siempre se reenvían (asumiendo que First pasó el gate).
            // Para gate por-chunk, se necesitaría trackear el state de cada scribble.
            publish_raw_link(ctx, LINK_MSG_SCRIBBLE_LEAF, &pkt.data[1..]);
        }
        TcpMsg::AdvancedFeatures => {
            handle_advanced_features(ctx, user, &pkt.data[1..]);
        }
        _ => {
            debug!("mensaje {:?} de id={} (no procesado en esta fase)", msg, user.id);
        }
    }
    Ok(false)
}

/// Desenvuelve el wrapper `MSG_CHAT_ADVANCED_FEATURES_PROTOCOL` (op 250) y
/// procesa el paquete Ares interno. Estructura del payload:
/// `[innerSize:u16][inner_op:u8][inner_payload]`. Se usa para voice chat.
fn handle_advanced_features(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    payload: &[u8],
) {
    let mut r = PacketReader::new(payload);
    if r.read_u16_le().is_err() {
        return;
    }
    let inner_op = match r.read_u8() {
        Ok(v) => v,
        Err(_) => return,
    };
    let inner = &payload[r.position()..];
    // CustomFont (204): fuente custom del cliente (paridad sb0t
    // `TCPAdvancedProcessor.Font`). Va ANTES del gate de muzzle: la fuente
    // no es transmisión de voz y sb0t no la gatea por muzzle.
    if TcpMsg::from_u8(inner_op) == Some(TcpMsg::CustomFont) {
        handle_custom_font(ctx, user, inner);
        return;
    }
    // Voice chat: el emisor muzzled no transmite (paridad sb0t).
    if user.is_muzzled() {
        return;
    }
    let sender = user.name.read().clone();
    match TcpMsg::from_u8(inner_op) {
        // Público: retransmitir a la sala (VcFirst=206, VcChunk=208).
        Some(op @ TcpMsg::VcFirst) | Some(op @ TcpMsg::VcChunk) => {
            vc_relay_public(ctx, user, &sender, op, inner);
        }
        // Privado: VcFirstTo=207 / VcChunkTo=209 (mismos valores que los
        // SERVER_VC_*_FROM que se reenvían al target).
        Some(op @ TcpMsg::VcFirstFrom) | Some(op @ TcpMsg::VcChunkFrom) => {
            vc_relay_private(ctx, user, &sender, op, inner);
        }
        _ => {} // VcSupported/VcIgnore/etc: no-op por ahora.
    }
}

/// Tabla clásica de colores de Ares por índice → HTML "#rrggbb" (paridad
/// `Helpers.AresColorToHTMLColor`/`acols` de sb0t, mismos 40 colores .NET).
const ARES_COLORS: &[&str] = &[
    "#ffffff", "#000000", "#000080", "#008000", "#ff0000", "#800000", "#800080", "#ffa500",
    "#ffff00", "#00ff00", "#008080", "#00ffff", "#0000ff", "#ff00ff", "#808080", "#c0c0c0",
    "#ff4500", "#8b4513", "#008b8b", "#4b0082", "#dc143c", "#228b22", "#9932cc", "#ff69b4",
    "#2f4f4f", "#b0c4de", "#7cfc00", "#20b2aa", "#deb887", "#7fff00", "#b8860b", "#8b008b",
    "#00bfff", "#ffd700", "#f08080", "#9370db", "#808000", "#db7093", "#bc8f8f", "#2e8b57",
    "#6a5acd", "#00ff7f", "#ff6347", "#ee82ee", "#f5deb3", "#9acd32",
];

fn ares_color_to_html(index: u8) -> String {
    ARES_COLORS
        .get(index as usize)
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Procesa el paquete CustomFont (204) de un cliente Ares. Formato sb0t:
/// `size u8, fontname str, oldN u8, oldT u8, [nameColor str], [textColor str]`
/// donde los strings de colores son opcionales y caen a la tabla clásica de
/// Ares por índice. Guarda la fuente en el user y la retransmite a los
/// custom clients del vroom (binario) y a los clientes web (`FONT:`), igual
/// que sb0t. Con <2 bytes, resetea la fuente (paridad exacta).
fn handle_custom_font(
    ctx: &AppContext,
    user: &Arc<server_core::user_pool::AresUser>,
    payload: &[u8],
) {
    use server_core::types::IFont;

    if payload.len() < 2 {
        *user.font.write() = IFont::default();
        return;
    }
    let mut r = PacketReader::new_crypto(payload, user.ares_crypto);
    let size = match r.read_u8() {
        Ok(v) => v,
        Err(_) => return,
    };
    let face = r.read_string_nt().unwrap_or_default();
    let old_n = r.read_u8().unwrap_or(0);
    let old_t = r.read_u8().unwrap_or(0);
    let mut name_color = if r.remaining() > 0 {
        r.read_string_nt().unwrap_or_default()
    } else {
        String::new()
    };
    let mut text_color = if r.remaining() > 0 {
        r.read_string_nt().unwrap_or_default()
    } else {
        String::new()
    };
    if name_color.is_empty() {
        name_color = ares_color_to_html(old_n);
    }
    if text_color.is_empty() {
        text_color = ares_color_to_html(old_t);
    }

    // Flag de sala `fonts` (sb0t `Settings.Get<bool>("fonts_enabled")`).
    if !ctx.room_flags.get("fonts") {
        return;
    }

    let font = IFont {
        face: face.clone(),
        color: 0,
        size: size.min(18), // sb0t: "fonts cannot be bigger than 18"
        bold: false,
        italic: false,
        underline: false,
        name_color: name_color.clone(),
        text_color: text_color.clone(),
        old_name_color: old_n,
        old_text_color: old_t,
        enabled: true,
    };
    *user.font.write() = font;

    if user.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // Relay (paridad sb0t): binario a custom clients del vroom, `FONT:` a web.
    let sender_name = user.name.read().clone();
    let vroom = *user.vroom.read();
    let ws_msg = format!(
        "FONT:{},{},{}:{}{}{}",
        sender_name.encode_utf16().count(),
        old_n.to_string().len(),
        old_t.to_string().len(),
        sender_name,
        old_n,
        old_t
    );
    for other in ctx.user_pool.users() {
        if !other.logged_in
            || *other.vroom.read() != vroom
            || other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if let Some(tx) = &other.ws_text_sender {
            let _ = tx.send(ws_msg.clone());
        } else if other.cbot && other.id != user.id {
            let _ = other.send(build_custom_font_wrapped(
                &sender_name,
                size.min(18),
                &face,
                old_n,
                old_t,
                &name_color,
                &text_color,
                other.ares_crypto,
            ));
        }
    }
}

/// Construye el `MSG_CHAT_SERVER_CUSTOM_FONT` (204) envuelto en
/// ADVANCED_FEATURES (250), cifrando los strings para el destinatario
/// (paridad `TCPOutbound.CustomFont`).
#[allow(clippy::too_many_arguments)]
fn build_custom_font_wrapped(
    name: &str,
    size: u8,
    face: &str,
    old_n: u8,
    old_t: u8,
    name_color: &str,
    text_color: &str,
    crypto: Option<proto_ares::AresCrypto>,
) -> Bytes {
    let mut inner = PacketWriter::with_msg_crypto(TcpMsg::CustomFont, crypto);
    inner.write_string_nt(name).ok();
    inner.write_u8(size).ok();
    inner.write_string_nt(face).ok();
    inner.write_u8(old_n).ok();
    inner.write_u8(old_t).ok();
    inner.write_string_nt(name_color).ok();
    inner.write_string_nt(text_color).ok();
    let inner_bytes = inner.into_bytes();
    let inner_size = (inner_bytes.len() - 1) as u16;
    let mut outer = PacketWriter::with_msg(TcpMsg::AdvancedFeatures);
    outer.write_u16_le(inner_size).ok();
    outer.write_bytes(&inner_bytes).ok();
    Bytes::copy_from_slice(outer.as_bytes())
}

/// Construye un paquete de voice chat envuelto en ADVANCED_FEATURES:
/// `[250][innerSize:u16][inner_op][sender\0][voice]` (el framing externo lo
/// agrega la writer task).
fn build_vc_wrapped(inner_op: TcpMsg, sender: &str, voice: &[u8]) -> Bytes {
    let mut inner = PacketWriter::with_msg(inner_op);
    inner.write_string_nt(sender).ok();
    inner.write_bytes(voice).ok();
    let inner_bytes = inner.into_bytes(); // [inner_op][sender\0][voice]
    let inner_size = (inner_bytes.len() - 1) as u16; // = largo de sender\0+voice

    let mut outer = PacketWriter::with_msg(TcpMsg::AdvancedFeatures);
    outer.write_u16_le(inner_size).ok();
    outer.write_bytes(&inner_bytes).ok();
    Bytes::copy_from_slice(outer.as_bytes())
}

/// Retransmite voz pública a los usuarios de la sala con VC público activo.
fn vc_relay_public(
    ctx: &AppContext,
    sender: &Arc<server_core::user_pool::AresUser>,
    sender_name: &str,
    op: TcpMsg,
    voice: &[u8],
) {
    let vroom = *sender.vroom.read();
    for u in ctx.user_pool.users() {
        if u.id != sender.id
            && u.logged_in
            && *u.vroom.read() == vroom
            && u.voice_chat_public
            && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            let _ = u.send(build_vc_wrapped(op, sender_name, voice));
        }
    }
}

/// Retransmite voz privada a un target. `inner = [targetName\0][voice]`.
fn vc_relay_private(
    ctx: &AppContext,
    sender: &Arc<server_core::user_pool::AresUser>,
    sender_name: &str,
    op: TcpMsg,
    inner: &[u8],
) {
    // El nombre del target va cifrado si el emisor negoció cifrado.
    let mut r = PacketReader::new_crypto(inner, sender.ares_crypto);
    let target_name = match r.read_string_nt() {
        Ok(s) => s,
        Err(_) => return,
    };
    let voice = &inner[r.position()..];
    if let Some(target) = ctx.user_pool.get_by_name(&target_name) {
        if target.voice_chat_private
            && !target.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            let _ = target.send(build_vc_wrapped(op, sender_name, voice));
        }
    }
}

/// Rutea un texto de comando (con o sin '/') a los built-ins, disparando los
/// eventos de scripting que genere. Usado por el opcode `ClientCommand` y los
/// atajos AUTHLOGIN/AUTHREGISTER.
fn route_command_text(
    ctx: &Arc<AppContext>,
    user: &Arc<server_core::user_pool::AresUser>,
    scripting: &ScriptHandle,
    text: &str,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let slashed = if text.starts_with('/') {
        text.to_string()
    } else {
        format!("/{}", text)
    };
    if let Some((cmd, args)) = astra_commands::parse_command(&slashed) {
        let (_handled, events) = astra_commands::dispatch_builtin(ctx, user, cmd, args);
        // Eventos side-effect del builtin (ej. AdminLevelChanged tras /ban).
        for ev in events {
            scripting.dispatch(ev);
        }
        // Notificar a los scripts de TODO comando (onCommand), lo haya manejado
        // un builtin o no. Paridad sb0t: los scripts ven todos los comandos y
        // pueden reaccionar (ej. sumar salida a /help). Excepción: con captcha
        // pendiente sb0t corta antes de llegar a los scripts.
        if !user.needs_captcha.load(std::sync::atomic::Ordering::Relaxed) {
            let name = user.name.read().clone();
            astra_commands::dispatch(ctx, scripting, &name, cmd, args);
        }
    }
}

/// Maneja el opcode AUTOLOGIN: restaura el nivel de la cuenta asociada al GUID.
fn handle_autologin(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>) {
    let _ = astra_commands::dispatch_autologin(ctx, user);
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
    let mut r = PacketReader::new_crypto(data, user.ares_crypto);
    let text = match r.read_string_nt() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }

    // Si tiene captcha pendiente, no puede hablar en público.
    // (Los comandos pasan al dispatcher, que aplica su propio gate: solo
    // help/login con captcha pendiente — paridad core/Events.cs:470-481.)
    if user.needs_captcha.load(std::sync::atomic::Ordering::Relaxed)
        && !text.trim_start().starts_with(['/', '#'])
    {
        let _ = user.send_pvt(
            &ctx.settings.bot_name,
            "Please solve the captcha before chatting. Check your PMs.",
        );
        return;
    }

    let name = user.name.read().clone();

    // Eval inline `@código` (paridad sb0t TextSending, gated a Owner): se
    // evalúa como JS en el primer script con `userobj` = el emisor, y el
    // texto NO se difunde.
    if let Some(code) = text.strip_prefix('@') {
        if (*user.level.read() as u8) >= server_core::ILevel::Owner as u8 {
            if let Err(e) = scripting.eval_chat(&name, code) {
                let _ = user.print(&ctx.settings.bot_name, &format!("eval error: {}", e));
            }
        }
        return;
    }

    // Si el mensaje empieza con '/' o '#', es un comando (paridad sb0t
    // TCPProcessor.cs:343: ambos prefijos, y el texto no se difunde).
    if let Some((cmd, args)) = astra_commands::parse_command(&text) {
        let (handled, events) = astra_commands::dispatch_builtin(ctx, user, cmd, args);
        debug!("comando de '{}' (builtin={}): /{} {}", name, handled, cmd, args);
        // Disparar los side-effects de scripting que el comando generó
        // (incluye onVroomJoin, onNick, onRegistered, etc., que ahora
        // emite el propio handler en `commands`).
        for ev in events {
            scripting.dispatch(ev);
        }
        // Los scripts ven TODOS los comandos vía onCommand, los haya manejado
        // un builtin o no (paridad core/Events.cs → js.Command siempre corre),
        // salvo con captcha pendiente (sb0t corta antes de llegar a scripts).
        if !user.needs_captcha.load(std::sync::atomic::Ordering::Relaxed) {
            astra_commands::dispatch(ctx, scripting, &name, cmd, args);
        }
        return;
    }

    // Muzzled: puede ejecutar comandos pero no hablar en público.
    // (is_muzzled auto-expira los muzzles temporales de /mtimeout)
    if user.is_muzzled() {
        let _ = user.send_pvt(
            &ctx.settings.bot_name,
            "You are muzzled and cannot chat in public.",
        );
        return;
    }

    // Word filter: solo aplica (censura) a usuarios regulares (Moderator+ exentos).
    if (*user.level.read() as u8) < server_core::ILevel::Moderator as u8 {
        if let Some(action) = ctx.word_filter.check(&text) {
            apply_filter_action(ctx, user, action, &name);
            return;
        }
    }

    // Filtro Announce: dispara para cualquier usuario, NO bloquea el
    // mensaje (paridad `FilterType.Announce` de sb0t) — se difunden
    // líneas enlatadas además del mensaje normal. Con `adminannounce` on,
    // no dispara para usuarios regulares (sb0t WordFilter.cs:195).
    let announce_blocked = ctx.room_flags.get("adminannounce")
        && *user.level.read() == server_core::ILevel::Regular;
    if !announce_blocked {
        if let Some((_, lines, remainder)) = ctx.word_filter.check_announce(&text) {
            broadcast_announce_lines(ctx, &name, user.external_ip, &lines, &remainder);
        }
    }

    // Echo heckle (/echo): reenvía el texto configurado solo a este usuario.
    if let Some(echo) = user.echo_text.read().clone() {
        let _ = user.send_pvt(&ctx.settings.bot_name, &echo);
    }

    // Efectos de castigo por-usuario (/kiddy, /lower).
    let mut text = server_core::text_effects::apply_punish_effects(user, &text);

    // Caps monitoring de sala: los mensajes TODO-EN-MAYÚSCULAS se bajan a
    // minúsculas (paridad sb0t CapsMonitoring).
    if ctx.room_flags.get("caps") && server_core::text_effects::is_shouting(&text) {
        text = text.to_lowercase();
    }

    // Hook onTextBefore: un script puede cancelar el mensaje (None) o
    // REESCRIBIRLO (Some(texto)) — paridad sb0t TextSending.
    let text = match scripting.check_text_before(&name, &text) {
        Some(t) => t,
        None => {
            debug!("onTextBefore canceló mensaje de '{}'", name);
            return;
        }
    };

    // Hablar en público saca del idle y anuncia el tiempo ausente
    // (paridad sb0t TCPProcessor: unidle dentro del path de TextSending).
    if ctx.unidle_user(user).is_some() {
        scripting.dispatch(astra_scripting::ScriptEvent::Unidled { name: name.clone() });
    }

    broadcast_to_room(ctx, user, |c| outbound::build_public_c(&name, &text, c));
    ctx.record_message(&name, &text, false);
    vspy_copy(ctx, user, &name, &text);
    ctx.publish_link_event(LinkEvent::Public {
        origin: None,
        from: name.clone(),
        text: text.clone(),
    });
    // Disparar evento de scripting
    ctx.stats.on_message();
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
    let mut r = PacketReader::new_crypto(data, user.ares_crypto);
    let text = match r.read_string_nt() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }
    // Muzzled: sin voz en público (aplica también a emotes).
    if user.is_muzzled() {
        let _ = user.send_pvt(
            &ctx.settings.bot_name,
            "You are muzzled and cannot chat in public.",
        );
        return;
    }

    let name = user.name.read().clone();
    // Hook onEmoteBefore: puede cancelar (None) o reescribir (Some).
    let text = match scripting.check_emote_before(&name, &text) {
        Some(t) => t,
        None => {
            debug!("onEmoteBefore canceló emote de '{}'", name);
            return;
        }
    };
    // Paridad sb0t TCPProcessor (emote path): primero el unidle si estaba
    // idle, luego un emote que empiece con "idles" marca al usuario como
    // ausente (`#me idles almorzando`), y el emote se difunde igual.
    if ctx.unidle_user(user).is_some() {
        scripting.dispatch(astra_scripting::ScriptEvent::Unidled { name: name.clone() });
    }
    if text.starts_with("idles") && ctx.mark_user_idle(user) {
        scripting.dispatch(astra_scripting::ScriptEvent::Idled { name: name.clone() });
    }
    broadcast_to_room(ctx, user, |c| outbound::build_emote_c(&name, &text, c));
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
    let mut r = PacketReader::new_crypto(data, user.ares_crypto);
    let target_name = match r.read_string_nt() {
        Ok(s) => s,
        Err(_) => return,
    };
    let text = match r.read_string_nt() {
        Ok(s) => s,
        Err(_) => return,
    };
    if text.is_empty() {
        return;
    }

    // Buscar al destinatario
    if let Some(target) = ctx.user_pool.get_by_name(&target_name) {
        let from = user.name.read().clone();
        // Hook onPMBefore: puede cancelar (None) o reescribir el PM (Some).
        let text = match scripting.check_pm_before(&from, &target_name, &text) {
            Some(t) => t,
            None => {
                debug!("onPMBefore canceló PM de '{}' a '{}'", from, target_name);
                return;
            }
        };
        // /pmblock: si el target bloquea PMs y el emisor es regular, se trata
        // como ignore (Moderator+ siempre pasan).
        let blocked_by_pmblock = target.pm_blocked.load(std::sync::atomic::Ordering::Relaxed)
            && (*user.level.read() as u8) < server_core::ILevel::Moderator as u8;
        if blocked_by_pmblock
            || target
                .ignore_list
                .read()
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(&from))
        {
            let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerIsIgnoringYou, user.ares_crypto);
            w.write_string_nt(&target_name).ok();
            let _ = user.send(Bytes::copy_from_slice(w.as_bytes()));
            ctx.publish_link_event(LinkEvent::PrivateIgnored {
                origin: None,
                from,
                to: target_name,
            });
        } else {
            // Evento onPrivate (tras pasar el hook onPMBefore y los bloqueos).
            ctx.stats.on_pm();
            scripting.dispatch(astra_scripting::ScriptEvent::Private {
                from: from.clone(),
                to: target_name.clone(),
                text: text.clone(),
            });
            // El PM se cifra con la key del DESTINATARIO.
            let pkt = outbound::build_pvt_c(&from, &text, target.ares_crypto);
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
        let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerNosuch, user.ares_crypto);
        w.write_string_nt(&format!("User '{}' not found", target_name)).ok();
        user.send(Bytes::copy_from_slice(w.as_bytes()));
    }
}

/// Maneja MSG_CHAT_CLIENT_IGNORELIST (45).
/// Formato: lista de strings Ares consecutivas.
fn handle_ignore_list(user: &Arc<server_core::user_pool::AresUser>, data: &[u8]) {
    let mut r = PacketReader::new_crypto(data, user.ares_crypto);
    let mut list = Vec::new();
    while r.remaining() > 0 {
        let Ok(entry) = r.read_string_nt() else {
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
async fn handle_personal_message(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>, data: &[u8], scripting: &ScriptHandle) {
    let mut r = PacketReader::new_crypto(data, user.ares_crypto);
    let text = match r.read_string_nt() {
        Ok(s) => s,
        Err(_) => return,
    };
    *user.personal_message.lock() = text.clone();
    // Evento onPersonalMessage.
    scripting.dispatch(astra_scripting::ScriptEvent::PersonalMessage {
        name: user.name.read().clone(),
        text: text.clone(),
    });

    // Broadcast a la sala: cada uno recibe un "MSG_CHAT_SERVER_PERSONAL_MESSAGE"
    // con el nuevo PM. Lo simplificamos: el server reenvía a cada user.
    let uname = user.name.read().clone();
    broadcast_to_room(ctx, user, |c| {
        let mut w = PacketWriter::with_msg_crypto(TcpMsg::PersonalMessage, c);
        w.write_string_nt(&uname).ok();
        w.write_string_nt(&text).ok();
        Bytes::copy_from_slice(w.as_bytes())
    });
    ctx.publish_link_event(LinkEvent::PersonalMessage {
        origin: None,
        name: user.name.read().clone(),
        text,
    });
}

/// Broadcast a todos los usuarios en la misma vroom que `sender`.
/// `sender` también lo recibe (compat con sb0t original).
/// `build` construye el paquete binario para un destinatario dado su `crypto`
/// (`None` = plano). Se llama una vez con `None` (paquete compartido para WS y
/// clientes sin cifrar) y una vez por cada cliente Ares cifrado.
pub(crate) fn broadcast_to_room<F>(ctx: &AppContext, sender: &server_core::user_pool::AresUser, build: F)
where
    F: Fn(server_core::outbound::Crypto) -> Bytes,
{
    let vroom = *sender.vroom.read();
    let plain = build(None);
    for u in ctx.user_pool.users() {
        if u.logged_in && *u.vroom.read() == vroom && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            if u.web_client {
                // Cliente web: traducir el binario (plano) al formato texto ib0t.
                if let Some(text) = astra_web::ws_outbound::translate_broadcast(&plain, sender, &u) {
                    if let Some(tx) = &u.ws_text_sender {
                        let _ = tx.send(text);
                    }
                }
            } else if let Some(crypto) = u.ares_crypto {
                // Cliente Ares cifrado: reconstruir con su key.
                let _ = u.send(build(Some(crypto)));
            } else {
                // Cliente Ares sin cifrar: el paquete plano compartido.
                let _ = u.send(plain.clone());
            }
        }
    }
}

/// Envía una copia de un mensaje público/emote a los admins suscritos a
/// `/vspy` que estén en un vroom DISTINTO al del emisor (monitoreo
/// cross-vroom, paridad sb0t VSpy).
fn vspy_copy(ctx: &AppContext, sender: &server_core::user_pool::AresUser, name: &str, text: &str) {
    let sender_vroom = *sender.vroom.read();
    let line = format!("[vroom {}] {}: {}", sender_vroom, name, text);
    for u in ctx.user_pool.users() {
        if u.logged_in
            && u.sub_vspy.load(std::sync::atomic::Ordering::Relaxed)
            && *u.vroom.read() != sender_vroom
            && (*u.level.read() as u8) >= server_core::ILevel::Moderator as u8
        {
            let _ = u.send_pvt(&ctx.settings.bot_name, &line);
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
    // Paridad sb0t: `pmgreetmsg` = greet por PM al que entra; `greetmsg` =
    // greet público a la sala (Server.Print). Pueden estar ambos activos.
    if ctx.room_flags.get("pmgreetmsg") {
        let _ = user.send_pvt(&ctx.settings.bot_name, &text);
    }
    if ctx.room_flags.get("greetmsg") {
        ctx.broadcast_print(&text);
    }
}

/// Envía el MOTD (message of the day) al usuario que entra, línea por línea
/// como PM del bot. No-op si no hay MOTD configurado. Paridad conceptual con
/// `ViewMOTD` de sb0t (texto plano; sin tags de media).
fn send_motd(ctx: &AppContext, user: &server_core::user_pool::AresUser) {
    if ctx.motd.is_empty() {
        return;
    }
    let mctx = server_core::MotdContext {
        name: &user.name.read(),
        room_name: &ctx.settings.room_name,
        ip: &user.external_ip.to_string(),
        user_count: ctx.user_pool.len(),
    };
    for line in ctx.motd.rendered_lines(&mctx) {
        let _ = user.send_pvt(&ctx.settings.bot_name, &line);
    }
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
    let _ = user.send_pvt(
        &ctx.settings.bot_name,
        "Your message was blocked by a word filter.",
    );

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
        FilterAction::Announce => {
            // No debería llegar acá: `check()` (censura) nunca devuelve
            // Announce — ver `check_announce` para ese path, que no
            // bloquea el mensaje.
            debug!("word filter: Announce inesperado en apply_filter_action de '{}'", name);
        }
    }
}

/// Difunde las líneas de un filtro `Announce` a toda la sala (paridad del
/// `case FilterType.Announce` de sb0t en `WordFilter.FilterBefore`), con
/// los placeholders `+n` (nombre del emisor), `+ip` (su IP) y `+r` (resto
/// del texto tras el trigger) sustituidos. El mensaje original del emisor
/// se sigue procesando normalmente — esto es adicional, no un reemplazo.
fn broadcast_announce_lines(
    ctx: &AppContext,
    sender_name: &str,
    sender_ip: std::net::IpAddr,
    lines: &[String],
    remainder: &str,
) {
    let bot = ctx.settings.bot_name.clone();
    for line in lines {
        let msg = line
            .replace("+n", sender_name)
            .replace("+ip", &sender_ip.to_string())
            .replace("+r", remainder);
        for u in ctx.user_pool.users() {
            if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = u.send_public(&bot, &msg);
            }
        }
    }
}

/// Remueve un usuario del pool y difunde su PART (mismo patrón que `/kick`).
fn filter_remove_user(ctx: &AppContext, user: &Arc<server_core::user_pool::AresUser>) {
    ctx.record_departure(user);
    ctx.user_pool.remove(user.id);
    ctx.stats.on_user_part();
    for u in ctx.user_pool.users() {
        if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = u.send(outbound::build_part_c(user, u.ares_crypto));
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
    w.write_string_nt(text).ok();
    Bytes::copy_from_slice(w.as_bytes())
}

pub(crate) async fn send_server_error_to_stream(mut stream: TcpStream, text: &str) -> anyhow::Result<()> {
    stream.write_all(&server_error_packet(text)).await?;
    Ok(())
}

/// Construye el CryptoKey (`MSG_CHAT_SERVER_CRYPTO_KEY`, op 230) envuelto en
/// ADVANCED_FEATURES: `[250][innerSize:u16][230][IV++Key ofuscado con e67]`.
/// El framing externo lo agrega la writer task. El blob va SIN cifrar AES
/// (el cliente aún no tiene la key); solo ofuscado con su GUID.
fn build_crypto_key(crypto: &proto_ares::AresCrypto, guid: &[u8; 16]) -> Bytes {
    let obf = crypto.to_obfuscated(guid);
    let mut inner = PacketWriter::with_msg(TcpMsg::ServerCryptoKey);
    inner.write_bytes(&obf).ok();
    let inner_bytes = inner.into_bytes(); // [230][48 bytes]
    let inner_size = (inner_bytes.len() - 1) as u16;

    let mut outer = PacketWriter::with_msg(TcpMsg::AdvancedFeatures);
    outer.write_u16_le(inner_size).ok();
    outer.write_bytes(&inner_bytes).ok();
    Bytes::copy_from_slice(outer.as_bytes())
}

fn build_login_ack(user: &server_core::user_pool::AresUser, room_name: &str) -> Bytes {
    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerLoginAck, user.ares_crypto);
    w.write_string_nt(&user.name.read()).ok();
    w.write_string_nt(room_name).ok();
    w.write_string_nt(&user.version).ok();
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

    let mut w = PacketWriter::with_msg_crypto(TcpMsg::ServerMyFeatures, user.ares_crypto);
    w.write_string_nt(&format!(
        "Astra {} - chat server - https://bsjaramillo.github.io/astra",
        env!("CARGO_PKG_VERSION")
    ))
    .ok();
    w.write_u8(flag).ok();
    w.write_u8(63).ok();
    w.write_u8(0).ok();
    w.write_u32_le(0).ok();
    w.write_u8(1).ok();
    Bytes::copy_from_slice(w.as_bytes())
}
