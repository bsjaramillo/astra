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
use tracing::{error, info, warn};

mod tcp_handler;

use tcp_handler::handle_tcp_client;

/// Servidor de chat Astra — compatible con Ares Galaxy.
#[derive(Parser, Debug)]
#[command(name = "astra", version, about = "Servidor de chat compatible con Ares Galaxy")]
struct Cli {
    /// Puerto TCP principal.
    #[arg(short, long, default_value_t = 5009)]
    port: u16,

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

    // Init tracing
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();

    info!("╔════════════════════════════════════════╗");
    info!("║      Astra Chat Server v{}         ║", env!("CARGO_PKG_VERSION"));
    info!("║   Compatible con Ares Galaxy           ║");
    info!("╚════════════════════════════════════════╝");

    // Cargar configuración
    let mut settings = Settings::load_or_default(&cli.config);
    settings.port = cli.port;
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
        tokio::spawn(async move {
            if let Err(e) =
                astra_udp::run_listener(listener_mgr, listener_socket, user_count).await
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
                let pkt = server_core::outbound::build_url(&item.address, &item.text);
                for u in url_ctx.user_pool.users() {
                    if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = u.send(pkt.clone());
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
                let pkt = server_core::outbound::build_topic(&topic);
                for u in clock_ctx.user_pool.users() {
                    if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = u.send(pkt.clone());
                    }
                }
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
                format!(", udp_nodos={}, udp_rooms={}", m.count_nodes(), m.count_rooms())
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

    // Idle detector (cada 60s) — verifica users idle y dispara onIdled
    let idle_ctx = ctx.clone();
    let idle_scripting = scripting.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            // Chequear cada user en el pool
            let mut to_idle = Vec::new();
            for u in idle_ctx.user_pool.users() {
                if !u.logged_in {
                    continue;
                }
                // check_idle retorna Some(()) si pasó de active a idle
                if idle_ctx.idle.check_idle(u.id).is_some() {
                    to_idle.push(u.name.read().clone());
                }
            }
            for name in to_idle {
                idle_scripting.dispatch(astra_scripting::ScriptEvent::Idled { name });
            }
        }
    });

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
    let mut peek = [0u8; 16];
    let n = stream.peek(&mut peek).await.unwrap_or(0);
    let route = classify_connection(&peek[..n]);

    match route {
        ConnectionKind::Web if web_enabled => {
            astra_web::handle_stream(ctx, stream, peer).await?;
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
