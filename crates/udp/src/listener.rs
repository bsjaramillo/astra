//! Listener UDP async: recibe paquetes y dispatcha al manager.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use proto_ares::{UdpMsg, UdpPacketReader};
use server_core::time::unix_time;
use tokio::net::UdpSocket;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::manager::UdpNodeManager;
use crate::protocol;
use crate::types::NodeAddr;

/// Provider del user count actual (inyectado por el binario para no
/// acoplar este crate al `UserPool` de server-core).
pub type UserCountFn = Arc<dyn Fn() -> u16 + Send + Sync>;

/// Loop principal del listener UDP.
///
/// El caller debe haber bindeado el socket al puerto deseado.
/// El listener comparte el socket con el prober (ambos pueden send/recv).
///
/// Además, ejecuta tareas periódicas:
/// - cada 1 min: `UdpNodeManager::expire()` para limpiar nodos muertos
/// - cada 15 min: log de stats
pub async fn run_listener(
    manager: Arc<UdpNodeManager>,
    socket: Arc<UdpSocket>,
    user_count: UserCountFn,
) -> anyhow::Result<()> {
    let local = socket.local_addr().ok();
    if let Some(addr) = local {
        info!("UDP listener activo en {}", addr);
    }

    let mut buf = vec![0u8; 1500];
    let mut tick = interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((n, peer)) => {
                        handle_packet(&socket, &manager, &buf[..n], peer, &user_count).await;
                        let _ = n;
                    }
                    Err(e) => {
                        warn!("UDP recv error: {}", e);
                    }
                }
            }
            _ = tick.tick() => {
                let now = unix_time() as i64;
                manager.expire(now);
                let mins = 15;
                if now % (mins * 60 * 1000) < 60_000 {
                    manager.stats.log_summary();
                }
            }
        }
    }
}

/// Procesa un paquete UDP entrante.
async fn handle_packet(
    socket: &UdpSocket,
    manager: &Arc<UdpNodeManager>,
    data: &[u8],
    peer: SocketAddr,
    user_count: &UserCountFn,
) {
    if data.is_empty() {
        return;
    }
    let opcode = data[0];
    let payload = &data[1..];
    let msg = match UdpMsg::from_u8(opcode) {
        Some(m) => m,
        None => {
            warn!("UDP opcode desconocido {} de {}", opcode, peer);
            return;
        }
    };

    debug!("UDP opcode: {:?} de {}", msg, peer);
    let now = unix_time() as i64;

    match msg {
        UdpMsg::ServerListSendInfo => {
            manager.stats.sendinfo_recv.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            handle_send_info(socket, manager, peer, now, user_count()).await;
        }
        UdpMsg::ServerListAckInfo => {
            handle_ack_info(manager, payload, peer, now);
        }
        UdpMsg::ServerListAddIps => {
            manager.stats.addips_recv.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            handle_add_ips(manager, payload, peer, now);
        }
        UdpMsg::ServerListAckIps => {
            manager.stats.ackips_recv.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            handle_ack_ips(manager, payload, peer, now);
        }
        UdpMsg::ServerListSendNodes => {
            manager.stats.sendnodes_recv.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            handle_send_nodes(socket, manager, peer, now).await;
        }
        UdpMsg::ServerListWantCheckFirewall => {
            handle_want_check_firewall(socket, manager, peer, now).await;
        }
        UdpMsg::ServerListReadyToCheckFirewall => {
            if let Some((cookie, their_ip)) = protocol::parse_ready_check_firewall(payload) {
                debug!("READYTOCHECKFIREWALL cookie={:x} their_ip={}", cookie, their_ip);
                let reply = protocol::build_proceed_check_firewall(manager.my_port, cookie);
                let _ = socket.send_to(&reply, peer).await;
                // Actualizar nodo (si lo conocemos)
                manager.add_node(peer.ip(), manager.my_port);
            }
        }
        UdpMsg::ServerListProceedCheckFirewall => {
            handle_proceed_check_firewall(socket, manager, payload, peer, now).await;
        }
        UdpMsg::ServerListCheckFirewallBusy => {
            debug!("CHECKFIREWALLBUSY (stub)");
        }
        UdpMsg::ServerListAckNodes => {
            debug!("ACKNODES recibido (no usado)");
        }
    }
}

/// Responde a SENDINFO con nuestro ACKINFO.
async fn handle_send_info(
    socket: &UdpSocket,
    manager: &Arc<UdpNodeManager>,
    peer: SocketAddr,
    _now: i64,
    users: u16,
) {
    let name = std::env::var("ASTRA_ROOM_NAME").unwrap_or_else(|_| "Astra Chat".to_string());
    let topic = std::env::var("ASTRA_ROOM_TOPIC").unwrap_or_else(|_| "Welcome to Astra".to_string());
    let version = format!("Astra {}", env!("CARGO_PKG_VERSION"));

    // nodos activos que conocemos
    let servers = manager.active_nodes(6, unix_time() as i64);

    let pkt = protocol::build_ackinfo(
        manager.my_port,
        users,
        &name,
        &topic,
        0, // language
        &version,
        &servers,
    );
    if let Err(e) = socket.send_to(&pkt, peer).await {
        warn!("error enviando ACKINFO a {}: {}", peer, e);
    } else {
        manager.stats.ackinfo_sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        debug!("ACKINFO enviado a {}", peer);
    }
}

/// Procesa ACKINFO: extrae la room info y los nodos del peer.
fn handle_ack_info(
    manager: &UdpNodeManager,
    payload: &[u8],
    peer: SocketAddr,
    now: i64,
) {
    let mut item = match protocol::parse_ackinfo(payload) {
        Some(i) => i,
        None => {
            warn!("ACKINFO malformado de {}", peer);
            return;
        }
    };
    // El IP viene del peer (puede diferir del payload)
    item.ip = peer.ip();

    info!(
        "ACKINFO de {}: '{}' ({} users) version={} +{} nodos",
        peer, item.name, item.users, item.version, item.servers.len()
    );

    // Marcar peer como exitoso
    manager.add_node(peer.ip(), item.port);
    manager.record_success(peer.ip(), now);

    // Guardar la room
    manager.upsert_room(
        item.ip,
        item.port,
        &item.name,
        &item.topic,
        &item.version,
        item.users,
        item.language,
        now,
    );

    // Agregar los nodos que nos pasó
    for s in &item.servers {
        manager.add_node(s.ip, s.port);
    }
}

/// Procesa ADDIPS: agrega los nodos del peer y le contesta con ACKIPS.
fn handle_add_ips(manager: &UdpNodeManager, payload: &[u8], peer: SocketAddr, _now: i64) {
    let (port, nodes) = match protocol::parse_addips(payload) {
        Some(p) => p,
        None => {
            warn!("ADDIPS malformado de {}", peer);
            return;
        }
    };
    debug!("ADDIPS de {}: {} nodos, port={}", peer, nodes.len(), port);

    // Actualizar el puerto del peer
    manager.add_node(peer.ip(), port);
    // Agregar los nodos del payload
    for n in nodes {
        manager.add_node(n.ip, n.port);
    }
}

/// Procesa ACKIPS: solo confirma que el peer recibió nuestros nodos.
fn handle_ack_ips(manager: &UdpNodeManager, payload: &[u8], peer: SocketAddr, now: i64) {
    let (port, nodes) = match protocol::parse_ackips(payload) {
        Some(p) => p,
        None => return,
    };
    debug!("ACKIPS de {}: {} nodos confirmados", peer, nodes.len());
    manager.add_node(peer.ip(), port);
    manager.record_success(peer.ip(), now);
    for n in nodes {
        manager.add_node(n.ip, n.port);
    }
}

/// Responde a SENDNODES con una lista de nodos.
async fn handle_send_nodes(
    socket: &UdpSocket,
    manager: &UdpNodeManager,
    peer: SocketAddr,
    _now: i64,
) {
    let nodes = manager.active_nodes(20, unix_time() as i64);
    let pkt = protocol::build_acknodes(manager.my_port, &nodes);
    if let Err(e) = socket.send_to(&pkt, peer).await {
        warn!("error enviando ACKNODES: {}", e);
    } else {
        manager.stats.acknodes_sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        debug!("ACKNODES enviado a {} con {} nodos", peer, nodes.len());
    }
}

/// Máximo de TCP probes de firewall simultáneos. Por encima de esto
/// respondemos CHECKFIREWALLBUSY con nodos alternativos.
const MAX_FIREWALL_PROBES: usize = 4;

/// Timeout del TCP probe de firewall check.
const FIREWALL_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Responde a WANTCHECKFIREWALL con READYTOCHECKFIREWALL.
///
/// El cookie queda registrado en el manager y se valida cuando el peer
/// responda con PROCEEDCHECKFIREWALL.
async fn handle_want_check_firewall(
    socket: &UdpSocket,
    manager: &Arc<UdpNodeManager>,
    peer: SocketAddr,
    now: i64,
) {
    let cookie = manager.issue_firewall_cookie(peer.ip(), now);
    // El campo IP le informa al solicitante su propia IP externa.
    let pkt = protocol::build_ready_check_firewall(cookie, peer.ip());
    if let Err(e) = socket.send_to(&pkt, peer).await {
        warn!("error enviando READYTOCHECKFIREWALL: {}", e);
    } else {
        debug!("READYTOCHECKFIREWALL cookie={:x} → {}", cookie, peer);
    }
}

/// Maneja PROCEEDCHECKFIREWALL: TCP probe real al puerto del solicitante.
///
/// El solicitante detecta la conexión TCP entrante y concluye que no está
/// firewalled. Solo probamos la IP origen del paquete UDP y solo si el
/// cookie fue emitido por nosotros a esa IP (anti-reflection). Si ya hay
/// demasiados probes en curso, respondemos CHECKFIREWALLBUSY con nodos
/// alternativos para que pruebe con otro server.
async fn handle_proceed_check_firewall(
    socket: &UdpSocket,
    manager: &Arc<UdpNodeManager>,
    payload: &[u8],
    peer: SocketAddr,
    now: i64,
) {
    let Some((port, cookie)) = protocol::parse_proceed_check_firewall(payload) else {
        return;
    };
    if port == 0 {
        return;
    }
    if !manager.take_firewall_cookie(cookie, peer.ip(), now) {
        debug!("PROCEEDCHECKFIREWALL con cookie inválido/vencido de {}", peer);
        return;
    }

    // Reservar un slot de probe; si estamos saturados → BUSY.
    use std::sync::atomic::Ordering;
    let reserved = manager
        .probes_in_flight
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            (n < MAX_FIREWALL_PROBES).then_some(n + 1)
        })
        .is_ok();
    if !reserved {
        let servers = manager.active_nodes(6, now);
        let pkt = protocol::build_check_firewall_busy(manager.my_port, &servers);
        let _ = socket.send_to(&pkt, peer).await;
        debug!("CHECKFIREWALLBUSY → {} (probes saturados)", peer);
        return;
    }

    let target = SocketAddr::new(peer.ip(), port);
    let mgr = manager.clone();
    tokio::spawn(async move {
        match tokio::time::timeout(
            FIREWALL_PROBE_TIMEOUT,
            tokio::net::TcpStream::connect(target),
        )
        .await
        {
            Ok(Ok(_stream)) => debug!("firewall probe OK: {} alcanzable", target),
            Ok(Err(e)) => debug!("firewall probe falló a {}: {}", target, e),
            Err(_) => debug!("firewall probe timeout a {}", target),
        }
        mgr.probes_in_flight.fetch_sub(1, Ordering::SeqCst);
    });
}

#[allow(dead_code)]
fn _silence_unused(_: &NodeAddr) {}

#[cfg(test)]
mod tests {
    use super::*;
    use server_core::db::Database;

    async fn spawn_test_listener() -> (Arc<UdpNodeManager>, SocketAddr) {
        let db = Database::in_memory().unwrap();
        let manager = Arc::new(UdpNodeManager::new(db, 0));
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr = socket.local_addr().unwrap();
        let user_count: UserCountFn = Arc::new(|| 0);
        let mgr = manager.clone();
        tokio::spawn(async move {
            let _ = run_listener(mgr, socket, user_count).await;
        });
        (manager, addr)
    }

    #[tokio::test]
    async fn firewall_check_full_flow_probes_requester() {
        let (_manager, server_addr) = spawn_test_listener().await;

        // 1. Peer pide el firewall check
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        peer.send_to(&protocol::build_want_check_firewall(), server_addr)
            .await
            .unwrap();

        // 2. Server responde READYTOCHECKFIREWALL con cookie
        let mut buf = [0u8; 512];
        let (n, _) = tokio::time::timeout(Duration::from_secs(5), peer.recv_from(&mut buf))
            .await
            .expect("timeout esperando READYTOCHECKFIREWALL")
            .unwrap();
        assert_eq!(buf[0], UdpMsg::ServerListReadyToCheckFirewall as u8);
        let (cookie, echoed_ip) = protocol::parse_ready_check_firewall(&buf[1..n]).unwrap();
        assert_eq!(echoed_ip, "127.0.0.1".parse::<std::net::IpAddr>().unwrap());

        // 3. Peer abre su puerto TCP y manda PROCEEDCHECKFIREWALL
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_port = tcp.local_addr().unwrap().port();
        peer.send_to(
            &protocol::build_proceed_check_firewall(tcp_port, cookie),
            server_addr,
        )
        .await
        .unwrap();

        // 4. El server hace el TCP probe → conexión entrante en el peer
        let accepted = tokio::time::timeout(Duration::from_secs(5), tcp.accept()).await;
        assert!(accepted.is_ok(), "el server debía hacer el TCP probe");
    }

    #[tokio::test]
    async fn firewall_check_ignores_invalid_cookie() {
        let (_manager, server_addr) = spawn_test_listener().await;

        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_port = tcp.local_addr().unwrap().port();

        // PROCEED sin haber hecho WANT (cookie inventado) → sin probe
        peer.send_to(
            &protocol::build_proceed_check_firewall(tcp_port, 0xBAD_C0DE),
            server_addr,
        )
        .await
        .unwrap();

        let accepted = tokio::time::timeout(Duration::from_millis(800), tcp.accept()).await;
        assert!(accepted.is_err(), "no debía haber probe con cookie inválido");
    }
}
