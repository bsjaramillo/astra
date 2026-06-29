//! Carga la lista semilla de nodos/rooms desde un JSON.
//!
//! Formato esperado (subset del `rooms.json` de `chatrooms.mywire.org`):
//!
//! ```json
//! {
//!   "Count": 20,
//!   "Items": [
//!     {
//!       "port": 5009,
//!       "users": 5,
//!       "name": "TestRoom",
//!       "topic": "Test topic",
//!       "servidor": "sb0t 5.43.5",
//!       "externalIp": "1.2.3.4",
//!       "lastUpdate": 1782601355644
//!     }
//!   ]
//! }
//! ```

use std::net::IpAddr;
use std::path::Path;

use serde::Deserialize;

use server_core::db::Database;

use crate::manager::UdpNodeManager;

/// Schema del JSON (subset).
#[derive(Debug, Deserialize)]
struct SeedFile {
    /// Cantidad de items (no se usa, solo informativo)
    #[allow(dead_code)]
    #[serde(rename = "Count")]
    count: Option<u32>,
    /// Lista de rooms/nodos
    #[serde(rename = "Items")]
    items: Vec<SeedItem>,
}

#[derive(Debug, Deserialize)]
struct SeedItem {
    /// Puerto TCP
    port: u16,
    /// Nombre de la sala
    name: String,
    /// Topic
    topic: String,
    /// Versión del server (campo "servidor" en el JSON)
    servidor: Option<String>,
    /// IP externa
    #[serde(rename = "externalIp")]
    external_ip: String,
    /// Última actualización
    #[serde(rename = "lastUpdate")]
    last_update: i64,
}

/// Resultado del seed.
#[derive(Debug)]
pub struct SeedStats {
    /// Cuántos nodos se insertaron
    pub nodes_added: usize,
    /// Cuántas rooms se insertaron
    pub rooms_added: usize,
    /// Errores de parseo
    pub errors: Vec<String>,
}

/// Carga un seed desde un archivo JSON. Si la DB ya tiene nodos, no hace nada.
///
/// `path` es la ruta al archivo JSON. Si el archivo no existe, retorna Ok con stats vacíos.
pub fn load_seed(db: &Database, path: &Path) -> anyhow::Result<SeedStats> {
    if !path.exists() {
        tracing::warn!("seed no encontrado en {}", path.display());
        return Ok(SeedStats {
            nodes_added: 0,
            rooms_added: 0,
            errors: vec![format!("archivo no encontrado: {}", path.display())],
        });
    }

    // Si ya hay nodos en la DB, no hacer nada
    if db.count_nodes()? > 0 {
        tracing::info!("seed: ya hay {} nodos en DB, no se carga seed", db.count_nodes()?);
        return Ok(SeedStats {
            nodes_added: 0,
            rooms_added: 0,
            errors: vec![],
        });
    }

    let content = std::fs::read_to_string(path)?;
    let seed: SeedFile = serde_json::from_str(&content)?;

    let mut stats = SeedStats {
        nodes_added: 0,
        rooms_added: 0,
        errors: vec![],
    };

    for item in seed.items {
        let ip: IpAddr = match item.external_ip.parse() {
            Ok(ip) => ip,
            Err(e) => {
                stats.errors.push(format!("IP inválida '{}': {}", item.external_ip, e));
                continue;
            }
        };

        if let Err(e) = db.upsert_node(&ip.to_string(), item.port) {
            stats.errors.push(format!("error guardando nodo: {}", e));
            continue;
        }
        stats.nodes_added += 1;

        if let Err(e) = db.upsert_room(
            &ip.to_string(),
            item.port,
            &item.name,
            &item.topic,
            item.servidor.as_deref().unwrap_or(""),
            0, // users no aplica para el seed
            0, // language
            item.last_update,
        ) {
            stats.errors.push(format!("error guardando room: {}", e));
        } else {
            stats.rooms_added += 1;
        }
    }

    tracing::info!(
        "seed cargado desde {}: {} nodos, {} rooms",
        path.display(),
        stats.nodes_added,
        stats.rooms_added
    );

    Ok(stats)
}

/// Carga el seed usando un `UdpNodeManager` (después de creado).
pub fn load_seed_into_manager(manager: &UdpNodeManager, path: &Path) -> anyhow::Result<SeedStats> {
    // Cargar a través de la DB subyacente
    // (Hack: acceder a la DB via un truco. Mejor: hacer un método load_seed en el manager)
    // Por simplicidad, hacemos la carga directa y luego reconstruimos el manager.
    // ... pero eso es feo. Mejor: agregamos un método load_seed al manager.
    let db = manager.db_arc();
    load_seed(&db, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_seed_file(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("seed.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn load_valid_seed() {
        let dir = std::env::temp_dir().join(format!("astra_seed_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_seed_file(
            &dir,
            r#"{
                "Count": 2,
                "Items": [
                    {"port": 5009, "users": 5, "name": "Room1", "topic": "T1", "servidor": "sb0t", "externalIp": "1.1.1.1", "lastUpdate": 1000},
                    {"port": 5010, "users": 3, "name": "Room2", "topic": "T2", "servidor": "sb0t", "externalIp": "2.2.2.2", "lastUpdate": 2000}
                ]
            }"#,
        );

        let db = Database::in_memory().unwrap();
        let stats = load_seed(&db, &path).unwrap();
        assert_eq!(stats.nodes_added, 2);
        assert_eq!(stats.rooms_added, 2);
        assert_eq!(db.count_nodes().unwrap(), 2);
        assert_eq!(db.count_nodes().unwrap(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_seed_skips_if_already_loaded() {
        let dir = std::env::temp_dir().join(format!("astra_seed_skip_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_seed_file(
            &dir,
            r#"{"Count":1,"Items":[{"port":5009,"users":0,"name":"X","topic":"Y","servidor":"s","externalIp":"1.1.1.1","lastUpdate":0}]}"#,
        );

        let db = Database::in_memory().unwrap();
        // Cargar una vez
        load_seed(&db, &path).unwrap();
        assert_eq!(db.count_nodes().unwrap(), 1);

        // Segunda carga: no debe agregar nada
        let stats = load_seed(&db, &path).unwrap();
        assert_eq!(stats.nodes_added, 0);
        assert_eq!(db.count_nodes().unwrap(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_seed_missing_file() {
        let db = Database::in_memory().unwrap();
        let path = std::path::PathBuf::from("/nonexistent/seed.json");
        let stats = load_seed(&db, &path).unwrap();
        assert_eq!(stats.nodes_added, 0);
        assert!(!stats.errors.is_empty());
    }

    #[test]
    fn load_seed_invalid_ip() {
        let dir = std::env::temp_dir().join(format!("astra_seed_badip_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_seed_file(
            &dir,
            r#"{"Count":2,"Items":[
                {"port":5009,"users":0,"name":"X","topic":"Y","servidor":"s","externalIp":"1.1.1.1","lastUpdate":0},
                {"port":5010,"users":0,"name":"X","topic":"Y","servidor":"s","externalIp":"not-an-ip","lastUpdate":0}
            ]}"#,
        );

        let db = Database::in_memory().unwrap();
        let stats = load_seed(&db, &path).unwrap();
        assert_eq!(stats.nodes_added, 1); // solo el primero
        assert!(!stats.errors.is_empty()); // el segundo tuvo error

        std::fs::remove_dir_all(&dir).ok();
    }
}
