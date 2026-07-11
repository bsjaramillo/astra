//! Encode/decode de paquetes UDP del protocolo Ares (room search).
//!
//! Equivalente a `core/Udp/UdpPacketReader.cs`, `UdpPacketWriter.cs`
//! y `UdpOutbound.cs` del sb0t original.

use std::net::{IpAddr, Ipv4Addr};

use bytes::Bytes;
use proto_ares::{UdpMsg, UdpPacketReader, UdpPacketWriter};

use crate::types::{NodeAddr, UdpChannelItem};

/// Lee una IPv4 de un reader. Retorna `None` si no hay suficientes bytes.
fn read_ip(reader: &mut UdpPacketReader) -> Option<IpAddr> {
    if reader.remaining() < 4 {
        return None;
    }
    let a = reader.read_u8();
    let b = reader.read_u8();
    let c = reader.read_u8();
    let d = reader.read_u8();
    Some(IpAddr::V4(Ipv4Addr::new(a, b, c, d)))
}

/// Escribe una IPv4 en un writer.
fn write_ip(writer: &mut UdpPacketWriter, ip: IpAddr) {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            writer.write_u8(o[0]);
            writer.write_u8(o[1]);
            writer.write_u8(o[2]);
            writer.write_u8(o[3]);
        }
        IpAddr::V6(_) => {
            // El protocolo UDP Ares solo soporta IPv4. Escribimos 0.0.0.0.
            writer.write_u8(0);
            writer.write_u8(0);
            writer.write_u8(0);
            writer.write_u8(0);
        }
    }
}

/// Parsea un ACKINFO recibido (opcionalmente con lista de servers).
///
/// Formato: `u16 port, u16 users, str name, str topic, u8 language, str version, u8 count, N*(ip+port)`
pub fn parse_ackinfo(data: &[u8]) -> Option<UdpChannelItem> {
    let mut r = UdpPacketReader::new(data);
    let port = r.read_u16_le() as u16;
    let users = r.read_u16_le() as u16;
    let name = r.read_string().ok()?;
    let topic = r.read_string().ok()?;
    let language = r.read_u8();
    let version = r.read_string().ok()?;
    let count = r.read_u8() as usize;

    let mut servers = Vec::with_capacity(count);
    for _ in 0..count {
        if r.remaining() < 6 {
            break;
        }
        let ip = read_ip(&mut r)?;
        let node_port = r.read_u16_le() as u16;
        servers.push(NodeAddr::new(ip, node_port));
    }

    Some(UdpChannelItem {
        ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED), // se completa desde el peer addr
        port,
        users,
        name,
        topic,
        language,
        version,
        servers,
    })
}

/// Parsea un ADDIPS (lista de IPs a agregar).
///
/// Formato: `u16 port, N*(ip+port)`
/// Devuelve el puerto del emisor y la lista de nodos.
pub fn parse_addips(data: &[u8]) -> Option<(u16, Vec<NodeAddr>)> {
    let mut r = UdpPacketReader::new(data);
    let port = r.read_u16_le() as u16;

    let mut servers = Vec::new();
    while r.remaining() >= 6 {
        let ip = read_ip(&mut r)?;
        let node_port = r.read_u16_le() as u16;
        servers.push(NodeAddr::new(ip, node_port));
    }

    Some((port, servers))
}

/// Parsea un ACKIPS.
pub fn parse_ackips(data: &[u8]) -> Option<(u16, Vec<NodeAddr>)> {
    parse_addips(data)
}

/// Parsea un SENDNODES (no tiene payload, solo opcode).
pub fn parse_sendnodes(_data: &[u8]) -> Option<()> {
    Some(())
}

/// Parsea un READYTOCHECKFIREWALL.
///
/// Formato: `u32 cookie, ipv4`
pub fn parse_ready_check_firewall(data: &[u8]) -> Option<(u32, IpAddr)> {
    if data.len() < 8 {
        return None;
    }
    let mut r = UdpPacketReader::new(data);
    let cookie = r.read_u32_le();
    let ip = read_ip(&mut r)?;
    Some((cookie, ip))
}

/// Parsea un PROCEEDCHECKFIREWALL.
///
/// Formato: `u16 port, u32 cookie`
pub fn parse_proceed_check_firewall(data: &[u8]) -> Option<(u16, u32)> {
    if data.len() < 6 {
        return None;
    }
    let mut r = UdpPacketReader::new(data);
    let port = r.read_u16_le() as u16;
    let cookie = r.read_u32_le();
    Some((port, cookie))
}

// ============================================================================
// Constructores de paquetes
// ============================================================================

/// Construye `OP_SERVERLIST_SENDINFO` (sin payload extra, solo opcode).
pub fn build_sendinfo() -> Bytes {
    let mut w = UdpPacketWriter::new();
    Bytes::copy_from_slice(&w.to_ares_packet(UdpMsg::ServerListSendInfo))
}

/// Construye `OP_SERVERLIST_ACKINFO` con la info del server + lista de nodos.
///
/// Formato: `u16 port, u16 users, str name, str topic, u8 lang, str version, u8 count, N*(ip+port)`
pub fn build_ackinfo(
    port: u16,
    users: u16,
    name: &str,
    topic: &str,
    language: u8,
    version: &str,
    servers: &[NodeAddr],
) -> Bytes {
    let mut w = UdpPacketWriter::new();
    w.write_u16_le(port);
    w.write_u16_le(users);
    w.write_string(name);
    w.write_string(topic);
    w.write_u8(language);
    w.write_string(version);

    let count = servers.len().min(6) as u8; // sb0t original limita a 6
    w.write_u8(count);
    for s in servers.iter().take(6) {
        write_ip(&mut w, s.ip);
        w.write_u16_le(s.port);
    }
    Bytes::copy_from_slice(&w.to_ares_packet(UdpMsg::ServerListAckInfo))
}

/// Construye `OP_SERVERLIST_ADDIPS` con nuestro puerto + lista de nodos.
pub fn build_addips(my_port: u16, servers: &[NodeAddr]) -> Bytes {
    build_node_list_packet(UdpMsg::ServerListAddIps, my_port, servers)
}

/// Construye `OP_SERVERLIST_ACKIPS` (mismo payload que ADDIPS: `u16 port,
/// N*(ip+port)`; solo cambia el opcode).
pub fn build_ackips(my_port: u16, servers: &[NodeAddr]) -> Bytes {
    build_node_list_packet(UdpMsg::ServerListAckIps, my_port, servers)
}

/// Escribe el payload común a ADDIPS/ACKIPS/CHECKFIREWALLBUSY
/// (`u16 port, N*(ip+port)`) con el opcode indicado.
fn build_node_list_packet(op: UdpMsg, my_port: u16, servers: &[NodeAddr]) -> Bytes {
    let mut w = UdpPacketWriter::new();
    w.write_u16_le(my_port);
    for s in servers.iter().take(6) {
        write_ip(&mut w, s.ip);
        w.write_u16_le(s.port);
    }
    Bytes::copy_from_slice(&w.to_ares_packet(op))
}

/// Construye `OP_SERVERLIST_ACKNODES` con lista de nodos Ares 2.x.
///
/// Formato: `u16 port, N*(ip+port)`
/// Solo se devuelven nodos "reconocidos" como Ares 2.x.
pub fn build_acknodes(my_port: u16, servers: &[NodeAddr]) -> Bytes {
    let mut w = UdpPacketWriter::new();
    w.write_u16_le(my_port);
    let count = servers.len().min(20);
    for s in servers.iter().take(count) {
        write_ip(&mut w, s.ip);
        w.write_u16_le(s.port);
    }
    Bytes::copy_from_slice(&w.to_ares_packet(UdpMsg::ServerListAckNodes))
}

/// Construye `OP_SERVERLIST_WANTCHECKFIREWALL` (sin payload, solo opcode).
/// Solo lo necesitamos para parsear incoming; outgoing es solo opcode.
pub fn build_want_check_firewall() -> Bytes {
    let mut w = UdpPacketWriter::new();
    Bytes::copy_from_slice(&w.to_ares_packet(UdpMsg::ServerListWantCheckFirewall))
}

/// Construye `OP_SERVERLIST_READYTOCHECKFIREWALL` con cookie + IP.
///
/// Formato: `u32 cookie, ipv4`
pub fn build_ready_check_firewall(cookie: u32, ip: IpAddr) -> Bytes {
    let mut w = UdpPacketWriter::new();
    w.write_u32_le(cookie);
    write_ip(&mut w, ip);
    Bytes::copy_from_slice(&w.to_ares_packet(UdpMsg::ServerListReadyToCheckFirewall))
}

/// Construye `OP_SERVERLIST_PROCEEDCHECKFIREWALL` con puerto + cookie.
///
/// Formato: `u16 port, u32 cookie`
pub fn build_proceed_check_firewall(port: u16, cookie: u32) -> Bytes {
    let mut w = UdpPacketWriter::new();
    w.write_u16_le(port);
    w.write_u32_le(cookie);
    Bytes::copy_from_slice(&w.to_ares_packet(UdpMsg::ServerListProceedCheckFirewall))
}

/// Construye `OP_SERVERLIST_CHECKFIREWALLBUSY` con lista de nodos.
pub fn build_check_firewall_busy(my_port: u16, servers: &[NodeAddr]) -> Bytes {
    build_node_list_packet(UdpMsg::ServerListCheckFirewallBusy, my_port, servers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn ackinfo_roundtrip() {
        let nodes = vec![
            NodeAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 5009),
            NodeAddr::new(IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2)), 5010),
        ];
        let pkt = build_ackinfo(5009, 5, "TestRoom", "Test topic", 0, "Astra 0.1", &nodes);
        assert_eq!(pkt[0], UdpMsg::ServerListAckInfo as u8);
        let parsed = parse_ackinfo(&pkt[1..]).unwrap();
        assert_eq!(parsed.port, 5009);
        assert_eq!(parsed.users, 5);
        assert_eq!(parsed.name, "TestRoom");
        assert_eq!(parsed.topic, "Test topic");
        assert_eq!(parsed.language, 0);
        assert_eq!(parsed.version, "Astra 0.1");
        assert_eq!(parsed.servers.len(), 2);
        assert_eq!(parsed.servers[0].port, 5009);
    }

    #[test]
    fn addips_roundtrip() {
        let nodes = vec![NodeAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 1234)];
        let pkt = build_addips(5009, &nodes);
        assert_eq!(pkt[0], UdpMsg::ServerListAddIps as u8);
        let (port, parsed_nodes) = parse_addips(&pkt[1..]).unwrap();
        assert_eq!(port, 5009);
        assert_eq!(parsed_nodes.len(), 1);
        assert_eq!(parsed_nodes[0].port, 1234);
    }

    #[test]
    fn ackips_and_check_firewall_busy_use_distinct_opcodes() {
        // Regresión: build_ackips y build_check_firewall_busy delegaban en
        // build_addips completo (incluido el opcode hardcodeado), así que
        // ambos emitían accidentalmente ADDIPS (11) en vez de su propio
        // opcode. El payload es el mismo formato; solo el opcode debe variar.
        let nodes = vec![NodeAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 1234)];

        let addips = build_addips(5009, &nodes);
        let ackips = build_ackips(5009, &nodes);
        let busy = build_check_firewall_busy(5009, &nodes);

        assert_eq!(addips[0], UdpMsg::ServerListAddIps as u8);
        assert_eq!(ackips[0], UdpMsg::ServerListAckIps as u8);
        assert_eq!(busy[0], UdpMsg::ServerListCheckFirewallBusy as u8);
        assert_ne!(ackips[0], addips[0]);
        assert_ne!(busy[0], addips[0]);

        // El payload (todo lo que sigue al opcode) es idéntico entre los tres.
        assert_eq!(&addips[1..], &ackips[1..]);
        assert_eq!(&addips[1..], &busy[1..]);
    }

    #[test]
    fn acknodes_roundtrip() {
        let nodes = vec![
            NodeAddr::new(IpAddr::V4(Ipv4Addr::new(5, 5, 5, 5)), 5009),
            NodeAddr::new(IpAddr::V4(Ipv4Addr::new(6, 6, 6, 6)), 5009),
        ];
        let pkt = build_acknodes(5009, &nodes);
        let mut r = UdpPacketReader::new(&pkt[1..]);
        let port = r.read_u16_le() as u16;
        assert_eq!(port, 5009);
        // quedan 12 bytes (2 nodos × 6 bytes)
        assert_eq!(r.remaining(), 12);
    }

    #[test]
    fn ready_check_firewall_roundtrip() {
        let pkt = build_ready_check_firewall(0xDEADBEEF, IpAddr::V4(Ipv4Addr::new(7, 7, 7, 7)));
        let (cookie, ip) = parse_ready_check_firewall(&pkt[1..]).unwrap();
        assert_eq!(cookie, 0xDEADBEEF);
        assert_eq!(ip.to_string(), "7.7.7.7");
    }

    #[test]
    fn proceed_check_firewall_roundtrip() {
        let pkt = build_proceed_check_firewall(5009, 0xCAFEBABE);
        let (port, cookie) = parse_proceed_check_firewall(&pkt[1..]).unwrap();
        assert_eq!(port, 5009);
        assert_eq!(cookie, 0xCAFEBABE);
    }

    #[test]
    fn sendinfo_is_just_opcode() {
        let pkt = build_sendinfo();
        assert_eq!(pkt.len(), 1);
        assert_eq!(pkt[0], UdpMsg::ServerListSendInfo as u8);
    }

    #[test]
    fn empty_addips() {
        let pkt = build_addips(5009, &[]);
        let (port, nodes) = parse_addips(&pkt[1..]).unwrap();
        assert_eq!(port, 5009);
        assert!(nodes.is_empty());
    }

    #[test]
    fn ackinfo_limits_to_6() {
        let nodes: Vec<_> = (0..20)
            .map(|i| NodeAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, i as u8)), 5009))
            .collect();
        let pkt = build_ackinfo(5009, 0, "x", "y", 0, "v", &nodes);
        let parsed = parse_ackinfo(&pkt[1..]).unwrap();
        // Solo debe tener 6 (el sb0t original limita a 6)
        assert_eq!(parsed.servers.len(), 6);
    }
}
