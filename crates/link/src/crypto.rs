//! Crypto del protocolo Link — paridad exacta con `core/Crypto.cs` de sb0t.
//!
//! ## Esquema (sb0t original)
//!
//! 1. El leaf se presenta con **credentials**: `SHA1(reverse(UTF8(name) ++ guid))`
//!    (20 bytes). El hub las compara contra su lista de trusted leaves.
//! 2. El hub genera una key AES-256 (32 bytes) y un IV (16 bytes) aleatorios
//!    y los manda en el `HubAck` **ofuscados** con [`e67`]: 8 rondas con
//!    claves u16 little-endian tomadas de `MD5(guid_del_leaf)` en offsets
//!    0,2,...,14 (en ese orden).
//! 3. El leaf des-ofusca con [`d67`] en orden inverso (offsets 14,12,...,0)
//!    y obtiene `IV = bytes[0..16]`, `Key = bytes[16..48]`.
//! 4. A partir de ahí, **solo los strings** de los mensajes link viajan
//!    encriptados: AES-256-CBC con PKCS7, escritos como `u16 len + ciphertext`
//!    (+ null terminator). Los campos binarios van en claro.
//!
//! El IV es estático por sesión (igual que sb0t — débil criptográficamente,
//! pero la paridad binaria manda).

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest as Md5Digest, Md5};
use sha1::{Digest as Sha1Digest, Sha1};

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// Material de sesión del link (key AES-256 + IV).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkCrypto {
    /// Key AES-256.
    pub key: [u8; 32],
    /// IV de CBC (fijo por sesión, como sb0t).
    pub iv: [u8; 16],
}

impl LinkCrypto {
    /// Genera key + IV aleatorios (lado hub).
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut key);
        rand::thread_rng().fill_bytes(&mut iv);
        Self { key, iv }
    }

    /// Encripta `data` con AES-256-CBC + PKCS7 (equivalente a `Crypto.Encrypt`).
    pub fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        Aes256CbcEnc::new(&self.key.into(), &self.iv.into())
            .encrypt_padded_vec_mut::<Pkcs7>(data)
    }

    /// Desencripta. `None` si el padding/longitud es inválido.
    pub fn decrypt(&self, data: &[u8]) -> Option<Vec<u8>> {
        Aes256CbcDec::new(&self.key.into(), &self.iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(data)
            .ok()
    }

    /// Serializa `IV ++ Key` (48 bytes) ofuscado para el `HubAck`
    /// (equivalente a `HubOutbound.HubAck`): 8 rondas de [`e67`] con
    /// claves u16 LE de `MD5(leaf_guid)` en offsets 0,2,...,14.
    pub fn to_obfuscated(&self, leaf_guid: &[u8; 16]) -> [u8; 48] {
        let digest = md5_bytes(leaf_guid);
        let mut buf = [0u8; 48];
        buf[..16].copy_from_slice(&self.iv);
        buf[16..].copy_from_slice(&self.key);
        let mut data = buf.to_vec();
        for i in (0..16).step_by(2) {
            let k = u16::from_le_bytes([digest[i], digest[i + 1]]);
            data = e67(&data, k);
        }
        buf.copy_from_slice(&data);
        buf
    }

    /// Des-ofusca los 48 bytes del `HubAck` (equivalente a
    /// `LeafProcessor.HubAck`): [`d67`] con offsets 14,12,...,0.
    pub fn from_obfuscated(data: &[u8; 48], my_guid: &[u8; 16]) -> Self {
        let digest = md5_bytes(my_guid);
        let mut buf = data.to_vec();
        let mut i = 16;
        while i >= 2 {
            i -= 2;
            let k = u16::from_le_bytes([digest[i], digest[i + 1]]);
            buf = d67(&buf, k);
        }
        let mut iv = [0u8; 16];
        let mut key = [0u8; 32];
        iv.copy_from_slice(&buf[..16]);
        key.copy_from_slice(&buf[16..48]);
        Self { key, iv }
    }
}

/// Credentials de login del leaf: `SHA1(reverse(UTF8(name) ++ guid))`
/// (equivalente a `LeafOutbound.LeafLogin` / `TrustedLeavesManager.GetTrusted`).
/// Nota: se invierte el buffer **completo** (name+guid concatenados), no
/// cada parte por separado.
pub fn credentials(name: &str, guid: &[u8; 16]) -> [u8; 20] {
    let mut buf: Vec<u8> = Vec::with_capacity(name.len() + 16);
    buf.extend_from_slice(name.as_bytes());
    buf.extend_from_slice(guid);
    buf.reverse();
    let digest = Sha1::digest(&buf);
    let mut out = [0u8; 20];
    out.copy_from_slice(&digest);
    out
}

/// Deriva los 16 bytes de GUID desde el string de configuración
/// (`settings.guid`): si tiene ≥16 bytes usa los primeros 16, si no
/// usa `SHA1(guid)[..16]`. Debe coincidir en leaf y hub para que las
/// credentials y la ofuscación de key funcionen.
pub fn guid_bytes_from_string(guid: &str) -> [u8; 16] {
    let mut arr = [0u8; 16];
    if guid.len() >= 16 {
        arr.copy_from_slice(&guid.as_bytes()[..16]);
    } else {
        let digest = Sha1::digest(guid.as_bytes());
        arr.copy_from_slice(&digest[..16]);
    }
    arr
}

fn md5_bytes(data: &[u8]) -> [u8; 16] {
    let digest = Md5::digest(data);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}

/// Cifrado de stream `e67` de sb0t (`Crypto.e67`): XOR con el byte alto de
/// un estado u16 que evoluciona con cada byte **cifrado** producido.
pub fn e67(data: &[u8], mut b: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &d in data {
        let enc = d ^ (b >> 8) as u8;
        out.push(enc);
        b = (enc as u16)
            .wrapping_add(b)
            .wrapping_mul(23219)
            .wrapping_add(36126);
    }
    out
}

/// Inversa de [`e67`] (`Crypto.d67`): el estado evoluciona con el byte
/// **cifrado** consumido.
pub fn d67(data: &[u8], mut b: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &d in data {
        out.push(d ^ (b >> 8) as u8);
        b = (d as u16)
            .wrapping_add(b)
            .wrapping_mul(23219)
            .wrapping_add(36126);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e67_d67_roundtrip() {
        let data = b"mensaje de prueba con bytes \x00\xff\x80";
        for key in [0u16, 1, 0xABCD, 0xFFFF] {
            let enc = e67(data, key);
            let dec = d67(&enc, key);
            assert_eq!(dec, data, "key={:x}", key);
        }
    }

    #[test]
    fn e67_reference_vector() {
        // Vector calculado a mano siguiendo Crypto.e67 de sb0t:
        // b=0x0100, data=[0x41]:
        //   enc = 0x41 ^ 0x01 = 0x40
        //   (siguiente estado no observable con 1 byte)
        assert_eq!(e67(&[0x41], 0x0100), vec![0x40]);
        // Segundo byte usa el estado evolucionado:
        // b' = ((0x40 + 0x0100) * 23219 + 36126) & 0xFFFF
        let b2 = ((0x40u32 + 0x0100) * 23219 + 36126) & 0xFFFF;
        let expected_second = 0x42u8 ^ (b2 >> 8) as u8;
        assert_eq!(e67(&[0x41, 0x42], 0x0100), vec![0x40, expected_second]);
    }

    #[test]
    fn obfuscation_roundtrip() {
        let crypto = LinkCrypto::generate();
        let guid = [0x5A; 16];
        let obf = crypto.to_obfuscated(&guid);
        assert_ne!(&obf[..16], &crypto.iv, "la key no debe viajar en claro");
        let recovered = LinkCrypto::from_obfuscated(&obf, &guid);
        assert_eq!(recovered, crypto);
    }

    #[test]
    fn obfuscation_wrong_guid_fails() {
        let crypto = LinkCrypto::generate();
        let obf = crypto.to_obfuscated(&[0x5A; 16]);
        let recovered = LinkCrypto::from_obfuscated(&obf, &[0x5B; 16]);
        assert_ne!(recovered, crypto);
    }

    #[test]
    fn aes_roundtrip() {
        let crypto = LinkCrypto::generate();
        let plain = "hola desde el leaf — ñandú 日本語".as_bytes();
        let enc = crypto.encrypt(plain);
        assert_ne!(enc, plain);
        assert_eq!(enc.len() % 16, 0, "CBC produce múltiplos del block size");
        assert_eq!(crypto.decrypt(&enc).unwrap(), plain);
    }

    #[test]
    fn aes_known_vector() {
        // Vector conocido AES-256-CBC PKCS7 (verificable con openssl):
        // echo -n "hello world" | openssl enc -aes-256-cbc \
        //   -K 000102...1f -iv 000102...0f | xxd
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let iv: [u8; 16] = core::array::from_fn(|i| i as u8);
        let c = LinkCrypto { key, iv };
        let enc = c.encrypt(b"hello world");
        assert_eq!(
            enc,
            [
                0xb5, 0x75, 0xa2, 0xc0, 0x3e, 0x57, 0x71, 0x10, 0xf6, 0xc0, 0x10, 0x3c,
                0x28, 0x71, 0x98, 0x96
            ]
        );
    }

    #[test]
    fn aes_decrypt_garbage_returns_none() {
        let crypto = LinkCrypto::generate();
        assert!(crypto.decrypt(b"esto no es un bloque valido").is_none());
    }

    #[test]
    fn credentials_matches_manual_sha1() {
        // Réplica manual del algoritmo sb0t para validar la implementación
        let name = "MiSala";
        let guid = [0xAB; 16];
        let mut buf = Vec::new();
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&guid);
        buf.reverse();
        let expected = Sha1::digest(&buf);
        assert_eq!(credentials(name, &guid)[..], expected[..]);
    }

    #[test]
    fn guid_bytes_derivation() {
        // ≥16 bytes: primeros 16 tal cual
        let long = "0123456789abcdefEXTRA";
        assert_eq!(&guid_bytes_from_string(long), b"0123456789abcdef");
        // <16 bytes: SHA1[..16]
        let short = "corto";
        let expected = Sha1::digest(short.as_bytes());
        assert_eq!(guid_bytes_from_string(short)[..], expected[..16]);
    }
}
