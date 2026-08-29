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
    /// Texto del paint aplicado al usuario (paridad `commands/Paint.cs` de
    /// sb0t: `Paint.Add(client, text)` guarda un texto que se PREPONE a sus
    /// mensajes; `None` = sin paint).
    pub paint_text: parking_lot::RwLock<Option<String>>,
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
    pub font: parking_lot::RwLock<IFont>,
    /// Custom name.
    pub custom_name: parking_lot::RwLock<Option<String>>,
    /// El cliente pidió NO ver los custom names de los demás
    /// (`MSG_CHAT_CLIENT_BLOCK_CUSTOMNAMES`, 242): recibe el público normal
    /// con el nick real (paridad sb0t `AresClient.BlockCustomNames`).
    pub block_custom_names: AtomicBool,
    /// Personal message (protegido para acceso concurrente).
    pub personal_message: parking_lot::Mutex<String>,
    /// Avatar. (protegido por Mutex para asignación thread-safe vía Arc)
    pub avatar: parking_lot::Mutex<Option<Vec<u8>>>,
    /// Avatar ORIGINAL del cliente (el que él mismo mandó en login/AVATAR).
    /// NO lo tocan los scripts (`set:avatar`): es lo que restaura
    /// `user.restoreAvatar()` (paridad sb0t `OrgAvatar`/`RestoreAvatar`).
    pub org_avatar: parking_lot::Mutex<Option<Vec<u8>>>,
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
    /// ¿Se pidió matar esta sesión? Lo setea `request_kill()` (kick/ban/
    /// hijack). El loop de lectura del socket lo consulta y corta.
    killed: AtomicBool,
    /// Señal para despertar al loop de lectura en cuanto se pide el kill, sin
    /// esperar a que el cliente mande otro paquete. `notify_one()` guarda un
    /// permiso si nadie está esperando, así que la señal nunca se pierde.
    kill_signal: tokio::sync::Notify,
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
            paint_text: parking_lot::RwLock::new(None),
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
            font: parking_lot::RwLock::new(IFont::default()),
            custom_name: parking_lot::RwLock::new(None),
            block_custom_names: AtomicBool::new(false),
            personal_message: parking_lot::Mutex::new(String::new()),
            avatar: parking_lot::Mutex::new(None),
            org_avatar: parking_lot::Mutex::new(None),
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
            killed: AtomicBool::new(false),
            kill_signal: tokio::sync::Notify::new(),
        }
    }

    /// Marca la sesión para cierre inmediato y despierta a su loop de lectura.
    ///
    /// Sacar al usuario del pool (kick/ban/hijack) NO cerraba el socket: la
    /// sesión seguía viva y el cliente podía seguir hablando aunque ya no
    /// apareciera en ninguna userlist (bug "expulsado pero sigue escribiendo").
    /// Con esto, el handler del socket corta en cuanto se pide el kick, sin
    /// depender de que el cliente mande algo o de que falle una escritura.
    pub fn request_kill(&self) {
        self.killed.store(true, Ordering::Relaxed);
        self.kill_signal.notify_one();
    }

    /// ¿Esta sesión está marcada para cierre? Los handlers dejan de procesar
    /// mensajes entrantes en cuanto es `true`.
    pub fn is_killed(&self) -> bool {
        self.killed.load(Ordering::Relaxed)
    }

    /// Espera hasta que se pida el kill de esta sesión (para usar en un
    /// `select!` junto a la lectura del socket).
    pub async fn killed_notified(&self) {
        // Si el kill llegó antes de que nadie esperara, `notify_one()` dejó un
        // permiso y esto retorna de inmediato; el chequeo del flag cubre el
        // caso de un permiso ya consumido por otro `await`.
        if self.is_killed() {
            return;
        }
        self.kill_signal.notified().await;
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

    /// Envía un zumbido (buzz/nudge) de `from`. Clientes web: ident `BUZZ`
    /// del protocolo ib0t; clientes Ares custom: el `cb0t_nudge` nativo.
    /// Retorna `false` si el cliente no sabe recibirlo (Ares no custom).
    pub fn send_buzz(&self, from: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!("BUZZ:{}:{}", ws_len(from), from))
                .is_ok();
        }
        if !self.custom_client {
            return false;
        }
        self.send(crate::outbound::build_nudge_c(from, self.ares_crypto))
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
    /// clientes web va como `NOSUCH:`; para clientes Ares como el paquete
    /// `ServerNosuch` — paridad `client.Print` de sb0t (`AresClient.Print` →
    /// `TCPOutbound.NoSuch`): el texto se muestra en la ventana de la sala de
    /// ESE cliente, NO como PM del bot y sin difundirse a la sala.
    pub fn print(&self, _bot_name: &str, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!("NOSUCH:{}:{}", ws_len(text), text))
                .is_ok();
        }
        self.send(crate::outbound::build_nosuch_c(text, self.ares_crypto))
    }

    /// Línea suelta en la sala, sin emisor (`NoSuch`). A diferencia de
    /// `print`, para clientes Ares NO va como PM del bot sino como el paquete
    /// `ServerNosuch`, que es lo que pinta el chat público sin nick delante:
    /// el transporte de los mensajes con custom name (sb0t `TCPOutbound.NoSuch`).
    pub fn send_nosuch(&self, text: &str) -> bool {
        if let Some(tx) = &self.ws_text_sender {
            return tx
                .send(format!("NOSUCH:{}:{}", ws_len(text), text))
                .is_ok();
        }
        self.send(crate::outbound::build_nosuch_c(text, self.ares_crypto))
    }

    /// Manda HTML a un cliente Ares con soporte de HTML (paridad
    /// `AresClient.SendHTML` → `TCPOutbound.HTML`). Se usa para los
    /// marcadores MOTDSTART/MOTDEND y el embed de media del MOTD. Los
    /// clientes web no lo procesan (sb0t: `ib0tClient.SupportsHTML == false`).
    pub fn send_html(&self, text: &str) -> bool {
        self.send(crate::outbound::build_html_c(text, self.ares_crypto))
    }

    /// PM real desde el bot (u otro emisor): para clientes web abre la
    /// ventana privada (`PM:`, mismo formato que `WebOutbound.build_pm`);
    /// para clientes Ares es el mismo paquete `Pmt` que usa `print`.
    pub fn send_pm(&self, from: &str, text: &str) -> bool {
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
}

/// Largo en unidades UTF-16 (paridad `String.length` de JavaScript): el
/// protocolo de texto ib0t/web usa largos declarados por el cliente real
/// (JS), que cuenta code units UTF-16, no chars/bytes — un emoji o char
/// astral (fuera del BMP) ocupa 2, no 1. Si aquí contáramos chars, un nick o
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

/// Normaliza un nick para usarlo como clave del índice `by_name`: minúsculas
/// + sin códigos de color/formato. Así un nick coloreado (`\x03John`) se
/// resuelve con su nombre "limpio" (`John`), y viceversa.
fn normalize_name(name: &str) -> String {
    crate::text_effects::strip_colors(name).to_lowercase()
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
        by_name.insert(normalize_name(&name), user.clone());
        users.insert(user.id, user);
    }

    /// Elimina un usuario del pool.
    pub fn remove(&self, id: u16) {
        let mut users = self.users.write();
        let mut by_name = self.by_name.write();
        if let Some(user) = users.remove(&id) {
            let name = user.name.read().clone();
            by_name.remove(&normalize_name(&name));
        }
    }

    /// Devuelve un usuario por ID.
    pub fn get(&self, id: u16) -> Option<Arc<AresUser>> {
        self.users.read().get(&id).cloned()
    }

    /// Devuelve un usuario por nick (case-insensitive y sin códigos de color:
    /// un nick coloreado se resuelve por su nombre "limpio").
    pub fn get_by_name(&self, name: &str) -> Option<Arc<AresUser>> {
        self.by_name.read().get(&normalize_name(name)).cloned()
    }

    /// Actualiza el índice por nick de un usuario ya registrado.
    pub fn rename(&self, id: u16, old_name: &str, new_name: &str) {
        let mut by_name = self.by_name.write();
        by_name.remove(&normalize_name(old_name));
        if let Some(user) = self.users.read().get(&id).cloned() {
            by_name.insert(normalize_name(new_name), user);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn user() -> AresUser {
        AresUser::new(1, IpAddr::V4(Ipv4Addr::LOCALHOST), [0u8; 16])
    }

    #[test]
    fn send_buzz_web_usa_el_ident_de_texto() {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let mut u = user();
        u.ws_text_sender = Some(tx);
        assert!(u.send_buzz("Pedrito"));
        assert_eq!(rx.try_recv().unwrap(), "BUZZ:7:Pedrito");
    }

    #[test]
    fn send_buzz_ares_gatea_en_custom_client() {
        let (tx, mut rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        let mut u = user();
        u.sender = Some(tx);
        // Cliente Ares que no es custom client: no sabe qué hacer con el nudge.
        assert!(!u.send_buzz("Pedrito"));
        assert!(rx.try_recv().is_err());
        // Custom client (cb0t): CustomData (200) con ident `cb0t_nudge`.
        u.custom_client = true;
        assert!(u.send_buzz("Pedrito"));
        let pkt = rx.try_recv().unwrap();
        assert_eq!(pkt[0], proto_ares::TcpMsg::CustomData as u8);
        assert!(pkt.windows(11).any(|w| w == b"cb0t_nudge\0"));
    }

    #[tokio::test]
    async fn kill_wakes_a_waiting_reader() {
        let u = Arc::new(user());
        assert!(!u.is_killed());
        let waiter = {
            let u = u.clone();
            tokio::spawn(async move { u.killed_notified().await })
        };
        // Darle tiempo a la task a quedarse esperando la señal.
        tokio::task::yield_now().await;
        u.request_kill();
        // Sin timeout: si la señal no llega, el test cuelga y falla igual.
        waiter.await.unwrap();
        assert!(u.is_killed());
    }

    #[tokio::test]
    async fn kill_before_waiting_is_not_lost() {
        let u = user();
        u.request_kill();
        // El kick puede llegar mientras el loop de lectura está procesando otra
        // cosa: la señal tiene que seguir ahí cuando vuelva a esperarla.
        u.killed_notified().await;
        u.killed_notified().await;
        assert!(u.is_killed());
    }

    #[test]
    fn get_by_name_resolves_colored_nick() {
        // Un nick coloreado (\x03 + dígitos) se resuelve por su nombre "limpio"
        // y case-insensitive.
        let pool = UserPool::new();
        let mut u = user();
        *u.name.write() = "\x03John".to_string();
        u.logged_in = true;
        pool.add(Arc::new(u));

        assert!(pool.get_by_name("John").is_some());
        assert!(pool.get_by_name("john").is_some());
        assert!(pool.get_by_name("\x0301John").is_some());
        assert!(pool.get_by_name("Johnny").is_none());

        // rename mantiene el índice limpio.
        let id = pool.users()[0].id;
        pool.rename(id, "John", "\x05Jane");
        assert!(pool.get_by_name("John").is_none());
        assert!(pool.get_by_name("Jane").is_some());
    }
}
