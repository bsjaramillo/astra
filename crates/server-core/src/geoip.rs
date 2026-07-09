//! Resolución GeoIP y ASN vía bases de datos MMDB **opcionales**.
//!
//! Soporta las bases en formato MaxMind (`.mmdb`), tanto GeoLite2 de MaxMind
//! como las gratuitas sin cuenta de DB-IP Lite. Si los archivos no están
//! presentes, el manager queda "vacío" y los comandos que lo usan
//! (`/trace`, `asnban`) degradan a un mensaje honesto en vez de fallar.
//!
//! Rutas por defecto (en `data/`): `city.mmdb` y `asn.mmdb`.

use std::net::IpAddr;
use std::path::Path;

use maxminddb::{geoip2, Reader};

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

/// Manager de GeoIP: readers MMDB opcionales para ciudad y ASN.
pub struct GeoIp {
    city: Option<Reader<Vec<u8>>>,
    asn: Option<Reader<Vec<u8>>>,
}

impl GeoIp {
    /// Carga las bases desde `data_dir/city.mmdb` y `data_dir/asn.mmdb` si
    /// existen. Si no, quedan en `None` (el manager es un no-op).
    pub fn load(data_dir: &Path) -> Self {
        let city = Self::open(&data_dir.join("city.mmdb"), "city");
        let asn = Self::open(&data_dir.join("asn.mmdb"), "asn");
        Self { city, asn }
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

    /// ¿Hay alguna base de ciudad cargada?
    pub fn has_city(&self) -> bool {
        self.city.is_some()
    }

    /// ¿Hay alguna base ASN cargada?
    pub fn has_asn(&self) -> bool {
        self.asn.is_some()
    }

    /// Resuelve la ciudad/país de una IP. `None` si no hay base o no matchea.
    pub fn lookup_city(&self, ip: IpAddr) -> Option<GeoCity> {
        let reader = self.city.as_ref()?;
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
        let reader = self.asn.as_ref()?;
        let rec: geoip2::Asn = reader.lookup(ip).ok()?;
        rec.autonomous_system_number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_is_noop() {
        // Directorio sin .mmdb → manager vacío, sin panics.
        let g = GeoIp::load(Path::new("/nonexistent-astra-geoip-dir"));
        assert!(!g.has_city());
        assert!(!g.has_asn());
        assert!(g.lookup_city("8.8.8.8".parse().unwrap()).is_none());
        assert!(g.lookup_asn("8.8.8.8".parse().unwrap()).is_none());
    }
}
