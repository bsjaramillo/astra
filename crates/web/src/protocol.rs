//! Protocolo de mensajes entre el server Astra y los clientes web (ib0t).
//!
//! El formato es texto plano con un opcode + `:` + args.
//! Ejemplos:
//!   - `LOGIN:1,32,8:ib0t 5.43ffffffffffffffffffffffffffffffffAliceen-USib0t`
//!   - `PUBLIC:Alice,hola mundo`
//!   - `JOIN:0,5,100,...:Alice`
//!
//! Para los args que tienen sub-args de longitud variable (como JOIN), se
//! precede con la lista de longitudes separadas por coma:
//!   `JOIN:1,5,1,1,1,1,1,5,1,1,1,1,1,1,1:0,100,5,1.2.3.4,1234,...`
//! Esto permite al parser reconstruir los strings sin ambigüedad.

#![allow(dead_code)]

use std::net::IpAddr;
use std::net::Ipv4Addr;

/// Opcode de un mensaje del cliente al server (entrante).
pub mod incoming {
    pub const LOGIN: &str = "LOGIN";
    pub const PUBLIC: &str = "PUBLIC";
    pub const EMOTE: &str = "EMOTE";
    pub const PM: &str = "PM";
    pub const PING: &str = "PING";
    pub const COMMAND: &str = "COMMAND";
    pub const AVATAR: &str = "AVATAR";
    pub const CUSTOM_DATA_HEAD: &str = "CUSTOM_DATA_HEAD";
    pub const CUSTOM_DATA_BODY: &str = "CUSTOM_DATA_BODY";
}

/// Opcode de un mensaje del server al cliente (saliente).
pub mod outgoing {
    pub const ACK: &str = "ACK";
    pub const MYFEATURES: &str = "MYFEATURES";
    pub const TOPIC: &str = "TOPIC";
    pub const JOIN: &str = "JOIN";
    pub const PART: &str = "PART";
    pub const USERLIST: &str = "USERLIST";
    pub const USERLIST_END: &str = "USERLIST_END";
    pub const PUBLIC: &str = "PUBLIC";
    pub const EMOTE: &str = "EMOTE";
    pub const PM: &str = "PM";
    pub const OPCHANGE: &str = "OPCHANGE";
    pub const NOSUCH: &str = "NOSUCH";
    pub const PMBLOB: &str = "PMBLOB";
    pub const ROOM_SCRIBBLES: &str = "ROOM_SCRIBBLES";
    pub const AVATAR: &str = "AVATAR";
    pub const AVATAR_END: &str = "AVATAR_END";
}

/// Parsea un mensaje entrante del cliente. Retorna `(opcode, args)`.
pub fn parse_incoming(text: &str) -> Option<(&str, &str)> {
    let i = text.find(':')?;
    Some((&text[..i], &text[i + 1..]))
}

/// Construye un mensaje saliente. Formato: `IDENT:args`.
pub fn build(ident: &str, args: &str) -> String {
    format!("{}:{}", ident, args)
}

/// Construye un mensaje con args de longitud variable. Formato:
/// `IDENT:len1,len2,...:arg1arg2...`
pub fn build_with_lens(ident: &str, args: &[&str]) -> String {
    let lens: Vec<String> = args.iter().map(|a| a.chars().count().to_string()).collect();
    let lens_str = lens.join(",");
    let mut s = format!("{}:{}:", ident, lens_str);
    for a in args {
        s.push_str(a);
    }
    s
}

/// Parsea args de longitud variable. Acepta dos formatos:
/// - `len1,len2:arg1arg2...` (solo los args)
/// - `IDENT:len1,len2:arg1arg2...` (paquete completo con IDENT)
pub fn parse_lens_args(text: &str) -> Option<Vec<String>> {
    let first_colon = text.find(':')?;
    let before = &text[..first_colon];
    let after = &text[first_colon + 1..];

    // Detectar si `before` es lens (solo dígitos y comas) o un IDENT
    let (lens_str, data) = if before.chars().all(|c| c.is_ascii_digit() || c == ',') && !before.is_empty() {
        // Formato: `len1,len2:args`
        (before, after)
    } else {
        // Formato: `IDENT:len1,len2:args`
        let second_colon = after.find(':')?;
        let lens_candidate = &after[..second_colon];
        if !lens_candidate.chars().all(|c| c.is_ascii_digit() || c == ',') {
            return None;
        }
        (lens_candidate, &after[second_colon + 1..])
    };

    if data.is_empty() {
        return None;
    }

    let lens: Vec<usize> = lens_str
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<_>>>()?;

    let chars: Vec<char> = data.chars().collect();
    let total = chars.len();
    let mut result = Vec::with_capacity(lens.len());
    let mut pos = 0;
    for l in lens {
        if pos + l > total {
            return None;
        }
        let s: String = chars[pos..pos + l].iter().collect();
        result.push(s);
        pos += l;
    }
    Some(result)
}

// ============================================================================
// Constructores de mensajes salientes
// ============================================================================

/// Construye `ACK:nombre,roomname,version`.
pub fn build_ack(name: &str, room_name: &str, version: &str) -> String {
    build(outgoing::ACK, &format!("{},{},{}", name, room_name, version))
}

/// Construye `MYFEATURES:version_str,flags,unknown,lang,cookie,unknown`.
///
/// `flags` es un byte (0x1f = PVT|sharing|compression|VC|opus).
pub fn build_myfeatures(version: &str, flags: u8, language: u8, cookie: u32) -> String {
    build(
        outgoing::MYFEATURES,
        &format!(
            "{},{},0,{},{},1",
            version, flags, language, cookie
        ),
    )
}

/// Construye `TOPIC:text`.
pub fn build_topic(text: &str) -> String {
    build(outgoing::TOPIC, text)
}

/// Construye `JOIN:args_con_lens`.
pub fn build_join(args: &[&str]) -> String {
    build_with_lens(outgoing::JOIN, args)
}

/// Construye `PART:name`.
pub fn build_part(name: &str) -> String {
    build(outgoing::PART, name)
}

/// Construye `USERLIST:args_con_lens`.
pub fn build_userlist(args: &[&str]) -> String {
    build_with_lens(outgoing::USERLIST, args)
}

/// Construye `USERLIST_END:`.
pub fn build_userlist_end() -> String {
    format!("{}:", outgoing::USERLIST_END)
}

/// Construye `PUBLIC:from,text`.
pub fn build_public(from: &str, text: &str) -> String {
    build(outgoing::PUBLIC, &format!("{},{}", from, text))
}

/// Construye `EMOTE:from,text`.
pub fn build_emote(from: &str, text: &str) -> String {
    build(outgoing::EMOTE, &format!("{},{}", from, text))
}

/// Construye `PM:from,text`.
pub fn build_pm(from: &str, text: &str) -> String {
    build(outgoing::PM, &format!("{},{}", from, text))
}

/// Construye `OPCHANGE:level`.
pub fn build_opchange(level: u8) -> String {
    build(outgoing::OPCHANGE, &level.to_string())
}

/// Construye `NOSUCH:reason`.
pub fn build_nosuch(reason: &str) -> String {
    build(outgoing::NOSUCH, reason)
}

/// Construye un userlist item (mismo formato que JOIN).
pub fn build_user_item(
    port: u16,
    users: u16,
    file_count: u16,
    external_ip: IpAddr,
    data_port: u16,
    node_ip: IpAddr,
    node_port: u16,
    name: &str,
    local_ip: IpAddr,
    browsable: bool,
    level: u8,
    age: u8,
    sex: u8,
    country: u8,
    region: &str,
    features: u8,
) -> String {
    let ext = ip_to_str(external_ip);
    let node = ip_to_str(node_ip);
    let local = ip_to_str(local_ip);

    let args = [
        port.to_string(),
        users.to_string(),
        file_count.to_string(),
        ext,
        data_port.to_string(),
        node,
        node_port.to_string(),
        "0".to_string(),
        name.to_string(),
        local,
        if browsable { "1" } else { "0" }.to_string(),
        level.to_string(),
        age.to_string(),
        sex.to_string(),
        country.to_string(),
        region.to_string(),
        features.to_string(),
    ];
    let s: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    build_userlist(&s)
}

/// Construye `USERLIST_BOT:name` (la línea del bot fantasma).
pub fn build_userlist_bot(name: &str) -> String {
    let args: Vec<String> = [
        "0", "0", "0", "0.0.0.0", "69", "0.0.0.0", "0", "0", name, "0.0.0.0", "1", "3", "0", "0", "0", "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let s: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    build_userlist(&s)
}

fn ip_to_str(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(_) => Ipv4Addr::UNSPECIFIED.to_string(),
    }
}

// ============================================================================
// Parser de LOGIN (args de longitud variable)
// ============================================================================

/// Datos parseados de un LOGIN.
#[derive(Debug, Clone)]
pub struct LoginArgs {
    /// Versión del cliente (ej "ib0t 5.43" o "2000" / "5000" / "6000" para variantes)
    pub version: String,
    /// GUID como bytes crudos
    pub guid: [u8; 16],
    /// Nick
    pub name: String,
    /// Código de idioma
    pub lang: String,
    /// Personal message / detalle
    pub pmsg: String,
    /// Inbizier web (versión 5000)
    pub inbizier_web: bool,
    /// Inbizier mobile (versión 6000)
    pub inbizier_mobile: bool,
}

/// Parsea los args de un LOGIN. Formato:
/// `LOGIN:1,32,5,2,4:2000ffffffffffffffffffffffffffffffffAliceen-USpersonal`
pub fn parse_login(args_text: &str) -> Option<LoginArgs> {
    let items = parse_lens_args(args_text)?;
    if items.len() < 3 {
        return None;
    }

    let version = items[0].clone();
    let guid_hex = &items[1];
    // El cliente Ares/ib0t manda 32 hex chars (16 bytes); algunos clientes
    // web (ej. "inbizio web") mandan 64 hex chars (32 bytes). Tomamos los
    // primeros 16 bytes en cualquier caso (paridad con sb0t WebProcessor).
    if guid_hex.len() < 32 {
        return None;
    }
    let mut guid = [0u8; 16];
    let bytes = guid_hex.as_bytes();
    for i in 0..16 {
        let hi = hex_nibble(bytes[i * 2])?;
        let lo = hex_nibble(bytes[i * 2 + 1])?;
        guid[i] = (hi << 4) | lo;
    }

    let name = items[2].trim().to_string();
    let lang = items.get(3).cloned().unwrap_or_default();
    let pmsg = items.get(4).cloned().unwrap_or_default();

    let inbizier_web = version == "5000";
    let inbizier_mobile = version == "6000";

    Some(LoginArgs {
        version,
        guid,
        name,
        lang,
        pmsg,
        inbizier_web,
        inbizier_mobile,
    })
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_incoming_basic() {
        let (ident, args) = parse_incoming("PUBLIC:hola mundo").unwrap();
        assert_eq!(ident, "PUBLIC");
        assert_eq!(args, "hola mundo");
    }

    #[test]
    fn parse_incoming_no_colon() {
        assert!(parse_incoming("PUBLIC").is_none());
    }

    #[test]
    fn build_basic() {
        assert_eq!(build("PUBLIC", "hola"), "PUBLIC:hola");
    }

    #[test]
    fn build_with_lens_single() {
        let s = build_with_lens("TEST", &["hola"]);
        assert_eq!(s, "TEST:4:hola");
    }

    #[test]
    fn build_with_lens_multiple() {
        let s = build_with_lens("TEST", &["abc", "de", "f"]);
        assert_eq!(s, "TEST:3,2,1:abcdef");
    }

    #[test]
    fn parse_lens_args_roundtrip() {
        let original = vec!["hello", "world", "foo bar"];
        let s = build_with_lens("X", &original);
        let parsed = parse_lens_args(&s).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn parse_login_basic() {
        // Versión "2000" (4), GUID 32B "AA..AA" (32), name "Alice" (5), lang "en-US" (5), pmsg "hello" (5)
        let guid = "A".repeat(32);
        let s = format!("4,32,5,5,5:2000{}Aliceen-UShello", guid);
        let login = parse_login(&s).unwrap();
        assert_eq!(login.version, "2000");
        assert_eq!(login.guid[0], 0xAA);
        assert_eq!(login.guid[15], 0xAA);
        assert_eq!(login.name, "Alice");
        assert_eq!(login.lang, "en-US");
        assert_eq!(login.pmsg, "hello");
        assert!(!login.inbizier_web);
        assert!(!login.inbizier_mobile);
    }

    #[test]
    fn parse_login_inbizio_64char_guid() {
        // Login real de un cliente "inbizio web": guid de 64 hex chars (32B),
        // useragent largo, y campos pmsg/avatar. Debe parsear (tomando los
        // primeros 16 bytes del guid).
        let args = "4,64,13,109,18,18,0:6000000d7bcb2510574ad8128d95a74679466e6223af1bcb518460b87c162fae6469ElMagoDelSiamMozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15inbizio web v0.1.5inbizio web v0.1.5";
        let login = parse_login(args).expect("inbizio login debe parsear");
        assert_eq!(login.version, "6000");
        assert!(login.inbizier_mobile);
        assert_eq!(login.name, "ElMagoDelSiam");
        assert_eq!(login.guid[0], 0x00);
        assert_eq!(login.guid[1], 0x0d);
        assert_eq!(login.guid[2], 0x7b);
    }

    #[test]
    fn parse_login_rejects_short_guid() {
        // guid de menos de 32 hex chars → inválido.
        let args = "4,8,5:2000deadbeefAlice";
        assert!(parse_login(args).is_none());
    }

    #[test]
    fn parse_login_inbizier() {
        // version="5000" (4), guid=32B, name="WebUser" (7), lang="es" (2)
        let guid = "BB".repeat(16);
        let s = format!("4,32,7,2:5000{}WebUseres", guid);
        let login = parse_login(&s).unwrap();
        assert!(login.inbizier_web);
        assert!(!login.inbizier_mobile);
    }

    #[test]
    fn test_build_userlist_bot() {
        let s = build_userlist_bot("MyBot");
        assert!(s.starts_with("USERLIST:"));
    }

    #[test]
    fn parse_lens_args_with_unicode() {
        // Strings con chars multi-byte
        let s = build_with_lens("X", &["héllo", "wörld"]);
        let parsed = parse_lens_args(&s).unwrap();
        assert_eq!(parsed, vec!["héllo", "wörld"]);
    }
}
