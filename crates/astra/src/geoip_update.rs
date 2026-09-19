//! Descarga y refresco automático de las bases MMDB de GeoIP/ASN.
//!
//! `GeoIp` no puede resolver ASN sin `data/asn.mmdb`, y sin ASN el filtro
//! anti-VPN (parte ASN) y `asnban` quedan inertes. Este loop descarga la base,
//! la valida y la instala **sin reiniciar**.
//!
//! Fuente por defecto: **DB-IP Lite** (gratuita, sin cuenta, CC-BY). Su URL es
//! mensual (`dbip-asn-lite-YYYY-MM.mmdb.gz`), así que `resolve_url` sustituye
//! `{YYYY-MM}` por el mes actual y, si el server responde 404 (el mes aún no
//! publicado), reintenta con el mes anterior.
//!
//! Flujo de un ciclo:
//!   1. resolver URL y descargar bytes;
//!   2. descomprimir si viene gzip (magic `1f 8b`);
//!   3. **validar** que sea un MMDB legible (`GeoIp::reader_from_bytes`) — una
//!      descarga corrupta o una página HTML de error nunca pisa la base buena;
//!   4. escribir atómicamente a `data_dir/<city|asn>.mmdb.tmp` + rename, y
//!      recargar el reader en memoria.
//!
//! Si algo falla, se conserva la base anterior (fail-safe).

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use server_core::AppContext;
use tracing::{debug, info, warn};

/// Loop principal. Se spawnea desde `main` si `[geoip] enabled = true`.
pub async fn update_loop(ctx: Arc<AppContext>) {
    let client = match reqwest::Client::builder()
        .user_agent(format!("astra/{}", server_core::VERSION))
        .timeout(Duration::from_secs(300)) // MMDB ASN ~9 MB; margen holgado
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            debug!("geoip update deshabilitado: no se pudo crear el cliente HTTP: {e}");
            return;
        }
    };

    let notify = ctx.geoip.update_notify();

    // Sin base no hay ASN y el filtro no funciona, así que el updater baja la
    // base apenas arranca — pero SOLO si está habilitado. Deshabilitado no
    // genera tráfico saliente. Un `request_update()` (botón "actualizar
    // ahora") despierta el loop y fuerza una descarga puntual aunque el
    // updater esté apagado.
    let mut force = false;
    loop {
        let cfg = ctx.geoip.config();
        if cfg.enabled || force {
            if !cfg.asn_url.trim().is_empty() {
                refresh_one(&ctx, &client, &cfg.asn_url, "asn").await;
            }
            if !cfg.city_url.trim().is_empty() {
                refresh_one(&ctx, &client, &cfg.city_url, "city").await;
            }
        }
        force = false;

        let hours = cfg.refresh_hours.max(1);
        let sleep = tokio::time::sleep(Duration::from_secs(hours * 60 * 60));
        tokio::pin!(sleep);
        tokio::select! {
            _ = &mut sleep => {}
            _ = notify.notified() => {
                // Refresco manual: forzar el ciclo aunque `enabled` sea false.
                force = true;
            }
        }
    }
}

/// Descarga y aplica una base (`asn` o `city`).
async fn refresh_one(ctx: &AppContext, client: &reqwest::Client, url_template: &str, label: &str) {
    match download(client, url_template).await {
        Ok((bytes, used_url)) => {
            let Some(reader) = server_core::GeoIp::reader_from_bytes(&bytes, label) else {
                warn!("geoip: descarga de {} inválida; se conserva la base anterior", label);
                return;
            };

            // Persistir en disco (atómico) además de en memoria, para no
            // re-descargar en el próximo arranque.
            let path = ctx.geoip.data_dir().join(format!("{label}.mmdb"));
            if let Err(e) = write_atomic(&path, &bytes) {
                warn!("geoip: no se pudo escribir {}: {}", path.display(), e);
                // Aun sin persistir, instalar en memoria es mejor que nada.
            }

            if label == "asn" {
                ctx.geoip.replace_asn(reader);
            } else {
                ctx.geoip.replace_city(reader);
            }
            info!(
                "geoip: base {} actualizada desde {} ({} bytes)",
                label,
                used_url,
                bytes.len()
            );
        }
        Err(e) => {
            // Sin red o sin base disponible: no es fatal, se reintenta luego.
            debug!("geoip: fallo al descargar {} (se reintenta): {}", label, e);
        }
    }
}

/// Descarga la URL resolviendo `{YYYY-MM}`. Si el mes actual da 404, prueba el
/// mes anterior (DB-IP publica el día 1; en el cambio de mes puede tardar).
async fn download(client: &reqwest::Client, url_template: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let candidates = url_candidates(url_template);
    let mut last_err: Option<anyhow::Error> = None;
    for url in &candidates {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let raw = resp.bytes().await?.to_vec();
                let body = maybe_gunzip(raw)?;
                return Ok((body, url.clone()));
            }
            Ok(resp) => {
                last_err = Some(anyhow::anyhow!("HTTP {} en {}", resp.status(), url));
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!("{} en {}", e, url));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("sin candidatos de URL")))
}

/// Expande `{YYYY-MM}` al mes actual y al anterior. Sin placeholder, devuelve
/// la URL tal cual (un solo candidato).
fn url_candidates(template: &str) -> Vec<String> {
    if !template.contains("{YYYY-MM}") {
        return vec![template.to_string()];
    }
    let now = chrono::Utc::now();
    let this_month = now.format("%Y-%m").to_string();
    let prev = (now - chrono::Duration::days(31)).format("%Y-%m").to_string();
    let mut out = vec![template.replace("{YYYY-MM}", &this_month)];
    if prev != this_month {
        out.push(template.replace("{YYYY-MM}", &prev));
    }
    out
}

/// Descomprime si el cuerpo empieza con el magic de gzip (`1f 8b`).
fn maybe_gunzip(raw: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    if raw.len() < 2 || raw[0] != 0x1f || raw[1] != 0x8b {
        return Ok(raw);
    }
    let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Escribe bytes de forma atómica: archivo temporal en el mismo directorio y
/// `rename` (atómico en el mismo filesystem). Evita dejar un `asn.mmdb` a
/// medias si el proceso muere en medio de la escritura.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("mmdb.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_without_placeholder_is_single_candidate() {
        let c = url_candidates("https://example.test/asn.mmdb");
        assert_eq!(c, vec!["https://example.test/asn.mmdb"]);
    }

    #[test]
    fn url_with_placeholder_expands_to_two_months() {
        let c = url_candidates("https://example.test/dbip-{YYYY-MM}.mmdb.gz");
        assert_eq!(c.len(), 2);
        assert!(c[0].contains("dbip-"));
        for u in &c {
            assert!(!u.contains("{YYYY-MM}"), "placeholder sin resolver: {u}");
            assert!(u.ends_with(".mmdb.gz"));
        }
        // Los dos candidatos deben ser meses distintos.
        assert_ne!(c[0], c[1]);
    }

    #[test]
    fn gunzip_passthrough_for_non_gzip() {
        let raw = b"not gzip".to_vec();
        assert_eq!(maybe_gunzip(raw.clone()).unwrap(), raw);
    }

    #[test]
    fn gunzip_decompresses_gzip() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"hello mmdb").unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(maybe_gunzip(gz).unwrap(), b"hello mmdb");
    }

    /// Integración real contra DB-IP (requiere red). Descarga la base ASN,
    /// la valida como MMDB y comprueba que resuelve el ASN de una IP conocida
    /// (8.8.8.8 → AS15169 Google). Correr con:
    /// `cargo test -p astra geoip_download_real -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn geoip_download_real_and_lookup() {
        let client = reqwest::Client::builder()
            .user_agent("astra-test")
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();
        let url_template = server_core::settings::GeoIpConfig::default().asn_url;
        let (bytes, url) = download(&client, &url_template)
            .await
            .expect("descarga del feed DB-IP");
        assert!(bytes.len() > 1_000_000, "MMDB demasiado chico: {url}");
        let reader = server_core::GeoIp::reader_from_bytes(&bytes, "asn")
            .expect("el cuerpo descargado debe ser un MMDB válido");
        let rec: server_core::maxminddb::geoip2::Asn = reader
            .lookup("8.8.8.8".parse::<std::net::IpAddr>().unwrap())
            .expect("lookup de 8.8.8.8");
        assert_eq!(rec.autonomous_system_number, Some(15169));
    }

    #[test]
    fn write_atomic_replaces_file() {
        let dir = std::env::temp_dir().join(format!(
            "astra_geoip_test_{}",
            server_core::time::unix_time()
        ));
        let path = dir.join("asn.mmdb");
        write_atomic(&path, b"one").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        // No debe quedar el temporal.
        assert!(!dir.join("asn.mmdb.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
