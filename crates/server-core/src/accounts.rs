//! Manager de cuentas registradas.
//!
//! Las cuentas se persisten en SQLite (tabla `accounts`).
//!
//! ## Hashing de passwords (compatible con sb0t)
//!
//! El sb0t original usa `SHA1(password)` directamente. Mantenemos SHA-1
//! para compatibilidad con cuentas existentes. Para cuentas nuevas se
//! podría migrar a un esquema más seguro, pero la prioridad es la
//! compatibilidad binaria.

use std::sync::Arc;

use sha1::{Digest, Sha1};

use crate::db::{AccountRecord, Database};

/// Manager de cuentas.
pub struct AccountManager {
    db: Arc<Database>,
}

impl AccountManager {
    /// Crea el manager.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Hashea una contraseña con SHA-1 (compatible con sb0t).
    pub fn hash_password(&self, password: &str) -> Vec<u8> {
        let mut hasher = Sha1::new();
        hasher.update(password.as_bytes());
        hasher.finalize().to_vec()
    }

    /// Registra una cuenta nueva (reemplaza si ya existe para el GUID).
    pub fn register(
        &self,
        name: &str,
        guid: &[u8; 16],
        password: &str,
        level: u8,
    ) -> crate::db::DbResult<()> {
        let hash = self.hash_password(password);
        self.db.upsert_account(name, level, guid, &hash)
    }

    /// Cambia el nivel de una cuenta.
    pub fn set_level(&self, guid: &[u8; 16], level: u8) -> crate::db::DbResult<bool> {
        self.db.update_account_level(guid, level)
    }

    /// Elimina una cuenta.
    pub fn unregister(&self, guid: &[u8; 16]) -> crate::db::DbResult<bool> {
        self.db.delete_account(guid)
    }

    /// Busca por nombre.
    pub fn find_by_name(&self, name: &str) -> crate::db::DbResult<Option<AccountRecord>> {
        self.db.find_account_by_name(name)
    }

    /// Busca por GUID.
    pub fn find_by_guid(&self, guid: &[u8; 16]) -> crate::db::DbResult<Option<AccountRecord>> {
        self.db.find_account_by_guid(guid)
    }

    /// Busca por password (modo no-strict del sb0t).
    pub fn find_by_password(
        &self,
        password: &str,
    ) -> crate::db::DbResult<Option<AccountRecord>> {
        let hash = self.hash_password(password);
        self.db.find_account_by_password(&hash)
    }

    /// Verifica credenciales (modo strict: requiere coincidencia de GUID).
    pub fn verify_strict(
        &self,
        name: &str,
        guid: &[u8; 16],
        password: &str,
    ) -> crate::db::DbResult<bool> {
        if let Some(acc) = self.find_by_name(name)? {
            if acc.guid == *guid {
                let hash = self.hash_password(password);
                return Ok(acc.password == hash);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_verify() {
        let db = Database::in_memory().unwrap();
        let am = AccountManager::new(db);
        let guid = [0xAB; 16];
        am.register("alice", &guid, "secret", 50).unwrap();
        assert!(am.verify_strict("alice", &guid, "secret").unwrap());
        assert!(!am.verify_strict("alice", &guid, "wrong").unwrap());
        assert!(!am.verify_strict("alice", &[0; 16], "secret").unwrap());
    }

    #[test]
    fn sha1_matches_sb0t_format() {
        let am = AccountManager::new(Database::in_memory().unwrap());
        let hash = am.hash_password("test");
        // SHA1("test") = a94a8fef8c17c0b6a48a6c1b...
        assert_eq!(hash.len(), 20);
        assert_eq!(hash[0], 0xa9);
        assert_eq!(hash[1], 0x4a);
    }

    #[test]
    fn unregister() {
        let db = Database::in_memory().unwrap();
        let am = AccountManager::new(db);
        let guid = [0xCD; 16];
        am.register("bob", &guid, "pwd", 1).unwrap();
        assert!(am.unregister(&guid).unwrap());
        assert!(am.find_by_guid(&guid).unwrap().is_none());
    }
}
