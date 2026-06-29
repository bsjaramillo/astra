//! Utilidades de tiempo: timestamps en milisegundos desde el epoch Unix.

use std::time::{SystemTime, UNIX_EPOCH};

/// Devuelve el tiempo actual en milisegundos desde el epoch Unix.
///
/// Equivalente a `Helpers.UnixTime` del sb0t original.
pub fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Devuelve el tiempo actual en segundos desde el epoch Unix.
pub fn unix_time_secs() -> u64 {
    unix_time() / 1000
}
