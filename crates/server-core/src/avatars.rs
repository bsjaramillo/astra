//! Manager de avatares. Equivalente a `core/Avatars.cs`.

#![allow(dead_code)]

use parking_lot::RwLock;
use std::collections::HashMap;

/// Tamaño máximo de un avatar que se puede mandar a un cliente Ares nativo
/// (paridad `TCPProcessor.Avatar` de sb0t: `if (avatar.Length < 4064)`).
///
/// Los clientes web no tienen este límite — reciben el avatar completo en
/// base64 por el protocolo de texto (`full_avatar`). Es solo el canal binario
/// Ares el que no admite imágenes grandes.
pub const MAX_ARES_AVATAR: usize = 4064;

/// Escala y recomprime un avatar para el canal Ares nativo (paridad
/// `AresClient.Scale` de sb0t): si alguna dimensión supera `AVATAR_MAX_PX`
/// (384), lo reescala para caber en 384×384 y lo codifica como JPEG. Retorna
/// el JPEG resultante, o el original si no se puede decodificar/escalar
/// (para no romper la entrega del avatar).
pub fn scale_avatar(bytes: &[u8]) -> Vec<u8> {
    const MAX_PX: u32 = 384;
    const JPEG_QUALITY: u8 = 70;
    let Ok(img) = image::load_from_memory(bytes) else {
        return bytes.to_vec();
    };
    let (w, h) = (img.width(), img.height());
    if w <= MAX_PX && h <= MAX_PX {
        // Ya entra: solo re-comprimir a JPEG si no lo es (paridad sb0t, que
        // siempre pasa por JPEG).
        if !bytes.starts_with(&[0xFF, 0xD8]) {
            let mut out = Vec::new();
            if img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg).is_ok() {
                return out;
            }
        }
        return bytes.to_vec();
    }
    let scale = (MAX_PX as f32 / w.max(h) as f32).max(0.05);
    let mut nw = (w as f32 * scale).round().max(1.0) as u32;
    let mut nh = (h as f32 * scale).round().max(1.0) as u32;
    // Re-encodea encogiéndolo hasta entrar en el tope del canal Ares (si no
    // se puede, devuelve el original; el caller ya tiene un guard de tamaño).
    loop {
        let resized = img.resize(nw, nh, image::imageops::FilterType::Triangle);
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut out);
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, JPEG_QUALITY);
        if resized.write_with_encoder(encoder).is_ok() {
            if out.len() < MAX_ARES_AVATAR || nw <= 48 || nh <= 48 {
                return out;
            }
        }
        nw = (nw as f32 * 0.75).round().max(1.0) as u32;
        nh = (nh as f32 * 0.75).round().max(1.0) as u32;
        if nw < 16 || nh < 16 {
            return bytes.to_vec();
        }
    }
}

/// Manager de avatares.
pub struct AvatarManager {
    /// Avatares cacheados por user ID.
    avatars: RwLock<HashMap<u16, Vec<u8>>>,
}

impl Default for AvatarManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AvatarManager {
    /// Crea un manager vacío.
    pub fn new() -> Self {
        Self {
            avatars: RwLock::new(HashMap::new()),
        }
    }

    /// Setea el avatar de un usuario.
    pub fn set(&self, user_id: u16, avatar: Vec<u8>) {
        self.avatars.write().insert(user_id, avatar);
    }

    /// Obtiene el avatar de un usuario.
    pub fn get(&self, user_id: u16) -> Option<Vec<u8>> {
        self.avatars.read().get(&user_id).cloned()
    }

    /// Elimina el avatar de un usuario.
    pub fn remove(&self, user_id: u16) {
        self.avatars.write().remove(&user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Genera un PNG de `w`×`h` en memoria.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255u8, 0, 0, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn small_avatar_stays_under_max() {
        // 100×100 → sin escalar, pero se re-comprime a JPEG.
        let b = scale_avatar(&png(100, 100));
        assert!(b.len() < MAX_ARES_AVATAR, "len={}", b.len());
        assert!(b.starts_with(&[0xFF, 0xD8]), "debe ser JPEG");
    }

    #[test]
    fn big_avatar_is_scaled_down() {
        // 2000×2000 (como una foto de móvil) → escala a ≤384px JPEG < tope.
        let b = scale_avatar(&png(2000, 2000));
        assert!(b.len() < MAX_ARES_AVATAR, "len={}", b.len());
        assert!(b.starts_with(&[0xFF, 0xD8]));
        // Decodificar para confirmar dimensiones ≤384.
        let img = image::load_from_memory(&b).unwrap();
        assert!(img.width() <= 384 && img.height() <= 384);
    }

    #[test]
    fn garbage_bytes_return_unchanged() {
        let junk = vec![0x00, 0x01, 0x02, 0xFF];
        assert_eq!(scale_avatar(&junk), junk);
    }
}
