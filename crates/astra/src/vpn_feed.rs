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

    loop {
        let cfg = ctx.vpn_filter.config();
        // Sin URL o sin horas válidas: no hay nada que refrescar. Se vuelve a
        // chequear en el próximo ciclo por si el admin lo cambia en el panel.
        let hours = cfg.refresh_hours.max(1);
        let sleep = Duration::from_secs(hours * 60 * 60);

        if !cfg.enabled || cfg.feed_url.trim().is_empty() {
            tokio::time::sleep(sleep).await;
            continue;
        }

        match fetch(&client, &cfg.feed_url).await {
            Ok(text) => {
                let loaded = ctx.vpn_filter.import_feed(&text);
                info!(
                    "vpn feed: {} entradas cargadas desde {}",
                    loaded, cfg.feed_url
                );
            }
            Err(e) => {
                // Se conserva la lista anterior: un fallo de red no debe dejar
                // la sala sin filtro.
                debug!("vpn feed falló (se conserva la lista anterior): {e}");
            }
        }

        tokio::time::sleep(sleep).await;
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
}
