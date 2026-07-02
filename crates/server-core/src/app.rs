//! Contexto global de la aplicación: configuración, estado, DB y ciclo de vida.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use super::accounts::AccountManager;
use super::bans::BanSystem;
use super::db::Database;
use super::security::SecurityManager;
use super::settings::Settings;
use super::stats::Stats;
use super::user_history::UserHistory;
use super::user_pool::UserPool;

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

impl LinkUserSnapshot {
    /// Construye un snapshot a partir de un usuario local conectado.
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
            muzzled: user.muzzled,
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
    /// Instante de arranque (para calcular uptime).
    pub start_time: Instant,
    /// Topic actual de la sala (mutable en runtime).
    pub room_topic: RwLock<String>,
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
            start_time: Instant::now(),
            room_topic: RwLock::new(initial_room_topic),
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

    /// Publica un evento para replicación Link.
    pub fn publish_link_event(&self, event: LinkEvent) {
        let _ = self.link_events.send(event);
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
