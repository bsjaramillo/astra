//! Transformaciones de texto de los efectos de moderación de sb0t
//! (`Kiddied`, `Lowered`, `KewlText`, `Paint`).

use std::sync::atomic::Ordering;

use crate::user_pool::AresUser;

/// Convierte los códigos de color/formato abreviados de sb0t al formato que
/// entienden los clientes (paridad `Helpers.SetColors` de sb0t).
///
/// En el texto del MOTD/greet se escribe `\x02` + dígito como prefijo de
/// formato (`\x02304` = itálicas, color 04); el cliente Ares interpreta
/// `\x02`+dígito como el inicio de un código de color y sin esta conversión
/// los colores no se renderizan. Aquí `\x02`+dígito se convierte al carácter
/// de control del formato:
///
/// - `\x02` + `5` → `\x05` (negrita)
/// - `\x02` + `3` → `\x03` (itálicas)
/// - `\x02` + `6` → `\x06`
/// - `\x02` + `7` → `\x07` (subrayado)
/// - `\x02` + `9` → `\x09` (color)
///
/// Los dos dígitos que siguen (el color) se conservan: `\x02304` →
/// `\x03` + `04`.
pub fn set_colors(text: &str) -> String {
    text.replace("\u{2}5", "\u{5}")
        .replace("\u{2}3", "\u{3}")
        .replace("\u{2}6", "\u{6}")
        .replace("\u{2}7", "\u{7}")
        .replace("\u{2}9", "\u{9}")
}

/// Elimina los códigos de color/formato de Ares/cb0t de un texto (paridad
/// `Helpers.StripColors` de cb0t): `\x03`/`\x05` + 2 dígitos, `\x02`, `\x06`,
/// `\x07`, `\x09` y el soft-hyphen unicode `\xAD` (U+00AD). También quita
/// caracteres de formato / zero-width unicode (ZWSP, ZWJ, BOM…) que rompen
/// la comparación de strings — p. ej. para resolver nicks con color.
pub fn strip_colors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c as u32 {
            // Código de color con valor de 2 dígitos: `\x03NN` / `\x05NN`.
            0x03 | 0x05 => {
                let mut n = 0;
                while n < 2 {
                    match chars.peek().copied() {
                        Some(d) if d.is_ascii_digit() => {
                            chars.next();
                            n += 1;
                        }
                        _ => break,
                    }
                }
            }
            // Códigos de formato simples.
            0x02 | 0x06 | 0x07 | 0x09 | 0xAD => {}
            // Caracteres zero-width / formato unicode.
            0x200B | 0x200C | 0x200D | 0x2060 | 0xFEFF => {}
            _ => out.push(c),
        }
    }
    out
}

/// Aplica las transformaciones "de castigo" per-usuario a un texto público
/// saliente, según los flags del usuario (`kiddied`, `lowered`).
///
/// Se aplica en el path de `handle_public` antes de difundir.
pub fn apply_punish_effects(user: &AresUser, text: &str) -> String {
    let mut out = text.to_string();
    if user.lowered.load(Ordering::Relaxed) {
        out = out.to_lowercase();
    }
    if user.kiddied.load(Ordering::Relaxed) {
        out = kiddy_transform(&out);
    }
    if user.kewl.load(Ordering::Relaxed) {
        out = kewl_transform(&out);
    }
    if let Some(paint) = user.paint_text.read().clone() {
        if !paint.trim().is_empty() {
            // Paridad `commands/Paint.cs` de sb0t (`ServerEvents.cs`:
            // `text = Paint.IsPainted(client) + text`): el paint es un TEXTO
            // que se prepone al mensaje, no un wrap con unicode.
            out = format!("{}{}", paint, out);
        }
    }
    out
}

/// "Kewl text": sustitución leetspeak (a→4, e→3, i→1, o→0, s→5, t→7).
pub fn kewl_transform(text: &str) -> String {
    text.chars()
        .map(|c| match c.to_ascii_lowercase() {
            'a' => '4',
            'e' => '3',
            'i' => '1',
            'o' => '0',
            's' => '5',
            't' => '7',
            _ => c,
        })
        .collect()
}

/// ¿El texto está "gritado" (mayoría de letras en mayúscula y con
/// suficiente longitud)? Usado por el caps-monitoring de sala.
pub fn is_shouting(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 5 {
        return false;
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    // Más del 70% de las letras en mayúscula.
    upper * 10 >= letters.len() * 7
}

/// "Kiddie speak": alterna mayúsculas/minúsculas de forma tosca y estira
/// vocales, imitando el efecto `Kiddied` de sb0t (burla del texto).
pub fn kiddy_transform(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    let mut upper = false;
    for c in text.chars() {
        if c.is_alphabetic() {
            if upper {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            upper = !upper;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn user() -> AresUser {
        AresUser::new(1, IpAddr::V4(Ipv4Addr::LOCALHOST), [0u8; 16])
    }

    #[test]
    fn no_effects_is_identity() {
        let u = user();
        assert_eq!(apply_punish_effects(&u, "Hello World"), "Hello World");
    }

    #[test]
    fn lowered_lowercases() {
        let u = user();
        u.lowered.store(true, Ordering::Relaxed);
        assert_eq!(apply_punish_effects(&u, "SHOUTING Now"), "shouting now");
    }

    #[test]
    fn kiddied_alternates_case() {
        let u = user();
        u.kiddied.store(true, Ordering::Relaxed);
        // "abcd" → a B c D
        assert_eq!(apply_punish_effects(&u, "abcd"), "aBcD");
    }

    #[test]
    fn kiddy_transform_keeps_non_alpha() {
        assert_eq!(kiddy_transform("a b1c"), "a B1c");
    }

    #[test]
    fn kewl_and_paint_transforms() {
        assert_eq!(kewl_transform("elite"), "3l173");
        assert_eq!(kewl_transform("SASO"), "5450");
        // Paint: el texto configurado se PREPONE (paridad sb0t Paint.cs).
        let u = user();
        *u.paint_text.write() = Some("🎨 ".to_string());
        assert_eq!(apply_punish_effects(&u, "hola"), "🎨 hola");
        *u.paint_text.write() = None;
        assert_eq!(apply_punish_effects(&u, "hola"), "hola");
    }

    #[test]
    fn combined_effects_via_user() {
        let u = user();
        u.kewl.store(true, Ordering::Relaxed);
        assert_eq!(apply_punish_effects(&u, "test"), "7357");
    }

    #[test]
    fn shouting_detection() {
        assert!(is_shouting("HELLO EVERYONE"));
        assert!(!is_shouting("hello everyone"));
        assert!(!is_shouting("HI")); // muy corto
        assert!(is_shouting("STOP IT NOW")); // todo mayúsculas
        assert!(!is_shouting("STOP it now")); // 4/9 mayúsculas → no
        assert!(!is_shouting("Hello There Friend")); // title case, no grito
    }

    #[test]
    fn set_colors_converts_sb0t_shorthands() {
        // `\x02` + dígito → carácter de control del formato (Helpers.SetColors).
        assert_eq!(set_colors("\x025Hola"), "\x05Hola"); // negrita
        assert_eq!(set_colors("\x023Hola"), "\x03Hola"); // itálicas
        assert_eq!(set_colors("\x026Hola"), "\x06Hola");
        assert_eq!(set_colors("\x027Hola"), "\x07Hola"); // subrayado
        assert_eq!(set_colors("\x029Hola"), "\x09Hola");
    }

    #[test]
    fn set_colors_keeps_color_digits() {
        // Los dos dígitos del color se conservan: `\x02304` → `\x03` + "04".
        assert_eq!(set_colors("\x02304hola"), "\x03\x30\x34hola");
        assert_eq!(set_colors("\x02512hola"), "\x05\x31\x32hola");
        // Texto sin códigos no cambia.
        assert_eq!(set_colors("hola"), "hola");
    }
}
