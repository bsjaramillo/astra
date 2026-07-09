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
    /// ¿Cloaked?
    pub cloaked: bool,
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
    /// Link.
    pub link: ILink,
    /// Join time (ms epoch).
    pub join_time: u64,
    /// Last scribble.
    pub last_scribble: u32,
    /// Inbizier flags.
    pub inbizier_web: bool,
    pub inbizier_mobile: bool,
    /// Cache de ASN.
    pub asn_cache: parking_lot::RwLock<Option<u32>>,
    /// Canal de envío al cliente (None si no está conectado).
    pub sender: Option<mpsc::UnboundedSender<bytes::Bytes>>,
    /// Canal de envío para clientes web (texto pre-formateado). Si está
    /// presente, el broadcast usa este canal en lugar de `sender` (que
    /// sería binario y el cliente web no lo entendería).
    pub ws_text_sender: Option<mpsc::UnboundedSender<String>>,
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
            cloaked: false,
            needs_captcha: AtomicBool::new(false),
            logged_in: false,
            registered: false,
            idle: false,
            connected: true,
            encrypted: false,
            font: IFont::default(),
            custom_name: parking_lot::RwLock::new(None),
            personal_message: parking_lot::Mutex::new(String::new()),
            avatar: parking_lot::Mutex::new(None),
            full_avatar: parking_lot::Mutex::new(None),
            link: ILink::default(),
            join_time: now,
            last_scribble: 0,
            inbizier_web: false,
            inbizier_mobile: false,
            asn_cache: parking_lot::RwLock::new(None),
            sender: None,
            ws_text_sender: None,
        }
    }

    /// Envía un paquete al cliente. Retorna `true` si se encoló OK.
    pub fn send(&self, data: bytes::Bytes) -> bool {
        if let Some(tx) = &self.sender {
            tx.send(data).is_ok()
        } else {
            false
        }
    }
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
