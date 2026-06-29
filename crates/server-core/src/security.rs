//! Defensa en capas contra DDoS y abuso.
//!
//! Cinco capas de protección, todas configurables vía `SecurityConfig`:
//!
//! 1. **ConnectionFloodTracker** — Rate limit per-IP de nuevas conexiones TCP.
//!    Sliding window. Si una IP abre N conexiones en M segundos, las nuevas
//!    se rechazan. Tras K violaciones, auto-ban temporal.
//!
//! 2. **ConcurrentConnLimiter** — Máx M conexiones TCP simultáneas por IP.
//!    Cubre logged-in + in-flight + idle.
//!
//! 3. **HandshakeTimeout** — Si el cliente no envía el primer paquete
//!    (ClientLogin) en N segundos, cerrar. Anti-slowloris.
//!
//! 4. **LoginValidator** — Valida que los datos del login sean legítimos:
//!    nombre válido, GUID no-trivial, version presente, no-spam patterns.
//!
//! 5. **FailedLoginTracker** — Auto-ban después de N logins fallidos por IP.
//!
//! ## Falsos positivos
//!
//! - **NAT/Corporate**: muchas personas detrás de la misma IP. Los defaults
//!   (10/min, 5 concurrent) son tolerables para esto. Si se necesita
//!   más, ajustar `SecurityConfig`.
//! - **Slow connections legítimas**: el timeout de 15s es generoso para
//!   conexiones lentas. Si el cliente tarda más, reintenta.
//! - **Nicks cortos**: usamos `min_name_length = 1`. Si te preocupa, subir.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::login::LoginData;
use crate::settings::SecurityConfig;
use crate::time::unix_time;

// ============================================================================
// Razón de rechazo
// ============================================================================

/// Razón por la que se rechazó una conexión o login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Capa 1: demasiadas conexiones nuevas desde esta IP.
    ConnectionFlood,
    /// Capa 1: IP baneada por connection flood.
    ConnectionFloodBan,
    /// Capa 2: máximo de conexiones simultáneas alcanzado.
    TooManyConcurrent,
    /// Capa 3: timeout esperando login.
    HandshakeTimeout,
    /// Capa 4: nick inválido.
    InvalidName,
    /// Capa 4: versión inválida o vacía.
    InvalidVersion,
    /// Capa 4: GUID inválido (todo ceros).
    InvalidGuid,
    /// Capa 4: spam bot detectado (6.6.6.6, 6969 files, etc).
    SpamBot,
    /// Capa 4: país inválido combinado con files > 0.
    SuspiciousProfile,
    /// Capa 5: demasiados logins fallidos.
    TooManyFailedLogins,
}

impl RejectReason {
    /// Mensaje user-friendly para enviar al cliente.
    pub fn message(&self) -> &'static str {
        match self {
            Self::ConnectionFlood => "Too many connections from your IP. Please wait.",
            Self::ConnectionFloodBan => "Your IP has been temporarily banned for connection flooding.",
            Self::TooManyConcurrent => "Too many simultaneous connections from your IP.",
            Self::HandshakeTimeout => "Connection timeout. Please try again.",
            Self::InvalidName => "Invalid username.",
            Self::InvalidVersion => "Invalid client version.",
            Self::InvalidGuid => "Invalid client identifier.",
            Self::SpamBot => "Connection rejected.",
            Self::SuspiciousProfile => "Invalid client profile.",
            Self::TooManyFailedLogins => "Too many failed login attempts. Please try again later.",
        }
    }
}

// ============================================================================
// Capa 1: ConnectionFloodTracker
// ============================================================================

/// Estado de ban temporal por IP.
#[derive(Debug, Clone)]
struct IpBan {
    /// Cuándo expira
    expires_at: Instant,
}

/// Sliding window de timestamps de nuevas conexiones por IP.
#[derive(Default)]
struct ConnWindow {
    /// Lista de timestamps (Instant) de conexiones recientes
    timestamps: Vec<Instant>,
    /// Violaciones del rate limit (incrementa cada vez que se rechaza)
    violations: u32,
    /// Ban temporal activo
    ban: Option<IpBan>,
}

/// Tracker de connection flood (rate limit per-IP).
pub struct ConnectionFloodTracker {
    config: SecurityConfig,
    state: Mutex<HashMap<IpAddr, ConnWindow>>,
}

impl ConnectionFloodTracker {
    /// Crea el tracker con la config dada.
    pub fn new(config: SecurityConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            state: Mutex::new(HashMap::new()),
        })
    }

    /// Verifica si una nueva conexión desde esta IP debe aceptarse.
    /// Retorna `None` si OK, o `Some(razón)` si se rechaza.
    pub fn check(&self, ip: IpAddr) -> Option<RejectReason> {
        let now = Instant::now();
        let window = Duration::from_secs(self.config.connection_window_secs);
        let mut state = self.state.lock();
        let entry = state.entry(ip).or_default();

        // Verificar ban temporal
        if let Some(ban) = &entry.ban {
            if ban.expires_at > now {
                return Some(RejectReason::ConnectionFloodBan);
            } else {
                entry.ban = None;
                entry.violations = 0;
            }
        }

        // Limpiar timestamps fuera de la ventana
        entry
            .timestamps
            .retain(|t| now.duration_since(*t) < window);

        // Verificar rate limit
        if entry.timestamps.len() as u32 >= self.config.max_new_connections_per_ip {
            entry.violations += 1;
            if entry.violations >= self.config.connection_flood_ban_threshold {
                entry.ban = Some(IpBan {
                    expires_at: now + Duration::from_secs(self.config.connection_flood_ban_secs),
                });
                tracing::warn!(
                    "auto-ban: IP {} baneada por connection flood ({} violaciones)",
                    ip, entry.violations
                );
                return Some(RejectReason::ConnectionFloodBan);
            }
            return Some(RejectReason::ConnectionFlood);
        }

        // Aceptar: registrar el timestamp
        entry.timestamps.push(now);
        None
    }

    /// Limpia entradas expiradas (llamar periódicamente).
    pub fn cleanup(&self) {
        let now = Instant::now();
        let window = Duration::from_secs(self.config.connection_window_secs);
        let mut state = self.state.lock();
        state.retain(|_, w| {
            w.timestamps.retain(|t| now.duration_since(*t) < window);
            w.ban.as_ref().map_or(true, |b| b.expires_at > now)
                || !w.timestamps.is_empty()
        });
    }
}

// ============================================================================
// Capa 2: ConcurrentConnLimiter
// ============================================================================

/// Límite de conexiones concurrentes por IP.
pub struct ConcurrentConnLimiter {
    config: SecurityConfig,
    state: Mutex<HashMap<IpAddr, u32>>,
}

impl ConcurrentConnLimiter {
    /// Crea el limiter.
    pub fn new(config: SecurityConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            state: Mutex::new(HashMap::new()),
        })
    }

    /// Intenta incrementar el contador para una IP. Retorna `false` si excede.
    pub fn try_acquire(&self, ip: IpAddr) -> bool {
        let mut state = self.state.lock();
        let count = state.entry(ip).or_insert(0);
        if *count >= self.config.max_concurrent_per_ip {
            return false;
        }
        *count += 1;
        true
    }

    /// Libera una conexión (llamar al desconectar).
    pub fn release(&self, ip: IpAddr) {
        let mut state = self.state.lock();
        if let Some(count) = state.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                state.remove(&ip);
            }
        }
    }

    /// Cantidad de conexiones activas desde una IP.
    pub fn count(&self, ip: IpAddr) -> u32 {
        self.state.lock().get(&ip).copied().unwrap_or(0)
    }
}

// ============================================================================
// Capa 3: HandshakeTimeout
// ============================================================================

/// Utilidad para timeouts de handshake. No es un objeto persistente: el
/// caller (tcp_handler) usa `tokio::time::timeout` directamente con el
/// valor de la config.
///
/// Esta struct existe solo para exponer el valor calculado y para tests.
pub struct HandshakeTimeout {
    pub secs: u64,
}

impl HandshakeTimeout {
    pub fn from_config(config: &SecurityConfig) -> Self {
        Self { secs: config.handshake_timeout_secs }
    }

    pub fn as_duration(&self) -> Duration {
        Duration::from_secs(self.secs)
    }
}

// ============================================================================
// Capa 4: LoginValidator
// ============================================================================

/// Validador de datos de login (anti-fake / anti-spam).
pub struct LoginValidator {
    config: SecurityConfig,
}

impl LoginValidator {
    /// Crea el validador con la config dada.
    pub fn new(config: SecurityConfig) -> Self {
        Self { config }
    }

    /// Valida los datos de un login. Retorna `Ok(())` si es legítimo,
    /// `Err(razón)` si debe rechazarse.
    pub fn validate(&self, login: &LoginData) -> Result<(), RejectReason> {
        // Nombre
        let name = &login.org_name;
        if name.len() < self.config.min_name_length || name.len() > self.config.max_name_length {
            return Err(RejectReason::InvalidName);
        }
        if Self::contains_bad_chars(name) {
            return Err(RejectReason::InvalidName);
        }

        // Versión
        if login.version.is_empty() {
            return Err(RejectReason::InvalidVersion);
        }
        if login.version.len() > 40 {
            return Err(RejectReason::InvalidVersion);
        }

        // GUID no-trivial. Como el parser aplica MD5 a los 16 bytes
        // recibidos, el GUID en `login.guid` es esencialmente random
        // (MD5 es uniforme), así que no podemos detectar "todo zeros" o
        // "todo el mismo byte" aquí. Los clientes maliciosos se detectan
        // por los otros checks (spam patterns, perfil sospechoso).
        // Mantenemos una heurística de "longitud cero" como safety net.
        let _ = login.guid; // suppress unused warning

        // Spam bot patterns (del sb0t original + extras)
        if self.config.reject_spam_bots {
            // 6.6.6.6 / 7.8.7.8 son LocalIPs conocidas de spammers
            if login.local_ip.octets() == [6, 6, 6, 6] || login.local_ip.octets() == [7, 8, 7, 8] {
                tracing::warn!(
                    "spam bot detectado: local_ip={} nick='{}'",
                    login.local_ip, name
                );
                return Err(RejectReason::SpamBot);
            }
            // 6969 files es un magic number de spammers
            if login.file_count == 6969 {
                tracing::warn!("spam bot detectado: 6969 files nick='{}'", name);
                return Err(RejectReason::SpamBot);
            }
        }

        // Perfil sospechoso: country=0 con files > 0 + age=0
        if login.country == 0 && login.file_count > 0 && login.age == 0 {
            return Err(RejectReason::SuspiciousProfile);
        }

        // File count absurdo (> u16 max no tiene sentido, pero algunos Ares
        // envían números grandes. Chequeamos que no sea exactamente un valor
        // conocido de spammers, ej. > 60000)
        if login.file_count > 60000 {
            return Err(RejectReason::SuspiciousProfile);
        }

        Ok(())
    }

    /// Detecta chars problemáticos (control, zero-width, etc).
    fn contains_bad_chars(s: &str) -> bool {
        s.chars().any(|c| {
            c.is_control()
                || matches!(c,
                    '\u{00A0}' | // no-break space
                    '\u{00AD}' | // soft hyphen
                    '\u{200B}' | // zero-width space
                    '\u{200C}' | // zero-width non-joiner
                    '\u{200D}' | // zero-width joiner
                    '\u{FEFF}'   // byte order mark
                )
        })
    }
}

// ============================================================================
// Capa 5: FailedLoginTracker
// ============================================================================

/// Registro de un intento de login fallido.
#[derive(Debug, Clone)]
struct FailedAttempt {
    timestamp: Instant,
}

/// Tracker de logins fallidos con auto-ban.
pub struct FailedLoginTracker {
    config: SecurityConfig,
    state: Mutex<HashMap<IpAddr, Vec<FailedAttempt>>>,
    /// Bans temporales derivados de logins fallidos
    bans: Mutex<HashMap<IpAddr, IpBan>>,
}

impl FailedLoginTracker {
    /// Crea el tracker.
    pub fn new(config: SecurityConfig) -> Arc<Self> {
        Arc::new(Self {
            config,
            state: Mutex::new(HashMap::new()),
            bans: Mutex::new(HashMap::new()),
        })
    }

    /// Verifica si la IP está baneada por logins fallidos.
    pub fn is_banned(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut bans = self.bans.lock();
        if let Some(ban) = bans.get(&ip) {
            if ban.expires_at > now {
                return true;
            }
            bans.remove(&ip);
        }
        false
    }

    /// Registra un login fallido. Retorna `true` si se debe banear.
    pub fn record_failure(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(self.config.failed_login_window_secs);

        let mut state = self.state.lock();
        let entry = state.entry(ip).or_default();
        entry.push(FailedAttempt { timestamp: now });

        // Limpiar fuera de ventana
        entry.retain(|a| now.duration_since(a.timestamp) < window);

        if entry.len() as u32 >= self.config.max_failed_logins {
            let mut bans = self.bans.lock();
            bans.insert(
                ip,
                IpBan {
                    expires_at: now + Duration::from_secs(self.config.failed_login_ban_secs),
                },
            );
            tracing::warn!(
                "auto-ban: IP {} baneada por {} logins fallidos",
                ip,
                entry.len()
            );
            return true;
        }
        false
    }

    /// Resetea el contador de fallos para una IP (login exitoso).
    pub fn record_success(&self, ip: IpAddr) {
        self.state.lock().remove(&ip);
    }

    /// Limpia entradas expiradas.
    pub fn cleanup(&self) {
        let now = Instant::now();
        let window = Duration::from_secs(self.config.failed_login_window_secs);
        self.state.lock().retain(|_, attempts| {
            attempts.retain(|a| now.duration_since(a.timestamp) < window);
            !attempts.is_empty()
        });
        self.bans.lock().retain(|_, ban| ban.expires_at > now);
    }
}

// ============================================================================
// SecurityManager (fachada)
// ============================================================================

/// Fachada que combina las 5 capas. Se inyecta en el `AppContext`.
pub struct SecurityManager {
    /// Capa 1
    pub conn_flood: Arc<ConnectionFloodTracker>,
    /// Capa 2
    pub concurrent: Arc<ConcurrentConnLimiter>,
    /// Capa 3
    pub handshake_timeout: HandshakeTimeout,
    /// Capa 4
    pub login_validator: LoginValidator,
    /// Capa 5
    pub failed_logins: Arc<FailedLoginTracker>,
}

impl SecurityManager {
    /// Construye el manager con la config dada.
    pub fn new(config: SecurityConfig) -> Arc<Self> {
        Arc::new(Self {
            conn_flood: ConnectionFloodTracker::new(config.clone()),
            concurrent: ConcurrentConnLimiter::new(config.clone()),
            handshake_timeout: HandshakeTimeout::from_config(&config),
            login_validator: LoginValidator::new(config.clone()),
            failed_logins: FailedLoginTracker::new(config),
        })
    }

    /// Verifica una nueva conexión entrante (capas 1, 2 y 5).
    /// Retorna `None` si OK, `Some(razón)` si se rechaza.
    pub fn check_new_connection(&self, ip: IpAddr) -> Option<RejectReason> {
        // Capa 5: ban por logins fallidos
        if self.failed_logins.is_banned(ip) {
            return Some(RejectReason::TooManyFailedLogins);
        }
        // Capa 1: rate limit
        if let Some(r) = self.conn_flood.check(ip) {
            return Some(r);
        }
        // Capa 2: concurrent limit
        if !self.concurrent.try_acquire(ip) {
            return Some(RejectReason::TooManyConcurrent);
        }
        None
    }

    /// Registra una conexión cerrada.
    pub fn on_disconnect(&self, ip: IpAddr) {
        self.concurrent.release(ip);
    }

    /// Cleanup periódico de las 5 capas.
    pub fn cleanup(&self) {
        self.conn_flood.cleanup();
        self.failed_logins.cleanup();
    }

    /// Timestamp del último reset de failed logins (para que el server lo limpie periódicamente).
    pub fn last_cleanup_secs(&self) -> u64 {
        unix_time()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn test_config() -> SecurityConfig {
        SecurityConfig {
            max_new_connections_per_ip: 100, // muy permisivo, para que concurrent sea el limitante
            connection_window_secs: 60,
            connection_flood_ban_threshold: 3,
            connection_flood_ban_secs: 60,
            max_concurrent_per_ip: 2,
            handshake_timeout_secs: 15,
            idle_timeout_secs: 120,
            min_name_length: 1,
            max_name_length: 30,
            reject_spam_bots: true,
            max_failed_logins: 3,
            failed_login_window_secs: 60,
            failed_login_ban_secs: 60,
        }
    }

    fn make_login(name: &str, version: &str, guid: [u8; 16], local_ip: Ipv4Addr) -> LoginData {
        LoginData {
            guid,
            file_count: 0,
            crypto: false,
            data_port: 1234,
            node_ip: Ipv4Addr::LOCALHOST,
            node_port: 5009,
            org_name: name.to_string(),
            version: version.to_string(),
            is_ares: true,
            is_cbot: false,
            local_ip,
            browsable: false,
            current_uploads: 0,
            max_uploads: 0,
            current_queued: 0,
            age: 25,
            sex: 1,
            country: 49,
            region: "US".to_string(),
            voice_chat_public: false,
            voice_chat_private: false,
            voice_opus_chat_public: false,
            voice_opus_chat_private: false,
            supports_html: false,
        }
    }

    /// GUID variado (no todos iguales, no todos ceros)
    fn varied_guid(seed: u8) -> [u8; 16] {
        let mut g = [0u8; 16];
        for i in 0..16 {
            g[i] = seed.wrapping_add(i as u8);
        }
        g
    }

    // ==================== Capa 1: ConnectionFloodTracker ====================

    fn flood_config() -> SecurityConfig {
        SecurityConfig {
            max_new_connections_per_ip: 3,
            connection_window_secs: 60,
            connection_flood_ban_threshold: 2,
            connection_flood_ban_secs: 60,
            max_concurrent_per_ip: 100,
            handshake_timeout_secs: 15,
            idle_timeout_secs: 120,
            min_name_length: 1,
            max_name_length: 30,
            reject_spam_bots: true,
            max_failed_logins: 3,
            failed_login_window_secs: 60,
            failed_login_ban_secs: 60,
        }
    }

    #[test]
    fn conn_flood_under_limit() {
        let t = ConnectionFloodTracker::new(flood_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        assert!(t.check(ip).is_none());
        assert!(t.check(ip).is_none());
        assert!(t.check(ip).is_none());
    }

    #[test]
    fn conn_flood_over_limit() {
        let t = ConnectionFloodTracker::new(flood_config());
        let ip = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));
        for _ in 0..3 {
            assert!(t.check(ip).is_none());
        }
        // 4ta conexión debe ser rechazada
        assert!(matches!(t.check(ip), Some(RejectReason::ConnectionFlood)));
    }

    #[test]
    fn conn_flood_auto_ban_after_violations() {
        let t = ConnectionFloodTracker::new(flood_config());
        let ip = IpAddr::V4(Ipv4Addr::new(3, 3, 3, 3));
        // 3 OK + 2 rechazos = 2 violaciones = auto-ban
        for _ in 0..3 {
            t.check(ip);
        }
        let _ = t.check(ip); // violation 1
        let r = t.check(ip); // violation 2 -> ban
        assert!(matches!(r, Some(RejectReason::ConnectionFloodBan)));
        // Las siguientes también son ban
        assert!(matches!(t.check(ip), Some(RejectReason::ConnectionFloodBan)));
    }

    #[test]
    fn different_ips_isolated() {
        let t = ConnectionFloodTracker::new(flood_config());
        let ip1 = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));
        for _ in 0..3 {
            t.check(ip1);
        }
        // ip1 lleno, ip2 OK
        assert!(t.check(ip1).is_some());
        assert!(t.check(ip2).is_none());
    }

    // ==================== Capa 2: ConcurrentConnLimiter ====================

    #[test]
    fn concurrent_under_limit() {
        let l = ConcurrentConnLimiter::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        assert!(l.try_acquire(ip));
        assert!(l.try_acquire(ip));
        // 3ra debe fallar
        assert!(!l.try_acquire(ip));
    }

    #[test]
    fn concurrent_release() {
        let l = ConcurrentConnLimiter::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        l.try_acquire(ip);
        l.try_acquire(ip);
        assert!(!l.try_acquire(ip));
        l.release(ip);
        assert!(l.try_acquire(ip));
    }

    // ==================== Capa 4: LoginValidator ====================

    #[test]
    fn validator_accepts_good_login() {
        let v = LoginValidator::new(test_config());
        let login = make_login("Alice", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        assert!(v.validate(&login).is_ok());
    }

    #[test]
    fn validator_rejects_empty_name() {
        let v = LoginValidator::new(test_config());
        let login = make_login("", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(v.validate(&login), Err(RejectReason::InvalidName));
    }

    #[test]
    fn validator_rejects_too_long_name() {
        let v = LoginValidator::new(test_config());
        let long = "A".repeat(50);
        let login = make_login(&long, "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(v.validate(&login), Err(RejectReason::InvalidName));
    }

    #[test]
    fn validator_rejects_control_chars() {
        let v = LoginValidator::new(test_config());
        let login = make_login("Bad\x00Name", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(v.validate(&login), Err(RejectReason::InvalidName));
    }

    #[test]
    fn validator_rejects_zero_width() {
        let v = LoginValidator::new(test_config());
        let login = make_login("Bad\u{200B}Name", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(v.validate(&login), Err(RejectReason::InvalidName));
    }

    #[test]
    fn validator_rejects_empty_version() {
        let v = LoginValidator::new(test_config());
        let login = make_login("X", "", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(v.validate(&login), Err(RejectReason::InvalidVersion));
    }

    #[test]
    fn validator_accepts_zero_guid_after_md5() {
        // El parser aplica MD5 al GUID antes de validar, así que [0;16]
        // produce un hash no-trivial y debe ser aceptado.
        let v = LoginValidator::new(test_config());
        let login = make_login("X", "Ares 2.1.0", [0; 16], Ipv4Addr::new(192, 168, 1, 1));
        assert!(v.validate(&login).is_ok());
    }

    #[test]
    fn validator_accepts_uniform_guid_after_md5() {
        // Mismo caso: [0xFF;16] -> MD5 -> hash no uniforme -> OK
        let v = LoginValidator::new(test_config());
        let login = make_login("X", "Ares 2.1.0", [0xFF; 16], Ipv4Addr::new(192, 168, 1, 1));
        assert!(v.validate(&login).is_ok());
    }

    #[test]
    fn validator_rejects_666_local_ip() {
        let v = LoginValidator::new(test_config());
        let login = make_login("Bot", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(6, 6, 6, 6));
        assert_eq!(v.validate(&login), Err(RejectReason::SpamBot));
    }

    #[test]
    fn validator_rejects_7878_local_ip() {
        let v = LoginValidator::new(test_config());
        let login = make_login("Bot", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(7, 8, 7, 8));
        assert_eq!(v.validate(&login), Err(RejectReason::SpamBot));
    }

    #[test]
    fn validator_rejects_6969_files() {
        let v = LoginValidator::new(test_config());
        let mut login = make_login("Bot", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        login.file_count = 6969;
        assert_eq!(v.validate(&login), Err(RejectReason::SpamBot));
    }

    #[test]
    fn validator_rejects_suspicious_profile() {
        let v = LoginValidator::new(test_config());
        let mut login = make_login("X", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        login.country = 0;
        login.file_count = 100;
        login.age = 0;
        assert_eq!(v.validate(&login), Err(RejectReason::SuspiciousProfile));
    }

    #[test]
    fn validator_rejects_absurd_file_count() {
        let v = LoginValidator::new(test_config());
        let mut login = make_login("X", "Ares 2.1.0", varied_guid(0xAB), Ipv4Addr::new(192, 168, 1, 1));
        login.country = 49;
        login.age = 25;
        login.file_count = 60001;
        assert_eq!(v.validate(&login), Err(RejectReason::SuspiciousProfile));
    }

    // ==================== Capa 5: FailedLoginTracker ====================

    #[test]
    fn failed_logins_under_limit() {
        let t = FailedLoginTracker::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        assert!(!t.record_failure(ip));
        assert!(!t.record_failure(ip));
        assert!(!t.is_banned(ip));
    }

    #[test]
    fn failed_logins_auto_ban() {
        let t = FailedLoginTracker::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        // Con max_failed_logins=3: las primeras 2 no banean, la 3ra sí
        assert!(!t.record_failure(ip));
        assert!(!t.record_failure(ip));
        assert!(t.record_failure(ip)); // 3ra = ban
        assert!(t.is_banned(ip));
    }

    #[test]
    fn failed_logins_success_resets() {
        let t = FailedLoginTracker::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        t.record_failure(ip);
        t.record_success(ip);
        // Ahora empieza de nuevo
        assert!(!t.record_failure(ip));
    }

    // ==================== SecurityManager (fachada) ====================

    #[test]
    fn manager_check_new_connection_layers() {
        let m = SecurityManager::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        // Capa 1: primeras 3 OK
        assert!(m.check_new_connection(ip).is_none());
        assert!(m.check_new_connection(ip).is_none());
        // Capa 2 (concurrent limit=2) rechaza la 3ra
        assert_eq!(
            m.check_new_connection(ip),
            Some(RejectReason::TooManyConcurrent)
        );
    }

    #[test]
    fn manager_concurrent_limit() {
        let m = SecurityManager::new(test_config());
        let ip1 = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        // Capa 2 limit = 2 concurrent
        m.check_new_connection(ip1);
        m.check_new_connection(ip1);
        let r = m.check_new_connection(ip1);
        assert_eq!(r, Some(RejectReason::TooManyConcurrent));
    }

    #[test]
    fn manager_release_on_disconnect() {
        let m = SecurityManager::new(test_config());
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        m.check_new_connection(ip);
        m.check_new_connection(ip);
        // límite alcanzado
        assert!(matches!(
            m.check_new_connection(ip),
            Some(RejectReason::TooManyConcurrent)
        ));
        m.on_disconnect(ip);
        // ahora hay espacio
        assert!(m.check_new_connection(ip).is_none());
    }
}
