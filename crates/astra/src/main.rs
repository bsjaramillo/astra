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
mod directory;
mod update_check;

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
        /// URL del rooms.json a descargar. Si se omite, usa `seed_url` del
        /// archivo de configuración (que por defecto apunta al seed público).
        #[arg(long)]
        url: Option<String>,
    },
}

/// Descarga el `rooms.json` desde `url`, lo valida y lo escribe en
/// `seed_path`. Retorna la cantidad de rooms. NO toca la DB (eso lo hace el
/// caller con `load_seed`/`load_seed_force`).
async fn download_seed_to(url: &str, seed_path: &std::path::Path) -> anyhow::Result<usize> {
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
    let count = astra_udp::validate_seed(&body)
        .map_err(|e| anyhow::anyhow!("el JSON descargado no es un seed válido: {}", e))?;
    if let Some(parent) = seed_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(seed_path, &body)?;
    Ok(count)
}

/// Ejecuta `astra seed-refresh`: descarga el rooms.json, lo valida,
/// sobrescribe `<data_dir>/seed_rooms.json` y fuerza la recarga en la DB.
async fn seed_refresh(settings: &Settings, url: &str) -> anyhow::Result<()> {
    info!("descargando seed desde {}", url);
    let data_dir = std::path::PathBuf::from(&settings.data_dir);
    let seed_path = data_dir.join("seed_rooms.json");
    let count = download_seed_to(url, &seed_path).await?;
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
    let (mut settings, config_error) = Settings::load_reporting(&cli.config);
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

    // GUID del servidor para el Link. Si nunca se configuró (falta en el
    // toml, está vacío, o quedó el placeholder histórico), se genera uno
    // aleatorio y se persiste — paridad con sb0t, que hace lo mismo al
    // arrancar (`MainWindow.SetValues.cs`: `Guid.NewGuid()` + `Settings.Set`).
    //
    // No es cosmético: el `guid` es el secreto con el que un leaf se
    // autentica contra un hub. Compartido entre instalaciones no vale nada.
    //
    // Si el archivo existe pero no se pudo leer, se genera el guid en memoria
    // pero NO se persiste: escribir ahí sustituiría la configuración del
    // operador por los valores por defecto, que es justo lo que no se puede
    // hacer cuando lo único que sabemos es que hay algo mal en su archivo.
    let generated_guid = if settings.has_real_guid() {
        None
    } else {
        settings.regenerate_guid();
        if config_error.is_some() {
            None
        } else {
            match settings.save(&cli.config) {
                Ok(()) => Some(Ok(())),
                Err(e) => Some(Err(e)),
            }
        }
    };

    let web_enabled = settings.web_enabled && !cli.no_web;

    // Init tracing: a consola Y (si se puede) a archivo rotativo diario en
    // <data_dir>/logs/. El `WorkerGuard` del appender no-bloqueante debe vivir
    // todo el programa (si se dropea, se deja de vaciar el buffer al archivo).
    //
    // El log a archivo es BEST-EFFORT: si el directorio no se puede crear o no
    // es escribible (típico en Docker cuando el volumen pertenece a otro
    // usuario o está montado read-only), el server NO debe morir — degrada a
    // solo consola. Antes `rolling::daily` paniqueaba en ese caso.
    let log_level = if cli.verbose { "debug" } else { "info" };
    let logs_dir = std::path::Path::new(&settings.data_dir).join("logs");
    let file_layer_result = std::fs::create_dir_all(&logs_dir)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("astra.log")
                .build(&logs_dir)
                .map_err(|e| e.to_string())
        });
    let (file_layer, _log_guard) = match file_layer_result {
        Ok(appender) => {
            let (file_writer, guard) = tracing_appender::non_blocking(appender);
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer);
            (Some(layer), Some(guard))
        }
        Err(e) => {
            eprintln!(
                "WARN: no se pudo iniciar el log a archivo en {} ({}); usando solo consola",
                logs_dir.display(),
                e
            );
            (None, None)
        }
    };
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));
        tracing_subscriber::registry()
            .with(filter)
            // Consola (con colores).
            .with(tracing_subscriber::fmt::layer())
            // Archivo (sin colores). `Option<Layer>` = no-op si no hay archivo.
            .with(file_layer)
            .init();
    }
    if _log_guard.is_none() {
        warn!("logs solo a consola (el archivo no está disponible)");
    }

    info!("╔════════════════════════════════════════╗");
    info!("║      Astra Chat Server v{}         ║", env!("CARGO_PKG_VERSION"));
    info!("║   Compatible con Ares Galaxy           ║");
    info!("╚════════════════════════════════════════╝");
    info!("logs → {}/astra.log (rotación diaria)", logs_dir.display());

    // El archivo se leyó antes de que existiera el logging (hace falta
    // `data_dir` para saber dónde escribir los logs), así que el aviso se da
    // aquí. Va en ERROR y no en WARN a propósito: significa que el servidor
    // está corriendo con una configuración que NO es la que el operador
    // escribió, y eso incluye su contraseña de dueño y su puerto.
    if let Some(err) = &config_error {
        error!("{err}");
        error!(
            "ATENCIÓN: arrancando con la configuración POR DEFECTO. \
             Tu archivo no se ha modificado; corrige el error y reinicia."
        );
    }

    // Subcomandos: se ejecutan y terminan sin levantar el server.
    if let Some(Command::SeedRefresh { url }) = &cli.command {
        // `--url` gana; si se omite, se usa `seed_url` del config (o el default
        // público si el config no lo trae). Vacío = error explícito.
        let url = url.clone().unwrap_or_else(|| settings.seed_url.clone());
        if url.trim().is_empty() {
            anyhow::bail!("no hay URL de seed: pasa `--url <url>` o configura `seed_url` en el astra.toml");
        }
        return seed_refresh(&settings, &url).await;
    }

    info!("configuración cargada: puerto={}, sala='{}'", settings.port, settings.room_name);
    info!("data dir: {}", settings.data_dir);

    // Resultado de la generación del GUID (se hizo antes de tener logger).
    match generated_guid {
        None => {}
        Some(Ok(())) => info!(
            "GUID de servidor generado y guardado en {} (identifica esta sala en el Link)",
            cli.config.display()
        ),
        Some(Err(e)) => warn!(
            "GUID de servidor generado pero NO se pudo guardar en {}: {}. \
             Se usará solo en esta ejecución y cambiará al reiniciar: si tienes \
             el Link configurado, fija `guid` a mano en el archivo.",
            cli.config.display(),
            e
        ),
    }

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

    // Bots agente inteligentes (identidades propias, configurables desde el
    // panel). El gestor carga los persistidos, migra la config única legacy si
    // es la primera vez y queda inyectado para el CRUD del panel admin.
    // Los bots reciben el handle de scripting para los side-effects de los
    // comandos que ejecuten (se ejecutan con el nivel del solicitante).
    let bot_manager = astra_bot::BotManager::new(db.clone(), scripting.clone());
    bot_manager.load_all(&ctx);
    *ctx.bot_registry.write() = Some(bot_manager);
    let active: Vec<String> = ctx
        .bots
        .read()
        .iter()
        .filter(|b| b.is_enabled())
        .map(|b| b.bot_name())
        .collect();
    if !active.is_empty() {
        info!("bots agente activos: {}", active.join(", "));
    }
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
                                // Hub activo (paridad sb0t Server.Link: el leaf
                                // conoce el hub por el request de conexión).
                                *link_request_ctx.link_hub.write() =
                                    Some((name.clone(), addr.ip(), port));
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
                        {
                            let mut hub = link_request_ctx.link_hub.write();
                            if hub.as_ref().is_some_and(|(n, _, _)| n == &name) {
                                *hub = None;
                            }
                        }
                        info!("Link_disconnect: {}", name);
                    }
                    Ok(LinkRequest::KickHub { name }) => {
                        let mut links = link_request_ctx.link_servers.write();
                        links.retain(|(n, _p, _c)| n != &name);
                        let mut users = link_request_ctx.link_users.write();
                        users.retain(|(link, _)| link != &name);
                        {
                            let mut hub = link_request_ctx.link_hub.write();
                            if hub.as_ref().is_some_and(|(n, _, _)| n == &name) {
                                *hub = None;
                            }
                        }
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

        // Bootstrap automático: una sala NUEVA no tiene `seed_rooms.json` ni
        // nodos en la DB, así que no puede propagarse en la red UDP de Ares
        // (no aparecería en el buscador de salas de los clientes). Si el
        // room-search está activo y no hay ni seed ni nodos, se descarga el
        // seed inicial aquí — así "just works" sin correr `seed-refresh` a mano.
        let has_nodes = db.count_nodes().unwrap_or(0) > 0;
        let seed_url = settings.seed_url.trim();
        if !seed_path.exists() && !has_nodes && !seed_url.is_empty() {
            info!(
                "room-search: sin seed ni nodos; descargando seed inicial de {} ...",
                seed_url
            );
            match download_seed_to(seed_url, &seed_path).await {
                Ok(n) => info!("seed inicial descargado: {} rooms", n),
                Err(e) => warn!(
                    "no se pudo descargar el seed inicial ({}); la sala no aparecerá \
                     en el buscador hasta cargar un seed (correr `astra seed-refresh` \
                     o dejar `seed_rooms.json` en el data dir)",
                    e
                ),
            }
        } else if !seed_path.exists() && !has_nodes && seed_url.is_empty() {
            info!("room-search: descarga automática de seed desactivada (seed_url vacío)");
        }

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

        // IP externa reportada por la red Ares → AppContext (para
        // `Room.externalIp`/`Room.hashlink` del scripting, paridad
        // sb0t Settings.ExternalIP).
        let ext_ip_ctx = ctx.clone();
        mgr.set_on_external_ip(std::sync::Arc::new(move |ip| {
            *ext_ip_ctx.external_ip.write() = Some(ip);
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
                // Hub activo para Server.Link/scripting (nombre = addr, ya
                // que --link-client no lleva nombre de sala del hub).
                *link_ctx.link_hub.write() =
                    Some((link_addr.clone(), addr.ip(), addr.port()));
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
    // Chequeo de nuevas versiones (cada 6h contra ghcr.io): avisa por PM a
    // los admins/owners conectados y lo marca para `/admin`.
    if ctx.settings.update_check {
        tokio::spawn(update_check::check_loop(ctx.clone()));
    }

    // Publicación en el directorio público de salas. Opt-in: sin
    // `[directory] enabled = true` no sale ninguna petición de aquí.
    if ctx.settings.directory.enabled {
        // La credencial guardada, si la hay, se pasa a memoria: a partir de
        // ahí manda la de memoria, que es la que sobrevive a un config de
        // solo lectura.
        if !ctx.settings.directory.token.is_empty() {
            *ctx.directory_token.write() = Some(ctx.settings.directory.token.clone());
        }
        tokio::spawn(directory::heartbeat_loop(ctx.clone()));
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

    // Expiración de muzzles (cada 60s, paridad `Muzzles.Tick` de sb0t): los
    // muzzles puestos con `#muzzle` caducan pasado el `#mtimeout` de la sala y
    // la expiración se ANUNCIA a todos (Timeouts#1). `is_muzzled()` ya expira
    // de forma perezosa; este barrido es el que emite el aviso.
    let muzzle_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            let now = server_core::time::unix_time();
            for u in muzzle_ctx.user_pool.users() {
                if !u.muzzled.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                let until = u.muzzle_until.load(std::sync::atomic::Ordering::Relaxed);
                if until == 0 || now < until {
                    continue;
                }
                u.muzzled.store(false, std::sync::atomic::Ordering::Relaxed);
                u.muzzle_until
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                let name = u.name.read().clone();
                muzzle_ctx.publish_link_event(server_core::LinkEvent::UserUpdated {
                    origin: None,
                    user: server_core::LinkUserSnapshot::from_user(&u),
                });
                muzzle_ctx.broadcast_print(
                    &muzzle_ctx
                        .templates
                        .render("timeouts.muzzle_expired", &[("+n", &name)]),
                );
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
                // Hub habilitado por CLI (--link-server) O por config
                // (link_hub_enabled): antes solo el flag CLI contaba y el
                // setting del toml se ignoraba en silencio.
                let link_enabled = cli.link_server || ctx.settings.link_hub_enabled;
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
            // NOTA: el rate-limit de conexiones por IP para el path web NO se
            // aplica aquí, sino DENTRO de `astra_web` (en el handshake WS), y
            // SOLO a los handshakes WebSocket de clientes de sala. Las
            // peticiones HTTP del panel (GET /, /admin, /favicon y el polling
            // `fetch` cada 5s del panel de admin) NO deben contar como
            // "conexiones nuevas" — si no, el propio administrador se
            // auto-banea por hacer polling. `counted` (exención de proxies)
            // se resuelve allí con la misma regla.
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
