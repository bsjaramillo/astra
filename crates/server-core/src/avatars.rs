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
/// (384), lo reescala para caber en 384×384 y lo codifica como JPEG. Además
/// garantiza que el resultado quede **siempre** bajo `MAX_ARES_AVATAR`
/// (encogiendo de a poco si hace falta), porque un avatar más grande desborda
/// el buffer de los clientes nativos. Retorna el original si no se puede
/// decodificar.
pub fn scale_avatar(bytes: &[u8]) -> Vec<u8> {
    const MAX_PX: u32 = 384;
    const JPEG_QUALITY: u8 = 70;
    let Ok(img) = image::load_from_memory(bytes) else {
        return bytes.to_vec();
    };
    let (w, h) = (img.width(), img.height());
    let scale = (MAX_PX as f32 / w.max(h) as f32).min(1.0);
    let mut nw = (w as f32 * scale).round().max(1.0) as u32;
    let mut nh = (h as f32 * scale).round().max(1.0) as u32;
    // Re-encodea encogiéndolo hasta entrar en el tope del canal Ares (si no
    // se puede, devuelve el original; el caller ya tiene un guard de tamaño).
    loop {
        let resized = img.resize(nw, nh, image::imageops::FilterType::Triangle);
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut out);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, JPEG_QUALITY);
        if resized.write_with_encoder(encoder).is_ok()
            && (out.len() < MAX_ARES_AVATAR || nw <= 48 || nh <= 48)
        {
            return out;
        }
        nw = (nw as f32 * 0.75).round().max(1.0) as u32;
        nh = (nh as f32 * 0.75).round().max(1.0) as u32;
        if nw < 16 || nh < 16 {
            return bytes.to_vec();
        }
    }
}

/// Escala el avatar de **sala/bot** y el **default** al formato que usa sb0t
/// (`Avatars.Scale`): 48×48 JPEG calidad 69.
///
/// Es obligatorio pasarlo por aquí antes de mandarlo a clientes Ares nativos:
/// el protocolo Ares manda el avatar como bloque crudo y los clientes tienen
/// un buffer de ~4096 bytes. Un PNG/JPEG grande (p.ej. los assets default de
/// 11 KB) desborda ese buffer y **desincroniza el stream del cliente**: deja
/// de ver la userlist y todos los mensajes, sin desconectarse. sb0t escala
/// siempre (`Avatars.UpdateServerAvatar`/`UpdateDefaultAvatar`).
pub fn scale_room_avatar(bytes: &[u8]) -> Vec<u8> {
    let Ok(img) = image::load_from_memory(bytes) else {
        // No se pudo decodificar: no arriesgamos a mandar algo que rompa el
        // stream de los clientes nativos.
        return Vec::new();
    };
    // sb0t estira la imagen al cuadrado 48×48 (DrawImage a un rect fijo).
    let resized = img.resize_exact(48, 48, image::imageops::FilterType::CatmullRom);
    let mut out = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut out);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 69);
    if resized.write_with_encoder(encoder).is_ok() && out.len() < MAX_ARES_AVATAR {
        out
    } else {
        Vec::new()
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

    /// El avatar de sala/default SIEMPRE debe quedar bajo el tope del canal
    /// Ares: los assets default (256×256 PNG, ~11 KB) lo superan si se mandan
    /// crudos y rompen el stream de los clientes nativos.
    #[test]
    fn room_avatar_scales_default_assets_under_ares_max() {
        for asset in [
            crate::app::DEFAULT_ROOM_AVATAR,
            crate::app::DEFAULT_USER_AVATAR,
        ] {
            let scaled = scale_room_avatar(asset);
            assert!(!scaled.is_empty(), "el asset default debe decodificar");
            assert!(
                scaled.len() < MAX_ARES_AVATAR,
                "avatar de sala {} bytes >= {}",
                scaled.len(),
                MAX_ARES_AVATAR
            );
            assert!(scaled.starts_with(&[0xFF, 0xD8]), "debe ser JPEG");
        }
    }

    #[test]
    fn room_avatar_rejects_garbage() {
        assert!(scale_room_avatar(&[0x00, 0x01, 0x02]).is_empty());
    }
}
