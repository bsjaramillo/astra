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
    "/cname [text|-] - set or clear your custom name",
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
    "/version - show server version",
    "/register <password> - register your account",
    "/unregister - delete your account",
    "/login <password> - log into your account",
    "/grant <nick> <level> - set user level",
    "/revoke <nick> - reset user to regular",
    "/greets [on|off] - toggle or show greet status",
    "/addgreet <text> - add a greeting (placeholders +n +ip +uc +rn ...)",
    "/remgreet <index> - remove greeting by index",
    "/listgreets - list greetings",
    "/addfilter <word> [block|kick|ban] - add a chat word filter",
    "/remfilter <word> - remove a word filter",
    "/listfilters - list word filters",
    "/url [on|off] - toggle or show rotating room URLs",
    "/addurl <address> <text> - add a rotating room URL",
    "/remurl <index> - remove a room URL",
    "/listurl - list room URLs",
    "/history - show recent public messages",
    "/whowas <nick|ip> - search seen-user history",
    "/lastseen <nick|ip> - when a user was last seen",
    "/roominfo - show room statistics",
    "/status [text] - show or set room status",
    "/id <nick> - show a user's session id",
    "/info <nick> - show detailed user info",
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
    "/joinfilter [add|del <pat>|list] - filter nicks at login",
    "/filefilter [add|del <pat>|list] - filter shared file names",
    "/vspy [on|off] - watch other vrooms' chat",
    "/ipsend [on|off] - receive joiners' IP info",
    "/logsend [on|off] - receive a room activity log",
    "/bansend [on|off] - receive ban notifications",
    "/trace <nick|ip> - geolocate a user (needs GeoIP db)",
    "/define <word> - dictionary definition (Wordnik)",
    "/urban <term> - Urban Dictionary lookup",
];

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

    // Gate global `/disableadmins`: si está activo, solo el Owner puede usar
    // comandos admin (todo salvo los de usuario común y el propio toggle).
    if ctx.admins_disabled.load(std::sync::atomic::Ordering::Relaxed)
        && !has_level(user, ILevel::Owner)
        && !is_user_command(&cmd)
    {
        send_system_line(ctx, user, "Admin commands are currently disabled.");
        return (true, vec![]);
    }

    match cmd.as_str() {
        "help" => {
            handle_help(ctx, user, args);
            (true, vec![])
        }
        "nick" => {
            handle_nick(ctx, user, args);
            (true, vec![])
        }
        "vroom" => {
            handle_vroom(ctx, user, args);
            (true, vec![])
        }
        "cname" => {
            handle_cname(ctx, user, args);
            (true, vec![])
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
        "uptime" | "stats" => {
            handle_uptime(ctx, user, args);
            (true, vec![])
        }
        "version" => {
            handle_version(ctx, user, args);
            (true, vec![])
        }
        "register" => {
            handle_register(ctx, user, args);
            (true, vec![])
        }
        "unregister" | "rempassword" => {
            handle_unregister(ctx, user, args);
            (true, vec![])
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
        "adminmsg" | "adminannounce" => {
            handle_opmsg(ctx, user, args);
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
            handle_announce(ctx, user, args);
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
        | "sharefiles" | "roomsearch" | "avatars" | "stealth" => {
            handle_room_flag(ctx, user, &cmd, args);
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
        "kewltext" => {
            handle_text_effect(ctx, user, args, TextEffect::Kewl, true);
            (true, vec![])
        }
        "unkewltext" => {
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
        "clock" | "idle" => {
            // Toggles de sala persistidos (el efecto de clock lo aplica una task).
            handle_room_flag(ctx, user, &cmd, args);
            (true, vec![])
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
        "listpasswords" | "autologins" => {
            handle_listpasswords(ctx, user, args);
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
        "greetmsg" => {
            handle_greets(ctx, user, args);
            (true, vec![])
        }
        "addgreetmsg" | "pmgreetmsg" => {
            handle_addgreet(ctx, user, args);
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
            handle_cname(ctx, user, args);
            (true, vec![])
        }
        "uncustomname" => {
            handle_cname(ctx, user, "-");
            (true, vec![])
        }
        "listbans" => {
            handle_banlist(ctx, user, args);
            (true, vec![])
        }
        "wordfilters" | "viewfilter" => {
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
        "viewmotd" | "loadmotd" => {
            handle_motd(ctx, user, "");
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
fn is_user_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "help" | "users" | "whois" | "id" | "info" | "uptime" | "stats"
            | "version" | "topic" | "motd" | "roominfo" | "status"
            | "register" | "unregister" | "login" | "cname" | "nick" | "vroom"
    )
}

fn handle_help(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    for line in DEFAULT_HELP_LINES {
        send_system_line(ctx, user, line);
    }
    // Agregar líneas registradas por scripts vía `Help_addLine(cmd, line)`.
    // Solo se muestran cuando el user hace `/help` (sin args específicos).
    for (cmd, line) in astra_scripting::api::extra_help_lines() {
        let formatted = format!("/{} - {}", cmd, line);
        send_system_line(ctx, user, &formatted);
    }
}

fn handle_nick(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let new_name = args.trim();
    if new_name.is_empty() {
        send_system_line(ctx, user, "Usage: /nick <name>");
        return;
    }
    if new_name.chars().count() > 30 {
        send_system_line(ctx, user, "Nickname too long.");
        return;
    }

    let old_name = user.name.read().clone();
    if old_name.eq_ignore_ascii_case(new_name) {
        send_system_line(ctx, user, "You already have that nickname.");
        return;
    }
    if ctx.user_pool.get_by_name(new_name).is_some() {
        send_system_line(ctx, user, "Nickname already in use.");
        return;
    }

    *user.name.write() = new_name.to_string();
    ctx.user_pool.rename(user.id, &old_name, new_name);

    let mut part_user = AresUser::new(user.id, user.external_ip, user.guid);
    part_user.logged_in = true;
    *part_user.name.write() = old_name.clone();
    let part_pkt = outbound::build_part(&part_user);
    let join_pkt = outbound::build_join_or_userlist(user);
    for other in ctx.user_pool.users() {
        if other.logged_in && *other.vroom.read() == *user.vroom.read() && !other.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = other.send(part_pkt.clone());
            let _ = other.send(join_pkt.clone());
        }
    }

    ctx.publish_link_event(server_core::LinkEvent::NickChanged {
        origin: None,
        old_name,
        user: server_core::LinkUserSnapshot::from_user(user),
    });

    send_system_line(ctx, user, "Nickname updated.");
}

fn handle_vroom(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let Ok(new_vroom) = args.trim().parse::<u16>() else {
        send_system_line(ctx, user, "Usage: /vroom <id>");
        return;
    };

    let old_vroom = *user.vroom.read();
    if old_vroom == new_vroom {
        send_system_line(ctx, user, "You are already in that vroom.");
        return;
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

    let part_pkt = outbound::build_part(&part_user);
    let join_pkt = outbound::build_join_or_userlist(user);
    for other in ctx.user_pool.users() {
        if !other.logged_in || other.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let other_vroom = *other.vroom.read();
        if other_vroom == old_vroom {
            let _ = other.send(part_pkt.clone());
        }
        if other_vroom == new_vroom {
            let _ = other.send(join_pkt.clone());
        }
    }

    ctx.publish_link_event(server_core::LinkEvent::VroomChanged {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(user),
    });
    // El evento de scripting (onVroomJoin) lo dispara tcp_handler.rs
    // después de dispatch_builtin, porque commands no tiene acceso a
    // ScriptHandle (commands no depende de scripting).

    send_system_line(ctx, user, &format!("Moved to vroom {}.", new_vroom));
}

fn handle_cname(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        let current = user.custom_name.read().clone();
        match current {
            Some(value) => send_system_line(ctx, user, &format!("Custom name: {}", value)),
            None => send_system_line(ctx, user, "Custom name is not set."),
        }
        return;
    }

    let next = if trimmed == "-" {
        None
    } else {
        Some(trimmed.chars().take(40).collect::<String>())
    };

    *user.custom_name.write() = next.clone();
    ctx.publish_link_event(server_core::LinkEvent::CustomName {
        origin: None,
        name: user.name.read().clone(),
        custom_name: next.clone(),
    });

    match next {
        Some(value) => send_system_line(ctx, user, &format!("Custom name set to '{}'.", value)),
        None => send_system_line(ctx, user, "Custom name cleared."),
    }
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
    if args.trim().is_empty() {
        send_system_line(ctx, user, &format!("MOTD: {}", ctx.current_room_topic()));
        return;
    }

    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    let new_motd = truncate_text(args.trim(), 300);
    ctx.set_room_topic(new_motd.clone());
    broadcast_topic(ctx, &new_motd);
    send_system_line(ctx, user, "MOTD updated.");
}

fn handle_ban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> bool {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return false;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /ban <nick>");
        return false;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
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
        &format!("Banned '{}' (ident {}).", target.name.read(), ident),
    );
    send_system_line(ctx, &target, "You have been banned from this room.");

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
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
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
        send_system_line(ctx, user, "Unban successful.");
    } else {
        send_system_line(ctx, user, "No matching ban found.");
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

    let guid_hex = guid_to_hex(&target.guid);
    let level = *target.level.read() as u8;
    send_system_line(
        ctx,
        user,
        &format!(
            "WHOIS {}: ip={} local_ip={} guid={} level={} files={} ver='{}'",
            target.name.read(),
            target.external_ip,
            target.local_ip,
            guid_hex,
            level,
            target.file_count,
            target.version
        ),
    );
}

fn handle_kick(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /kick <nick>");
        return;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
    };

    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot kick a user of equal or higher level.");
        return;
    }

    send_system_line(ctx, &target, "You have been kicked from this room.");
    force_part_user(ctx, &target);
    send_system_line(ctx, user, &format!("Kicked '{}'.", target_name));
}

fn handle_muzzle(ctx: &AppContext, user: &Arc<AresUser>, args: &str, muzzle: bool) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        let cmd = if muzzle { "muzzle" } else { "unmuzzle" };
        send_system_line(ctx, user, &format!("Usage: /{} <nick>", cmd));
        return;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
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
        send_system_line(ctx, &target, "You have been muzzled.");
        send_system_line(ctx, user, &format!("Muzzled '{}'.", target_name));
    } else {
        send_system_line(ctx, &target, "You have been unmuzzled.");
        send_system_line(ctx, user, &format!("Unmuzzled '{}'.", target_name));
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
    let pkt = outbound::build_pvt(&from, text);
    let mut count = 0usize;
    for other in ctx.user_pool.users() {
        if !other.logged_in
            || other.id == user.id
            || other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        let _ = other.send(pkt.clone());
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
    let pkt = outbound::build_pvt(&ctx.settings.bot_name, &line);
    for other in ctx.user_pool.users() {
        if !other.logged_in
            || other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            continue;
        }
        if (*other.level.read() as u8) >= ILevel::Moderator as u8 {
            let _ = other.send(pkt.clone());
        }
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

fn handle_register(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !ctx.settings.allow_registration {
        send_system_line(ctx, user, "Registration is disabled in this room.");
        return;
    }

    let password = args.trim();
    if password.len() < 4 {
        send_system_line(ctx, user, "Usage: /register <password> (4+ chars)");
        return;
    }

    match ctx.accounts.find_by_guid(&user.guid) {
        Ok(Some(_)) => {
            send_system_line(ctx, user, "Already registered. Use /unregister first.");
            return;
        }
        Ok(None) => {}
        Err(_) => {
            send_system_line(ctx, user, "Registration failed (database error).");
            return;
        }
    }

    let name = user.name.read().clone();
    let live_level = (*user.level.read() as u8).max(ILevel::Regular as u8);
    match ctx.accounts.register(&name, &user.guid, password, live_level) {
        Ok(()) => send_system_line(ctx, user, "Account registered. Use /login <password>."),
        Err(_) => send_system_line(ctx, user, "Registration failed (database error)."),
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
    let _ = target.send(outbound::build_pvt(&from, text));
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

fn handle_unregister(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    match ctx.accounts.unregister(&user.guid) {
        Ok(true) => send_system_line(ctx, user, "Account deleted."),
        Ok(false) => send_system_line(ctx, user, "You are not registered."),
        Err(_) => send_system_line(ctx, user, "Unregister failed (database error)."),
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
    let Some(acc) = ctx.accounts.find_by_guid(&user.guid).ok().flatten() else {
        return false;
    };
    let level = level_from_u8(acc.level);
    apply_level(
        ctx,
        user,
        user,
        level,
        &format!("Auto-logged in (level {}).", acc.level),
    )
}

/// Retorna el nick del target si el nivel cambió.
fn handle_grant(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Option<String> {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
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
        send_system_line(ctx, user, "User not found.");
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

    let msg = format!("Your level is now {} ({}).", new_level as u8, level_name(new_level));
    if apply_level(ctx, user, &target, new_level, &msg) {
        send_system_line(
            ctx,
            user,
            &format!("'{}' is now level {} ({}).", target_name, new_level as u8, level_name(new_level)),
        );
        Some(target.name.read().clone())
    } else {
        None
    }
}

/// Retorna el nick del target si el nivel cambió.
fn handle_revoke(ctx: &AppContext, user: &Arc<AresUser>, args: &str) -> Option<String> {
    if !has_level(user, ILevel::Admin) {
        send_system_line(ctx, user, "Access denied. Admin+ required.");
        return None;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /revoke <nick>");
        return None;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return None;
    };

    if !outranks(user, &target) {
        send_system_line(ctx, user, "You cannot modify a user of equal or higher level.");
        return None;
    }

    if apply_level(ctx, user, &target, ILevel::Regular, "Your level has been reset to regular.") {
        send_system_line(ctx, user, &format!("'{}' is now a regular user.", target_name));
        Some(target.name.read().clone())
    } else {
        send_system_line(ctx, user, &format!("'{}' is already a regular user.", target_name));
        None
    }
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
    (*user.level.read() as u8) >= min as u8
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
        send_system_line(ctx, user, "Usage: /addfilter <word> [block|kick|ban]");
        return;
    }
    // El último token puede ser la acción; el resto es el patrón.
    let (pattern, action) = match args.rsplit_once(char::is_whitespace) {
        Some((p, last)) if matches!(last.to_ascii_lowercase().as_str(), "block" | "kick" | "ban") => {
            (p.trim(), FilterAction::parse(last))
        }
        _ => (args, FilterAction::Block),
    };
    if pattern.is_empty() {
        send_system_line(ctx, user, "Usage: /addfilter <word> [block|kick|ban]");
        return;
    }
    ctx.word_filter.add(pattern, action);
    send_system_line(
        ctx,
        user,
        &format!("Filter '{}' → {} added.", pattern.to_ascii_lowercase(), action.as_str()),
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
    for (pattern, action) in &filters {
        send_system_line(ctx, user, &format!("{} → {}", pattern, action.as_str()));
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

fn handle_history(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let msgs = ctx.recent_messages(20);
    if msgs.is_empty() {
        send_system_line(ctx, user, "No message history yet.");
        return;
    }
    send_system_line(ctx, user, &format!("Recent messages ({}):", msgs.len()));
    for m in &msgs {
        let sep = if m.is_emote { " " } else { ": " };
        send_system_line(ctx, user, &format!("{}{}{}", m.name, sep, m.text));
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
    let results = ctx.db.search_user_history(query, 10).unwrap_or_default();
    if results.is_empty() {
        send_system_line(ctx, user, "No matching history.");
        return;
    }
    send_system_line(ctx, user, &format!("Whowas '{}' ({}):", query, results.len()));
    for (name, version, ip, last_seen) in &results {
        send_system_line(
            ctx,
            user,
            &format!(
                "{} [{}] ver='{}' seen {}",
                name, ip, version, format_time_ago(*last_seen)
            ),
        );
    }
}

fn handle_lastseen(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let query = args.trim();
    if query.is_empty() {
        send_system_line(ctx, user, "Usage: /lastseen <nick|ip>");
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

fn handle_roominfo(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
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

    send_system_line(ctx, user, &format!("Room: {}", ctx.settings.room_name));
    send_system_line(ctx, user, &format!("Users: {} ({} ops, {} owners)", total, ops, owners));
    let secs = ctx.uptime_secs();
    send_system_line(
        ctx,
        user,
        &format!("Uptime: {}d {}h {}m", secs / 86400, (secs / 3600) % 24, (secs / 60) % 60),
    );
    let status = ctx.room_status();
    if !status.is_empty() {
        send_system_line(ctx, user, &format!("Status: {}", status));
    }
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
}

fn handle_id(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, &format!("Your id is {}.", user.id));
        return;
    }
    match ctx.user_pool.get_by_name(target_name) {
        Some(t) => send_system_line(ctx, user, &format!("'{}' has id {}.", t.name.read(), t.id)),
        None => send_system_line(ctx, user, "User not found."),
    }
}

fn handle_info(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    // Alias detallado de whois; si no hay args, muestra info propia.
    let target = if args.trim().is_empty() {
        user.clone()
    } else {
        match ctx.user_pool.get_by_name(args.trim()) {
            Some(t) => t,
            None => {
                send_system_line(ctx, user, "User not found.");
                return;
            }
        }
    };
    let level = *target.level.read() as u8;
    send_system_line(
        ctx,
        user,
        &format!(
            "INFO {}: id={} ip={} level={} files={} vroom={} ver='{}' region='{}'",
            target.name.read(),
            target.id,
            target.external_ip,
            level,
            target.file_count,
            *target.vroom.read(),
            target.version,
            target.region,
        ),
    );
}

fn handle_customnames(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
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

    let part_pkt = outbound::build_part(&part_user);
    let join_pkt = outbound::build_join_or_userlist(&target);
    for other in ctx.user_pool.users() {
        if !other.logged_in || other.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let ov = *other.vroom.read();
        if ov == old_vroom {
            let _ = other.send(part_pkt.clone());
        }
        if ov == new_vroom {
            let _ = other.send(join_pkt.clone());
        }
    }
    ctx.publish_link_event(server_core::LinkEvent::VroomChanged {
        origin: None,
        user: server_core::LinkUserSnapshot::from_user(&target),
    });
    send_system_line(ctx, &target, &format!("You were moved to vroom {}.", new_vroom));
    send_system_line(ctx, user, &format!("Moved '{}' to vroom {}.", target_name, new_vroom));
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
    let part_pkt = outbound::build_part(&part_user);
    let join_pkt = outbound::build_join_or_userlist(&target);
    for other in ctx.user_pool.users() {
        if other.logged_in
            && *other.vroom.read() == *target.vroom.read()
            && !other.quarantined.load(std::sync::atomic::Ordering::Relaxed)
        {
            let _ = other.send(part_pkt.clone());
            let _ = other.send(join_pkt.clone());
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

fn handle_admins(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let mut ops: Vec<(String, u8)> = ctx
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in && (*u.level.read() as u8) > ILevel::Regular as u8)
        .map(|u| (u.name.read().clone(), *u.level.read() as u8))
        .collect();
    ops.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase())));
    if ops.is_empty() {
        send_system_line(ctx, user, "No ops online.");
        return;
    }
    send_system_line(ctx, user, &format!("Ops online ({}):", ops.len()));
    for (name, level) in &ops {
        send_system_line(ctx, user, &format!("{} (level {})", name, level));
    }
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
    // El bot lo dice en público a toda la sala.
    let pkt = outbound::build_public(&ctx.settings.bot_name, text);
    for u in ctx.user_pool.users() {
        if u.logged_in && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = u.send(pkt.clone());
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
    let pkt = if let Some(emote) = text.strip_prefix("/me ") {
        outbound::build_emote(&name, emote)
    } else {
        outbound::build_public(&name, text)
    };
    let vroom = *target.vroom.read();
    for u in ctx.user_pool.users() {
        if u.logged_in && *u.vroom.read() == vroom && !u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = u.send(pkt.clone());
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
    let dest = dest.strip_prefix("astrahash://").unwrap_or(dest);
    let Some((ip_str, port_str)) = dest.rsplit_once(':') else {
        send_system_line(ctx, user, "Destination must be ip:port.");
        return;
    };
    let (Ok(ip), Ok(port)) = (ip_str.parse::<IpAddr>(), port_str.parse::<u16>()) else {
        send_system_line(ctx, user, "Invalid ip:port.");
        return;
    };
    let _ = target.send(outbound::build_redirect(ip, port, &ctx.settings.room_name));
    send_system_line(ctx, user, &format!("Redirected '{}' to {}:{}.", target_name, ip, port));
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
    // Envía líneas en blanco a todos para "limpiar" la pantalla (paridad sb0t).
    let blank = outbound::build_public(" ", " ");
    for u in ctx.user_pool.users() {
        if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        for _ in 0..50 {
            let _ = u.send(blank.clone());
        }
    }
    send_system_line(ctx, user, "Screen cleared.");
}

fn handle_locate(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }
    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /locate <nick>");
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

/// Feeds internos a los que un admin puede suscribirse.
#[derive(Clone, Copy)]
enum Subscription {
    Vspy,
    IpSend,
    LogSend,
    BanSend,
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
                    let _ = user.send(server_core::outbound::build_pvt(&bot, line));
                }
            });
        }
        Err(_) => {
            let _ = user.send(server_core::outbound::build_pvt(
                &bot,
                "Lookup requires the async runtime (unavailable here).",
            ));
        }
    }
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
        "ipsend" | "logsend" | "bansend" => "requires a connected link hub",
        "trace" | "vspy" => "requires packet-tracing support",
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

fn force_part_user(ctx: &AppContext, target: &Arc<AresUser>) {
    let part_pkt = outbound::build_part(target);
    let tname = target.name.read();
    let ws_part = format!("OFFLINE:{}:{}", tname.chars().count(), tname);
    drop(tname);

    ctx.user_pool.remove(target.id);
    ctx.stats.on_user_part();

    for u in ctx.user_pool.users() {
        if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let _ = u.send(part_pkt.clone());
        if let Some(tx) = &u.ws_text_sender {
            let _ = tx.send(ws_part.clone());
        }
    }
}

fn broadcast_topic(ctx: &AppContext, text: &str) {
    let pkt = outbound::build_topic(text);
    let ws_msg = format!("TOPIC:{}:{}", text.chars().count(), text);
    for u in ctx.user_pool.users() {
        if !u.logged_in || u.quarantined.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let _ = u.send(pkt.clone());
        if let Some(tx) = &u.ws_text_sender {
            let _ = tx.send(ws_msg.clone());
        }
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
    let pkt = outbound::build_pvt(from, text);
    let _ = user.send(pkt);
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
    fn parse_empty_command() {
        // Solo "/" sin nada más → no es un comando válido
        assert!(parse_command("/").is_none());
        assert!(parse_command("/  ").is_none());
    }

    #[test]
    fn builtin_help_sends_lines() {
        let ctx = make_test_ctx();
        let (user, mut rx) = make_test_user(1, "Alice");

        let (handled, _) = dispatch_builtin(&ctx, &user, "help", "");
        assert!(handled);

        for expected in DEFAULT_HELP_LINES {
            let pkt = rx.try_recv().expect("expected help line");
            let (_from, text) = decode_pvt(pkt);
            assert_eq!(&text, expected);
        }
        assert!(rx.try_recv().is_err(), "no extra lines expected");
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
    fn builtin_motd_without_args_shows_current_topic_as_motd() {
        let ctx = make_test_ctx();
        let (user, mut rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(user.clone());

        let (handled, _) = dispatch_builtin(&ctx, &user, "motd", "");
        assert!(handled);

        let msg = rx.try_recv().expect("motd response");
        let (_from, text) = decode_pvt(msg);
        assert_eq!(text, "MOTD: Welcome to Astra");
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
        assert_eq!(text, "Access denied. Moderator+ required.");
        assert!(!ctx.bans.is_banned(&bob.guid, bob.external_ip));
    }

    #[test]
    fn builtin_ban_and_unban_online_user() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (ban_handled, _) = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        assert!(ban_handled);
        assert!(ctx.bans.is_banned(&bob.guid, bob.external_ip));
        assert!(ctx.user_pool.get_by_name("Bob").is_none());

        let ack_text = next_pvt_text(&mut alice_rx);
        assert!(ack_text.starts_with("Banned 'Bob' (ident "));

        let notice = next_pvt_text(&mut bob_rx);
        assert_eq!(notice, "You have been banned from this room.");

        let (unban_handled, _) = dispatch_builtin(&ctx, &alice, "unban", "Bob");
        assert!(unban_handled);
        assert!(!ctx.bans.is_banned(&bob.guid, bob.external_ip));

        let unban_text = next_pvt_text(&mut alice_rx);
        assert_eq!(unban_text, "Unban successful.");
    }

    #[test]
    fn builtin_unban_without_match_reports_not_found() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
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
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let _ = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        let _ = next_pvt_text(&mut alice_rx); // ban ack

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

        // Muzzle repetido no cambia nada
        let _ = dispatch_builtin(&ctx, &alice, "muzzle", "Bob");
        assert_eq!(next_pvt_text(&mut alice_rx), "'Bob' is already muzzled.");

        let _ = dispatch_builtin(&ctx, &alice, "unmuzzle", "Bob");
        assert!(!bob.muzzled.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(next_pvt_text(&mut bob_rx), "You have been unmuzzled.");
        assert_eq!(next_pvt_text(&mut alice_rx), "Unmuzzled 'Bob'.");
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
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *bob.level.write() = ILevel::Voice;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "whois", "Bob");
        assert!(handled);

        let msg = alice_rx.try_recv().expect("whois");
        let (_from, text) = decode_pvt(msg);
        assert!(text.contains("WHOIS Bob"));
        assert!(text.contains("ip=10.0.0.2"));
        assert!(text.contains("guid=02020202020202020202020202020202"));
        assert!(text.contains("level=2"));
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
        assert_eq!(next_pvt_text(&mut alice_rx), "badword → ban");
        assert_eq!(next_pvt_text(&mut alice_rx), "spammy → block");

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
    fn builtin_history_shows_recent_messages() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        ctx.record_message("Bob", "hola a todos", false);
        ctx.record_message("Carol", "waves", true);

        let _ = dispatch_builtin(&ctx, &alice, "history", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Recent messages (2):");
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob: hola a todos");
        assert_eq!(next_pvt_text(&mut alice_rx), "Carol waves");
    }

    #[test]
    fn builtin_history_requires_moderator() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(alice.clone());
        let _ = dispatch_builtin(&ctx, &alice, "history", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Access denied. Moderator+ required.");
    }

    #[test]
    fn builtin_lastseen_online_and_history() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
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
        assert_eq!(next_pvt_text(&mut alice_rx), "Whowas 'char' (1):");
        let line = next_pvt_text(&mut alice_rx);
        assert!(line.contains("Charlie [9.9.9.9]"), "got: {}", line);
    }

    #[test]
    fn builtin_roominfo_and_status() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "roominfo", "");
        assert!(next_pvt_text(&mut alice_rx).starts_with("Room: "));
        assert!(next_pvt_text(&mut alice_rx).starts_with("Users: 1"));
        assert!(next_pvt_text(&mut alice_rx).starts_with("Uptime: "));

        // set status → aparece en roominfo
        let _ = dispatch_builtin(&ctx, &alice, "status", "under maintenance");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room status set to 'under maintenance'.");
        let _ = dispatch_builtin(&ctx, &alice, "status", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Status: under maintenance");
    }

    #[test]
    fn builtin_id_and_customnames() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
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
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());

        let _ = dispatch_builtin(&ctx, &alice, "trace", "8.8.8.8");
        assert!(next_pvt_text(&mut alice_rx).contains("requires a GeoIP database"));
    }

    #[test]
    fn builtin_vspy_toggle() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Moderator;
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
        *alice.level.write() = ILevel::Admin;
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
        assert_eq!(last, "Screen cleared.");
    }

    #[test]
    fn builtin_unavailable_commands_respond() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());

        let (handled, _) = dispatch_builtin(&ctx, &alice, "loadtemplate", "");
        assert!(handled);
        assert!(next_pvt_text(&mut alice_rx).contains("not available"));
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
        *alice.level.write() = ILevel::Moderator;
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
        assert_eq!(next_pvt_text(&mut alice_rx), "Ops online (2):");
        assert_eq!(next_pvt_text(&mut alice_rx), "Bob (level 80)");
        assert_eq!(next_pvt_text(&mut alice_rx), "Alice (level 50)");
    }

    #[test]
    fn builtin_announce_broadcasts_public() {
        let ctx = make_test_ctx();
        let (alice, _a_rx) = make_test_user(1, "Alice");
        let (bob, mut bob_rx) = make_test_user(2, "Bob");
        *alice.level.write() = ILevel::Moderator;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let _ = dispatch_builtin(&ctx, &alice, "announce", "server reboot soon");
        // Bob recibe un público del bot
        let pkt = bob_rx.try_recv().expect("announce");
        assert_eq!(pkt[0], TcpMsg::Public as u8);
        let mut r = PacketReader::new(&pkt[1..]);
        let from = r.read_string_nt().unwrap();
        let text = r.read_string_nt().unwrap();
        assert_eq!(from, "Astra");
        assert_eq!(text, "server reboot soon");
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
        *alice.level.write() = ILevel::Moderator;
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

        // Un moderador ya no puede usar comandos admin
        let (handled, _) = dispatch_builtin(&ctx, &mod_u, "kiddy", "Owner");
        assert!(handled);
        assert_eq!(next_pvt_text(&mut mod_rx), "Admin commands are currently disabled.");

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
        *alice.level.write() = ILevel::Admin;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        // Ban de Bob → registra acción
        let _ = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        let _ = next_pvt_text(&mut alice_rx); // "Banned 'Bob'..."
        assert_eq!(ctx.bans.len(), 1);

        let _ = dispatch_builtin(&ctx, &alice, "banstats", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Active bans: 1 | recent actions: 1");
        assert_eq!(next_pvt_text(&mut alice_rx), "Alice banned Bob [10.0.0.2]");

        let _ = dispatch_builtin(&ctx, &alice, "clearbans", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Cleared 1 ban(s).");
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

        *alice.level.write() = ILevel::Admin;
        let _ = dispatch_builtin(&ctx, &alice, "url", "off");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room URLs disabled.");
        assert!(!ctx.urls.is_enabled());
        let _ = dispatch_builtin(&ctx, &alice, "url", "");
        assert_eq!(next_pvt_text(&mut alice_rx), "Room URLs are off (0 configured).");
    }
}
