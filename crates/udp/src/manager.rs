//! `UdpNodeManager`: gestiona la lista de nodos UDP conocidos.
//!
//! Combina un cache en memoria con persistencia en SQLite.
//! Es el equivalente directo de `core/Udp/UdpNodeManager.cs` del sb0t original.

use std::net::IpAddr;
use std::sync::Arc;

use parking_lot::RwLock;
use rand::Rng;

use server_core::db::{Database, NodeRecord, RoomRecord};

use crate::types::{NodeAddr, UdpNode, UdpStats};

/// Manager de nodos UDP (cache en memoria + persistencia).
pub struct UdpNodeManager {
    /// DB compartida
    db: Arc<Database>,
    /// Cache en memoria de los nodos
    nodes: RwLock<Vec<UdpNode>>,
    /// Cache en memoria de las rooms descubiertas
    rooms: RwLock<Vec<RoomRecord>>,
    /// Stats
    pub stats: UdpStats,
    /// Puerto del propio server
    pub my_port: u16,
}

impl UdpNodeManager {
    /// Devuelve un `Arc<Database>` para módulos que necesiten acceso directo a la DB.
    pub fn db_arc(&self) -> Arc<Database> {
        self.db.clone()
    }

    /// Crea un nuevo manager, cargando los nodos y rooms desde la DB.
    pub fn new(db: Arc<Database>, my_port: u16) -> Self {
        let nodes: Vec<UdpNode> = db
            .list_nodes()
            .unwrap_or_default()
            .into_iter()
            .map(|n| UdpNode {
                ip: n.ip.parse().unwrap_or(IpAddr::from([0, 0, 0, 0])),
                port: n.port,
                ack: n.ack,
                try_count: n.try_count,
                last_connect: n.last_connect,
                last_sent_ips: n.last_sent_ips,
            })
            .collect();

        let rooms = db.list_rooms().unwrap_or_default();

        if !nodes.is_empty() {
            tracing::info!("UdpNodeManager: cargados {} nodos, {} rooms", nodes.len(), rooms.len());
        }

        Self {
            db,
            nodes: RwLock::new(nodes),
            rooms: RwLock::new(rooms),
            stats: UdpStats::new(),
            my_port,
        }
    }

    // ========================================================================
    // Nodes
    // ========================================================================

    /// Cantidad de nodos conocidos.
    pub fn count_nodes(&self) -> usize {
        self.nodes.read().len()
    }

    /// Devuelve todos los nodos.
    pub fn all_nodes(&self) -> Vec<UdpNode> {
        self.nodes.read().clone()
    }

    /// Busca un nodo por IP.
    pub fn find_by_ip(&self, ip: IpAddr) -> Option<UdpNode> {
        self.nodes.read().iter().find(|n| n.ip == ip).cloned()
    }

    /// Añade o actualiza un nodo. Devuelve `true` si es nuevo.
    pub fn add_node(&self, ip: IpAddr, port: u16) -> bool {
        {
            let nodes = self.nodes.read();
            if let Some(existing) = nodes.iter().find(|n| n.ip == ip) {
                // Solo actualiza el puerto si cambió
                if existing.port != port {
                    drop(nodes);
                    self.update_node_port(ip, port);
                }
                return false;
            }
        }
        let new_node = UdpNode::new(ip, port);
        self.nodes.write().push(new_node);
        // Persistir
        if let Err(e) = self.db.upsert_node(&ip.to_string(), port) {
            tracing::error!("error persistiendo nodo: {}", e);
        }
        true
    }

    /// Actualiza el puerto de un nodo existente.
    pub fn update_node_port(&self, ip: IpAddr, port: u16) {
        let mut nodes = self.nodes.write();
        if let Some(n) = nodes.iter_mut().find(|n| n.ip == ip) {
            n.port = port;
        }
        if let Err(e) = self.db.update_node_port(&ip.to_string(), port) {
            tracing::error!("error actualizando puerto: {}", e);
        }
    }

    /// Incrementa el ack de un nodo (respuesta exitosa).
    pub fn record_success(&self, ip: IpAddr, now_ms: i64) {
        let mut nodes = self.nodes.write();
        if let Some(n) = nodes.iter_mut().find(|n| n.ip == ip) {
            n.ack += 1;
            n.try_count = 0;
            n.last_connect = now_ms;
        }
        if let Err(e) = self.db.bump_node_ack(&ip.to_string(), 0, now_ms) {
            tracing::error!("error bump ack: {}", e);
        }
    }

    /// Marca un nodo como "intento fallido" (incrementa try_count).
    pub fn record_failure(&self, ip: IpAddr) {
        let mut nodes = self.nodes.write();
        if let Some(n) = nodes.iter_mut().find(|n| n.ip == ip) {
            n.try_count += 1;
        }
        if let Err(e) = self.db.record_node_failure(&ip.to_string(), 0) {
            tracing::error!("error bump failure: {}", e);
        }
    }

    /// Elige el próximo nodo a prober (el más viejo en last_sent_ips).
    /// Devuelve `None` si no hay nodos.
    pub fn next_probe_target(&self, now_ms: i64) -> Option<UdpNode> {
        let nodes = self.nodes.read();
        if nodes.is_empty() {
            return None;
        }
        // Nodos que no se han probado en los últimos 15 minutos
        let candidates: Vec<&UdpNode> = nodes
            .iter()
            .filter(|n| n.last_sent_ips == 0 || (now_ms - n.last_sent_ips) > 900_000)
            .collect();
        if candidates.is_empty() {
            return None;
        }
        // Ordenar por last_sent_ips (más viejo primero)
        let mut sorted: Vec<&UdpNode> = candidates;
        sorted.sort_by_key(|n| n.last_sent_ips);
        let target = sorted[0].clone();
        Some(target)
    }

    /// Marca un nodo como "probe enviado en este momento".
    pub fn mark_probe_sent(&self, ip: IpAddr, port: u16, now_ms: i64) {
        let mut nodes = self.nodes.write();
        if let Some(n) = nodes.iter_mut().find(|n| n.ip == ip && n.port == port) {
            n.last_sent_ips = now_ms;
            n.try_count += 1;
        }
        if let Err(e) = self.db.update_node_last_sent_ips(&ip.to_string(), port, now_ms) {
            tracing::error!("error mark probe: {}", e);
        }
    }

    /// Devuelve hasta `max` nodos "activos" (con ack > 0 y conexión reciente).
    /// Usado para enviar en ACKINFO / ADDIPS a otros servers.
    pub fn active_nodes(&self, max: usize, now_ms: i64) -> Vec<NodeAddr> {
        let nodes = self.nodes.read();
        let mut active: Vec<&UdpNode> = nodes
            .iter()
            .filter(|n| n.ack > 0 && (n.last_connect + 900_000) > now_ms)
            .collect();
        // Orden aleatorio (como el original)
        active.sort_by(|_, _| {
            let mut rng = rand::thread_rng();
            if rng.gen() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        });
        active
            .into_iter()
            .take(max)
            .map(|n| NodeAddr::new(n.ip, n.port))
            .collect()
    }

    /// Expira nodos: try > 4 y last_connect > 1h.
    pub fn expire(&self, now_ms: i64) {
        let min_try = 4;
        let max_age_ms = 3_600_000;
        if let Err(e) = self.db.expire_nodes(min_try, max_age_ms, now_ms) {
            tracing::error!("error expirando nodos: {}", e);
        }
        // Refrescar cache
        let nodes = self.db.list_nodes().unwrap_or_default();
        let new_cache: Vec<UdpNode> = nodes
            .into_iter()
            .map(|n| UdpNode {
                ip: n.ip.parse().unwrap_or(IpAddr::from([0, 0, 0, 0])),
                port: n.port,
                ack: n.ack,
                try_count: n.try_count,
                last_connect: n.last_connect,
                last_sent_ips: n.last_sent_ips,
            })
            .collect();
        *self.nodes.write() = new_cache;
    }

    // ========================================================================
    // Rooms
    // ========================================================================

    /// Cantidad de rooms.
    pub fn count_rooms(&self) -> usize {
        self.rooms.read().len()
    }

    /// Lista todas las rooms.
    pub fn all_rooms(&self) -> Vec<RoomRecord> {
        self.rooms.read().clone()
    }

    /// Inserta/actualiza una room.
    pub fn upsert_room(
        &self,
        ip: IpAddr,
        port: u16,
        name: &str,
        topic: &str,
        version: &str,
        users: u16,
        language: u8,
        last_update_ms: i64,
    ) {
        if let Err(e) = self.db.upsert_room(
            &ip.to_string(),
            port,
            name,
            topic,
            version,
            users,
            language,
            last_update_ms,
        ) {
            tracing::error!("error guardando room: {}", e);
        }
        // Refrescar cache
        let rooms = self.db.list_rooms().unwrap_or_default();
        *self.rooms.write() = rooms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_empty_db() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        assert_eq!(m.count_nodes(), 0);
        assert_eq!(m.count_rooms(), 0);
    }

    #[test]
    fn add_node() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        assert!(m.add_node(ip, 5009));
        assert!(!m.add_node(ip, 5009)); // ya existe
        assert_eq!(m.count_nodes(), 1);
    }

    #[test]
    fn record_success_increments_ack() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        let ip: IpAddr = "1.1.1.1".parse().unwrap();
        m.add_node(ip, 5009);
        m.record_success(ip, 1000);
        m.record_success(ip, 2000);
        let n = m.find_by_ip(ip).unwrap();
        assert_eq!(n.ack, 2);
        assert_eq!(n.try_count, 0);
        assert_eq!(n.last_connect, 2000);
    }

    #[test]
    fn record_failure_increments_try() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        let ip: IpAddr = "2.2.2.2".parse().unwrap();
        m.add_node(ip, 5009);
        m.record_failure(ip);
        m.record_failure(ip);
        let n = m.find_by_ip(ip).unwrap();
        assert_eq!(n.try_count, 2);
    }

    #[test]
    fn upsert_room() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        let ip: IpAddr = "3.3.3.3".parse().unwrap();
        m.upsert_room(ip, 5009, "TestRoom", "Test topic", "Astra 0.1", 5, 0, 1234);
        assert_eq!(m.count_rooms(), 1);
        let rooms = m.all_rooms();
        assert_eq!(rooms[0].name, "TestRoom");
        assert_eq!(rooms[0].users, 5);
    }

    #[test]
    fn next_probe_target() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        let ip1: IpAddr = "1.1.1.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();
        m.add_node(ip1, 5009);
        m.add_node(ip2, 5010);
        // Sin last_sent_ips, ambos candidatos. El más viejo (0) gana.
        let t = m.next_probe_target(1000).unwrap();
        assert!(t.ip == ip1 || t.ip == ip2);
    }

    #[test]
    fn active_nodes_filters_old() {
        let db = Database::in_memory().unwrap();
        let m = UdpNodeManager::new(db, 5009);
        let ip1: IpAddr = "1.1.1.1".parse().unwrap();
        m.add_node(ip1, 5009);
        m.record_success(ip1, 1000);
        // 1 hora después, ya no es "activo" (ventana = 15 min = 900_000 ms)
        let active = m.active_nodes(10, 1000 + 3_600_000);
        assert!(active.is_empty());
        // Dentro de la ventana, sí
        let active = m.active_nodes(10, 1000 + 100_000);
        assert_eq!(active.len(), 1);
    }
}
