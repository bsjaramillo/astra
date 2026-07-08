//! Capa de persistencia con SQLite.
//!
//! Implementa los schemas del sb0t original (`bans`, `accounts`, `user_history`)
//! y provee acceso thread-safe a la base de datos.
//!
//! ## Schemas (idénticos al sb0t original)
//!
//! ```sql
//! CREATE TABLE bans (
//!     name TEXT NOT NULL,
//!     version TEXT NOT NULL,
//!     guid TEXT NOT NULL,
//!     externalip TEXT NOT NULL,
//!     localip TEXT NOT NULL,
//!     port INT NOT NULL,
//!     ident INT NOT NULL
//! );
//!
//! CREATE TABLE accounts (
//!     name TEXT NOT NULL,
//!     level INT NOT NULL,
//!     guid TEXT NOT NULL,
//!     password BLOB NOT NULL
//! );
//!
//! CREATE TABLE user_history (
//!     name TEXT NOT NULL,
//!     version TEXT NOT NULL,
//!     guid TEXT NOT NULL,
//!     externalip TEXT NOT NULL,
//!     localip TEXT NOT NULL,
//!     port INT NOT NULL,
//!     join_time INT NOT NULL,
//!     last_seen INT NOT NULL
//! );
//! ```

#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::time::unix_time;

/// Error de base de datos.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Error de SQLite
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// I/O
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type DbResult<T> = Result<T, DbError>;

/// Wrapper thread-safe sobre la conexión SQLite.
///
/// Usa `parking_lot::Mutex` para acceso síncrono. Las operaciones son
/// lo suficientemente rápidas para no necesitar `spawn_blocking`. Si en
/// el futuro hay queries lentas, se puede migrar a un pool.
pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    /// Abre o crea la base de datos en el path dado.
    pub fn open<P: AsRef<Path>>(path: P) -> DbResult<Arc<Self>> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;
        Self::run_migrations(&conn)?;

        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            path,
        }))
    }

    /// Crea una base de datos en memoria (útil para tests).
    pub fn in_memory() -> DbResult<Arc<Self>> {
        let conn = Connection::open_in_memory()?;
        Self::run_migrations(&conn)?;
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        }))
    }

    /// Path al archivo de la base de datos.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ejecuta un statement SQL (INSERT/UPDATE/DELETE/CREATE).
    /// Usado por BanSystem, AccountManager, etc.
    pub fn execute<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> DbResult<usize> {
        let conn = self.conn.lock();
        conn.execute(sql, params).map_err(Into::into)
    }

    /// Ejecuta un SELECT (read-only) y devuelve filas como Vec<Vec<Value>>.
    /// Usado por el sistema de scripting para exponer queries de solo-lectura.
    pub fn execute_select(
        &self,
        sql: &str,
    ) -> Result<(Vec<String>, Vec<Vec<rusqlite::types::Value>>), String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql).map_err(|e| format!("prepare: {}", e))?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();
        let rows = stmt
            .query_map([], |row| {
                let mut values = Vec::new();
                for i in 0..col_count {
                    let v: rusqlite::types::Value = row.get(i)?;
                    values.push(v);
                }
                Ok(values)
            })
            .map_err(|e| format!("query_map: {}", e))?;
        let mut result_rows = Vec::new();
        for row in rows {
            result_rows.push(row.map_err(|e| format!("row: {}", e))?);
        }
        Ok((col_names, result_rows))
    }

    fn run_migrations(conn: &Connection) -> DbResult<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS bans (
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                guid TEXT NOT NULL,
                externalip TEXT NOT NULL,
                localip TEXT NOT NULL,
                port INTEGER NOT NULL,
                ident INTEGER NOT NULL,
                expires_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS accounts (
                name TEXT NOT NULL,
                level INTEGER NOT NULL,
                guid TEXT NOT NULL,
                password BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS user_history (
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                guid TEXT NOT NULL,
                externalip TEXT NOT NULL,
                localip TEXT NOT NULL,
                port INTEGER NOT NULL,
                join_time INTEGER NOT NULL,
                last_seen INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS nodes (
                ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                ack INTEGER NOT NULL DEFAULT 0,
                try_count INTEGER NOT NULL DEFAULT 0,
                last_connect INTEGER NOT NULL DEFAULT 0,
                last_sent_ips INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (ip, port)
            );

            CREATE TABLE IF NOT EXISTS rooms (
                ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                name TEXT NOT NULL,
                topic TEXT NOT NULL,
                version TEXT NOT NULL,
                users INTEGER NOT NULL,
                language INTEGER NOT NULL DEFAULT 0,
                last_update INTEGER NOT NULL,
                PRIMARY KEY (ip, port)
            );

            CREATE INDEX IF NOT EXISTS idx_bans_guid ON bans(guid);
            CREATE INDEX IF NOT EXISTS idx_bans_ip ON bans(externalip);
            CREATE INDEX IF NOT EXISTS idx_accounts_guid ON accounts(guid);
            CREATE INDEX IF NOT EXISTS idx_accounts_pwd ON accounts(password);
            CREATE INDEX IF NOT EXISTS idx_history_ip ON user_history(externalip);
            CREATE INDEX IF NOT EXISTS idx_nodes_ack ON nodes(ack);
            CREATE INDEX IF NOT EXISTS idx_nodes_last ON nodes(last_connect);
            CREATE INDEX IF NOT EXISTS idx_rooms_users ON rooms(users);
            "#,
        )?;
        Ok(())
    }

    // ========================================================================
    // Bans
    // ========================================================================

    /// Registra un ban.
    pub fn add_ban(
        &self,
        name: &str,
        version: &str,
        guid: &[u8; 16],
        external_ip: IpAddr,
        local_ip: IpAddr,
        port: u16,
        ident: u16,
    ) -> DbResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO bans (name, version, guid, externalip, localip, port, ident) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                name,
                version,
                guid_to_hex(guid),
                external_ip.to_string(),
                local_ip.to_string(),
                port as i64,
                ident as i64
            ],
        )?;
        Ok(())
    }

    /// Elimina un ban por `ident`.
    pub fn remove_ban(&self, ident: u16) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM bans WHERE ident = ?1",
            params![ident as i64],
        )?;
        Ok(n > 0)
    }

    /// Elimina un ban por GUID.
    pub fn remove_ban_by_guid(&self, guid: &[u8; 16]) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM bans WHERE guid = ?1",
            params![guid_to_hex(guid)],
        )?;
        Ok(n > 0)
    }

    /// Elimina un ban por IP externa.
    pub fn remove_ban_by_ip(&self, ip: IpAddr) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM bans WHERE externalip = ?1",
            params![ip.to_string()],
        )?;
        Ok(n > 0)
    }

    /// Verifica si un GUID o IP está baneada.
    pub fn is_banned(&self, guid: &[u8; 16], external_ip: IpAddr) -> bool {
        let conn = self.conn.lock();
        let result: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM bans WHERE guid = ?1 OR externalip = ?2 LIMIT 1",
                params![guid_to_hex(guid), external_ip.to_string()],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);
        result.is_some()
    }

    /// Lista todos los bans.
    pub fn list_bans(&self) -> DbResult<Vec<BanRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT name, version, guid, externalip, localip, port, ident \
             FROM bans ORDER BY ident",
        )?;
        let iter = stmt.query_map([], |row| {
            let guid_hex: String = row.get(2)?;
            let ext: String = row.get(3)?;
            let loc: String = row.get(4)?;
            Ok(BanRecord {
                name: row.get(0)?,
                version: row.get(1)?,
                guid: guid_from_hex(&guid_hex).unwrap_or([0; 16]),
                external_ip: ext.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
                local_ip: loc.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
                port: row.get::<_, i64>(5)? as u16,
                ident: row.get::<_, i64>(6)? as u16,
            })
        })?;
        let mut out = Vec::new();
        for b in iter {
            out.push(b?);
        }
        Ok(out)
    }

    // ========================================================================
    // Accounts
    // ========================================================================

    /// Crea o reemplaza la cuenta de un GUID.
    pub fn upsert_account(
        &self,
        name: &str,
        level: u8,
        guid: &[u8; 16],
        password_hash: &[u8],
    ) -> DbResult<()> {
        let conn = self.conn.lock();
        // El sb0t original elimina por guid y luego inserta (no hay PK)
        conn.execute(
            "DELETE FROM accounts WHERE guid = ?1",
            params![guid_to_hex(guid)],
        )?;
        conn.execute(
            "INSERT INTO accounts (name, level, guid, password) VALUES (?1, ?2, ?3, ?4)",
            params![
                name,
                level as i64,
                guid_to_hex(guid),
                password_hash
            ],
        )?;
        Ok(())
    }

    /// Actualiza el nivel de una cuenta.
    pub fn update_account_level(&self, guid: &[u8; 16], level: u8) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE accounts SET level = ?1 WHERE guid = ?2",
            params![level as i64, guid_to_hex(guid)],
        )?;
        Ok(n > 0)
    }

    /// Elimina una cuenta por GUID.
    pub fn delete_account(&self, guid: &[u8; 16]) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM accounts WHERE guid = ?1",
            params![guid_to_hex(guid)],
        )?;
        Ok(n > 0)
    }

    /// Busca una cuenta por nombre.
    pub fn find_account_by_name(&self, name: &str) -> DbResult<Option<AccountRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT name, level, guid, password FROM accounts WHERE name = ?1 LIMIT 1",
        )?;
        let mut iter = stmt.query_map(params![name], |row| {
            let guid_hex: String = row.get(2)?;
            Ok(AccountRecord {
                name: row.get(0)?,
                level: row.get::<_, i64>(1)? as u8,
                guid: guid_from_hex(&guid_hex).unwrap_or([0; 16]),
                password: row.get(3)?,
            })
        })?;
        match iter.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Busca una cuenta por GUID.
    pub fn find_account_by_guid(&self, guid: &[u8; 16]) -> DbResult<Option<AccountRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT name, level, guid, password FROM accounts WHERE guid = ?1 LIMIT 1",
        )?;
        let mut iter = stmt.query_map(params![guid_to_hex(guid)], |row| {
            let guid_hex: String = row.get(2)?;
            Ok(AccountRecord {
                name: row.get(0)?,
                level: row.get::<_, i64>(1)? as u8,
                guid: guid_from_hex(&guid_hex).unwrap_or([0; 16]),
                password: row.get(3)?,
            })
        })?;
        match iter.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Busca una cuenta por password (para SecureLogin con strict=false).
    pub fn find_account_by_password(&self, password_hash: &[u8]) -> DbResult<Option<AccountRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT name, level, guid, password FROM accounts WHERE password = ?1 LIMIT 1",
        )?;
        let mut iter = stmt.query_map(params![password_hash], |row| {
            let guid_hex: String = row.get(2)?;
            Ok(AccountRecord {
                name: row.get(0)?,
                level: row.get::<_, i64>(1)? as u8,
                guid: guid_from_hex(&guid_hex).unwrap_or([0; 16]),
                password: row.get(3)?,
            })
        })?;
        match iter.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    // ========================================================================
    // User History
    // ========================================================================

    /// Registra un join en el historial.
    pub fn add_user_history(
        &self,
        name: &str,
        version: &str,
        guid: &[u8; 16],
        external_ip: IpAddr,
        local_ip: IpAddr,
        port: u16,
        join_time_ms: u64,
    ) -> DbResult<()> {
        let conn = self.conn.lock();
        let now = unix_time();
        conn.execute(
            "INSERT INTO user_history \
             (name, version, guid, externalip, localip, port, join_time, last_seen) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                name,
                version,
                guid_to_hex(guid),
                external_ip.to_string(),
                local_ip.to_string(),
                port as i64,
                join_time_ms as i64,
                now as i64
            ],
        )?;
        Ok(())
    }

    /// Cuenta cuántos joins desde una IP hubo en los últimos 15 segundos.
    pub fn count_joins_recent(&self, external_ip: IpAddr, window_ms: u64) -> DbResult<u32> {
        let conn = self.conn.lock();
        let threshold = unix_time().saturating_sub(window_ms) as i64;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM user_history \
             WHERE externalip = ?1 AND last_seen >= ?2",
            params![external_ip.to_string(), threshold],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    /// Limpia entradas antiguas (>30 días).
    pub fn prune_old_history(&self, max_age_secs: u64) -> DbResult<usize> {
        let conn = self.conn.lock();
        let threshold = unix_time().saturating_sub(max_age_secs) as i64;
        let n = conn.execute(
            "DELETE FROM user_history WHERE last_seen < ?1",
            params![threshold],
        )?;
        Ok(n)
    }

    /// Cuenta cuántos joins totales (sin filtro de tiempo) hay desde una IP.
    /// Usado para detectar IPs "nuevas" (sin historial) para el captcha gate.
    pub fn count_user_history_by_ip(&self, external_ip: IpAddr) -> DbResult<u32> {
        let conn = self.conn.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM user_history WHERE externalip = ?1",
            params![external_ip.to_string()],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    /// Elimina bans expirados. Retorna la cantidad de bans eliminados.
    /// Un ban está expirado si `expires_at > 0` y `expires_at < now`.
    pub fn prune_expired_bans(&self, now_secs: i64) -> DbResult<usize> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM bans WHERE expires_at > 0 AND expires_at < ?1",
            params![now_secs],
        )?;
        Ok(n)
    }

    /// Actualiza la fecha de expiración de un ban (en segundos unix).
    /// `expires_at = 0` significa "nunca expira".
    pub fn set_ban_expiry(&self, ident: u16, expires_at: i64) -> DbResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE bans SET expires_at = ?1 WHERE ident = ?2",
            params![expires_at, ident as i64],
        )?;
        Ok(())
    }
}

/// Registro de un ban.
#[derive(Debug, Clone)]
pub struct BanRecord {
    /// Nick al momento del ban
    pub name: String,
    /// Versión del cliente
    pub version: String,
    /// GUID
    pub guid: [u8; 16],
    /// IP externa
    pub external_ip: IpAddr,
    /// IP local reportada
    pub local_ip: IpAddr,
    /// Puerto
    pub port: u16,
    /// Identificador único
    pub ident: u16,
}

/// Registro de una cuenta.
#[derive(Debug, Clone)]
pub struct AccountRecord {
    /// Nick
    pub name: String,
    /// Nivel (ILevel como byte)
    pub level: u8,
    /// GUID
    pub guid: [u8; 16],
    /// Hash de la contraseña
    pub password: Vec<u8>,
}

// ============================================================================
// Nodes (UDP room discovery)
// ============================================================================

/// Registro de un nodo UDP conocido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRecord {
    /// IP del nodo
    pub ip: String,
    /// Puerto
    pub port: u16,
    /// Contador de acknowledgments (mayor = más confiable)
    pub ack: i64,
    /// Cantidad de intentos fallidos
    pub try_count: i64,
    /// Último connect exitoso (ms epoch)
    pub last_connect: i64,
    /// Último envío de ADDIPS (ms epoch)
    pub last_sent_ips: i64,
}

/// Registro de una room descubierta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomRecord {
    /// IP del server
    pub ip: String,
    /// Puerto
    pub port: u16,
    /// Nombre de la sala
    pub name: String,
    /// Topic
    pub topic: String,
    /// Versión del server
    pub version: String,
    /// Usuarios conectados
    pub users: u16,
    /// Idioma
    pub language: u8,
    /// Última actualización (ms epoch)
    pub last_update: i64,
}

impl Database {
    // ========================================================================
    // Nodes
    // ========================================================================

    /// Inserta o reemplaza un nodo.
    pub fn upsert_node(&self, ip: &str, port: u16) -> DbResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO nodes (ip, port, ack, try_count, last_connect, last_sent_ips) \
             VALUES (?1, ?2, 1, 0, 0, 0) \
             ON CONFLICT(ip, port) DO NOTHING",
            params![ip, port as i64],
        )?;
        Ok(())
    }

    /// Actualiza el ack de un nodo (incrementa).
    pub fn bump_node_ack(&self, ip: &str, port: u16, now_ms: i64) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE nodes SET ack = ack + 1, last_connect = ?1, try_count = 0 \
             WHERE ip = ?2 AND port = ?3",
            params![now_ms, ip, port as i64],
        )?;
        Ok(n > 0)
    }

    /// Marca un nodo como "intento fallido" (incrementa try_count).
    pub fn record_node_failure(&self, ip: &str, port: u16) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE nodes SET try_count = try_count + 1 WHERE ip = ?1 AND port = ?2",
            params![ip, port as i64],
        )?;
        Ok(n > 0)
    }

    /// Actualiza el puerto de un nodo.
    pub fn update_node_port(&self, ip: &str, port: u16) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE nodes SET port = ?1 WHERE ip = ?2",
            params![port as i64, ip],
        )?;
        Ok(n > 0)
    }

    /// Actualiza last_sent_ips.
    pub fn update_node_last_sent_ips(&self, ip: &str, port: u16, now_ms: i64) -> DbResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE nodes SET last_sent_ips = ?1 WHERE ip = ?2 AND port = ?3",
            params![now_ms, ip, port as i64],
        )?;
        Ok(())
    }

    /// Devuelve todos los nodos.
    pub fn list_nodes(&self) -> DbResult<Vec<NodeRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT ip, port, ack, try_count, last_connect, last_sent_ips FROM nodes",
        )?;
        let iter = stmt.query_map([], |row| {
            Ok(NodeRecord {
                ip: row.get(0)?,
                port: row.get::<_, i64>(1)? as u16,
                ack: row.get(2)?,
                try_count: row.get(3)?,
                last_connect: row.get(4)?,
                last_sent_ips: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for n in iter {
            out.push(n?);
        }
        Ok(out)
    }

    /// Cantidad de nodos.
    pub fn count_nodes(&self) -> DbResult<usize> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Elimina un nodo.
    pub fn delete_node(&self, ip: &str, port: u16) -> DbResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM nodes WHERE ip = ?1 AND port = ?2",
            params![ip, port as i64],
        )?;
        Ok(n > 0)
    }

    /// Expira nodos: borra los que tienen try > 4 y no se conectaron en la última hora.
    pub fn expire_nodes(&self, min_try: i64, max_age_ms: i64, now_ms: i64) -> DbResult<usize> {
        let conn = self.conn.lock();
        let threshold = now_ms - max_age_ms;
        let n = conn.execute(
            "DELETE FROM nodes WHERE try_count >= ?1 AND last_connect > 0 AND last_connect < ?2",
            params![min_try, threshold],
        )?;
        Ok(n)
    }

    // ========================================================================
    // Rooms
    // ========================================================================

    /// Inserta o reemplaza una room.
    pub fn upsert_room(
        &self,
        ip: &str,
        port: u16,
        name: &str,
        topic: &str,
        version: &str,
        users: u16,
        language: u8,
        last_update_ms: i64,
    ) -> DbResult<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO rooms (ip, port, name, topic, version, users, language, last_update) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(ip, port) DO UPDATE SET \
             name = excluded.name, topic = excluded.topic, version = excluded.version, \
             users = excluded.users, language = excluded.language, last_update = excluded.last_update",
            params![
                ip,
                port as i64,
                name,
                topic,
                version,
                users as i64,
                language as i64,
                last_update_ms
            ],
        )?;
        Ok(())
    }

    /// Lista todas las rooms (ordenadas por users descendente).
    pub fn list_rooms(&self) -> DbResult<Vec<RoomRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT ip, port, name, topic, version, users, language, last_update \
             FROM rooms ORDER BY users DESC",
        )?;
        let iter = stmt.query_map([], |row| {
            Ok(RoomRecord {
                ip: row.get(0)?,
                port: row.get::<_, i64>(1)? as u16,
                name: row.get(2)?,
                topic: row.get(3)?,
                version: row.get(4)?,
                users: row.get::<_, i64>(5)? as u16,
                language: row.get::<_, i64>(6)? as u8,
                last_update: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in iter {
            out.push(r?);
        }
        Ok(out)
    }

    /// Busca una room por IP y puerto.
    pub fn find_room(&self, ip: &str, port: u16) -> DbResult<Option<RoomRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT ip, port, name, topic, version, users, language, last_update \
             FROM rooms WHERE ip = ?1 AND port = ?2",
        )?;
        let mut iter = stmt.query_map(params![ip, port as i64], |row| {
            Ok(RoomRecord {
                ip: row.get(0)?,
                port: row.get::<_, i64>(1)? as u16,
                name: row.get(2)?,
                topic: row.get(3)?,
                version: row.get(4)?,
                users: row.get::<_, i64>(5)? as u16,
                language: row.get::<_, i64>(6)? as u8,
                last_update: row.get(7)?,
            })
        })?;
        match iter.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }
}

/// Convierte un GUID de 16 bytes a hex string de 32 chars.
pub(crate) fn guid_to_hex(guid: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in guid {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Parsea un GUID hex de 32 chars a 16 bytes.
pub(crate) fn guid_from_hex(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    let bytes = s.as_bytes();
    for i in 0..16 {
        let hi = hex_nibble(bytes[i * 2])?;
        let lo = hex_nibble(bytes[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn open_in_memory() {
        let db = Database::in_memory().unwrap();
        assert_eq!(db.path().to_str().unwrap(), ":memory:");
    }

    #[test]
    fn ban_crud() {
        let db = Database::in_memory().unwrap();
        let guid = [0xAB; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));

        assert!(!db.is_banned(&guid, ip));

        db.add_ban("baduser", "Ares 2.1.0", &guid, ip, ip, 1234, 1).unwrap();

        assert!(db.is_banned(&guid, ip));
        assert!(db.remove_ban_by_guid(&guid).unwrap());
        assert!(!db.is_banned(&guid, ip));
    }

    #[test]
    fn ban_check_by_ip_only() {
        let db = Database::in_memory().unwrap();
        let guid1 = [0x01; 16];
        let guid2 = [0x02; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(5, 5, 5, 5));

        db.add_ban("a", "v", &guid1, ip, ip, 0, 1).unwrap();
        // guid distinto, pero misma IP => baneado
        assert!(db.is_banned(&guid2, ip));
    }

    #[test]
    fn account_upsert() {
        let db = Database::in_memory().unwrap();
        let guid = [0xCC; 16];
        let pwd = b"sha1hash";

        db.upsert_account("alice", 50, &guid, pwd).unwrap();
        let acc = db.find_account_by_name("alice").unwrap().unwrap();
        assert_eq!(acc.name, "alice");
        assert_eq!(acc.level, 50);

        // upsert reemplaza
        db.upsert_account("alice2", 80, &guid, pwd).unwrap();
        let acc = db.find_account_by_guid(&guid).unwrap().unwrap();
        assert_eq!(acc.name, "alice2");
        assert_eq!(acc.level, 80);
    }

    #[test]
    fn account_delete() {
        let db = Database::in_memory().unwrap();
        let guid = [0xDD; 16];
        db.upsert_account("x", 1, &guid, b"pwd").unwrap();
        assert!(db.delete_account(&guid).unwrap());
        assert!(db.find_account_by_guid(&guid).unwrap().is_none());
    }

    #[test]
    fn user_history_join_flood() {
        let db = Database::in_memory().unwrap();
        let guid = [0xEE; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(7, 7, 7, 7));

        // Primer join
        db.add_user_history("alice", "v", &guid, ip, ip, 0, unix_time()).unwrap();
        assert_eq!(db.count_joins_recent(ip, 15000).unwrap(), 1);

        // Segundo join
        let guid2 = [0xEF; 16];
        db.add_user_history("alice2", "v", &guid2, ip, ip, 0, unix_time()).unwrap();
        assert_eq!(db.count_joins_recent(ip, 15000).unwrap(), 2);
    }

    #[test]
    fn guid_hex_roundtrip() {
        let g = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0,
                 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let hex = guid_to_hex(&g);
        assert_eq!(hex, "123456789abcdef01122334455667788");
        let back = guid_from_hex(&hex).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn guid_hex_invalid() {
        assert!(guid_from_hex("tooshort").is_none());
        assert!(guid_from_hex("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ").is_none());
    }

    #[test]
    fn persistent_db_file() {
        let tmp = std::env::temp_dir().join(format!("astra_test_{}.db", unix_time()));
        let _db1 = Database::open(&tmp).unwrap();
        let guid = [0x42; 16];
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        _db1.add_ban("x", "v", &guid, ip, ip, 0, 1).unwrap();
        drop(_db1);

        // reabrir y verificar que el ban persistió
        let db2 = Database::open(&tmp).unwrap();
        assert!(db2.is_banned(&guid, ip));
        let _ = std::fs::remove_file(&tmp);
    }
}
