//! # server-core
//!
//! Núcleo del servidor Astra: event loop, `UserPool`, `Room`, `Stats`,
//! `Settings`, `BanSystem`, `Captcha`, `IdleManager`, `Avatars`.
//!
//! Esta crate orquesta el bucle principal del servidor, mantiene el
//! estado de los usuarios conectados y enruta los mensajes TCP/UDP
//! hacia los handlers correspondientes.

#![warn(missing_docs)]

/// Versión del servidor Astra.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Puerto por defecto del servidor (mismo que el sb0t original).
pub const DEFAULT_PORT: u16 = 5009;

/// Estado global de la aplicación.
pub mod app;

/// Pool de usuarios conectados.
pub mod user_pool;

/// Representación de la sala (room).
pub mod room;

/// Estadísticas globales (uptime, picos, totales).
pub mod stats;

/// Configuración persistente.
pub mod settings;

/// Sistema de bans.
pub mod bans;

/// Manager de captchas.
pub mod captcha;

/// Manager de avatares.
pub mod avatars;

/// Manager de idle.
pub mod idle;

/// Capa de persistencia (SQLite).
pub mod db;

/// Parser de login.
pub mod login;

/// Historial de usuarios (join flood detection).
pub mod user_history;

/// Manager de cuentas.
pub mod accounts;

/// Manager de vrooms (canales virtuales).
pub mod vroom;

/// Defensa en capas contra DDoS.
pub mod security;

/// Constructores de paquetes salientes.
pub mod outbound;

/// Utilidades de tiempo.
pub mod time;

/// Tipos de datos básicos del usuario (ILevel, IFont, ILink).
pub mod types;

/// Mensajes de bienvenida (greets).
pub mod greets;

/// MOTD (message of the day) mostrado al entrar.
pub mod motd;

/// Textos del sistema editables ("templates").
pub mod templates;

/// Filtro de palabras del chat público.
pub mod word_filter;

/// Enlaces rotados de la sala (URLs).
pub mod urls;

/// Range bans (prefijos de IP) y ASN bans.
pub mod ip_bans;

/// Transformaciones de texto de los efectos de moderación (kiddy, lower...).
pub mod text_effects;

/// Flags de sala (toggles caps/scribbles/audios/... on|off).
pub mod room_flags;

/// Filtros de nombre (join / file).
pub mod name_filters;

/// Resolución GeoIP / ASN via bases MMDB opcionales.
pub mod geoip;

/// Reensamblado de transferencias CUSTOM_DATA (imágenes/audio ib0t web).
pub mod custom_data;

/// Niveles de permiso configurables por comando (`/cmdlevel`).
pub mod command_levels;

/// Lista de proxies reversos confiables (panel Proxy).
pub mod proxy_trust;

/// Auto-nivel por reconocimiento de IP+GUID (`/addautologin`).
pub mod ip_autologin;

pub mod flood_control;

/// Re-exports comunes.
pub use app::{
    admin_action, AppContext, KillScriptFn, LinkEvent, LinkRequest, LinkUserSnapshot,
    ListScriptsFn, LoadScriptFn, ScriptingHooks,
};
pub use command_levels::CommandLevelManager;
pub use proxy_trust::TrustedProxyManager;
pub use ip_autologin::{IpAutologinEntry, IpAutologinManager};
pub use greets::{GreetContext, GreetManager};
pub use motd::{MotdContext, MotdManager};
pub use templates::{TemplateManager, TEMPLATE_DEFAULTS};
pub use geoip::GeoIp;
pub use ip_bans::{AsnBanManager, RangeBanManager};
pub use name_filters::NameFilterManager;
pub use room_flags::RoomFlags;
pub use room::Room;
pub use stats::Stats;
pub use types::{IFont, ILevel, ILink};
pub use urls::{UrlItem, UrlManager};
pub use user_pool::{AresUser, UserPool};
pub use vroom::VroomManager;
pub use word_filter::{FilterAction, RemoveLineResult, WordFilterManager};
