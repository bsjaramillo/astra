//! Tipos del sistema de UDP room search.

use std::net::IpAddr;

use parking_lot::RwLock;

/// Nodo UDP conocido (un server Ares que participa en la red de room search).
///
/// Equivalente a `core/UdpNode.cs` del sb0t original. Los nodos se
/// almacenan en SQLite y se cachean en memoria para acceso rápido.
#[derive(Debug, Clone)]
pub struct UdpNode {
    /// IP del nodo
    pub ip: IpAddr,
    /// Puerto TCP del nodo
    pub port: u16,
    /// Contador de acknowledgments (mayor = más confiable)
    pub ack: i64,
    /// Cantidad de intentos fallidos consecutivos
    pub try_count: i64,
    /// Último connect exitoso (ms epoch)
    pub last_connect: i64,
    /// Último envío de ADDIPS (ms epoch)
    pub last_sent_ips: i64,
}

impl UdpNode {
    /// Crea un nodo con valores por defecto.
    pub fn new(ip: IpAddr, port: u16) -> Self {
        Self {
            ip,
            port,
            ack: 0,
            try_count: 0,
            last_connect: 0,
            last_sent_ips: 0,
        }
    }

    /// ¿Es válido este nodo? (descartamos el propio)
    pub fn is_valid(&self) -> bool {
        self.port > 0
    }
}

/// Item de canal UDP: la info que un server Ares envía en su ACKINFO.
///
/// Equivalente a `core/Udp/UdpChannelItem.cs`.
#[derive(Debug, Clone)]
pub struct UdpChannelItem {
    /// IP del server
    pub ip: IpAddr,
    /// Puerto TCP
    pub port: u16,
    /// Cantidad de usuarios conectados
    pub users: u16,
    /// Nombre de la sala
    pub name: String,
    /// Topic de la sala
    pub topic: String,
    /// Idioma
    pub language: u8,
    /// Versión del server
    pub version: String,
    /// Nodos adicionales que este server conoce (para cascada de discovery)
    pub servers: Vec<NodeAddr>,
}

impl UdpChannelItem {
    /// Crea un item con datos mínimos.
    pub fn new(ip: IpAddr, port: u16, name: String, topic: String) -> Self {
        Self {
            ip,
            port,
            users: 0,
            name,
            topic,
            language: 0,
            version: String::new(),
            servers: Vec::new(),
        }
    }
}

/// Dirección IP + puerto (usado para listas de nodos en paquetes UDP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeAddr {
    /// IP
    pub ip: IpAddr,
    /// Puerto
    pub port: u16,
}

impl NodeAddr {
    /// Crea un NodeAddr.
    pub fn new(ip: IpAddr, port: u16) -> Self {
        Self { ip, port }
    }
}

/// Estadísticas globales del sistema UDP (thread-safe).
#[derive(Default)]
pub struct UdpStats {
    /// Cantidad de SENDINFO recibidos
    pub sendinfo_recv: std::sync::atomic::AtomicU64,
    /// Cantidad de ACKINFO enviados
    pub ackinfo_sent: std::sync::atomic::AtomicU64,
    /// Cantidad de ADDIPS recibidos
    pub addips_recv: std::sync::atomic::AtomicU64,
    /// Cantidad de ACKIPS enviados
    pub ackips_sent: std::sync::atomic::AtomicU64,
    /// Cantidad de ADDIPS enviados
    pub addips_sent: std::sync::atomic::AtomicU64,
    /// Cantidad de ACKIPS recibidos
    pub ackips_recv: std::sync::atomic::AtomicU64,
    /// Cantidad de SENDNODES recibidos
    pub sendnodes_recv: std::sync::atomic::AtomicU64,
    /// Cantidad de ACKNODES enviados
    pub acknodes_sent: std::sync::atomic::AtomicU64,
    /// Cantidad de probes enviados
    pub probes_sent: std::sync::atomic::AtomicU64,
    /// Cantidad de probes exitosos
    pub probes_ok: std::sync::atomic::AtomicU64,
}

impl UdpStats {
    /// Crea stats vacías.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resetea todos los contadores.
    pub fn reset(&self) {
        use std::sync::atomic::Ordering;
        self.sendinfo_recv.store(0, Ordering::Relaxed);
        self.ackinfo_sent.store(0, Ordering::Relaxed);
        self.addips_recv.store(0, Ordering::Relaxed);
        self.ackips_sent.store(0, Ordering::Relaxed);
        self.addips_sent.store(0, Ordering::Relaxed);
        self.ackips_recv.store(0, Ordering::Relaxed);
        self.sendnodes_recv.store(0, Ordering::Relaxed);
        self.acknodes_sent.store(0, Ordering::Relaxed);
        self.probes_sent.store(0, Ordering::Relaxed);
        self.probes_ok.store(0, Ordering::Relaxed);
    }

    /// Log resumen de stats.
    pub fn log_summary(&self) {
        use std::sync::atomic::Ordering;
        tracing::info!(
            "UDP stats: SENDINFO={} ACKINFO={} ADDIPS={}/{} ACKIPS={}/{} SENDNODES={} ACKNODES={} probes={}/{}",
            self.sendinfo_recv.load(Ordering::Relaxed),
            self.ackinfo_sent.load(Ordering::Relaxed),
            self.addips_recv.load(Ordering::Relaxed),
            self.addips_sent.load(Ordering::Relaxed),
            self.ackips_sent.load(Ordering::Relaxed),
            self.ackips_recv.load(Ordering::Relaxed),
            self.sendnodes_recv.load(Ordering::Relaxed),
            self.acknodes_sent.load(Ordering::Relaxed),
            self.probes_sent.load(Ordering::Relaxed),
            self.probes_ok.load(Ordering::Relaxed),
        );
    }
}

/// Lista thread-safe de nodos (cache en memoria del manager).
pub type NodeList = RwLock<Vec<UdpNode>>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn node_new() {
        let n = UdpNode::new(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)), 5009);
        assert_eq!(n.port, 5009);
        assert_eq!(n.ack, 0);
        assert!(n.is_valid());
    }

    #[test]
    fn stats_reset() {
        let s = UdpStats::new();
        s.probes_sent.fetch_add(5, std::sync::atomic::Ordering::Relaxed);
        s.reset();
        assert_eq!(s.probes_sent.load(std::sync::atomic::Ordering::Relaxed), 0);
    }
}
