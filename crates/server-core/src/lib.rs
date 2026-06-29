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

/// Defensa en capas contra DDoS.
pub mod security;

/// Constructores de paquetes salientes.
pub mod outbound;

/// Utilidades de tiempo.
pub mod time;

/// Re-exports comunes.
pub use app::AppContext;
pub use room::Room;
pub use stats::Stats;
pub use user_pool::{AresUser, UserPool};
