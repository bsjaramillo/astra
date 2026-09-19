//! Filtro de entrada anti-VPN/proxy.
//!
//! Detecta conexiones originadas en redes de VPN, proxies o datacenters
//! combinando dos fuentes **offline**:
//!
//! - **ASN**: reusa la base `asn.mmdb` de [`crate::geoip::GeoIp`]. El admin
//!   marca ASNs provistos por VPNs/datacenters conocidos.
//! - **CIDR**: blocklist de rangos en notación CIDR (IPv4/IPv6), cargada a
//!   mano o desde un feed (por defecto X4BNet `lists_vpn`).
//!
//! La **acción** ante un hit es configurable en vivo desde el panel `/admin`:
//! `report` (solo log/evento), `reject` (cierra la conexión), `captcha`
//! (deja entrar pero exige resolver el captcha) o `quarantine` (entra
//! silenciado). Igual que [`crate::ip_bans`], el manager mantiene un cache en
//! memoria con write-through a SQLite, de modo que los cambios del panel rigen
//! sin reiniciar (a diferencia de `astra.toml`, que es `Arc<Settings>`
//! inmutable en runtime).
//!
//! Por defecto el filtro está **deshabilitado** y en modo `report`: una sala
//! que no lo configure no cambia de comportamiento.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;

use ipnet::IpNet;
use parking_lot::RwLock;

use crate::db::Database;

/// Acción configurable ante un hit del filtro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VpnAction {
    /// Solo registrar (log + evento de scripting). No bloquea.
    #[default]
    Report,
    /// Rechazar la conexión con un mensaje genérico.
    Reject,
    /// Dejar entrar pero exigir captcha antes de poder hablar.
    Captcha,
    /// Dejar entrar en cuarentena (silenciado).
    Quarantine,
}

impl VpnAction {
    /// Representación persistida (string estable en DB/panel).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Reject => "reject",
            Self::Captcha => "captcha",
            Self::Quarantine => "quarantine",
        }
    }

    /// Parsea desde el string persistido. Acepta variantes en mayúsculas.
    pub fn from_str_lossy(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "reject" => Self::Reject,
            "captcha" => Self::Captcha,
            "quarantine" => Self::Quarantine,
            _ => Self::Report,
        }
    }

    /// Todas las acciones, para poblar el `<select>` del panel.
    pub fn all() -> [Self; 4] {
        [Self::Report, Self::Reject, Self::Captcha, Self::Quarantine]
    }
}

/// Tipo de entrada de la blocklist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnBlockKind {
    /// Rango CIDR (IPv4 o IPv6).
    Cidr,
    /// Número de ASN.
    Asn,
}

impl VpnBlockKind {
    /// Representación persistida (`cidr` / `asn`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cidr => "cidr",
            Self::Asn => "asn",
        }
    }

    /// Parsea desde el string persistido. `None` si no es un tipo conocido.
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cidr" => Some(Self::Cidr),
            "asn" => Some(Self::Asn),
            _ => None,
        }
    }
}

/// Una entrada de la blocklist tal como la ve el panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnBlockEntry {
    /// `cidr` o `asn`.
    pub kind: VpnBlockKind,
    /// El valor: CIDR canonico o numero de ASN en string.
    pub value: String,
    /// `manual` (agregado por el admin) o `feed` (descargado).
    pub source: String,
}

/// Resultado de un match contra la blocklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnHit {
    /// Qué regla matcheó.
    pub kind: VpnBlockKind,
    /// Valor de la regla (CIDR o ASN) que causó el hit.
    pub rule: String,
    /// ASN de la IP, si la base ASN estaba cargada (informativo).
    pub asn: Option<u32>,
}

/// Snapshot de configuración del filtro (live, persistido en DB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnConfig {
    /// Interruptor maestro.
    pub enabled: bool,
    /// Acción ante un hit.
    pub action: VpnAction,
    /// URL del feed. Vacío = sin descarga automática.
    pub feed_url: String,
    /// Horas entre refrescos del feed.
    pub refresh_hours: u64,
}

impl Default for VpnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            action: VpnAction::Report,
            feed_url: DEFAULT_FEED_URL.to_string(),
            refresh_hours: 24,
        }
    }
}

/// Feed por defecto: lista de redes de VPNs conocidas (X4BNet, `lists_vpn`).
/// Es VPN-only; para incluir datacenters cambiar a la ruta `datacenter`.
pub const DEFAULT_FEED_URL: &str =
    "https://raw.githubusercontent.com/X4BNet/lists_vpn/main/output/vpn/ipv4.txt";

/// Manager del filtro anti-VPN. Cache en memoria + write-through a SQLite.
pub struct VpnFilterManager {
    db: Arc<Database>,
    config: RwLock<VpnConfig>,
    /// Entradas CIDR agrupadas por primer octeto (IPv4) para acotar el scan.
    v4: RwLock<HashMap<u8, Vec<(IpNet, String)>>>,
    /// Entradas IPv6 (menos frecuentes; scan directo).
    v6: RwLock<Vec<(IpNet, String)>>,
    /// ASNs bloqueados.
    asns: RwLock<HashSet<u32>>,
    /// Señal para pedir un refresco inmediato del feed. La dispara el panel al
    /// activar el filtro o al pulsar "refrescar", para no esperar el intervalo
    /// (24h) con la lista vacía.
    refresh_notify: Arc<tokio::sync::Notify>,
    /// Candado para que dos descargas del feed no corran a la vez (panel +
    /// tick periódico).
    refreshing: std::sync::atomic::AtomicBool,
}

impl VpnFilterManager {
    /// Crea el manager cargando config y entradas desde la DB.
    pub fn new(db: Arc<Database>) -> Self {
        let config = db.load_vpn_config().unwrap_or_default();
        let mut v4: HashMap<u8, Vec<(IpNet, String)>> = HashMap::new();
        let mut v6: Vec<(IpNet, String)> = Vec::new();
        let mut asns: HashSet<u32> = HashSet::new();
        for entry in db.list_vpn_blocks().unwrap_or_default() {
            match entry.kind {
                VpnBlockKind::Cidr => match entry.value.parse::<IpNet>() {
                    Ok(net) => Self::insert_cidr(&mut v4, &mut v6, net, entry.source),
                    Err(e) => tracing::warn!(
                        "vpn_filter: CIDR inválido en DB '{}': {}",
                        entry.value,
                        e
                    ),
                },
                VpnBlockKind::Asn => {
                    if let Ok(a) = entry.value.parse::<u32>() {
                        asns.insert(a);
                    }
                }
            }
        }
        Self {
            db,
            config: RwLock::new(config),
            v4: RwLock::new(v4),
            v6: RwLock::new(v6),
            asns: RwLock::new(asns),
            refresh_notify: Arc::new(tokio::sync::Notify::new()),
            refreshing: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Notificación para pedir un refresco inmediato del feed.
    pub fn refresh_notify(&self) -> Arc<tokio::sync::Notify> {
        self.refresh_notify.clone()
    }

    /// Pide un refresco inmediato del feed (despierta al loop).
    pub fn request_refresh(&self) {
        self.refresh_notify.notify_one();
    }

    /// Intenta tomar el candado de descarga. `false` si ya hay una en curso.
    pub fn try_begin_refresh(&self) -> bool {
        self.refreshing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
    }

    /// Libera el candado de descarga.
    pub fn end_refresh(&self) {
        self.refreshing
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// ¿Hay una descarga de feed en curso?
    pub fn is_refreshing(&self) -> bool {
        self.refreshing.load(std::sync::atomic::Ordering::Acquire)
    }

    fn insert_cidr(
        v4: &mut HashMap<u8, Vec<(IpNet, String)>>,
        v6: &mut Vec<(IpNet, String)>,
        net: IpNet,
        source: String,
    ) {
        match net {
            IpNet::V4(_) => {
                let bucket = match net {
                    IpNet::V4(n) => n.network().octets()[0],
                    _ => 0,
                };
                v4.entry(bucket).or_default().push((net, source));
            }
            IpNet::V6(_) => v6.push((net, source)),
        }
    }

    /// Snapshot de la configuración actual.
    pub fn config(&self) -> VpnConfig {
        self.config.read().clone()
    }

    /// ¿Está activo el filtro?
    pub fn is_enabled(&self) -> bool {
        self.config.read().enabled
    }

    /// Acción configurada.
    pub fn action(&self) -> VpnAction {
        self.config.read().action
    }

    /// Cantidad total de entradas (CIDR + ASN).
    pub fn len(&self) -> usize {
        self.asns.read().len() + self.v4.read().values().map(Vec::len).sum::<usize>() + self.v6.read().len()
    }

    /// ¿No hay entradas?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Actualiza el interruptor maestro. Persiste.
    ///
    /// Al **activar**, pide un refresco inmediato del feed: sin esto, activar
    /// el filtro desde el panel dejaría la lista vacía hasta el próximo tick
    /// (hasta 24h), así que no detectaría nada hasta entonces.
    pub fn set_enabled(&self, enabled: bool) {
        self.config.write().enabled = enabled;
        self.persist_config();
        if enabled {
            self.request_refresh();
        }
    }

    /// Actualiza la acción ante hits. Persiste.
    pub fn set_action(&self, action: VpnAction) {
        self.config.write().action = action;
        self.persist_config();
    }

    /// Configura el feed y la cadencia de refresco. Persiste y pide un
    /// refresco inmediato (la URL cambió: hay que cargar la lista nueva).
    pub fn set_feed(&self, feed_url: String, refresh_hours: u64) {
        {
            let mut c = self.config.write();
            c.feed_url = feed_url;
            c.refresh_hours = refresh_hours.max(1);
        }
        self.persist_config();
        self.request_refresh();
    }

    fn persist_config(&self) {
        let c = self.config.read().clone();
        if let Err(e) = self.db.save_vpn_config(&c) {
            tracing::warn!("vpn_filter: no se pudo guardar la config: {}", e);
        }
    }

    /// Clasifica una IP: resuelve ASN (si hay base) y busca en la blocklist.
    /// Retorna `None` si no hay hit (o el filtro está apagado).
    pub fn classify(&self, geoip: &crate::geoip::GeoIp, ip: IpAddr) -> Option<VpnHit> {
        if !self.is_enabled() {
            return None;
        }
        self.classify_raw(geoip, ip)
    }

    /// Igual que [`Self::classify`] pero sin chequear el interruptor. Útil
    /// para `/vpncheck` (diagnóstico aunque el filtro esté apagado).
    pub fn classify_raw(
        &self,
        geoip: &crate::geoip::GeoIp,
        ip: IpAddr,
    ) -> Option<VpnHit> {
        let asn = geoip.lookup_asn(ip);

        if let Some(asn) = asn {
            if self.asns.read().contains(&asn) {
                return Some(VpnHit {
                    kind: VpnBlockKind::Asn,
                    rule: asn.to_string(),
                    asn: Some(asn),
                });
            }
        }

        match ip {
            IpAddr::V4(v4) => {
                let bucket = v4.octets()[0];
                let v4map = self.v4.read();
                if let Some(list) = v4map.get(&bucket) {
                    for (net, _source) in list {
                        if net.contains(&ip) {
                            return Some(VpnHit {
                                kind: VpnBlockKind::Cidr,
                                rule: net.to_string(),
                                asn,
                            });
                        }
                    }
                }
            }
            IpAddr::V6(_) => {
                for (net, _source) in self.v6.read().iter() {
                    if net.contains(&ip) {
                        return Some(VpnHit {
                            kind: VpnBlockKind::Cidr,
                            rule: net.to_string(),
                            asn,
                        });
                    }
                }
            }
        }
        let _ = asn;
        None
    }

    /// ¿La IP matchea alguna entrada? (sin importar la acción).
    pub fn is_blocked(&self, geoip: &crate::geoip::GeoIp, ip: IpAddr) -> bool {
        self.classify_raw(geoip, ip).is_some()
    }

    /// Agrega una entrada manual (CIDR o ASN). Retorna `false` si el valor
    /// no es válido para el tipo indicado.
    pub fn add(&self, kind: VpnBlockKind, value: &str) -> bool {
        let value = value.trim();
        if value.is_empty() {
            return false;
        }
        match kind {
            VpnBlockKind::Cidr => {
                let Ok(net) = value.parse::<IpNet>() else {
                    return false;
                };
                let canonical = net.to_string();
                let is_new = self
                    .db
                    .add_vpn_block(kind.as_str(), &canonical, "manual")
                    .unwrap_or(false);
                if is_new {
                    let mut v4 = self.v4.write();
                    let mut v6 = self.v6.write();
                    Self::insert_cidr(&mut v4, &mut v6, net, "manual".to_string());
                }
                is_new
            }
            VpnBlockKind::Asn => {
                let Ok(asn) = value.parse::<u32>() else {
                    return false;
                };
                if asn == 0 {
                    return false;
                }
                let is_new = self
                    .db
                    .add_vpn_block(kind.as_str(), &asn.to_string(), "manual")
                    .unwrap_or(false);
                if is_new {
                    self.asns.write().insert(asn);
                }
                is_new
            }
        }
    }

    /// Elimina una entrada (por tipo y valor). Retorna `true` si existía.
    pub fn remove(&self, kind: VpnBlockKind, value: &str) -> bool {
        let value = value.trim();
        let removed = self
            .db
            .remove_vpn_block(kind.as_str(), value)
            .unwrap_or(false);
        if !removed {
            return false;
        }
        match kind {
            VpnBlockKind::Cidr => {
                if let Ok(net) = value.parse::<IpNet>() {
                    match net {
                        IpNet::V4(n) => {
                            let bucket = n.network().octets()[0];
                            let mut v4 = self.v4.write();
                            if let Some(list) = v4.get_mut(&bucket) {
                                list.retain(|(n, _)| *n != net);
                            }
                        }
                        IpNet::V6(_) => {
                            self.v6.write().retain(|(n, _)| *n != net);
                        }
                    }
                }
            }
            VpnBlockKind::Asn => {
                if let Ok(asn) = value.parse::<u32>() {
                    self.asns.write().remove(&asn);
                }
            }
        }
        true
    }

    /// Lista todas las entradas (CIDR + ASN) para el panel.
    pub fn list(&self) -> Vec<VpnBlockEntry> {
        self.db.list_vpn_blocks().unwrap_or_default()
    }

    /// Borra todas las entradas de una fuente (`manual` o `feed`). Retorna
    /// cuántas borró y rearma el cache.
    pub fn clear_source(&self, source: &str) -> usize {
        let n = self.db.clear_vpn_blocks_by_source(source).unwrap_or(0);
        if n > 0 {
            self.rebuild_cache();
        }
        n
    }

    /// Reemplaza las entradas de fuente `feed` por las del texto importado
    /// (una línea por CIDR/ASN, comentarios con `#`). Retorna cuántas cargó.
    pub fn import_feed(&self, text: &str) -> usize {
        let mut entries: Vec<(VpnBlockKind, String)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Algunas listas traen `1.2.3.0/24 # comentario`.
            let token = line.split_whitespace().next().unwrap_or(line);
            let token = token.trim_end_matches(',');
            if token.is_empty() {
                continue;
            }
            if let Ok(net) = token.parse::<IpNet>() {
                let key = format!("cidr:{}", net);
                if seen.insert(key) {
                    entries.push((VpnBlockKind::Cidr, net.to_string()));
                }
                continue;
            }
            // ASN: formatos `AS1234`, `as1234` o `1234`.
            let asn_str = token
                .strip_prefix("AS")
                .or_else(|| token.strip_prefix("as"))
                .unwrap_or(token);
            if let Ok(asn) = asn_str.parse::<u32>() {
                if asn != 0 {
                    let key = format!("asn:{}", asn);
                    if seen.insert(key) {
                        entries.push((VpnBlockKind::Asn, asn.to_string()));
                    }
                }
            }
        }

        // Reemplazo atómico: borrar feed viejo y cargar el nuevo.
        let _ = self.db.clear_vpn_blocks_by_source("feed");
        for (kind, value) in &entries {
            let _ = self.db.add_vpn_block(kind.as_str(), value, "feed");
        }
        self.rebuild_cache();
        entries.len()
    }

    fn rebuild_cache(&self) {
        let mut v4: HashMap<u8, Vec<(IpNet, String)>> = HashMap::new();
        let mut v6: Vec<(IpNet, String)> = Vec::new();
        let mut asns: HashSet<u32> = HashSet::new();
        for entry in self.db.list_vpn_blocks().unwrap_or_default() {
            match entry.kind {
                VpnBlockKind::Cidr => {
                    if let Ok(net) = entry.value.parse::<IpNet>() {
                        Self::insert_cidr(&mut v4, &mut v6, net, entry.source);
                    }
                }
                VpnBlockKind::Asn => {
                    if let Ok(a) = entry.value.parse::<u32>() {
                        asns.insert(a);
                    }
                }
            }
        }
        *self.v4.write() = v4;
        *self.v6.write() = v6;
        *self.asns.write() = asns;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geoip::GeoIp;
    use std::path::Path;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    fn empty_geoip() -> GeoIp {
        GeoIp::load(
            mem_db(),
            Path::new("/nonexistent-astra-vpn-geoip"),
            crate::settings::GeoIpConfig::default(),
        )
    }

    #[test]
    fn default_disabled_and_report() {
        let m = VpnFilterManager::new(mem_db());
        assert!(!m.is_enabled());
        assert_eq!(m.action(), VpnAction::Report);
        assert!(m.is_empty());
    }

    #[test]
    fn disabled_filter_returns_none_even_on_match() {
        let m = VpnFilterManager::new(mem_db());
        m.add(VpnBlockKind::Cidr, "1.2.3.0/24");
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        // Apagado: classify no matchea.
        assert!(m.classify(&empty_geoip(), ip).is_none());
        // classify_raw sí, para diagnóstico.
        assert!(m.classify_raw(&empty_geoip(), ip).is_some());
    }

    #[test]
    fn cidr_match_when_enabled() {
        let m = VpnFilterManager::new(mem_db());
        m.add(VpnBlockKind::Cidr, "1.2.3.0/24");
        m.set_enabled(true);
        let hit = m.classify(&empty_geoip(), "1.2.3.99".parse().unwrap());
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().kind, VpnBlockKind::Cidr);
        assert!(m.classify(&empty_geoip(), "1.2.4.1".parse().unwrap()).is_none());
    }

    #[test]
    fn cidr_bucketed_by_first_octet() {
        let m = VpnFilterManager::new(mem_db());
        m.add(VpnBlockKind::Cidr, "10.0.0.0/8");
        m.add(VpnBlockKind::Cidr, "192.168.0.0/16");
        m.set_enabled(true);
        assert!(m.is_blocked(&empty_geoip(), "10.5.5.5".parse().unwrap()));
        assert!(m.is_blocked(&empty_geoip(), "192.168.1.1".parse().unwrap()));
        assert!(!m.is_blocked(&empty_geoip(), "8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn ipv6_cidr_match() {
        let m = VpnFilterManager::new(mem_db());
        m.add(VpnBlockKind::Cidr, "2001:db8::/32");
        m.set_enabled(true);
        assert!(m.is_blocked(&empty_geoip(), "2001:db8::1".parse().unwrap()));
        assert!(!m.is_blocked(&empty_geoip(), "2001:dead::1".parse().unwrap()));
    }

    #[test]
    fn asn_match() {
        let m = VpnFilterManager::new(mem_db());
        assert!(m.add(VpnBlockKind::Asn, "64500"));
        assert!(!m.add(VpnBlockKind::Asn, "0"));
        assert!(!m.add(VpnBlockKind::Asn, "no-es-asn"));
        m.set_enabled(true);
        assert!(m.asns.read().contains(&64500));
    }

    #[test]
    fn invalid_cidr_rejected() {
        let m = VpnFilterManager::new(mem_db());
        assert!(!m.add(VpnBlockKind::Cidr, "no-es-cidr"));
        assert!(!m.add(VpnBlockKind::Cidr, ""));
        assert!(m.is_empty());
    }

    #[test]
    fn remove_works() {
        let m = VpnFilterManager::new(mem_db());
        m.add(VpnBlockKind::Cidr, "1.2.3.0/24");
        m.set_enabled(true);
        assert!(m.is_blocked(&empty_geoip(), "1.2.3.4".parse().unwrap()));
        assert!(m.remove(VpnBlockKind::Cidr, "1.2.3.0/24"));
        assert!(!m.is_blocked(&empty_geoip(), "1.2.3.4".parse().unwrap()));
        assert!(!m.remove(VpnBlockKind::Cidr, "1.2.3.0/24"));
    }

    #[test]
    fn persists_across_instances() {
        let db = mem_db();
        {
            let m = VpnFilterManager::new(db.clone());
            m.add(VpnBlockKind::Cidr, "203.0.113.0/24");
            m.add(VpnBlockKind::Asn, "64511");
            m.set_enabled(true);
            m.set_action(VpnAction::Reject);
        }
        let m2 = VpnFilterManager::new(db);
        assert!(m2.is_enabled());
        assert_eq!(m2.action(), VpnAction::Reject);
        assert!(m2.is_blocked(&empty_geoip(), "203.0.113.5".parse().unwrap()));
    }

    #[test]
    fn import_feed_parses_and_replaces() {
        let m = VpnFilterManager::new(mem_db());
        let n = m.import_feed(
            "# comentario\n1.2.3.0/24\n1.2.3.0/24\n5.6.0.0/16 # inline\nAS64500\n1234\nbasura\n",
        );
        // 1.2.3.0/24, 5.6.0.0/16, AS64500, 1234 = 4
        assert_eq!(n, 4);
        assert_eq!(m.list().len(), 4);
        // Reimportar reemplaza, no duplica.
        let n2 = m.import_feed("9.9.9.0/24\n");
        assert_eq!(n2, 1);
        assert_eq!(m.list().len(), 1);
    }

    #[test]
    fn clear_source_only_removes_that_source() {
        let m = VpnFilterManager::new(mem_db());
        m.add(VpnBlockKind::Cidr, "1.2.3.0/24");
        m.import_feed("5.6.0.0/16\n");
        assert_eq!(m.list().len(), 2);
        let removed = m.clear_source("feed");
        assert_eq!(removed, 1);
        assert_eq!(m.list().len(), 1);
        assert_eq!(m.list()[0].source, "manual");
    }

    #[test]
    fn action_parse_roundtrip() {
        for a in VpnAction::all() {
            assert_eq!(VpnAction::from_str_lossy(a.as_str()), a);
        }
        assert_eq!(VpnAction::from_str_lossy("REJECT"), VpnAction::Reject);
        assert_eq!(VpnAction::from_str_lossy("basura"), VpnAction::Report);
    }
}
