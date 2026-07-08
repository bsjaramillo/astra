//! Manager de vrooms (canales virtuales).
//!
//! Cada vroom es un canal independiente dentro de la sala principal.
//! Los users se mueven entre vrooms con `/vroom <id>`. Los mensajes se
//! envían solo a users en el mismo vroom (vía filtrado en `user_pool`).
//!
//! Vroom 0 siempre existe (es la sala principal). Los demás se crean
//! dinámicamente cuando un user hace `/vroom <nuevo_id>` o vía
//! `Channels_create()` desde scripting.

use std::collections::HashMap;

use parking_lot::RwLock;

/// Info de un vroom individual.
#[derive(Debug, Clone)]
pub struct VroomInfo {
    /// ID numérico del vroom
    pub id: u16,
    /// Nombre legible (default: "Room {id}")
    pub name: String,
    /// Topic del vroom (default: vacío)
    pub topic: String,
}

/// Manager de vrooms. Thread-safe.
pub struct VroomManager {
    /// Mapa id → info
    vrooms: RwLock<HashMap<u16, VroomInfo>>,
}

impl VroomManager {
    /// Crea un manager con el vroom 0 (sala principal) pre-creado.
    pub fn new() -> Self {
        let mut vrooms = HashMap::new();
        vrooms.insert(
            0,
            VroomInfo {
                id: 0,
                name: "Main Room".to_string(),
                topic: String::new(),
            },
        );
        Self {
            vrooms: RwLock::new(vrooms),
        }
    }

    /// Crea un vroom nuevo. Retorna `false` si el ID ya existe.
    pub fn create(&self, id: u16, name: Option<String>, topic: Option<String>) -> bool {
        let mut vrooms = self.vrooms.write();
        if vrooms.contains_key(&id) {
            return false;
        }
        vrooms.insert(
            id,
            VroomInfo {
                id,
                name: name.unwrap_or_else(|| format!("Room {}", id)),
                topic: topic.unwrap_or_default(),
            },
        );
        true
    }

    /// Elimina un vroom. Retorna `false` si no existe o si es el 0 (protegido).
    pub fn delete(&self, id: u16) -> bool {
        if id == 0 {
            return false; // vroom 0 no se puede borrar
        }
        self.vrooms.write().remove(&id).is_some()
    }

    /// Lista los IDs de vrooms activos como JSON array (ej: `[0, 1, 2]`).
    pub fn list_ids(&self) -> Vec<u16> {
        let vrooms = self.vrooms.read();
        let mut ids: Vec<u16> = vrooms.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Lista los IDs como JSON array string (para retornar a JS).
    pub fn list_ids_json(&self) -> String {
        let ids = self.list_ids();
        let json: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        format!("[{}]", json.join(","))
    }

    /// Obtiene info de un vroom. `None` si no existe.
    pub fn get(&self, id: u16) -> Option<VroomInfo> {
        self.vrooms.read().get(&id).cloned()
    }

    /// Serializa info de un vroom como JSON string para JS.
    /// Formato: `{"id":0,"name":"Main Room","topic":"..."}` o `null`.
    pub fn get_json(&self, id: u16) -> String {
        match self.get(id) {
            Some(v) => {
                let topic_escaped = v.topic.replace('\\', "\\\\").replace('"', "\\\"");
                let name_escaped = v.name.replace('\\', "\\\\").replace('"', "\\\"");
                format!(
                    "{{\"id\":{},\"name\":\"{}\",\"topic\":\"{}\"}}",
                    v.id, name_escaped, topic_escaped
                )
            }
            None => "null".to_string(),
        }
    }

    /// Cambia el topic de un vroom existente. Retorna `false` si no existe.
    pub fn set_topic(&self, id: u16, topic: String) -> bool {
        let mut vrooms = self.vrooms.write();
        if let Some(v) = vrooms.get_mut(&id) {
            v.topic = topic;
            true
        } else {
            false
        }
    }

    /// Cantidad de vrooms.
    pub fn count(&self) -> usize {
        self.vrooms.read().len()
    }
}

impl Default for VroomManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vroom_0_exists_by_default() {
        let m = VroomManager::new();
        assert!(m.get(0).is_some());
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn create_and_get() {
        let m = VroomManager::new();
        assert!(m.create(1, Some("Sala 1".into()), Some("topic 1".into())));
        let v = m.get(1).unwrap();
        assert_eq!(v.id, 1);
        assert_eq!(v.name, "Sala 1");
        assert_eq!(v.topic, "topic 1");
    }

    #[test]
    fn create_duplicate_fails() {
        let m = VroomManager::new();
        assert!(m.create(1, None, None));
        assert!(!m.create(1, None, None));
    }

    #[test]
    fn delete_vroom_0_fails() {
        let m = VroomManager::new();
        assert!(!m.delete(0));
    }

    #[test]
    fn delete_existing() {
        let m = VroomManager::new();
        m.create(1, None, None);
        assert!(m.delete(1));
        assert!(m.get(1).is_none());
    }

    #[test]
    fn list_ids_includes_0() {
        let m = VroomManager::new();
        let ids = m.list_ids();
        assert!(ids.contains(&0));
    }

    #[test]
    fn list_ids_json_format() {
        let m = VroomManager::new();
        m.create(2, None, None);
        m.create(1, None, None);
        assert_eq!(m.list_ids_json(), "[0,1,2]");
    }

    #[test]
    fn get_json_format() {
        let m = VroomManager::new();
        let json = m.get_json(0);
        assert!(json.contains("\"id\":0"));
        assert!(json.contains("\"name\":\"Main Room\""));
    }

    #[test]
    fn get_json_nonexistent() {
        let m = VroomManager::new();
        assert_eq!(m.get_json(99), "null");
    }

    #[test]
    fn set_topic_updates() {
        let m = VroomManager::new();
        assert!(m.set_topic(0, "nuevo topic".into()));
        assert_eq!(m.get(0).unwrap().topic, "nuevo topic");
    }

    #[test]
    fn set_topic_nonexistent_fails() {
        let m = VroomManager::new();
        assert!(!m.set_topic(99, "x".into()));
    }
}
