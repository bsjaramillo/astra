//! Configuración persistente del servidor.
//!
//! Cargada desde `astra.toml` (o valores por defecto). Equivalente a
//! `core/Settings.cs` del sb0t original.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Configuración principal de Astra.
/// Configuración del servidor.
///
/// `#[serde(default)]` a nivel de struct: cualquier campo ausente toma su
/// valor por defecto, que es lo que `astra.toml.example` promete y lo que
/// espera quien escribe un toml mínimo con tres líneas. Sin esto, omitir una
/// sola clave invalidaba el archivo entero y el servidor arrancaba con todo
/// por defecto.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    /// URL del `rooms.json` para el seed del room-search UDP. Si el
    /// room-search está activo y no hay ni seed local ni nodos en la DB, el
    /// server descarga este JSON al arrancar para poder propagarse en la red
    /// de Ares (y aparecer en el buscador de salas de los clientes). Pon una
    /// cadena vacía para DESACTIVAR la descarga automática (puedes cargar el
    /// seed a mano con `astra seed-refresh` o dejando `seed_rooms.json`).
    #[serde(default = "default_seed_url")]
    pub seed_url: String,
    /// Chequeo periódico de nuevas versiones de Astra contra el registry de
    /// imágenes (ghcr.io). Cuando aparece una versión más nueva, se avisa por
    /// PM a los admins/owners conectados y se marca en el panel `/admin`.
    /// Pon `false` para que el server no haga requests salientes.
    #[serde(default = "default_update_check")]
    pub update_check: bool,

    /// Auto-owner para quien entra "desde el propio servidor" (paridad
    /// `Helpers.IsLocalHost` de sb0t, ajuste `local_host`).
    ///
    /// Con esto activo, un cliente cuya IP sea loopback o coincida con
    /// [`server_ip`](Self::server_ip) entra como **Owner**, sin captcha ni
    /// cuarentena, y exento del ban, del chequeo de proxy y del gate
    /// `onJoinCheck` de los scripts.
    ///
    /// **Desactivado por defecto**, igual que en sb0t: es un permiso total
    /// concedido por IP, y basta con que algo enmascare la IP de origen para
    /// que se lo lleve quien no debe.
    ///
    /// A diferencia de sb0t, aquí NO entran los rangos privados
    /// (`10.x`, `192.168.x`, `172.16-31.x`): Astra corre en contenedor, y
    /// con el userland proxy de Docker todas las conexiones llegarían desde
    /// el gateway `172.17.0.1` — es decir, la sala entera sería Owner.
    #[serde(default)]
    pub local_host: bool,

    /// IP pública del servidor, para [`local_host`](Self::local_host): quien
    /// se conecte desde esta misma IP se considera "local".
    ///
    /// Vacío = solo se acepta loopback. Es el equivalente del ajuste `ip` de
    /// sb0t; no se autodetecta a propósito (una detección errónea aquí
    /// reparte permisos de Owner).
    #[serde(default)]
    pub server_ip: String,
}

fn default_update_check() -> bool {
    true
}

fn default_live_scripts_endpoint() -> String {
    "https://api.github.com".to_string()
}

fn default_seed_url() -> String {
    "http://chatrooms.mywire.org/rooms.json".to_string()
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

    /// Capa 2: máx conexiones TCP simultáneas de clientes Ares nativos por IP.
    /// Default: 5
    pub max_concurrent_per_ip: u32,

    /// Cap de conexiones CRUDAS por IP (anti-Slowloris), aplicado en el
    /// multiplexado antes de clasificar — cuenta CUALQUIER conexión, incluso
    /// las que no mandaron ni un byte. Más alto que `max_concurrent_per_ip`
    /// para no romper la web detrás de un proxy (todos los usuarios web
    /// comparten la IP del proxy; se exime a las IP en `trusted_proxies`).
    /// `0` = sin límite. Default: 30.
    #[serde(default = "default_max_raw_connections_per_ip")]
    pub max_raw_connections_per_ip: u32,

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

fn default_max_raw_connections_per_ip() -> u32 {
    30
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
            max_raw_connections_per_ip: default_max_raw_connections_per_ip(),

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
            seed_url: default_seed_url(),
            update_check: true,
            local_host: false,
            server_ip: String::new(),
        }
    }
}

impl Settings {
    /// Carga la configuración desde un archivo TOML. Si no existe, usa defaults.
    pub fn load_or_default(path: &Path) -> Self {
        Self::load_reporting(path).0
    }

    /// Como [`Self::load_or_default`], pero además dice si el archivo **existe
    /// pero no se pudo leer**.
    ///
    /// Quien llama necesita distinguir ese caso de "no hay config todavía",
    /// por dos motivos: el aviso hay que dárselo al operador cuando el logging
    /// ya esté montado (aquí se emite demasiado pronto para que se vea), y
    /// sobre todo **no se debe sobrescribir un archivo que no entendimos** —
    /// si lo hiciéramos, un error de sintaxis borraría la configuración
    /// entera del operador sin dejar rastro.
    pub fn load_reporting(path: &Path) -> (Self, Option<String>) {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(s) => return (s, None),
                    Err(e) => {
                        return (
                            Self::default(),
                            Some(format!("error parseando {}: {e}", path.display())),
                        )
                    }
                },
                Err(e) => {
                    return (
                        Self::default(),
                        Some(format!("error leyendo {}: {e}", path.display())),
                    )
                }
            }
        }
        (Self::default(), None)
    }

    /// Prefijo de los valores de `guid` que vinieron de fábrica (el default
    /// del código y el del `astra.toml.example`). Se tratan igual que
    /// "vacío": son idénticos en todas las instalaciones, así que no sirven
    /// como secreto.
    pub const PLACEHOLDER_GUID_PREFIX: &'static str = "astra-default-guid";

    /// ¿El `guid` es un secreto real, o un placeholder / vacío?
    pub fn has_real_guid(&self) -> bool {
        let g = self.guid.trim();
        !g.is_empty() && !g.starts_with(Self::PLACEHOLDER_GUID_PREFIX)
    }

    /// Genera un `guid` de servidor aleatorio (32 hex) y lo asigna.
    ///
    /// Paridad `MainWindow.SetValues.cs` de sb0t: si no hay GUID guardado,
    /// se crea uno con `Guid.NewGuid()` y **se persiste** — nunca se pide al
    /// usuario que lo invente.
    ///
    /// Importa que sea único por sala: el `guid` es el secreto con el que un
    /// leaf se autentica contra un hub (`credentials = SHA1(reverse(name ++
    /// guid))`). Con un valor compartido por todas las instalaciones,
    /// cualquiera que sepa el nombre de la sala podría hacerse pasar por ella.
    pub fn regenerate_guid(&mut self) {
        use rand::RngCore;
        let mut raw = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut raw);
        self.guid = raw.iter().map(|b| format!("{:02x}", b)).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_guid_is_not_a_real_secret() {
        // El default histórico es idéntico en todas las instalaciones, así
        // que debe tratarse como "sin configurar" y disparar la generación.
        let s = Settings::default();
        assert!(!s.has_real_guid());
    }

    #[test]
    fn example_config_placeholder_is_not_a_real_secret() {
        // `astra.toml.example` traía su propia variante del placeholder; si
        // no se detecta, todo el que copie el ejemplo comparte secreto.
        let mut s = Settings::default();
        s.guid = "astra-default-guid-12".to_string();
        assert!(!s.has_real_guid());
    }

    #[test]
    fn empty_guid_is_not_a_real_secret() {
        let mut s = Settings::default();
        s.guid = "   ".to_string();
        assert!(!s.has_real_guid());
    }

    #[test]
    fn regenerate_guid_produces_a_usable_unique_secret() {
        let mut a = Settings::default();
        let mut b = Settings::default();
        a.regenerate_guid();
        b.regenerate_guid();

        assert!(a.has_real_guid());
        assert_ne!(a.guid, b.guid, "dos salas no pueden compartir el guid");
        // 16 bytes en hex. El derivador del link (`guid_bytes_from_string`)
        // toma los primeros 16 caracteres, así que el string debe ser al
        // menos así de largo para no caer en el camino del SHA1.
        assert_eq!(a.guid.len(), 32);
        assert!(a.guid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_configured_guid_is_left_alone() {
        let mut s = Settings::default();
        s.guid = "mi-guid-configurado-a-mano".to_string();
        assert!(s.has_real_guid());
    }
}
