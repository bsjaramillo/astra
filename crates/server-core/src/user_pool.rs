//! Pool de usuarios conectados.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::mpsc;

use crate::types::{IFont, ILevel, ILink};

/// Estructura interna de un usuario Ares (`AresUser`).
///
/// Mantiene el estado del usuario mientras está conectado. Se comparte
/// entre el bucle de red y los handlers de mensajes.
pub struct AresUser {
    /// ID de sesión (único dentro del servidor).
    pub id: u16,
    /// Nick actual.
    pub name: parking_lot::RwLock<String>,
    /// Nick original.
    pub org_name: parking_lot::RwLock<String>,
    /// IP externa.
    pub external_ip: IpAddr,
    /// IP original.
    pub original_ip: IpAddr,
    /// DNS reverso.
    pub dns: parking_lot::RwLock<String>,
    /// GUID.
    pub guid: [u8; 16],
    /// Versión del cliente.
    pub version: String,
    /// Cantidad de archivos.
    pub file_count: u16,
    /// Puerto de datos.
    pub data_port: u16,
    /// IP del supernode.
    pub node_ip: IpAddr,
    /// Puerto del supernode.
    pub node_port: u16,
    /// IP local reportada.
    pub local_ip: IpAddr,
    /// ¿Browsable?
    pub browsable: bool,
    /// Edad.
    pub age: u8,
    /// Sexo.
    pub sex: u8,
    /// País.
    pub country: u8,
    /// Región.
    pub region: String,
    /// ¿Fast ping?
    pub fast_ping: bool,
    /// ¿Ghosting?
    pub ghosting: bool,
    /// Lista de ignorados.
    pub ignore_list: parking_lot::RwLock<Vec<String>>,
    /// ¿Custom client?
    pub custom_client: bool,
    /// Tags custom.
    pub custom_client_tags: Vec<String>,
    /// ¿Voice chat public?
    pub voice_chat_public: bool,
    /// ¿Voice chat private?
    pub voice_chat_private: bool,
    /// ¿Opus voice chat public?
    pub voice_opus_chat_public: bool,
    /// ¿Opus voice chat private?
    pub voice_opus_chat_private: bool,
    /// Vroom.
    pub vroom: parking_lot::RwLock<u16>,
    /// ¿Owner?
    pub is_owner: bool,
    /// ¿Quarantined?
    pub quarantined: AtomicBool,
    /// ¿Supports HTML?
    pub supports_html: bool,
    /// ¿Web client?
    pub web_client: bool,
    /// ¿Es Ares real?
    pub ares: bool,
    /// ¿Es cbot?
    pub cbot: bool,
    /// Nivel.
    pub level: parking_lot::RwLock<ILevel>,
    /// ¿Muzzled? (mutable en runtime via /muzzle)
    pub muzzled: AtomicBool,
    /// Si es un muzzle temporal (`/mtimeout`), epoch-ms en que expira (0 = permanente).
    pub muzzle_until: std::sync::atomic::AtomicU64,
    /// ¿"Kiddied"? Si sí, su texto público se transforma (efecto sb0t).
    pub kiddied: AtomicBool,
    /// ¿"Lowered"? Si sí, su texto público se pasa a minúsculas (`/lower`).
    pub lowered: AtomicBool,
    /// ¿"Kewl text"? Si sí, su texto se transforma a leetspeak (`/kewltext`).
    pub kewl: AtomicBool,
    /// ¿"Painted"? Si sí, su texto se decora (`/paint`).
    pub painted: AtomicBool,
    /// Texto de "echo" (heckle): si está seteado, se le reenvía al usuario
    /// cada vez que habla en público (`/echo`).
    pub echo_text: parking_lot::RwLock<Option<String>>,
    /// Suscripción `/vspy`: si es admin y lo activó, recibe copia de los
    /// mensajes de OTROS vrooms.
    pub sub_vspy: AtomicBool,
    /// Suscripción `/ipsend`: recibe PM con la IP de quien entra.
    pub sub_ipsend: AtomicBool,
    /// Suscripción `/logsend`: recibe un log de eventos de la sala.
    pub sub_logsend: AtomicBool,
    /// Suscripción `/bansend`: recibe aviso cuando alguien es baneado/rechazado.
    pub sub_bansend: AtomicBool,
    /// Suscripción `/errors`: recibe PM del bot cuando un script tira error
    /// (paridad `ErrorDispatcher` de sb0t).
    pub sub_errors: AtomicBool,
    /// ¿Cloaked?
    pub cloaked: AtomicBool,
    /// ¿Captcha pendiente?
    pub needs_captcha: AtomicBool,
    /// ¿Logged in?
    pub logged_in: bool,
    /// ¿Registered?
    pub registered: bool,
    /// ¿Idle?
    pub idle: bool,
    /// ¿Connected?
    pub connected: bool,
    /// ¿Encrypted?
    pub encrypted: bool,
    /// Material de cifrado AES del cliente Ares (si negoció `crypto=250`).
    /// Se setea en el login antes de envolver en `Arc`; inmutable después.
    pub ares_crypto: Option<proto_ares::AresCrypto>,
    /// Fuente.
    pub font: IFont,
    /// Custom name.
    pub custom_name: parking_lot::RwLock<Option<String>>,
    /// Personal message (protegido para acceso concurrente).
    pub personal_message: parking_lot::Mutex<String>,
    /// Avatar. (protegido por Mutex para asignación thread-safe vía Arc)
    pub avatar: parking_lot::Mutex<Option<Vec<u8>>>,
    /// Full avatar.
    pub full_avatar: parking_lot::Mutex<Option<Vec<u8>>>,
    /// ¿Ya mandó su propio avatar (o se le asignó el default)? Paridad
    /// `AresClient.AvatarReceived` de sb0t — evita que el timer de avatar
    /// default (`Avatars.CheckAvatars`) pise un avatar ya recibido/limpiado
    /// intencionalmente.
    pub avatar_received: AtomicBool,
    /// Link.
    pub link: ILink,
    /// Join time (ms epoch).
    pub join_time: u64,
    /// Last scribble.
    pub last_scribble: u32,
    /// Inbizier flags.
    pub inbizier_web: bool,
    pub inbizier_mobile: bool,
    /// Bloqueo de PMs entrantes (`/pmblock`). Si está activo, los PMs de
    /// usuarios regulares no se entregan (Moderator+ siempre pasan).
    pub pm_blocked: AtomicBool,
    /// Cache de ASN.
    pub asn_cache: parking_lot::RwLock<Option<u32>>,
    /// Canal de envío al cliente (None si no está conectado).
    pub sender: Option<mpsc::UnboundedSender<bytes::Bytes>>,
    /// Canal de envío para clientes web (texto pre-formateado). Si está
    /// presente, el broadcast usa este canal en lugar de `sender` (que
    /// sería binario y el cliente web no lo entendería).
    pub ws_text_sender: Option<mpsc::UnboundedSender<String>>,
    /// Estado de control de flood de texto (rate-limit + duplicados).
    pub flood: crate::flood_control::FloodRecord,
}

impl AresUser {
    /// Crea un nuevo usuario con la IP externa y GUID dados.
    pub fn new(id: u16, external_ip: IpAddr, guid: [u8; 16]) -> Self {
        let now = crate::time::unix_time();
        Self {
            id,
            name: parking_lot::RwLock::new(String::new()),
            org_name: parking_lot::RwLock::new(String::new()),
            external_ip,
            original_ip: external_ip,
            dns: parking_lot::RwLock::new(String::new()),
            guid,
            version: String::new(),
            file_count: 0,
            data_port: 0,
            node_ip: external_ip,
            node_port: 0,
            local_ip: external_ip,
            browsable: false,
            age: 0,
            sex: 0,
            country: 0,
            region: String::new(),
            fast_ping: false,
            ghosting: false,
            ignore_list: parking_lot::RwLock::new(Vec::new()),
            custom_client: false,
            custom_client_tags: Vec::new(),
            voice_chat_public: false,
            voice_chat_private: false,
            voice_opus_chat_public: false,
            voice_opus_chat_private: false,
            vroom: parking_lot::RwLock::new(0),
            is_owner: false,
            quarantined: AtomicBool::new(false),
            supports_html: false,
            web_client: false,
            ares: true,
            cbot: false,
            level: parking_lot::RwLock::new(ILevel::Anonymous),
            muzzled: AtomicBool::new(false),
            muzzle_until: std::sync::atomic::AtomicU64::new(0),
            kiddied: AtomicBool::new(false),
            lowered: AtomicBool::new(false),
            kewl: AtomicBool::new(false),
            painted: AtomicBool::new(false),
            echo_text: parking_lot::RwLock::new(None),
            sub_vspy: AtomicBool::new(false),
            sub_ipsend: AtomicBool::new(false),
            sub_logsend: AtomicBool::new(false),
            sub_bansend: AtomicBool::new(false),
            sub_errors: AtomicBool::new(false),
            cloaked: AtomicBool::new(false),
            needs_captcha: AtomicBool::new(false),
            logged_in: false,
            registered: false,
            idle: false,
            connected: true,
            encrypted: false,
            ares_crypto: None,
            font: IFont::default(),
            custom_name: parking_lot::RwLock::new(None),
            personal_message: parking_lot::Mutex::new(String::new()),
            avatar: parking_lot::Mutex::new(None),
            full_avatar: parking_lot::Mutex::new(None),
            avatar_received: AtomicBool::new(false),
            link: ILink::default(),
            join_time: now,
            last_scribble: 0,
            inbizier_web: false,
            inbizier_mobile: false,
            pm_blocked: AtomicBool::new(false),
            asn_cache: parking_lot::RwLock::new(None),
            sender: None,
            ws_text_sender: None,
            flood: crate::flood_control::FloodRecord::new(),
        }
    }

    /// ¿Está muzzled ahora mismo? Los muzzles temporales (`/mtimeout`) se
    /// auto-expiran: si `muzzle_until` ya pasó, se limpia el muzzle.
    pub fn is_muzzled(&self) -> bool {
        if !self.muzzled.load(Ordering::Relaxed) {
            return false;
        }
        let until = self.muzzle_until.load(Ordering::Relaxed);
        if until != 0 && crate::time::unix_time() >= until {
            self.muzzled.store(false, Ordering::Relaxed);
            self.muzzle_until.store(0, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Envía un paquete al cliente. Retorna `true` si se encoló OK.
    pub fn send(&self, data: bytes::Bytes) -> bool {
        if let Some(tx) = &self.sender {
            tx.send(data).is_ok()
        } else {
            false
        }
    }

    /// Envía un PM al cliente. Para clientes web (WS) usa el formato de texto
    /// ib0t (`PM:len,len:...`); para clientes Ares, binario cifrando con su
    /// key si negoció cifrado. Usar en vez de `send(build_pvt(...))`.
    pub fn send_pvt(&self, from: &str, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!(
                    "PM:{},{}:{}{}",
                    ws_len(from),
                    ws_len(text),
                    from,
                    text
                ))
                .is_ok();
        }
        self.send(crate::outbound::build_pvt_c(from, text, self.ares_crypto))
    }

    /// Como [`send_pvt`](Self::send_pvt) pero para un mensaje público.
    pub fn send_public(&self, from: &str, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!(
                    "PUBLIC:{},{}:{}{}",
                    ws_len(from),
                    ws_len(text),
                    from,
                    text
                ))
                .is_ok();
        }
        self.send(crate::outbound::build_public_c(from, text, self.ares_crypto))
    }

    /// Como [`send_pvt`](Self::send_pvt) pero para un emote.
    pub fn send_emote(&self, from: &str, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!(
                    "EMOTE:{},{}:{}{}",
                    ws_len(from),
                    ws_len(text),
                    from,
                    text
                ))
                .is_ok();
        }
        self.send(crate::outbound::build_emote_c(from, text, self.ares_crypto))
    }

    /// Como [`send_pvt`](Self::send_pvt) pero para el topic de la sala.
    pub fn send_topic(&self, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!("TOPIC:{}:{}", ws_len(text), text))
                .is_ok();
        }
        self.send(crate::outbound::build_topic_c(text, self.ares_crypto))
    }

    /// Aviso de expulsión/error fatal antes de cerrar la conexión. Para
    /// clientes web va como `ERROR:` (mismo formato que usa el handshake WS
    /// para bans); para clientes Ares es el paquete `ServerError`.
    pub fn send_server_error(&self, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx.send(format!("ERROR:{}", text)).is_ok();
        }
        self.send(crate::outbound::build_server_error_c(text, self.ares_crypto))
    }

    /// Línea de sistema (respuestas de comandos, avisos del server). Para
    /// clientes web va como `NOSUCH:` (paridad `ib0tClient.Print` de sb0t:
    /// texto de servidor en la ventana principal, no un PM); para clientes
    /// Ares va como PM del bot.
    pub fn print(&self, bot_name: &str, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!("NOSUCH:{}:{}", ws_len(text), text))
                .is_ok();
        }
        self.send(crate::outbound::build_pvt_c(bot_name, text, self.ares_crypto))
    }
}

/// Largo en unidades UTF-16 (paridad `String.length` de JavaScript): el
/// protocolo de texto ib0t/web usa largos declarados por el cliente real
/// (JS), que cuenta code units UTF-16, no chars/bytes — un emoji o char
/// astral (fuera del BMP) ocupa 2, no 1. Si acá contáramos chars, un nick o
/// mensaje con esos caracteres desalinearía el parseo del lado del cliente.
fn ws_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Pool de usuarios conectados al servidor.
///
/// Mantiene un `HashMap<u16, Arc<AresUser>>` con thread-safety via `RwLock`.
pub struct UserPool {
    /// Generador de IDs de sesión.
    next_id: AtomicU16,
    /// Mapa de usuarios por ID.
    users: RwLock<HashMap<u16, Arc<AresUser>>>,
    /// Mapa de usuarios por nick (case-insensitive).
    by_name: RwLock<HashMap<String, Arc<AresUser>>>,
}

impl Default for UserPool {
    fn default() -> Self {
        Self::new()
    }
}

impl UserPool {
    /// Crea un pool vacío.
    pub fn new() -> Self {
        Self {
            next_id: AtomicU16::new(1),
            users: RwLock::new(HashMap::new()),
            by_name: RwLock::new(HashMap::new()),
        }
    }

    /// Genera un nuevo ID de sesión.
    pub fn next_id(&self) -> u16 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Registra un nuevo usuario en el pool.
    pub fn add(&self, user: Arc<AresUser>) {
        let mut users = self.users.write();
        let mut by_name = self.by_name.write();
        let name = user.name.read().clone();
        by_name.insert(name.to_lowercase(), user.clone());
        users.insert(user.id, user);
    }

    /// Elimina un usuario del pool.
    pub fn remove(&self, id: u16) {
        let mut users = self.users.write();
        let mut by_name = self.by_name.write();
        if let Some(user) = users.remove(&id) {
            let name = user.name.read().clone();
            by_name.remove(&name.to_lowercase());
        }
    }

    /// Devuelve un usuario por ID.
    pub fn get(&self, id: u16) -> Option<Arc<AresUser>> {
        self.users.read().get(&id).cloned()
    }

    /// Devuelve un usuario por nick (case-insensitive).
    pub fn get_by_name(&self, name: &str) -> Option<Arc<AresUser>> {
        self.by_name.read().get(&name.to_lowercase()).cloned()
    }

    /// Actualiza el índice por nick de un usuario ya registrado.
    pub fn rename(&self, id: u16, old_name: &str, new_name: &str) {
        let mut by_name = self.by_name.write();
        by_name.remove(&old_name.to_lowercase());
        if let Some(user) = self.users.read().get(&id).cloned() {
            by_name.insert(new_name.to_lowercase(), user);
        }
    }

    /// Cantidad de usuarios conectados.
    pub fn len(&self) -> usize {
        self.users.read().len()
    }

    /// ¿Está vacío?
    pub fn is_empty(&self) -> bool {
        self.users.read().is_empty()
    }

    /// Devuelve una lista con todos los usuarios.
    pub fn users(&self) -> Vec<Arc<AresUser>> {
        self.users.read().values().cloned().collect()
    }
}
