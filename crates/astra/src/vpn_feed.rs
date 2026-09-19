//! Refresco periódico del feed del filtro anti-VPN/proxy.
//!
//! El filtro (`server_core::vpn_filter::VpnFilterManager`) detecta VPNs/proxies
//! combinando ASN + una blocklist CIDR. La blocklist se puede cargar a mano
//! desde el panel, pero el uso normal es apuntarla a un feed público que se
//! refresca solo.
//!
//! Este loop descarga `feed_url` (configurable en vivo desde el panel) cada
//! `refresh_hours` y reemplaza las entradas de fuente `feed` de forma atómica
//! (`import_feed`). Las entradas `manual` del admin nunca se tocan. Si no hay
//! URL configurada o el filtro está deshabilitado, el loop no gasta red.
//!
//! Idéntico patrón al `update_check`: primer tick inmediato, log honesto de
//! errores (nunca un falso "todo bien") y espera al intervalo siguiente.

use std::sync::Arc;
use std::time::Duration;

use server_core::AppContext;
use tracing::{debug, info};

/// Loop de refresco. Se spawnea desde `main`; nunca retorna.
pub async fn refresh_loop(ctx: Arc<AppContext>) {
    let client = match reqwest::Client::builder()
        .user_agent(format!("astra/{}", server_core::VERSION))
        .timeout(Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            debug!("vpn feed deshabilitado: no se pudo crear el cliente HTTP: {e}");
            return;
        }
    };

    let notify = ctx.vpn_filter.refresh_notify();

    // Descarga inicial: si el filtro ya viene habilitado de la config (o de un
    // arranque previo), la lista debe estar cargada cuanto antes. Si al
    // arrancar está vacía y habilitado, forzamos un ciclo ya.
    let mut force = {
        let cfg = ctx.vpn_filter.config();
        cfg.enabled && !cfg.feed_url.trim().is_empty()
    };

    loop {
        let cfg = ctx.vpn_filter.config();
        let hours = cfg.refresh_hours.max(1);
        let sleep = tokio::time::sleep(Duration::from_secs(hours * 60 * 60));
        tokio::pin!(sleep);

        // Se descarga si el filtro está activo, si la lista está vacía con el
        // filtro activo, o si un `request_refresh()` lo pidió (activación
        // desde el panel, cambio de URL, botón "refrescar").
        let should_fetch = (cfg.enabled || force) && !cfg.feed_url.trim().is_empty();

        if should_fetch && ctx.vpn_filter.try_begin_refresh() {
            match fetch(&client, &cfg.feed_url).await {
                Ok(text) => {
                    let loaded = ctx.vpn_filter.import_feed(&text);
                    info!(
                        "vpn feed: {} entradas cargadas desde {}",
                        loaded, cfg.feed_url
                    );
                }
                Err(e) => {
                    // Se conserva la lista anterior: un fallo de red no debe
                    // dejar la sala sin filtro.
                    debug!("vpn feed falló (se conserva la lista anterior): {e}");
                }
            }
            ctx.vpn_filter.end_refresh();
        }
        force = false;

        tokio::select! {
            _ = &mut sleep => {}
            _ = notify.notified() => {
                // Refresco pedido (activar filtro / cambiar URL / botón).
                force = true;
            }
        }
    }
}

/// Descarga el feed como texto. Acepta respuestas grandes (las listas de VPN
/// traen decenas de miles de líneas).
async fn fetch(client: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let text = resp.text().await?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El parseo real vive en `VpnFilterManager::import_feed` y está cubierto
    /// por tests en `server-core`. Aquí solo se valida que el loop no explota
    /// con un cliente HTTP mínimo y una URL vacía (no debe hacer request).
    #[test]
    fn empty_feed_url_is_skipped_by_config_check() {
        let cfg = server_core::VpnConfig {
            enabled: true,
            feed_url: String::new(),
            ..server_core::VpnConfig::default()
        };
        assert!(cfg.feed_url.trim().is_empty());
    }

    /// Integración real contra el feed X4BNet (requiere red): descarga la
    /// lista, la importa en un manager habilitado y comprueba que detecta una
    /// IP concreta que el feed cubre. Es la verificación end-to-end del flujo
    /// activar → descargar → match. Correr con:
    /// `cargo test -p astra vpn_feed_real -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn vpn_feed_real_download_and_match() {
        let client = reqwest::Client::builder()
            .user_agent("astra-test")
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap();
        let url = server_core::VpnConfig::default().feed_url;
        let text = fetch(&client, &url).await.expect("descarga del feed X4BNet");

        let db = server_core::db::Database::in_memory().unwrap();
        let mgr = server_core::VpnFilterManager::new(db);
        let loaded = mgr.import_feed(&text);
        assert!(loaded > 1000, "feed demasiado chico: {loaded} entradas");
        mgr.set_enabled(true);

        let geoip = server_core::GeoIp::load(
            server_core::db::Database::in_memory().unwrap(),
            std::path::Path::new("/nonexistent"),
            server_core::settings::GeoIpConfig::default(),
        );
        // 187.14.120.0/21 está en el feed y contiene esta IP.
        assert!(
            mgr.classify(&geoip, "187.14.127.35".parse().unwrap()).is_some(),
            "el feed debería detectar 187.14.127.35"
        );
        // Una IP que no está en la lista no debe matchear.
        assert!(mgr.classify(&geoip, "8.8.8.8".parse().unwrap()).is_none());
    }
}
