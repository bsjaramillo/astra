//! Configuración persistente del servidor.
//!
//! Cargada desde `astra.toml` (o valores por defecto). Equivalente a
//! `core/Settings.cs` del sb0t original.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Configuración principal de Astra.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Puerto TCP principal.
    pub port: u16,
    /// Nombre del bot del servidor.
    pub bot_name: String,
    /// Nombre de la sala por defecto.
    pub room_name: String,
    /// Topic por defecto.
    pub room_topic: String,
    /// Password del owner (hash).
    pub owner_password: String,
    /// ¿Permitir registro de cuentas?
    pub allow_registration: bool,
    /// ¿Habilitar room search UDP?
    pub roomsearch: bool,
    /// Idioma por defecto.
    pub language: u8,
    /// ¿Habilitar web (ib0t)?
    pub web_enabled: bool,
    /// Puerto del panel web (si está habilitado).
    pub web_port: u16,
    /// Directorio de datos (bans, cuentas, logs).
    pub data_dir: String,
    /// ¿Habilitar Link Hub? El link NO usa un puerto propio: las conexiones
    /// de otros servidores entran multiplexadas por el puerto TCP principal
    /// (`port`), junto con los clientes Ares y WebSocket.
    pub link_hub_enabled: bool,
    /// GUID del server (para autenticación en link)
    pub guid: String,
    /// URL de Supabase (opcional).
    pub supabase_url: Option<String>,
    /// Key de Supabase (opcional).
    pub supabase_key: Option<String>,
    /// Configuración de seguridad.
    #[serde(default)]
    pub security: SecurityConfig,
    /// Leaves confiados para el Link Hub (paridad sb0t).
    ///
    /// Si la lista está vacía, el hub opera en modo legacy: acepta
    /// cualquier leaf y la sesión viaja sin encriptar. Con al menos un
    /// entry, solo se aceptan leaves cuyas credentials
    /// (`SHA1(reverse(name ++ guid))`) coincidan, y la sesión se encripta
    /// con AES-256-CBC.
    #[serde(default)]
    pub link_trusted_leaves: Vec<TrustedLeaf>,
    /// Endpoint de la API de GitHub para `/livescripts`/`/downloadscript`
    /// (paridad `liveScriptsEndpoint` de sb0t, default real de sb0t:
    /// `https://api.github.com`).
    #[serde(default = "default_live_scripts_endpoint")]
    pub live_scripts_endpoint: String,
}

fn default_live_scripts_endpoint() -> String {
    "https://api.github.com".to_string()
}

/// Un leaf autorizado a conectarse al Link Hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedLeaf {
    /// Nombre de la sala del leaf (su `room_name`).
    pub name: String,
    /// GUID del leaf (su `guid` de astra.toml, mismo string).
    pub guid: String,
}

/// Configuración de seguridad (defensa en capas).
///
/// Todos los valores son ajustables via `astra.toml`. Los defaults son
/// razonables para un servidor público pero conservador.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Capa 1: máx nuevas conexiones por IP en `connection_window_secs`.
    /// Default: 10 conexiones / 60s
    pub max_new_connections_per_ip: u32,
    /// Capa 1: ventana de tiempo en segundos para el rate limit.
    pub connection_window_secs: u64,
    /// Capa 1: violaciones antes de auto-banear (en la misma ventana).
    pub connection_flood_ban_threshold: u32,
    /// Capa 1: duración del auto-ban por connection flood (segundos).
    pub connection_flood_ban_secs: u64,

    /// Capa 2: máx conexiones TCP simultáneas por IP (logged-in o no).
    /// Default: 5
    pub max_concurrent_per_ip: u32,

    /// Capa 3: timeout en segundos para que el cliente envíe el primer
    /// paquete (ClientLogin). Si no llega, cerrar conexión.
    /// Default: 15s
    pub handshake_timeout_secs: u64,
    /// Capa 3: timeout en segundos sin recibir NADA del cliente después del
    /// login (red de seguridad para conexiones realmente muertas, no para
    /// "usuario está leyendo sin escribir" — eso es normal y no debe cortar
    /// la conexión). sb0t no tiene un timeout equivalente: se apoya en que
    /// el server le manda FASTPING al cliente cada 2s (ver `run_fast_ping`),
    /// lo que además evita que NAT/firewalls intermedios reciclen la
    /// conexión por inactividad. Default: 1800s (30 min).
    pub idle_timeout_secs: u64,

    /// Capa 4: longitud mínima del nick.
    pub min_name_length: usize,
    /// Capa 4: longitud máxima del nick.
    pub max_name_length: usize,
    /// Capa 4: habilitar detección de spam bots (6.6.6.6, etc.)
    pub reject_spam_bots: bool,

    /// Capa 5: máx logins fallidos por IP antes de auto-ban.
    pub max_failed_logins: u32,
    /// Capa 5: ventana para contar logins fallidos (segundos).
    pub failed_login_window_secs: u64,
    /// Capa 5: duración del auto-ban por logins fallidos (segundos).
    pub failed_login_ban_secs: u64,

    /// Captcha: si está habilitado, los users nuevos (IP sin historial)
    /// deben resolver un captcha antes de poder hablar en público.
    /// Default: false (deshabilitado; el owner debe opt-in).
    pub captcha_enabled: bool,
    /// Captcha: tiempo máximo para resolver el challenge (segundos).
    pub captcha_expiration_secs: u64,
    /// Captcha: máx intentos fallidos antes de kickear al user.
    pub captcha_max_attempts: u32,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            // Capa 1
            max_new_connections_per_ip: 10,
            connection_window_secs: 60,
            connection_flood_ban_threshold: 3,
            connection_flood_ban_secs: 300, // 5 min

            // Capa 2
            max_concurrent_per_ip: 5,

            // Capa 3
            handshake_timeout_secs: 15,
            idle_timeout_secs: 1800,

            // Capa 4
            min_name_length: 1,
            max_name_length: 30,
            reject_spam_bots: true,

            // Capa 5
            max_failed_logins: 5,
            failed_login_window_secs: 3600, // 1 hora
            failed_login_ban_secs: 3600,    // 1 hora

            // Captcha
            captcha_enabled: false,
            captcha_expiration_secs: 300, // 5 min
            captcha_max_attempts: 3,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 5009,
            bot_name: "Astra".to_string(),
            room_name: "Astra Chat".to_string(),
            room_topic: "Welcome to Astra".to_string(),
            owner_password: String::new(),
            allow_registration: true,
            roomsearch: true,
            language: 0,
            web_enabled: true,
            web_port: 5010,
            data_dir: "./data".to_string(),
            link_hub_enabled: false,
            guid: "astra-default-guid".to_string(),
            supabase_url: None,
            supabase_key: None,
            security: SecurityConfig::default(),
            link_trusted_leaves: Vec::new(),
            live_scripts_endpoint: default_live_scripts_endpoint(),
        }
    }
}

impl Settings {
    /// Carga la configuración desde un archivo TOML. Si no existe, usa defaults.
    pub fn load_or_default(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(s) => return s,
                    Err(e) => {
                        tracing::warn!("error parseando {}: {} - usando defaults", path.display(), e);
                    }
                },
                Err(e) => {
                    tracing::warn!("error leyendo {}: {} - usando defaults", path.display(), e);
                }
            }
        }
        Self::default()
    }

    /// Guarda la configuración al archivo TOML.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
}
