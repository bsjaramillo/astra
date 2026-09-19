//! Resolución GeoIP y ASN vía bases de datos MMDB **opcionales**.
//!
//! Soporta las bases en formato MaxMind (`.mmdb`), tanto GeoLite2 de MaxMind
//! como las gratuitas sin cuenta de DB-IP Lite. Si los archivos no están
//! presentes, el manager queda "vacío" y los comandos que lo usan
//! (`/trace`, `asnban`, el filtro anti-VPN) degradan a un mensaje honesto en
//! vez de fallar.
//!
//! Rutas por defecto (en `data/`): `city.mmdb` y `asn.mmdb`.
//!
//! Los readers son **reemplazables en runtime** (`replace_asn`/`replace_city`):
//! el loop de `geoip_update` descarga un MMDB nuevo y lo instala sin reiniciar,
//! de modo que el filtro anti-VPN puede empezar a resolver ASN en caliente.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use maxminddb::{geoip2, Reader};
use parking_lot::RwLock;

use crate::db::Database;
use crate::settings::GeoIpConfig;

/// Resultado de un lookup de ciudad.
#[derive(Debug, Clone, Default)]
pub struct GeoCity {
    /// País (nombre en inglés).
    pub country: Option<String>,
    /// Código de país ISO (ej. "US").
    pub country_code: Option<String>,
    /// Región/estado.
    pub region: Option<String>,
    /// Ciudad.
    pub city: Option<String>,
}

/// Manager de GeoIP: readers MMDB opcionales para ciudad y ASN, actualizables.
pub struct GeoIp {
    /// Base de datos para persistir la config live del updater.
    db: Arc<Database>,
    /// Directorio donde viven `city.mmdb` y `asn.mmdb`.
    data_dir: PathBuf,
    city: RwLock<Option<Reader<Vec<u8>>>>,
    asn: RwLock<Option<Reader<Vec<u8>>>>,
    /// Config del updater (live: editable desde el panel sin reiniciar).
    config: RwLock<GeoIpConfig>,
    /// Señal para pedir un refresco inmediato (botón "actualizar ahora" del
    /// panel): despierta al loop del binario sin esperar el intervalo.
    update_notify: Arc<tokio::sync::Notify>,
}

impl GeoIp {
    /// Carga las bases desde `data_dir/city.mmdb` y `data_dir/asn.mmdb` si
    /// existen. Si no, quedan en `None` (el manager es un no-op).
    ///
    /// La config live del updater se toma de la DB; si la tabla está vacía
    /// (primera ejecución), se usa `bootstrap` (lo que diga `astra.toml`) y se
    /// persiste. Así el panel manda a partir de ahí sin reinicio.
    pub fn load(db: Arc<Database>, data_dir: &Path, bootstrap: GeoIpConfig) -> Self {
        let city = Self::open(&data_dir.join("city.mmdb"), "city");
        let asn = Self::open(&data_dir.join("asn.mmdb"), "asn");
        let config = match db.load_geoip_config() {
            Ok(Some(cfg)) => cfg,
            _ => {
                // Primera vez: persistir el bootstrap para que el panel parte
                // de ese estado (y para no releer el TOML en cada arranque).
                let _ = db.save_geoip_config(&bootstrap);
                bootstrap
            }
        };
        Self {
            db,
            data_dir: data_dir.to_path_buf(),
            city: RwLock::new(city),
            asn: RwLock::new(asn),
            config: RwLock::new(config),
            update_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn open(path: &Path, label: &str) -> Option<Reader<Vec<u8>>> {
        if !path.exists() {
            return None;
        }
        match Reader::open_readfile(path) {
            Ok(r) => {
                tracing::info!("geoip: base {} cargada desde {}", label, path.display());
                Some(r)
            }
            Err(e) => {
                tracing::warn!("geoip: no se pudo abrir {}: {}", path.display(), e);
                None
            }
        }
    }

    /// Directorio de datos (donde se esperan `city.mmdb`/`asn.mmdb`).
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// ¿Hay alguna base de ciudad cargada?
    pub fn has_city(&self) -> bool {
        self.city.read().is_some()
    }

    /// ¿Hay alguna base ASN cargada?
    pub fn has_asn(&self) -> bool {
        self.asn.read().is_some()
    }

    /// Instala un reader ASN nuevo (descargado por el updater). Reemplaza el
    /// actual sin reiniciar; si había uno, lo descarta.
    pub fn replace_asn(&self, reader: Reader<Vec<u8>>) {
        *self.asn.write() = Some(reader);
        tracing::info!("geoip: base ASN actualizada en runtime");
    }

    /// Instala un reader de ciudad nuevo (descargado por el updater).
    pub fn replace_city(&self, reader: Reader<Vec<u8>>) {
        *self.city.write() = Some(reader);
        tracing::info!("geoip: base ciudad actualizada en runtime");
    }

    /// Abre un MMDB desde bytes ya en memoria y lo valida. `None` si el buffer
    /// no es un MMDB legible (protege contra descargas corruptas/HTML de error).
    pub fn reader_from_bytes(bytes: &[u8], label: &str) -> Option<Reader<Vec<u8>>> {
        match Reader::from_source(bytes.to_vec()) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!("geoip: {} descargado no es un MMDB válido: {}", label, e);
                None
            }
        }
    }

    /// Resuelve la ciudad/país de una IP. `None` si no hay base o no matchea.
    pub fn lookup_city(&self, ip: IpAddr) -> Option<GeoCity> {
        let guard = self.city.read();
        let reader = guard.as_ref()?;
        let rec: geoip2::City = reader.lookup(ip).ok()?;
        let country = rec.country.as_ref();
        Some(GeoCity {
            country: country
                .and_then(|c| c.names.as_ref())
                .and_then(|n| n.get("en").map(|s| s.to_string())),
            country_code: country.and_then(|c| c.iso_code.map(|s| s.to_string())),
            region: rec
                .subdivisions
                .as_ref()
                .and_then(|s| s.first())
                .and_then(|s| s.names.as_ref())
                .and_then(|n| n.get("en").map(|s| s.to_string())),
            city: rec
                .city
                .as_ref()
                .and_then(|c| c.names.as_ref())
                .and_then(|n| n.get("en").map(|s| s.to_string())),
        })
    }

    /// Resuelve el número de ASN de una IP. `None` si no hay base o no matchea.
    pub fn lookup_asn(&self, ip: IpAddr) -> Option<u32> {
        let guard = self.asn.read();
        let reader = guard.as_ref()?;
        let rec: geoip2::Asn = reader.lookup(ip).ok()?;
        rec.autonomous_system_number
    }

    /// Snapshot de la config live del updater.
    pub fn config(&self) -> GeoIpConfig {
        self.config.read().clone()
    }

    /// Notificación para pedir un refresco inmediato. La dispara el panel.
    pub fn update_notify(&self) -> Arc<tokio::sync::Notify> {
        self.update_notify.clone()
    }

    /// Pide un refresco inmediato (despierta al loop del updater).
    pub fn request_update(&self) {
        self.update_notify.notify_one();
    }

    /// ¿El updater automático está activo?
    pub fn is_update_enabled(&self) -> bool {
        self.config.read().enabled
    }

    /// Actualiza la config del updater y la persiste. Campos `None` se dejan
    /// como están.
    pub fn set_config(
        &self,
        enabled: Option<bool>,
        asn_url: Option<String>,
        city_url: Option<String>,
        refresh_hours: Option<u64>,
    ) {
        {
            let mut c = self.config.write();
            if let Some(e) = enabled {
                c.enabled = e;
            }
            if let Some(u) = asn_url {
                c.asn_url = u;
            }
            if let Some(u) = city_url {
                c.city_url = u;
            }
            if let Some(h) = refresh_hours {
                c.refresh_hours = h.max(1);
            }
        }
        let snapshot = self.config.read().clone();
        if let Err(e) = self.db.save_geoip_config(&snapshot) {
            tracing::warn!("geoip: no se pudo guardar la config del updater: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Arc<Database> {
        Database::in_memory().unwrap()
    }

    #[test]
    fn missing_files_is_noop() {
        // Directorio sin .mmdb → manager vacío, sin panics.
        let g = GeoIp::load(
            mem_db(),
            Path::new("/nonexistent-astra-geoip-dir"),
            GeoIpConfig::default(),
        );
        assert!(!g.has_city());
        assert!(!g.has_asn());
        assert!(g.lookup_city("8.8.8.8".parse().unwrap()).is_none());
        assert!(g.lookup_asn("8.8.8.8".parse().unwrap()).is_none());
    }

    #[test]
    fn invalid_mmdb_bytes_are_rejected() {
        // Bytes arbitrarios (p.ej. una página HTML de error) no deben cargar.
        assert!(GeoIp::reader_from_bytes(b"<html>error</html>", "asn").is_none());
        assert!(GeoIp::reader_from_bytes(&[], "asn").is_none());
    }

    #[test]
    fn bootstrap_config_persists_and_is_overridden_by_db() {
        let db = mem_db();
        let bootstrap = GeoIpConfig {
            enabled: true,
            asn_url: "https://example.test/{YYYY-MM}.mmdb.gz".to_string(),
            city_url: String::new(),
            refresh_hours: 12,
        };
        {
            let g = GeoIp::load(db.clone(), Path::new("/tmp"), bootstrap.clone());
            assert_eq!(g.config(), bootstrap);
            // Un cambio desde el panel se persiste.
            g.set_config(Some(false), Some("https://other.test/a.mmdb".to_string()), None, None);
        }
        // Nuevo arranque: la DB manda sobre el bootstrap.
        let g2 = GeoIp::load(
            db,
            Path::new("/tmp"),
            GeoIpConfig {
                enabled: true,
                asn_url: "bootstrap".to_string(),
                city_url: String::new(),
                refresh_hours: 24,
            },
        );
        let c = g2.config();
        assert!(!c.enabled);
        assert_eq!(c.asn_url, "https://other.test/a.mmdb");
        assert_eq!(c.refresh_hours, 12);
    }
}
