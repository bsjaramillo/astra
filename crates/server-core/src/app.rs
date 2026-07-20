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
    /// Envío dirigido a UN leaf (paridad ILeaf de sb0t: leaf.print/sendText/
    /// sendEmote/scribble). En el leaf lo emite el scripting (origin=None) y
    /// el LinkClient lo manda al hub; en el hub lo emite el reader de la
    /// conexión origen (origin=Some) y lo consume SOLO la conexión cuyo
    /// ident coincide.
    ToLeaf {
        /// Nombre del leaf origen si vino desde Link.
        origin: Option<String>,
        /// Ident del leaf destino (asignado por el hub).
        target_ident: u32,
        /// Qué mandar.
        payload: LeafDirected,
    },
    /// Un leaf se conectó/desconectó del hub (para que los demás leaves
    /// mantengan su lista de peers — paridad HubLeafConnected/Disconnected).
    LeafAnnounce {
        /// Ident del leaf anunciado.
        ident: u32,
        /// Nombre.
        name: String,
        /// IP externa.
        ip: std::net::IpAddr,
        /// Puerto.
        port: u16,
        /// true = conectado, false = desconectado.
        connected: bool,
    },
}

/// Payload de un envío dirigido a un leaf (paridad de los MSG_LINK_LEAF_*
/// de sb0t: PRINT_ALL/PRINT_VROOM/PRINT_LEVEL/PUBLIC_TO_LEAF/EMOTE_TO_LEAF/
/// SCRIBBLE_LEAF).
#[derive(Debug, Clone)]
pub enum LeafDirected {
    /// Línea de sistema a todos los usuarios del leaf (op 60).
    PrintAll { text: String },
    /// Línea de sistema a un vroom del leaf (op 61).
    PrintVroom { vroom: u16, text: String },
    /// Línea de sistema a usuarios con nivel > `level` (op 62).
    PrintLevel { level: u8, text: String },
    /// Texto público como `from` en el leaf (op 90).
    Public { from: String, text: String },
    /// Emote como `from` en el leaf (op 91).
    Emote { from: String, text: String },
    /// Scribble a los custom clients del leaf (op 34).
    Scribble { from: String, height: u32, data: Vec<u8> },
}

/// Un leaf conocido vía link: en el hub, sus leaves conectados; en un leaf,
/// los OTROS leaves del hub (aprendidos por LeafAnnounce). Es lo que expone
/// `Link.leaves()` al scripting (paridad JSLeaf).
#[derive(Debug, Clone)]
pub struct LinkLeafInfo {
    /// Ident asignado por el hub.
    pub ident: u32,
    /// Nombre de la sala del leaf.
    pub name: String,
    /// IP externa.
    pub ip: std::net::IpAddr,
    /// Puerto.
    pub port: u16,
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
    /// Gate de cambio de vroom (onVroomJoinCheck), registrado por `main`.
    pub vroom_check: parking_lot::RwLock<Option<Box<dyn Fn(&str, u16) -> bool + Send + Sync>>>,
    /// Versión más nueva de Astra disponible, detectada por el update check
    /// periódico (`None` = al día). La consumen los avisos a admins/owners
    /// y el estado del panel `/admin`.
    pub available_update: parking_lot::RwLock<Option<String>>,
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
    /// IP externa del server, reportada por un peer de la red Ares en el
    /// handshake `READYTOCHECKFIREWALL` del room-search UDP (paridad sb0t
    /// `Settings.ExternalIP`, `UdpProcessor.ReadyToCheckFirewall`). `None`
    /// hasta que llegue el primer reporte (o si el room-search está apagado).
    pub external_ip: parking_lot::RwLock<Option<std::net::IpAddr>>,
    /// Hub al que este server está conectado como leaf: (name, ip, port).
    /// Seteado por el consumer de `LinkRequest::CreateLink` / `--link-client`
    /// (paridad sb0t `Server.Link`: el leaf conoce el hub por el request de
    /// conexión, no por el protocolo). `None` = no linkeado.
    pub link_hub: parking_lot::RwLock<Option<(String, std::net::IpAddr, u16)>>,
    /// Leaves conocidos vía link (ver [`LinkLeafInfo`]). En el hub los
    /// registra cada conexión; en un leaf los llena LeafAnnounce.
    pub link_leaves: parking_lot::RwLock<Vec<LinkLeafInfo>>,
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
    /// Historial reciente de usuarios que se desconectaron (ring buffer para
    /// `Users.records()` del scripting, paridad sb0t). Cap 50, más reciente
    /// al frente.
    pub user_records: parking_lot::RwLock<std::collections::VecDeque<UserRecord>>,
}

/// Registro histórico de un usuario que estuvo conectado (para
/// `Users.records()` del scripting).
#[derive(Debug, Clone)]
pub struct UserRecord {
    /// Nombre.
    pub name: String,
    /// IP externa.
    pub external_ip: std::net::IpAddr,
    /// IP local.
    pub local_ip: std::net::IpAddr,
    /// Versión del cliente.
    pub version: String,
    /// Puerto de datos.
    pub port: u16,
    /// GUID (16 bytes).
    pub guid: [u8; 16],
    /// DNS.
    pub dns: String,
    /// Momento de conexión (epoch secs).
    pub join_time: u64,
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
            vroom_check: parking_lot::RwLock::new(None),
            available_update: parking_lot::RwLock::new(None),
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
            external_ip: parking_lot::RwLock::new(None),
            link_hub: parking_lot::RwLock::new(None),
            link_leaves: parking_lot::RwLock::new(Vec::new()),
            link_servers: parking_lot::RwLock::new(Vec::new()),
            link_users: parking_lot::RwLock::new(Vec::new()),
            link_requests: broadcast::channel(256).0,
            start_time: Instant::now(),
            room_topic: RwLock::new(initial_room_topic),
            room_status: RwLock::new(String::new()),
            message_history: parking_lot::Mutex::new(std::collections::VecDeque::new()),
            link_events,
            user_records: parking_lot::RwLock::new(std::collections::VecDeque::new()),
        }
    }

    /// Uptime en segundos.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Registra en el historial que un usuario se desconectó (para
    /// `Users.records()` del scripting). Ring buffer de 50, más reciente al
    /// frente; evita duplicar el mismo GUID consecutivo.
    pub fn record_departure(&self, user: &std::sync::Arc<crate::user_pool::AresUser>) {
        let rec = UserRecord {
            name: user.name.read().clone(),
            external_ip: user.external_ip,
            local_ip: user.local_ip,
            version: user.version.clone(),
            port: user.data_port,
            guid: user.guid,
            dns: user.dns.read().clone(),
            join_time: user.join_time,
        };
        let mut recs = self.user_records.write();
        recs.push_front(rec);
        recs.truncate(50);
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

    /// Replay del historial de chat al usuario que entra, si el flag de sala
    /// `history` está activo. Paridad sb0t `commands/History.Show`
    /// (`ServerEvents.cs:186`): últimos 20 públicos/emotes reproducidos como
    /// mensajes del nick original con prefijo `[-HH:MM:SS]` (antigüedad del
    /// mensaje) y una línea de cierre.
    pub fn replay_history(&self, user: &crate::user_pool::AresUser) {
        if !self.room_flags.get("history") {
            return;
        }
        let msgs = self.recent_messages(20);
        if msgs.is_empty() {
            return;
        }
        let now = crate::time::unix_time() / 1000;
        for m in &msgs {
            let offset = now.saturating_sub(m.time_secs);
            let (h, rem) = (offset / 3600, offset % 3600);
            let (mi, s) = (rem / 60, rem % 60);
            let text = format!("[-{:02}:{:02}:{:02}] {}", h, mi, s, m.text);
            if m.is_emote {
                user.send_emote(&m.name, &text);
            } else {
                user.send_public(&m.name, &text);
            }
        }
        // Template `Notification#7` de sb0t (overrideable vía catálogo).
        let closing = self.templates.resolve("-=-=-=-=- end of chat history -=-=-=-=-");
        user.print(&self.settings.bot_name, &closing);
    }

    /// Si el flag `lastseen` está activo, anuncia a la sala con qué nick y
    /// cuándo se vio por última vez la IP del usuario que entra (paridad
    /// sb0t `ServerEvents.cs:198`, Notification#6: "+n was last seen as +o
    /// at +t from +ip"). No-op si no hay historial previo.
    pub fn announce_last_seen(&self, user: &crate::user_pool::AresUser) {
        if !self.room_flags.get("lastseen") {
            return;
        }
        let ip = user.external_ip.to_string();
        let name = user.name.read().clone();
        // Último avistamiento ANTERIOR a este join (el join actual ya está
        // grabado en el historial — sin el cutoff, el usuario "se encontraba
        // a sí mismo": "X was last seen as X at <recién>").
        let Ok(Some((old_name, last_seen))) =
            self.db.last_seen_before(&ip, user.join_time as i64)
        else {
            return;
        };
        // Paridad sb0t `Whowas.Last` (Whowas.cs:72): SOLO se anuncia si el
        // último registro usó OTRO nick. Mismo nick → silencio.
        if old_name == name {
            return;
        }
        let old_name = &old_name;
        let last_seen = &last_seen;
        // `last_seen` viene en milisegundos (ver user_history::add_user).
        let when = chrono::DateTime::from_timestamp(*last_seen / 1000, 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        let text = self.templates.render(
            "lastseen.join",
            &[("+n", &name), ("+o", old_name), ("+t", &when), ("+ip", &ip)],
        );
        self.broadcast_print(&text);
    }

    /// Broadcast de una línea de sistema (texto del server) a todos los
    /// usuarios logueados. Equivalente a `Server.Print` de sb0t.
    pub fn broadcast_print(&self, text: &str) {
        for u in self.user_pool.users() {
            if u.logged_in {
                u.print(&self.settings.bot_name, text);
            }
        }
    }

    /// Marca al usuario como idle (acción manual: comando `idle`/`idles` o
    /// emote que empieza con `idles`). Respeta el cooldown de 5 min. Si el
    /// flag de sala `idle` está activo, anuncia `+n idles at +t` (paridad
    /// sb0t `ServerEvents.Idled`). Retorna `true` si se marcó.
    pub fn mark_user_idle(&self, user: &crate::user_pool::AresUser) -> bool {
        if !self.idle.try_idle(user.id) {
            return false;
        }
        if self.room_flags.get("idle") {
            let name = user.name.read().clone();
            let text = self
                .templates
                .render("idle.enter", &[("+n", &name), ("+t", &clock_hhmm())]);
            self.broadcast_print(&text);
        }
        true
    }

    /// Si el usuario estaba idle, lo saca (habló en público o emoteó) y
    /// anuncia el tiempo ausente (paridad sb0t `ServerEvents.Unidled`,
    /// templates Idle#1-4 según la magnitud). Retorna `Some(segundos)`.
    pub fn unidle_user(&self, user: &crate::user_pool::AresUser) -> Option<u64> {
        let secs = self.idle.unidle(user.id)?;
        if self.room_flags.get("idle") {
            let d = secs / 86_400;
            let h = (secs % 86_400) / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            let name = user.name.read().clone();
            let t = clock_hhmm();
            let (d_s, h_s, m_s, s_s) = (d.to_string(), h.to_string(), m.to_string(), s.to_string());
            let text = if d > 0 {
                self.templates.render(
                    "idle.return.d",
                    &[("+n", &name), ("+t", &t), ("+d", &d_s), ("+h", &h_s), ("+m", &m_s), ("+s", &s_s)],
                )
            } else if h > 0 {
                self.templates.render(
                    "idle.return.h",
                    &[("+n", &name), ("+t", &t), ("+h", &h_s), ("+m", &m_s), ("+s", &s_s)],
                )
            } else if m > 0 {
                self.templates.render(
                    "idle.return.m",
                    &[("+n", &name), ("+t", &t), ("+m", &m_s), ("+s", &s_s)],
                )
            } else {
                self.templates
                    .render("idle.return.s", &[("+n", &name), ("+t", &t), ("+s", &s_s)])
            };
            self.broadcast_print(&text);
        }
        Some(secs)
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

    /// Expulsión inmediata: remueve al usuario del pool y difunde su PART a
    /// toda la sala (clientes Ares y web). NO registra la salida en el
    /// historial — eso lo hace el cleanup del socket al cerrarse. Usado por
    /// `/kick`, el hijack de login, y `user.kick()/ban()` desde scripts.
    /// Saca al usuario del pool SIN difundir PART ni anuncio — paridad
    /// `AresClient.Disconnect(ghost: true)` de sb0t: es lo que usa el hijack
    /// de login cuando el mismo usuario reconecta (misma IP, mismo nick). La
    /// sala nunca ve "has parted" y el nombre no desaparece de la userlist:
    /// la sesión nueva lo reemplaza sin parpadeo. El cleanup del socket viejo
    /// tampoco va a anunciar nada (detecta que ya no está en el pool).
    pub fn ghost_part_user(&self, target: &std::sync::Arc<crate::user_pool::AresUser>) {
        self.user_pool.remove(target.id);
        self.stats.on_user_part();
    }

    /// ¿La muerte de esta sesión debe ser SILENCIOSA (ghost)? Sí cuando la
    /// sesión ya no está en el pool (un hijack de login la sacó antes) o
    /// cuando OTRA sesión viva usa el mismo nick (el usuario ya reconectó —
    /// p.ej. cambió de red — y difundir el PART borraría de las userlists a
    /// la sesión nueva, que los clientes indexan por nombre).
    pub fn is_ghost_departure(&self, id: u16, name: &str) -> bool {
        if self.user_pool.get(id).is_none() {
            return true;
        }
        self.user_pool
            .users()
            .iter()
            .any(|u| u.id != id && u.logged_in && u.name.read().eq_ignore_ascii_case(name))
    }

    pub fn force_part_user(&self, target: &std::sync::Arc<crate::user_pool::AresUser>) {
        let tname = target.name.read().clone();
        let ws_part = format!("PART:{}:{}", tname.encode_utf16().count(), tname);

        self.user_pool.remove(target.id);
        self.stats.on_user_part();

        for u in self.user_pool.users() {
            if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            if let Some(tx) = &u.ws_text_sender {
                let _ = tx.send(ws_part.clone());
            } else {
                let _ = u.send(crate::outbound::build_part_c(target, u.ares_crypto));
            }
        }
    }

    /// Remueve un usuario del pool y difunde su PART a la sala (cifrando por
    /// destinatario). Helper interno para acciones admin recibidas por Link.
    fn remove_and_broadcast_part(&self, user: &std::sync::Arc<crate::user_pool::AresUser>) {
        let part = crate::outbound::build_part(user);
        self.record_departure(user);
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

    /// Registra el gate de cambio de vroom (lo setea `main` con un closure
    /// que consulta `onVroomJoinCheck` en el scripting; server-core no puede
    /// depender del crate de scripting directamente).
    pub fn set_vroom_check(&self, f: Box<dyn Fn(&str, u16) -> bool + Send + Sync>) {
        *self.vroom_check.write() = Some(f);
    }

    /// ¿Los scripts permiten que `name` entre al vroom `vroom`? `true` si no
    /// hay gate registrado (paridad sb0t `VroomJoinCheck`).
    pub fn check_vroom_join(&self, name: &str, vroom: u16) -> bool {
        match self.vroom_check.read().as_ref() {
            Some(f) => f(name, vroom),
            None => true,
        }
    }

    /// Última versión nueva de Astra conocida (si hay una pendiente).
    pub fn available_update(&self) -> Option<String> {
        self.available_update.read().clone()
    }

    /// Avisa a un usuario que hay una versión nueva del server: PM real del
    /// bot (ventana privada en clientes web) y, para web, también una línea
    /// en la ventana principal. Para clientes Ares el PM del bot ya es la
    /// única superficie (print == Pmt), así que no se duplica.
    pub fn send_update_notice(&self, user: &crate::user_pool::AresUser, new_version: &str) {
        let text = self
            .templates
            .render("update.available", &[("+v", new_version), ("+c", crate::VERSION)]);
        let _ = user.send_pm(&self.settings.bot_name, &text);
        if user.ws_text_sender.is_some() {
            let _ = user.print(&self.settings.bot_name, &text);
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

/// Hora local `HH:MM` (paridad `Helpers.Time()` de sb0t, usada en los
/// anuncios de idle).
fn clock_hhmm() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_ctx() -> AppContext {
        let db = crate::db::Database::in_memory().unwrap();
        AppContext::new(Settings::default(), db)
    }

    fn add_user(ctx: &AppContext, id: u16, name: &str) -> std::sync::Arc<crate::user_pool::AresUser> {
        let mut u = crate::user_pool::AresUser::new(
            id,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, id as u8)),
            [id as u8; 16],
        );
        u.logged_in = true;
        *u.name.write() = name.to_string();
        ctx.user_pool.add(std::sync::Arc::new(u));
        ctx.user_pool.get(id).unwrap()
    }

    /// Regresión del lastseen que "se encontraba a sí mismo": solo debe
    /// anunciar si el ÚLTIMO registro previo de la IP usó OTRO nick
    /// (paridad sb0t Whowas.Last, Whowas.cs:72).
    #[test]
    fn last_seen_only_announces_previous_different_nick() {
        let ctx = make_ctx();
        let ip = "10.0.0.1";

        // Sin historial previo → None.
        assert_eq!(ctx.db.last_seen_before(ip, 1_000_000).unwrap(), None);

        // Sesión vieja con OTRO nick (last_seen = 500k), y el join ACTUAL ya
        // grabado (last_seen = 1M, mismo instante que join_time).
        ctx.db
            .execute(
                "INSERT INTO user_history (name, version, guid, externalip, localip, port, join_time, last_seen) \
                 VALUES ('ViejoNick', 'v', 'g', ?1, 'l', 0, 400000, 500000)",
                rusqlite::params![ip],
            )
            .unwrap();
        ctx.db
            .execute(
                "INSERT INTO user_history (name, version, guid, externalip, localip, port, join_time, last_seen) \
                 VALUES ('ElMago', 'v', 'g', ?1, 'l', 0, 1000000, 1000000)",
                rusqlite::params![ip],
            )
            .unwrap();

        // El cutoff excluye el join actual: encuentra al nick viejo.
        let (name, ls) = ctx.db.last_seen_before(ip, 1_000_000).unwrap().unwrap();
        assert_eq!(name, "ViejoNick");
        assert_eq!(ls, 500_000);

        // Si el registro previo más reciente es el MISMO nick, el anuncio
        // debe callar (lo verifica announce_last_seen comparando nombres):
        ctx.db
            .execute(
                "INSERT INTO user_history (name, version, guid, externalip, localip, port, join_time, last_seen) \
                 VALUES ('ElMago', 'v', 'g', ?1, 'l', 0, 700000, 800000)",
                rusqlite::params![ip],
            )
            .unwrap();
        let (name, _) = ctx.db.last_seen_before(ip, 1_000_000).unwrap().unwrap();
        assert_eq!(name, "ElMago", "el previo más reciente ahora es el mismo nick");
        // announce_last_seen con name == "ElMago" → silencio (rama old_name == name).
    }

    /// Regresión del "User has parted" fantasma tras un cambio de red: la
    /// muerte de una sesión debe ser SILENCIOSA (ghost) si otra sesión viva
    /// usa el mismo nick, o si un hijack de login ya la sacó del pool.
    #[test]
    fn ghost_departure_detection() {
        let ctx = make_ctx();
        let old = add_user(&ctx, 1, "Nomada");

        // Salida normal (única sesión con el nick): NO es ghost.
        assert!(!ctx.is_ghost_departure(1, "Nomada"));

        // El usuario reconectó (cambio de red): sesión nueva, mismo nick.
        let _new = add_user(&ctx, 2, "Nomada");
        assert!(
            ctx.is_ghost_departure(1, "Nomada"),
            "con una sesión nueva viva del mismo nick, la vieja debe salir en silencio"
        );
        // La sesión nueva, en cambio, saldría anunciando (la vieja no cuenta
        // dos veces: sigue en el pool pero es OTRA id).
        // (nota: en la práctica la vieja se va primero; esto documenta la simetría)
        assert!(ctx.is_ghost_departure(2, "Nomada")); // la vieja aún está → silencio

        // Hijack: ghost_part_user saca a la vieja SIN anunciar; su cleanup
        // posterior la ve fuera del pool → ghost.
        ctx.ghost_part_user(&old);
        assert!(ctx.user_pool.get(1).is_none());
        assert!(ctx.is_ghost_departure(1, "Nomada"));

        // Ahora solo queda la nueva: su salida es normal.
        assert!(!ctx.is_ghost_departure(2, "Nomada"));

        // Case-insensitive (los nicks de Ares no distinguen mayúsculas).
        let _third = add_user(&ctx, 3, "NOMADA");
        assert!(ctx.is_ghost_departure(2, "Nomada"));
    }
}
