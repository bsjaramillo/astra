//! Textos del sistema editables ("templates"), paridad conceptual con
//! `commands/Template.cs` de sb0t: un catálogo de mensajes con claves y un
//! texto por defecto, que el admin puede reescribir (por ejemplo, para
//! traducirlos o adaptarlos al tono de su sala).
//!
//! ## Cobertura
//!
//! El catálogo incluye **todos los mensajes del sistema que emite el server**
//! vía `send_system_line` (errores, usos, avisos y confirmaciones), más las
//! notificaciones de moderación con valores interpolados. Hay dos formas de
//! ruteo:
//!
//! - **Estáticos** (sin valores insertados): el call site pasa el literal y
//!   [`resolve`](TemplateManager::resolve) lo mapea a su override por
//!   coincidencia exacta del texto por defecto — se resuelve una sola vez, de
//!   forma centralizada, en `send_system_line`. No hace falta keyear cada
//!   call site.
//! - **Dinámicos** (con `+n`/`+a`/`+l`/`+i`): el call site usa
//!   [`render`](TemplateManager::render) con la clave y los valores.
//!
//! ## Placeholders
//!
//! Convención (subset sb0t): `+n` = nombre del sujeto, `+a` = nombre del admin
//! que ejecuta, `+l` = nivel, `+i` = ident/valor extra.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::db::Database;

/// Catálogo de textos del sistema: `(clave, texto por defecto)`.
///
/// Las claves son estables (se persisten los overrides por clave), así que no
/// se renombran una vez publicadas.
pub const TEMPLATE_DEFAULTS: &[(&str, &str)] = &[
    // Aviso de nueva versión del server (+v = versión nueva, +c = corriendo)
    (
        "update.available",
        "A new version of Astra is available: v+v (running v+c). Update from astra-creator with 'u' or pull the latest image.",
    ),
    // Control de acceso / errores comunes
    ("error.access_moderator", "Access denied. Moderator+ required."),
    ("error.access_admin", "Access denied. Admin+ required."),
    ("error.user_not_found", "User not found."),
    // Kick
    ("kick.target", "You have been kicked from this room."),
    ("kick.confirm", "Kicked '+n'."),
    // Ban  (+i = ident)
    ("ban.target", "You have been banned from this room."),
    ("ban.confirm", "Banned '+n' (ident +i)."),
    ("unban.success", "Unban successful."),
    ("unban.none", "No matching ban found."),
    // Muzzle
    ("muzzle.target", "You have been muzzled."),
    ("muzzle.confirm", "Muzzled '+n'."),
    ("unmuzzle.target", "You have been unmuzzled."),
    // Notification #29 de sb0t: el target tiene nivel >= al del admin. Es el
    // MISMO texto para todos los comandos (ban/kick/muzzle/echo/redirect/...).
    (
        "notification.level_too_low",
        "your admin level is too low to use this command on +n",
    ),
    // Category.Timeouts #0/#1 de sb0t (`#mtimeout` y expiración del muzzle).
    ("timeouts.muzzle_set", "+n has set the muzzle timeout to +i"),
    ("timeouts.muzzle_expired", "+n your muzzle timeout has expired"),
    ("unmuzzle.confirm", "Unmuzzled '+n'."),
    // Grant / revoke  (+l = nivel, ej. "80 (admin)")
    ("grant.target", "Your level is now +l."),
    ("grant.confirm", "'+n' is now level +l."),
    ("revoke.target", "Your level has been reset to regular."),
    ("revoke.confirm", "'+n' is now a regular user."),
    // Idle (paridad Category.Idle de sb0t; +t = hora HH:MM local)
    ("idle.enter", "+n idles at +t"),
    ("idle.return.s", "+n returned at +t - away time [+s seconds]"),
    ("idle.return.m", "+n returned at +t - away time [+m minutes +s seconds]"),
    ("idle.return.h", "+n returned at +t - away time [+h hours +m minutes +s seconds]"),
    ("idle.return.d", "+n returned at +t - away time [+d days +h hours +m minutes +s seconds]"),
    // Info (paridad Category.Info#1 de sb0t: listado de usuarios de /info)
    ("info.user", "+n [vroom: +v] [id: +i]"),
    // Room search (paridad Category.RoomSearch#0-7 de sb0t, textos exactos;
    // el comando `#roomsearch <texto>` los BROADCASTEA a toda la sala).
    ("roomsearch.disabled", "Room search service is not enabled"),
    ("roomsearch.empty", "Channel database is empty, try again later"),
    ("roomsearch.notfound", "Unable to find any channels containing +n"),
    ("roomsearch.header", "Results for +n as follows:"),
    ("roomsearch.name", "Name: +n"),
    ("roomsearch.topic", "Topic: +t"),
    ("roomsearch.info", "Language: +l | Server: +s | Users: +u"),
    ("roomsearch.hashlink", "Hashlink: \\\\+h"),
    // Last seen al entrar (paridad Notification#6 de sb0t)
    ("lastseen.join", "+n was last seen as +o at +t from +ip"),
    // Anuncios públicos de acciones admin (Category.AdminAction de sb0t;
    // +a = admin o nombre de la sala si stealth/cloak)
    ("adminaction.ban", "+n was banned by +a"),
    ("adminaction.unban", "+n was unbanned by +a"),
    ("adminaction.kick", "+n was kicked by +a"),
    ("adminaction.muzzle", "+n was muzzled by +a"),
    ("adminaction.unmuzzle", "+n was unmuzzled by +a"),
    ("adminaction.ban10", "+n was banned for 10 minutes by +a"),
    ("adminaction.ban60", "+n was banned for 60 minutes by +a"),
    ("adminaction.cbans", "+a has cleared the ban list"),
    ("adminaction.redirect", "+n has been redirected to +r by +a"),
    ("adminaction.customname", "+n's custom name has been set by +a"),
    ("adminaction.uncustomname", "+n's custom name has been unset by +a"),
    // AdminAction #7-#18 y #23-#26 de sb0t: los efectos de texto, el avatar,
    // el personal message y los range/ASN bans también se ANUNCIAN a la sala.
    // Category.EnableDisable de sb0t: cada toggle de sala se ANUNCIA a
    // toda la sala con su texto propio (no es un ack privado al op).
    ("enabledisable.sharefiles.on", "+n has enabled File Share monitoring"),
    ("enabledisable.sharefiles.off", "+n has disabled File Share monitoring"),
    ("enabledisable.idle.on", "+n has enabled Idle Monitoring"),
    ("enabledisable.idle.off", "+n has disabled Idle Monitoring"),
    ("enabledisable.clock.on", "+n enabled the topic clock"),
    ("enabledisable.clock.off", "+n disabled the topic clock"),
    ("enabledisable.greetmsg.on", "+n has enabled the greet message"),
    ("enabledisable.greetmsg.off", "+n has disabled the greet message"),
    ("enabledisable.pmgreetmsg.on", "+n has enabled the PM greet message"),
    ("enabledisable.pmgreetmsg.off", "+n has disabled the PM greet message"),
    ("greetings.pmgreet.set", "+n has set the PM greet message"),
    ("enabledisable.caps.on", "+n has enabled CAPS monitoring"),
    ("enabledisable.caps.off", "+n has disabled CAPS monitoring"),
    ("enabledisable.anon.on", "+n has enabled Anon monitoring"),
    ("enabledisable.anon.off", "+n has disabled Anon monitoring"),
    ("enabledisable.customnames.on", "+n has enabled custom names"),
    ("enabledisable.customnames.off", "+n has disabled custom names"),
    ("enabledisable.general.on", "+n has enabled general commands"),
    ("enabledisable.general.off", "+n has disabled general commands"),
    ("enabledisable.url.on", "dynamic url tag was enabled by +n"),
    ("enabledisable.url.off", "dynamic url tag was disabled by +n"),
    ("enabledisable.roominfo.on", "+n has enabled Room Information Updates"),
    ("enabledisable.roominfo.off", "+n has disabled Room Information Updates"),
    ("enabledisable.lastseen.on", "+n has enabled Last Seen monitoring"),
    ("enabledisable.lastseen.off", "+n has disabled Last Seen monitoring"),
    ("enabledisable.history.on", "+n has enabled chat history feature"),
    ("enabledisable.history.off", "+n has disabled chat history feature"),
    ("enabledisable.stealth.on", "+n has enabled stealth mode"),
    ("enabledisable.stealth.off", "+n has disabled stealth mode"),
    ("enabledisable.colors.on", "+n has enabled colors"),
    ("enabledisable.colors.off", "+n has disabled colors"),
    ("enabledisable.filter.on", "+n has enabled room filters"),
    ("enabledisable.filter.off", "+n has disabled room filters"),
    ("enabledisable.scribbles.on", "+n has enabled scribbles"),
    ("enabledisable.scribbles.off", "+n has disabled scribbles"),
    ("enabledisable.audios.on", "+n has enabled audios"),
    ("enabledisable.audios.off", "+n has disabled audios"),
    ("enabledisable.buzzes.on", "+n has enabled buzzes"),
    ("enabledisable.buzzes.off", "+n has disabled buzzes"),
    ("adminaction.kewltext", "+n has been set kewl text by +a"),
    ("adminaction.unkewltext", "+n has been unset kewl text by +a"),
    ("adminaction.lower", "+n has been lowered by +a"),
    ("adminaction.unlower", "+n has been unlowered by +a"),
    ("adminaction.kiddy", "+n has been kiddied by +a"),
    ("adminaction.unkiddy", "+n has been unkiddied by +a"),
    ("adminaction.echo", "+n has been echoed by +a"),
    ("adminaction.unecho", "+n has been unechoed by +a"),
    ("adminaction.paint", "+n has been painted by +a"),
    ("adminaction.unpaint", "+n has been unpainted by +a"),
    ("adminaction.rangeban", "+r has been range banned by +a"),
    ("adminaction.rangeunban", "+r has been range unbanned by +a"),
    ("adminaction.disableavatar", "+n's avatar was disabled by +a"),
    ("adminaction.changemessage", "+n's personal message was set by +a"),
    ("adminaction.asnban", "+r has been ASN banned by +a"),
    ("adminaction.asnunban", "+r has been unbanned by +a"),
    // Listado de admins (Category.AdminList de sb0t; se difunde a la sala)
    ("adminlist.header", "ADMIN LIST REQUESTED BY [+n]"),
    ("adminlist.entry", "Level +l : +n"),
    ("adminlist.footer", "List Complete"),
    // Announce (Notification#19: aviso a mods de quién anunció)
    ("announce.by", "+a announced"),
    // Clearscreen (Notification#14)
    ("clearscreen.by", "screen cleared by +n"),
    // Whois (Category.Whois #0-9 de sb0t)
    ("whois.name", "Name: +n"),
    ("whois.orgname", "Original Name: +n"),
    ("whois.asn", "ASN: +n"),
    ("whois.extip", "External IP: +n"),
    ("whois.localip", "Local IP: +n"),
    ("whois.dataport", "Data Port: +n"),
    ("whois.version", "Version: +n"),
    ("whois.vroom", "Vroom: +n"),
    ("whois.id", "ID: +n"),
    ("whois.linked", "Linked: +n"),
    ("whois.registered", "Registered: +n"),
    // Shout (Messaging#0 de sb0t)
    ("shout.line", "+n> [SHOUT] +t"),
    // Avisos a mods (Notification #15/#16 de sb0t)
    ("clone.by", "+n was cloned by +a"),
    ("oldname.by", "+n has had their original name restored by +a"),
    ("move.by", "+n was moved to vroom +v by +a"),
    // Status actualizado (RoomInfo#6 de sb0t)
    ("status.updated", "+n has updated the host status"),
    // Bloque de info de sala (Category.RoomInfo #0-5 de sb0t; el `/roominfo`
    // y el broadcast periódico los renderizan con el placeholder `+n`).
    ("roominfo.title", "Room Information"),
    ("roominfo.hosts", "Current hosts: +n"),
    ("roominfo.usercount", "Current user count: +n"),
    ("roominfo.admins", "Current admin count: +n"),
    ("roominfo.uptime", "Server uptime: +n"),
    ("roominfo.status", "Host status: +n"),
    // Whowas (Category.WhoWas de sb0t)
    ("whowas.entry", "whowas: +n +ip +v +t"),
    ("whowas.none", "no results were found containing +n"),
    // Autologin (AdminLogin #4/#5 de sb0t)
    ("autologin.added", "+n has been added to auto login as a level +l admin"),
    ("autologin.removed", "+n has been removed from auto login"),
    // Locate (Category.Locate de sb0t: quién está en qué vroom)
    ("locate.header", "vroom location list"),
    ("locate.entry", "+n is currently in vroom +v"),
    ("locate.footer", "end of list"),
    ("locate.empty", "location list empty"),

    // -- Mensajes generales del sistema (errores, usos, avisos y
    //    confirmaciones sin valores interpolados). Se auto-resuelven en
    //    `send_system_line` por coincidencia exacta del texto por defecto,
    //    sin tocar cada call site. --
    ("sys.admin_commands_are_currently_disabled", "Admin commands are currently disabled."),
    ("sys.usage_nick_name", "Usage: /nick <name>"),
    ("sys.nickname_too_long", "Nickname too long."),
    ("sys.you_already_have_that_nickname", "You already have that nickname."),
    ("sys.nickname_already_in_use", "Nickname already in use."),
    ("sys.nickname_updated", "Nickname updated."),
    ("sys.usage_vroom_id", "Usage: /vroom <id>"),
    ("sys.you_are_already_in_that_vroom", "You are already in that vroom."),
    ("sys.custom_name_is_not_set", "Custom name is not set."),
    ("sys.custom_name_cleared", "Custom name cleared."),
    ("sys.topic_updated", "Topic updated."),
    ("sys.no_motd_is_set", "No MOTD is set."),
    ("sys.motd_updated", "MOTD updated."),
    ("sys.usage_ban_nick", "Usage: /ban <nick>"),
    ("sys.failed_to_persist_ban", "Failed to persist ban."),
    ("sys.usage_unkiddy_nick", "Usage: /unkiddy <nick>"),
    ("sys.usage_unban_nick_ip_ident", "Usage: /unban <nick|ip|ident>"),
    ("sys.ban_list_is_empty", "Ban list is empty."),
    ("sys.active_bans", "Active bans:"),
    ("sys.usage_whois_nick", "Usage: /whois <nick>"),
    ("sys.usage_kick_nick", "Usage: /kick <nick>"),
    ("sys.you_cannot_kick_a_user_of_equal", "You cannot kick a user of equal or higher level."),
    ("sys.you_cannot_muzzle_a_user_of_equal", "You cannot muzzle a user of equal or higher level."),
    ("sys.usage_pmall_text", "Usage: /pmall <text>"),
    ("sys.usage_opmsg_text", "Usage: /opmsg <text>"),
    ("sys.registration_is_disabled_in_this_room", "Registration is disabled in this room."),
    ("sys.usage_register_password_4_chars", "Usage: /register <password> (4+ chars)"),
    ("sys.already_registered_use_unregister_first", "Already registered. Use /unregister first."),
    ("sys.registration_failed_database_error", "Registration failed (database error)."),
    ("sys.account_registered_use_login_password", "Account registered. Use /login <password>."),
    ("sys.usage_whisper_nick_text", "Usage: /whisper <nick> <text>"),
    ("sys.account_deleted", "Account deleted."),
    ("sys.you_are_not_registered", "You are not registered."),
    ("sys.unregister_failed_database_error", "Unregister failed (database error)."),
    ("sys.usage_login_password", "Usage: /login <password>"),
    ("sys.logged_in_as_owner_level_unchanged", "Logged in as Owner (level unchanged)."),
    ("sys.invalid_password", "Invalid password."),
    ("sys.logged_in_level_unchanged", "Logged in (level unchanged)."),
    ("sys.you_cannot_modify_a_user_of_equal", "You cannot modify a user of equal or higher level."),
    ("sys.you_cannot_grant_a_level_equal_or", "You cannot grant a level equal or above your own."),
    ("sys.usage_revoke_nick", "Usage: /revoke <nick>"),
    ("sys.access_denied_owner_required", "Access denied. Owner required."),
    ("sys.usage_addautologin_nick_moderator_admin", "Usage: /addautologin <nick> <moderator|admin>"),
    ("sys.usage_remautologin_id", "Usage: /remautologin <id>"),
    ("sys.no_autologin_entry_with_that_id", "No autologin entry with that id."),
    ("sys.autologin_entry_removed", "Autologin entry removed."),
    ("sys.no_ip_autologin_entries", "No IP autologin entries."),
    ("sys.usage_cmdlevel_command_level_reset", "Usage: /cmdlevel <command> [level|reset]"),
    ("sys.no_command_levels_are_overridden_all_at", "No command levels are overridden (all at default)."),
    ("sys.host_action_propagated_to_linked_servers", "Host action propagated to linked servers."),
    ("sys.access_denied_host_owner_required", "Access denied. Host (Owner) required."),
    ("sys.greets_enabled", "Greets enabled."),
    ("sys.greets_disabled", "Greets disabled."),
    ("sys.usage_greets_on_off", "Usage: /greets [on|off]"),
    ("sys.usage_addgreet_text_placeholders_n_ip_id", "Usage: /addgreet <text>  (placeholders: +n +ip +id +f +v +uc +rn +ut +l)"),
    ("sys.failed_to_persist_greet", "Failed to persist greet."),
    ("sys.usage_remgreet_index", "Usage: /remgreet <index>"),
    ("sys.no_greet_at_that_index", "No greet at that index."),
    ("sys.no_greets_configured", "No greets configured."),
    ("sys.usage_addfilter_word_block_kick_ban_announce", "Usage: /addfilter <word> [block|kick|ban|announce]"),
    ("sys.usage_remfilter_word", "Usage: /remfilter <word>"),
    ("sys.no_matching_filter", "No matching filter."),
    ("sys.no_word_filters_configured", "No word filters configured."),
    ("sys.usage_addline_index_text", "Usage: /addline <index>, <text>"),
    ("sys.no_filter_at_that_index", "No filter at that index."),
    ("sys.usage_remline_index_line", "Usage: /remline <index>, <line>"),
    ("sys.no_such_filter_or_line_index", "No such filter or line index."),
    ("sys.usage_viewfilter_index", "Usage: /viewfilter <index>"),
    ("sys.this_filter_has_no_lines_yet", "This filter has no lines yet."),
    ("sys.this_filter_is_not_an_announce_type", "This filter is not an announce-type filter."),
    ("sys.room_urls_enabled", "Room URLs enabled."),
    ("sys.room_urls_disabled", "Room URLs disabled."),
    ("sys.usage_url_on_off", "Usage: /url [on|off]"),
    ("sys.usage_addurl_address_text", "Usage: /addurl <address> <text>"),
    ("sys.failed_to_persist_url", "Failed to persist URL."),
    ("sys.usage_remurl_index", "Usage: /remurl <index>"),
    ("sys.no_url_at_that_index", "No URL at that index."),
    ("sys.no_room_urls_configured", "No room URLs configured."),
    ("sys.no_message_history_yet", "No message history yet."),
    ("sys.usage_whowas_nick_ip", "Usage: /whowas <nick|ip>"),
    ("sys.no_matching_history", "No matching history."),
    ("sys.usage_lastseen_nick_ip", "Usage: /lastseen <nick|ip>"),
    ("sys.room_status_is_not_set", "Room status is not set."),
    ("sys.room_status_cleared", "Room status cleared."),
    ("sys.no_users_have_a_custom_name_set", "No users have a custom name set."),
    ("sys.usage_rangeban_ip_prefix", "Usage: /rangeban <ip-prefix>"),
    ("sys.range_ban_already_exists_or_invalid", "Range ban already exists (or invalid)."),
    ("sys.usage_rangeunban_ip_prefix_index", "Usage: /rangeunban <ip-prefix|index>"),
    ("sys.range_ban_removed", "Range ban removed."),
    ("sys.no_matching_range_ban", "No matching range ban."),
    ("sys.no_range_bans", "No range bans."),
    ("sys.usage_asnban_asn", "Usage: /asnban <asn>"),
    ("sys.asn_already_banned_or_invalid", "ASN already banned (or invalid)."),
    ("sys.usage_asnunban_asn", "Usage: /asnunban <asn>"),
    ("sys.asn_not_banned", "ASN not banned."),
    ("sys.no_asn_bans", "No ASN bans."),
    ("sys.usage_move_nick_vroom", "Usage: /move <nick> <vroom>"),
    ("sys.you_cannot_move_a_user_of_equal", "You cannot move a user of equal or higher level."),
    ("sys.user_is_already_in_that_vroom", "User is already in that vroom."),
    ("sys.usage_changename_nick_newname", "Usage: /changename <nick> <newname>"),
    ("sys.new_name_too_long", "New name too long."),
    ("sys.that_name_is_already_in_use", "That name is already in use."),
    ("sys.usage_oldname_nick", "Usage: /oldname <nick>"),
    ("sys.usage_changemessage_nick_text", "Usage: /changemessage <nick> <text>"),
    ("sys.no_ops_online", "No ops online."),
    ("sys.usage_announce_text", "Usage: /announce <text>"),
    ("sys.usage_echo_nick_text_empty_text_clears", "Usage: /echo <nick> [text]  (empty text clears)"),
    ("sys.you_cannot_echo_a_user_of_equal", "You cannot echo a user of equal or higher level."),
    ("sys.usage_clone_nick_text", "Usage: /clone <nick> <text>"),
    ("sys.usage_kiddy_nick", "Usage: /kiddy <nick>"),
    ("sys.you_cannot_kiddy_a_user_of_equal", "You cannot kiddy a user of equal or higher level."),
    ("sys.usage_mtimeout_nick_seconds", "Usage: /mtimeout <nick> <seconds>"),
    ("sys.usage_redirect_nick_ip_port", "Usage: /redirect <nick> <ip:port>"),
    ("sys.you_cannot_redirect_a_user_of_equal", "You cannot redirect a user of equal or higher level."),
    ("sys.destination_must_be_ip_port", "Destination must be ip:port."),
    ("sys.invalid_ip_port", "Invalid ip:port."),
    ("sys.avatars_disabled", "Avatars disabled."),
    ("sys.avatars_enabled", "Avatars enabled."),
    ("sys.usage_disableavatar_on_off", "Usage: /disableavatar [on|off]"),
    ("sys.room_flags", "Room flags:"),
    ("sys.cloak_enabled", "Cloak enabled."),
    ("sys.cloak_disabled", "Cloak disabled."),
    ("sys.usage_cloak_on_off", "Usage: /cloak [on|off]"),
    ("sys.screen_cleared", "Screen cleared."),
    ("sys.usage_locate_nick", "Usage: /locate <nick>"),
    ("sys.no_quarantined_users", "No quarantined users."),
    ("sys.usage_unquarantine_nick_index", "Usage: /unquarantine <nick|index>"),
    ("sys.no_matching_quarantined_user", "No matching quarantined user."),
    ("sys.no_registered_accounts", "No registered accounts."),
    ("sys.filter_already_exists_or_invalid", "Filter already exists (or invalid)."),
    ("sys.usage_filter_add_word_block_kick_ban", "Usage: /filter [add <word> [block|kick|ban]|del <word>|list]"),
    ("sys.usage_link_name_server_port", "Usage: /link <name> <server> <port>"),
    ("sys.invalid_port", "Invalid port."),
    ("sys.link_subsystem_is_not_running", "Link subsystem is not running."),
    ("sys.usage_unlink_name", "Usage: /unlink <name>"),
    ("sys.scripting_is_not_available", "Scripting is not available."),
    ("sys.no_scripts_loaded", "No scripts loaded."),
    ("sys.usage_loadscript_name", "Usage: /loadscript <name>"),
    ("sys.usage_killscript_name", "Usage: /killscript <name>"),
    ("sys.usage_define_word", "Usage: /define <word>"),
    ("sys.usage_urban_term", "Usage: /urban <term>"),
    ("sys.usage_trace_nick_ip", "Usage: /trace <nick|ip>"),
    ("sys.user_not_found_or_invalid_ip", "User not found (or invalid IP)."),
    ("sys.usage_effect_nick", "Usage: /<effect> <nick>"),
    ("sys.you_cannot_target_a_user_of_equal", "You cannot target a user of equal or higher level."),
];

/// Manager de textos del sistema: defaults en el binario + overrides en SQLite.
pub struct TemplateManager {
    db: Arc<Database>,
    /// Overrides por clave (solo las que el admin cambió).
    overrides: RwLock<HashMap<String, String>>,
    /// Índice inverso `texto por defecto → clave`, para resolver un mensaje
    /// estático (que el call site pasa como literal) a su override sin tener
    /// que keyear cada call site. Se arma una vez desde [`TEMPLATE_DEFAULTS`].
    by_default: HashMap<&'static str, &'static str>,
}

impl TemplateManager {
    /// Crea el manager cargando los overrides guardados.
    pub fn new(db: Arc<Database>) -> Self {
        let mut overrides = HashMap::new();
        for (k, v) in db.list_templates().unwrap_or_default() {
            // Ignorar overrides de claves que ya no existen en el catálogo.
            if is_valid_key(&k) {
                overrides.insert(k, v);
            }
        }
        let mut by_default = HashMap::new();
        for (k, def) in TEMPLATE_DEFAULTS {
            // Si dos entradas compartieran default (no debería), gana la primera.
            by_default.entry(*def).or_insert(*k);
        }
        Self {
            db,
            overrides: RwLock::new(overrides),
            by_default,
        }
    }

    /// Resuelve un mensaje del sistema pasado como texto literal: si su texto
    /// coincide exactamente con el default de alguna clave del catálogo,
    /// devuelve el override configurado (o el mismo default); si no está en el
    /// catálogo, devuelve el texto tal cual. Esto permite que TODOS los
    /// mensajes estáticos sean editables sin keyear cada call site — se llama
    /// una vez, de forma centralizada, en `send_system_line`.
    pub fn resolve(&self, text: &str) -> String {
        match self.by_default.get(text) {
            Some(key) => self.get(key),
            None => text.to_string(),
        }
    }

    /// Texto actual de una clave (override o default). Si la clave no existe
    /// en el catálogo, devuelve la clave misma (nunca debería pasar en
    /// call sites correctos, pero evita panics).
    pub fn get(&self, key: &str) -> String {
        if let Some(v) = self.overrides.read().get(key) {
            return v.clone();
        }
        default_for(key).unwrap_or(key).to_string()
    }

    /// ¿Existe esa clave en el catálogo (o tiene un override)? Sirve para los
    /// textos opcionales, como el anuncio de un toggle que solo existe en
    /// Astra y no tiene equivalente en sb0t.
    pub fn has(&self, key: &str) -> bool {
        default_for(key).is_some() || self.overrides.read().contains_key(key)
    }

    /// Texto de una clave con los placeholders sustituidos.
    pub fn render(&self, key: &str, subs: &[(&str, &str)]) -> String {
        let mut s = self.get(key);
        for (ph, val) in subs {
            s = s.replace(ph, val);
        }
        s
    }

    /// Setea (o borra, si `text` coincide con el default) el override de una
    /// clave. Retorna `false` si la clave no existe en el catálogo.
    pub fn set(&self, key: &str, text: &str) -> bool {
        let Some(def) = default_for(key) else {
            return false;
        };
        if text == def {
            // Igual al default → no guardamos override (o borramos el que había).
            self.overrides.write().remove(key);
            let _ = self.db.remove_template(key);
        } else {
            self.overrides.write().insert(key.to_string(), text.to_string());
            let _ = self.db.set_template(key, text);
        }
        true
    }

    /// Restaura una clave a su default (borra el override).
    pub fn reset(&self, key: &str) {
        self.overrides.write().remove(key);
        let _ = self.db.remove_template(key);
    }

    /// Lista el catálogo completo para el panel:
    /// `(key, default, current, is_override)`, en el orden de
    /// [`TEMPLATE_DEFAULTS`].
    pub fn list(&self) -> Vec<(String, String, String, bool)> {
        let ov = self.overrides.read();
        TEMPLATE_DEFAULTS
            .iter()
            .map(|(k, def)| {
                let cur = ov.get(*k).cloned();
                let is_ov = cur.is_some();
                (
                    k.to_string(),
                    def.to_string(),
                    cur.unwrap_or_else(|| def.to_string()),
                    is_ov,
                )
            })
            .collect()
    }

    /// Aplica en bloque un texto con líneas `key = valor` (el formato que
    /// edita el panel). Las claves desconocidas o líneas vacías/comentario
    /// (`#`) se ignoran. Las claves del catálogo que NO aparezcan en el
    /// texto se dejan como están (no se resetean), para que el panel pueda
    /// mandar solo lo que cambió si quisiera. Retorna cuántas se aplicaron.
    pub fn apply_bulk(&self, text: &str) -> usize {
        let mut n = 0;
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim_start(); // preservar espacios finales intencionales igual no aporta; sí quitamos el de después del '='
            if self.set(key, val.trim_end()) {
                n += 1;
            }
        }
        n
    }

    /// Exporta el catálogo como texto editable (`key = valor` por línea, con
    /// el valor actual), para precargar el textarea del panel.
    pub fn export_text(&self) -> String {
        let mut out = String::new();
        for (k, _def, cur, _ov) in self.list() {
            out.push_str(&k);
            out.push_str(" = ");
            out.push_str(&cur);
            out.push('\n');
        }
        out
    }
}

fn is_valid_key(key: &str) -> bool {
    TEMPLATE_DEFAULTS.iter().any(|(k, _)| *k == key)
}

fn default_for(key: &str) -> Option<&'static str> {
    TEMPLATE_DEFAULTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn default_when_no_override() {
        let m = TemplateManager::new(mem_db());
        assert_eq!(m.get("kick.target"), "You have been kicked from this room.");
    }

    #[test]
    fn render_substitutes() {
        let m = TemplateManager::new(mem_db());
        assert_eq!(m.render("kick.confirm", &[("+n", "Bob")]), "Kicked 'Bob'.");
        assert_eq!(
            m.render("ban.confirm", &[("+n", "Bob"), ("+i", "7")]),
            "Banned 'Bob' (ident 7)."
        );
    }

    #[test]
    fn set_override_and_persist() {
        let db = mem_db();
        {
            let m = TemplateManager::new(db.clone());
            assert!(m.set("kick.confirm", "Expulsé a +n."));
            assert_eq!(m.render("kick.confirm", &[("+n", "Ana")]), "Expulsé a Ana.");
        }
        let m2 = TemplateManager::new(db);
        assert_eq!(m2.get("kick.confirm"), "Expulsé a +n.");
    }

    #[test]
    fn set_unknown_key_fails() {
        let m = TemplateManager::new(mem_db());
        assert!(!m.set("nope.nope", "x"));
    }

    #[test]
    fn set_to_default_clears_override() {
        let db = mem_db();
        let m = TemplateManager::new(db.clone());
        m.set("kick.target", "Otro texto");
        assert!(m.list().iter().find(|e| e.0 == "kick.target").unwrap().3); // is_override
        m.set("kick.target", "You have been kicked from this room.");
        assert!(!m.list().iter().find(|e| e.0 == "kick.target").unwrap().3);
        // y no quedó en la DB
        let m2 = TemplateManager::new(db);
        assert!(!m2.list().iter().find(|e| e.0 == "kick.target").unwrap().3);
    }

    #[test]
    fn reset_restores_default() {
        let m = TemplateManager::new(mem_db());
        m.set("muzzle.target", "silenciado");
        m.reset("muzzle.target");
        assert_eq!(m.get("muzzle.target"), "You have been muzzled.");
    }

    #[test]
    fn apply_bulk_and_export_roundtrip() {
        let m = TemplateManager::new(mem_db());
        let n = m.apply_bulk("kick.confirm = Fuera +n\n# comentario\n\nban.target = Estás baneado\ndesconocida = x");
        assert_eq!(n, 2); // dos claves válidas (la desconocida no cuenta)
        assert_eq!(m.get("kick.confirm"), "Fuera +n");
        assert_eq!(m.get("ban.target"), "Estás baneado");
        // export contiene las líneas actualizadas
        let txt = m.export_text();
        assert!(txt.contains("kick.confirm = Fuera +n"));
        assert!(txt.contains("ban.target = Estás baneado"));
    }

    #[test]
    fn list_covers_full_catalog() {
        let m = TemplateManager::new(mem_db());
        assert_eq!(m.list().len(), TEMPLATE_DEFAULTS.len());
    }

    #[test]
    fn resolve_maps_static_text_to_override() {
        let m = TemplateManager::new(mem_db());
        // sin override: devuelve el mismo texto
        assert_eq!(m.resolve("User not found."), "User not found.");
        // con override sobre la clave cuyo default es ese texto
        m.set("error.user_not_found", "No existe ese usuario.");
        assert_eq!(m.resolve("User not found."), "No existe ese usuario.");
        // texto no catalogado pasa tal cual
        assert_eq!(m.resolve("Kicked 'Bob'."), "Kicked 'Bob'.");
    }

    #[test]
    fn every_default_is_resolvable() {
        // Todos los deferauls del catálogo deben resolver por su propio texto
        // (garantiza que el índice inverso no perdió ninguno por colisión).
        let m = TemplateManager::new(mem_db());
        for (_k, def) in TEMPLATE_DEFAULTS {
            assert_eq!(&m.resolve(def), def);
        }
    }
}
