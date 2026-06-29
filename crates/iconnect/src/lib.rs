//! # iconnect
//!
//! API pública (traits) que define la interfaz entre el core del servidor
//! y los plugins/scripts. Equivalente directo del namespace `iconnect` del
//! sb0t original. Todos los traits que el código legacy expone están aquí.
//!
//! Los traits definidos son implementados por las estructuras internas
//! de `server-core` y expuestos a los plugins de forma agnóstica al lenguaje.
//!
//! ## Mapeo con el original (C# → Rust)
//!
//! | C# (sb0t) | Rust (Astra) |
//! |---|---|
//! | `IUser` | [`IUser`] |
//! | `IRoom` | [`IRoom`] |
//! | `IChannel` / `IChannels` / `IChannelItem` | [`IChannel`], [`IChannels`], [`IChannelItem`] |
//! | `IHostApp` | [`IHostApp`] |
//! | `IExtension` | [`IExtension`] |
//! | `ICommandDefault` | [`ICommandDefault`] |
//! | `ILink` | [`ILink`] |
//! | `ILeaf` | [`ILeaf`] |
//! | `IHub` | [`IHub`] |
//! | `IStats` | [`IStats`] |
//! | `IAccounts` | [`IAccounts`] |
//! | `IBan` | [`IBan`] |
//! | `IPassword` | [`IPassword`] |
//! | `IPrivateMsg` | [`IPrivateMsg`] |
//! | `IQuarantined` | [`IQuarantined`] |
//! | `ISpell` | [`ISpell`] |
//! | `IFont` | [`IFont`] |
//! | `IHashlink` / `IHashlinkRoom` | [`IHashlink`], [`IHashlinkRoom`] |
//! | `ILevel` | [`ILevel`] |
//! | `IScripting` | [`IScripting`] |
//! | `IRecord` | [`IRecord`] |
//! | `ILinkError` | [`ILinkError`] |
//! | `IPool` | [`IPool`] |
//! | `ICompression` | [`ICompression`] |
//! | `MimeType` | [`MimeType`] |
//! | `RejectedMsg` | [`RejectedMsg`] |

#![warn(missing_docs)]

use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;

// ============================================================================
// ILevel
// ============================================================================

/// Nivel de un usuario en la sala (equivalente a `ILevel` en el original).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ILevel {
    /// Sin loguear / anónimo
    Anonymous = 0,
    /// Usuario regular
    Regular = 1,
    /// Voice (con voz en salas con mute)
    Voice = 2,
    /// Moderador
    Moderator = 50,
    /// Administrador
    Admin = 80,
    /// Owner (dueño de la sala)
    Owner = 100,
    /// Sistema (para mensajes del bot)
    System = 255,
}

impl Default for ILevel {
    fn default() -> Self {
        Self::Anonymous
    }
}

// ============================================================================
// IFont
// ============================================================================

/// Fuente personalizada de un usuario (`IFont`).
#[derive(Debug, Clone, Default)]
pub struct IFont {
    /// Fuente "face" (nombre de la fuente, ej. "Arial")
    pub face: String,
    /// Color de la fuente (RGBA)
    pub color: u32,
    /// Tamaño en puntos
    pub size: u8,
    /// ¿Es bold?
    pub bold: bool,
    /// ¿Es italic?
    pub italic: bool,
    /// ¿Es underline?
    pub underline: bool,
}

// ============================================================================
// IUser
// ============================================================================

/// Representa a un usuario conectado a la sala (`IUser`).
///
/// Este es el trait más usado por los plugins y comandos. Cada método
/// tiene su contraparte directa en el original C#.
#[async_trait]
pub trait IUser: Send + Sync {
    // --- Identidad ---
    /// ID de sesión (16 bits, único dentro del servidor).
    fn id(&self) -> u16;
    /// Nick del usuario.
    fn name(&self) -> &str;
    /// Nick original (antes de cualquier rename).
    fn org_name(&self) -> &str;
    /// IP externa del usuario.
    fn external_ip(&self) -> IpAddr;
    /// IP original (puede diferir de la externa si hay proxy)
    fn original_ip(&self) -> IpAddr;
    /// DNS reverso del usuario.
    fn dns(&self) -> &str;
    /// GUID del usuario (16 bytes hasheados con MD5).
    fn guid(&self) -> &[u8; 16];

    // --- Estado de Ares ---
    /// Versión del cliente Ares.
    fn version(&self) -> &str;
    /// Cantidad de archivos compartidos.
    fn file_count(&self) -> u16;
    /// Puerto de datos del cliente (P2P).
    fn data_port(&self) -> u16;
    /// IP del supernode al que está conectado.
    fn node_ip(&self) -> IpAddr;
    /// Puerto del supernode.
    fn node_port(&self) -> u16;
    /// IP local reportada por el cliente.
    fn local_ip(&self) -> IpAddr;
    /// ¿Tiene browsing habilitado?
    fn browsable(&self) -> bool;

    // --- Perfil ---
    /// Edad declarada.
    fn age(&self) -> u8;
    /// Sexo (0=desconocido, 1=masculino, 2=femenino).
    fn sex(&self) -> u8;
    /// Código de país.
    fn country(&self) -> u8;
    /// Región declarada.
    fn region(&self) -> &str;

    // --- Protocolo ---
    /// ¿Usa fast ping (keep-alive corto)?
    fn fast_ping(&self) -> bool;
    /// ¿Está ghosting (en más de una sala)?
    fn ghosting(&self) -> bool;
    /// Lista de ignorados del usuario.
    fn ignore_list(&self) -> &[String];
    /// ¿Es un cliente custom (no Ares oficial)?
    fn custom_client(&self) -> bool;
    /// Tags del cliente custom.
    fn custom_client_tags(&self) -> &[String];
    /// ¿Soporta voz?
    fn voice_chat_public(&self) -> bool;
    fn voice_chat_private(&self) -> bool;

    // --- Sala ---
    /// ID de la sala virtual (vroom) en la que está.
    fn vroom(&self) -> u16;
    /// ¿Es owner de la sala actual?
    fn is_owner(&self) -> bool;
    /// ¿Está en cuarentena?
    fn is_quarantined(&self) -> bool;
    /// ¿Soporta HTML?
    fn supports_html(&self) -> bool;
    /// ¿Es un cliente web (ib0t)?
    fn is_web_client(&self) -> bool;
    /// ¿Cliente Ares "real" (no bot)?
    fn is_ares(&self) -> bool;
    /// ¿Es cbot (otro bot)?
    fn is_cbot(&self) -> bool;

    // --- Estado de moderación ---
    /// Nivel del usuario (ver [`ILevel`]).
    fn level(&self) -> ILevel;
    /// ¿Está muzzled (sin voz)?
    fn is_muzzled(&self) -> bool;
    /// ¿Está cloaked (invisible para otros)?
    fn is_cloaked(&self) -> bool;
    /// ¿Está captcha-bloqueado?
    fn needs_captcha(&self) -> bool;
    /// ¿Está logged in (autenticado)?
    fn is_logged_in(&self) -> bool;
    /// ¿Está registrado (cuenta existe)?
    fn is_registered(&self) -> bool;
    /// ¿Está idle?
    fn is_idle(&self) -> bool;
    /// ¿Sigue conectado?
    fn is_connected(&self) -> bool;
    /// ¿Encriptado?
    fn is_encrypted(&self) -> bool;

    // --- Personalización ---
    /// Fuente del usuario.
    fn font(&self) -> &IFont;
    /// Nick personalizado (custom name).
    fn custom_name(&self) -> Option<&str>;
    /// Mensaje personal.
    fn personal_message(&self) -> &str;
    /// Avatar (bytes JPEG/PNG).
    fn avatar(&self) -> Option<&[u8]>;
    /// Avatar completo (resolución alta).
    fn full_avatar(&self) -> Option<&[u8]>;
    /// Link credentials.
    fn link(&self) -> &ILink;
    /// Timestamp de join (ms epoch).
    fn join_time(&self) -> u64;
    /// Timestamp de último scribble enviado.
    fn last_scribble(&self) -> u32;

    // --- Acciones ---
    /// Envía bytes crudos al socket.
    async fn write_raw(&self, data: &[u8]);
    /// Imprime un mensaje al usuario.
    async fn print(&self, text: &str);
    /// Imprime un emote.
    async fn send_emote(&self, text: &str);
    /// Envía un PM "fake" (como si fuera de otro usuario).
    async fn pm(&self, sender: &str, text: &str);
    /// Envía HTML al usuario.
    async fn send_html(&self, html: &str);
    /// Envía un texto plano (no interpretado).
    async fn send_text(&self, text: &str);
    /// Envía un URL tag.
    async fn url(&self, address: &str, text: &str);
    /// Setea un topic virtual (solo visible para este usuario).
    async fn topic(&self, text: &str);
    /// Envía un scribble (imagen de pizarra).
    async fn scribble_bytes(&self, sender: &str, img: &[u8], height: i32);
    /// Envía un scribble por URL.
    async fn scribble_url(&self, sender: &str, url: &str);
    /// Envía un nudge (empujón).
    async fn nudge(&self, sender: &str);
    /// Restaura el avatar por defecto.
    async fn restore_avatar(&self);
    /// Redirige a otra sala (por hashlink).
    async fn redirect(&self, hashlink: &str);
    /// Desconecta al usuario.
    async fn disconnect(&self);
    /// Banea al usuario.
    async fn ban(&self);

    // --- Setters ---
    /// Cambia el nick.
    async fn set_name(&self, new_name: &str);
    /// Setea el nivel.
    async fn set_level(&self, level: ILevel);
    /// Setea muzzle.
    async fn set_muzzled(&self, muzzled: bool);
    /// Setea cloak.
    async fn set_cloaked(&self, cloaked: bool);
    /// Setea custom name.
    async fn set_custom_name(&self, name: Option<String>);
    /// Setea personal message.
    async fn set_personal_message(&self, msg: String);
    /// Setea avatar.
    async fn set_avatar(&self, avatar: Option<Vec<u8>>);
    /// Setea avatar full.
    async fn set_full_avatar(&self, avatar: Option<Vec<u8>>);
    /// Setea la vroom.
    async fn set_vroom(&self, vroom: u16);
    /// Setea last scribble.
    async fn set_last_scribble(&self, ts: u32);
    /// Setea ignore list.
    async fn set_ignore_list(&self, list: Vec<String>);
    /// Setea custom client tags.
    async fn set_custom_client_tags(&self, tags: Vec<String>);

    // --- Misc ---
    /// Devuelve el ASN de la IP del usuario.
    fn get_asn(&self) -> u32;
    /// ¿Es inbizier web?
    fn is_inbizier_web(&self) -> bool;
    /// ¿Es inbizier mobile?
    fn is_inbizier_mobile(&self) -> bool;
    /// Setea inbizier flags.
    async fn set_inbizier(&self, web: bool, mobile: bool);
}

// ============================================================================
// IRoom
// ============================================================================

/// Representa la sala (`IRoom`).
#[async_trait]
pub trait IRoom: Send + Sync {
    /// Nombre de la sala.
    fn name(&self) -> &str;
    /// Topic actual.
    fn topic(&self) -> &str;
    /// Cantidad de usuarios conectados.
    fn user_count(&self) -> usize;
    /// Cantidad de vrooms activas.
    fn vroom_count(&self) -> usize;
    /// ¿La sala está abierta?
    fn is_open(&self) -> bool;

    /// Broadcast público.
    async fn public(&self, sender: &str, text: &str);
    /// Broadcast emote.
    async fn emote(&self, sender: &str, text: &str);
    /// Broadcast PM a todos.
    async fn pm_all(&self, sender: &str, text: &str);
    /// Cierra la sala.
    async fn close(&self);
}

// ============================================================================
// IChannel
// ============================================================================

/// Información de un item de canal en la lista UDP de búsqueda (`IChannelItem`).
#[derive(Debug, Clone)]
pub struct IChannelItem {
    /// Nombre de la sala.
    pub name: String,
    /// Topic.
    pub topic: String,
    /// Cantidad de usuarios.
    pub users: u32,
    /// IP del servidor.
    pub ip: Ipv4Addr,
    /// Puerto del servidor.
    pub port: u16,
    /// Idioma (código Ares).
    pub language: u8,
    /// ¿Es AES?
    pub aes: bool,
}

/// Lista de canales UDP (`IChannels`).
#[async_trait]
pub trait IChannels: Send + Sync {
    /// Itera sobre todos los items.
    fn for_each(&self, f: &mut dyn FnMut(&IChannelItem));
}

/// Un canal individual (`IChannel`).
#[async_trait]
pub trait IChannel: Send + Sync {
    /// Nombre del canal.
    fn name(&self) -> &str;
    /// Topic.
    fn topic(&self) -> &str;
    /// Cantidad de usuarios.
    fn user_count(&self) -> u32;
}

// ============================================================================
// IHostApp
// ============================================================================

/// Aplicación host (entry point del binario, `IHostApp`).
#[async_trait]
pub trait IHostApp: Send + Sync {
    /// Versión de Astra.
    fn version(&self) -> &str;
    /// Puerto TCP en escucha.
    fn port(&self) -> u16;
    /// Inicia el servidor.
    async fn start(&self) -> anyhow::Result<()>;
    /// Detiene el servidor.
    async fn stop(&self) -> anyhow::Result<()>;
}

// ============================================================================
// IExtension
// ============================================================================

/// Extensión de un plugin (`IExtension`).
#[async_trait]
pub trait IExtension: Send + Sync {
    /// Nombre de la extensión.
    fn name(&self) -> &str;
    /// Versión de la extensión.
    fn version(&self) -> &str;
    /// ¿Está habilitada?
    fn is_enabled(&self) -> bool;
    /// Inicializa la extensión.
    async fn init(&self) -> anyhow::Result<()>;
    /// Tick periódico (1 segundo aprox).
    async fn tick(&self);
}

// ============================================================================
// ICommandDefault
// ============================================================================

/// Comando slash por defecto (`ICommandDefault`).
#[derive(Debug, Clone)]
pub struct ICommandDefault {
    /// Nombre del comando (sin la barra).
    pub name: String,
    /// Nivel mínimo para usarlo.
    pub level: ILevel,
    /// Descripción.
    pub description: String,
    /// Sintaxis de uso.
    pub syntax: String,
    /// ¿Está habilitado?
    pub enabled: bool,
}

// ============================================================================
// ILeaf / IHub
// ============================================================================

/// Hoja de un link (servidor subordinado, `ILeaf`).
#[async_trait]
pub trait ILeaf: Send + Sync {
    /// Identificador del leaf.
    fn ident(&self) -> &str;
    /// ¿Está conectado?
    fn is_connected(&self) -> bool;
    /// Desconecta.
    async fn disconnect(&self);
}

/// Hub central de links (`IHub`).
#[async_trait]
pub trait IHub: Send + Sync {
    /// Lista de leaves conectados.
    fn leaves(&self) -> Vec<String>;
    /// Inicia el hub.
    async fn start(&self) -> anyhow::Result<()>;
    /// Detiene el hub.
    async fn stop(&self) -> anyhow::Result<()>;
}

// ============================================================================
// ILink / ILinkError
// ============================================================================

/// Link entre servidores (`ILink`).
#[derive(Debug, Clone, Default)]
pub struct ILink {
    /// Identificador.
    pub ident: String,
    /// Hash de autenticación.
    pub hash: String,
    /// ¿Es outbound?
    pub outbound: bool,
    /// ¿Es trusted?
    pub trusted: bool,
}

/// Error de link (`ILinkError`).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ILinkError {
    /// Timeout
    #[error("link timeout")]
    Timeout,
    /// Auth inválida
    #[error("link auth invalid")]
    AuthInvalid,
    /// Ya conectado
    #[error("link already connected")]
    AlreadyConnected,
    /// Desconocido
    #[error("link error: {0}")]
    Other(String),
}

// ============================================================================
// IStats
// ============================================================================

/// Estadísticas globales (`IStats`).
#[async_trait]
pub trait IStats: Send + Sync {
    /// Pico de usuarios simultáneos.
    fn peak_users(&self) -> u32;
    /// Total de usuarios que han entrado.
    fn total_users(&self) -> u64;
    /// Uptime en segundos.
    fn uptime_secs(&self) -> u64;
    /// Bytes recibidos.
    fn bytes_in(&self) -> u64;
    /// Bytes enviados.
    fn bytes_out(&self) -> u64;
}

// ============================================================================
// IAccounts
// ============================================================================

/// Cuentas registradas (`IAccounts`).
#[async_trait]
pub trait IAccounts: Send + Sync {
    /// Verifica si una cuenta existe.
    async fn exists(&self, name: &str) -> bool;
    /// Verifica credenciales.
    async fn verify(&self, name: &str, password: &str) -> bool;
    /// Crea una cuenta.
    async fn create(&self, name: &str, password: &str) -> anyhow::Result<()>;
    /// Cambia la contraseña.
    async fn change_password(&self, name: &str, new_password: &str) -> anyhow::Result<()>;
    /// Elimina una cuenta.
    async fn delete(&self, name: &str) -> anyhow::Result<()>;
    /// Lista todas las cuentas.
    async fn list(&self) -> Vec<String>;
}

// ============================================================================
// IBan
// ============================================================================

/// Ban (`IBan`).
#[derive(Debug, Clone)]
pub struct IBan {
    /// Hash del GUID/IP baneado.
    pub target: String,
    /// Razón.
    pub reason: String,
    /// Timestamp.
    pub timestamp: u64,
    /// Duración en segundos (0 = permanente).
    pub duration_secs: u64,
    /// ¿Es ban de IP?
    pub is_ip: bool,
}

// ============================================================================
// IPassword
// ============================================================================

/// Password (`IPassword`).
#[async_trait]
pub trait IPassword: Send + Sync {
    /// Hashea una contraseña.
    fn hash(&self, password: &str) -> String;
    /// Verifica un hash.
    fn verify(&self, password: &str, hash: &str) -> bool;
}

// ============================================================================
// IPrivateMsg
// ============================================================================

/// PM (private message, `IPrivateMsg`).
#[derive(Debug, Clone)]
pub struct IPrivateMsg {
    /// Sender.
    pub from: String,
    /// Recipient.
    pub to: String,
    /// Contenido.
    pub text: String,
    /// Timestamp.
    pub timestamp: u64,
}

// ============================================================================
// IQuarantined
// ============================================================================

/// Estado de cuarentena (`IQuarantined`).
#[async_trait]
pub trait IQuarantined: Send + Sync {
    /// ¿Está en cuarentena?
    fn is_quarantined(&self) -> bool;
    /// Pone en cuarentena.
    async fn quarantine(&self);
    /// Saca de cuarentena.
    async fn unquarantine(&self);
}

// ============================================================================
// ISpell
// ============================================================================

/// Spell check (`ISpell`).
#[async_trait]
pub trait ISpell: Send + Sync {
    /// Verifica una palabra.
    fn check(&self, word: &str) -> bool;
    /// Sugiere correcciones.
    fn suggest(&self, word: &str) -> Vec<String>;
}

// ============================================================================
// IHashlink / IHashlinkRoom
// ============================================================================

/// Hashlink de una sala (`IHashlink`).
#[derive(Debug, Clone)]
pub struct IHashlink {
    /// Hash (formato Ares hashlink)
    pub hash: String,
    /// Nombre de la sala destino.
    pub name: String,
    /// IP del servidor destino.
    pub ip: Ipv4Addr,
    /// Puerto del servidor destino.
    pub port: u16,
}

/// Hashlink de una room (`IHashlinkRoom`).
#[async_trait]
pub trait IHashlinkRoom: Send + Sync {
    /// Nombre de la sala.
    fn name(&self) -> &str;
    /// Topic.
    fn topic(&self) -> &str;
    /// Hashlink.
    fn hashlink(&self) -> &IHashlink;
}

// ============================================================================
// IScripting
// ============================================================================

/// Motor de scripting (`IScripting`).
#[async_trait]
pub trait IScripting: Send + Sync {
    /// Ejecuta un script.
    async fn run(&self, code: &str) -> anyhow::Result<()>;
    /// Detiene todos los scripts.
    async fn stop_all(&self);
    /// Lista scripts cargados.
    async fn list(&self) -> Vec<String>;
}

// ============================================================================
// IRecord
// ============================================================================

/// Record (historial) de un usuario (`IRecord`).
#[derive(Debug, Clone)]
pub struct IRecord {
    /// Nick.
    pub name: String,
    /// GUID.
    pub guid: String,
    /// IP.
    pub ip: IpAddr,
    /// Primer join (ms epoch).
    pub first_seen: u64,
    /// Último join.
    pub last_seen: u64,
    /// Total de joins.
    pub total_joins: u32,
}

// ============================================================================
// IPool
// ============================================================================

/// Pool genérico de objetos (`IPool<T>`).
#[async_trait]
pub trait IPool: Send + Sync {
    /// Tipo de item.
    type Item;
    /// Cantidad de items.
    fn len(&self) -> usize;
    /// ¿Está vacío?
    fn is_empty(&self) -> bool;
    /// Itera sobre los items.
    fn for_each(&self, f: &mut dyn FnMut(&Self::Item));
}

// ============================================================================
// ICompression
// ============================================================================

/// Compresión (`ICompression`).
pub trait ICompression: Send + Sync {
    /// Comprime datos.
    fn compress(&self, data: &[u8]) -> anyhow::Result<Vec<u8>>;
    /// Descomprime datos.
    fn decompress(&self, data: &[u8]) -> anyhow::Result<Vec<u8>>;
}

// ============================================================================
// MimeType
// ============================================================================

/// MIME type (`MimeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeType {
    /// image/png
    Png,
    /// image/jpeg
    Jpeg,
    /// image/gif
    Gif,
    /// image/bmp
    Bmp,
    /// text/plain
    Text,
    /// application/octet-stream
    OctetStream,
    /// Otro
    Other,
}

impl MimeType {
    /// Devuelve el string MIME.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Text => "text/plain",
            Self::OctetStream => "application/octet-stream",
            Self::Other => "application/octet-stream",
        }
    }
}

// ============================================================================
// RejectedMsg
// ============================================================================

/// Mensaje de rechazo (`RejectedMsg`).
#[derive(Debug, Clone)]
pub struct RejectedMsg {
    /// Razón del rechazo.
    pub reason: String,
    /// ¿Es baneable?
    pub bannable: bool,
}
