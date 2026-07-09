//! Cifrado del cliente Ares (paridad `core/Crypto.cs` de sb0t).
//!
//! Cuando un cliente pide cifrado (`crypto == 250` en el login), el server
//! genera una key AES-256 (32 bytes) + IV (16 bytes) aleatorios y se los manda
//! en un paquete `MSG_CHAT_SERVER_CRYPTO_KEY`, con `IV ++ Key` **ofuscado**
//! con [`e67`] usando las palabras u16 LE del GUID (MD5) del cliente en offsets
//! 0,2,...,14. A partir de ahí, **solo los strings** de los paquetes viajan
//! cifrados con AES-256-CBC + PKCS7, escritos como `u16 len + ciphertext`
//! (+ null terminator); los campos binarios van en claro.
//!
//! El IV es estático por sesión (igual que sb0t — débil, pero la paridad manda).

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};

type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// Material de sesión del cliente Ares cifrado (key AES-256 + IV).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AresCrypto {
    /// Key AES-256.
    pub key: [u8; 32],
    /// IV de CBC (fijo por sesión, como sb0t).
    pub iv: [u8; 16],
}

impl AresCrypto {
    /// Genera key + IV aleatorios (lado server).
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut key);
        rand::thread_rng().fill_bytes(&mut iv);
        Self { key, iv }
    }

    /// Encripta con AES-256-CBC + PKCS7 (equivalente a `Crypto.Encrypt`).
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

    /// Serializa `IV ++ Key` (48 bytes) ofuscado para `MSG_CHAT_SERVER_CRYPTO_KEY`
    /// (equivalente a `TCPOutbound.CryptoKey`): [`e67`] con palabras u16 LE del
    /// `guid` (ya MD5-hasheado, 16 bytes) en offsets 0,2,...,14, en ese orden.
    pub fn to_obfuscated(&self, guid: &[u8; 16]) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[..16].copy_from_slice(&self.iv);
        buf[16..].copy_from_slice(&self.key);
        let mut data = buf.to_vec();
        for i in (0..16).step_by(2) {
            let k = u16::from_le_bytes([guid[i], guid[i + 1]]);
            data = e67(&data, k);
        }
        buf.copy_from_slice(&data);
        buf
    }
}

/// Cifrado de stream `e67` de sb0t (`Crypto.e67`): XOR con el byte alto de un
/// estado u16 que evoluciona con cada byte **cifrado** producido.
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
    fn aes_roundtrip() {
        let c = AresCrypto::generate();
        for msg in [&b""[..], b"hi", b"un mensaje mas largo con \x00\xff bytes"] {
            let enc = c.encrypt(msg);
            assert_eq!(c.decrypt(&enc).unwrap(), msg);
        }
    }

    #[test]
    fn e67_d67_roundtrip() {
        let data = b"clave AES + IV \x00\xff\x80 de prueba................";
        for key in [0u16, 1, 0xABCD, 0xFFFF] {
            assert_eq!(d67(&e67(data, key), key), data);
        }
    }

    #[test]
    fn obfuscation_reversible_with_guid() {
        // El cliente des-ofusca con d67 en orden inverso (14,12,...,0).
        let c = AresCrypto::generate();
        let guid = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let obf = c.to_obfuscated(&guid);
        let mut buf = obf.to_vec();
        let mut i = 16;
        while i >= 2 {
            i -= 2;
            let k = u16::from_le_bytes([guid[i], guid[i + 1]]);
            buf = d67(&buf, k);
        }
        assert_eq!(&buf[..16], &c.iv);
        assert_eq!(&buf[16..48], &c.key);
    }
}
