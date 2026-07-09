//! Transformaciones de texto de los efectos de moderación de sb0t
//! (`Kiddied`, `Lowered`, `KewlText`, `Paint`).

use std::sync::atomic::Ordering;

use crate::user_pool::AresUser;

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
    out
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
    fn shouting_detection() {
        assert!(is_shouting("HELLO EVERYONE"));
        assert!(!is_shouting("hello everyone"));
        assert!(!is_shouting("HI")); // muy corto
        assert!(is_shouting("STOP IT NOW")); // todo mayúsculas
        assert!(!is_shouting("STOP it now")); // 4/9 mayúsculas → no
        assert!(!is_shouting("Hello There Friend")); // title case, no grito
    }
}
