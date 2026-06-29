//! # astra-commands
//!
//! Dispatcher de comandos slash (`/comando args`) para el chat.
//!
//! Conecta los mensajes TCP entrantes con los scripts JS. Cuando un usuario
//! envía un mensaje que empieza con `/`, el dispatcher lo enruta a los
//! scripts registrados vía `onCommand(from, command, args)`.
//!
//! ## Uso (en el tcp_handler)
//!
//! ```ignore
//! if text.starts_with('/') {
//!     let (cmd, args) = astra_commands::parse_command(&text);
//!     astra_commands::dispatch(&ctx, scripting, user_id, user_name, cmd, args);
//!     return; // no hacer broadcast del comando
//! }
//! ```

#![warn(missing_docs)]

use std::sync::Arc;

use server_core::AppContext;

use astra_scripting::{ScriptEvent, ScriptHandle};

/// Parsea un mensaje que empieza con `/` y retorna `(comando, args)`.
///
/// Ejemplos:
/// - `/hola` → `("hola", "")`
/// - `/hola mundo` → `("hola", "mundo")`
/// - `/kick alice spam` → `("kick", "alice spam")`
/// - `no es comando` → retorna None
pub fn parse_command(text: &str) -> Option<(&str, &str)> {
    let text = text.trim();
    if !text.starts_with('/') {
        return None;
    }
    let text = &text[1..];
    let mut parts = text.splitn(2, char::is_whitespace);
    let cmd = parts.next()?.trim();
    if cmd.is_empty() {
        return None;
    }
    let args = parts.next().unwrap_or("").trim();
    Some((cmd, args))
}

/// Dispatcha un comando a todos los scripts JS que tengan `onCommand`.
///
/// `from` es el nick del usuario que ejecutó el comando. `command` y
/// `args` vienen de `parse_command`.
pub fn dispatch(
    _ctx: &AppContext,
    scripting: &ScriptHandle,
    from: &str,
    command: &str,
    args: &str,
) {
    let event = ScriptEvent::Command {
        from: from.to_string(),
        command: command.to_string(),
        args: args.to_string(),
    };
    scripting.dispatch(event);
}

/// Helper: parsea y dispatcha un mensaje en un solo paso.
///
/// Si el mensaje NO es un comando (no empieza con `/`), retorna `false`.
/// Si es un comando, lo dispatcha y retorna `true`.
pub fn try_dispatch(
    ctx: &AppContext,
    scripting: &ScriptHandle,
    from: &str,
    text: &str,
) -> bool {
    if let Some((cmd, args)) = parse_command(text) {
        dispatch(ctx, scripting, from, cmd, args);
        true
    } else {
        false
    }
}



/// Helper: mantiene un `ScriptHandle` para usar sin Arc manual.
pub struct CommandDispatcher {
    pub scripting: ScriptHandle,
}

impl CommandDispatcher {
    /// Dispatcha un mensaje.
    pub fn try_dispatch(&self, ctx: &AppContext, from: &str, text: &str) -> bool {
        try_dispatch(ctx, &self.scripting, from, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_command() {
        let (cmd, args) = parse_command("/hola").unwrap();
        assert_eq!(cmd, "hola");
        assert_eq!(args, "");
    }

    #[test]
    fn parse_command_with_args() {
        let (cmd, args) = parse_command("/kick alice spam").unwrap();
        assert_eq!(cmd, "kick");
        assert_eq!(args, "alice spam");
    }

    #[test]
    fn parse_command_with_trailing_space() {
        let (cmd, args) = parse_command("/hola   ").unwrap();
        assert_eq!(cmd, "hola");
        assert_eq!(args, "");
    }

    #[test]
    fn parse_command_with_one_arg() {
        let (cmd, args) = parse_command("/motd hola").unwrap();
        assert_eq!(cmd, "motd");
        assert_eq!(args, "hola");
    }

    #[test]
    fn parse_non_command() {
        assert!(parse_command("hola mundo").is_none());
        assert!(parse_command("").is_none());
    }

    #[test]
    fn parse_empty_command() {
        // Solo "/" sin nada más → no es un comando válido
        assert!(parse_command("/").is_none());
        assert!(parse_command("/  ").is_none());
    }
}
