//! Manager de avatares. Equivalente a `core/Avatars.cs`.

#![allow(dead_code)]

use parking_lot::RwLock;
use std::collections::HashMap;

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
