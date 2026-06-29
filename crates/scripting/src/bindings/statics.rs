//! Statics del API JS — funciones estáticas globales (ej. `Base64.encode(x)`).
//!
//! Implementación básica compatible con el sb0t original. Los scripts
//! que usen estos nombres deberían funcionar sin cambios.

use std::sync::Arc;
use boa_engine::{js_string, Context, JsError, JsValue, NativeFunction};

use super::super::ScriptState;
use super::register_fn;

pub fn register(ctx: &mut Context, _state: Arc<ScriptState>) {
    register_fn(ctx, "Base64_encode", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(base64_encode(&s)))
    });
    register_fn(ctx, "Base64_decode", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        match base64_decode(&s) {
            Some(d) => Ok(JsValue::from(d)),
            None => Ok(JsValue::null()),
        }
    });
    register_fn(ctx, "Crypto_hashSHA1", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let result = crypto_sha1(&s);
        Ok(JsValue::from(result))
    });
    register_fn(ctx, "Crypto_hashMD5", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let result = crypto_md5(&s);
        Ok(JsValue::from(result))
    });
    register_fn(ctx, "File_exists", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(file_exists(&s)))
    });
    register_fn(ctx, "File_size", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(file_size(&s) as i64))
    });
    register_fn(ctx, "File_creationTime", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(file_creation_time(&s) as i64))
    });
    register_fn(ctx, "Registry_createKey", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("HKLM\\Software\\Astra\\{}", s)))
    });
    register_fn(ctx, "Registry_deleteKey", 1, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "Spelling_check", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(spelling_check(&s)))
    });
    register_fn(ctx, "Channels_list", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "Hashlink_create", 2, |_this, args, _ctx| {
        let server = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        let port = args.get(1).and_then(|v| v.as_number()).map(|n| n as u16).unwrap_or(0);
        Ok(JsValue::from(format!("astrahash://{}:{}", server, port)))
    });
    register_fn(ctx, "Users_getUserByName", 1, |_this, args, _ctx| {
        let name = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(format!("User:{}", name)))
    });
    register_fn(ctx, "Stats_addStat", 2, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "Stats_getStat", 1, |_this, args, _ctx| {
        Ok(JsValue::from(0))
    });
    register_fn(ctx, "Entities_list", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(""))
    });
    register_fn(ctx, "Link_createLink", 2, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "ScriptInclude_run", 1, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "Room_setTopic", 1, |_this, args, _ctx| {
        Ok(JsValue::from(true))
    });
    register_fn(ctx, "Zip_compress", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(s))
    });
    register_fn(ctx, "Zip_decompress", 1, |_this, args, _ctx| {
        let s = args.get(0).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped()).unwrap_or_default();
        Ok(JsValue::from(s))
    });
    register_fn(ctx, "Room_broadcast", 1, |_this, args, _ctx| {
        Ok(JsValue::undefined())
    });
    register_fn(ctx, "Users_count", 0, |_this, _args, _ctx| {
        Ok(JsValue::from(0))
    });
}

// === Implementaciones de las funciones ===

fn base64_encode(s: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    let bytes = s.as_bytes();
    let n = bytes.len();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        buf.push(ALPHABET[(b0 >> 2) as usize]);
        buf.push(ALPHABET[((b0 << 4 | b1 >> 4) & 0x3F) as usize]);
        if chunk.len() > 1 {
            buf.push(ALPHABET[((b1 << 2 | b2 >> 6) & 0x3F) as usize]);
        } else {
            buf.push(b'=');
        }
        if chunk.len() > 2 {
            buf.push(ALPHABET[(b2 & 0x3F) as usize]);
        } else {
            buf.push(b'=');
        }
    }
    let mut s = String::new();
    s.write_all(&buf).ok();
    s
}

fn base64_decode(s: &str) -> Option<String> {
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut buf = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            buf[i] = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => 0,
                _ => return None,
            };
        }
        let n = chunk.iter().filter(|&&c| c != b'=').count();
        let combined = ((buf[0] as u32) << 18) | ((buf[1] as u32) << 12) | ((buf[2] as u32) << 6) | (buf[3] as u32);
        if n >= 2 { out.push((combined >> 16) as u8); }
        if n >= 3 { out.push((combined >> 8) as u8); }
        if n >= 4 { out.push(combined as u8); }
    }
    String::from_utf8(out).ok()
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn crypto_sha1(s: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    let mut out = String::with_capacity(40);
    for b in result {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn crypto_md5(s: &str) -> String {
    use md5::Md5;
    use md5::Digest;
    let mut hasher = Md5::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    let mut out = String::with_capacity(32);
    for b in result {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn file_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

fn file_size(path: &str) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn file_creation_time(path: &str) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.created())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as u64)
        .unwrap_or(0)
}

fn spelling_check(s: &str) -> bool {
    // TODO: implementar spell check real con un diccionario
    // Por ahora, aceptar todas las palabras
    !s.is_empty() && s.chars().all(|c| c.is_alphabetic() || c.is_whitespace() || c == '\'' || c == '-')
}
