//! Representación de la sala (room).

use parking_lot::RwLock;
use std::sync::Arc;

use super::user_pool::UserPool;

/// Representa la sala activa. Equivalente conceptual de `core/Room.cs`.
pub struct Room {
    /// Nombre.
    pub name: RwLock<String>,
    /// Topic.
    pub topic: RwLock<String>,
    /// ¿Está abierta?
    pub open: RwLock<bool>,
    /// Pool de usuarios (referencia).
    pub user_pool: Arc<UserPool>,
}

impl Room {
    /// Crea una nueva sala.
    pub fn new(name: impl Into<String>, topic: impl Into<String>, user_pool: Arc<UserPool>) -> Self {
        Self {
            name: RwLock::new(name.into()),
            topic: RwLock::new(topic.into()),
            open: RwLock::new(true),
            user_pool,
        }
    }

    /// Cambia el topic.
    pub fn set_topic(&self, new_topic: impl Into<String>) {
        *self.topic.write() = new_topic.into();
    }

    /// Cambia el nombre.
    pub fn set_name(&self, new_name: impl Into<String>) {
        *self.name.write() = new_name.into();
    }

    /// Cierra la sala (los usuarios no pueden entrar nuevos).
    pub fn close(&self) {
        *self.open.write() = false;
    }

    /// Abre la sala.
    pub fn open(&self) {
        *self.open.write() = true;
    }

    /// Cantidad de usuarios en la sala.
    pub fn user_count(&self) -> usize {
        self.user_pool.len()
    }
}
