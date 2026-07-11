//! Contexto global de la aplicación: configuración, estado, DB y ciclo de vida.

/// Logo de Astra (variante "Principal", tile naranja) usado como avatar de
/// sala/bot por defecto si el admin no subió uno propio. PNG 256×256.
pub const DEFAULT_ROOM_AVATAR: &[u8] = include_bytes!("../assets/room_avatar.png");
/// Logo de Astra (variante "Espacial", fondo oscuro) usado como avatar por
/// defecto de los usuarios que no mandan el suyo. PNG 256×256.
pub const DEFAULT_USER_AVATAR: &[u8] = include_bytes!("../assets/default_avatar.png");

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use super::accounts::AccountManager;
use super::bans::BanSystem;
use super::captcha::CaptchaManager;
use super::command_levels::CommandLevelManager;
use super::custom_data::CustomDataStore;
use super::db::Database;
use super::greets::GreetManager;
use super::motd::MotdManager;
use super::templates::TemplateManager;
use super::idle::IdleManager;
use super::geoip::GeoIp;
use super::ip_autologin::IpAutologinManager;
use super::ip_bans::{AsnBanManager, RangeBanManager};
use super::name_filters::NameFilterManager;
use super::proxy_trust::TrustedProxyManager;
use super::room_flags::RoomFlags;
use super::urls::UrlManager;
use super::word_filter::WordFilterManager;
use super::security::SecurityManager;
use super::settings::Settings;
use super::stats::Stats;
use super::user_history::UserHistory;
use super::user_pool::UserPool;
use super::vroom::VroomManager;

/// Closures inyectadas para hablar con el scripting engine (`astra_scripting`)
/// sin que este crate dependa de él (`astra_scripting` ya depende de
/// `server_core::AppContext`, así que sería una dependencia circular).
/// Mismo patrón que `RoomInfoFn`/`UserCountFn` en `crates/udp`.
pub type ListScriptsFn = Arc<dyn Fn() -> Vec<String> + Send + Sync>;
/// Carga un script por nombre. `Ok(name)` o `Err(mensaje)`.
pub type LoadScriptFn = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
/// Descarga un script por nombre.
pub type KillScriptFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Bundle de las 3 closures de gestión de scripts, seteado una sola vez en
/// `main.rs` tras arrancar el `ScriptManager`. `None` antes de ese punto
/// (p.ej. en tests que construyen un `AppContext` sin scripting).
#[derive(Clone)]
pub struct ScriptingHooks {
    /// Lista los scripts cargados.
    pub list: ListScriptsFn,
    /// Carga un script por nombre.
    pub load: LoadScriptFn,
    /// Descarga un script por nombre.
    pub kill: KillScriptFn,
}

/// Snapshot serializable de un usuario para replicación Link.
#[derive(Debug, Clone)]
pub struct LinkUserSnapshot {
    /// Nick original.
    pub org_name: String,
    /// Nick actual.
    pub name: String,
    /// Versión del cliente.
    pub version: String,
    /// GUID.
    pub guid: [u8; 16],
    /// Cantidad de archivos.
    pub file_count: u16,
    /// IP externa.
    pub external_ip: IpAddr,
    /// IP local.
    pub local_ip: IpAddr,
    /// Puerto de datos.
    pub port: u16,
    /// DNS.
    pub dns: String,
    /// Browsable.
    pub browsable: bool,
    /// Edad.
    pub age: u8,
    /// Sexo.
    pub sex: u8,
    /// País.
    pub country: u8,
    /// Región.
    pub region: String,
    /// Nivel.
    pub level: u8,
    /// Vroom.
    pub vroom: u16,
    /// Cliente custom.
    pub custom_client: bool,
    /// Muzzled.
    pub muzzled: bool,
    /// Web client.
    pub web_client: bool,
    /// Encrypted.
    pub encrypted: bool,
    /// Registered.
    pub registered: bool,
    /// Idle.
    pub idle: bool,
}

/// Request al link layer (enviado por scripting, consumido por el manager de links).
#[derive(Debug, Clone)]
pub enum LinkRequest {
    /// Crear una conexión link a `server:port` con nombre `name`.
    CreateLink { name: String, server: String, port: u16 },
    /// Desconectar el link con ese nombre.
    DisconnectLink { name: String },
    /// Forzar desconexión de un hub (kick).
    KickHub { name: String },
}

impl LinkUserSnapshot {
    pub fn from_user(user: &crate::user_pool::AresUser) -> Self {
        Self {
            org_name: user.org_name.read().clone(),
            name: user.name.read().clone(),
            version: user.version.clone(),
            guid: user.guid,
            file_count: user.file_count,
            external_ip: user.external_ip,
            local_ip: user.local_ip,
            port: user.data_port,
            dns: user.dns.read().clone(),
            browsable: user.browsable,
            age: user.age,
            sex: user.sex,
            country: user.country,
            region: user.region.clone(),
            level: *user.level.read() as u8,
            vroom: *user.vroom.read(),
            custom_client: user.custom_client,
            muzzled: user.muzzled.load(std::sync::atomic::Ordering::Relaxed),
            web_client: user.web_client,
            encrypted: user.encrypted,
            registered: user.registered,
            idle: user.idle,
        }
    }
}

/// Evento replicable por Link entre servidores.
#[derive(Debug, Clone)]
pub enum LinkEvent {
    /// Un usuario se unió.
    Join {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Snapshot del usuario.
        user: LinkUserSnapshot,
    },
    /// Un usuario actualizó su estado visible.
    UserUpdated {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Snapshot del usuario.
        user: LinkUserSnapshot,
    },
    /// Un usuario cambió de nick.
    NickChanged {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Nick anterior.
        old_name: String,
        /// Snapshot actualizado del usuario.
        user: LinkUserSnapshot,
    },
    /// Un usuario cambió de vroom.
    VroomChanged {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Snapshot actualizado del usuario.
        user: LinkUserSnapshot,
    },
    /// Un usuario cambió su custom name.
    CustomName {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Nick del usuario.
        name: String,
        /// Nuevo custom name; `None` limpia el valor.
        custom_name: Option<String>,
    },
    /// Un usuario salió.
    Part {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Nick del usuario.
        name: String,
    },
    /// Texto público.
    Public {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Emisor.
        from: String,
        /// Texto.
        text: String,
    },
    /// Emote.
    Emote {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Emisor.
        from: String,
        /// Texto.
        text: String,
    },
    /// PM entre usuarios.
    Private {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Emisor.
        from: String,
        /// Destinatario.
        to: String,
        /// Texto.
        text: String,
    },
    /// Public dirigido a un usuario específico.
    PublicToUser {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Emisor.
        from: String,
        /// Destinatario.
        to: String,
        /// Texto.
        text: String,
    },
    /// Emote dirigido a un usuario específico.
    EmoteToUser {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Emisor.
        from: String,
        /// Destinatario.
        to: String,
        /// Texto.
        text: String,
    },
    /// El destinatario ignora al emisor de un privado.
    PrivateIgnored {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Emisor original del PM.
        from: String,
        /// Destinatario que está ignorando.
        to: String,
    },
    /// Personal message.
    PersonalMessage {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Usuario.
        name: String,
        /// Texto.
        text: String,
    },
    /// Acción admin de red (`host*`): ban/kick/muzzle/unmuzzle propagada a
    /// todos los servidores enlazados. Cada servidor la aplica a su pool local.
    AdminAction {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Tipo de acción (ver [`admin_action`]).
        kind: u8,
        /// Nick del usuario objetivo.
        target: String,
    },
    /// Mensaje Link genérico para opcodes no modelados explícitamente.
    Raw {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Opcode Link crudo.
        msg: u8,
        /// Payload crudo del mensaje.
        payload: Vec<u8>,
    },
}

/// Tipos de acción admin de red para [`LinkEvent::AdminAction`].
pub mod admin_action {
    /// Ban permanente.
    pub const BAN: u8 = 1;
    /// Kick (desconexión).
    pub const KICK: u8 = 2;
    /// Muzzle (silenciar).
    pub const MUZZLE: u8 = 3;
    /// Unmuzzle.
    pub const UNMUZZLE: u8 = 4;
}

/// Máximo de mensajes retenidos en el historial reciente.
const MESSAGE_HISTORY_CAP: usize = 50;

/// Una entrada del historial de mensajes recientes.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// Nick del emisor.
    pub name: String,
    /// Texto del mensaje.
    pub text: String,
    /// ¿Es un emote?
    pub is_emote: bool,
    /// Momento (epoch secs).
    pub time_secs: u64,
}

/// Estado global compartido del servidor.
///
/// Se pasa por `Arc` a todos los módulos. Es la "raíz" del grafo de
/// dependencias del server.
pub struct AppContext {
    /// Configuración cargada.
    pub settings: Arc<Settings>,
    /// Estadísticas globales.
    pub stats: Arc<Stats>,
    /// Pool de usuarios conectados.
    pub user_pool: Arc<UserPool>,
    /// Base de datos SQLite.
    pub db: Arc<Database>,
    /// Sistema de bans.
    pub bans: Arc<BanSystem>,
    /// Historial de usuarios (join flood).
    pub user_history: Arc<UserHistory>,
    /// Manager de cuentas.
    pub accounts: Arc<AccountManager>,
    /// Manager de seguridad (5 capas anti-DDoS).
    pub security: Arc<SecurityManager>,
    /// Manager de captchas (gate opcional para IPs nuevas).
    pub captcha: Arc<CaptchaManager>,
    /// Manager de vrooms (canales virtuales dentro de la sala).
    pub vrooms: Arc<VroomManager>,
    /// Manager de idle (detecta transitions active↔idle).
    pub idle: Arc<IdleManager>,
    /// Manager de greets (mensajes de bienvenida).
    pub greets: Arc<GreetManager>,
    /// MOTD (message of the day) mostrado al entrar a la sala.
    pub motd: Arc<MotdManager>,
    /// Textos del sistema editables ("templates").
    pub templates: Arc<TemplateManager>,
    /// Manager de filtros de palabras del chat público.
    pub word_filter: Arc<WordFilterManager>,
    /// Manager de URLs rotadas de la sala.
    pub urls: Arc<UrlManager>,
    /// Range bans (prefijos de IP).
    pub range_bans: Arc<RangeBanManager>,
    /// ASN bans.
    pub asn_bans: Arc<AsnBanManager>,
    /// Flags de sala (toggles caps/scribbles/audios/...).
    pub room_flags: Arc<RoomFlags>,
    /// Transferencias CUSTOM_DATA públicas en curso (imágenes/audio ib0t).
    pub custom_data: Arc<CustomDataStore>,
    /// Transferencias CUSTOM_DATA privadas (PM) en curso.
    pub pm_custom_data: Arc<CustomDataStore>,
    /// Filtros de nick para el login (`/joinfilter`).
    pub join_filters: Arc<NameFilterManager>,
    /// Filtros de nombres de archivo (`/filefilter`).
    pub file_filters: Arc<NameFilterManager>,
    /// Niveles de permiso configurables por comando (`/cmdlevel`).
    pub command_levels: Arc<CommandLevelManager>,
    /// Proxies reversos confiables para resolver la IP real detrás de
    /// `X-Forwarded-For`/`X-Real-IP` en el path WS (panel Proxy).
    pub trusted_proxies: Arc<TrustedProxyManager>,
    /// Auto-nivel por reconocimiento de IP+GUID sin cuenta (`/addautologin`).
    pub ip_autologins: Arc<IpAutologinManager>,
    /// Avatar de sala (bot), en memoria. Persistido en
    /// `<data_dir>/avatars/server`. `None` = sin avatar de sala configurado.
    pub server_avatar: RwLock<Option<Vec<u8>>>,
    /// Avatar default asignado a usuarios Ares nativos que no mandan el
    /// suyo (paridad `Avatars.CheckAvatars` de sb0t). Persistido en
    /// `<data_dir>/avatars/default`.
    pub default_avatar: RwLock<Option<Vec<u8>>>,
    /// Closures hacia el scripting engine (`/listscripts`, `/loadscript`,
    /// `/killscript`). `None` hasta que `main.rs` las setea tras arrancar
    /// el `ScriptManager`.
    pub scripting_hooks: RwLock<Option<ScriptingHooks>>,
    /// Resolución GeoIP/ASN (bases MMDB opcionales en `data_dir`).
    pub geoip: Arc<GeoIp>,
    /// Log reciente de acciones de ban para `/banstats`: `(banner, target, ip)`.
    pub ban_log: parking_lot::Mutex<std::collections::VecDeque<(String, String, String)>>,
    /// Si está activo, los comandos admin se ignoran salvo para el Owner
    /// (`/disableadmins`). Paridad con `Settings.DisableAdmins` de sb0t.
    pub admins_disabled: std::sync::atomic::AtomicBool,
    /// Ruta del archivo de config (`astra.toml`) para que el panel admin
    /// pueda leerlo/escribirlo. `None` si se arrancó sin archivo.
    pub config_path: RwLock<Option<std::path::PathBuf>>,
    /// Snapshot de nodos UDP conocidos (name, port, user_count).
    /// Actualizado por `UdpNodeManager` cuando se agregan/actualizan nodos.
    pub udp_nodes: parking_lot::RwLock<Vec<(String, u16, u32)>>,
    /// Snapshot de links activos: (name, port, is_connected, users_count).
    /// Actualizado por `LinkClient`/`LinkServer` cuando cambian.
    pub link_servers: parking_lot::RwLock<Vec<(String, u16, bool)>>,
    /// Users remotos conocidos via link: (link_name, user_name).
    /// Cada link actualiza su lista cuando recibe userlist.
    pub link_users: parking_lot::RwLock<Vec<(String, String)>>,
    /// Bus de requests al link layer: `Link_createLink`, `Link_disconnect`, etc.
    pub link_requests: broadcast::Sender<LinkRequest>,
    /// Instante de arranque (para calcular uptime).
    pub start_time: Instant,
    /// Topic actual de la sala (mutable en runtime).
    pub room_topic: RwLock<String>,
    /// Status de la sala (mostrado en `/roominfo`, mutable via `/status`).
    pub room_status: RwLock<String>,
    /// Historial reciente de mensajes públicos/emotes (ring buffer para
    /// `/history`). Entradas `(name, text, is_emote, time_secs)`.
    pub message_history: parking_lot::Mutex<std::collections::VecDeque<HistoryEntry>>,
    /// Bus interno de eventos Link.
    pub link_events: broadcast::Sender<LinkEvent>,
}

impl AppContext {
    /// Crea un nuevo contexto con la configuración y base de datos dadas.
    pub fn new(settings: Settings, db: Arc<Database>) -> Self {
        let initial_room_topic = settings.room_topic.clone();
        let stats = Arc::new(Stats::new());
        let user_pool = Arc::new(UserPool::new());
        let bans = Arc::new(BanSystem::new(db.clone()));
        let user_history = Arc::new(UserHistory::new(db.clone()));
        let accounts = Arc::new(AccountManager::new(db.clone()));
        let security = SecurityManager::new(settings.security.clone());
        let captcha = Arc::new(CaptchaManager::new(
            settings.security.captcha_expiration_secs,
            settings.security.captcha_max_attempts,
        ));
        let vrooms = Arc::new(VroomManager::new());
        let idle = Arc::new(IdleManager::new());
        let greets = Arc::new(GreetManager::new(db.clone()));
        let motd = Arc::new(MotdManager::new(db.clone()));
        let templates = Arc::new(TemplateManager::new(db.clone()));
        let word_filter = Arc::new(WordFilterManager::new(db.clone()));
        let urls = Arc::new(UrlManager::new(db.clone()));
        let range_bans = Arc::new(RangeBanManager::new(db.clone()));
        let asn_bans = Arc::new(AsnBanManager::new(db.clone()));
        let room_flags = Arc::new(RoomFlags::new(db.clone()));
        let custom_data = Arc::new(CustomDataStore::new());
        let pm_custom_data = Arc::new(CustomDataStore::new());
        let join_filters = Arc::new(NameFilterManager::new(db.clone(), "join"));
        let file_filters = Arc::new(NameFilterManager::new(db.clone(), "file"));
        let command_levels = Arc::new(CommandLevelManager::new(db.clone()));
        let trusted_proxies = Arc::new(TrustedProxyManager::new(db.clone()));
        let ip_autologins = Arc::new(IpAutologinManager::new(db.clone()));
        let avatars_dir = std::path::Path::new(&settings.data_dir).join("avatars");
        // Si el admin no subió un avatar propio, se usa el logo de Astra como
        // default: la variante naranja ("Principal") para el bot/sala y la
        // variante espacial para los usuarios sin avatar.
        let server_avatar = RwLock::new(Some(
            std::fs::read(avatars_dir.join("server"))
                .unwrap_or_else(|_| DEFAULT_ROOM_AVATAR.to_vec()),
        ));
        let default_avatar = RwLock::new(Some(
            std::fs::read(avatars_dir.join("default"))
                .unwrap_or_else(|_| DEFAULT_USER_AVATAR.to_vec()),
        ));
        let geoip = Arc::new(GeoIp::load(std::path::Path::new(&settings.data_dir)));
        let (link_events, _) = broadcast::channel(1024);
        Self {
            settings: Arc::new(settings),
            stats,
            user_pool,
            db,
            bans,
            user_history,
            accounts,
            security,
            captcha,
            vrooms,
            idle,
            greets,
            motd,
            templates,
            word_filter,
            urls,
            range_bans,
            asn_bans,
            room_flags,
            custom_data,
            pm_custom_data,
            join_filters,
            file_filters,
            command_levels,
            trusted_proxies,
            ip_autologins,
            server_avatar,
            default_avatar,
            scripting_hooks: RwLock::new(None),
            geoip,
            ban_log: parking_lot::Mutex::new(std::collections::VecDeque::new()),
            admins_disabled: std::sync::atomic::AtomicBool::new(false),
            config_path: RwLock::new(None),
            udp_nodes: parking_lot::RwLock::new(Vec::new()),
            link_servers: parking_lot::RwLock::new(Vec::new()),
            link_users: parking_lot::RwLock::new(Vec::new()),
            link_requests: broadcast::channel(256).0,
            start_time: Instant::now(),
            room_topic: RwLock::new(initial_room_topic),
            room_status: RwLock::new(String::new()),
            message_history: parking_lot::Mutex::new(std::collections::VecDeque::new()),
            link_events,
        }
    }

    /// Uptime en segundos.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Retorna una copia del topic actual.
    pub fn current_room_topic(&self) -> String {
        self.room_topic.read().clone()
    }

    /// Actualiza el topic actual en memoria.
    pub fn set_room_topic(&self, topic: impl Into<String>) {
        *self.room_topic.write() = topic.into();
    }

    /// Retorna el status actual de la sala.
    pub fn room_status(&self) -> String {
        self.room_status.read().clone()
    }

    /// Actualiza el status de la sala.
    pub fn set_room_status(&self, status: impl Into<String>) {
        *self.room_status.write() = status.into();
    }

    /// Registra la ruta del archivo de config (para el panel admin).
    pub fn set_config_path(&self, path: std::path::PathBuf) {
        *self.config_path.write() = Some(path);
    }

    /// Ruta del archivo de config, si se conoce.
    pub fn config_path(&self) -> Option<std::path::PathBuf> {
        self.config_path.read().clone()
    }

    /// Registra un mensaje en el historial reciente (ring buffer acotado).
    pub fn record_message(&self, name: &str, text: &str, is_emote: bool) {
        let mut hist = self.message_history.lock();
        if hist.len() >= MESSAGE_HISTORY_CAP {
            hist.pop_front();
        }
        hist.push_back(HistoryEntry {
            name: name.to_string(),
            text: text.to_string(),
            is_emote,
            time_secs: crate::time::unix_time() / 1000,
        });
    }

    /// Retorna las últimas `n` entradas del historial (más viejas primero).
    pub fn recent_messages(&self, n: usize) -> Vec<HistoryEntry> {
        let hist = self.message_history.lock();
        let start = hist.len().saturating_sub(n);
        hist.iter().skip(start).cloned().collect()
    }

    /// Registra una acción de ban en el log (para `/banstats`).
    pub fn record_ban(&self, banner: &str, target: &str, ip: &str) {
        let mut log = self.ban_log.lock();
        if log.len() >= 50 {
            log.pop_front();
        }
        log.push_back((banner.to_string(), target.to_string(), ip.to_string()));
    }

    /// Retorna las últimas `n` acciones de ban (más viejas primero).
    pub fn recent_bans(&self, n: usize) -> Vec<(String, String, String)> {
        let log = self.ban_log.lock();
        let start = log.len().saturating_sub(n);
        log.iter().skip(start).cloned().collect()
    }

    /// Notifica (PM del bot) a los admins suscritos a un feed dado.
    ///
    /// `select` elige, para cada usuario, si recibe el mensaje. Se usa para
    /// `/ipsend`, `/logsend`, `/bansend` (y filtros de nivel/vroom).
    pub fn notify_subscribers<F>(&self, text: &str, select: F)
    where
        F: Fn(&crate::user_pool::AresUser) -> bool,
    {
        let pkt = crate::outbound::build_pvt(&self.settings.bot_name, text);
        for u in self.user_pool.users() {
            if u.logged_in && select(&u) {
                let _ = u.send(pkt.clone());
            }
        }
    }

    /// Publica un evento para replicación Link.
    pub fn publish_link_event(&self, event: LinkEvent) {
        let _ = self.link_events.send(event);
    }

    /// Aplica una acción admin de red ([`LinkEvent::AdminAction`]) al pool
    /// local: usada al recibir un `host*` desde otro servidor enlazado. Retorna
    /// `true` si el objetivo estaba conectado localmente y se aplicó.
    pub fn apply_admin_action(&self, kind: u8, target: &str) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        let Some(user) = self.user_pool.get_by_name(target) else {
            return false;
        };
        match kind {
            admin_action::BAN => {
                let _ = self.bans.ban(
                    &user.name.read(),
                    &user.version,
                    &user.guid,
                    user.external_ip,
                    user.local_ip,
                    user.data_port,
                );
                self.remove_and_broadcast_part(&user);
            }
            admin_action::KICK => {
                self.remove_and_broadcast_part(&user);
            }
            admin_action::MUZZLE => {
                user.muzzled.store(true, Relaxed);
            }
            admin_action::UNMUZZLE => {
                user.muzzled.store(false, Relaxed);
            }
            _ => return false,
        }
        true
    }

    /// Remueve un usuario del pool y difunde su PART a la sala (cifrando por
    /// destinatario). Helper interno para acciones admin recibidas por Link.
    fn remove_and_broadcast_part(&self, user: &std::sync::Arc<crate::user_pool::AresUser>) {
        let part = crate::outbound::build_part(user);
        self.user_pool.remove(user.id);
        self.stats.on_user_part();
        for u in self.user_pool.users() {
            if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            if u.ares_crypto.is_some() {
                let _ = u.send(crate::outbound::build_part_c(user, u.ares_crypto));
            } else {
                let _ = u.send(part.clone());
            }
        }
    }

    /// Crea una suscripción al bus interno de Link.
    pub fn subscribe_link_events(&self) -> broadcast::Receiver<LinkEvent> {
        self.link_events.subscribe()
    }

    /// Cantidad de suscriptores Link activos.
    pub fn link_receiver_count(&self) -> usize {
        self.link_events.receiver_count()
    }
}
