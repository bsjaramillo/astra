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
// Constructores de mensajes salientes (formato ib0t/sb0t)
// ============================================================================
//
// Todos son length-prefixed: `IDENT:len1,len2,...:val1val2...` donde cada
// `len` es el conteo de chars del valor concatenado a continuación. El nivel
// se envía como el valor decimal del byte ("0", "50", "100"), igual que
// `WebOutbound.cs` de sb0t.

fn clen(s: &str) -> usize {
    s.chars().count()
}

/// `ACK:{len}:{name}` — ack de login.
pub fn build_ack(name: &str) -> String {
    format!("ACK:{}:{}", clen(name), name)
}

/// `SERVER_INFO:2:II` — info del server para clientes inbizier.
pub fn build_server_info() -> String {
    "SERVER_INFO:2:II".to_string()
}

/// `TOPIC_FIRST:{len}:{text}` — topic enviado al entrar.
pub fn build_topic_first(text: &str) -> String {
    format!("TOPIC_FIRST:{}:{}", clen(text), text)
}

/// `TOPIC:{len}:{text}` — cambio de topic en runtime.
pub fn build_topic(text: &str) -> String {
    format!("TOPIC:{}:{}", clen(text), text)
}

/// `USERINFO:...` — info detallada de un usuario (userlist para inbizier).
pub fn build_userinfo(
    name: &str,
    pmsg: &str,
    avatar_b64: &str,
    id: u16,
    level: u8,
    inbizier_web: bool,
    inbizier_mobile: bool,
) -> String {
    userinfo_like("USERINFO", name, pmsg, avatar_b64, id, level, inbizier_web, inbizier_mobile)
}

/// `JOININFO:...` — igual que USERINFO, cuando alguien entra.
pub fn build_joininfo(
    name: &str,
    pmsg: &str,
    avatar_b64: &str,
    id: u16,
    level: u8,
    inbizier_web: bool,
    inbizier_mobile: bool,
) -> String {
    userinfo_like("JOININFO", name, pmsg, avatar_b64, id, level, inbizier_web, inbizier_mobile)
}

fn userinfo_like(
    ident: &str,
    name: &str,
    pmsg: &str,
    avatar_b64: &str,
    id: u16,
    level: u8,
    inbizier_web: bool,
    inbizier_mobile: bool,
) -> String {
    let id_str = id.to_string();
    let web = if inbizier_web { "1" } else { "0" };
    let mobile = if inbizier_mobile { "1" } else { "0" };
    // lens: name,pmsg,avatar,id,1,1,1 ; vals: name+pmsg+avatar+id+level+web+mobile
    format!(
        "{}:{},{},{},{},1,1,1:{}{}{}{}{}{}{}",
        ident,
        clen(name),
        clen(pmsg),
        clen(avatar_b64),
        clen(&id_str),
        name,
        pmsg,
        avatar_b64,
        id_str,
        level,
        web,
        mobile
    )
}

/// `USERLIST:{nameLen},1:{name}{level}` — item de userlist simple (no inbizier).
pub fn build_userlist_item(name: &str, level: u8) -> String {
    format!("USERLIST:{},1:{}{}", clen(name), name, level)
}

/// `USERLIST_END:` — fin de la userlist.
pub fn build_userlist_end() -> String {
    "USERLIST_END:".to_string()
}

/// `OFFLINE:{len}:{name}` — un usuario salió (part).
pub fn build_offline(name: &str) -> String {
    format!("OFFLINE:{}:{}", clen(name), name)
}

/// `PUBLIC:{nameLen},{textLen}:{name}{text}`.
pub fn build_public(name: &str, text: &str) -> String {
    format!("PUBLIC:{},{}:{}{}", clen(name), clen(text), name, text)
}

/// `EMOTE:{nameLen},{textLen}:{name}{text}`.
pub fn build_emote(name: &str, text: &str) -> String {
    format!("EMOTE:{},{}:{}{}", clen(name), clen(text), name, text)
}

/// `PM:{nameLen},{textLen}:{name}{text}`.
pub fn build_pm(name: &str, text: &str) -> String {
    format!("PM:{},{}:{}{}", clen(name), clen(text), name, text)
}

/// `UPDATE:{nameLen},1:{name}{level}` — cambio de nivel de un usuario.
pub fn build_update(name: &str, level: u8) -> String {
    format!("UPDATE:{},1:{}{}", clen(name), name, level)
}

/// `NOSUCH:{len}:{text}`.
pub fn build_nosuch(text: &str) -> String {
    format!("NOSUCH:{}:{}", clen(text), text)
}

/// `URL:{addrLen},{tagLen}:{addr}{tag}`.
pub fn build_url(addr: &str, tag: &str) -> String {
    format!("URL:{},{}:{}{}", clen(addr), clen(tag), addr, tag)
}

/// `PERSMSG:{nameLen},{textLen}:{name}{text}` — cambio de personal message.
pub fn build_persmsg(name: &str, text: &str) -> String {
    format!("PERSMSG:{},{}:{}{}", clen(name), clen(text), name, text)
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
    /// Código de idioma (ib0t clásico) o user-agent (inbizier)
    pub lang: String,
    /// Personal message
    pub pmsg: String,
    /// Avatar en base64 (inbizier con 7 campos; vacío si no vino o es el default)
    pub avatar_b64: String,
    /// Inbizier web (versión 5000)
    pub inbizier_web: bool,
    /// Inbizier mobile (versión 6000)
    pub inbizier_mobile: bool,
}

/// Parsea los args de un LOGIN.
///
/// ib0t clásico: `2000/{guid}/{name}/{lang}/{pmsg?}`.
/// Inbizier (5000/6000) con 7 campos (paridad `WebProcessor.Login` de sb0t):
/// `[0]=version [1]=guid_hex [2]=name [3]=useragent [4]=client_version
///  [5]=personal_message [6]=avatar_base64 (o "/default.png")`.
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

    let inbizier_web = version == "5000";
    let inbizier_mobile = version == "6000";
    let inbizier = inbizier_web || inbizier_mobile;

    // sb0t: para inbizier con 7 campos, el pmsg real es [5] y el avatar [6];
    // [4] es el string de versión del cliente (fallback de pmsg).
    let (pmsg, avatar_b64) = if inbizier && items.len() == 7 {
        let pmsg = items[5].clone();
        let av = &items[6];
        let avatar = if av.is_empty() || av == "/default.png" {
            String::new()
        } else {
            av.clone()
        };
        (pmsg, avatar)
    } else {
        (items.get(4).cloned().unwrap_or_default(), String::new())
    };

    Some(LoginArgs {
        version,
        guid,
        name,
        lang,
        pmsg,
        avatar_b64,
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
    fn build_ack_is_length_prefixed() {
        assert_eq!(build_ack("Alice"), "ACK:5:Alice");
    }

    #[test]
    fn build_public_two_fields() {
        // PUBLIC:{nameLen},{textLen}:{name}{text}
        assert_eq!(build_public("Bob", "hi there"), "PUBLIC:3,8:Bobhi there");
    }

    #[test]
    fn build_userinfo_shape() {
        // USERINFO:name,pmsg,av,id,1,1,1:{name}{pmsg}{av}{id}{level}{web}{mobile}
        let s = build_userinfo("Ann", "", "", 7, 100, true, false);
        assert_eq!(s, "USERINFO:3,0,0,1,1,1,1:Ann710010");
    }

    #[test]
    fn build_userlist_item_simple() {
        assert_eq!(build_userlist_item("Cy", 50), "USERLIST:2,1:Cy50");
    }

    #[test]
    fn build_offline_and_end() {
        assert_eq!(build_offline("Zoe"), "OFFLINE:3:Zoe");
        assert_eq!(build_userlist_end(), "USERLIST_END:");
    }

    #[test]
    fn level_is_decimal_byte() {
        // level 100 → "100", concatenado tras el id.
        let s = build_userinfo("X", "", "", 0, 255, false, true);
        assert_eq!(s, "USERINFO:1,0,0,1,1,1,1:X025501");
    }

    #[test]
    fn parse_lens_args_with_unicode() {
        // Strings con chars multi-byte
        let s = build_with_lens("X", &["héllo", "wörld"]);
        let parsed = parse_lens_args(&s).unwrap();
        assert_eq!(parsed, vec!["héllo", "wörld"]);
    }
}
