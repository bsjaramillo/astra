//! Estadísticas globales del servidor.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

/// Estadísticas del servidor (uptime, picos, totales).
pub struct Stats {
    /// Pico de usuarios simultáneos.
    peak_users: AtomicU32,
    /// Total histórico de usuarios.
    total_users: AtomicU64,
    /// Bytes recibidos.
    bytes_in: AtomicU64,
    /// Bytes enviados.
    bytes_out: AtomicU64,
    /// Mensajes públicos recibidos.
    messages: AtomicU64,
    /// Mensajes privados recibidos.
    pms: AtomicU64,
    /// Eventos de flood detectados.
    floods: AtomicU64,
    /// Usuarios que se fueron (parts).
    parts: AtomicU64,
    /// Logins inválidos.
    invalid_logins: AtomicU64,
    /// Conexiones rechazadas.
    rejections: AtomicU64,
    /// Timestamp de arranque.
    start_time: Instant,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    /// Crea una nueva instancia de estadísticas.
    pub fn new() -> Self {
        Self {
            peak_users: AtomicU32::new(0),
            total_users: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            messages: AtomicU64::new(0),
            pms: AtomicU64::new(0),
            floods: AtomicU64::new(0),
            parts: AtomicU64::new(0),
            invalid_logins: AtomicU64::new(0),
            rejections: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Registra un nuevo usuario y actualiza el pico si corresponde.
    pub fn on_user_join(&self, current: u32) {
        self.total_users.fetch_add(1, Ordering::Relaxed);
        let mut peak = self.peak_users.load(Ordering::Relaxed);
        while current > peak {
            match self.peak_users.compare_exchange(
                peak,
                current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    }

    /// Registra un usuario que se fue.
    pub fn on_user_part(&self) {
        // No decrementamos total_users (es histórico)
        self.parts.fetch_add(1, Ordering::Relaxed);
    }

    /// Registra un mensaje público recibido.
    pub fn on_message(&self) {
        self.messages.fetch_add(1, Ordering::Relaxed);
    }

    /// Registra un mensaje privado recibido.
    pub fn on_pm(&self) {
        self.pms.fetch_add(1, Ordering::Relaxed);
    }

    /// Registra un evento de flood.
    pub fn on_flood(&self) {
        self.floods.fetch_add(1, Ordering::Relaxed);
    }

    /// Registra un login inválido.
    pub fn on_invalid_login(&self) {
        self.invalid_logins.fetch_add(1, Ordering::Relaxed);
    }

    /// Registra una conexión rechazada.
    pub fn on_rejection(&self) {
        self.rejections.fetch_add(1, Ordering::Relaxed);
    }

    /// Mensajes públicos recibidos.
    pub fn messages(&self) -> u64 {
        self.messages.load(Ordering::Relaxed)
    }

    /// Mensajes privados recibidos.
    pub fn pms(&self) -> u64 {
        self.pms.load(Ordering::Relaxed)
    }

    /// Eventos de flood.
    pub fn floods(&self) -> u64 {
        self.floods.load(Ordering::Relaxed)
    }

    /// Parts.
    pub fn parts(&self) -> u64 {
        self.parts.load(Ordering::Relaxed)
    }

    /// Logins inválidos.
    pub fn invalid_logins(&self) -> u64 {
        self.invalid_logins.load(Ordering::Relaxed)
    }

    /// Conexiones rechazadas.
    pub fn rejections(&self) -> u64 {
        self.rejections.load(Ordering::Relaxed)
    }

    /// Suma bytes recibidos.
    pub fn add_bytes_in(&self, n: u64) {
        self.bytes_in.fetch_add(n, Ordering::Relaxed);
    }

    /// Suma bytes enviados.
    pub fn add_bytes_out(&self, n: u64) {
        self.bytes_out.fetch_add(n, Ordering::Relaxed);
    }

    /// Pico de usuarios simultáneos.
    pub fn peak_users(&self) -> u32 {
        self.peak_users.load(Ordering::Relaxed)
    }

    /// Total histórico de usuarios.
    pub fn total_users(&self) -> u64 {
        self.total_users.load(Ordering::Relaxed)
    }

    /// Uptime en segundos.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Bytes recibidos.
    pub fn bytes_in(&self) -> u64 {
        self.bytes_in.load(Ordering::Relaxed)
    }

    /// Bytes enviados.
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }
}
