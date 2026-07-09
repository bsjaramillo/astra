//! Tipos de datos básicos del usuario y la sala.
//!
//! Originalmente vivían en el crate `iconnect` (la capa de interfaces del
//! plugin API de sb0t). Como Astra no expone un ABI de plugins de terceros
//! —la extensibilidad se hace vía scripting JS embebido—, esa capa se
//! eliminó y solo se conservan aquí los tres tipos de datos que el server
//! realmente usa.

/// Nivel de un usuario en la sala (equivalente a `ILevel` en sb0t).
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

/// Fuente del texto de un usuario (equivalente a `IFont` en sb0t).
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

/// Estado de link de un usuario (equivalente a `ILink` en sb0t).
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
