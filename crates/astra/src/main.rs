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

use clap::Parser;
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
    let web_port = settings.web_port;

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

        // El UDP listener comparte el socket con el prober
        let udp_bind = format!("0.0.0.0:{}", settings.port);
        let udp_socket = UdpSocket::bind(udp_bind.parse::<SocketAddr>()?).await?;
        info!("UDP socket bindeado en {}", udp_bind);
        let udp_socket = Arc::new(udp_socket);

        // Spawn listener (comparte socket)
        let listener_mgr = mgr.clone();
        let listener_socket = udp_socket.clone();
        tokio::spawn(async move {
            if let Err(e) = astra_udp::run_listener(listener_mgr, listener_socket).await {
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

    // Iniciar WebSocket server (clientes ib0t/HTML5)
    if web_enabled {
        let web_ctx = ctx.clone();
        tokio::spawn(async move {
            let server = astra_web::WsServer::new(web_ctx, web_port);
            if let Err(e) = server.serve().await {
                error!("WS server crashed: {}", e);
            }
        });
    } else {
        warn!("WebSocket server desactivado");
    }

    // Iniciar Link Server (hub) si está habilitado
    if cli.link_server {
        let link_ctx = ctx.clone();
        let link_port = settings.link_hub_port;
        tokio::spawn(async move {
            let server = std::sync::Arc::new(astra_link::LinkServer::new(link_ctx));
            if let Err(e) = server.start(link_port).await {
                error!("link server crashed: {}", e);
            }
        });
        info!("link server: iniciado en puerto {}", settings.link_hub_port);
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
    // Stats reporter (cada 30s) + cleanup periódico
    let stats_ctx = ctx.clone();
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
        }
    });

    // Accept loop
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let ctx = ctx.clone();
                let scripting = scripting.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_client(ctx, stream, peer, scripting).await {
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
