//! Prober: publica periódicamente nuestra existencia a los nodos conocidos.
//!
//! Equivalente a `UdpListener.Push()` + `UdpOutbound.AddIps` de sb0t: cada
//! ciclo le manda `ADDIPS` (nuestro puerto + hasta 6 nodos que conocemos) al
//! próximo nodo elegible (el que hace más tiempo no recibe un push nuestro).
//!
//! Este es el mecanismo de "salida hacia la red": cuando un nodo X recibe
//! nuestro ADDIPS, nos agrega a SU propia lista de nodos. Más tarde, cuando X
//! le responda a un tercero (otro server u otro cliente Ares) con su propio
//! ACKINFO/ADDIPS, puede incluirnos en la lista de `servers` gossip — así es
//! como, transitivamente, un cliente Ares real llega a enterarse de esta sala
//! y la muestra en su lista de rooms. Sin este push, nadie se entera de
//! nuestra existencia (aunque respondamos perfecto a quien nos pregunte
//! directamente con SENDINFO).
use std::sync::Arc;
use std::time::Duration;

use server_core::time::unix_time;
use tokio::net::UdpSocket;
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::manager::UdpNodeManager;
use crate::protocol;

/// Loop principal del prober. Cada 1 segundo evalúa si hay un nodo elegible
/// (sin push nuestro en los últimos 15 minutos) y, si lo hay, le anuncia
/// nuestra existencia. Cadencia igual a `UdpListener.Timer_1_Second` de sb0t:
/// con muchos nodos en la lista (cientos), tickear cada 1s es lo que permite
/// cubrir toda la lista dentro de la ventana de 15 minutos; con un tick más
/// espaciado, una lista grande tarda mucho más en recibir su primer push.
pub async fn run_prober(manager: Arc<UdpNodeManager>, socket: Arc<UdpSocket>) {
    info!("UDP prober (push ADDIPS) iniciado");
    let mut tick = interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        push_once(&manager, &socket).await;
    }
}

/// Ejecuta un ciclo de push (útil para tests): le manda ADDIPS (nuestro
/// puerto + hasta 6 nodos activos que conocemos, sin incluir al target) al
/// próximo nodo elegible.
pub async fn push_once(manager: &UdpNodeManager, socket: &UdpSocket) {
    let now = unix_time() as i64;

    // Asegurar que haya nodos a quien anunciarnos
    if manager.count_nodes() == 0 {
        debug!("prober: no hay nodos a quien publicarnos");
        return;
    }

    // Pickear el nodo al que hace más tiempo no le pusheamos
    let target = manager.next_probe_target(now);
    let target = match target {
        Some(t) => t,
        None => {
            debug!("prober: todos los nodos fueron pusheados recientemente, esperando");
            return;
        }
    };

    manager.mark_probe_sent(target.ip, target.port, now);

    let servers = manager.active_nodes_excluding(target.ip, 6, now);
    let pkt = protocol::build_addips(manager.my_port, &servers);
    let addr = std::net::SocketAddr::new(target.ip, target.port);

    debug!("prober: enviando ADDIPS a {} (+{} nodos)", addr, servers.len());
    match socket.send_to(&pkt, addr).await {
        Ok(_) => {
            manager
                .stats
                .addips_sent
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            debug!("prober: ADDIPS enviado a {}", addr);
        }
        Err(e) => {
            warn!("prober: error enviando ADDIPS a {}: {}", addr, e);
            manager.record_failure(target.ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto_ares::UdpMsg;
    use server_core::db::Database;

    #[tokio::test]
    async fn push_once_sends_addips_to_known_node() {
        // Server "remoto" simulado: solo necesitamos recibir el datagrama.
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = peer.local_addr().unwrap();

        let db = Database::in_memory().unwrap();
        let manager = UdpNodeManager::new(db, 5009);
        manager.add_node(peer_addr.ip(), peer_addr.port());

        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        push_once(&manager, &socket).await;

        let mut buf = [0u8; 512];
        let (n, _) = tokio::time::timeout(Duration::from_secs(2), peer.recv_from(&mut buf))
            .await
            .expect("timeout esperando ADDIPS")
            .unwrap();

        assert_eq!(buf[0], UdpMsg::ServerListAddIps as u8);
        let (port, _nodes) = protocol::parse_addips(&buf[1..n]).unwrap();
        assert_eq!(port, 5009, "el ADDIPS debe llevar nuestro propio puerto");
        assert_eq!(
            manager.stats.addips_sent.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn push_once_noop_without_known_nodes() {
        let db = Database::in_memory().unwrap();
        let manager = UdpNodeManager::new(db, 5009);
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        // No debe panicquear ni enviar nada si no hay nodos conocidos.
        push_once(&manager, &socket).await;
        assert_eq!(
            manager.stats.addips_sent.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }
}
