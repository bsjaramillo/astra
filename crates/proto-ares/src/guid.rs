//! Identificador de 16 bytes compatible con la convención Ares.
//!
//! En el protocolo Ares original, los GUIDs se calculan aplicando MD5
//! sobre los 16 bytes recibidos, por seguridad. Esta estructura preserva
//! ese comportamiento.

use md5::{Digest, Md5};

/// GUID de 16 bytes usado para identificar usuarios de Ares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Guid([u8; 16]);

impl Guid {
    /// Crea un GUID a partir de 16 bytes en bruto (se les aplica MD5 internamente,
    /// igual que el cliente Ares original).
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let mut hasher = Md5::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let mut out = [0u8; 16];
        out.copy_from_slice(&digest);
        Self(out)
    }

    /// Crea un GUID ya hasheado (para tests o datos pre-procesados).
    pub fn from_raw_hashed(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Devuelve los 16 bytes en bruto.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl std::fmt::Display for Guid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in &self.0 {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}
