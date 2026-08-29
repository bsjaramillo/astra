//! Cifrado "soft" de los PMs custom de cb0t (`PMCrypto.cs`).
//!
//! cb0t envía los PMs enriquecidos (`cb0t_pm_msg`) cifrados con un esquema
//! propio (NO es la crypto de sesión Ares):
//!
//! ```text
//! payload = [ 8 bytes DES key ][ DES-CBC(PKCS7) ciphertext ]
//! IV      = SHA1(receiver_name)[0..8]
//! ```
//!
//! El emisor deriva el IV del nombre del RECEPTOR; el receptor lo deriva de
//! su propio nombre. Para que el servidor lea un PM dirigido al bot agente,
//! se descifra con el nombre del bot.

use sha1::Digest;

/// IV del DES: primeros 8 bytes de `SHA1(utf8(name))`.
fn sha1_first8(name: &str) -> [u8; 8] {
    let mut h = sha1::Sha1::new();
    h.update(name.as_bytes());
    let d = h.finalize();
    d[..8].try_into().unwrap_or([0u8; 8])
}

/// Cifra `data` como `PMCrypto.SoftEncrypt(receiver, data)`: genera una key
/// DES aleatoria, cifra en CBC/PKCS7 con IV derivado del receptor y antepone
/// la key al ciphertext. (Se usa sobre todo en tests/roundtrip.)
pub fn soft_encrypt(receiver: &str, data: &[u8]) -> Vec<u8> {
    use cbc::cipher::{BlockEncryptMut, KeyIvInit};
    type DesCbcEnc = cbc::Encryptor<des::Des>;

    let key: [u8; 8] = rand::random();
    let iv = sha1_first8(receiver);

    let mut buf = data.to_vec();
    buf.resize(data.len() + 8, 0);
    let out = DesCbcEnc::new(&key.into(), &iv.into())
        .encrypt_padded_mut::<cbc::cipher::block_padding::Pkcs7>(&mut buf, data.len())
        .expect("pkcs7 padding");
    let mut result = Vec::with_capacity(8 + out.len());
    result.extend_from_slice(&key);
    result.extend_from_slice(out);
    result
}

/// Descifra un payload `cb0t_pm_msg` dirigido a `receiver`. Devuelve los
/// bytes en claro. `None` si el payload es inválido o la key/IV no cuadran
/// (padding inválido).
pub fn soft_decrypt_bytes(receiver: &str, data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 8 {
        return None;
    }
    use cbc::cipher::{BlockDecryptMut, KeyIvInit};
    type DesCbcDec = cbc::Decryptor<des::Des>;

    let key: [u8; 8] = data[..8].try_into().ok()?;
    let iv = sha1_first8(receiver);
    let mut buf = data[8..].to_vec();
    let pt = DesCbcDec::new(&key.into(), &iv.into())
        .decrypt_padded_mut::<cbc::cipher::block_padding::Pkcs7>(&mut buf)
        .ok()?;
    Some(pt.to_vec())
}

/// Como [`soft_decrypt_bytes`] pero asumiendo texto UTF-8 (el caso de un PM).
pub fn soft_decrypt(receiver: &str, data: &[u8]) -> Option<String> {
    String::from_utf8(soft_decrypt_bytes(receiver, data)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let msg = b"hola, que tal?";
        let payload = soft_encrypt("Nova", msg);
        let plain = soft_decrypt("Nova", &payload).expect("decrypt");
        assert_eq!(plain.as_bytes(), msg);
    }

    #[test]
    fn decrypt_with_wrong_name_fails() {
        let payload = soft_encrypt("Nova", b"secreto");
        // Otro receptor → IV distinto → padding inválido.
        assert!(soft_decrypt("OtraBot", &payload).is_none());
    }

    #[test]
    fn payload_too_short() {
        assert!(soft_decrypt("Nova", &[1, 2, 3]).is_none());
        assert!(soft_decrypt_bytes("Nova", &[]).is_none());
    }

    #[test]
    fn handles_multibyte_utf8() {
        let msg = "ñandú 🎉 música".as_bytes();
        let payload = soft_encrypt("Nova", msg);
        let plain = soft_decrypt("Nova", &payload).expect("decrypt");
        assert_eq!(plain.as_bytes(), msg);
    }
}