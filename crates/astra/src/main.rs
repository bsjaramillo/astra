//! # astra
//!
//! Binario principal del servidor de chat Astra.
//!
//! ## Uso
//!
//! ```bash
//! astra                              # Inicia con configuración por defecto
//! astra --port 5009 --config astra.toml
//! astra --no-roomsearch              # Desactiva room search UDP
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use server_core::{db::Database, settings::Settings, AppContext};
use tokio::net::{TcpListener, UdpSocket};
use tracing::{debug, error, info, warn};

mod tcp_handler;

use tcp_handler::handle_tcp_client;

/// Servidor de chat Astra — compatible con Ares Galaxy.
#[derive(Parser, Debug)]
#[command(name = "astra", version, about = "Servidor de chat compatible con Ares Galaxy")]
struct Cli {
    /// Puerto TCP principal. Si se omite, se usa el `port` del archivo de
    /// configuración (`--config`); si tampoco está ahí, el default de
    /// `Settings` (5009).
    #[arg(short, long)]
    port: Option<u16>,

    /// Archivo de configuración TOML.
    #[arg(short, long, default_value = "astra.toml")]
    config: PathBuf,

    /// Desactiva room search UDP.
    #[arg(long)]
    no_roomsearch: bool,

    /// Desactiva el panel web.
    #[arg(long)]
    no_web: bool,

    /// Directorio de datos (DB, logs, seed). Default: ./data
    #[arg(long)]
    data_dir: Option<String>,

    /// Activar como Link Server (hub). Escucha conexiones de otros servers.
    #[arg(long)]
    link_server: bool,

    /// Activar como Link Client (leaf). Conecta a un hub en la dirección dada.
    /// Formato: ip:port
    #[arg(long, value_name = "ADDR")]
    link_client: Option<String>,

    /// Modo verbose (más logs).
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

/// Subcomandos del CLI.
#[derive(Subcommand, Debug)]
enum Command {
    /// Descarga la lista de rooms y regenera el seed local + la DB de nodos.
    SeedRefresh {
        /// URL del rooms.json a descargar.
        #[arg(long, default_value = "http://chatrooms.mywire.org/rooms.json")]
        url: String,
    },
}

/// Ejecuta `astra seed-refresh`: descarga el rooms.json, lo valida,
/// sobrescribe `<data_dir>/seed_rooms.json` y fuerza la recarga en la DB.
async fn seed_refresh(settings: &Settings, url: &str) -> anyhow::Result<()> {
    info!("descargando seed desde {}", url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let body = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Validar antes de sobrescribir el archivo local
    let count = astra_udp::validate_seed(&body)
        .map_err(|e| anyhow::anyhow!("el JSON descargado no es un seed válido: {}", e))?;

    let data_dir = std::path::PathBuf::from(&settings.data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let seed_path = data_dir.join("seed_rooms.json");
    std::fs::write(&seed_path, &body)?;
    info!("seed guardado en {} ({} rooms)", seed_path.display(), count);

    let db_path = data_dir.join("astra.db");
    let db = Database::open(&db_path)
        .map_err(|e| anyhow::anyhow!("error abriendo DB en {}: {}", db_path.display(), e))?;
    let stats = astra_udp::load_seed_force(&db, &seed_path)?;
    info!(
        "DB actualizada: {} nodos, {} rooms, {} errores",
        stats.nodes_added,
        stats.rooms_added,
        stats.errors.len()
    );
    if !stats.errors.is_empty() {
        warn!("errores del seed: {:?}", stats.errors);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Cargar configuración PRIMERO: necesitamos `data_dir` para saber dónde
    // escribir el archivo de log.
    let mut settings = Settings::load_or_default(&cli.config);
    if let Some(port) = cli.port {
        settings.port = port;
    }
    if cli.no_roomsearch {
        settings.roomsearch = false;
    }
    if cli.no_web {
        settings.web_enabled = false;
    }
    if let Some(d) = &cli.data_dir {
        settings.data_dir = d.clone();
    }
    let web_enabled = settings.web_enabled && !cli.no_web;

    // Init tracing: a consola Y a archivo rotativo diario en <data_dir>/logs/.
    // El `WorkerGuard` del appender no-bloqueante debe vivir todo el programa
    // (si se dropea, se deja de vaciar el buffer al archivo).
    let log_level = if cli.verbose { "debug" } else { "info" };
    let logs_dir = std::path::Path::new(&settings.data_dir).join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "astra.log");
    let (file_writer, _log_guard) = tracing_appender::non_blocking(file_appender);
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));
        tracing_subscriber::registry()
            .with(filter)
            // Consola (con colores).
            .with(tracing_subscriber::fmt::layer())
            // Archivo (sin códigos ANSI de color).
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(file_writer),
            )
            .init();
    }

    info!("╔════════════════════════════════════════╗");
    info!("║      Astra Chat Server v{}         ║", env!("CARGO_PKG_VERSION"));
    info!("║   Compatible con Ares Galaxy           ║");
    info!("╚════════════════════════════════════════╝");
    info!("logs → {}/astra.log (rotación diaria)", logs_dir.display());

    // Subcomandos: se ejecutan y terminan sin levantar el server.
    if let Some(Command::SeedRefresh { url }) = &cli.command {
        return seed_refresh(&settings, url).await;
    }

    info!("configuración cargada: puerto={}, sala='{}'", settings.port, settings.room_name);
    info!("data dir: {}", settings.data_dir);

    // Abrir/crear la base de datos SQLite
    let db_path = std::path::PathBuf::from(&settings.data_dir).join("astra.db");
    let db = Database::open(&db_path)
        .map_err(|e| anyhow::anyhow!("error abriendo DB en {}: {}", db_path.display(), e))?;
    info!("base de datos SQLite abierta en {}", db_path.display());

    // Crear contexto de aplicación
    let ctx = Arc::new(AppContext::new(settings.clone(), db.clone()));
    // Registrar la ruta del config para que el panel admin pueda editarlo.
    ctx.set_config_path(cli.config.clone());
    info!(
        "contexto de aplicación inicializado ({} bans cargados)",
        ctx.bans.len()
    );

    // Inicializar sistema de scripting (boa_engine)
    let scripts_dir = std::path::PathBuf::from(&settings.data_dir).join("scripts");
    let scripting_manager = astra_scripting::ScriptManager::new(ctx.clone(), scripts_dir.clone());
    let scripting = scripting_manager.start_in_thread();
    info!(
        "scripting inicializado en {} (handle Send + Clone para dispatchear eventos)",
        scripts_dir.display()
    );
    // Gate de vroom (onVroomJoinCheck): server-core no puede depender del
    // scripting, así que se inyecta como closure (mismo patrón que
    // ScriptingHooks).
    {
        let h = scripting.clone();
        ctx.set_vroom_check(Box::new(move |name, vroom| h.check_vroom_join(name, vroom)));
    }

    // Hooks para que /listscripts, /loadscript y /killscript (crates/commands)
    // puedan hablar con el ScriptManager sin que server-core dependa de
    // astra_scripting (sería circular, ya que astra_scripting depende de
    // server_core::AppContext).
    {
        let h1 = scripting.clone();
        let h2 = scripting.clone();
        let h3 = scripting.clone();
        *ctx.scripting_hooks.write() = Some(server_core::ScriptingHooks {
            list: std::sync::Arc::new(move || h1.list_scripts()),
            load: std::sync::Arc::new(move |name: &str| h2.load_script(name)),
            kill: std::sync::Arc::new(move |name: &str| h3.kill_script(name)),
        });
    }

    // Bridge LinkEvent → ScriptEvent: reenvía TODOS los eventos del bus
    // (Linked/Unlinked/LinkError/LeafJoin/LeafPart/Join/Part/...) a los scripts.
    {
        let scripting_for_bridge = scripting.clone();
        let mut link_events_rx = ctx.subscribe_link_events();
        let bridge_ctx = ctx.clone();
        tokio::spawn(async move {
            use server_core::LinkEvent;
            use astra_scripting::ScriptEvent;
            loop {
                match link_events_rx.recv().await {
                    Ok(event) => {
                        let script_event = match &event {
                            LinkEvent::Part { name, .. } => Some(ScriptEvent::LeafPart { name: name.clone() }),
                            LinkEvent::Join { user, .. } => {
                                Some(ScriptEvent::LeafJoin { name: user.name.clone() })
                            }
                            _ => None,
                        };
                        if let Some(se) = script_event {
                            scripting_for_bridge.dispatch(se);
                        }
                        // Actualizar snapshot link_servers y link_users
                        match &event {
                            LinkEvent::Join { user, origin } => {
                                if let Some(o) = origin {
                                    let mut users = bridge_ctx.link_users.write();
                                    let entry = (o.clone(), user.name.clone());
                                    if !users.contains(&entry) {
                                        users.push(entry);
                                    }
                                }
                            }
                            LinkEvent::Part { name, origin } => {
                                if let Some(o) = origin {
                                    let mut users = bridge_ctx.link_users.write();
                                    users.retain(|(link, _)| link != o);
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("scripting bridge: se perdieron {} eventos Link", skipped);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // Consumer de LinkRequest: procesa CreateLink/DisconnectLink/KickHub
    {
        let link_request_ctx = ctx.clone();
        let mut link_req_rx = link_request_ctx.link_requests.subscribe();
        tokio::spawn(async move {
            use server_core::LinkRequest;
            loop {
                match link_req_rx.recv().await {
                    Ok(LinkRequest::CreateLink { name, server, port }) => {
                        // Parsear "host:port" y conectar
                        let addr_str = format!("{}:{}", server, port);
                        match addr_str.parse::<std::net::SocketAddr>() {
                            Ok(addr) => {
                                let client = std::sync::Arc::new(astra_link::LinkClient::new(link_request_ctx.clone()));
                                // Actualizar link_servers snapshot
                                {
                                    let mut links = link_request_ctx.link_servers.write();
                                    links.retain(|(n, _, _)| n != &name);
                                    links.push((name.clone(), port, true));
                                }
                                tokio::spawn(async move {
                                    client.run(addr).await;
                                });
                                info!("Link_createLink: {} -> {}", name, addr);
                            }
                            Err(e) => {
                                warn!("Link_createLink: addr inválida {}: {}", addr_str, e);
                            }
                        }
                    }
                    Ok(LinkRequest::DisconnectLink { name }) => {
                        // Para un disconnect real, necesitaríamos trackear el
                        // active flag del LinkClient. Por ahora marcamos disconnected
                        // y el thread del LinkClient se encargará.
                        let mut links = link_request_ctx.link_servers.write();
                        for entry in links.iter_mut() {
                            if entry.0 == name {
                                entry.2 = false;
                            }
                        }
                        info!("Link_disconnect: {}", name);
                    }
                    Ok(LinkRequest::KickHub { name }) => {
                        let mut links = link_request_ctx.link_servers.write();
                        links.retain(|(n, _p, _c)| n != &name);
                        let mut users = link_request_ctx.link_users.write();
                        users.retain(|(link, _)| link != &name);
                        info!("Link_kickHub: {}", name);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("link request consumer: se perdieron {} requests", skipped);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // Inicializar sistema UDP de room search
    let udp_manager = if settings.roomsearch {
        // Cargar seed ANTES de crear el manager (para que el cache esté poblado)
        let seed_path = std::path::PathBuf::from(&settings.data_dir).join("seed_rooms.json");
        match astra_udp::load_seed(&db, &seed_path) {
            Ok(stats) => {
                if stats.nodes_added > 0 {
                    info!("seed cargado: {} nodos, {} rooms", stats.nodes_added, stats.rooms_added);
                }
                if !stats.errors.is_empty() {
                    warn!("errores en seed: {:?}", stats.errors);
                }
            }
            Err(e) => warn!("error cargando seed: {}", e),
        }

        // Ahora sí, crear el manager (lee de la DB ya con seed)
        let mgr = astra_udp::UdpNodeManager::new(db.clone(), settings.port);
        let mgr = Arc::new(mgr);
        info!(
            "UDP manager inicializado: {} nodos, {} rooms",
            mgr.count_nodes(),
            mgr.count_rooms()
        );

        // Wirear el callback para sincronizar con AppContext.udp_nodes
        // (snapshot para el scripting JS).
        let udp_nodes_ctx = ctx.clone();
        mgr.set_on_change(std::sync::Arc::new(move |snapshots: &[astra_udp::NodeSnapshot]| {
            *udp_nodes_ctx.udp_nodes.write() = snapshots
                .iter()
                .map(|s| (s.name.clone(), s.port, s.users))
                .collect();
        }));

        // El UDP listener comparte el socket con el prober
        let udp_bind = format!("0.0.0.0:{}", settings.port);
        let udp_socket = UdpSocket::bind(udp_bind.parse::<SocketAddr>()?).await?;
        info!("UDP socket bindeado en {}", udp_bind);
        let udp_socket = Arc::new(udp_socket);

        // Spawn listener (comparte socket)
        let listener_mgr = mgr.clone();
        let listener_socket = udp_socket.clone();
        let listener_pool = ctx.user_pool.clone();
        let user_count: astra_udp::UserCountFn =
            Arc::new(move || listener_pool.len().min(u16::MAX as usize) as u16);
        let room_info_ctx = ctx.clone();
        let room_info: astra_udp::RoomInfoFn = Arc::new(move || {
            (
                room_info_ctx.settings.room_name.clone(),
                room_info_ctx.current_room_topic(),
            )
        });
        tokio::spawn(async move {
            if let Err(e) =
                astra_udp::run_listener(listener_mgr, listener_socket, user_count, room_info).await
            {
                error!("UDP listener crashed: {}", e);
            }
        });

        // Spawn prober (mismo socket)
        let prober_mgr = mgr.clone();
        let prober_socket = udp_socket.clone();
        tokio::spawn(async move {
            astra_udp::run_prober(prober_mgr, prober_socket).await;
        });

        Some(mgr)
    } else {
        warn!("room search UDP desactivado");
        None
    };

    // Iniciar TCP listener
    let bind_addr: SocketAddr = format!("0.0.0.0:{}", settings.port).parse()?;
    let listener = TcpListener::bind(bind_addr).await?;
    info!("TCP listener escuchando en {}", bind_addr);

    if web_enabled {
        info!("WebSocket habilitado en el mismo puerto TCP principal: {}", settings.port);
    } else {
        warn!("WebSocket desactivado");
    }

    if cli.link_server {
        info!("Link habilitado en el mismo puerto TCP principal: {}", settings.port);
    }

    // Iniciar Link Client (leaf) si se especificó --link-client
    if let Some(link_addr) = cli.link_client.clone() {
        let link_ctx = ctx.clone();
        match link_addr.parse::<std::net::SocketAddr>() {
            Ok(addr) => {
                let client = std::sync::Arc::new(astra_link::LinkClient::new(link_ctx));
                tokio::spawn(async move {
                    client.run(addr).await;
                });
                info!("link client: iniciado, conectando a {}", addr);
            }
            Err(e) => {
                error!("link client: dirección inválida '{}': {}", link_addr, e);
            }
        }
    }
    // Rotación de URLs de la sala (cada 60s): difunde el siguiente banner
    // clicable a todos los usuarios conectados (paridad con sb0t Urls.Tick).
    let url_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.tick().await; // primer tick inmediato: skip
        loop {
            interval.tick().await;
            if let Some(item) = url_ctx.urls.next_url() {
                let ws_msg = astra_web::protocol::build_url(&item.address, &item.text);
                for u in url_ctx.user_pool.users() {
                    if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                        continue;
                    }
                    if let Some(tx) = &u.ws_text_sender {
                        let _ = tx.send(ws_msg.clone());
                    } else {
                        let _ = u.send(server_core::outbound::build_url_c(
                            &item.address,
                            &item.text,
                            u.ares_crypto,
                        ));
                    }
                }
            }
        }
    });

    // Clock de sala (cada 60s): si el flag `clock` está on, difunde la hora
    // como topic (paridad con sb0t Topics.EnableClock).
    let clock_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            if clock_ctx.room_flags.get("clock") {
                let now = chrono::Utc::now().format("%H:%M UTC").to_string();
                let base = clock_ctx.settings.room_topic.clone();
                let topic = format!("{} [{}]", base, now);
                clock_ctx.set_room_topic(topic.clone());
                let ws_msg = astra_web::protocol::build_topic(&topic);
                for u in clock_ctx.user_pool.users() {
                    if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                        continue;
                    }
                    if let Some(tx) = &u.ws_text_sender {
                        let _ = tx.send(ws_msg.clone());
                    } else {
                        let _ = u.send(server_core::outbound::build_topic_c(&topic, u.ares_crypto));
                    }
                }
            }
        }
    });

    // RoomInfo periódico (cada 20 min): si el flag `roominfo` está on,
    // difunde el bloque de info de sala (paridad sb0t RoomInfo.Tick).
    let roominfo_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1200));
        interval.tick().await;
        loop {
            interval.tick().await;
            if roominfo_ctx.room_flags.get("roominfo") {
                for line in astra_commands::roominfo_lines(&roominfo_ctx) {
                    roominfo_ctx.broadcast_print(&line);
                }
            }
        }
    });

    // FastPing periódico (cada 2s) a todos los clientes Ares TCP logueados.
    // Paridad `ServerCore.cs` de sb0t: el server les manda esto a los
    // clientes para (a) mantener viva la conexión contra NAT/firewalls
    // intermedios que reciclan mappings ociosos, y (b) fallar rápido (error
    // de escritura) si el socket ya está muerto. Sin este ping, un cliente
    // que se queda leyendo sin escribir nada puede perder la conexión en
    // silencio (el NAT la recicla) sin que ni el cliente ni el server lo
    // noten hasta mucho después. No aplica a clientes web (usan su propio
    // ident PING/PONG de WebSocket).
    let fastping_ctx = ctx.clone();
    tokio::spawn(async move {
        let pkt = bytes::Bytes::from(
            proto_ares::PacketWriter::with_msg(proto_ares::TcpMsg::FastPing).into_bytes(),
        );
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;
            for u in fastping_ctx.user_pool.users() {
                if u.logged_in && !u.web_client {
                    let _ = u.send(pkt.clone());
                }
            }
        }
    });

    // Avatar default (cada 2s): paridad `Avatars.CheckAvatars` de sb0t —
    // a cualquier cliente Ares nativo logueado que lleve >10s conectado sin
    // haber mandado su propio avatar, se le asigna el avatar default (si
    // hay uno configurado desde el panel) y se difunde el cambio como un
    // avatar en vivo más. No aplica a clientes web (sb0t tampoco lo hace:
    // `CheckAvatars` solo recorre `UserPool.AUsers`).
    let avatar_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;
            let Some(default_bytes) = avatar_ctx.default_avatar.read().clone() else {
                continue;
            };
            let now_ms = server_core::time::unix_time();
            for u in avatar_ctx.user_pool.users() {
                if u.web_client
                    || !u.logged_in
                    || u.avatar_received.load(std::sync::atomic::Ordering::Relaxed)
                    || now_ms.saturating_sub(u.join_time) < 10_000
                {
                    continue;
                }
                *u.avatar.lock() = Some(default_bytes.clone());
                u.avatar_received.store(true, std::sync::atomic::Ordering::Relaxed);
                let name = u.name.read().clone();
                let bytes = default_bytes.clone();
                tcp_handler::broadcast_to_room(&avatar_ctx, &u, move |c| {
                    server_core::outbound::build_avatar_c(&name, &bytes, c)
                });
            }
        }
    });

    // Stats reporter (cada 30s) + cleanup periódico
    let stats_ctx = ctx.clone();
    let stats_scripting = scripting.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let udp_info = if let Some(m) = &udp_manager {
                use std::sync::atomic::Ordering;
                format!(
                    ", udp_nodos={}, udp_rooms={}, udp_addips_sent={}, udp_addips_recv={}",
                    m.count_nodes(),
                    m.count_rooms(),
                    m.stats.addips_sent.load(Ordering::Relaxed),
                    m.stats.addips_recv.load(Ordering::Relaxed),
                )
            } else {
                String::new()
            };
            info!(
                "stats: users={}, peak={}, total={}, bans={}, uptime={}s, bytes_in={}, bytes_out={}{}",
                stats_ctx.user_pool.len(),
                stats_ctx.stats.peak_users(),
                stats_ctx.stats.total_users(),
                stats_ctx.bans.len(),
                stats_ctx.uptime_secs(),
                stats_ctx.stats.bytes_in(),
                stats_ctx.stats.bytes_out(),
                udp_info,
            );
            // Cleanup user history cache + podar DB (mantener 30 días)
            let now_ms = server_core::time::unix_time();
            stats_ctx.user_history.cleanup(now_ms);
            stats_ctx.user_history.prune(30 * 24 * 60 * 60);
            // Cleanup de las 5 capas de seguridad
            stats_ctx.security.cleanup();
            // Prune de bans expirados + dispatch de BansAutoCleared
            let pruned = stats_ctx.bans.prune_expired();
            if pruned > 0 {
                info!("bans: pruned {} expired bans", pruned);
                stats_scripting.dispatch(astra_scripting::ScriptEvent::BansAutoCleared);
            }
        }
    });

    // NOTA: no hay auto-idle por inactividad — en sb0t el idle es siempre
    // una acción manual del usuario (comando `idle`/`idles` o emote que
    // empieza con "idles"); ver server-core/src/idle.rs.

    // Accept loop
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let ctx = ctx.clone();
                let scripting = scripting.clone();
                let web_enabled = web_enabled;
                let link_enabled = cli.link_server;
                tokio::spawn(async move {
                    if let Err(e) = handle_muxed_connection(ctx, stream, peer, scripting, web_enabled, link_enabled).await {
                        warn!("cliente {} error: {}", peer, e);
                    }
                });
            }
            Err(e) => {
                error!("accept error: {}", e);
            }
        }
    }
}

async fn handle_muxed_connection(
    ctx: Arc<AppContext>,
    mut stream: tokio::net::TcpStream,
    peer: SocketAddr,
    scripting: astra_scripting::ScriptHandle,
    web_enabled: bool,
    link_enabled: bool,
) -> anyhow::Result<()> {
    let ip = peer.ip();

    // ── FIX DDoS #2: cap de conexiones CRUDAS por IP, ANTES de clasificar.
    // Así también cuentan las conexiones que todavía no mandaron ni un byte
    // (Slowloris). Es un límite ALTO (default 30) — separado del límite de 5
    // concurrentes de clientes nativos — para no romper la web detrás de un
    // proxy (todos los usuarios web comparten la IP del proxy). Se exime a
    // proxies reversos confiables y loopback.
    let counted = !ctx.trusted_proxies.is_trusted(ip);
    if counted && !ctx.security.raw_conn.try_acquire(ip) {
        warn!("REJECTED (cap de conexiones crudas por IP, anti-Slowloris): {}", peer);
        let _ = tcp_handler::send_server_error_to_stream(
            stream,
            "Too many simultaneous connections from your IP.",
        )
        .await;
        return Ok(());
    }
    let sec = ctx.security.clone();
    let _conn_guard = scopeguard::guard((), move |_| {
        if counted {
            sec.on_raw_disconnect(ip);
        }
    });

    // ── FIX DDoS #1: timeout en la clasificación. Una conexión que no manda el
    // primer byte dentro de `handshake_timeout_secs` se cierra, en vez de
    // quedarse colgada en el peek para siempre (Slowloris de conexiones mudas).
    let mut peek = [0u8; 16];
    let peek_timeout =
        std::time::Duration::from_secs(ctx.settings.security.handshake_timeout_secs.max(1));
    let n = match tokio::time::timeout(peek_timeout, stream.peek(&mut peek)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            debug!("peek falló para {}: {}", peer, e);
            return Ok(());
        }
        Err(_) => {
            debug!("clasificación: timeout esperando el primer byte de {}", peer);
            return Ok(());
        }
    };
    let route = classify_connection(&peek[..n]);

    match route {
        ConnectionKind::Web if web_enabled => {
            // Rate-limit de conexiones nuevas por IP para el path web (paridad
            // con el path Ares nativo, que ya lo aplica en `check_new_connection`).
            // Sin esto, un cliente web que reconecta en bucle (o un flood)
            // martillaba el server sin freno: el cap de concurrentes no lo
            // frena porque abre y cierra rápido. Se exime a proxies reversos
            // confiables y loopback (detrás de un proxy todos los usuarios web
            // comparten IP, así que ahí NO se puede limitar por IP).
            if counted {
                if let Some(reason) = ctx.security.conn_flood.check(ip) {
                    warn!(
                        "REJECTED web (rate limit de conexiones por IP): {} — {}",
                        peer,
                        reason.message()
                    );
                    return Ok(());
                }
            }
            astra_web::handle_stream(ctx, stream, peer, scripting).await?;
        }
        ConnectionKind::Link if link_enabled => {
            astra_link::handle_stream(ctx, stream).await.map_err(|e| anyhow::anyhow!(e))?;
        }
        _ => {
            handle_tcp_client(ctx, stream, peer, scripting).await?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionKind {
    Web,
    Link,
    Ares,
}

fn classify_connection(buf: &[u8]) -> ConnectionKind {
    if looks_like_http(buf) {
        return ConnectionKind::Web;
    }
    if looks_like_link(buf) {
        return ConnectionKind::Link;
    }
    ConnectionKind::Ares
}

fn looks_like_http(buf: &[u8]) -> bool {
    matches!(
        buf,
        [b'G', b'E', b'T', b' ', ..]
            | [b'P', b'O', b'S', b'T', ..]
            | [b'H', b'E', b'A', b'D', ..]
            | [b'O', b'P', b'T', b'I', ..]
            | [b'C', b'O', b'N', b'N', ..]
    )
}

fn looks_like_link(buf: &[u8]) -> bool {
    buf.len() >= 3 && buf[2] == astra_link::protocol::MSG_LINK_PROTO
}
