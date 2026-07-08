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
use iconnect::ILevel;

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
        "banlist" => {
            handle_banlist(ctx, user, args);
            (true, vec![])
        }
        "whois" => {
            handle_whois(ctx, user, args);
            (true, vec![])
        }
        "kick" => {
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
        "unregister" => {
            handle_unregister(ctx, user, args);
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
        _ => (false, vec![]),
    }
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

    let new_topic = truncate_text(args.trim(), 300);
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

    // Expulsión inmediata del pool para reflejar el ban en runtime.
    force_part_user(ctx, &target);
    true
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

fn can_edit_topic(user: &AresUser) -> bool {
    let level = *user.level.read() as u8;
    level >= ILevel::Moderator as u8
}

fn guid_to_hex(guid: &[u8; 16]) -> String {
    guid.iter().map(|b| format!("{:02x}", b)).collect()
}

fn force_part_user(ctx: &AppContext, target: &Arc<AresUser>) {
    let part_pkt = outbound::build_part(target);
    let ws_part = format!("PART:{}", target.name.read());

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
    let ws_msg = format!("TOPIC:{}", text);
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
        let from = r.read_string().expect("from");
        let text = r.read_string().expect("text");
        (from, text)
    }

    fn decode_topic(pkt: bytes::Bytes) -> String {
        assert_eq!(pkt[0], TcpMsg::ServerTopic as u8);
        let mut r = PacketReader::new(&pkt[1..]);
        r.read_string().expect("topic")
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
}
