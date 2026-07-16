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
use std::net::IpAddr;

use server_core::{outbound, AppContext, AresUser};
use server_core::{FilterAction, ILevel};

use astra_scripting::{ScriptEvent, ScriptHandle};

const DEFAULT_HELP_LINES: &[&str] = &[
    "Available commands:",
    "/help - show this help",
    "/nick <name> - change your nickname",
    "/vroom <id> - move to another virtual room",
    "/customname [text|-] - set or clear your custom name",
    "/users - list connected users",
    "/topic [text] - show or set room topic",
    "/motd [text] - show or set message of the day",
    "/ban <nick> - ban online user",
    "/unban <nick|ip|ident> - remove ban",
    "/banlist - list active bans",
    "/whois <nick> - show user info",
    "/kick <nick> - kick user without banning",
    "/muzzle <nick> - mute user in public chat",
    "/unmuzzle <nick> - restore user's public voice",
    "/pmall <text> - PM every connected user",
    "/opmsg <text> - message all moderators+",
    "/uptime - show server uptime and stats",
    "/stats - server metrics block (mod)",
    "/shout <text> - shout as server text",
    "/version - show server version",
    "/register <password> - register your account",
    "/unregister - delete your account",
    "/login <password> - log into your account",
    "/logout - log out of your account",
    "/setlevel <nick> <0-3> - set user level (sb0t scale, owner)",
    "/idle - mark yourself as away (also: /me idles <text>)",
    "/idles - same as /idle",
    "/grant <nick> <level> - set user level",
    "/revoke <nick> - reset user to regular",
    "/cmdlevel <command> [level|reset] - view or set a command's required level (owner)",
    "/greets [on|off] - toggle or show greet status",
    "/addgreet <text> - add a greeting (placeholders +n +ip +uc +rn ...)",
    "/remgreet <index> - remove greeting by index",
    "/listgreets - list greetings",
    "/addfilter <word> [block|kick|ban|announce] - add a chat word filter",
    "/remfilter <word> - remove a word filter",
    "/listfilters - list word filters",
    "/addline <index>, <text> - add a response line to an announce-type filter",
    "/remline <index>, <line> - remove a response line (removes the filter if it was the last line)",
    "/viewfilter <index> - view the response lines of an announce-type filter",
    "/url [on|off] - toggle or show rotating room URLs",
    "/addurl <address> <text> - add a rotating room URL",
    "/remurl <index> - remove a room URL",
    "/listurl - list room URLs",
    "/history [on|off] - replay recent chat to joining users",
    "/whowas <nick|ip> - search seen-user history",
    "/lastseen <on|off|nick|ip> - last-seen announce on join, or query",
    "/roominfo [on|off] - room statistics (toggle = periodic broadcast)",
    "/status [text] - show or set room status",
    "/id <nick> - show a user's session id",
    "/info - list all users with vroom and id",
    "/customnames - list online users' custom names",
    "/rangeban <ip-prefix> - ban an IP prefix",
    "/rangeunban <ip-prefix|index> - remove a range ban",
    "/listrangebans - list range bans",
    "/asnban <asn> - ban an ASN",
    "/asnunban <asn> - remove an ASN ban",
    "/listasnbans - list ASN bans",
    "/clearbans - remove all bans",
    "/banstats - show recent ban actions",
    "/move <nick> <vroom> - move a user to a vroom",
    "/changename <nick> <newname> - force-rename a user",
    "/oldname <nick> - show a user's original name",
    "/changemessage <nick> <text> - set a user's personal message",
    "/admins - list online ops",
    "/announce <text> - announce to the whole room",
    "/adminmsg <text> - message all ops",
    "/pmroom <text> - PM every user",
    "/echo <nick> [text] - heckle a user privately (empty clears)",
    "/clone <nick> <text> - make a message appear from a user",
    "/kiddy <nick> - toggle kiddie-text on a user",
    "/mtimeout <nick> <secs> - muzzle a user temporarily",
    "/redirect <nick> <ip:port> - redirect a user to another server",
    "/disableadmins - disable admin commands (owner)",
    "/enableadmins - re-enable admin commands (owner)",
    "/roomflags - show all room permission flags",
    "/caps [on|off] - lowercase shouted messages",
    "/scribbles [on|off] - allow room scribbles",
    "/avatars [on|off] - allow avatars",
    "/audios [on|off] - allow voice messages",
    "/buzzes [on|off] - allow buzzes",
    "/colors [on|off] - allow colored text",
    "/anon [on|off] - monitor anonymous users",
    "/general [on|off] - general chat toggle",
    "/sharefiles [on|off] - monitor file sharing",
    "/roomsearch [on|off] - list room in UDP search",
    "/stealth [on|off] - hide admin identity in actions",
    "/disableavatar [on|off] - disable avatars",
    "/cloak [on|off] - cloak yourself in admin actions",
    "/lower <nick> - force a user's text to lowercase (/unlower)",
    "/kewltext <nick> - leetspeak a user's text (/unkewltext)",
    "/paint <nick> - decorate a user's text (/unpaint)",
    "/clearscreen - clear everyone's chat",
    "/clock [on|off] - show a clock in the topic",
    "/idle [on|off] - toggle idle monitoring",
    "/locate <nick> - show a user's country/region",
    "/listquarantined - list quarantined users",
    "/unquarantine <nick|index> - release a quarantined user",
    "/listpasswords - list registered accounts (owner)",
    "/addautologin <nick> <level> - auto-grant a level by IP recognition, no account needed (owner)",
    "/remautologin <id> - remove an IP autologin entry (owner)",
    "/autologins - list IP autologin entries (owner)",
    "/joinfilter [add|del <pat>|list] - filter nicks at login",
    "/filefilter [add|del <pat>|list] - filter shared file names",
    "/vspy [on|off] - watch other vrooms' chat",
    "/ipsend [on|off] - receive joiners' IP info",
    "/logsend [on|off] - receive a room activity log",
    "/bansend [on|off] - receive ban notifications",
    "/trace <nick|ip> - geolocate a user (needs GeoIP db)",
    "/define <word> - dictionary definition (Wordnik)",
    "/urban <term> - Urban Dictionary lookup",
    "/listscripts - list loaded scripts (owner)",
    "/loadscript <name> - load a script from disk (owner)",
    "/killscript <name> - unload a script (owner)",
    "/livescripts - search GitHub for community scripts (owner)",
    "/downloadscript <owner/repo> - download and load a script from GitHub (owner)",
    "/errors [on|off] - receive script error notifications",
];

/// Parsea un mensaje que empieza con `/` o `#` y retorna `(comando, args)`.
/// sb0t acepta ambos prefijos en texto público (`TCPProcessor.cs:343`).
///
/// Ejemplos:
/// - `/hola` → `("hola", "")`
/// - `#hola mundo` → `("hola", "mundo")`
/// - `/kick alice spam` → `("kick", "alice spam")`
/// - `no es comando` → retorna None
pub fn parse_command(text: &str) -> Option<(&str, &str)> {
    let text = text.trim();
    if !text.starts_with('/') && !text.starts_with('#') {
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
///
/// **Paridad sb0t (importante):** el 2º argumento que recibe `onCommand` es
/// la **línea completa** del comando (nombre + args), NO solo el nombre —
/// ver `TCPProcessor.Command` → `Events.Command(client, text, ...)` donde
/// `text` es todo lo escrito tras el `#`. Los scripts hacen
/// `command.split(" ")` para leer sus argumentos, así que pasar solo el
/// nombre rompe cualquier subcomando con parámetros (ej. `#nuevojuego perro`).
pub fn dispatch(
    ctx: &AppContext,
    scripting: &ScriptHandle,
    from: &str,
    command: &str,
    args: &str,
) {
    let full = command_full_line(command, args);
    let event = ScriptEvent::Command {
        from: from.to_string(),
        command: full,
        target: resolve_command_target(ctx, args),
        args: args.to_string(),
    };
    scripting.dispatch(event);
}

/// Reconstruye la línea completa del comando (`"<cmd> <args>"`) que sb0t
/// pasa a `onCommand` como 2º argumento.
pub fn command_full_line(command: &str, args: &str) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{} {}", command, args)
    }
}

/// Resuelve el `target` de un comando (paridad sb0t: el primer token de los
/// args, si corresponde a un usuario online). Vacío = sin target (null en JS).
pub fn resolve_command_target(ctx: &AppContext, args: &str) -> String {
    let first = args.trim().split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        return String::new();
    }
    match ctx.user_pool.get_by_name(first) {
        Some(u) => u.name.read().clone(),
        None => String::new(),
    }
}

/// Ejecuta comandos built-in del servidor.
///
/// Retorna `(handled, events)`:
/// - `handled = true` si el comando fue manejado (no debe pasar a scripts)
/// - `events` es una lista de eventos a disparar al scripting como side-effects
///   (ej: `AdminLevelChanged` cuando /ban tiene éxito).
pub fn dispatch_builtin(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    command: &str,
    args: &str,
) -> (bool, Vec<astra_scripting::ScriptEvent>) {
    let cmd = command.to_ascii_lowercase();

    // Gate de captcha (paridad core/Events.cs:470-481): con captcha pendiente
    // solo se responde `help` y se permite `login`; el resto se ignora en
    // silencio (sb0t ni siquiera llega a los scripts).
    if user.needs_captcha.load(std::sync::atomic::Ordering::Relaxed)
        && cmd != "help"
        && cmd != "login"
    {
        return (true, vec![]);
    }

    // Gate global `/disableadmins`: si está activo, solo el Owner puede usar
    // comandos admin (todo salvo los de usuario común y el propio toggle).
    if ctx.admins_disabled.load(std::sync::atomic::Ordering::Relaxed)
        && !has_level(user, ILevel::Owner)
        && !is_user_command(&cmd)
    {
        // Silencioso, como sb0t (ServerEvents.cs:886): ni siquiera responde.
        return (true, vec![]);
    }

    // Gate centralizado y configurable por comando (paridad con el sistema
    // de `[CommandLevel]` + registro de sb0t). Solo aplica a comandos
    // gestionados por `CommandLevelManager`; los demás (p.ej. registrados
    // por scripts) no se ven afectados.
    if let Some(required) = ctx.command_levels.get(&cmd) {
        if !has_level(user, required) {
            send_system_line(ctx, user, access_denied_text(required));
            return (true, vec![]);
        }
    }

    match cmd.as_str() {
        "help" => {
            handle_help(ctx, user, args);
            // onHelp(userobj): el script recibe a quien pidió ayuda y le
            // imprime sus líneas (paridad core/Events.cs → js.Help(client)).
            let from = user.name.read().clone();
            (true, vec![astra_scripting::ScriptEvent::Help { from }])
        }
        "nick" => {
            let events = handle_nick(ctx, user, args);
            (true, events)
        }
        "vroom" => {
            let events = handle_vroom(ctx, user, args);
            (true, events)
        }
        "users" => {
            handle_users(ctx, user, args);
            (true, vec![])
        }
        "topic" => {
            handle_topic(ctx, user, args);
            (true, vec![])
        }
        "motd" => {
            handle_motd(ctx, user, args);
            (true, vec![])
        }
        "ban" => {
            let target_name = args.trim().to_string();
            let success = handle_ban(ctx, user, args);
            let events = if success {
                vec![astra_scripting::ScriptEvent::AdminLevelChanged {
                    name: target_name,
                }]
            } else {
                vec![]
            };
            (true, events)
        }
        "unban" => {
            let target_name = args.trim().to_string();
            let success = handle_unban(ctx, user, args);
            let events = if success {
                vec![astra_scripting::ScriptEvent::AdminLevelChanged {
                    name: target_name,
                }]
            } else {
                vec![]
            };
            (true, events)
        }
        // Comandos host* (nivel Host = Owner en Astra). Operan sobre el pool
        // local; en un setup multi-servidor se propagarían por el link (pendiente).
        "hostban" => {
            if require_host(ctx, user) {
                let name = args.trim().to_string();
                let ok = handle_ban(ctx, user, args);
                publish_host_action(ctx, user, server_core::admin_action::BAN, &name);
                if ok {
                    return (true, vec![astra_scripting::ScriptEvent::AdminLevelChanged { name }]);
                }
            }
            (true, vec![])
        }
        "hostkick" | "hostkill" => {
            if require_host(ctx, user) {
                handle_kick(ctx, user, args);
                publish_host_action(ctx, user, server_core::admin_action::KICK, args.trim());
            }
            (true, vec![])
        }
        "hostmuzzle" => {
            if require_host(ctx, user) {
                handle_muzzle(ctx, user, args, true);
                publish_host_action(ctx, user, server_core::admin_action::MUZZLE, args.trim());
            }
            (true, vec![])
        }
        "hostunmuzzle" => {
            if require_host(ctx, user) {
                handle_muzzle(ctx, user, args, false);
                publish_host_action(ctx, user, server_core::admin_action::UNMUZZLE, args.trim());
            }
            (true, vec![])
        }
        "hostunban" => {
            if require_host(ctx, user) {
                handle_unban(ctx, user, args);
            }
            (true, vec![])
        }
        "hostcban" => {
            if require_host(ctx, user) {
                handle_hostcban(ctx, user);
            }
            (true, vec![])
        }
        "hostclone" => {
            if require_host(ctx, user) {
                handle_clone(ctx, user, args);
            }
            (true, vec![])
        }
        "ban10" => {
            handle_ban_timed(ctx, user, args, 600);
            (true, vec![])
        }
        "ban60" => {
            handle_ban_timed(ctx, user, args, 3600);
            (true, vec![])
        }
        "banlist" => {
            handle_banlist(ctx, user, args);
            (true, vec![])
        }
        "whois" => {
            handle_whois(ctx, user, args);
            (true, vec![])
        }
        "kick" | "kill" => {
            handle_kick(ctx, user, args);
            (true, vec![])
        }
        "muzzle" => {
            handle_muzzle(ctx, user, args, true);
            (true, vec![])
        }
        "unmuzzle" => {
            handle_muzzle(ctx, user, args, false);
            (true, vec![])
        }
        "pmall" => {
            handle_pmall(ctx, user, args);
            (true, vec![])
        }
        "opmsg" => {
            handle_opmsg(ctx, user, args);
            (true, vec![])
        }
        "uptime" => {
            handle_uptime(ctx, user, args);
            (true, vec![])
        }
        // `/stats` (Mod, paridad Eval.Stats): bloque multilínea de métricas.
        "stats" => {
            handle_stats(ctx, user, args);
            (true, vec![])
        }
        "version" => {
            handle_version(ctx, user, args);
            (true, vec![])
        }
        "register" => {
            let events = handle_register(ctx, user, args);
            (true, events)
        }
        "unregister" => {
            let events = handle_unregister(ctx, user, args);
            (true, events)
        }
        // `/rempassword <índice>` (Host, paridad Eval.RemovePassword): borra
        // una cuenta de la lista de /listpasswords. Distinto de /unregister
        // (auto-baja del propio usuario).
        "rempassword" => {
            handle_rempassword(ctx, user, args);
            (true, vec![])
        }
        // `/logout`/`/logoff` (paridad AccountManager.Logout): cierra la
        // sesión de la cuenta — vuelve a nivel regular sin borrar la cuenta.
        "logout" | "logoff" => {
            let events = handle_logout(ctx, user);
            (true, events)
        }
        // `/setlevel <nick> <0-3>` (Owner, paridad core/Events.cs:519).
        "setlevel" => {
            let events = handle_setlevel(ctx, user, args);
            (true, events)
        }
        "whisper" => {
            handle_whisper(ctx, user, args);
            (true, vec![])
        }
        "pmblock" => {
            handle_pmblock(ctx, user, args);
            (true, vec![])
        }
        "login" => {
            let changed = handle_login(ctx, user, args);
            let events = if changed {
                vec![astra_scripting::ScriptEvent::AdminLevelChanged {
                    name: user.name.read().clone(),
                }]
            } else {
                vec![]
            };
            (true, events)
        }
        "grant" => {
            let events = match handle_grant(ctx, user, args) {
                Some(target) => {
                    vec![astra_scripting::ScriptEvent::AdminLevelChanged { name: target }]
                }
                None => vec![],
            };
            (true, events)
        }
        "revoke" => {
            let events = match handle_revoke(ctx, user, args) {
                Some(target) => {
                    vec![astra_scripting::ScriptEvent::AdminLevelChanged { name: target }]
                }
                None => vec![],
            };
            (true, events)
        }
        "cmdlevel" => {
            handle_cmdlevel(ctx, user, args);
            (true, vec![])
        }
        "greets" => {
            handle_greets(ctx, user, args);
            (true, vec![])
        }
        "addgreet" => {
            handle_addgreet(ctx, user, args);
            (true, vec![])
        }
        "remgreet" => {
            handle_remgreet(ctx, user, args);
            (true, vec![])
        }
        "listgreets" => {
            handle_listgreets(ctx, user, args);
            (true, vec![])
        }
        "addfilter" => {
            handle_addfilter(ctx, user, args);
            (true, vec![])
        }
        "remfilter" => {
            handle_remfilter(ctx, user, args);
            (true, vec![])
        }
        "listfilters" => {
            handle_listfilters(ctx, user, args);
            (true, vec![])
        }
        "url" => {
            handle_url(ctx, user, args);
            (true, vec![])
        }
        "addurl" => {
            handle_addurl(ctx, user, args);
            (true, vec![])
        }
        "remurl" => {
            handle_remurl(ctx, user, args);
            (true, vec![])
        }
        "listurl" | "listurls" => {
            handle_listurl(ctx, user, args);
            (true, vec![])
        }
        "history" => {
            handle_history(ctx, user, args);
            (true, vec![])
        }
        "whowas" => {
            handle_whowas(ctx, user, args);
            (true, vec![])
        }
        "lastseen" => {
            handle_lastseen(ctx, user, args);
            (true, vec![])
        }
        "roominfo" => {
            handle_roominfo(ctx, user, args);
            (true, vec![])
        }
        "status" => {
            handle_status(ctx, user, args);
            (true, vec![])
        }
        "id" => {
            handle_id(ctx, user, args);
            (true, vec![])
        }
        "info" => {
            handle_info(ctx, user, args);
            (true, vec![])
        }
        "customnames" => {
            handle_customnames(ctx, user, args);
            (true, vec![])
        }
        "rangeban" => {
            handle_rangeban(ctx, user, args);
            (true, vec![])
        }
        "rangeunban" => {
            handle_rangeunban(ctx, user, args);
            (true, vec![])
        }
        "listrangebans" => {
            handle_listrangebans(ctx, user, args);
            (true, vec![])
        }
        "asnban" => {
            handle_asnban(ctx, user, args);
            (true, vec![])
        }
        "asnunban" => {
            handle_asnunban(ctx, user, args);
            (true, vec![])
        }
        "listasnbans" => {
            handle_listasnbans(ctx, user, args);
            (true, vec![])
        }
        "clearbans" | "cbans" => {
            handle_clearbans(ctx, user, args);
            (true, vec![])
        }
        "banstats" => {
            handle_banstats(ctx, user, args);
            (true, vec![])
        }
        "move" => {
            handle_move(ctx, user, args);
            (true, vec![])
        }
        "changename" => {
            handle_changename(ctx, user, args);
            (true, vec![])
        }
        "oldname" => {
            handle_oldname(ctx, user, args);
            (true, vec![])
        }
        "changemessage" => {
            handle_changemessage(ctx, user, args);
            (true, vec![])
        }
        "admins" => {
            handle_admins(ctx, user, args);
            (true, vec![])
        }
        "announce" => {
            handle_announce(ctx, user, args);
            (true, vec![])
        }
        "adminmsg" => {
            handle_opmsg(ctx, user, args);
            (true, vec![])
        }
        // `/adminannounce on|off` (Host, paridad Eval.AdminAnnounce): toggle
        // que hace que los word-filters tipo Announce NO disparen para
        // usuarios regulares. Distinto de /adminmsg (mensaje a admins).
        "adminannounce" => {
            handle_room_flag(ctx, user, &cmd, args);
            (true, vec![])
        }
        "pmroom" => {
            handle_pmall(ctx, user, args);
            (true, vec![])
        }
        "echo" => {
            handle_echo(ctx, user, args);
            (true, vec![])
        }
        "clone" => {
            handle_clone(ctx, user, args);
            (true, vec![])
        }
        "kiddy" => {
            handle_kiddy(ctx, user, args);
            (true, vec![])
        }
        "unkiddy" => {
            handle_unkiddy(ctx, user, args);
            (true, vec![])
        }
        "unecho" => {
            // sb0t: limpia el echo del target (texto vacío = clear).
            handle_echo(ctx, user, args.trim());
            (true, vec![])
        }
        "shout" => {
            handle_shout(ctx, user, args);
            (true, vec![])
        }
        "mtimeout" => {
            handle_mtimeout(ctx, user, args);
            (true, vec![])
        }
        "redirect" => {
            handle_redirect(ctx, user, args);
            (true, vec![])
        }
        "disableadmins" => {
            handle_disableadmins(ctx, user, true);
            (true, vec![])
        }
        "enableadmins" => {
            handle_disableadmins(ctx, user, false);
            (true, vec![])
        }
        // Flags de sala (toggles on|off). `disableavatar` mapea a `avatars`.
        "caps" | "anon" | "general" | "audios" | "buzzes" | "scribbles" | "colors"
        | "sharefiles" | "avatars" | "stealth" => {
            handle_room_flag(ctx, user, &cmd, args);
            (true, vec![])
        }
        // En sb0t `roomsearch <texto>` BUSCA en la lista de canales Ares
        // (Eval.cs:1300, imprime top-5 con hashlinks) — no es un toggle.
        "roomsearch" => {
            handle_unavailable(ctx, user, "roomsearch");
            (true, vec![])
        }
        "disableavatar" => {
            handle_disableavatar(ctx, user, args);
            (true, vec![])
        }
        "roomflags" => {
            handle_roomflags(ctx, user, args);
            (true, vec![])
        }
        "cloak" => {
            handle_cloak(ctx, user, args);
            (true, vec![])
        }
        "lower" => {
            handle_text_effect(ctx, user, args, TextEffect::Lower, true);
            (true, vec![])
        }
        "unlower" => {
            handle_text_effect(ctx, user, args, TextEffect::Lower, false);
            (true, vec![])
        }
        // `addkewltext`/`remkewltext` son los nombres del dispatch de sb0t
        // (ServerEvents.cs:921); kewltext/unkewltext quedan como alias.
        "addkewltext" | "kewltext" => {
            handle_text_effect(ctx, user, args, TextEffect::Kewl, true);
            (true, vec![])
        }
        "remkewltext" | "unkewltext" => {
            handle_text_effect(ctx, user, args, TextEffect::Kewl, false);
            (true, vec![])
        }
        "paint" => {
            handle_text_effect(ctx, user, args, TextEffect::Paint, true);
            (true, vec![])
        }
        "unpaint" => {
            handle_text_effect(ctx, user, args, TextEffect::Paint, false);
            (true, vec![])
        }
        "clearscreen" => {
            handle_clearscreen(ctx, user, args);
            (true, vec![])
        }
        "clock" => {
            // Toggle de sala persistido (el efecto de clock lo aplica una task).
            handle_room_flag(ctx, user, &cmd, args);
            (true, vec![])
        }
        "idle" | "idles" => {
            let events = handle_idle(ctx, user, &cmd, args);
            (true, events)
        }
        "locate" => {
            handle_locate(ctx, user, args);
            (true, vec![])
        }
        "listquarantined" => {
            handle_listquarantined(ctx, user, args);
            (true, vec![])
        }
        "unquarantine" => {
            handle_unquarantine(ctx, user, args);
            (true, vec![])
        }
        "listpasswords" => {
            handle_listpasswords(ctx, user, args);
            (true, vec![])
        }
        "addautologin" => {
            handle_addautologin(ctx, user, args);
            (true, vec![])
        }
        "remautologin" => {
            handle_remautologin(ctx, user, args);
            (true, vec![])
        }
        "autologins" => {
            handle_ip_autologins(ctx, user);
            (true, vec![])
        }
        "joinfilter" | "joinfilters" => {
            handle_name_filter(ctx, user, args, true);
            (true, vec![])
        }
        "filefilter" | "filefilters" => {
            handle_name_filter(ctx, user, args, false);
            (true, vec![])
        }
        // Suscripciones per-admin a feeds internos (sin infra externa).
        "vspy" => {
            handle_subscription(ctx, user, args, Subscription::Vspy);
            (true, vec![])
        }
        "ipsend" => {
            handle_subscription(ctx, user, args, Subscription::IpSend);
            (true, vec![])
        }
        "logsend" => {
            handle_subscription(ctx, user, args, Subscription::LogSend);
            (true, vec![])
        }
        "bansend" => {
            handle_subscription(ctx, user, args, Subscription::BanSend);
            (true, vec![])
        }
        // Comandos que dependen de servicios/datos externos (APIs de
        // diccionario). Ver handlers dedicados.
        "define" => {
            handle_define(ctx, user, args);
            (true, vec![])
        }
        "urban" => {
            handle_urban(ctx, user, args);
            (true, vec![])
        }
        "trace" => {
            handle_trace(ctx, user, args);
            (true, vec![])
        }
        // ---- Aliases con los nombres originales de sb0t ----
        // `/greetmsg on|off` (Host, paridad Eval.GreetMsg): toggle del greet
        // PÚBLICO al entrar. El kill-switch general de greets es `/greets`.
        "greetmsg" => {
            handle_room_flag(ctx, user, &cmd, args);
            (true, vec![])
        }
        "addgreetmsg" => {
            handle_addgreet(ctx, user, args);
            (true, vec![])
        }
        // `/pmgreetmsg on|off` (Host, paridad Eval.PMGreetMsg): toggle del
        // greet por PM al entrar. Distinto de /addgreetmsg (añade un greet).
        "pmgreetmsg" => {
            handle_room_flag(ctx, user, &cmd, args);
            (true, vec![])
        }
        "remgreetmsg" => {
            handle_remgreet(ctx, user, args);
            (true, vec![])
        }
        "listgreetmsg" => {
            handle_listgreets(ctx, user, args);
            (true, vec![])
        }
        "customname" => {
            handle_customname(ctx, user, args, true);
            (true, vec![])
        }
        "uncustomname" => {
            handle_customname(ctx, user, args, false);
            (true, vec![])
        }
        "listbans" => {
            handle_banlist(ctx, user, args);
            (true, vec![])
        }
        "wordfilters" => {
            handle_listfilters(ctx, user, args);
            (true, vec![])
        }
        "filter" => {
            handle_filter_dispatch(ctx, user, args);
            (true, vec![])
        }
        // Aliases planos de sb0t para los filtros (Astra usa subcomandos).
        "addwordfilter" => {
            handle_addfilter(ctx, user, args);
            (true, vec![])
        }
        "remwordfilter" => {
            handle_remfilter(ctx, user, args);
            (true, vec![])
        }
        "addline" => {
            handle_addline(ctx, user, args);
            (true, vec![])
        }
        "remline" => {
            handle_remline(ctx, user, args);
            (true, vec![])
        }
        "viewfilter" => {
            handle_viewfilter(ctx, user, args);
            (true, vec![])
        }
        "addjoinfilter" => {
            handle_name_filter(ctx, user, &format!("add {}", args.trim()), true);
            (true, vec![])
        }
        "remjoinfilter" => {
            handle_name_filter(ctx, user, &format!("del {}", args.trim()), true);
            (true, vec![])
        }
        "addfilefilter" => {
            handle_name_filter(ctx, user, &format!("add {}", args.trim()), false);
            (true, vec![])
        }
        "remfilefilter" => {
            handle_name_filter(ctx, user, &format!("del {}", args.trim()), false);
            (true, vec![])
        }
        "addtopic" => {
            handle_topic(ctx, user, args);
            (true, vec![])
        }
        "remtopic" => {
            handle_topic(ctx, user, "-");
            (true, vec![])
        }
        "viewmotd" => {
            handle_motd(ctx, user, "");
            (true, vec![])
        }
        // `/loadmotd` (Host, paridad Eval.LoadMotd): recarga el MOTD desde
        // la persistencia (por si se editó por fuera del proceso).
        "loadmotd" => {
            handle_loadmotd(ctx, user);
            (true, vec![])
        }
        "link" => {
            handle_link(ctx, user, args);
            (true, vec![])
        }
        "unlink" => {
            handle_unlink(ctx, user, args);
            (true, vec![])
        }
        "loadtemplate" => {
            handle_unavailable(ctx, user, "loadtemplate");
            (true, vec![])
        }
        "listscripts" => {
            handle_listscripts(ctx, user);
            (true, vec![])
        }
        "loadscript" => {
            handle_loadscript(ctx, user, args);
            (true, vec![])
        }
        "killscript" => {
            handle_killscript(ctx, user, args);
            (true, vec![])
        }
        "livescripts" => {
            handle_livescripts(ctx, user);
            (true, vec![])
        }
        "downloadscript" => {
            handle_downloadscript(ctx, user, args);
            (true, vec![])
        }
        "errors" => {
            handle_subscription(ctx, user, args, Subscription::Errors);
            (true, vec![])
        }
        _ => (false, vec![]),
    }
}

/// Efecto de texto per-usuario que un mod puede aplicar a un target.
#[derive(Clone, Copy)]
enum TextEffect {
    Lower,
    Kewl,
    Paint,
}

/// ¿Es un comando de usuario común (no gateado por `/disableadmins`)?
/// Paridad sb0t: bajo DisableAdmins solo pasan los comandos del core de
/// cuentas (`core/Events.cs`: help/register/login/logout/nick/setlevel-no,
/// idle/idles) — el resto, incluso lecturas como whois, se bloquea. Los
/// extras de Astra (users/topic/motd/uptime) se mantienen accesibles.
fn is_user_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "help" | "register" | "unregister" | "login" | "logout" | "logoff"
            | "nick" | "idle" | "idles"
            // Extras de Astra sin equivalente sb0t (lecturas inocuas).
            | "users" | "topic" | "motd" | "uptime"
    )
}

/// Extrae el nombre de comando al inicio de una línea de `DEFAULT_HELP_LINES`
/// (ej. `"/ban <nick> - ..."` → `Some("ban")`). Retorna `None` para líneas
/// que no describen un comando (ej. el encabezado `"Available commands:"`).
fn help_line_command(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('/')?;
    let end = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some(&rest[..end])
    }
}

fn handle_help(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    for line in DEFAULT_HELP_LINES {
        if let Some(name) = help_line_command(line) {
            if let Some(required) = ctx.command_levels.get(name) {
                if !has_level(user, required) {
                    continue;
                }
            }
        }
        send_system_line(ctx, user, line);
    }
    // Agregar líneas registradas por scripts vía `Help_addLine(cmd, line)`.
    // Solo se muestran cuando el user hace `/help` (sin args específicos).
    for (cmd, line) in astra_scripting::api::extra_help_lines() {
        let formatted = format!("/{} - {}", cmd, line);
        send_system_line(ctx, user, &formatted);
    }
}

fn handle_nick(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Vec<astra_scripting::ScriptEvent> {
    let new_name = args.trim();
    if new_name.is_empty() {
        send_system_line(ctx, user, "Usage: /nick <name>");
        return vec![];
    }
    if new_name.chars().count() > 30 {
        send_system_line(ctx, user, "Nickname too long.");
        return vec![];
    }

    let old_name = user.name.read().clone();
    if old_name.eq_ignore_ascii_case(new_name) {
        send_system_line(ctx, user, "You already have that nickname.");
        return vec![];
    }
    if ctx.user_pool.get_by_name(new_name).is_some() {
        send_system_line(ctx, user, "Nickname already in use.");
        return vec![];
    }

    *user.name.write() = new_name.to_string();
    ctx.user_pool.rename(user.id, &old_name, new_name);

    let mut part_user = AresUser::new(user.id, user.external_ip, user.guid);
    part_user.logged_in = true;
    *part_user.name.write() = old_name.clone();
    for other in ctx.user_pool.users() {
        if other.logged_in && *other.vroom.read() == *user.vroom.read() && !other.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = other.send(outbound::build_part_c(&part_user, other.ares_crypto));
            let _ = other.send(outbound::build_join_or_userlist_c(user, other.ares_crypto));
        }
    }

    ctx.publish_link_event(server_core::LinkEvent::NickChanged {
        origin: None,
        old_name: old_name.clone(),
        user: server_core::LinkUserSnapshot::from_user(user),
    });

    send_system_line(ctx, user, "Nickname updated.");
    vec![astra_scripting::ScriptEvent::Nick {
        old: old_name,
        new: new_name.to_string(),
    }]
}

fn handle_vroom(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Vec<astra_scripting::ScriptEvent> {
    let Ok(new_vroom) = args.trim().parse::<u16>() else {
        send_system_line(ctx, user, "Usage: /vroom <id>");
        return vec![];
    };

    let old_vroom = *user.vroom.read();
    if old_vroom == new_vroom {
        send_system_line(ctx, user, "You are already in that vroom.");
        return vec![];
    }

    // Gate de scripts (onVroomJoinCheck, paridad sb0t): un script puede
    // rechazar el cambio. Silencioso, como sb0t.
    {
        let uname = user.name.read().clone();
        if !ctx.check_vroom_join(&uname, new_vroom) {
            return vec![];
        }
    }

    // Auto-crear el vroom destino si no existe (compat con sb0t)
    if ctx.vrooms.get(new_vroom).is_none() {
        let _ = ctx.vrooms.create(new_vroom, None, None);
    }

    let mut part_user = AresUser::new(user.id, user.external_ip, user.guid);
    part_user.logged_in = true;
    *part_user.name.write() = user.name.read().clone();
    *part_user.vroom.write() = old_vroom;

    *user.vroom.write() = new_vroom;

    for other in ctx.user_pool.users() {
        if !other.logged_in || other.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let other_vroom = *other.vroom.read();
        if other_vroom == old_vroom {
            let _ = other.send(outbound::build_part_c(&part_user, other.ares_crypto));
        }
        if other_vroom == new_vroom {
            let _ = other.send(outbound::build_join_or_userlist_c(user, other.ares_crypto));
        }
    }

    ctx.publish_link_event(server_core::LinkEvent::VroomChanged {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(user),
    });

    send_system_line(ctx, user, &format!("Moved to vroom {}.", new_vroom));
    // onVroomJoin — path-independent: lo despachan tanto el path TCP como el
    // web al dispatchear los eventos que retorna el builtin.
    vec![astra_scripting::ScriptEvent::VroomJoin {
        name: user.name.read().clone(),
        vroom: new_vroom,
    }]
}

fn handle_customname(ctx: &AppContext, user: &Arc<AresUser>, args: &str, set: bool) {
    // Paridad sb0t Eval.CustomName/UncustomName: forma target-based para
    // mods (`/customname <nick> <nombre>`) y self-service para el propio
    // usuario (permitido si nivel > Regular o el flag `general` está on).
    let trimmed = args.trim();

    if trimmed.is_empty() {
        if set {
            // Extra de Astra: sin args muestra el propio custom name.
            let current = user.custom_name.read().clone();
            match current {
                Some(value) => send_system_line(ctx, user, &format!("Custom name: {}", value)),
                None => send_system_line(ctx, user, "Custom name is not set."),
            }
        } else {
            // `/uncustomname` sin args: limpia el propio.
            apply_custom_name(ctx, user, user, None);
        }
        return;
    }

    // ¿Primer token es un usuario online? → forma target-based.
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    if let Some(target) = ctx.user_pool.get_by_name(first) {
        if target.id != user.id {
            if !can_edit_topic(user) {
                send_system_line(ctx, user, &ctx.templates.get("error.access_moderator"));
                return;
            }
            let value = if set {
                if rest.len() < 2 {
                    send_system_line(ctx, user, "Usage: /customname <nick> <name>");
                    return;
                }
                Some(rest)
            } else {
                None
            };
            if let Some(v) = value {
                if custom_name_blocked(v) {
                    return;
                }
            }
            apply_custom_name(ctx, user, &target, value);
            return;
        }
    }

    // Self-service (sb0t: nivel > Regular o Settings.General). Además, el
    // custom name iniciado por el propio usuario requiere que la sala tenga
    // custom names habilitados (sb0t `Settings.Get<bool>("customnames")`,
    // AresClient.cs:270; toggle `#customnames on|off`, o `Room.customNames`
    // desde scripts). El seteo por un mod (target-based, arriba) no se gatea.
    if !(has_level(user, ILevel::Voice) || ctx.room_flags.get("general")) {
        return;
    }
    if set && !ctx.room_flags.get("customnames") {
        send_system_line(ctx, user, "Custom names are disabled in this room.");
        return;
    }
    let value = if set { Some(trimmed) } else { None };
    if let Some(v) = value {
        if v.len() < 2 || custom_name_blocked(v) {
            return;
        }
    }
    apply_custom_name(ctx, user, user, value);
}

/// Substrings vetados en custom names (paridad sb0t: evitar spam de salas).
fn custom_name_blocked(v: &str) -> bool {
    let up = v.to_uppercase();
    up.contains("CHATROOM") || up.contains("HTTP") || up.contains("WWW") || up.contains("ARLNK")
}

/// Aplica (o limpia) el custom name de `target` y anuncia la acción a la
/// sala (AdminAction #5/#6 de sb0t, stealth-aware).
fn apply_custom_name(
    ctx: &AppContext,
    issuer: &Arc<AresUser>,
    target: &Arc<AresUser>,
    value: Option<&str>,
) {
    let next = value.map(|v| v.chars().take(40).collect::<String>());
    *target.custom_name.write() = next.clone();
    ctx.publish_link_event(server_core::LinkEvent::CustomName {
        origin: None,
        name: target.name.read().clone(),
        custom_name: next.clone(),
    });
    let key = if next.is_some() {
        "adminaction.customname"
    } else {
        "adminaction.uncustomname"
    };
    announce_admin_action(ctx, issuer, key, &target.name.read().clone());
}
fn handle_users(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let mut users: Vec<String> = ctx
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed))
        .map(|u| u.name.read().clone())
        .collect();

    users.sort_by_cached_key(|n| n.to_ascii_lowercase());
    let count = users.len();
    let names = if users.is_empty() {
        "none".to_string()
    } else {
        users.join(", ")
    };

    send_system_line(ctx, user, &format!("Users online: {}", count));
    send_system_line(ctx, user, &format!("{}", names));
}

fn handle_topic(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if args.trim().is_empty() {
        send_system_line(ctx, user, &format!("Topic: {}", ctx.current_room_topic()));
        return;
    }

    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    // "-" limpia el topic (paridad con /remtopic).
    let new_topic = if args.trim() == "-" {
        String::new()
    } else {
        truncate_text(args.trim(), 300)
    };
    ctx.set_room_topic(new_topic.clone());
    broadcast_topic(ctx, &new_topic);
    send_system_line(ctx, user, "Topic updated.");
}

fn handle_motd(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    // Sin argumentos: mostrar el MOTD actual (message of the day) al que lo
    // pide, línea por línea con los placeholders sustituidos para él.
    if args.trim().is_empty() {
        if ctx.motd.is_empty() {
            send_system_line(ctx, user, "No MOTD is set.");
            return;
        }
        let mctx = server_core::MotdContext {
            name: &user.name.read(),
            room_name: &ctx.settings.room_name,
            ip: &user.external_ip.to_string(),
            user_count: ctx.user_pool.len(),
        };
        for line in ctx.motd.rendered_lines(&mctx) {
            send_system_line(ctx, user, &line);
        }
        return;
    }

    if !can_edit_topic(user) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_moderator"));
        return;
    }

    // Setear el MOTD desde el chat (una línea; el editor multilínea real es
    // el panel de administración). No toca el topic de la sala.
    ctx.motd.set(args.trim());
    send_system_line(ctx, user, "MOTD updated.");
}


/// Anuncia una acción de moderación a toda la sala (paridad `Server.Print`
/// de sb0t con `Category.AdminAction`). Con el flag `stealth` activo o el
/// admin cloaked, se firma con el nombre de la sala en vez del admin.
fn announce_admin_action(ctx: &AppContext, issuer: &Arc<AresUser>, key: &str, target: &str) {
    let signer = if ctx.room_flags.get("stealth")
        || issuer.cloaked.load(std::sync::atomic::Ordering::Relaxed)
    {
        ctx.settings.room_name.clone()
    } else {
        issuer.name.read().clone()
    };
    ctx.broadcast_print(&ctx.templates.render(key, &[("+n", target), ("+a", &signer)]));
}

fn handle_ban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> bool {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_moderator"));
        return false;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /ban <nick>");
        return false;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return false;
    };

    let ident = ctx.bans.ban(
        &target.name.read(),
        &target.version,
        &target.guid,
        target.external_ip,
        target.local_ip,
        target.data_port,
    );

    if ident == 0 {
        send_system_line(ctx, user, "Failed to persist ban.");
        return false;
    }

    send_system_line(
        ctx,
        user,
        &ctx.templates.render(
            "ban.confirm",
            &[("+n", &target.name.read()), ("+i", &ident.to_string())],
        ),
    );
    send_system_line(ctx, &target, &ctx.templates.get("ban.target"));

    // Registrar la acción para /banstats.
    ctx.record_ban(
        &user.name.read(),
        &target.name.read(),
        &target.external_ip.to_string(),
    );

    // Feed /bansend a los admins suscritos.
    let bansend_line = format!(
        "BANSEND: {} banned {} [{}]",
        user.name.read(),
        target.name.read(),
        target.external_ip
    );
    ctx.notify_subscribers(&bansend_line, |u| {
        u.sub_bansend.load(std::sync::atomic::Ordering::Relaxed)
    });

    // Anuncio público (AdminAction#0 de sb0t).
    announce_admin_action(ctx, user, "adminaction.ban", &target.name.read().clone());

    // Expulsión inmediata del pool para reflejar el ban en runtime.
    force_part_user(ctx, &target);
    true
}

/// `/ban10` y `/ban60`: ban temporal (expira en `secs` segundos). Paridad sb0t.
fn handle_ban_timed(ctx: &AppContext, user: &Arc<AresUser>, args: &str, secs: i64) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, &format!("Usage: /ban{} <nick>", secs / 60));
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    let ident = ctx.bans.ban_with_expiry(
        &target.name.read(),
        &target.version,
        &target.guid,
        target.external_ip,
        target.local_ip,
        target.data_port,
        secs,
    );
    if ident == 0 {
        send_system_line(ctx, user, "Failed to persist ban.");
        return;
    }
    let mins = secs / 60;
    send_system_line(
        ctx,
        user,
        &format!("Banned '{}' for {} minutes (ident {}).", target.name.read(), mins, ident),
    );
    send_system_line(
        ctx,
        &target,
        &format!("You have been banned from this room for {} minutes.", mins),
    );
    let key = if secs <= 600 { "adminaction.ban10" } else { "adminaction.ban60" };
    announce_admin_action(ctx, user, key, &target.name.read().clone());
    ctx.record_ban(
        &user.name.read(),
        &target.name.read(),
        &target.external_ip.to_string(),
    );
    force_part_user(ctx, &target);
}

/// `/unkiddy <nick>`: fuerza el modo kiddy a OFF (a diferencia de `/kiddy` que togglea).
fn handle_unkiddy(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /unkiddy <nick>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    target.kiddied.store(false, std::sync::atomic::Ordering::Relaxed);
    send_system_line(ctx, user, &format!("Kiddy mode off for '{}'.", target_name));
}

fn handle_unban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> bool {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_moderator"));
        return false;
    }

    let target = args.trim();
    if target.is_empty() {
        send_system_line(ctx, user, "Usage: /unban <nick|ip|ident>");
        return false;
    }

    let removed = if let Ok(ident) = target.parse::<u16>() {
        ctx.bans.unban(ident)
    } else if let Ok(ip) = target.parse::<IpAddr>() {
        ctx.bans.unban_by_ip(ip)
    } else if let Some(u) = ctx.user_pool.get_by_name(target) {
        ctx.bans.unban_by_guid(&u.guid) || ctx.bans.unban_by_ip(u.external_ip)
    } else {
        let mut by_name_ident: Option<u16> = None;
        ctx.bans.for_each(|b| {
            if b.name.eq_ignore_ascii_case(target) && by_name_ident.is_none() {
                by_name_ident = Some(b.ident);
            }
        });
        by_name_ident.map(|id| ctx.bans.unban(id)).unwrap_or(false)
    };

    if removed {
        send_system_line(ctx, user, &ctx.templates.get("unban.success"));
        announce_admin_action(ctx, user, "adminaction.unban", target);
    } else {
        send_system_line(ctx, user, &ctx.templates.get("unban.none"));
    }
    removed
}

fn handle_banlist(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    if ctx.bans.is_empty() {
        send_system_line(ctx, user, "Ban list is empty.");
        return;
    }

    send_system_line(ctx, user, "Active bans:");
    ctx.bans.for_each(|b| {
        let guid_hex = guid_to_hex(&b.guid);
        send_system_line(
            ctx,
            user,
            &format!(
                "#{} name='{}' ip={} guid={}",
                b.ident, b.name, b.external_ip, guid_hex
            ),
        );
    });
}

fn handle_whois(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /whois <nick>");
        return;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };

    // Formato multilínea de sb0t (Category.Whois #0-9).
    let registered = matches!(ctx.accounts.find_by_guid(&target.guid), Ok(Some(_)));
    let name = target.name.read().clone();
    let lines = [
        ctx.templates.render("whois.name", &[("+n", &name)]),
        ctx.templates.render("whois.extip", &[("+n", &target.external_ip.to_string())]),
        ctx.templates.render("whois.localip", &[("+n", &target.local_ip.to_string())]),
        ctx.templates.render("whois.dataport", &[("+n", &target.data_port.to_string())]),
        ctx.templates.render("whois.version", &[("+n", &target.version)]),
        ctx.templates.render("whois.vroom", &[("+n", &target.vroom.read().to_string())]),
        ctx.templates.render("whois.id", &[("+n", &target.id.to_string())]),
        ctx.templates.render("whois.registered", &[("+n", if registered { "True" } else { "False" })]),
    ];
    for line in lines {
        send_system_line(ctx, user, &line);
    }
    // Extra de Astra (no está en sb0t, útil para moderación).
    send_system_line(
        ctx,
        user,
        &format!(
            "Level: {} | files: {} | guid: {}",
            *target.level.read() as u8,
            target.file_count,
            guid_to_hex(&target.guid)
        ),
    );
}

fn handle_kick(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_moderator"));
        return;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /kick <nick>");
        return;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return;
    };

    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot kick a user of equal or higher level.");
        return;
    }

    send_system_line(ctx, &target, &ctx.templates.get("kick.target"));
    force_part_user(ctx, &target);
    send_system_line(ctx, user, &ctx.templates.render("kick.confirm", &[("+n", target_name)]));
    announce_admin_action(ctx, user, "adminaction.kick", target_name);
}

fn handle_muzzle(ctx: &AppContext, user: &Arc<AresUser>, args: &str, muzzle: bool) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_moderator"));
        return;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        let cmd = if muzzle { "muzzle" } else { "unmuzzle" };
        send_system_line(ctx, user, &format!("Usage: /{} <nick>", cmd));
        return;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return;
    };

    if muzzle && !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot muzzle a user of equal or higher level.");
        return;
    }

    let already = target
        .muzzled
        .swap(muzzle, std::sync::atomic::Ordering::Relaxed);
    if already == muzzle {
        let state = if muzzle { "already muzzled" } else { "not muzzled" };
        send_system_line(ctx, user, &format!("'{}' is {}.", target_name, state));
        return;
    }

    ctx.publish_link_event(server_core::LinkEvent::UserUpdated {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(&target),
    });

    if muzzle {
        send_system_line(ctx, &target, &ctx.templates.get("muzzle.target"));
        send_system_line(ctx, user, &ctx.templates.render("muzzle.confirm", &[("+n", target_name)]));
        announce_admin_action(ctx, user, "adminaction.muzzle", target_name);
    } else {
        send_system_line(ctx, &target, &ctx.templates.get("unmuzzle.target"));
        send_system_line(ctx, user, &ctx.templates.render("unmuzzle.confirm", &[("+n", target_name)]));
        announce_admin_action(ctx, user, "adminaction.unmuzzle", target_name);
    }
}

fn handle_pmall(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }

    let text = args.trim();
    if text.is_empty() {
        send_system_line(ctx, user, "Usage: /pmall <text>");
        return;
    }

    let from = user.name.read().clone();
    let mut count = 0usize;
    for other in ctx.user_pool.users() {
        if !other.logged_in
            || other.id == user.id
            || other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        let _ = other.send_pvt(&from, text);
        count += 1;
    }
    send_system_line(ctx, user, &format!("PM sent to {} user(s).", count));
}

fn handle_opmsg(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    let text = args.trim();
    if text.is_empty() {
        send_system_line(ctx, user, "Usage: /opmsg <text>");
        return;
    }

    let from = user.name.read().clone();
    let line = format!("[ops] {}: {}", from, text);
    let bot = ctx.settings.bot_name.clone();
    for other in ctx.user_pool.users() {
        if !other.logged_in
            || other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if (*other.level.read() as u8) >= ILevel::Moderator as u8 {
            let _ = other.send_pvt(&bot, &line);
        }
    }
}

/// Aviso solo para moderadores+ (paridad `Server.Print(ILevel.Moderator,...)`).
fn notify_mods(ctx: &AppContext, text: &str) {
    for u in ctx.user_pool.users() {
        if u.logged_in && (*u.level.read() as u8) >= ILevel::Moderator as u8 {
            let _ = u.print(&ctx.settings.bot_name, text);
        }
    }
}

/// `/shout <texto>` (paridad sb0t `Eval.Shout`, Messaging#0): grito visible
/// para toda la sala como texto del server "+n> [SHOUT] +t". Permitido para
/// nivel > Regular o con el flag `general`; los muzzleados no gritan.
fn handle_shout(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !(has_level(user, ILevel::Voice) || ctx.room_flags.get("general")) {
        return;
    }
    if user.is_muzzled() {
        return;
    }
    let text = args.trim();
    if text.is_empty() {
        send_system_line(ctx, user, "Usage: /shout <text>");
        return;
    }
    let name = user.name.read().clone();
    let line = ctx.templates.render("shout.line", &[("+n", &name), ("+t", text)]);
    ctx.broadcast_print(&line);
}

/// `/stats` (Mod, paridad sb0t `Eval.Stats`): bloque multilínea con las
/// métricas del server (Category.Stats; se omiten las que Astra no trackea:
/// language/hashlink/roomsearch size).
fn handle_stats(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let secs = ctx.uptime_secs();
    let uptime = format!("{}d {}h {}m", secs / 86400, (secs / 3600) % 24, (secs / 60) % 60);
    let quarantined = ctx
        .user_pool
        .users()
        .iter()
        .filter(|u| u.quarantined.load(std::sync::atomic::Ordering::Relaxed))
        .count();
    let lines = [
        format!("Stats for {}", ctx.settings.room_name),
        String::new(),
        format!("Uptime: {}", uptime),
        format!("Bytes received: {}", ctx.stats.bytes_in()),
        format!("Bytes sent: {}", ctx.stats.bytes_out()),
        format!("Invalid logins: {}", ctx.stats.invalid_logins()),
        format!("Flooded users: {}", ctx.stats.floods()),
        format!("Rejected users: {}", ctx.stats.rejections()),
        format!("Join count: {}", ctx.stats.total_users()),
        format!("Part count: {}", ctx.stats.parts()),
        format!("User count: {}", ctx.user_pool.len()),
        format!("Quarantined user count: {}", quarantined),
        format!("Peak user count: {}", ctx.stats.peak_users()),
        format!("Message count: {}", ctx.stats.messages()),
        format!("PM count: {}", ctx.stats.pms()),
    ];
    for line in lines {
        send_system_line(ctx, user, &line);
    }
}

fn handle_uptime(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let secs = ctx.uptime_secs();
    let (d, h, m, s) = (secs / 86400, (secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    send_system_line(
        ctx,
        user,
        &format!("Uptime: {}d {}h {}m {}s", d, h, m, s),
    );
    send_system_line(
        ctx,
        user,
        &format!(
            "Users: {} online, {} peak, {} total joins",
            ctx.user_pool.len(),
            ctx.stats.peak_users(),
            ctx.stats.total_users()
        ),
    );
}

fn handle_version(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    send_system_line(
        ctx,
        user,
        &format!("Astra v{}", env!("CARGO_PKG_VERSION")),
    );
}

fn handle_register(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Vec<astra_scripting::ScriptEvent> {
    if !ctx.settings.allow_registration {
        send_system_line(ctx, user, "Registration is disabled in this room.");
        return vec![];
    }

    let password = args.trim();
    if password.len() < 4 {
        send_system_line(ctx, user, "Usage: /register <password> (4+ chars)");
        return vec![];
    }

    match ctx.accounts.find_by_guid(&user.guid) {
        Ok(Some(_)) => {
            send_system_line(ctx, user, "Already registered. Use /unregister first.");
            return vec![];
        }
        Ok(None) => {}
        Err(_) => {
            send_system_line(ctx, user, "Registration failed (database error).");
            return vec![];
        }
    }

    let name = user.name.read().clone();
    let ip = user.external_ip.to_string();
    let live_level = (*user.level.read() as u8).max(ILevel::Regular as u8);
    match ctx.accounts.register(&name, &user.guid, password, live_level) {
        Ok(()) => {
            send_system_line(ctx, user, "Account registered. Use /login <password>.");
            vec![
                astra_scripting::ScriptEvent::Registering { name: name.clone(), ip: ip.clone() },
                astra_scripting::ScriptEvent::Registered { name, ip },
            ]
        }
        Err(_) => {
            send_system_line(ctx, user, "Registration failed (database error).");
            vec![]
        }
    }
}

/// `/whisper <nick> <text>`: envía un PM privado al target, apareciendo como
/// del emisor (paridad sb0t).
fn handle_whisper(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let target_name = parts.next().unwrap_or("").trim();
    let text = parts.next().unwrap_or("").trim();
    if target_name.is_empty() || text.is_empty() {
        send_system_line(ctx, user, "Usage: /whisper <nick> <text>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    let from = user.name.read().clone();
    let _ = target.send_pvt(&from, text);
    send_system_line(ctx, user, &format!("Whispered to '{}'.", target_name));
}

/// `/pmblock`: togglea el bloqueo de PMs entrantes del propio usuario.
fn handle_pmblock(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let now = !user.pm_blocked.load(std::sync::atomic::Ordering::Relaxed);
    user.pm_blocked.store(now, std::sync::atomic::Ordering::Relaxed);
    send_system_line(
        ctx,
        user,
        if now {
            "PM blocking ON. Regular users can no longer PM you."
        } else {
            "PM blocking OFF."
        },
    );
}

fn handle_unregister(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) -> Vec<astra_scripting::ScriptEvent> {
    match ctx.accounts.unregister(&user.guid) {
        Ok(true) => {
            send_system_line(ctx, user, "Account deleted.");
            vec![astra_scripting::ScriptEvent::Unregistered { name: user.name.read().clone() }]
        }
        Ok(false) => {
            send_system_line(ctx, user, "You are not registered.");
            vec![]
        }
        Err(_) => {
            send_system_line(ctx, user, "Unregister failed (database error).");
            vec![]
        }
    }
}

/// Retorna `true` si el nivel del usuario cambió (para AdminLevelChanged).
fn handle_login(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> bool {
    let password = args.trim();
    if password.is_empty() {
        send_system_line(ctx, user, "Usage: /login <password>");
        return false;
    }

    // 1. Owner password del astra.toml
    if !ctx.settings.owner_password.is_empty() && password == ctx.settings.owner_password {
        let changed = apply_level(ctx, user, user, ILevel::Owner, "Logged in as Owner.");
        if !changed {
            send_system_line(ctx, user, "Logged in as Owner (level unchanged).");
        }
        return changed;
    }

    // 2. Cuenta registrada (strict: nick + GUID + password)
    let name = user.name.read().clone();
    let account = match ctx.accounts.verify_strict(&name, &user.guid, password) {
        Ok(true) => ctx.accounts.find_by_guid(&user.guid).ok().flatten(),
        // 3. Fallback no-strict (modo sb0t): busca cuenta solo por password
        _ => ctx.accounts.find_by_password(password).ok().flatten(),
    };

    let Some(acc) = account else {
        send_system_line(ctx, user, "Invalid password.");
        return false;
    };

    let level = level_from_u8(acc.level);
    let changed = apply_level(ctx, user, user, level, &format!("Logged in (level {}).", acc.level));
    if !changed {
        send_system_line(ctx, user, "Logged in (level unchanged).");
    }
    changed
}

/// Auto-login por GUID (sb0t SecureLogin / AUTOLOGIN): si el GUID tiene una
/// cuenta registrada, restaura su nivel. Retorna `true` si el nivel cambió.
pub fn dispatch_autologin(ctx: &AppContext, user: &Arc<AresUser>) -> bool {
    if let Some(acc) = ctx.accounts.find_by_guid(&user.guid).ok().flatten() {
        let level = level_from_u8(acc.level);
        return apply_level(
            ctx,
            user,
            user,
            level,
            &format!("Auto-logged in (level {}).", acc.level),
        );
    }
    // Sin cuenta registrada: probar reconocimiento por IP+GUID (paridad
    // `Joined()` de sb0t, que también corre `AutoLogin.GetLevel` para
    // cualquier usuario, con o sin cuenta).
    if let Some(level) = ctx.ip_autologins.get_level(&user.guid, user.external_ip) {
        return apply_level(
            ctx,
            user,
            user,
            level,
            &format!("Auto-logged in via IP recognition (level {}).", level as u8),
        );
    }
    false
}

/// Retorna el nick del target si el nivel cambió.
fn handle_grant(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Option<String> {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_admin"));
        return None;
    }

    // El nivel es el último token; el nick (puede tener espacios) es el resto.
    let args = args.trim();
    let mut parts = args.rsplitn(2, char::is_whitespace);
    let level_str = parts.next().unwrap_or("");
    let target_name = parts.next().unwrap_or("").trim();
    let Some(new_level) = parse_level(level_str) else {
        send_system_line(
            ctx,
            user,
            "Usage: /grant <nick> <regular|voice|moderator|admin|owner>",
        );
        return None;
    };
    if target_name.is_empty() {
        send_system_line(
            ctx,
            user,
            "Usage: /grant <nick> <regular|voice|moderator|admin|owner>",
        );
        return None;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return None;
    };

    let own_level = *user.level.read() as u8;
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot modify a user of equal or higher level.");
        return None;
    }
    if new_level as u8 >= own_level {
        send_system_line(ctx, user, "You cannot grant a level equal or above your own.");
        return None;
    }

    let level_disp = format!("{} ({})", new_level as u8, level_name(new_level));
    let msg = ctx.templates.render("grant.target", &[("+l", &level_disp)]);
    if apply_level(ctx, user, &target, new_level, &msg) {
        send_system_line(
            ctx,
            user,
            &ctx.templates.render("grant.confirm", &[("+n", target_name), ("+l", &level_disp)]),
        );
        Some(target.name.read().clone())
    } else {
        None
    }
}

/// Retorna el nick del target si el nivel cambió.
fn handle_revoke(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Option<String> {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, &ctx.templates.get("error.access_admin"));
        return None;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /revoke <nick>");
        return None;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return None;
    };

    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot modify a user of equal or higher level.");
        return None;
    }

    let revoke_notice = ctx.templates.get("revoke.target");
    if apply_level(ctx, user, &target, ILevel::Regular, &revoke_notice) {
        send_system_line(ctx, user, &ctx.templates.render("revoke.confirm", &[("+n", target_name)]));
        Some(target.name.read().clone())
    } else {
        send_system_line(ctx, user, &format!("'{}' is already a regular user.", target_name));
        None
    }
}

/// `/addautologin <nick> <moderator|admin>` — otorga un nivel a un usuario
/// conectado Y lo recuerda por IP+GUID (paridad `AutoLogin.Add` de sb0t):
/// la próxima vez que se conecte desde (aprox.) la misma IP, el nivel se
/// restaura solo, sin necesidad de cuenta ni login. Nunca permite Owner
/// (paridad del rango `byte 1-3` de sb0t).
fn handle_addautologin(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let args = args.trim();
    let mut parts = args.rsplitn(2, char::is_whitespace);
    let level_str = parts.next().unwrap_or("");
    let target_name = parts.next().unwrap_or("").trim();
    // Escala de sb0t (`Eval.AddAutologin`): 1 = moderator, 2 = admin,
    // 3 = host. OJO: NO es la escala de `/grant` (donde "1" es regular y
    // "2" voice) — `#addautologin bob 1` en sb0t da MODERADOR.
    let level = match level_str {
        "1" => ILevel::Moderator,
        "2" => ILevel::Admin,
        "3" => ILevel::Owner,
        // Extra de Astra: aceptar también los nombres.
        other => match parse_level(other) {
            Some(l) if (l as u8) >= ILevel::Moderator as u8 => l,
            _ => {
                send_system_line(ctx, user, "Usage: /addautologin <nick> <1-3>  (1=moderator, 2=admin, 3=host)");
                return;
            }
        },
    };
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /addautologin <nick> <1-3>  (1=moderator, 2=admin, 3=host)");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return;
    };
    let name = target.name.read().clone();
    // El nivel se MUESTRA en la escala de sb0t (1-3), aunque internamente
    // Astra use 50/80/100.
    let sb0t_level = match level {
        ILevel::Owner => 3,
        ILevel::Admin => 2,
        _ => 1,
    };
    match ctx.ip_autologins.add(&target.guid, &name, level, target.external_ip) {
        Ok(()) => {
            let msg = format!(
                "Your level is now {} ({}), auto-restored on reconnect from this IP.",
                level as u8,
                level_name(level)
            );
            // Aplica el nivel AHORA (con el opchange + refresh de userlist).
            // `apply_level` retorna false si ya tenía ese nivel: en ese caso
            // igual hay que refrescarle el estado al cliente, porque el
            // autologin recién creado es lo que el admin quiere ver aplicado.
            if !apply_level(ctx, user, &target, level, &msg) {
                push_level_refresh(ctx, &target);
            }
            // Anuncio a la sala (AdminLogin#4 de sb0t).
            ctx.broadcast_print(&ctx.templates.render(
                "autologin.added",
                &[("+n", &name), ("+l", &sb0t_level.to_string())],
            ));
            send_system_line(
                ctx,
                user,
                &format!("'{}' added to IP autologin as {} ({}).", name, sb0t_level, level_name(level)),
            );
        }
        Err(e) => send_system_line(ctx, user, &format!("Failed: {}", e)),
    }
}

/// Reenvía al cliente su nivel actual: el paquete de "admin login"
/// (opchange) y un refresh de su entrada en la userlist de la sala. Se usa
/// cuando el nivel no cambió pero el cliente necesita verlo aplicado.
fn push_level_refresh(ctx: &AppContext, target: &Arc<AresUser>) {
    let level = *target.level.read();
    let _ = target.send(outbound::build_opchange(
        level as u8 >= ILevel::Moderator as u8,
    ));
    let name = target.name.read().clone();
    let vroom = *target.vroom.read();
    let level_str = (level as u8).to_string();
    let ws_msg = format!("UPDATE:{},{}:{}{}", name.encode_utf16().count(), level_str.len(), name, level_str);
    for u in ctx.user_pool.users() {
        if !u.logged_in
            || *u.vroom.read() != vroom
            || u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            let _ = tx.send(ws_msg.clone());
        } else {
            let _ = u.send(outbound::build_join_or_userlist_c(target, u.ares_crypto));
        }
    }
}

/// `/remautologin <id>` — elimina una entrada y degrada a Regular a
/// cualquier usuario conectado que matchee por GUID o IP (paridad
/// `AutoLogin.Remove` de sb0t, que escanea `Ares`/`Web` por igual).
fn handle_remautologin(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let Ok(id) = args.trim().parse::<i64>() else {
        send_system_line(ctx, user, "Usage: /remautologin <id>");
        return;
    };
    let Some((guid_hex_str, ip, entry_name)) = ctx.ip_autologins.remove(id) else {
        send_system_line(ctx, user, "No autologin entry with that id.");
        return;
    };
    for u in ctx.user_pool.users() {
        if !u.logged_in {
            continue;
        }
        if guid_to_hex(&u.guid) == guid_hex_str || u.external_ip == ip {
            apply_level(
                ctx,
                user,
                &u,
                ILevel::Regular,
                "Your auto-login entry was removed; you are now a regular user.",
            );
        }
    }
    // Anuncio a la sala (AdminLogin#5 de sb0t).
    ctx.broadcast_print(
        &ctx.templates
            .render("autologin.removed", &[("+n", &entry_name)]),
    );
    send_system_line(ctx, user, "Autologin entry removed.");
}

/// `/autologins` — lista las entradas de auto-nivel por IP.
fn handle_ip_autologins(ctx: &AppContext, user: &Arc<AresUser>) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let list = ctx.ip_autologins.list();
    if list.is_empty() {
        send_system_line(ctx, user, "No IP autologin entries.");
        return;
    }
    for (id, name, ip, level) in list {
        send_system_line(ctx, user, &format!("{} - {} [{}] [{}]", id, name, ip, level_name(level)));
    }
}

/// `/cmdlevel [name] [level|reset]` — ver o configurar el nivel mínimo
/// requerido por un comando gestionado. Equivalente a la GUI de
/// `gui/CommandManager.cs` de sb0t (registro de Windows); Astra no tiene
/// GUI, así que esto se expone como comando in-room, gateado a Owner porque
/// permite reconfigurar los demás gates (evita auto-escalado por un Admin).
fn handle_cmdlevel(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();

    if name.is_empty() {
        send_system_line(ctx, user, "Usage: /cmdlevel <command> [level|reset]");
        let overridden: Vec<String> = ctx
            .command_levels
            .list()
            .into_iter()
            .filter(|(_, _, is_override)| *is_override)
            .map(|(cmd, level, _)| format!("{}={}", cmd, level_name(level)))
            .collect();
        if overridden.is_empty() {
            send_system_line(ctx, user, "No command levels are overridden (all at default).");
        } else {
            send_system_line(ctx, user, &format!("Overridden: {}", overridden.join(", ")));
        }
        return;
    }

    if !server_core::CommandLevelManager::is_managed(&name) {
        send_system_line(ctx, user, &format!("'{}' is not a managed command.", name));
        return;
    }

    if rest.is_empty() {
        let current = ctx.command_levels.get(&name).unwrap_or(ILevel::Regular);
        let default = server_core::CommandLevelManager::default_level(&name).unwrap_or(ILevel::Regular);
        if current == default {
            send_system_line(ctx, user, &format!("/{}: {} (default).", name, level_name(current)));
        } else {
            send_system_line(
                ctx,
                user,
                &format!("/{}: {} (default {}).", name, level_name(current), level_name(default)),
            );
        }
        return;
    }

    if rest.eq_ignore_ascii_case("reset") {
        if ctx.command_levels.reset(&name) {
            send_system_line(ctx, user, &format!("/{} reset to its default level.", name));
        } else {
            send_system_line(ctx, user, &format!("/{} was already at its default.", name));
        }
        return;
    }

    let Some(level) = parse_level(rest) else {
        send_system_line(
            ctx,
            user,
            "Usage: /cmdlevel <command> <regular|voice|moderator|admin|owner|reset>",
        );
        return;
    };
    ctx.command_levels.set(&name, level);
    send_system_line(ctx, user, &format!("/{} now requires {}+.", name, level_name(level)));
}

/// Aplica un nivel a `target`: actualiza el nivel en vivo, persiste en la
/// cuenta si existe, envía OpChange y notifica. Retorna `true` si cambió.
fn apply_level(
    ctx: &AppContext,
    _issuer: &Arc<AresUser>,
    target: &Arc<AresUser>,
    new_level: ILevel,
    notice: &str,
) -> bool {
    {
        let mut level = target.level.write();
        if *level == new_level {
            return false;
        }
        *level = new_level;
    }

    // Persistir en la cuenta registrada si existe.
    if let Ok(Some(_)) = ctx.accounts.find_by_guid(&target.guid) {
        let _ = ctx.accounts.set_level(&target.guid, new_level as u8);
    }

    let _ = target.send(outbound::build_opchange(
        new_level as u8 >= ILevel::Moderator as u8,
    ));
    send_system_line(ctx, target, notice);

    // Difunde el nuevo nivel a todos en la misma vroom: a los clientes web
    // como UPDATE (paridad ib0tClient.Level setter -> WebOutbound.UpdateTo,
    // así el userlist/badge de todos se refresca en vivo) y a los clientes
    // Ares TCP como un refresh de join/userlist (paridad UpdateUserStatus).
    let level_byte = new_level as u8;
    let name = target.name.read().clone();
    let vroom = *target.vroom.read();
    let level_str = level_byte.to_string();
    let ws_msg = format!("UPDATE:{},{}:{}{}", name.encode_utf16().count(), level_str.len(), name, level_str);
    for u in ctx.user_pool.users() {
        if !u.logged_in
            || *u.vroom.read() != vroom
            || u.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if let Some(tx) = &u.ws_text_sender {
            let _ = tx.send(ws_msg.clone());
        } else {
            let _ = u.send(outbound::build_join_or_userlist_c(target, u.ares_crypto));
        }
    }

    ctx.publish_link_event(server_core::LinkEvent::UserUpdated {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(target),
    });
    true
}

fn parse_level(s: &str) -> Option<ILevel> {
    match s.to_ascii_lowercase().as_str() {
        "regular" | "user" | "1" => Some(ILevel::Regular),
        "voice" | "2" => Some(ILevel::Voice),
        "moderator" | "mod" | "50" => Some(ILevel::Moderator),
        "admin" | "administrator" | "80" => Some(ILevel::Admin),
        "owner" | "100" => Some(ILevel::Owner),
        _ => None,
    }
}

fn level_from_u8(level: u8) -> ILevel {
    match level {
        l if l >= ILevel::Owner as u8 => ILevel::Owner,
        l if l >= ILevel::Admin as u8 => ILevel::Admin,
        l if l >= ILevel::Moderator as u8 => ILevel::Moderator,
        l if l >= ILevel::Voice as u8 => ILevel::Voice,
        _ => ILevel::Regular,
    }
}

fn level_name(level: ILevel) -> &'static str {
    match level {
        ILevel::Anonymous => "anonymous",
        ILevel::Regular => "regular",
        ILevel::Voice => "voice",
        ILevel::Moderator => "moderator",
        ILevel::Admin => "admin",
        ILevel::Owner => "owner",
        ILevel::System => "system",
    }
}

fn has_level(user: &AresUser, min: ILevel) -> bool {
    // Todo usuario conectado se trata como Regular como mínimo aunque su
    // `level` en memoria siga en `Anonymous` (el valor default de
    // `AresUser::new`, paridad de "sin nivel asignado aún"): de lo
    // contrario, los comandos gateados a Regular (la mayoría de los
    // comandos de autoservicio) quedarían inaccesibles para cualquier
    // usuario que no haya recibido explícitamente un nivel.
    let level = (*user.level.read() as u8).max(ILevel::Regular as u8);
    level >= min as u8
}

/// Texto de rechazo para el gate centralizado por comando, en el mismo
/// estilo que los mensajes que ya usaban los handlers individuales.
fn access_denied_text(required: ILevel) -> &'static str {
    match required {
        ILevel::Owner => "Access denied. Owner required.",
        ILevel::Admin => "Access denied. Admin+ required.",
        ILevel::Moderator => "Access denied. Moderator+ required.",
        ILevel::Voice => "Access denied. Voice+ required.",
        _ => "Access denied.",
    }
}

/// Propaga una acción `host*` a los servidores enlazados (si hay link activo).
/// Cada servidor la aplica a su pool local vía `apply_admin_action`. Notifica
/// al emisor que la acción viaja por la red (el target puede no estar acá).
fn publish_host_action(ctx: &AppContext, user: &Arc<AresUser>, kind: u8, target: &str) {
    let target = target.trim();
    if target.is_empty() || ctx.link_receiver_count() == 0 {
        return;
    }
    ctx.publish_link_event(server_core::LinkEvent::AdminAction {
        origin: None,
        kind,
        target: target.to_string(),
    });
    send_system_line(ctx, user, "Host action propagated to linked servers.");
}

/// Gate para los comandos `host*`: en sb0t es nivel Host (dueño de la red);
/// en Astra mapea a Owner. Notifica si el usuario no califica.
fn require_host(ctx: &AppContext, user: &Arc<AresUser>) -> bool {
    if has_level(user, ILevel::Owner) {
        true
    } else {
        send_system_line(ctx, user, "Access denied. Host (Owner) required.");
        false
    }
}

/// `/hostcban`: limpia TODO — bans, range bans, muzzles y efectos de texto de
/// todos los usuarios (paridad `HostCBans` de sb0t).
fn handle_hostcban(ctx: &AppContext, user: &Arc<AresUser>) {
    use std::sync::atomic::Ordering::Relaxed;
    let bans = ctx.bans.clear_all();
    let ranges = ctx.range_bans.clear();
    let mut cleared_users = 0usize;
    for u in ctx.user_pool.users() {
        let had = u.muzzled.swap(false, Relaxed)
            | u.kiddied.swap(false, Relaxed)
            | u.lowered.swap(false, Relaxed)
            | u.kewl.swap(false, Relaxed)
            | u.painted.swap(false, Relaxed);
        if u.echo_text.read().is_some() {
            *u.echo_text.write() = None;
        }
        if had {
            cleared_users += 1;
        }
    }
    send_system_line(
        ctx,
        user,
        &format!(
            "Cleared {} ban(s), {} range ban(s), and effects on {} user(s).",
            bans, ranges, cleared_users
        ),
    );
}

fn outranks(issuer: &AresUser, target: &AresUser) -> bool {
    (*issuer.level.read() as u8) > (*target.level.read() as u8)
}

// ============================================================================
// Greets (mensajes de bienvenida) — requiere Admin+
// ============================================================================

fn handle_greets(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.greets.set_enabled(true);
            send_system_line(ctx, user, "Greets enabled.");
        }
        "off" => {
            ctx.greets.set_enabled(false);
            send_system_line(ctx, user, "Greets disabled.");
        }
        "" => {
            let state = if ctx.greets.is_enabled() { "on" } else { "off" };
            send_system_line(
                ctx,
                user,
                &format!("Greets are {} ({} configured).", state, ctx.greets.len()),
            );
        }
        _ => send_system_line(ctx, user, "Usage: /greets [on|off]"),
    }
}

fn handle_addgreet(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let template = args.trim();
    if template.is_empty() {
        send_system_line(ctx, user, "Usage: /addgreet <text>  (placeholders: +n +ip +id +f +v +uc +rn +ut +l)");
        return;
    }
    let id = ctx.greets.add(template);
    if id != 0 {
        send_system_line(ctx, user, &format!("Greet #{} added.", ctx.greets.len() - 1));
    } else {
        send_system_line(ctx, user, "Failed to persist greet.");
    }
}

fn handle_remgreet(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Ok(index) = args.trim().parse::<usize>() else {
        send_system_line(ctx, user, "Usage: /remgreet <index>");
        return;
    };
    match ctx.greets.remove_at(index) {
        Some(t) => send_system_line(ctx, user, &format!("Removed greet: {}", t)),
        None => send_system_line(ctx, user, "No greet at that index."),
    }
}

fn handle_listgreets(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let greets = ctx.greets.list();
    if greets.is_empty() {
        send_system_line(ctx, user, "No greets configured.");
        return;
    }
    send_system_line(ctx, user, &format!("Greets ({}):", greets.len()));
    for (i, g) in greets.iter().enumerate() {
        send_system_line(ctx, user, &format!("{} - {}", i, g));
    }
}

// ============================================================================
// Word filters — requiere Admin+
// ============================================================================

fn handle_addfilter(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let args = args.trim();
    if args.is_empty() {
        send_system_line(ctx, user, "Usage: /addfilter <word> [block|kick|ban|announce]");
        return;
    }
    // El último token puede ser la acción; el resto es el patrón.
    let (pattern, action) = match args.rsplit_once(char::is_whitespace) {
        Some((p, last))
            if matches!(last.to_ascii_lowercase().as_str(), "block" | "kick" | "ban" | "announce") =>
        {
            (p.trim(), FilterAction::parse(last))
        }
        _ => (args, FilterAction::Block),
    };
    if pattern.is_empty() {
        send_system_line(ctx, user, "Usage: /addfilter <word> [block|kick|ban|announce]");
        return;
    }
    ctx.word_filter.add(pattern, action);
    let extra = if action == FilterAction::Announce {
        " Use /addline to add response lines."
    } else {
        ""
    };
    send_system_line(
        ctx,
        user,
        &format!(
            "Filter '{}' → {} added.{}",
            pattern.to_ascii_lowercase(),
            action.as_str(),
            extra
        ),
    );
}

fn handle_remfilter(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let pattern = args.trim();
    if pattern.is_empty() {
        send_system_line(ctx, user, "Usage: /remfilter <word>");
        return;
    }
    if ctx.word_filter.remove(pattern) {
        send_system_line(ctx, user, &format!("Filter '{}' removed.", pattern.to_ascii_lowercase()));
    } else {
        send_system_line(ctx, user, "No matching filter.");
    }
}

fn handle_listfilters(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let filters = ctx.word_filter.list();
    if filters.is_empty() {
        send_system_line(ctx, user, "No word filters configured.");
        return;
    }
    send_system_line(ctx, user, &format!("Word filters ({}):", filters.len()));
    for (i, (pattern, action)) in filters.iter().enumerate() {
        // El índice acá es el que usan /addline, /remline y /viewfilter.
        send_system_line(ctx, user, &format!("{} - {} → {}", i, pattern, action.as_str()));
    }
}

/// `/addline <índice>, <texto>` — agrega una línea de respuesta a un
/// filtro `announce` existente, referenciado por su índice en
/// `/listfilters` (paridad `WordFilter.AddLine` de sb0t).
fn handle_addline(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Some((index_str, text)) = args.split_once(',') else {
        send_system_line(ctx, user, "Usage: /addline <index>, <text>");
        return;
    };
    let Ok(index) = index_str.trim().parse::<usize>() else {
        send_system_line(ctx, user, "Usage: /addline <index>, <text>");
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        send_system_line(ctx, user, "Usage: /addline <index>, <text>");
        return;
    }
    let filters = ctx.word_filter.list();
    let Some((pattern, _)) = filters.get(index) else {
        send_system_line(ctx, user, "No filter at that index.");
        return;
    };
    match ctx.word_filter.add_line(pattern, text) {
        Ok(()) => send_system_line(ctx, user, &format!("Line added to filter '{}'.", pattern)),
        Err(e) => send_system_line(ctx, user, &format!("Failed: {}", e)),
    }
}

/// `/remline <índice>, <línea>` — elimina una línea de respuesta; si era
/// la última, borra el filtro entero (paridad `WordFilter.RemLine`).
fn handle_remline(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Some((index_str, line_str)) = args.split_once(',') else {
        send_system_line(ctx, user, "Usage: /remline <index>, <line>");
        return;
    };
    let (Ok(index), Ok(line_index)) = (
        index_str.trim().parse::<usize>(),
        line_str.trim().parse::<usize>(),
    ) else {
        send_system_line(ctx, user, "Usage: /remline <index>, <line>");
        return;
    };
    let filters = ctx.word_filter.list();
    let Some((pattern, _)) = filters.get(index) else {
        send_system_line(ctx, user, "No filter at that index.");
        return;
    };
    let pattern = pattern.clone();
    match ctx.word_filter.remove_line(&pattern, line_index) {
        server_core::RemoveLineResult::LineRemoved => {
            send_system_line(ctx, user, &format!("Line removed from filter '{}'.", pattern));
        }
        server_core::RemoveLineResult::FilterRemoved => {
            send_system_line(ctx, user, &format!("Filter '{}' removed (last line).", pattern));
        }
        server_core::RemoveLineResult::NotFound => {
            send_system_line(ctx, user, "No such filter or line index.");
        }
    }
}

/// `/viewfilter <índice>` — muestra las líneas de un filtro `announce`.
fn handle_viewfilter(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Ok(index) = args.trim().parse::<usize>() else {
        send_system_line(ctx, user, "Usage: /viewfilter <index>");
        return;
    };
    let filters = ctx.word_filter.list();
    let Some((pattern, _)) = filters.get(index) else {
        send_system_line(ctx, user, "No filter at that index.");
        return;
    };
    match ctx.word_filter.view(pattern) {
        Some(lines) if !lines.is_empty() => {
            for (i, line) in lines.iter().enumerate() {
                send_system_line(ctx, user, &format!("line {}: {}", i, line));
            }
        }
        Some(_) => send_system_line(ctx, user, "This filter has no lines yet."),
        None => send_system_line(ctx, user, "This filter is not an announce-type filter."),
    }
}

// ============================================================================
// URLs rotadas de la sala — requiere Admin+
// ============================================================================

fn handle_url(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.urls.set_enabled(true);
            send_system_line(ctx, user, "Room URLs enabled.");
        }
        "off" => {
            ctx.urls.set_enabled(false);
            send_system_line(ctx, user, "Room URLs disabled.");
        }
        "" => {
            let state = if ctx.urls.is_enabled() { "on" } else { "off" };
            send_system_line(
                ctx,
                user,
                &format!("Room URLs are {} ({} configured).", state, ctx.urls.len()),
            );
        }
        _ => send_system_line(ctx, user, "Usage: /url [on|off]"),
    }
}

fn handle_addurl(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    // Formato: <address> <text...>  (address es el primer token)
    let args = args.trim();
    let Some((address, text)) = args.split_once(char::is_whitespace) else {
        send_system_line(ctx, user, "Usage: /addurl <address> <text>");
        return;
    };
    let address = address.trim();
    let text = text.trim();
    if address.is_empty() || text.is_empty() {
        send_system_line(ctx, user, "Usage: /addurl <address> <text>");
        return;
    }
    let id = ctx.urls.add(address, text);
    if id != 0 {
        send_system_line(ctx, user, &format!("URL #{} added.", ctx.urls.len() - 1));
    } else {
        send_system_line(ctx, user, "Failed to persist URL.");
    }
}

fn handle_remurl(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Ok(index) = args.trim().parse::<usize>() else {
        send_system_line(ctx, user, "Usage: /remurl <index>");
        return;
    };
    match ctx.urls.remove_at(index) {
        Some(item) => send_system_line(ctx, user, &format!("Removed URL: {}", item.text)),
        None => send_system_line(ctx, user, "No URL at that index."),
    }
}

fn handle_listurl(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let urls = ctx.urls.list();
    if urls.is_empty() {
        send_system_line(ctx, user, "No room URLs configured.");
        return;
    }
    send_system_line(ctx, user, &format!("Room URLs ({}):", urls.len()));
    for (i, u) in urls.iter().enumerate() {
        send_system_line(ctx, user, &format!("{} - {} [{}]", i, u.text, u.address));
    }
}

// ============================================================================
// Historial e información de sala/usuario
// ============================================================================

/// `/idle` y `/idles` (paridad sb0t):
/// - Sin args: marcarse ausente (core/Events.cs:537). Cooldown de 5 min
///   (`CheckIfCanIdle`); si no puede, se ignora en silencio como sb0t.
/// - `/idle on|off` (Host): toggle de los ANUNCIOS de idle/unidle — flag de
///   sala `idle`, sb0t `Settings.IdleMonitoring` (`Eval.cs:1411`).
fn handle_idle(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    cmd: &str,
    args: &str,
) -> Vec<astra_scripting::ScriptEvent> {
    let a = args.trim().to_ascii_lowercase();
    match a.as_str() {
        "" => {
            if ctx.mark_user_idle(user) {
                vec![astra_scripting::ScriptEvent::Idled {
                    name: user.name.read().clone(),
                }]
            } else {
                vec![]
            }
        }
        "on" | "off" if cmd == "idle" => {
            if !has_level(user, ILevel::Owner) {
                send_system_line(ctx, user, "Access denied. Host required.");
                return vec![];
            }
            let v = a == "on";
            ctx.room_flags.set("idle", v);
            send_system_line(
                ctx,
                user,
                &format!("Room flag 'idle' {}.", if v { "enabled" } else { "disabled" }),
            );
            vec![]
        }
        // `idles <algo>` / args inválidos: sb0t no hace nada (el emote
        // `#me idles ...` es el camino con argumentos).
        _ => vec![],
    }
}

/// `/history on|off` — toggle del replay de historial al entrar (paridad
/// sb0t `Eval.History`, Host: `Settings.History`). El replay en sí lo hace
/// `AppContext::replay_history` en el join. El nivel lo gatea el
/// `CommandLevelManager` en la entrada del dispatcher.
fn handle_history(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.room_flags.set("history", true);
            send_system_line(ctx, user, "Room flag 'history' enabled.");
        }
        "off" => {
            ctx.room_flags.set("history", false);
            send_system_line(ctx, user, "Room flag 'history' disabled.");
        }
        "" => {
            let state = if ctx.room_flags.get("history") { "on" } else { "off" };
            send_system_line(ctx, user, &format!("Room flag 'history' is {}.", state));
        }
        _ => send_system_line(ctx, user, "Usage: /history [on|off]"),
    }
}

fn handle_whowas(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let query = args.trim();
    if query.is_empty() {
        send_system_line(ctx, user, "Usage: /whowas <nick|ip>");
        return;
    }
    // Paridad sb0t Whowas.Query: hasta 50 resultados, formato WhoWas#0
    // "whowas: +n +ip +v +t" con fecha absoluta.
    let results = ctx.db.search_user_history(query, 50).unwrap_or_default();
    if results.is_empty() {
        send_system_line(
            ctx,
            user,
            &ctx.templates.render("whowas.none", &[("+n", query)]),
        );
        return;
    }
    for (name, version, ip, last_seen) in &results {
        // last_seen está en milisegundos.
        let when = chrono::DateTime::from_timestamp(*last_seen / 1000, 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        send_system_line(
            ctx,
            user,
            &ctx.templates.render(
                "whowas.entry",
                &[("+n", name), ("+ip", ip), ("+v", version), ("+t", &when)],
            ),
        );
    }
}

/// `/lastseen on|off` (Host, paridad sb0t `Eval.LastSeen`): toggle del
/// anuncio "was last seen as..." al entrar un usuario. Con un nick/IP como
/// arg, consulta el historial (extra de Astra, sb0t no tiene la consulta).
fn handle_lastseen(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let query = args.trim();
    match query.to_ascii_lowercase().as_str() {
        "on" => {
            ctx.room_flags.set("lastseen", true);
            send_system_line(ctx, user, "Room flag 'lastseen' enabled.");
            return;
        }
        "off" => {
            ctx.room_flags.set("lastseen", false);
            send_system_line(ctx, user, "Room flag 'lastseen' disabled.");
            return;
        }
        _ => {}
    }
    if query.is_empty() {
        send_system_line(ctx, user, "Usage: /lastseen <on|off|nick|ip>");
        return;
    }
    // Si está online, reportar "online now".
    if let Some(target) = ctx.user_pool.get_by_name(query) {
        send_system_line(ctx, user, &format!("'{}' is online now.", target.name.read()));
        return;
    }
    let results = ctx.db.search_user_history(query, 1).unwrap_or_default();
    match results.first() {
        Some((name, _, ip, last_seen)) => send_system_line(
            ctx,
            user,
            &format!("'{}' [{}] last seen {}", name, ip, format_time_ago(*last_seen)),
        ),
        None => send_system_line(ctx, user, "No matching history."),
    }
}

/// `/roominfo on|off` (Host, paridad sb0t `Eval.RoomInfo`): toggle del
/// broadcast periódico (20 min) del bloque de info de sala. Sin args,
/// muestra el bloque al solicitante (extra de Astra).
fn handle_roominfo(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.room_flags.set("roominfo", true);
            send_system_line(ctx, user, "Room flag 'roominfo' enabled.");
            return;
        }
        "off" => {
            ctx.room_flags.set("roominfo", false);
            send_system_line(ctx, user, "Room flag 'roominfo' disabled.");
            return;
        }
        _ => {}
    }
    for line in roominfo_lines(ctx) {
        send_system_line(ctx, user, &line);
    }
}

/// Bloque de info de sala (usado por `/roominfo` y por el broadcast
/// periódico de la task en `main.rs` cuando el flag `roominfo` está on).
pub fn roominfo_lines(ctx: &AppContext) -> Vec<String> {
    let users = ctx.user_pool.users();
    let total = users.iter().filter(|u| u.logged_in).count();
    let ops = users
        .iter()
        .filter(|u| u.logged_in && (*u.level.read() as u8) > ILevel::Regular as u8)
        .count();
    let owners = users
        .iter()
        .filter(|u| u.logged_in && (*u.level.read() as u8) >= ILevel::Owner as u8)
        .count();

    // Textos de sb0t (Category.RoomInfo #0-5).
    let secs = ctx.uptime_secs();
    let uptime = format!("{}d {}h {}m", secs / 86400, (secs / 3600) % 24, (secs / 60) % 60);
    vec![
        "Room Information".to_string(),
        String::new(),
        format!("Current hosts: {}", owners),
        format!("Current user count: {}", total),
        format!("Current admin count: {}", ops),
        format!("Server uptime: {}", uptime),
        format!("Host status: {}", ctx.room_status()),
    ]
}

fn handle_status(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let text = args.trim();
    if text.is_empty() {
        let status = ctx.room_status();
        if status.is_empty() {
            send_system_line(ctx, user, "Room status is not set.");
        } else {
            send_system_line(ctx, user, &format!("Status: {}", status));
        }
        return;
    }
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let next = if text == "-" { String::new() } else { truncate_text(text, 200) };
    ctx.set_room_status(next.clone());
    if next.is_empty() {
        send_system_line(ctx, user, "Room status cleared.");
    } else {
        send_system_line(ctx, user, &format!("Room status set to '{}'.", next));
    }
    // Anuncio a la sala (RoomInfo#6 de sb0t), stealth-aware.
    let by = if ctx.room_flags.get("stealth") {
        ctx.settings.room_name.clone()
    } else {
        user.name.read().clone()
    };
    ctx.broadcast_print(&ctx.templates.render("status.updated", &[("+n", &by)]));
}

fn handle_id(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let target_name = args.trim();
    if target_name.is_empty() {
        // Paridad sb0t Eval.ID: "Nick: id" del propio usuario.
        let name = user.name.read().clone();
        send_system_line(ctx, user, &format!("{}: {}", name, user.id));
        return;
    }
    match ctx.user_pool.get_by_name(target_name) {
        Some(t) => send_system_line(ctx, user, &format!("'{}' has id {}.", t.name.read(), t.id)),
        None => send_system_line(ctx, user, "User not found."),
    }
}

/// `/info` — listado de TODOS los usuarios conectados con nombre, vroom e
/// id (paridad sb0t `Eval.Info`): encabezado con el nombre de la sala y una
/// línea por usuario, excluyendo cloaked e incluyendo clientes web. (Cuando
/// exista link, se repetirá el listado por cada leaf.)
fn handle_info(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    send_system_line(ctx, user, &ctx.settings.room_name);
    send_system_line(ctx, user, "");
    for u in ctx.user_pool.users() {
        if !u.logged_in || u.cloaked.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let name = u.name.read().clone();
        let vroom = u.vroom.read().to_string();
        let id = u.id.to_string();
        let line = ctx.templates.render(
            "info.user",
            &[("+n", &name), ("+v", &vroom), ("+i", &id)],
        );
        send_system_line(ctx, user, &line);
    }
}

fn handle_customnames(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    // Paridad sb0t Eval.CustomNames: `on|off` togglea si la sala permite
    // custom names (CustomNamesEnabled). Sin args, Astra además lista los
    // custom names activos (extra).
    match _args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.room_flags.set("customnames", true);
            send_system_line(ctx, user, "Custom names enabled.");
            return;
        }
        "off" => {
            ctx.room_flags.set("customnames", false);
            send_system_line(ctx, user, "Custom names disabled.");
            return;
        }
        _ => {}
    }
    let mut found = false;
    for u in ctx.user_pool.users() {
        if !u.logged_in {
            continue;
        }
        if let Some(cname) = u.custom_name.read().clone() {
            found = true;
            send_system_line(ctx, user, &format!("{} → {}", u.name.read(), cname));
        }
    }
    if !found {
        send_system_line(ctx, user, "No users have a custom name set.");
    }
}

// ============================================================================
// Bans avanzados (range / ASN / clear / stats) — requiere Admin+
// ============================================================================

fn handle_rangeban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let prefix = args.trim();
    if prefix.is_empty() {
        send_system_line(ctx, user, "Usage: /rangeban <ip-prefix>");
        return;
    }
    if ctx.range_bans.add(prefix) {
        send_system_line(ctx, user, &format!("Range ban added: {}", prefix.replace('*', "")));
    } else {
        send_system_line(ctx, user, "Range ban already exists (or invalid).");
    }
}

fn handle_rangeunban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let arg = args.trim();
    if arg.is_empty() {
        send_system_line(ctx, user, "Usage: /rangeunban <ip-prefix|index>");
        return;
    }
    // Puede ser índice o prefijo literal.
    let removed = if let Ok(index) = arg.parse::<usize>() {
        ctx.range_bans.remove_at(index).is_some()
    } else {
        ctx.range_bans.remove(arg)
    };
    if removed {
        send_system_line(ctx, user, "Range ban removed.");
    } else {
        send_system_line(ctx, user, "No matching range ban.");
    }
}

fn handle_listrangebans(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let list = ctx.range_bans.list();
    if list.is_empty() {
        send_system_line(ctx, user, "No range bans.");
        return;
    }
    send_system_line(ctx, user, &format!("Range bans ({}):", list.len()));
    for (i, p) in list.iter().enumerate() {
        send_system_line(ctx, user, &format!("{} - {}", i, p));
    }
}

fn handle_asnban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Ok(asn) = args.trim().parse::<u32>() else {
        send_system_line(ctx, user, "Usage: /asnban <asn>");
        return;
    };
    if ctx.asn_bans.add(asn) {
        send_system_line(ctx, user, &format!("ASN {} banned.", asn));
    } else {
        send_system_line(ctx, user, "ASN already banned (or invalid).");
    }
}

fn handle_asnunban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let Ok(asn) = args.trim().parse::<u32>() else {
        send_system_line(ctx, user, "Usage: /asnunban <asn>");
        return;
    };
    if ctx.asn_bans.remove(asn) {
        send_system_line(ctx, user, &format!("ASN {} unbanned.", asn));
    } else {
        send_system_line(ctx, user, "ASN not banned.");
    }
}

fn handle_listasnbans(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let list = ctx.asn_bans.list();
    if list.is_empty() {
        send_system_line(ctx, user, "No ASN bans.");
        return;
    }
    let joined = list.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
    send_system_line(ctx, user, &format!("ASN bans ({}): {}", list.len(), joined));
}

fn handle_clearbans(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let n = ctx.bans.clear_all();
    send_system_line(ctx, user, &format!("Cleared {} ban(s).", n));
    if n > 0 {
        announce_admin_action(ctx, user, "adminaction.cbans", "");
    }
}

fn handle_banstats(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let bans = ctx.recent_bans(20);
    send_system_line(
        ctx,
        user,
        &format!("Active bans: {} | recent actions: {}", ctx.bans.len(), bans.len()),
    );
    for (banner, target, ip) in &bans {
        send_system_line(ctx, user, &format!("{} banned {} [{}]", banner, target, ip));
    }
}

// ============================================================================
// Moderación extra (Tanda 4)
// ============================================================================

fn handle_move(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let mut parts = args.trim().rsplitn(2, char::is_whitespace);
    let vroom_str = parts.next().unwrap_or("");
    let target_name = parts.next().unwrap_or("").trim();
    let (Ok(new_vroom), false) = (vroom_str.parse::<u16>(), target_name.is_empty()) else {
        send_system_line(ctx, user, "Usage: /move <nick> <vroom>");
        return;
    };
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot move a user of equal or higher level.");
        return;
    }

    let old_vroom = *target.vroom.read();
    if old_vroom == new_vroom {
        send_system_line(ctx, user, "User is already in that vroom.");
        return;
    }
    if ctx.vrooms.get(new_vroom).is_none() {
        let _ = ctx.vrooms.create(new_vroom, None, None);
    }

    // Part del vroom viejo + join al nuevo (mismo patrón que /vroom).
    let mut part_user = AresUser::new(target.id, target.external_ip, target.guid);
    part_user.logged_in = true;
    *part_user.name.write() = target.name.read().clone();
    *part_user.vroom.write() = old_vroom;
    *target.vroom.write() = new_vroom;

    for other in ctx.user_pool.users() {
        if !other.logged_in || other.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let ov = *other.vroom.read();
        if ov == old_vroom {
            let _ = other.send(outbound::build_part_c(&part_user, other.ares_crypto));
        }
        if ov == new_vroom {
            let _ = other.send(outbound::build_join_or_userlist_c(&target, other.ares_crypto));
        }
    }
    ctx.publish_link_event(server_core::LinkEvent::VroomChanged {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(&target),
    });
    send_system_line(ctx, &target, &format!("You were moved to vroom {}.", new_vroom));
    send_system_line(ctx, user, &format!("Moved '{}' to vroom {}.", target_name, new_vroom));
    // Aviso a mods (Notification#16 de sb0t).
    let issuer_name = user.name.read().clone();
    let tname = target.name.read().clone();
    notify_mods(
        ctx,
        &ctx.templates.render(
            "move.by",
            &[("+n", &tname), ("+a", &issuer_name), ("+v", &new_vroom.to_string())],
        ),
    );
}

fn handle_changename(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let target_name = parts.next().unwrap_or("").trim();
    let new_name = parts.next().unwrap_or("").trim();
    if target_name.is_empty() || new_name.is_empty() {
        send_system_line(ctx, user, "Usage: /changename <nick> <newname>");
        return;
    }
    if new_name.chars().count() > 30 {
        send_system_line(ctx, user, "New name too long.");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if ctx.user_pool.get_by_name(new_name).is_some() {
        send_system_line(ctx, user, "That name is already in use.");
        return;
    }
    let old_name = target.name.read().clone();
    *target.name.write() = new_name.to_string();
    ctx.user_pool.rename(target.id, &old_name, new_name);

    // Part con el nombre viejo + join con el nuevo (refresh en clientes).
    let mut part_user = AresUser::new(target.id, target.external_ip, target.guid);
    part_user.logged_in = true;
    *part_user.name.write() = old_name.clone();
    for other in ctx.user_pool.users() {
        if other.logged_in
            && *other.vroom.read() == *target.vroom.read()
            && !other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            let _ = other.send(outbound::build_part_c(&part_user, other.ares_crypto));
            let _ = other.send(outbound::build_join_or_userlist_c(&target, other.ares_crypto));
        }
    }
    ctx.publish_link_event(server_core::LinkEvent::NickChanged {
        origin: None,
        old_name: old_name.clone(),
        user: server_core::LinkUserSnapshot::from_user(&target),
    });
    send_system_line(ctx, user, &format!("Renamed '{}' to '{}'.", old_name, new_name));
}

fn handle_oldname(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /oldname <nick>");
        return;
    }
    match ctx.user_pool.get_by_name(target_name) {
        Some(t) => {
            let org = t.org_name.read().clone();
            let org = if org.is_empty() { t.name.read().clone() } else { org };
            send_system_line(ctx, user, &format!("'{}' original name: {}", target_name, org));
        }
        None => send_system_line(ctx, user, "User not found."),
    }
}

fn handle_changemessage(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let target_name = parts.next().unwrap_or("").trim();
    let text = parts.next().unwrap_or("").trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /changemessage <nick> <text>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot modify a user of equal or higher level.");
        return;
    }
    *target.personal_message.lock() = text.to_string();
    ctx.publish_link_event(server_core::LinkEvent::PersonalMessage {
        origin: None,
        name: target.name.read().clone(),
        text: text.to_string(),
    });
    send_system_line(ctx, user, &format!("Set personal message for '{}'.", target_name));
}

/// `/admins` (Mod, paridad sb0t `Eval.Admins`): difunde A TODA LA SALA el
/// listado de admins online (header con quién lo pidió — stealth-aware —,
/// una línea por op y footer, AdminList #0-2).
fn handle_admins(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let mut ops: Vec<(String, u8)> = ctx
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in && (*u.level.read() as u8) > ILevel::Regular as u8)
        .map(|u| (u.name.read().clone(), *u.level.read() as u8))
        .collect();
    ops.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase())));
    let requester = if ctx.room_flags.get("stealth") {
        ctx.settings.room_name.clone()
    } else {
        user.name.read().clone()
    };
    ctx.broadcast_print(&ctx.templates.render("adminlist.header", &[("+n", &requester)]));
    for (name, level) in &ops {
        ctx.broadcast_print(&ctx.templates.render(
            "adminlist.entry",
            &[("+n", name), ("+l", &level.to_string())],
        ));
    }
    ctx.broadcast_print(&ctx.templates.get("adminlist.footer"));
}

fn handle_announce(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let text = args.trim();
    if text.is_empty() {
        send_system_line(ctx, user, "Usage: /announce <text>");
        return;
    }
    // Paridad sb0t Eval.Announce: texto del server a toda la sala + aviso
    // "+a announced" solo a moderadores+.
    ctx.broadcast_print(text);
    let by = ctx.templates.render("announce.by", &[("+a", &user.name.read().clone())]);
    for u in ctx.user_pool.users() {
        if u.logged_in && (*u.level.read() as u8) >= ILevel::Moderator as u8 {
            let _ = u.print(&ctx.settings.bot_name, &by);
        }
    }
}

fn handle_echo(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let target_name = parts.next().unwrap_or("").trim();
    let text = parts.next().unwrap_or("").trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /echo <nick> [text]  (empty text clears)");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot echo a user of equal or higher level.");
        return;
    }
    if text.is_empty() {
        *target.echo_text.write() = None;
        send_system_line(ctx, user, &format!("Cleared echo on '{}'.", target_name));
    } else {
        *target.echo_text.write() = Some(text.to_string());
        send_system_line(ctx, user, &format!("Echo set on '{}'.", target_name));
    }
}

fn handle_clone(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let target_name = parts.next().unwrap_or("").trim();
    let text = parts.next().unwrap_or("").trim();
    if target_name.is_empty() || text.is_empty() {
        send_system_line(ctx, user, "Usage: /clone <nick> <text>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    // Difunde un mensaje público/emote como si lo dijera el target.
    let name = target.name.read().clone();
    let issuer_name = user.name.read().clone();
    notify_mods(
        ctx,
        &ctx.templates.render("clone.by", &[("+n", &name), ("+a", &issuer_name)]),
    );
    let emote = text.strip_prefix("/me ");
    let vroom = *target.vroom.read();
    for u in ctx.user_pool.users() {
        if u.logged_in && *u.vroom.read() == vroom && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            match emote {
                Some(e) => { let _ = u.send_emote(&name, e); }
                None => { let _ = u.send_public(&name, text); }
            }
        }
    }
    send_system_line(ctx, user, &format!("Cloned message as '{}'.", target_name));
}

fn handle_kiddy(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /kiddy <nick>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot kiddy a user of equal or higher level.");
        return;
    }
    let now_on = !target.kiddied.load(std::sync::atomic::Ordering::Relaxed);
    target.kiddied.store(now_on, std::sync::atomic::Ordering::Relaxed);
    send_system_line(
        ctx,
        user,
        &format!("Kiddy mode {} for '{}'.", if now_on { "on" } else { "off" }, target_name),
    );
}

fn handle_mtimeout(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let mut parts = args.trim().rsplitn(2, char::is_whitespace);
    let secs_str = parts.next().unwrap_or("");
    let target_name = parts.next().unwrap_or("").trim();
    let (Ok(secs), false) = (secs_str.parse::<u64>(), target_name.is_empty()) else {
        send_system_line(ctx, user, "Usage: /mtimeout <nick> <seconds>");
        return;
    };
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot muzzle a user of equal or higher level.");
        return;
    }
    let until = server_core::time::unix_time() + secs.saturating_mul(1000);
    target.muzzle_until.store(until, std::sync::atomic::Ordering::Relaxed);
    target.muzzled.store(true, std::sync::atomic::Ordering::Relaxed);
    ctx.publish_link_event(server_core::LinkEvent::UserUpdated {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(&target),
    });
    send_system_line(ctx, &target, &format!("You have been muzzled for {}s.", secs));
    send_system_line(ctx, user, &format!("Muzzled '{}' for {}s.", target_name, secs));
}

fn handle_redirect(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    // Formato: <nick> <ip:port> (o astrahash:// que resolvemos a ip:port)
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let target_name = parts.next().unwrap_or("").trim();
    let dest = parts.next().unwrap_or("").trim();
    if target_name.is_empty() || dest.is_empty() {
        send_system_line(ctx, user, "Usage: /redirect <nick> <ip:port>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot redirect a user of equal or higher level.");
        return;
    }
    // Destino: hashlink Ares (`arlnk://...` o `CHATROOM:ip:port|nombre`,
    // paridad sb0t Hashlinks.Decrypt) o `ip:port` plano (extra de Astra).
    let dest = dest.strip_prefix("astrahash://").unwrap_or(dest);
    let (ip, port, room_label): (IpAddr, u16, String) =
        if let Some(hr) = server_core::hashlink::decode(dest) {
            (IpAddr::V4(hr.ip), hr.port, hr.name)
        } else if let Some((ip_str, port_str)) = dest.rsplit_once(':') {
            match (ip_str.parse::<IpAddr>(), port_str.parse::<u16>()) {
                (Ok(ip), Ok(port)) => (ip, port, format!("{}:{}", ip, port)),
                _ => {
                    send_system_line(ctx, user, "Invalid destination (arlnk:// or ip:port).");
                    return;
                }
            }
        } else {
            send_system_line(ctx, user, "Invalid destination (arlnk:// or ip:port).");
            return;
    };
    let _ = target.send(outbound::build_redirect_c(ip, port, &ctx.settings.room_name, target.ares_crypto));
    send_system_line(ctx, user, &format!("Redirected '{}' to {}:{}.", target_name, ip, port));
    // Anuncio público (AdminAction#20 de sb0t), stealth-aware.
    let signer = if ctx.room_flags.get("stealth")
        || user.cloaked.load(std::sync::atomic::Ordering::Relaxed)
    {
        ctx.settings.room_name.clone()
    } else {
        user.name.read().clone()
    };
    let tname = target.name.read().clone();
    ctx.broadcast_print(&ctx.templates.render(
        "adminaction.redirect",
        &[("+n", &tname), ("+a", &signer), ("+r", &room_label)],
    ));
}

fn handle_disableadmins(ctx: &AppContext, user: &Arc<AresUser>, disable: bool) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    ctx.admins_disabled.store(disable, std::sync::atomic::Ordering::Relaxed);
    send_system_line(
        ctx,
        user,
        if disable { "Admin commands disabled." } else { "Admin commands enabled." },
    );
}

// ============================================================================
// Flags de sala (Tanda 5) — requiere Admin+
// ============================================================================

fn handle_room_flag(ctx: &AppContext, user: &Arc<AresUser>, flag: &str, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.room_flags.set(flag, true);
            send_system_line(ctx, user, &format!("Room flag '{}' enabled.", flag));
        }
        "off" => {
            ctx.room_flags.set(flag, false);
            send_system_line(ctx, user, &format!("Room flag '{}' disabled.", flag));
        }
        "" => {
            let state = if ctx.room_flags.get(flag) { "on" } else { "off" };
            send_system_line(ctx, user, &format!("Room flag '{}' is {}.", flag, state));
        }
        _ => send_system_line(ctx, user, &format!("Usage: /{} [on|off]", flag)),
    }
}

/// `/disableavatar [on|off]` — alias invertido del flag `avatars`
/// (`on` = deshabilitar avatares).
fn handle_disableavatar(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            ctx.room_flags.set("avatars", false);
            send_system_line(ctx, user, "Avatars disabled.");
        }
        "off" => {
            ctx.room_flags.set("avatars", true);
            send_system_line(ctx, user, "Avatars enabled.");
        }
        "" => {
            let disabled = !ctx.room_flags.get("avatars");
            send_system_line(ctx, user, &format!("Avatars are {}.", if disabled { "disabled" } else { "enabled" }));
        }
        _ => send_system_line(ctx, user, "Usage: /disableavatar [on|off]"),
    }
}

fn handle_roomflags(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    send_system_line(ctx, user, "Room flags:");
    for (name, value) in ctx.room_flags.list() {
        send_system_line(ctx, user, &format!("{} = {}", name, if value { "on" } else { "off" }));
    }
}

/// `/cloak [on|off]` — flag per-usuario que oculta al admin en las acciones.
fn handle_cloak(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    match args.trim().to_ascii_lowercase().as_str() {
        "on" => {
            user.cloaked.store(true, std::sync::atomic::Ordering::Relaxed);
            send_system_line(ctx, user, "Cloak enabled.");
        }
        "off" => {
            user.cloaked.store(false, std::sync::atomic::Ordering::Relaxed);
            send_system_line(ctx, user, "Cloak disabled.");
        }
        "" => {
            let on = user.cloaked.load(std::sync::atomic::Ordering::Relaxed);
            send_system_line(ctx, user, &format!("Cloak is {}.", if on { "on" } else { "off" }));
        }
        _ => send_system_line(ctx, user, "Usage: /cloak [on|off]"),
    }
}

// ============================================================================
// Cuentas / quarantine / filtros de nombre / misc (Tanda 7)
// ============================================================================

fn handle_clearscreen(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    // Paridad sb0t Eval.ClearScreen: 500 líneas vacías a cada cliente y
    // anuncio "screen cleared by +n" (stealth-aware).
    let bot = ctx.settings.bot_name.clone();
    for u in ctx.user_pool.users() {
        if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        for _ in 0..500 {
            let _ = u.print(&bot, "");
        }
    }
    let by = if ctx.room_flags.get("stealth") {
        ctx.settings.room_name.clone()
    } else {
        user.name.read().clone()
    };
    ctx.broadcast_print(&ctx.templates.render("clearscreen.by", &[("+n", &by)]));
}

fn handle_locate(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let target_name = args.trim();
    if target_name.is_empty() {
        // Paridad sb0t Eval.Locate: lista quién está en qué vroom (>0).
        // Disponible para nivel > Regular o con el flag `general` activo.
        if !(has_level(user, ILevel::Voice) || ctx.room_flags.get("general")) {
            return;
        }
        let mut found = false;
        for u in ctx.user_pool.users() {
            if !u.logged_in
                || u.cloaked.load(std::sync::atomic::Ordering::Relaxed)
                || *u.vroom.read() == 0
            {
                continue;
            }
            if !found {
                found = true;
                send_system_line(ctx, user, &ctx.templates.get("locate.header"));
                send_system_line(ctx, user, "");
            }
            let name = u.name.read().clone();
            let vroom = u.vroom.read().to_string();
            send_system_line(
                ctx,
                user,
                &ctx.templates.render("locate.entry", &[("+n", &name), ("+v", &vroom)]),
            );
        }
        if found {
            send_system_line(ctx, user, "");
            send_system_line(ctx, user, &ctx.templates.get("locate.footer"));
        } else {
            send_system_line(ctx, user, &ctx.templates.get("locate.empty"));
        }
        return;
    }
    // Extra de Astra: `/locate <nick>` = geoip del usuario (Mod+).
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let Some(t) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    let region = if t.region.is_empty() { "unknown".to_string() } else { t.region.clone() };
    send_system_line(
        ctx,
        user,
        &format!(
            "'{}' ip={} country={} region={}",
            t.name.read(),
            t.external_ip,
            t.country,
            region
        ),
    );
}

fn handle_listquarantined(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let q: Vec<String> = ctx
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.quarantined.load(std::sync::atomic::Ordering::Relaxed))
        .map(|u| u.name.read().clone())
        .collect();
    if q.is_empty() {
        send_system_line(ctx, user, "No quarantined users.");
        return;
    }
    send_system_line(ctx, user, &format!("Quarantined ({}):", q.len()));
    for (i, name) in q.iter().enumerate() {
        send_system_line(ctx, user, &format!("{} - {}", i, name));
    }
}

fn handle_unquarantine(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let target = args.trim();
    if target.is_empty() {
        send_system_line(ctx, user, "Usage: /unquarantine <nick|index>");
        return;
    }
    let q: Vec<_> = ctx
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.quarantined.load(std::sync::atomic::Ordering::Relaxed))
        .collect();
    let found = if let Ok(idx) = target.parse::<usize>() {
        q.get(idx).cloned()
    } else {
        q.into_iter().find(|u| u.name.read().eq_ignore_ascii_case(target))
    };
    match found {
        Some(u) => {
            u.quarantined.store(false, std::sync::atomic::Ordering::Relaxed);
            u.needs_captcha.store(false, std::sync::atomic::Ordering::Relaxed);
            send_system_line(ctx, user, &format!("Un-quarantined '{}'.", u.name.read()));
        }
        None => send_system_line(ctx, user, "No matching quarantined user."),
    }
}

fn handle_listpasswords(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let accounts = ctx.db.list_accounts().unwrap_or_default();
    if accounts.is_empty() {
        send_system_line(ctx, user, "No registered accounts.");
        return;
    }
    send_system_line(ctx, user, &format!("Accounts ({}):", accounts.len()));
    for (name, level) in &accounts {
        send_system_line(ctx, user, &format!("{} (level {})", name, level));
    }
}

/// `/loadmotd` (Host, paridad sb0t `Eval.LoadMotd`): recarga el MOTD desde
/// la persistencia y lo anuncia (Notification#8: "MOTD reloaded by +n").
fn handle_loadmotd(ctx: &AppContext, user: &Arc<AresUser>) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    ctx.motd.reload();
    let name = user.name.read().clone();
    let announcer = if ctx.room_flags.get("stealth") {
        ctx.settings.room_name.clone()
    } else {
        name
    };
    ctx.broadcast_print(&format!("MOTD reloaded by {}", announcer));
}

/// `/rempassword <índice|nick>` (Host, paridad sb0t `Eval.RemovePassword`):
/// elimina una cuenta de la lista de `/listpasswords`. sb0t usa el índice de
/// la lista; se acepta también el nick por comodidad.
fn handle_rempassword(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let arg = args.trim();
    if arg.is_empty() {
        send_system_line(ctx, user, "Usage: /rempassword <index|nick>");
        return;
    }
    let accounts = ctx.db.list_accounts().unwrap_or_default();
    let name = if let Ok(idx) = arg.parse::<usize>() {
        match accounts.get(idx) {
            Some((n, _)) => n.clone(),
            None => {
                send_system_line(ctx, user, "No account at that index.");
                return;
            }
        }
    } else {
        arg.to_string()
    };
    let Ok(Some(record)) = ctx.accounts.find_by_name(&name) else {
        send_system_line(ctx, user, "No account with that name.");
        return;
    };
    let _ = ctx.db.delete_account(&record.guid);
    send_system_line(ctx, user, &format!("Password removed for '{}'.", name));
}

/// `/logout` / `/logoff` (paridad sb0t `AccountManager.Logout`): cierra la
/// sesión de la cuenta — el nivel vuelve a regular, la cuenta NO se borra.
fn handle_logout(
    ctx: &AppContext,
    user: &Arc<AresUser>,
) -> Vec<astra_scripting::ScriptEvent> {
    let name = user.name.read().clone();
    if *user.level.read() == ILevel::Regular {
        // Sin sesión elevada: no hay nada que cerrar (sb0t exige Registered).
        return vec![];
    }
    let notice = ctx.templates.get("revoke.target");
    // apply_level ya difunde el cambio de nivel a la sala y lo persiste…
    // pero logout NO debe tocar el nivel guardado de la cuenta: solo la
    // sesión. Guardamos y restauramos el nivel persistido.
    let saved = ctx.accounts.find_by_guid(&user.guid).ok().flatten().map(|a| a.level);
    apply_level(ctx, user, user, ILevel::Regular, &notice);
    if let Some(level) = saved {
        let _ = ctx.accounts.set_level(&user.guid, level);
    }
    vec![astra_scripting::ScriptEvent::Logout { name }]
}

/// `/setlevel <nick> <0-3>` (Owner, paridad sb0t `core/Events.cs:519`):
/// escala sb0t — 0=regular, 1=moderator, 2=admin, 3=host(owner).
fn handle_setlevel(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    args: &str,
) -> Vec<astra_scripting::ScriptEvent> {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return vec![];
    }
    let args = args.trim();
    let mut parts = args.rsplitn(2, char::is_whitespace);
    let level_str = parts.next().unwrap_or("");
    let target_name = parts.next().unwrap_or("").trim();
    let new_level = match level_str {
        "0" => ILevel::Regular,
        "1" => ILevel::Moderator,
        "2" => ILevel::Admin,
        "3" => ILevel::Owner,
        _ => {
            send_system_line(ctx, user, "Usage: /setlevel <nick> <0-3>");
            return vec![];
        }
    };
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /setlevel <nick> <0-3>");
        return vec![];
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, &ctx.templates.get("error.user_not_found"));
        return vec![];
    };
    let level_disp = format!("{} ({})", new_level as u8, level_name(new_level));
    let msg = ctx.templates.render("grant.target", &[("+l", &level_disp)]);
    if apply_level(ctx, user, &target, new_level, &msg) {
        send_system_line(
            ctx,
            user,
            &ctx.templates.render("grant.confirm", &[("+n", target_name), ("+l", &level_disp)]),
        );
        vec![astra_scripting::ScriptEvent::AdminLevelChanged {
            name: target.name.read().clone(),
        }]
    } else {
        vec![]
    }
}

/// Handler compartido de `/joinfilter` y `/filefilter`.
/// Subcomandos: `add <pat>`, `del <pat>`, `list` (o vacío = list).
fn handle_name_filter(ctx: &AppContext, user: &Arc<AresUser>, args: &str, is_join: bool) {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return;
    }
    let mgr = if is_join { &ctx.join_filters } else { &ctx.file_filters };
    let label = if is_join { "join" } else { "file" };
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();
    match sub.as_str() {
        "add" if !rest.is_empty() => {
            if mgr.add(rest) {
                send_system_line(ctx, user, &format!("{} filter added: {}", label, rest.to_ascii_lowercase()));
            } else {
                send_system_line(ctx, user, "Filter already exists (or invalid).");
            }
        }
        "del" | "rem" | "remove" if !rest.is_empty() => {
            if mgr.remove(rest) {
                send_system_line(ctx, user, &format!("{} filter removed.", label));
            } else {
                send_system_line(ctx, user, "No matching filter.");
            }
        }
        "" | "list" => {
            let list = mgr.list();
            if list.is_empty() {
                send_system_line(ctx, user, &format!("No {} filters.", label));
                return;
            }
            send_system_line(ctx, user, &format!("{} filters ({}):", label, list.len()));
            for p in &list {
                send_system_line(ctx, user, &format!("  {}", p));
            }
        }
        _ => send_system_line(
            ctx,
            user,
            &format!("Usage: /{}filter [add <pat>|del <pat>|list]", label),
        ),
    }
}

/// `/filter [add <pat> [accion]|del <pat>|list]` — dispatcher estilo sb0t
/// para el word filter (alias de addfilter/remfilter/listfilters).
fn handle_filter_dispatch(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let mut parts = args.trim().splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest = parts.next().unwrap_or("").trim();
    match sub.as_str() {
        "add" => handle_addfilter(ctx, user, rest),
        "del" | "rem" | "remove" => handle_remfilter(ctx, user, rest),
        "" | "list" => handle_listfilters(ctx, user, ""),
        _ => send_system_line(ctx, user, "Usage: /filter [add <word> [block|kick|ban]|del <word>|list]"),
    }
}

/// `/link <name> <server> <port>` — solicita crear un link a otro server.
fn handle_link(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() < 3 {
        send_system_line(ctx, user, "Usage: /link <name> <server> <port>");
        return;
    }
    let Ok(port) = parts[2].parse::<u16>() else {
        send_system_line(ctx, user, "Invalid port.");
        return;
    };
    let req = server_core::LinkRequest::CreateLink {
        name: parts[0].to_string(),
        server: parts[1].to_string(),
        port,
    };
    if ctx.link_requests.send(req).is_ok() {
        send_system_line(ctx, user, &format!("Link request to {}:{} queued.", parts[1], port));
    } else {
        send_system_line(ctx, user, "Link subsystem is not running.");
    }
}

/// `/unlink <name>` — solicita desconectar un link.
fn handle_unlink(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let name = args.trim();
    if name.is_empty() {
        send_system_line(ctx, user, "Usage: /unlink <name>");
        return;
    }
    let req = server_core::LinkRequest::DisconnectLink { name: name.to_string() };
    if ctx.link_requests.send(req).is_ok() {
        send_system_line(ctx, user, &format!("Unlink request for '{}' queued.", name));
    } else {
        send_system_line(ctx, user, "Link subsystem is not running.");
    }
}

/// `/listscripts` — lista los scripts JS cargados.
fn handle_listscripts(ctx: &AppContext, user: &Arc<AresUser>) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let Some(hooks) = ctx.scripting_hooks.read().clone() else {
        send_system_line(ctx, user, "Scripting is not available.");
        return;
    };
    let names = (hooks.list)();
    if names.is_empty() {
        send_system_line(ctx, user, "No scripts loaded.");
    } else {
        for name in names {
            send_system_line(ctx, user, &name);
        }
    }
}

/// `/loadscript <name>` — carga `<data_dir>/scripts/<name>.js`.
fn handle_loadscript(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let name = args.trim();
    if name.is_empty() {
        send_system_line(ctx, user, "Usage: /loadscript <name>");
        return;
    }
    let Some(hooks) = ctx.scripting_hooks.read().clone() else {
        send_system_line(ctx, user, "Scripting is not available.");
        return;
    };
    match (hooks.load)(name) {
        Ok(loaded) => send_system_line(ctx, user, &format!("Script '{}' loaded.", loaded)),
        Err(e) => send_system_line(ctx, user, &format!("Failed to load '{}': {}", name, e)),
    }
}

/// `/killscript <name>` — descarga un script cargado.
fn handle_killscript(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let name = args.trim();
    if name.is_empty() {
        send_system_line(ctx, user, "Usage: /killscript <name>");
        return;
    }
    let Some(hooks) = ctx.scripting_hooks.read().clone() else {
        send_system_line(ctx, user, "Scripting is not available.");
        return;
    };
    match (hooks.kill)(name) {
        Ok(()) => send_system_line(ctx, user, &format!("Script '{}' unloaded.", name)),
        Err(e) => send_system_line(ctx, user, &format!("Failed to unload '{}': {}", name, e)),
    }
}

/// Feeds internos a los que un admin puede suscribirse.
#[derive(Clone, Copy)]
enum Subscription {
    Vspy,
    IpSend,
    LogSend,
    BanSend,
    Errors,
}

/// Toggle de una suscripción per-admin (`/vspy`, `/ipsend`, `/logsend`,
/// `/bansend`). Cada admin activa/desactiva su propio feed.
fn handle_subscription(ctx: &AppContext, user: &Arc<AresUser>, args: &str, sub: Subscription) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let (flag, label): (&std::sync::atomic::AtomicBool, &str) = match sub {
        Subscription::Vspy => (&user.sub_vspy, "vspy"),
        Subscription::IpSend => (&user.sub_ipsend, "ipsend"),
        Subscription::LogSend => (&user.sub_logsend, "logsend"),
        Subscription::BanSend => (&user.sub_bansend, "bansend"),
        Subscription::Errors => (&user.sub_errors, "errors"),
    };
    use std::sync::atomic::Ordering;
    let now_on = match args.trim().to_ascii_lowercase().as_str() {
        "on" => true,
        "off" => false,
        "" => !flag.load(Ordering::Relaxed), // toggle
        _ => {
            send_system_line(ctx, user, &format!("Usage: /{} [on|off]", label));
            return;
        }
    };
    flag.store(now_on, Ordering::Relaxed);
    send_system_line(
        ctx,
        user,
        &format!("{} feed {}.", label, if now_on { "enabled" } else { "disabled" }),
    );

    // Al activar ipsend, volcar las IPs de los usuarios actuales (como sb0t).
    if now_on {
        if let Subscription::IpSend = sub {
            for u in ctx.user_pool.users() {
                if u.logged_in {
                    send_system_line(
                        ctx,
                        user,
                        &format!(
                            "IPSEND: {} {} {} {}",
                            u.name.read(),
                            u.external_ip,
                            u.local_ip,
                            u.data_port
                        ),
                    );
                }
            }
        }
    }
}

/// API key de Wordnik hardcodeada (idéntica a sb0t DefineDictionary.cs).
const WORDNIK_API_KEY: &str = "0f69e2f981991cfe0e1351afd6a2d39da10077112d21165be";

/// `/define <word>` — busca la definición en Wordnik (misma URL/key que sb0t).
/// Hace el fetch en una task async y PMea el resultado al que lo pidió.
fn handle_define(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let word = args.trim().to_lowercase();
    if word.is_empty() {
        send_system_line(ctx, user, "Usage: /define <word>");
        return;
    }
    let bot = ctx.settings.bot_name.clone();
    let user = user.clone();
    let encoded = urlencode(&word);
    spawn_lookup(user.clone(), bot.clone(), move || async move {
        let url = format!(
            "http://api.wordnik.com/v4/word.json/{}/definitions?includeRelated=false&limit=3&sourceDictionaries=all&useCanonical=false",
            encoded
        );
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .header("api_key", WORDNIK_API_KEY)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .ok()?;
        let json: serde_json::Value = resp.json().await.ok()?;
        let defs = json.as_array()?;
        if defs.is_empty() {
            return Some(format!("No definition found for '{}'.", word));
        }
        let mut out = format!("Definitions for '{}':", word);
        for d in defs.iter().take(3) {
            if let Some(t) = d.get("text").and_then(|v| v.as_str()) {
                out.push('\n');
                out.push_str(t);
            }
        }
        Some(out)
    });
}

/// `/urban <term>` — busca en Urban Dictionary (misma URL que sb0t).
fn handle_urban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let term = args.trim().to_string();
    if term.is_empty() {
        send_system_line(ctx, user, "Usage: /urban <term>");
        return;
    }
    let bot = ctx.settings.bot_name.clone();
    let user = user.clone();
    let encoded = urlencode(&term);
    spawn_lookup(user.clone(), bot.clone(), move || async move {
        let url = format!(
            "http://www.urbandictionary.com/iphone/search/define?term={}",
            encoded
        );
        let client = reqwest::Client::new();
        let json: serde_json::Value = client
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let first = json.get("list")?.as_array()?.first()?;
        let def = first.get("definition")?.as_str()?;
        Some(format!("Urban '{}': {}", term, def.replace(['[', ']'], "")))
    });
}

/// URL-encode mínimo (espacios y caracteres no alfanuméricos → %XX).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Ejecuta un lookup async (HTTP) en una task de tokio y PMea el resultado
/// (o un error honesto) al usuario. Si no hay runtime tokio (p. ej. en
/// tests), degrada a un aviso sin colgar.
fn spawn_lookup<F, Fut>(user: Arc<AresUser>, bot: String, make: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<String>> + Send,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(async move {
                let result = make().await;
                let text = result.unwrap_or_else(|| "Lookup failed (service unavailable).".to_string());
                for line in text.lines() {
                    let _ = user.send_pvt(&bot, line);
                }
            });
        }
        Err(_) => {
            let _ = user.send_pvt(&bot, "Lookup requires the async runtime (unavailable here).");
        }
    }
}

/// Headers requeridos por la API de GitHub (paridad `LiveScript.cs` de sb0t).
fn github_headers(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header("User-Agent", "astra-server")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
}

#[derive(serde::Deserialize)]
struct GitHubSearchResponse {
    items: Vec<GitHubRepository>,
}

#[derive(serde::Deserialize)]
struct GitHubRepository {
    name: String,
    full_name: String,
    #[serde(rename = "private")]
    is_private: bool,
    owner: GitHubOwner,
    description: Option<String>,
}

#[derive(serde::Deserialize)]
struct GitHubOwner {
    login: String,
}

#[derive(serde::Deserialize)]
struct GitHubRelease {
    zipball_url: String,
}

/// `/livescripts` — busca en GitHub repos públicos con el topic
/// `areschatscript` (paridad `LiveScript.LiveScripts` de sb0t).
fn handle_livescripts(ctx: &AppContext, user: &Arc<AresUser>) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let bot = ctx.settings.bot_name.clone();
    let endpoint = ctx.settings.live_scripts_endpoint.clone();
    let user = user.clone();
    spawn_lookup(user, bot, move || async move {
        let client = reqwest::Client::new();
        let url = format!("{}/search/repositories?q=topic:areschatscript+is:public", endpoint);
        let req = github_headers(client.get(&url)).timeout(std::time::Duration::from_secs(10));
        let resp = req.send().await.ok()?;
        let parsed: GitHubSearchResponse = resp.json().await.ok()?;
        let lines: Vec<String> = parsed
            .items
            .iter()
            .filter(|r| !r.is_private)
            .map(|r| {
                format!(
                    "Script: {}  Author: {}  Path: {}  Description: {}",
                    r.name,
                    r.owner.login,
                    r.full_name,
                    r.description.as_deref().unwrap_or("")
                )
            })
            .collect();
        if lines.is_empty() {
            Some("No scripts available".to_string())
        } else {
            Some(lines.join("\n"))
        }
    });
}

/// `/downloadscript <owner/repo>` — descarga el último release de un repo
/// de GitHub, extrae el primer `.js` que encuentra, y lo carga (paridad
/// `LiveScript.GetDownload`/`Download` de sb0t). Simplificación deliberada:
/// sb0t renombra el directorio raíz extraído a `<filename>.js` (su modelo
/// permite que un "script" sea una carpeta); acá se busca el primer
/// archivo `.js` dentro del zip y se lo carga como script individual,
/// consistente con el modelo de `ScriptManager` de Astra.
fn handle_downloadscript(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !has_level(user, ILevel::Owner) {
        send_system_line(ctx, user, "Access denied. Owner required.");
        return;
    }
    let path = args.trim().to_string();
    let valid = regex::Regex::new(r"^[a-zA-Z0-9_-]+/[a-zA-Z0-9_-]+$")
        .map(|re| re.is_match(&path))
        .unwrap_or(false);
    if !valid {
        send_system_line(ctx, user, &format!("{} is not a valid path. Path must be like user/repository", path));
        return;
    }
    send_system_line(ctx, user, &format!("Starting download of the script from: {}", path));
    let bot = ctx.settings.bot_name.clone();
    let endpoint = ctx.settings.live_scripts_endpoint.clone();
    let data_dir = ctx.settings.data_dir.clone();
    let hooks = ctx.scripting_hooks.read().clone();
    let user = user.clone();
    spawn_lookup(user, bot, move || async move {
        download_and_load_script(endpoint, path, data_dir, hooks).await
    });
}

async fn download_and_load_script(
    endpoint: String,
    path: String,
    data_dir: String,
    hooks: Option<server_core::ScriptingHooks>,
) -> Option<String> {
    let client = reqwest::Client::new();

    let release_url = format!("{}/repos/{}/releases/latest", endpoint, path);
    let req = github_headers(client.get(&release_url)).timeout(std::time::Duration::from_secs(10));
    let Ok(resp) = req.send().await else {
        return Some(format!("Unable to get the script with path: {}", path));
    };
    let Ok(release) = resp.json::<GitHubRelease>().await else {
        return Some(format!("Unable to get the script with path: {}", path));
    };
    if release.zipball_url.is_empty() {
        return Some(format!("Unable to get the script with path: {}", path));
    }

    let req = github_headers(client.get(&release.zipball_url)).timeout(std::time::Duration::from_secs(30));
    let Ok(zip_resp) = req.send().await else {
        return Some(format!("Failed to download release zip for: {}", path));
    };
    let Ok(zip_bytes) = zip_resp.bytes().await else {
        return Some(format!("Failed to download release zip for: {}", path));
    };

    let filename = path.split('/').nth(1).unwrap_or("script").to_string();
    let scripts_dir = std::path::PathBuf::from(&data_dir).join("scripts");
    let zip_vec = zip_bytes.to_vec();
    let fname = filename.clone();
    let extract_result = tokio::task::spawn_blocking(move || {
        extract_script_folder(&zip_vec, &scripts_dir, &fname)
    })
    .await
    .unwrap_or_else(|e| Err(format!("extraction task panicked: {}", e)));

    let n_files = match extract_result {
        Ok(n) => n,
        Err(e) => return Some(format!("Unable to extract script from {}: {}", path, e)),
    };

    let Some(hooks) = hooks else {
        return Some(format!(
            "Successfully downloaded live script '{}' ({} files) (scripting unavailable, not auto-loaded).",
            filename, n_files
        ));
    };
    // Se carga por NOMBRE DE CARPETA (el modelo de carpetas resuelve el
    // archivo principal dentro de `<scripts_dir>/<filename>/`).
    match (hooks.load)(&filename) {
        Ok(_) => Some(format!(
            "Successfully downloaded and loaded live script '{}' ({} files).",
            filename, n_files
        )),
        Err(e) => Some(format!(
            "Downloaded '{}' ({} files) but failed to load: {}",
            filename, n_files, e
        )),
    }
}

/// Extrae TODO el contenido de un zip (bytes en memoria) a la carpeta del
/// script `<scripts_dir>/<name>/`, aplanando la carpeta raíz que agrega GitHub
/// a sus zipballs (`owner-repo-sha/...`). Así un script con varios archivos
/// (principal + sub-scripts + datos) queda completo, no solo el primer `.js`.
/// Retorna cuántos archivos se escribieron.
fn extract_script_folder(zip_bytes: &[u8], scripts_dir: &std::path::Path, name: &str) -> Result<usize, String> {
    use std::path::PathBuf;
    let dest = scripts_dir.join(name);
    // Reemplazar cualquier versión previa.
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|e| format!("mkdir failed: {}", e))?;

    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("invalid zip: {}", e))?;
    let mut written = 0usize;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip read error: {}", e))?;
        // Ruta segura (sin traversal). GitHub envuelve todo en una carpeta raíz
        // `owner-repo-sha/`; la aplanamos saltando el primer componente.
        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let stripped: PathBuf = enclosed.components().skip(1).collect();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&stripped);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("mkdir failed: {}", e))?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {}", e))?;
            }
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buf)
                .map_err(|e| format!("zip extract error: {}", e))?;
            std::fs::write(&out, &buf).map_err(|e| format!("write failed: {}", e))?;
            written += 1;
        }
    }
    if written == 0 {
        return Err("the downloaded archive had no files".to_string());
    }
    Ok(written)
}

/// `/trace <nick|ip>` — geolocaliza una IP usando la base GeoIP (si está
/// cargada). Sin base, degrada a mostrar el país del login (como /locate).
fn handle_trace(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let arg = args.trim();
    if arg.is_empty() {
        send_system_line(ctx, user, "Usage: /trace <nick|ip>");
        return;
    }
    // Resolver el arg a una IP (nick online o IP literal).
    let (ip, who) = if let Ok(ip) = arg.parse::<IpAddr>() {
        (ip, arg.to_string())
    } else if let Some(t) = ctx.user_pool.get_by_name(arg) {
        (t.external_ip, t.name.read().clone())
    } else {
        send_system_line(ctx, user, "User not found (or invalid IP).");
        return;
    };

    if !ctx.geoip.has_city() {
        send_system_line(
            ctx,
            user,
            "/trace requires a GeoIP database. Place a city.mmdb (MaxMind GeoLite2 or DB-IP Lite) in the data dir.",
        );
        return;
    }
    match ctx.geoip.lookup_city(ip) {
        Some(g) => {
            let parts = [
                g.city.clone(),
                g.region.clone(),
                g.country.clone().or(g.country_code.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            let loc = if parts.is_empty() { "unknown".to_string() } else { parts.join(", ") };
            let asn = ctx.geoip.lookup_asn(ip).map(|a| format!(" ASN{}", a)).unwrap_or_default();
            send_system_line(ctx, user, &format!("TRACE {} [{}]: {}{}", who, ip, loc, asn));
        }
        None => send_system_line(ctx, user, &format!("TRACE {} [{}]: no data.", who, ip)),
    }
}

/// Comandos reconocidos pero cuya funcionalidad requiere infraestructura
/// externa que Astra no incluye (ver comentario en el dispatcher).
fn handle_unavailable(ctx: &AppContext, user: &Arc<AresUser>, cmd: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let why = match cmd {
        "define" | "urban" => "requires an external dictionary service",
        "roomsearch" => "searches the Ares channel list, which is not available in this build",
        "ipsend" | "logsend" | "bansend" => "requires a connected link hub",
        "trace" | "vspy" => "requires packet-tracing support",
        "loadtemplate" => {
            "reloads sb0t's message templates; Astra uses built-in messages, so there is nothing to reload"
        }
        _ => "is not available in this build",
    };
    send_system_line(ctx, user, &format!("/{} {}.", cmd, why));
}

// ============================================================================
// Efectos de texto per-usuario (Tanda 6) — Moderator+
// ============================================================================

fn handle_text_effect(
    ctx: &AppContext,
    user: &Arc<AresUser>,
    args: &str,
    effect: TextEffect,
    enable: bool,
) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /<effect> <nick>");
        return;
    }
    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };
    if enable && !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot target a user of equal or higher level.");
        return;
    }
    let (flag, label) = match effect {
        TextEffect::Lower => (&target.lowered, "lower"),
        TextEffect::Kewl => (&target.kewl, "kewl text"),
        TextEffect::Paint => (&target.painted, "paint"),
    };
    flag.store(enable, std::sync::atomic::Ordering::Relaxed);
    send_system_line(
        ctx,
        user,
        &format!(
            "{} {} for '{}'.",
            label,
            if enable { "enabled" } else { "disabled" },
            target_name
        ),
    );
}

/// Formatea un timestamp epoch-ms como tiempo relativo ("5m ago", etc.).
fn format_time_ago(last_seen_ms: i64) -> String {
    let now_ms = server_core::time::unix_time() as i64;
    let secs = ((now_ms - last_seen_ms).max(0) / 1000) as u64;
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

fn can_edit_topic(user: &AresUser) -> bool {
    let level = *user.level.read() as u8;
    level >= ILevel::Moderator as u8
}

fn guid_to_hex(guid: &[u8; 16]) -> String {
    guid.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Saca a `target` de la sala: lo quita del pool y difunde su PART a todo
/// el mundo (paridad `AresClient.Disconnect`/`SendDepart` de sb0t). No
/// cierra el socket subyacente (la sesión vieja, si sigue viva, se
/// autolimpia cuando su loop de lectura falle); alcanza con liberar el
/// nombre y notificar la salida. Usado por `/kick` y por el hijack de
/// login (nick duplicado desde la misma IP externa).
pub fn force_part_user(ctx: &AppContext, target: &Arc<AresUser>) {
    // Delegado a server-core (`AppContext::force_part_user`) para que el
    // scripting (`user.kick()`/`ban()`) pueda expulsar sin ciclo de deps.
    ctx.force_part_user(target);
}

fn broadcast_topic(ctx: &AppContext, text: &str) {
    for u in ctx.user_pool.users() {
        if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let _ = u.send_topic(text);
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

fn send_system_line(ctx: &AppContext, user: &Arc<AresUser>, text: &str) {
    let from = &ctx.settings.bot_name;
    // Resolución central de textos del sistema: si `text` coincide con el
    // default de una clave del catálogo, se manda el override configurado por
    // el admin (o el mismo default). Los textos dinámicos o no catalogados
    // pasan tal cual.
    let resolved = ctx.templates.resolve(text);
    let _ = user.print(from, &resolved);
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
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;

    use proto_ares::{PacketReader, TcpMsg};
    use server_core::db::Database;

    #[test]
    fn extract_script_folder_extracts_all_and_flattens_github_root() {
        // Arma un zip estilo zipball de GitHub: TODO bajo una carpeta raíz
        // `owner-repo-sha/`, con varios archivos y un subdirectorio.
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            use std::io::Write as _;
            let root = "owner-mygame-abc123";
            zw.start_file(format!("{root}/mygame.js"), opts).unwrap();
            zw.write_all(b"function onLoad(){}").unwrap();
            zw.start_file(format!("{root}/helper.js"), opts).unwrap();
            zw.write_all(b"function h(){return 1;}").unwrap();
            zw.start_file(format!("{root}/data/config.txt"), opts).unwrap();
            zw.write_all(b"clave=valor").unwrap();
            zw.start_file(format!("{root}/README.md"), opts).unwrap();
            zw.write_all(b"# mygame").unwrap();
            zw.finish().unwrap();
        }
        let zip_bytes = buf.into_inner();

        let tmp = std::env::temp_dir().join(format!("astra_extract_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let n = extract_script_folder(&zip_bytes, &tmp, "mygame").expect("extract");
        assert_eq!(n, 4, "deben extraerse los 4 archivos");

        let base = tmp.join("mygame");
        // Todo se aplanó bajo <scripts>/mygame/ (sin la carpeta raíz de GitHub).
        assert!(base.join("mygame.js").is_file());
        assert!(base.join("helper.js").is_file());
        assert!(base.join("README.md").is_file());
        // El subdirectorio se preserva.
        assert!(base.join("data/config.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(base.join("data/config.txt")).unwrap(),
            "clave=valor"
        );
        // La carpeta raíz de GitHub NO debe aparecer.
        assert!(!base.join("owner-mygame-abc123").exists());
        std::fs::remove_dir_all(&tmp).ok();
    }
    use server_core::settings::Settings;
    use tokio::sync::mpsc;

    fn make_test_ctx() -> Arc<AppContext> {
        let db = Database::in_memory().expect("in-memory db");
        Arc::new(AppContext::new(Settings::default(), db))
    }

    fn make_test_user(id: u16, name: &str) -> (Arc<AresUser>, mpsc::UnboundedReceiver<bytes::Bytes>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut user = AresUser::new(id, IpAddr::V4(Ipv4Addr::new(10, 0, 0, id as u8)), [id as u8; 16]);
        user.logged_in = true;
        user.sender = Some(tx);
        *user.name.write() = name.to_string();
        (Arc::new(user), rx)
    }

    fn decode_pvt(pkt: bytes::Bytes) -> (String, String) {
        assert_eq!(pkt[0], TcpMsg::Pmt as u8);
        let mut r = PacketReader::new(&pkt[1..]);
        let from = r.read_string_nt().expect("from");
        let text = r.read_string_nt().expect("text");
        (from, text)
    }

    fn decode_topic(pkt: bytes::Bytes) -> String {
        assert_eq!(pkt[0], TcpMsg::ServerTopic as u8);
        let mut r = PacketReader::new(&pkt[1..]);
        r.read_string_nt().expect("topic")
    }

    fn next_pvt_text(rx: &mut mpsc::UnboundedReceiver<bytes::Bytes>) -> String {
        for _ in 0..16 {
            let pkt = rx.try_recv().expect("expected queued packet");
            if pkt[0] == TcpMsg::Pmt as u8 {
                let (_from, text) = decode_pvt(pkt);
                return text;
            }
        }
        panic!("no PM packet found in queue");
    }

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
    fn command_full_line_reconstructs_for_scripts() {
        // Paridad sb0t: onCommand recibe la línea completa.
        assert_eq!(command_full_line("nuevojuego", "perro"), "nuevojuego perro");
        assert_eq!(command_full_line("historialjuego", ""), "historialjuego");
        assert_eq!(command_full_line("kick", "Bob spam"), "kick Bob spam");
    }

    #[test]
    fn parse_command_hash_prefix() {
        // Paridad sb0t: '#' es prefijo de comando igual que '/'.
        let (cmd, args) = parse_command("#help").unwrap();
        assert_eq!(cmd, "help");
        assert_eq!(args, "");
        let (cmd, args) = parse_command("#kick alice spam").unwrap();
        assert_eq!(cmd, "kick");
        assert_eq!(args, "alice spam");
        assert!(parse_command("#").is_none());
    }

    #[test]
    fn captcha_gate_blocks_commands_except_help_and_login() {
        let ctx = make_test_ctx();
        let (alice, mut rx) = make_test_user(1, "Alice");
        alice.needs_captcha.store(true, std::sync::atomic::Ordering::Relaxed);

        // Comando cualquiera: se ignora en silencio (handled, sin eventos).
        let (handled, events) = dispatch_builtin(&ctx, &alice, "whois", "Bob");
        assert!(handled);
        assert!(events.is_empty());
        assert!(rx.try_recv().is_err(), "no debe responder nada");

        // /help sí responde, incluso con captcha pendiente (Events.cs:471).
        let (handled, events) = dispatch_builtin(&ctx, &alice, "help", "");
        assert!(handled);
        assert!(matches!(
            events.as_slice(),
            [astra_scripting::ScriptEvent::Help { from }] if from == "Alice"
        ));
        assert!(rx.try_recv().is_ok(), "help debe imprimir líneas");
    }

    #[test]
    fn parse_empty_command() {
        // Solo "/" sin nada más → no es un comando válido
        assert!(parse_command("/").is_none());
        assert!(parse_command("/  ").is_none());
    }

    #[test]
    fn builtin_help_sends_lines() {
        // Un usuario Regular no debe ver comandos Moderator+/Admin+/Owner
        // (ver `handle_help`); solo el owner ve la lista completa.
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");

        let (handled, _) = dispatch_builtin(&ctx, &alice, "help", "");
        assert!(handled);
        let mut lines = Vec::new();
        while let Ok(pkt) = alice_rx.try_recv() {
            lines.push(decode_pvt(pkt).1);
        }
        assert!(lines.contains(&"/help - show this help".to_string()));
        assert!(lines.contains(&"/nick <name> - change your nickname".to_string()));
        assert!(!lines.iter().any(|l| l.starts_with("/ban ")), "Regular no debe ver /ban");
        assert!(lines.len() < DEFAULT_HELP_LINES.len(), "debe filtrar al menos una línea");

        let (owner, mut owner_rx) = make_test_user(2, "Owner");
        *owner.level.write() = ILevel::Owner;
        let (handled, _) = dispatch_builtin(&ctx, &owner, "help", "");
        assert!(handled);
        for expected in DEFAULT_HELP_LINES {
            let pkt = owner_rx.try_recv().expect("expected help line for owner");
            let (_from, text) = decode_pvt(pkt);
            assert_eq!(&text, expected);
        }
        assert!(owner_rx.try_recv().is_err(), "no extra lines expected");
    }

    #[test]
    fn builtin_users_lists_connected_users() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let (handled, _) = dispatch_builtin(&ctx, &alice, "users", "");
        assert!(handled);

        let line1 = alice_rx.try_recv().expect("users line 1");
        let line2 = alice_rx.try_recv().expect("users line 2");
        let (_from1, t1) = decode_pvt(line1);
        let (_from2, t2) = decode_pvt(line2);

        assert_eq!(t1, "Users online: 2");
        assert_eq!(t2, "Alice, Bob");
    }

    #[test]
    fn builtin_unknown_is_not_handled() {
        let ctx = make_test_ctx();
        let (user, _rx) = make_test_user(1, "Alice");
        let (handled, _) = dispatch_builtin(&ctx, &user, "notreal", "");
        assert!(!handled);
    }

    #[test]
    fn builtin_topic_without_args_shows_current_topic() {
        let ctx = make_test_ctx();
        let (user, mut rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(user.clone());

        let (handled, _) = dispatch_builtin(&ctx, &user, "topic", "");
        assert!(handled);

        let msg = rx.try_recv().expect("topic response");
        let (_from, text) = decode_pvt(msg);
        assert_eq!(text, "Topic: Welcome to Astra");
    }

    #[test]
    fn builtin_topic_update_requires_moderator() {
        let ctx = make_test_ctx();
        let (user, mut rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(user.clone());

        let (handled, _) = dispatch_builtin(&ctx, &user, "topic", "nuevo topic");
        assert!(handled);

        let msg = rx.try_recv().expect("deny response");
        let (_from, text) = decode_pvt(msg);
        assert_eq!(text, "Access denied. Moderator+ required.");
        assert_eq!(ctx.current_room_topic(), "Welcome to Astra");
    }

    #[test]
    fn builtin_topic_update_broadcasts_when_moderator() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "topic", "nuevo topic");
        assert!(handled);
        assert_eq!(ctx.current_room_topic(), "nuevo topic");

        let bob_topic = bob_rx.try_recv().expect("bob topic");
        assert_eq!(decode_topic(bob_topic), "nuevo topic");

        let alice_topic = alice_rx.try_recv().expect("alice topic");
        assert_eq!(decode_topic(alice_topic), "nuevo topic");

        let ack = alice_rx.try_recv().expect("alice ack");
        let (_from, ack_text) = decode_pvt(ack);
        assert_eq!(ack_text, "Topic updated.");
    }

    #[test]
    fn builtin_motd_view_when_empty_and_after_set() {
        let ctx = make_test_ctx();
        // Alice necesita ser al menos moderador para setear el MOTD.
        let (user, mut rx) = make_test_user(1, "Alice");
        *user.level.write() = server_core::ILevel::Owner;
        ctx.user_pool.add(user.clone());

        // Sin MOTD configurado.
        let (handled, _) = dispatch_builtin(&ctx, &user, "motd", "");
        assert!(handled);
        let (_from, text) = decode_pvt(rx.try_recv().expect("no-motd response"));
        assert_eq!(text, "No MOTD is set.");

        // Setear un MOTD con placeholder y volver a verlo (sustituido para Alice).
        let (handled, _) = dispatch_builtin(&ctx, &user, "motd", "Hola +n");
        assert!(handled);
        let (_from, confirm) = decode_pvt(rx.try_recv().expect("set confirm"));
        assert_eq!(confirm, "MOTD updated.");

        let (handled, _) = dispatch_builtin(&ctx, &user, "motd", "");
        assert!(handled);
        let (_from, view) = decode_pvt(rx.try_recv().expect("motd view"));
        assert_eq!(view, "Hola Alice");
    }

    #[test]
    fn builtin_ban_requires_moderator() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        assert!(handled);

        let msg = alice_rx.try_recv().expect("deny");
        let (_from, text) = decode_pvt(msg);
        assert_eq!(text, "Access denied. Admin+ required.");
        assert!(!ctx.bans.is_banned(&bob.guid, bob.external_ip));
    }

    #[test]
    fn builtin_ban_and_unban_online_user() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (ban_handled, _) = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        assert!(ban_handled);
        assert!(ctx.bans.is_banned(&bob.guid, bob.external_ip));
        assert!(ctx.user_pool.get_by_name("Bob").is_none());

        let ack_text = next_pvt_text(&mut alice_rx);
        assert!(ack_text.starts_with("Banned 'Bob' (ident "));
        // Anuncio público de la acción (AdminAction#0 de sb0t).
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob was banned by Alice");

        let notice = next_pvt_text(&mut bob_rx);
        assert_eq!(notice, "You have been banned from this room.");

        let (unban_handled, _) = dispatch_builtin(&ctx, &alice, "unban", "Bob");
        assert!(unban_handled);
        assert!(!ctx.bans.is_banned(&bob.guid, bob.external_ip));

        let unban_text = next_pvt_text(&mut alice_rx);
        assert_eq!(unban_text, "Unban successful.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob was unbanned by Alice");
    }

    #[test]
    fn builtin_unban_without_match_reports_not_found() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "unban", "ghost");
        assert!(handled);

        let msg = alice_rx.try_recv().expect("unban not found");
        let (_from, text) = decode_pvt(msg);
        assert_eq!(text, "No matching ban found.");
    }

    #[test]
    fn builtin_banlist_reports_entries() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        let _ = next_pvt_text(&mut alice_rx); // ban ack
        let _ = next_pvt_text(&mut alice_rx); // anuncio "Bob was banned by Alice"

        let (handled, _) = dispatch_builtin(&ctx, &alice, "banlist", "");
        assert!(handled);

        let t1 = next_pvt_text(&mut alice_rx);
        assert_eq!(t1, "Active bans:");

        let t2 = next_pvt_text(&mut alice_rx);
        assert!(t2.contains("name='Bob'"));
        assert!(t2.contains("ip=10.0.0.2"));
    }

    fn make_test_ctx_with(settings: Settings) -> Arc<AppContext> {
        let db = Database::in_memory().expect("in-memory db");
        Arc::new(AppContext::new(settings, db))
    }

    #[test]
    fn builtin_kick_requires_moderator() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let (handled, _) = dispatch_builtin(&ctx, &alice, "kick", "Bob");
        assert!(handled);
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Moderator+ required.");
        assert!(ctx.user_pool.get_by_name("Bob").is_some());
    }

    #[test]
    fn builtin_kick_removes_target() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "kick", "Bob");
        assert!(handled);
        assert!(ctx.user_pool.get_by_name("Bob").is_none());
        assert!(!ctx.bans.is_banned(&bob.guid, bob.external_ip), "kick must not ban");
        assert_eq!(next_pvt_text(&mut bob_rx), "You have been kicked from this room.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Kicked 'Bob'.");
    }

    #[test]
    fn builtin_kick_cannot_target_equal_level() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        *bob.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let _ = dispatch_builtin(&ctx, &alice, "kick", "Bob");
        assert_eq!(
            next_pvt_text(&mut alice_rx),
            "You cannot kick a user of equal or higher level."
        );
        assert!(ctx.user_pool.get_by_name("Bob").is_some());
    }

    #[test]
    fn builtin_muzzle_and_unmuzzle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "muzzle", "Bob");
        assert!(bob.muzzled.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut bob_rx), "You have been muzzled.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Muzzled 'Bob'.");
        // Anuncio público a toda la sala (AdminAction#3 de sb0t).
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob was muzzled by Alice");
        assert_eq!(next_pvt_text(&mut bob_rx), "Bob was muzzled by Alice");

        // Muzzle repetido no cambia nada
        let _ = dispatch_builtin(&ctx, &alice, "muzzle", "Bob");
        assert_eq!(next_pvt_text(&mut alice_rx), "'Bob' is already muzzled.");

        let _ = dispatch_builtin(&ctx, &alice, "unmuzzle", "Bob");
        assert!(!bob.muzzled.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut bob_rx), "You have been unmuzzled.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Unmuzzled 'Bob'.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob was unmuzzled by Alice");
    }

    #[test]
    fn builtin_muzzle_requires_outrank() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        *bob.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "muzzle", "Bob");
        assert_eq!(
            next_pvt_text(&mut alice_rx),
            "You cannot muzzle a user of equal or higher level."
        );
        assert!(!bob.muzzled.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn builtin_pmall_requires_admin() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "pmall", "hola");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");
    }

    #[test]
    fn builtin_pmall_sends_to_everyone_else() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        let (carol, mut carol_rx) = make_test_user(3, "Carol");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);
        ctx.user_pool.add(carol);

        let _ = dispatch_builtin(&ctx, &alice, "pmall", "hola a todos");

        let (from_bob, text_bob) = decode_pvt(bob_rx.try_recv().expect("bob pm"));
        assert_eq!(from_bob, "Alice");
        assert_eq!(text_bob, "hola a todos");
        let (_, text_carol) = decode_pvt(carol_rx.try_recv().expect("carol pm"));
        assert_eq!(text_carol, "hola a todos");
        assert_eq!(next_pvt_text(&mut alice_rx), "PM sent to 2 user(s).");
    }

    #[test]
    fn builtin_opmsg_reaches_only_ops() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        let (carol, mut carol_rx) = make_test_user(3, "Carol");
        *alice.level.write() = ILevel::Moderator;
        *carol.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);
        ctx.user_pool.add(carol);

        let _ = dispatch_builtin(&ctx, &alice, "opmsg", "reunión ya");

        assert_eq!(next_pvt_text(&mut alice_rx), "[ops] Alice: reunión ya");
        assert_eq!(next_pvt_text(&mut carol_rx), "[ops] Alice: reunión ya");
        assert!(bob_rx.try_recv().is_err(), "regular user must not receive opmsg");
    }

    #[test]
    fn builtin_uptime_and_version_report() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "uptime", "");
        assert!(next_pvt_text(&mut alice_rx).starts_with("Uptime: 0d 0h 0m"));
        assert!(next_pvt_text(&mut alice_rx).starts_with("Users: 1 online"));

        let _ = dispatch_builtin(&ctx, &alice, "version", "");
        let v = next_pvt_text(&mut alice_rx);
        assert!(v.starts_with("Astra v"), "got: {}", v);
    }

    #[test]
    fn builtin_register_and_login_flow() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());

        // Password muy corto
        let _ = dispatch_builtin(&ctx, &alice, "register", "abc");
        assert_eq!(next_pvt_text(&mut alice_rx), "Usage: /register <password> (4+ chars)");

        // Registro OK
        let _ = dispatch_builtin(&ctx, &alice, "register", "secret1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Account registered. Use /login <password>.");
        assert!(ctx.accounts.find_by_guid(&alice.guid).unwrap().is_some());

        // Doble registro rechazado
        let _ = dispatch_builtin(&ctx, &alice, "register", "secret1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Already registered. Use /unregister first.");

        // Login con password incorrecto
        let _ = dispatch_builtin(&ctx, &alice, "login", "wrong");
        assert_eq!(next_pvt_text(&mut alice_rx), "Invalid password.");

        // Login correcto: sube de Anonymous a Regular (nivel de la cuenta)
        let (_, events) = dispatch_builtin(&ctx, &alice, "login", "secret1");
        assert_eq!(events.len(), 1);
        assert_eq!(*alice.level.read() as u8, ILevel::Regular as u8);
        assert_eq!(next_pvt_text(&mut alice_rx), "Logged in (level 1).");

        // Segundo login: mismo nivel → sin cambio, pero con feedback
        let (_, events) = dispatch_builtin(&ctx, &alice, "login", "secret1");
        assert!(events.is_empty());
        assert_eq!(next_pvt_text(&mut alice_rx), "Logged in (level unchanged).");

        // Unregister
        let _ = dispatch_builtin(&ctx, &alice, "unregister", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Account deleted.");
        assert!(ctx.accounts.find_by_guid(&alice.guid).unwrap().is_none());
    }

    #[test]
    fn builtin_register_disabled_rejects() {
        let settings = Settings {
            allow_registration: false,
            ..Settings::default()
        };
        let ctx = make_test_ctx_with(settings);
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "register", "secret1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Registration is disabled in this room.");
    }

    #[test]
    fn builtin_login_owner_password_grants_owner() {
        let settings = Settings {
            owner_password: "ownerpw".to_string(),
            ..Settings::default()
        };
        let ctx = make_test_ctx_with(settings);
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());

        let (_, events) = dispatch_builtin(&ctx, &alice, "login", "ownerpw");
        assert_eq!(*alice.level.read() as u8, ILevel::Owner as u8);
        assert_eq!(events.len(), 1);
        assert_eq!(next_pvt_text(&mut alice_rx), "Logged in as Owner.");
    }

    #[test]
    fn builtin_login_restores_account_level() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());
        ctx.accounts
            .register("Alice", &alice.guid, "secret1", ILevel::Moderator as u8)
            .unwrap();

        let (_, events) = dispatch_builtin(&ctx, &alice, "login", "secret1");
        assert_eq!(*alice.level.read() as u8, ILevel::Moderator as u8);
        assert_eq!(events.len(), 1);
        assert_eq!(next_pvt_text(&mut alice_rx), "Logged in (level 50).");
    }

    #[test]
    fn builtin_grant_sets_level_and_persists() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());
        ctx.accounts
            .register("Bob", &bob.guid, "bobpw", ILevel::Regular as u8)
            .unwrap();

        let (_, events) = dispatch_builtin(&ctx, &alice, "grant", "Bob moderator");
        assert_eq!(*bob.level.read() as u8, ILevel::Moderator as u8);
        assert_eq!(events.len(), 1, "AdminLevelChanged expected");

        // Persistido en la cuenta
        let acc = ctx.accounts.find_by_guid(&bob.guid).unwrap().unwrap();
        assert_eq!(acc.level, ILevel::Moderator as u8);

        assert_eq!(next_pvt_text(&mut bob_rx), "Your level is now 50 (moderator).");
        assert_eq!(next_pvt_text(&mut alice_rx), "'Bob' is now level 50 (moderator).");
    }

    #[test]
    fn builtin_grant_cannot_reach_own_level() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (_, events) = dispatch_builtin(&ctx, &alice, "grant", "Bob admin");
        assert!(events.is_empty());
        assert_eq!(
            next_pvt_text(&mut alice_rx),
            "You cannot grant a level equal or above your own."
        );
        assert_eq!(*bob.level.read() as u8, ILevel::Anonymous as u8);
    }

    #[test]
    fn builtin_grant_requires_admin() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let (_, events) = dispatch_builtin(&ctx, &alice, "grant", "Bob voice");
        assert!(events.is_empty());
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");
    }

    #[test]
    fn builtin_revoke_resets_level() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        *bob.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (_, events) = dispatch_builtin(&ctx, &alice, "revoke", "Bob");
        assert_eq!(*bob.level.read() as u8, ILevel::Regular as u8);
        assert_eq!(events.len(), 1);
        assert_eq!(next_pvt_text(&mut bob_rx), "Your level has been reset to regular.");
        assert_eq!(next_pvt_text(&mut alice_rx), "'Bob' is now a regular user.");
    }

    #[test]
    fn builtin_whois_reports_user_info() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        // whois es Moderator+ en sb0t.
        *alice.level.write() = ILevel::Moderator;
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *bob.level.write() = ILevel::Voice;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "whois", "Bob");
        assert!(handled);

        // Formato multilínea de sb0t (Category.Whois).
        assert_eq!(next_pvt_text(&mut alice_rx), "Name: Bob");
        assert_eq!(next_pvt_text(&mut alice_rx), "External IP: 10.0.0.2");
        assert_eq!(next_pvt_text(&mut alice_rx), "Local IP: 10.0.0.2");
        let _dataport = next_pvt_text(&mut alice_rx);
        let _version = next_pvt_text(&mut alice_rx);
        assert_eq!(next_pvt_text(&mut alice_rx), "Vroom: 0");
        assert_eq!(next_pvt_text(&mut alice_rx), "ID: 2");
        assert_eq!(next_pvt_text(&mut alice_rx), "Registered: False");
        // Línea extra de Astra con level/files/guid.
        let extra = next_pvt_text(&mut alice_rx);
        assert!(extra.contains("Level: 2"), "extra: {extra}");
        assert!(extra.contains("guid: 02020202020202020202020202020202"), "extra: {extra}");
    }

    #[test]
    fn builtin_greets_require_admin() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "addgreet", "hola +n");
        assert!(handled);
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");
        assert!(ctx.greets.is_empty());
    }

    #[test]
    fn builtin_greet_add_list_remove() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "addgreet", "welcome +n to +rn");
        assert_eq!(next_pvt_text(&mut alice_rx), "Greet #0 added.");
        assert_eq!(ctx.greets.list(), vec!["welcome +n to +rn"]);

        let _ = dispatch_builtin(&ctx, &alice, "listgreets", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Greets (1):");
        assert_eq!(next_pvt_text(&mut alice_rx), "0 - welcome +n to +rn");

        let _ = dispatch_builtin(&ctx, &alice, "remgreet", "0");
        assert_eq!(next_pvt_text(&mut alice_rx), "Removed greet: welcome +n to +rn");
        assert!(ctx.greets.is_empty());

        let _ = dispatch_builtin(&ctx, &alice, "remgreet", "5");
        assert_eq!(next_pvt_text(&mut alice_rx), "No greet at that index.");
    }

    #[test]
    fn builtin_greets_toggle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "greets", "off");
        assert_eq!(next_pvt_text(&mut alice_rx), "Greets disabled.");
        assert!(!ctx.greets.is_enabled());

        let _ = dispatch_builtin(&ctx, &alice, "greets", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Greets are off (0 configured).");
    }

    #[test]
    fn builtin_filter_add_list_remove() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "addfilter", "badword ban");
        assert_eq!(next_pvt_text(&mut alice_rx), "Filter 'badword' → ban added.");
        assert_eq!(
            ctx.word_filter.check("this is a badword").unwrap(),
            server_core::FilterAction::Ban
        );

        // Sin acción explícita → block
        let _ = dispatch_builtin(&ctx, &alice, "addfilter", "spammy");
        assert_eq!(next_pvt_text(&mut alice_rx), "Filter 'spammy' → block added.");

        let _ = dispatch_builtin(&ctx, &alice, "listfilters", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Word filters (2):");
        // Dos líneas de detalle (ordenadas por patrón): badword, spammy
        assert_eq!(next_pvt_text(&mut alice_rx), "0 - badword → ban");
        assert_eq!(next_pvt_text(&mut alice_rx), "1 - spammy → block");

        let _ = dispatch_builtin(&ctx, &alice, "remfilter", "badword");
        assert_eq!(next_pvt_text(&mut alice_rx), "Filter 'badword' removed.");
        assert!(ctx.word_filter.check("badword").is_none());

        let _ = dispatch_builtin(&ctx, &alice, "remfilter", "nope");
        assert_eq!(next_pvt_text(&mut alice_rx), "No matching filter.");
    }

    #[test]
    fn builtin_filter_requires_admin() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "addfilter", "x ban");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");
        assert!(ctx.word_filter.is_empty());
    }

    #[test]
    fn builtin_url_add_list_remove() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "addurl", "https://astra.dev Astra site");
        assert_eq!(next_pvt_text(&mut alice_rx), "URL #0 added.");
        let list = ctx.urls.list();
        assert_eq!(list[0].address, "https://astra.dev");
        assert_eq!(list[0].text, "Astra site");

        let _ = dispatch_builtin(&ctx, &alice, "listurl", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room URLs (1):");
        assert_eq!(next_pvt_text(&mut alice_rx), "0 - Astra site [https://astra.dev]");

        let _ = dispatch_builtin(&ctx, &alice, "remurl", "0");
        assert_eq!(next_pvt_text(&mut alice_rx), "Removed URL: Astra site");
        assert!(ctx.urls.is_empty());
    }

    #[test]
    fn builtin_history_is_a_host_toggle() {
        // Paridad sb0t Eval.History: `/history on|off` togglea el flag de
        // sala; el replay ocurre al entrar (AppContext::replay_history).
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());

        assert!(!ctx.room_flags.get("history"));
        let _ = dispatch_builtin(&ctx, &alice, "history", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flag 'history' enabled.");
        assert!(ctx.room_flags.get("history"));
        let _ = dispatch_builtin(&ctx, &alice, "history", "off");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flag 'history' disabled.");
        assert!(!ctx.room_flags.get("history"));
    }

    #[test]
    fn builtin_history_requires_host() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        let _ = dispatch_builtin(&ctx, &alice, "history", "on");
        let reply = next_pvt_text(&mut alice_rx);
        assert!(reply.contains("denied") || reply.contains("required"), "reply: {reply}");
        assert!(!ctx.room_flags.get("history"));
    }

    #[test]
    fn builtin_redirect_accepts_arlnk_hashlink() {
        let ctx = make_test_ctx();
        let (alice, mut a_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let link = server_core::hashlink::encode(&server_core::hashlink::HashlinkRoom {
            ip: "203.0.113.9".parse().unwrap(),
            port: 2500,
            name: "Otra Sala".to_string(),
        });
        let _ = dispatch_builtin(&ctx, &alice, "redirect", &format!("Bob arlnk://{}", link));

        // Bob recibe el paquete de redirect al ip:port decodificado.
        let pkt = bob_rx.try_recv().expect("redirect pkt");
        assert_eq!(pkt[0], TcpMsg::ServerRedirect as u8);
        // Confirmación al emisor + anuncio público con el nombre de la sala.
        assert!(next_pvt_text(&mut a_rx).starts_with("Redirected 'Bob' to 203.0.113.9:2500"));
        assert_eq!(
            next_pvt_text(&mut a_rx),
            "Bob has been redirected to Otra Sala by Alice"
        );
    }

    #[test]
    fn builtin_shout_uses_sb0t_format() {
        let ctx = make_test_ctx();
        let (alice, mut a_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "shout", "hola sala");
        assert_eq!(next_pvt_text(&mut bob_rx), "Alice> [SHOUT] hola sala");
        assert_eq!(next_pvt_text(&mut a_rx), "Alice> [SHOUT] hola sala");

        // Regular sin flag general: silencio.
        ctx.room_flags.set("general", false);
        let _ = dispatch_builtin(&ctx, &bob, "shout", "yo tambien");
        assert!(a_rx.try_recv().is_err());
    }

    #[test]
    fn builtin_stats_prints_sb0t_block() {
        let ctx = make_test_ctx();
        let (alice, mut a_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "stats", "");
        let header = next_pvt_text(&mut a_rx);
        assert!(header.starts_with("Stats for "), "header: {header}");
        let _blank = next_pvt_text(&mut a_rx);
        assert!(next_pvt_text(&mut a_rx).starts_with("Uptime: "));
        assert!(next_pvt_text(&mut a_rx).starts_with("Bytes received: "));
    }

    #[test]
    fn builtin_customname_target_form_and_selfservice() {
        let ctx = make_test_ctx();
        let (alice, mut a_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        let (bob, _b_rx) = make_test_user(2, "Bob");
        let (carol, _c_rx) = make_test_user(3, "Carol");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());
        ctx.user_pool.add(carol.clone());

        // Mod fija el custom name de otro usuario + anuncio público.
        let _ = dispatch_builtin(&ctx, &alice, "customname", "Bob Bobby");
        assert_eq!(bob.custom_name.read().clone(), Some("Bobby".to_string()));
        assert_eq!(
            next_pvt_text(&mut a_rx),
            "Bob's custom name has been set by Alice"
        );

        // Y lo limpia con uncustomname <nick>.
        let _ = dispatch_builtin(&ctx, &alice, "uncustomname", "Bob");
        assert_eq!(bob.custom_name.read().clone(), None);

        // Regular sin flag `general`: self-service denegado en silencio.
        ctx.room_flags.set("general", false);
        let _ = dispatch_builtin(&ctx, &carol, "customname", "Cazadora");
        assert_eq!(carol.custom_name.read().clone(), None);

        // Con `general` on pero custom names deshabilitados en la sala
        // (default sb0t `customnames`=false): self-service sigue bloqueado.
        ctx.room_flags.set("general", true);
        let _ = dispatch_builtin(&ctx, &carol, "customname", "Cazadora");
        assert_eq!(carol.custom_name.read().clone(), None);

        // `#customnames on` (Host, paridad Eval.cs:919) habilita y el regular
        // ya puede. Alice (Mod) no alcanza: hace falta un Owner.
        let (host, _h_rx) = make_test_user(4, "Hosty");
        *host.level.write() = ILevel::Owner;
        ctx.user_pool.add(host.clone());
        let _ = dispatch_builtin(&ctx, &host, "customnames", "on");
        assert!(ctx.room_flags.get("customnames"));
        let _ = dispatch_builtin(&ctx, &carol, "customname", "Cazadora");
        assert_eq!(carol.custom_name.read().clone(), Some("Cazadora".to_string()));

        // Substrings vetados (paridad sb0t): no se aplican.
        let _ = dispatch_builtin(&ctx, &carol, "customname", "visit www.spam.com");
        assert_eq!(carol.custom_name.read().clone(), Some("Cazadora".to_string()));
    }

    #[test]
    fn builtin_addautologin_uses_sb0t_scale_and_pushes_level() {
        let ctx = make_test_ctx();
        let (owner, mut owner_rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(owner.clone());
        ctx.user_pool.add(bob.clone());

        // Escala sb0t: 1 = MODERATOR (no "regular", como interpretaba
        // /grant). Este era el bug: `#addautologin Bob 1` dejaba a Bob en
        // Regular y, al no cambiar el nivel, no se le enviaba nada.
        let (handled, _) = dispatch_builtin(&ctx, &owner, "addautologin", "Bob 1");
        assert!(handled);
        assert_eq!(*bob.level.read(), ILevel::Moderator, "1 debe ser moderator");

        // Bob debe recibir el paquete de op-change (el "paquete de
        // actualización de login") además del aviso.
        let mut got_opchange = false;
        while let Ok(pkt) = bob_rx.try_recv() {
            if pkt[0] == TcpMsg::ServerOpChange as u8 {
                got_opchange = true;
            }
        }
        assert!(got_opchange, "Bob no recibió el paquete de op-change");

        // Y la sala ve el anuncio (AdminLogin#4 de sb0t).
        let mut lines = Vec::new();
        while let Ok(pkt) = owner_rx.try_recv() {
            if pkt[0] == TcpMsg::Pmt as u8 {
                let (_f, t) = decode_pvt(pkt);
                lines.push(t);
            }
        }
        assert!(
            lines.iter().any(|t| t == "Bob has been added to auto login as a level 1 admin"),
            "no se anunció el autologin a la sala; llegó: {:?}",
            lines
        );

        // Persistido con el nivel correcto → se restaura al reingresar.
        let level = ctx
            .ip_autologins
            .get_level(&bob.guid, bob.external_ip)
            .expect("entrada de autologin");
        assert_eq!(level, ILevel::Moderator);

        // 2 = admin, 3 = host (escala sb0t).
        let _ = dispatch_builtin(&ctx, &owner, "addautologin", "Bob 2");
        assert_eq!(*bob.level.read(), ILevel::Admin);
        let _ = dispatch_builtin(&ctx, &owner, "addautologin", "Bob 3");
        assert_eq!(*bob.level.read(), ILevel::Owner);
    }

    #[test]
    fn dispatch_autologin_restores_level_on_rejoin() {
        // Paridad sb0t `Joined()`: al entrar, el nivel del autologin se
        // aplica (y se le manda el op-change), sin depender de que el
        // cliente mande el opcode ClientAutologin.
        let ctx = make_test_ctx();
        let (owner, _o_rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        let (bob, _b_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(owner.clone());
        ctx.user_pool.add(bob.clone());
        let _ = dispatch_builtin(&ctx, &owner, "addautologin", "Bob 2");

        // Bob se reconecta (mismo guid/IP, nueva sesión regular).
        ctx.user_pool.remove(bob.id);
        let (bob2, mut bob2_rx) = make_test_user(2, "Bob");
        assert!((*bob2.level.read() as u8) < ILevel::Moderator as u8);
        ctx.user_pool.add(bob2.clone());

        assert!(dispatch_autologin(&ctx, &bob2), "el autologin debió aplicarse");
        assert_eq!(*bob2.level.read(), ILevel::Admin, "nivel no restaurado al reingresar");

        let mut got_opchange = false;
        while let Ok(pkt) = bob2_rx.try_recv() {
            if pkt[0] == TcpMsg::ServerOpChange as u8 {
                got_opchange = true;
            }
        }
        assert!(got_opchange, "no se envió el op-change al reingresar");
    }

    #[test]
    fn builtin_setlevel_uses_sb0t_scale() {
        let ctx = make_test_ctx();
        let (owner, mut owner_rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(owner.clone());
        ctx.user_pool.add(bob.clone());

        // 1 = moderator, 2 = admin, 3 = owner, 0 = regular (escala sb0t).
        let (_, events) = dispatch_builtin(&ctx, &owner, "setlevel", "Bob 2");
        assert!(matches!(
            events.as_slice(),
            [astra_scripting::ScriptEvent::AdminLevelChanged { name }] if name == "Bob"
        ));
        assert_eq!(*bob.level.read(), ILevel::Admin);
        let _ = owner_rx.try_recv();

        let (_, _) = dispatch_builtin(&ctx, &owner, "setlevel", "Bob 0");
        assert_eq!(*bob.level.read(), ILevel::Regular);

        // No-Owner: denegado.
        *bob.level.write() = ILevel::Admin;
        let (_, events) = dispatch_builtin(&ctx, &bob, "setlevel", "Owner 0");
        assert!(events.is_empty());
        assert_eq!(*owner.level.read(), ILevel::Owner);
    }

    #[test]
    fn builtin_logout_resets_session_level() {
        let ctx = make_test_ctx();
        let (alice, _rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let (handled, events) = dispatch_builtin(&ctx, &alice, "logout", "");
        assert!(handled);
        assert!(matches!(
            events.as_slice(),
            [astra_scripting::ScriptEvent::Logout { name }] if name == "Alice"
        ));
        assert_eq!(*alice.level.read(), ILevel::Regular);

        // Sin sesión elevada: logoff no hace nada.
        let (_, events) = dispatch_builtin(&ctx, &alice, "logoff", "");
        assert!(events.is_empty());
    }

    #[test]
    fn builtin_kewltext_sb0t_aliases_work() {
        let ctx = make_test_ctx();
        let (alice, _rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "addkewltext", "Bob");
        assert!(handled);
        let (handled, _) = dispatch_builtin(&ctx, &alice, "remkewltext", "Bob");
        assert!(handled);
    }

    #[test]
    fn builtin_adminannounce_is_separate_toggle() {
        let ctx = make_test_ctx();
        let (owner, mut rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        ctx.user_pool.add(owner.clone());

        assert!(!ctx.room_flags.get("adminannounce"));
        let _ = dispatch_builtin(&ctx, &owner, "adminannounce", "on");
        assert_eq!(next_pvt_text(&mut rx), "Room flag 'adminannounce' enabled.");
        assert!(ctx.room_flags.get("adminannounce"));
    }

    #[test]
    fn builtin_greetmsg_and_pmgreetmsg_are_separate_flags() {
        let ctx = make_test_ctx();
        let (owner, mut rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        ctx.user_pool.add(owner.clone());

        // pmgreetmsg default on (comportamiento histórico); greetmsg off.
        assert!(ctx.room_flags.get("pmgreetmsg"));
        assert!(!ctx.room_flags.get("greetmsg"));
        let _ = dispatch_builtin(&ctx, &owner, "pmgreetmsg", "off");
        assert_eq!(next_pvt_text(&mut rx), "Room flag 'pmgreetmsg' disabled.");
        let _ = dispatch_builtin(&ctx, &owner, "greetmsg", "on");
        assert_eq!(next_pvt_text(&mut rx), "Room flag 'greetmsg' enabled.");
        // addgreetmsg sigue siendo "agregar greet", no un toggle.
        let _ = dispatch_builtin(&ctx, &owner, "addgreetmsg", "hola +n");
        assert_eq!(ctx.greets.len(), 1);
    }

    #[test]
    fn builtin_rempassword_removes_account_of_other_user() {
        let ctx = make_test_ctx();
        let (owner, mut rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(owner.clone());
        ctx.user_pool.add(bob.clone());

        // Bob se registra.
        let _ = dispatch_builtin(&ctx, &bob, "register", "secreto1");
        assert_eq!(ctx.db.list_accounts().unwrap().len(), 1);

        // El Host lo borra por nick.
        let _ = dispatch_builtin(&ctx, &owner, "rempassword", "Bob");
        assert_eq!(next_pvt_text(&mut rx), "Password removed for 'Bob'.");
        assert!(ctx.db.list_accounts().unwrap().is_empty());
    }

    #[test]
    fn builtin_idle_marks_user_and_announces_when_flag_on() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());
        ctx.room_flags.set("idle", true);

        // Marcarse ausente: dispara onIdled y anuncia a la sala.
        let (handled, events) = dispatch_builtin(&ctx, &alice, "idles", "");
        assert!(handled);
        assert!(matches!(
            events.as_slice(),
            [astra_scripting::ScriptEvent::Idled { name }] if name == "Alice"
        ));
        assert!(ctx.idle.is_idle(alice.id));
        let announce = next_pvt_text(&mut bob_rx);
        assert!(announce.starts_with("Alice idles at "), "announce: {announce}");

        // Ya idle: reintento silencioso, sin evento.
        let (_, events) = dispatch_builtin(&ctx, &alice, "idle", "");
        assert!(events.is_empty());

        // Unidle al hablar: anuncia el tiempo ausente.
        assert!(ctx.unidle_user(&alice).is_some());
        let ret = next_pvt_text(&mut bob_rx);
        assert!(ret.starts_with("Alice returned at "), "ret: {ret}");
        assert!(ret.contains("away time ["), "ret: {ret}");
        // Cooldown de 5 min: no puede volver a idlear ya mismo.
        let (_, events) = dispatch_builtin(&ctx, &alice, "idles", "");
        assert!(events.is_empty());
        let _ = alice_rx.try_recv();
    }

    #[test]
    fn builtin_idle_toggle_is_host_only() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "idle", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Host required.");
        assert!(!ctx.room_flags.get("idle"));

        *alice.level.write() = ILevel::Owner;
        let _ = dispatch_builtin(&ctx, &alice, "idle", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flag 'idle' enabled.");
        assert!(ctx.room_flags.get("idle"));
    }

    #[test]
    fn builtin_info_lists_all_users_with_ids() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        let (carol, _carol_rx) = make_test_user(3, "Carol");
        carol.cloaked.store(true, std::sync::atomic::Ordering::Relaxed);
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());
        ctx.user_pool.add(carol.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "info", "");
        assert!(handled);
        // Encabezado = nombre de la sala, luego línea en blanco.
        let header = next_pvt_text(&mut alice_rx);
        assert_eq!(header, ctx.settings.room_name);
        let _blank = next_pvt_text(&mut alice_rx);
        // Una línea por usuario visible (Carol está cloaked → excluida).
        let mut lines = vec![next_pvt_text(&mut alice_rx), next_pvt_text(&mut alice_rx)];
        lines.sort();
        assert_eq!(lines[0], "Alice [vroom: 0] [id: 1]");
        assert_eq!(lines[1], "Bob [vroom: 0] [id: 2]");
        assert!(alice_rx.try_recv().is_err(), "no debe listar cloaked");
    }

    #[test]
    fn replay_history_sends_messages_with_age_prefix() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.record_message("Bob", "hola a todos", false);
        ctx.record_message("Carol", "waves", true);

        // Flag apagado: no se manda nada.
        ctx.replay_history(&alice);
        assert!(alice_rx.try_recv().is_err());

        ctx.room_flags.set("history", true);
        ctx.replay_history(&alice);
        // Público de Bob con prefijo de antigüedad.
        let pkt = alice_rx.try_recv().expect("public de Bob");
        assert_eq!(pkt[0], TcpMsg::Public as u8);
        let mut r = PacketReader::new(&pkt[1..]);
        assert_eq!(r.read_string_nt().unwrap(), "Bob");
        let text = r.read_string_nt().unwrap();
        assert!(text.starts_with("[-00:00:0"), "text: {text}");
        assert!(text.ends_with("] hola a todos"), "text: {text}");
        // Emote de Carol.
        let pkt = alice_rx.try_recv().expect("emote de Carol");
        assert_eq!(pkt[0], TcpMsg::Emote as u8);
        // Línea de cierre (PM del bot).
        let closing = next_pvt_text(&mut alice_rx);
        assert_eq!(closing, "-=-=-=-=- end of chat history -=-=-=-=-");
    }

    #[test]
    fn builtin_lastseen_online_and_history() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        // Bob online
        let _ = dispatch_builtin(&ctx, &alice, "lastseen", "Bob");
        assert_eq!(next_pvt_text(&mut alice_rx), "'Bob' is online now.");

        // Usuario en historial (offline)
        ctx.db
            .add_user_history("Ghost", "Ares 2.5", &[7u8; 16],
                "5.6.7.8".parse().unwrap(), "5.6.7.8".parse().unwrap(), 1234, 1000)
            .unwrap();
        let _ = dispatch_builtin(&ctx, &alice, "lastseen", "Ghost");
        let line = next_pvt_text(&mut alice_rx);
        assert!(line.starts_with("'Ghost' [5.6.7.8] last seen "), "got: {}", line);

        // No existe
        let _ = dispatch_builtin(&ctx, &alice, "lastseen", "nadie");
        assert_eq!(next_pvt_text(&mut alice_rx), "No matching history.");
    }

    #[test]
    fn builtin_whowas_searches_history() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        ctx.db
            .add_user_history("Charlie", "Ares 2.5", &[9u8; 16],
                "9.9.9.9".parse().unwrap(), "9.9.9.9".parse().unwrap(), 5009, 2000)
            .unwrap();

        let _ = dispatch_builtin(&ctx, &alice, "whowas", "char");
        // Formato sb0t WhoWas#0: "whowas: +n +ip +v +t".
        let line = next_pvt_text(&mut alice_rx);
        assert!(line.starts_with("whowas: Charlie 9.9.9.9 Ares 2.5 "), "got: {}", line);

        // Sin resultados: WhoWas#1.
        let _ = dispatch_builtin(&ctx, &alice, "whowas", "nadie");
        assert_eq!(
            next_pvt_text(&mut alice_rx),
            "no results were found containing nadie"
        );
    }

    #[test]
    fn builtin_roominfo_and_status() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "roominfo", "");
        // Bloque con los textos de sb0t (RoomInfo #0-5).
        assert_eq!(next_pvt_text(&mut alice_rx), "Room Information");
        let _blank = next_pvt_text(&mut alice_rx);
        assert_eq!(next_pvt_text(&mut alice_rx), "Current hosts: 1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Current user count: 1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Current admin count: 1");
        assert!(next_pvt_text(&mut alice_rx).starts_with("Server uptime: "));
        assert!(next_pvt_text(&mut alice_rx).starts_with("Host status:"));

        // set status → confirma + anuncia la actualización (RoomInfo#6)
        let _ = dispatch_builtin(&ctx, &alice, "status", "under maintenance");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room status set to 'under maintenance'.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Alice has updated the host status");
        let _ = dispatch_builtin(&ctx, &alice, "status", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Status: under maintenance");
    }

    #[test]
    fn builtin_id_and_customnames() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        *bob.custom_name.write() = Some("BobbyTables".to_string());
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "id", "Bob");
        assert_eq!(next_pvt_text(&mut alice_rx), "'Bob' has id 2.");

        let _ = dispatch_builtin(&ctx, &alice, "customnames", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob → BobbyTables");
    }

    #[test]
    fn builtin_ipsend_subscribe_and_dump() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "ipsend", "on");
        assert!(alice.sub_ipsend.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "ipsend feed enabled.");
        // Volcado inicial: incluye a Bob
        let mut found_bob = false;
        while let Ok(pkt) = alice_rx.try_recv() {
            if pkt[0] == TcpMsg::Pmt as u8 {
                let (_f, t) = decode_pvt(pkt);
                if t.contains("IPSEND: Bob 10.0.0.2") {
                    found_bob = true;
                }
            }
        }
        assert!(found_bob);
    }

    #[test]
    fn builtin_bansend_notifies_subscribers() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "bansend", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "bansend feed enabled.");

        // Alice banea a Bob → recibe el aviso bansend + el ack del ban.
        let _ = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        let mut got_bansend = false;
        while let Ok(pkt) = alice_rx.try_recv() {
            if pkt[0] == TcpMsg::Pmt as u8 {
                let (_f, t) = decode_pvt(pkt);
                if t.starts_with("BANSEND: Alice banned Bob") {
                    got_bansend = true;
                }
            }
        }
        assert!(got_bansend);
    }

    #[test]
    fn urlencode_escapes_correctly() {
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencode("plain"), "plain");
    }

    #[test]
    fn builtin_define_usage_and_access() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());

        // Regular user → denegado
        let _ = dispatch_builtin(&ctx, &alice, "define", "word");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Moderator+ required.");

        // Moderator sin args → usage
        *alice.level.write() = ILevel::Moderator;
        let _ = dispatch_builtin(&ctx, &alice, "define", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Usage: /define <word>");
    }

    #[tokio::test]
    async fn define_spawns_and_pms_result() {
        // Con runtime tokio, /define agenda un fetch. No dependemos del
        // servicio real: solo verificamos que no panica y que el comando se
        // marca como handled. (El PM del resultado llega async y puede fallar
        // si no hay red; eso no rompe el test.)
        let ctx = make_test_ctx();
        let (alice, _rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        let (handled, _) = dispatch_builtin(&ctx, &alice, "define", "xyzzy");
        assert!(handled);
    }

    #[test]
    fn builtin_trace_without_geoip_is_honest() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "trace", "8.8.8.8");
        assert!(next_pvt_text(&mut alice_rx).contains("requires a GeoIP database"));
    }

    #[test]
    fn builtin_vspy_toggle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "vspy", "");
        assert!(alice.sub_vspy.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "vspy feed enabled.");
        let _ = dispatch_builtin(&ctx, &alice, "vspy", "");
        assert!(!alice.sub_vspy.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn builtin_quarantine_list_and_release() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        bob.quarantined.store(true, std::sync::atomic::Ordering::Relaxed);
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "listquarantined", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Quarantined (1):");
        assert_eq!(next_pvt_text(&mut alice_rx), "0 - Bob");

        let _ = dispatch_builtin(&ctx, &alice, "unquarantine", "Bob");
        assert!(!bob.quarantined.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "Un-quarantined 'Bob'.");
    }

    #[test]
    fn builtin_listpasswords_owner_only() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin; // no alcanza (owner)
        ctx.user_pool.add(alice.clone());
        let _ = dispatch_builtin(&ctx, &alice, "listpasswords", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Owner required.");

        *alice.level.write() = ILevel::Owner;
        ctx.accounts.register("Bob", &[9u8; 16], "pw", ILevel::Moderator as u8).unwrap();
        let _ = dispatch_builtin(&ctx, &alice, "listpasswords", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Accounts (1):");
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob (level 50)");
    }

    #[test]
    fn builtin_joinfilter_add_and_match() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "joinfilter", "add spam*");
        assert_eq!(next_pvt_text(&mut alice_rx), "join filter added: spam*");
        assert!(ctx.join_filters.matches("SpamBot99"));
        assert!(!ctx.join_filters.matches("Alice"));

        let _ = dispatch_builtin(&ctx, &alice, "joinfilter", "list");
        assert_eq!(next_pvt_text(&mut alice_rx), "join filters (1):");
    }

    #[test]
    fn builtin_locate_and_clearscreen() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "locate", "Bob");
        assert!(next_pvt_text(&mut alice_rx).starts_with("'Bob' ip=10.0.0.2"));

        let _ = dispatch_builtin(&ctx, &alice, "clearscreen", "");
        // Muchas líneas en blanco + el ack final "Screen cleared."
        let last = {
            let mut t = String::new();
            while let Ok(pkt) = alice_rx.try_recv() {
                if pkt[0] == TcpMsg::Pmt as u8 {
                    let (_f, txt) = decode_pvt(pkt);
                    t = txt;
                }
            }
            t
        };
        assert_eq!(last, "screen cleared by Alice");
    }

    #[test]
    fn builtin_unavailable_commands_respond() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "loadtemplate", "");
        assert!(handled);
        assert!(next_pvt_text(&mut alice_rx).contains("built-in messages"));
    }

    #[test]
    fn host_commands_require_owner() {
        let ctx = make_test_ctx();
        let (admin, mut admin_rx) = make_test_user(1, "Admin");
        *admin.level.write() = ILevel::Admin; // Admin < Owner
        ctx.user_pool.add(admin.clone());
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(bob.clone());

        // Admin no alcanza el gate Owner (ahora aplicado centralizadamente
        // por `command_levels` antes de llegar a `require_host`).
        let (handled, _) = dispatch_builtin(&ctx, &admin, "hostban", "Bob");
        assert!(handled);
        assert!(next_pvt_text(&mut admin_rx).contains("Owner required"));

        // Como Owner, hostban banea a Bob.
        *admin.level.write() = ILevel::Owner;
        let (handled, _) = dispatch_builtin(&ctx, &admin, "hostban", "Bob");
        assert!(handled);
        assert!(ctx.bans.len() >= 1);
    }

    #[test]
    fn hostcban_clears_everything() {
        use std::sync::atomic::Ordering::Relaxed;
        let ctx = make_test_ctx();
        let (owner, mut owner_rx) = make_test_user(1, "Owner");
        *owner.level.write() = ILevel::Owner;
        ctx.user_pool.add(owner.clone());
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        bob.muzzled.store(true, Relaxed);
        ctx.user_pool.add(bob.clone());
        ctx.range_bans.add("10.0.0.");

        let (handled, _) = dispatch_builtin(&ctx, &owner, "hostcban", "");
        assert!(handled);
        assert!(!bob.muzzled.load(Relaxed));
        assert_eq!(ctx.range_bans.len(), 0);
        assert!(next_pvt_text(&mut owner_rx).contains("Cleared"));
    }

    #[test]
    fn builtin_text_effects_toggle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "lower", "Bob");
        assert!(bob.lowered.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "lower enabled for 'Bob'.");

        let _ = dispatch_builtin(&ctx, &alice, "kewltext", "Bob");
        assert!(bob.kewl.load(std::sync::atomic::Ordering::Relaxed));
        let _ = next_pvt_text(&mut alice_rx);

        let _ = dispatch_builtin(&ctx, &alice, "unlower", "Bob");
        assert!(!bob.lowered.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "lower disabled for 'Bob'.");

        let _ = dispatch_builtin(&ctx, &alice, "paint", "Bob");
        assert!(bob.painted.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn builtin_room_flags_toggle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        // caps default off
        let _ = dispatch_builtin(&ctx, &alice, "caps", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flag 'caps' enabled.");
        assert!(ctx.room_flags.get("caps"));

        let _ = dispatch_builtin(&ctx, &alice, "scribbles", "off");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flag 'scribbles' disabled.");
        assert!(!ctx.room_flags.get("scribbles"));

        let _ = dispatch_builtin(&ctx, &alice, "audios", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flag 'audios' is on.");
    }

    #[test]
    fn builtin_disableavatar_inverts_flag() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        assert!(ctx.room_flags.get("avatars"));
        let _ = dispatch_builtin(&ctx, &alice, "disableavatar", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "Avatars disabled.");
        assert!(!ctx.room_flags.get("avatars"));
    }

    #[test]
    fn builtin_roomflags_lists_all() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "roomflags", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room flags:");
        // Una línea por cada flag definido.
        let mut count = 0;
        while alice_rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, server_core::room_flags::FLAG_DEFAULTS.len());
    }

    #[test]
    fn builtin_cloak_toggles() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "cloak", "on");
        assert!(alice.cloaked.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "Cloak enabled.");
    }

    #[test]
    fn builtin_room_flag_requires_admin() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        let _ = dispatch_builtin(&ctx, &alice, "caps", "on");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");
    }

    #[test]
    fn builtin_move_changes_vroom() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "move", "Bob 5");
        assert_eq!(*bob.vroom.read(), 5);
        let _ = next_pvt_text(&mut alice_rx); // "You were moved..." goes to bob; alice gets ack
    }

    #[test]
    fn builtin_changename_renames() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "changename", "Bob Roberto");
        assert_eq!(*bob.name.read(), "Roberto");
        assert!(ctx.user_pool.get_by_name("Roberto").is_some());
        assert!(ctx.user_pool.get_by_name("Bob").is_none());
        assert_eq!(next_pvt_text(&mut alice_rx), "Renamed 'Bob' to 'Roberto'.");
    }

    #[test]
    fn builtin_admins_lists_ops() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        let (carol, _c_rx) = make_test_user(3, "Carol");
        *alice.level.write() = ILevel::Moderator;
        *bob.level.write() = ILevel::Admin;
        // carol regular
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);
        ctx.user_pool.add(carol);

        let _ = dispatch_builtin(&ctx, &alice, "admins", "");
        // Paridad sb0t: se difunde a TODA la sala con los textos AdminList.
        assert_eq!(next_pvt_text(&mut alice_rx), "ADMIN LIST REQUESTED BY [Alice]");
        assert_eq!(next_pvt_text(&mut alice_rx), "Level 80 : Bob");
        assert_eq!(next_pvt_text(&mut alice_rx), "Level 50 : Alice");
        assert_eq!(next_pvt_text(&mut alice_rx), "List Complete");
    }

    #[test]
    fn builtin_announce_broadcasts_public() {
        let ctx = make_test_ctx();
        let (alice, mut a_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let _ = dispatch_builtin(&ctx, &alice, "announce", "server reboot soon");
        // Alice (mod) ve el texto y el aviso de autor (Notification#19).
        assert_eq!(next_pvt_text(&mut a_rx), "server reboot soon");
        assert_eq!(next_pvt_text(&mut a_rx), "Alice announced");
        // Bob (regular) recibe el texto del server, sin el aviso de autor.
        assert_eq!(next_pvt_text(&mut bob_rx), "server reboot soon");
        assert!(bob_rx.try_recv().is_err(), "el aviso '+a announced' es solo para mods");
    }

    #[test]
    fn builtin_kiddy_and_echo_toggle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "kiddy", "Bob");
        assert!(bob.kiddied.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut alice_rx), "Kiddy mode on for 'Bob'.");

        let _ = dispatch_builtin(&ctx, &alice, "echo", "Bob you smell");
        assert_eq!(bob.echo_text.read().as_deref(), Some("you smell"));
        assert_eq!(next_pvt_text(&mut alice_rx), "Echo set on 'Bob'.");
        let _ = dispatch_builtin(&ctx, &alice, "echo", "Bob");
        assert!(bob.echo_text.read().is_none());
    }

    #[test]
    fn builtin_mtimeout_muzzles_temporarily() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "mtimeout", "Bob 60");
        assert!(bob.is_muzzled());
        assert!(bob.muzzle_until.load(std::sync::atomic::Ordering::Relaxed) > 0);
        let _ = next_pvt_text(&mut alice_rx); // bob notice; alice ack next
    }

    #[test]
    fn builtin_disableadmins_gate() {
        let ctx = make_test_ctx();
        let (owner, mut owner_rx) = make_test_user(1, "Owner");
        let (mod_u, mut mod_rx) = make_test_user(2, "Mod");
        *owner.level.write() = ILevel::Owner;
        *mod_u.level.write() = ILevel::Moderator;
        ctx.user_pool.add(owner.clone());
        ctx.user_pool.add(mod_u.clone());

        let _ = dispatch_builtin(&ctx, &owner, "disableadmins", "");
        assert_eq!(next_pvt_text(&mut owner_rx), "Admin commands disabled.");

        // Un moderador ya no puede usar comandos admin (silencioso, sb0t).
        let (handled, _) = dispatch_builtin(&ctx, &mod_u, "kiddy", "Owner");
        assert!(handled);
        assert!(mod_rx.try_recv().is_err(), "el gate es silencioso");

        // Pero sí comandos de usuario
        let (handled, _) = dispatch_builtin(&ctx, &mod_u, "help", "");
        assert!(handled);

        // El owner re-habilita
        let _ = dispatch_builtin(&ctx, &owner, "enableadmins", "");
        assert_eq!(next_pvt_text(&mut owner_rx), "Admin commands enabled.");
    }

    #[test]
    fn builtin_rangeban_add_check_remove() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "rangeban", "1.2.3.*");
        assert_eq!(next_pvt_text(&mut alice_rx), "Range ban added: 1.2.3.");
        assert!(ctx.range_bans.is_banned("1.2.3.55".parse().unwrap()));

        let _ = dispatch_builtin(&ctx, &alice, "listrangebans", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Range bans (1):");
        assert_eq!(next_pvt_text(&mut alice_rx), "0 - 1.2.3.");

        let _ = dispatch_builtin(&ctx, &alice, "rangeunban", "0");
        assert_eq!(next_pvt_text(&mut alice_rx), "Range ban removed.");
        assert!(!ctx.range_bans.is_banned("1.2.3.55".parse().unwrap()));
    }

    #[test]
    fn builtin_asnban_add_list_remove() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "asnban", "64500");
        assert_eq!(next_pvt_text(&mut alice_rx), "ASN 64500 banned.");
        assert!(ctx.asn_bans.is_banned(64500));

        let _ = dispatch_builtin(&ctx, &alice, "listasnbans", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "ASN bans (1): 64500");

        let _ = dispatch_builtin(&ctx, &alice, "asnunban", "64500");
        assert_eq!(next_pvt_text(&mut alice_rx), "ASN 64500 unbanned.");
        assert!(!ctx.asn_bans.is_banned(64500));
    }

    #[test]
    fn builtin_clearbans_and_banstats() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Owner;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        // Ban de Bob → registra acción
        let _ = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        let _ = next_pvt_text(&mut alice_rx); // "Banned 'Bob'..."
        let _ = next_pvt_text(&mut alice_rx); // anuncio público
        assert_eq!(ctx.bans.len(), 1);

        let _ = dispatch_builtin(&ctx, &alice, "banstats", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Active bans: 1 | recent actions: 1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Alice banned Bob [10.0.0.2]");

        let _ = dispatch_builtin(&ctx, &alice, "clearbans", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Cleared 1 ban(s).");
        assert_eq!(next_pvt_text(&mut alice_rx), "Alice has cleared the ban list");
        assert_eq!(ctx.bans.len(), 0);
    }

    #[test]
    fn builtin_rangeban_requires_admin() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        let _ = dispatch_builtin(&ctx, &alice, "rangeban", "1.2.3");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");
        assert!(ctx.range_bans.is_empty());
    }

    #[test]
    fn builtin_url_toggle_and_admin_gate() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        // Moderator no alcanza
        let _ = dispatch_builtin(&ctx, &alice, "addurl", "u t");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Admin+ required.");

        *alice.level.write() = ILevel::Owner;
        let _ = dispatch_builtin(&ctx, &alice, "url", "off");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room URLs disabled.");
        assert!(!ctx.urls.is_enabled());
        let _ = dispatch_builtin(&ctx, &alice, "url", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room URLs are off (0 configured).");
    }
}
