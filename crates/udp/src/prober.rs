//! Prober: envía `SENDINFO` periódicamente a nodos conocidos para
//! mantener actualizada la lista de rooms y descubrir nuevos nodos.

use std::sync::Arc;
use std::time::Duration;

use server_core::time::unix_time;
use tokio::net::UdpSocket;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::manager::UdpNodeManager;
use crate::protocol;

/// Loop principal del prober. Cada 15 segundos, elige un nodo y le
/// envía SENDINFO.
///
/// `socket` debe estar bind-eado al puerto del server.
pub async fn run_prober(manager: Arc<UdpNodeManager>, socket: Arc<UdpSocket>) {
    info!("UDP prober iniciado");
    let mut tick = interval(Duration::from_secs(15));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        probe_once(&manager, &socket).await;
    }
}

/// Ejecuta un ciclo de probing (útil para tests).
pub async fn probe_once(manager: &UdpNodeManager, socket: &UdpSocket) {
    let now = unix_time() as i64;

    // Asegurar que haya nodos
    if manager.count_nodes() == 0 {
        debug!("prober: no hay nodos para prober");
        return;
    }

    // Pickear el nodo más viejo
    let target = manager.next_probe_target(now);
    let target = match target {
        Some(t) => t,
        None => {
            debug!("prober: todos los nodos recientes, esperando");
            return;
        }
    };

    manager.stats.probes_sent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    manager.mark_probe_sent(target.ip, target.port, now);

    let pkt = protocol::build_sendinfo();
    let addr = std::net::SocketAddr::new(target.ip, target.port);

    debug!("prober: enviando SENDINFO a {}", addr);
    match socket.send_to(&pkt, addr).await {
        Ok(_) => {
            manager.stats.probes_ok.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug!("prober: SENDINFO enviado a {}", addr);
        }
        Err(e) => {
            warn!("prober: error enviando a {}: {}", addr, e);
            manager.record_failure(target.ip);
        }
    }
}
