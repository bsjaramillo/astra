//! Reensamblado de transferencias multi-chunk `CUSTOM_DATA_HEAD`/`CUSTOM_DATA_BODY`
//! del protocolo ib0t: imágenes (scribbles) y audio que los clientes web
//! mandan en varios paquetes de hasta ~30000 chars de base64.
//!
//! Paridad con `core/ib0t/CustomData.cs` de sb0t (`Dictionary<id, CustomData>`
//! global): el cliente manda un HEAD (`sender`, `size` = cantidad total de
//! chunks) seguido de `size` BODY con el mismo `id`; cuando se recibe el
//! último chunk, se entrega `(sender, target, data_completa)` para que el
//! caller la difunda como `SCRIBBLE_*`/`AUDIO_*` según el `type` del BODY.

use std::collections::HashMap;

use parking_lot::Mutex;

struct PendingTransfer {
    /// Nombre del emisor (server-side, no el que mande el cliente).
    sender: String,
    /// Destinatario si es una transferencia privada; `None` si es pública.
    target: Option<String>,
    /// Vroom del emisor al momento del HEAD (para difundir al room correcto).
    vroom: u16,
    /// Total de chunks esperados.
    size: u16,
    /// Chunks recibidos hasta ahora.
    count: u16,
    /// Datos acumulados.
    data: String,
}

/// Store de transferencias `CUSTOM_DATA` en curso. Se usa una instancia para
/// transferencias públicas y otra para privadas (mismo tipo, distinto uso).
#[derive(Default)]
pub struct CustomDataStore {
    pending: Mutex<HashMap<String, PendingTransfer>>,
}

impl CustomDataStore {
    /// Crea un store vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inicia una transferencia. No-op si el `id` ya existe (paridad sb0t:
    /// `if (customData.ContainsKey(id)) return;`).
    pub fn start(&self, id: &str, sender: String, target: Option<String>, vroom: u16, size: u16) {
        let mut pending = self.pending.lock();
        pending.entry(id.to_string()).or_insert(PendingTransfer {
            sender,
            target,
            vroom,
            size,
            count: 0,
            data: String::new(),
        });
    }

    /// Agrega un chunk. Si con este se completa la transferencia
    /// (`count == size`), la remueve y devuelve `(sender, target, vroom, data)`.
    /// Si el `id` no existe (HEAD nunca llegó, o ya se completó), no hace nada.
    pub fn append(&self, id: &str, chunk: &str) -> Option<(String, Option<String>, u16, String)> {
        let mut pending = self.pending.lock();
        let done = {
            let entry = pending.get_mut(id)?;
            entry.data.push_str(chunk);
            entry.count += 1;
            entry.count >= entry.size
        };
        if done {
            let entry = pending.remove(id)?;
            Some((entry.sender, entry.target, entry.vroom, entry.data))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_after_expected_chunks() {
        let store = CustomDataStore::new();
        store.start("id1", "Alice".into(), None, 0, 2);
        assert!(store.append("id1", "hello").is_none());
        let (sender, target, vroom, data) = store.append("id1", "world").unwrap();
        assert_eq!(sender, "Alice");
        assert_eq!(target, None);
        assert_eq!(vroom, 0);
        assert_eq!(data, "helloworld");
    }

    #[test]
    fn unknown_id_is_noop() {
        let store = CustomDataStore::new();
        assert!(store.append("nope", "x").is_none());
    }

    #[test]
    fn duplicate_head_is_ignored() {
        let store = CustomDataStore::new();
        store.start("id1", "Alice".into(), None, 0, 1);
        store.start("id1", "Bob".into(), None, 0, 5); // ignorado, ya existe
        let (sender, _, _, data) = store.append("id1", "chunk").unwrap();
        assert_eq!(sender, "Alice");
        assert_eq!(data, "chunk");
    }

    #[test]
    fn pm_transfer_carries_target() {
        let store = CustomDataStore::new();
        store.start("id2", "Alice".into(), Some("Bob".into()), 3, 1);
        let (sender, target, vroom, _) = store.append("id2", "x").unwrap();
        assert_eq!(sender, "Alice");
        assert_eq!(target, Some("Bob".to_string()));
        assert_eq!(vroom, 3);
    }
}
