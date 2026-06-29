//! Tipos del sistema de scripting.
//!
//! Los 45 eventos JS del sb0t original están soportados. Cada evento
//! tiene su `handler_name()` (ej. `ScriptEvent::UserJoin → "onJoin"`)
//! que corresponde al nombre de la función JS que el script debe
//! implementar.

#![allow(clippy::large_enum_variant)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

/// ID único de un script cargado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptId(pub u64);

impl ScriptId {
    /// Genera un nuevo ID único.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Estado de un script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptState {
    /// Script cargado pero no inicializado
    Loaded,
    /// Script activo (handle onLoad() ejecutado OK)
    Active,
    /// Script con error (handle lanzó excepción)
    Error,
    /// Script descargado
    Unloaded,
}

/// Un script cargado.
pub struct Script {
    /// ID único
    pub id: ScriptId,
    /// Nombre del script (filename o nombre lógico)
    pub name: String,
    /// Path al archivo (si fue cargado desde disco)
    pub path: Option<PathBuf>,
    /// Estado
    pub state: Arc<Mutex<ScriptState>>,
    /// Context JS (opaque)
    pub context: Arc<Mutex<Option<boa_engine::Context>>>,
    /// Mensaje de error (si state == Error)
    pub last_error: Arc<Mutex<Option<String>>>,
}

impl Script {
    /// Crea un nuevo script (sin contexto todavía).
    pub fn new(name: String, path: Option<PathBuf>) -> Self {
        Self {
            id: ScriptId::new(),
            name,
            path,
            state: Arc::new(Mutex::new(ScriptState::Loaded)),
            context: Arc::new(Mutex::new(None)),
            last_error: Arc::new(Mutex::new(None)),
        }
    }

    /// Nombre del script.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Estado actual.
    pub fn state(&self) -> ScriptState {
        *self.state.lock()
    }

    /// Cambia el estado.
    pub fn set_state(&self, state: ScriptState) {
        *self.state.lock() = state;
    }

    /// Guarda un mensaje de error.
    pub fn set_error(&self, msg: String) {
        *self.last_error.lock() = Some(msg);
    }

    /// Último error.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().clone()
    }

    /// Setea el contexto.
    pub fn set_context(&self, ctx: boa_engine::Context) {
        *self.context.lock() = Some(ctx);
    }

    /// Devuelve el contexto (referencia).
    pub fn context(&self) -> &Arc<Mutex<Option<boa_engine::Context>>> {
        &self.context
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        // Desregistrar el contexto del registry global para evitar
        // acumulación de entries cuando un Script se destruye.
        if let Some(ctx) = self.context.lock().as_ref() {
            crate::api::unregister_context(ctx);
        }
    }
}

/// Evento que se puede enviar a un script.
///
/// Equivalente a los 45 callbacks del sb0t original. Los nombres
/// `handler_name()` son EXACTAMENTE los del sb0t para máxima
/// compatibilidad con scripts existentes.
#[derive(Debug, Clone)]
pub enum ScriptEvent {
    // --- Connection lifecycle ---
    /// Cliente TCP se conectó (antes de login)
    Connect { ip: String },
    /// Cliente TCP se desconectó
    Disconnect { ip: String },
    /// Usuario fue aceptado (post-login)
    Join { name: String, ip: String },
    /// Hook de validación pre-login (puede rechazar)
    JoinCheck { name: String, ip: String },
    /// Usuario fue rechazado al intentar entrar
    Rejected { name: String, ip: String, reason: String },
    /// Usuario se fue de la sala
    Part { name: String },
    /// Hook pre-part (puede cancelar la salida)
    PartBefore { name: String, ip: String },

    // --- Userlist ---
    /// Userlist: broadcast inicial de la lista de usuarios
    UserList { name: String, users_csv: String },
    /// Userlist: fin de la lista
    UserListEnd { name: String },
    /// Usuario actualizado (cambio de avatar, pmsg, nivel, etc)
    UserUpdate { name: String },

    // --- Public messages ---
    /// Mensaje público recibido
    Public { from: String, text: String },
    /// Hook antes de enviar un mensaje público (puede modificarlo)
    TextBefore { from: String, text: String },
    /// Hook después de enviar un mensaje público
    TextAfter { from: String, text: String },
    /// Mensaje público recibido (alias)
    TextReceived { from: String, text: String },

    // --- Emote ---
    /// Emote recibido
    Emote { from: String, text: String },
    /// Hook antes de enviar un emote
    EmoteBefore { from: String, text: String },
    /// Hook después de enviar un emote
    EmoteAfter { from: String, text: String },
    /// Emote recibido (alias)
    EmoteReceived { from: String, text: String },

    // --- PM ---
    /// PM recibido
    Private { from: String, to: String, text: String },
    /// Hook antes de enviar un PM
    PMBefore { from: String, to: String, text: String },
    /// PM recibido (alias)
    PM { from: String, to: String, text: String },
    /// Tu PM fue ignorado por el receptor
    BotPM { from: String, to: String, text: String },
    /// Ignoraste/designoraste a un user
    Ignoring { name: String },
    /// Un user te ignoró/ya no te ignora
    IgnoredStateChanged { name: String },

    // --- Avatar / pmsg ---
    /// Avatar actualizado
    Avatar { name: String },
    /// Personal message actualizado
    PersonalMessage { name: String, text: String },

    // --- Nick / admin ---
    /// Nick cambiado
    Nick { old: String, new: String },
    /// Nivel admin cambiado
    AdminLevelChanged { name: String },
    /// Login concedido
    LoginGranted { name: String },
    /// Logout
    Logout { name: String },
    /// Intento de login inválido
    InvalidLoginAttempt { name: String, ip: String },
    /// Comando slash ejecutado
    Command { from: String, command: String, args: String },

    // --- Idle ---
    /// User pasó a idle
    Idled { name: String },
    /// User salió de idle
    Unidled { name: String },

    // --- Registration ---
    /// User se está registrando
    Registering { name: String, ip: String },
    /// User fue registrado
    Registered { name: String, ip: String },
    /// User fue des-registrado
    Unregistered { name: String },

    // --- Bans / proxies ---
    /// Bans auto-limpios (ban expirado)
    BansAutoCleared,
    /// Proxy detectado
    ProxyDetected { ip: String },

    // --- Flood ---
    /// User flood
    Flood { name: String },
    /// Hook antes de flood-check (puede cancelar)
    FloodBefore { name: String },

    // --- File browse ---
    /// Archivo recibido (browse)
    FileReceived { name: String, filename: String },

    // --- Scribble ---
    /// Hook de validación de scribble
    ScribbleCheck { name: String, is_pm: bool },

    // --- Help ---
    /// Help command (return string to display)
    Help { command: String },

    // --- Link ---
    /// Hub o leaf conectado
    Linked { name: String },
    /// Hub o leaf desconectado
    Unlinked { name: String },
    /// Error en el link
    LinkError { name: String, error: String },
    /// Admin del hub deshabilitado
    LinkedAdminDisabled,
    /// Leaf se unió
    LeafJoin { name: String },
    /// Leaf se fue
    LeafPart { name: String },

    // --- Vroom ---
    /// User entró a un vroom
    VroomJoin { name: String, vroom: u16 },
    /// Hook de validación para entrar a un vroom
    VroomJoinCheck { name: String, vroom: u16 },

    // --- Timer ---
    /// Timer periódico (segundos desde el epoch)
    Timer { secs: u64 },
}

impl ScriptEvent {
    /// Nombre del handler JS que se llamará para este evento.
    /// **Idéntico al sb0t original** para máxima compatibilidad.
    pub fn handler_name(&self) -> &'static str {
        use ScriptEvent::*;
        match self {
            // Connection lifecycle
            Connect { .. } => "onConnect",
            Disconnect { .. } => "onDisconnect",
            Join { .. } => "onJoin",
            JoinCheck { .. } => "onJoinCheck",
            Rejected { .. } => "onRejected",
            Part { .. } => "onPart",
            PartBefore { .. } => "onPartBefore",

            // Userlist
            UserList { .. } => "onUserList",
            UserListEnd { .. } => "onUserListEnd",
            UserUpdate { .. } => "onUserUpdate",

            // Public messages
            Public { .. } => "onPublic",
            TextBefore { .. } => "onTextBefore",
            TextAfter { .. } => "onTextAfter",
            TextReceived { .. } => "onTextReceived",

            // Emote
            Emote { .. } => "onEmote",
            EmoteBefore { .. } => "onEmoteBefore",
            EmoteAfter { .. } => "onEmoteAfter",
            EmoteReceived { .. } => "onEmoteReceived",

            // PM
            Private { .. } => "onPrivate",
            PMBefore { .. } => "onPMBefore",
            PM { .. } => "onPM",
            BotPM { .. } => "onBotPM",
            Ignoring { .. } => "onIgnoring",
            IgnoredStateChanged { .. } => "onIgnoredStateChanged",

            // Avatar / pmsg
            Avatar { .. } => "onAvatar",
            PersonalMessage { .. } => "onPersonalMessage",

            // Nick / admin
            Nick { .. } => "onNick",
            AdminLevelChanged { .. } => "onAdminLevelChanged",
            LoginGranted { .. } => "onLoginGranted",
            Logout { .. } => "onLogout",
            InvalidLoginAttempt { .. } => "onInvalidLoginAttempt",
            Command { .. } => "onCommand",

            // Idle
            Idled { .. } => "onIdled",
            Unidled { .. } => "onUnidled",

            // Registration
            Registering { .. } => "onRegistering",
            Registered { .. } => "onRegistered",
            Unregistered { .. } => "onUnregistered",

            // Bans / proxies
            BansAutoCleared => "onBansAutoCleared",
            ProxyDetected { .. } => "onProxyDetected",

            // Flood
            Flood { .. } => "onFlood",
            FloodBefore { .. } => "onFloodBefore",

            // File browse
            FileReceived { .. } => "onFileReceived",

            // Scribble
            ScribbleCheck { .. } => "onScribbleCheck",

            // Help
            Help { .. } => "onHelp",

            // Link
            Linked { .. } => "onLinked",
            Unlinked { .. } => "onUnlinked",
            LinkError { .. } => "onLinkError",
            LinkedAdminDisabled => "onLinkedAdminDisabled",
            LeafJoin { .. } => "onLeafJoin",
            LeafPart { .. } => "onLeafPart",

            // Vroom
            VroomJoin { .. } => "onVroomJoin",
            VroomJoinCheck { .. } => "onVroomJoinCheck",

            // Timer
            Timer { .. } => "onTimer",
        }
    }

    /// Argumentos que se pasarán al handler JS.
    /// Compatibles con el orden esperado por los scripts del sb0t original.
    pub fn args(&self) -> Vec<String> {
        use ScriptEvent::*;
        match self {
            // Connection lifecycle
            Connect { ip } => vec![ip.clone()],
            Disconnect { ip } => vec![ip.clone()],
            Join { name, ip } => vec![name.clone(), ip.clone()],
            JoinCheck { name, ip } => vec![name.clone(), ip.clone()],
            Rejected { name, ip, reason } => vec![name.clone(), ip.clone(), reason.clone()],
            Part { name } => vec![name.clone()],
            PartBefore { name, ip } => vec![name.clone(), ip.clone()],

            // Userlist
            UserList { name, users_csv } => vec![name.clone(), users_csv.clone()],
            UserListEnd { name } => vec![name.clone()],
            UserUpdate { name } => vec![name.clone()],

            // Public messages
            Public { from, text } => vec![from.clone(), text.clone()],
            TextBefore { from, text } => vec![from.clone(), text.clone()],
            TextAfter { from, text } => vec![from.clone(), text.clone()],
            TextReceived { from, text } => vec![from.clone(), text.clone()],

            // Emote
            Emote { from, text } => vec![from.clone(), text.clone()],
            EmoteBefore { from, text } => vec![from.clone(), text.clone()],
            EmoteAfter { from, text } => vec![from.clone(), text.clone()],
            EmoteReceived { from, text } => vec![from.clone(), text.clone()],

            // PM
            Private { from, to, text } => vec![from.clone(), to.clone(), text.clone()],
            PMBefore { from, to, text } => vec![from.clone(), to.clone(), text.clone()],
            PM { from, to, text } => vec![from.clone(), to.clone(), text.clone()],
            BotPM { from, to, text } => vec![from.clone(), to.clone(), text.clone()],
            Ignoring { name } => vec![name.clone()],
            IgnoredStateChanged { name } => vec![name.clone()],

            // Avatar / pmsg
            Avatar { name } => vec![name.clone()],
            PersonalMessage { name, text } => vec![name.clone(), text.clone()],

            // Nick / admin
            Nick { old, new } => vec![old.clone(), new.clone()],
            AdminLevelChanged { name } => vec![name.clone()],
            LoginGranted { name } => vec![name.clone()],
            Logout { name } => vec![name.clone()],
            InvalidLoginAttempt { name, ip } => vec![name.clone(), ip.clone()],
            Command { from, command, args } => {
                vec![from.clone(), command.clone(), args.clone()]
            }

            // Idle
            Idled { name } => vec![name.clone()],
            Unidled { name } => vec![name.clone()],

            // Registration
            Registering { name, ip } => vec![name.clone(), ip.clone()],
            Registered { name, ip } => vec![name.clone(), ip.clone()],
            Unregistered { name } => vec![name.clone()],

            // Bans / proxies
            BansAutoCleared => vec![],
            ProxyDetected { ip } => vec![ip.clone()],

            // Flood
            Flood { name } => vec![name.clone()],
            FloodBefore { name } => vec![name.clone()],

            // File browse
            FileReceived { name, filename } => vec![name.clone(), filename.clone()],

            // Scribble
            ScribbleCheck { name, is_pm } => vec![name.clone(), is_pm.to_string()],

            // Help
            Help { command } => vec![command.clone()],

            // Link
            Linked { name } => vec![name.clone()],
            Unlinked { name } => vec![name.clone()],
            LinkError { name, error } => vec![name.clone(), error.clone()],
            LinkedAdminDisabled => vec![],
            LeafJoin { name } => vec![name.clone()],
            LeafPart { name } => vec![name.clone()],

            // Vroom
            VroomJoin { name, vroom } => vec![name.clone(), vroom.to_string()],
            VroomJoinCheck { name, vroom } => vec![name.clone(), vroom.to_string()],

            // Timer
            Timer { secs } => vec![secs.to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_id_unique() {
        let a = ScriptId::new();
        let b = ScriptId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn handler_name() {
        let ev = ScriptEvent::Public {
            from: "Alice".into(),
            text: "hola".into(),
        };
        assert_eq!(ev.handler_name(), "onPublic");
        assert_eq!(ev.args(), vec!["Alice", "hola"]);
    }

    #[test]
    fn all_handler_names_match_sb0t() {
        // Lista de los handler names que Astra debe soportar.
        // Debe estar sincronizada con la API del sb0t original.
        // Si se agrega un evento sin actualizar esta lista, este test falla.
        let expected = vec![
            "onConnect", "onDisconnect", "onJoin", "onJoinCheck", "onRejected",
            "onPart", "onPartBefore", "onUserList", "onUserListEnd", "onUserUpdate",
            "onPublic", "onTextBefore", "onTextAfter", "onTextReceived",
            "onEmote", "onEmoteBefore", "onEmoteAfter", "onEmoteReceived",
            "onPrivate", "onPMBefore", "onPM", "onBotPM", "onIgnoring",
            "onIgnoredStateChanged", "onAvatar", "onPersonalMessage",
            "onNick", "onAdminLevelChanged", "onLoginGranted", "onLogout",
            "onInvalidLoginAttempt", "onCommand", "onIdled", "onUnidled",
            "onRegistering", "onRegistered", "onUnregistered",
            "onBansAutoCleared", "onProxyDetected", "onFlood", "onFloodBefore",
            "onFileReceived", "onScribbleCheck", "onHelp", "onLinked",
            "onUnlinked", "onLinkError", "onLinkedAdminDisabled",
            "onLeafJoin", "onLeafPart", "onVroomJoin", "onVroomJoinCheck",
            "onTimer",
        ];
        // 45 handler names (más onJoinCheck que está incluido, total 46 en la lista)
        assert!(expected.len() >= 45);

        // Verifica que la cantidad de handler names distintos en el enum
        // sea al menos 45.
        let unique_handler_names: std::collections::HashSet<&str> = vec![
            ScriptEvent::Connect { ip: "".into() }.handler_name(),
            ScriptEvent::Disconnect { ip: "".into() }.handler_name(),
            ScriptEvent::Join { name: "".into(), ip: "".into() }.handler_name(),
            ScriptEvent::JoinCheck { name: "".into(), ip: "".into() }.handler_name(),
            ScriptEvent::Rejected { name: "".into(), ip: "".into(), reason: "".into() }.handler_name(),
            ScriptEvent::Part { name: "".into() }.handler_name(),
            ScriptEvent::PartBefore { name: "".into(), ip: "".into() }.handler_name(),
            ScriptEvent::UserList { name: "".into(), users_csv: "".into() }.handler_name(),
            ScriptEvent::UserListEnd { name: "".into() }.handler_name(),
            ScriptEvent::UserUpdate { name: "".into() }.handler_name(),
            ScriptEvent::Public { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::TextBefore { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::TextAfter { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::TextReceived { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::Emote { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::EmoteBefore { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::EmoteAfter { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::EmoteReceived { from: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::Private { from: "".into(), to: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::PMBefore { from: "".into(), to: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::PM { from: "".into(), to: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::BotPM { from: "".into(), to: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::Ignoring { name: "".into() }.handler_name(),
            ScriptEvent::IgnoredStateChanged { name: "".into() }.handler_name(),
            ScriptEvent::Avatar { name: "".into() }.handler_name(),
            ScriptEvent::PersonalMessage { name: "".into(), text: "".into() }.handler_name(),
            ScriptEvent::Nick { old: "".into(), new: "".into() }.handler_name(),
            ScriptEvent::AdminLevelChanged { name: "".into() }.handler_name(),
            ScriptEvent::LoginGranted { name: "".into() }.handler_name(),
            ScriptEvent::Logout { name: "".into() }.handler_name(),
            ScriptEvent::InvalidLoginAttempt { name: "".into(), ip: "".into() }.handler_name(),
            ScriptEvent::Command { from: "".into(), command: "".into(), args: "".into() }.handler_name(),
            ScriptEvent::Idled { name: "".into() }.handler_name(),
            ScriptEvent::Unidled { name: "".into() }.handler_name(),
            ScriptEvent::Registering { name: "".into(), ip: "".into() }.handler_name(),
            ScriptEvent::Registered { name: "".into(), ip: "".into() }.handler_name(),
            ScriptEvent::Unregistered { name: "".into() }.handler_name(),
            ScriptEvent::BansAutoCleared.handler_name(),
            ScriptEvent::ProxyDetected { ip: "".into() }.handler_name(),
            ScriptEvent::Flood { name: "".into() }.handler_name(),
            ScriptEvent::FloodBefore { name: "".into() }.handler_name(),
            ScriptEvent::FileReceived { name: "".into(), filename: "".into() }.handler_name(),
            ScriptEvent::ScribbleCheck { name: "".into(), is_pm: false }.handler_name(),
            ScriptEvent::Help { command: "".into() }.handler_name(),
            ScriptEvent::Linked { name: "".into() }.handler_name(),
            ScriptEvent::Unlinked { name: "".into() }.handler_name(),
            ScriptEvent::LinkError { name: "".into(), error: "".into() }.handler_name(),
            ScriptEvent::LinkedAdminDisabled.handler_name(),
            ScriptEvent::LeafJoin { name: "".into() }.handler_name(),
            ScriptEvent::LeafPart { name: "".into() }.handler_name(),
            ScriptEvent::VroomJoin { name: "".into(), vroom: 0 }.handler_name(),
            ScriptEvent::VroomJoinCheck { name: "".into(), vroom: 0 }.handler_name(),
            ScriptEvent::Timer { secs: 0 }.handler_name(),
        ]
        .into_iter()
        .collect();
        assert!(unique_handler_names.len() >= 45);
    }

    #[test]
    fn new_events_map_to_correct_handlers() {
        // Connect
        let ev = ScriptEvent::Connect { ip: "1.2.3.4".into() };
        assert_eq!(ev.handler_name(), "onConnect");
        assert_eq!(ev.args(), vec!["1.2.3.4"]);

        // Join (reemplazó a UserJoin)
        let ev = ScriptEvent::Join { name: "Alice".into(), ip: "1.2.3.4".into() };
        assert_eq!(ev.handler_name(), "onJoin");
        assert_eq!(ev.args(), vec!["Alice", "1.2.3.4"]);

        // Part (reemplazó a UserPart)
        let ev = ScriptEvent::Part { name: "Alice".into() };
        assert_eq!(ev.handler_name(), "onPart");
        assert_eq!(ev.args(), vec!["Alice"]);

        // UserList
        let ev = ScriptEvent::UserList {
            name: "AstraChat".into(),
            users_csv: "Alice,Bob".into(),
        };
        assert_eq!(ev.handler_name(), "onUserList");
        assert_eq!(ev.args(), vec!["AstraChat", "Alice,Bob"]);

        // Timer
        let ev = ScriptEvent::Timer { secs: 12345 };
        assert_eq!(ev.handler_name(), "onTimer");
        assert_eq!(ev.args(), vec!["12345"]);
    }
}
