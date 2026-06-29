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
    "/users - list connected users",
    "/topic [text] - show or set room topic",
    "/motd [text] - show or set message of the day",
    "/ban <nick> - ban online user",
    "/unban <nick|ip|ident> - remove ban",
    "/banlist - list active bans",
    "/whois <nick> - show user info",
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
/// Retorna `true` si el comando fue manejado aquí y no debe pasar a scripts.
pub fn dispatch_builtin(ctx: &AppContext, user: &Arc<AresUser>, command: &str, args: &str) -> bool {
    match command.to_ascii_lowercase().as_str() {
        "help" => {
            handle_help(ctx, user, args);
            true
        }
        "users" => {
            handle_users(ctx, user, args);
            true
        }
        "topic" => {
            handle_topic(ctx, user, args);
            true
        }
        "motd" => {
            handle_motd(ctx, user, args);
            true
        }
        "ban" => {
            handle_ban(ctx, user, args);
            true
        }
        "unban" => {
            handle_unban(ctx, user, args);
            true
        }
        "banlist" => {
            handle_banlist(ctx, user, args);
            true
        }
        "whois" => {
            handle_whois(ctx, user, args);
            true
        }
        _ => false,
    }
}

fn handle_help(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    for line in DEFAULT_HELP_LINES {
        send_system_line(ctx, user, line);
    }
}

fn handle_users(ctx: &AppContext, user: &Arc<AresUser>, _args: &str) {
    let mut users: Vec<String> = ctx
        .user_pool
        .users()
        .into_iter()
        .filter(|u| u.logged_in && !u.quarantined)
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

fn handle_ban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    let target_name = args.trim();
    if target_name.is_empty() {
        send_system_line(ctx, user, "Usage: /ban <nick>");
        return;
    }

    let Some(target) = ctx.user_pool.get_by_name(target_name) else {
        send_system_line(ctx, user, "User not found.");
        return;
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
        return;
    }

    send_system_line(
        ctx,
        user,
        &format!("Banned '{}' (ident {}).", target.name.read(), ident),
    );
    send_system_line(ctx, &target, "You have been banned from this room.");

    // Expulsión inmediata del pool para reflejar el ban en runtime.
    force_part_user(ctx, &target);
}

fn handle_unban(ctx: &AppContext, user: &Arc<AresUser>, args: &str) {
    if !can_edit_topic(user) {
        send_system_line(ctx, user, "Access denied. Moderator+ required.");
        return;
    }

    let target = args.trim();
    if target.is_empty() {
        send_system_line(ctx, user, "Usage: /unban <nick|ip|ident>");
        return;
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
        if !u.logged_in || u.quarantined {
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
        if !u.logged_in || u.quarantined {
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

        let handled = dispatch_builtin(&ctx, &user, "help", "");
        assert!(handled);

        let first = rx.try_recv().expect("line 1");
        let second = rx.try_recv().expect("line 2");
        let third = rx.try_recv().expect("line 3");
        let fourth = rx.try_recv().expect("line 4");
        let fifth = rx.try_recv().expect("line 5");
        let sixth = rx.try_recv().expect("line 6");
        let seventh = rx.try_recv().expect("line 7");
        let eighth = rx.try_recv().expect("line 8");
        let ninth = rx.try_recv().expect("line 9");

        let (_from1, t1) = decode_pvt(first);
        let (_from2, t2) = decode_pvt(second);
        let (_from3, t3) = decode_pvt(third);
        let (_from4, t4) = decode_pvt(fourth);
        let (_from5, t5) = decode_pvt(fifth);
        let (_from6, t6) = decode_pvt(sixth);
        let (_from7, t7) = decode_pvt(seventh);
        let (_from8, t8) = decode_pvt(eighth);
        let (_from9, t9) = decode_pvt(ninth);

        assert_eq!(t1, "Available commands:");
        assert_eq!(t2, "/help - show this help");
        assert_eq!(t3, "/users - list connected users");
        assert_eq!(t4, "/topic [text] - show or set room topic");
        assert_eq!(t5, "/motd [text] - show or set message of the day");
        assert_eq!(t6, "/ban <nick> - ban online user");
        assert_eq!(t7, "/unban <nick|ip|ident> - remove ban");
        assert_eq!(t8, "/banlist - list active bans");
        assert_eq!(t9, "/whois <nick> - show user info");
    }

    #[test]
    fn builtin_users_lists_connected_users() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob);

        let handled = dispatch_builtin(&ctx, &alice, "users", "");
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
        assert!(!dispatch_builtin(&ctx, &user, "notreal", ""));
    }

    #[test]
    fn builtin_topic_without_args_shows_current_topic() {
        let ctx = make_test_ctx();
        let (user, mut rx) = make_test_user(1, "Alice");
        ctx.user_pool.add(user.clone());

        let handled = dispatch_builtin(&ctx, &user, "topic", "");
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

        let handled = dispatch_builtin(&ctx, &user, "topic", "nuevo topic");
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

        let handled = dispatch_builtin(&ctx, &alice, "topic", "nuevo topic");
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

        let handled = dispatch_builtin(&ctx, &user, "motd", "");
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

        let handled = dispatch_builtin(&ctx, &alice, "ban", "Bob");
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

        let ban_handled = dispatch_builtin(&ctx, &alice, "ban", "Bob");
        assert!(ban_handled);
        assert!(ctx.bans.is_banned(&bob.guid, bob.external_ip));
        assert!(ctx.user_pool.get_by_name("Bob").is_none());

        let ack_text = next_pvt_text(&mut alice_rx);
        assert!(ack_text.starts_with("Banned 'Bob' (ident "));

        let notice = next_pvt_text(&mut bob_rx);
        assert_eq!(notice, "You have been banned from this room.");

        let unban_handled = dispatch_builtin(&ctx, &alice, "unban", "Bob");
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

        let handled = dispatch_builtin(&ctx, &alice, "unban", "ghost");
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

        let handled = dispatch_builtin(&ctx, &alice, "banlist", "");
        assert!(handled);

        let t1 = next_pvt_text(&mut alice_rx);
        assert_eq!(t1, "Active bans:");

        let t2 = next_pvt_text(&mut alice_rx);
        assert!(t2.contains("name='Bob'"));
        assert!(t2.contains("ip=10.0.0.2"));
    }

    #[test]
    fn builtin_whois_reports_user_info() {
        let ctx = make_test_ctx();
        let (alice, mut alice_rx) = make_test_user(1, "Alice");
        let (bob, _bob_rx) = make_test_user(2, "Bob");
        *bob.level.write() = ILevel::Voice;
        ctx.user_pool.add(alice.clone());
        ctx.user_pool.add(bob.clone());

        let handled = dispatch_builtin(&ctx, &alice, "whois", "Bob");
        assert!(handled);

        let msg = alice_rx.try_recv().expect("whois");
        let (_from, text) = decode_pvt(msg);
        assert!(text.contains("WHOIS Bob"));
        assert!(text.contains("ip=10.0.0.2"));
        assert!(text.contains("guid=02020202020202020202020202020202"));
        assert!(text.contains("level=2"));
    }
}
