//! Chequeo periódico de nuevas versiones de Astra.
//!
//! La fuente de verdad es el registry de imágenes (ghcr.io), que es público:
//! sus tags (`0.1.NN`) son exactamente lo que las salas despliegan, así que
//! "hay un tag más nuevo" significa "puedes actualizar ya" (con astra-creator
//! `u` o `docker pull`).
//!
//! Bug histórico: `GET /v2/<img>/tags/list` sin `n` devuelve solo los primeros
//! 100 tags — el registry iba bien pero el check veía un corte a `0.1.17` y
//! nunca reportaba actualizaciones. Aquí se pagina completo con el cursor
//! `last` hasta agotar los tags.
//!
//! Cuando aparece una versión mayor a la corriendo, se guarda en
//! `AppContext::available_update` (la muestra el panel en Inicio) y se avisa
//! por PM a los admins/owners conectados CADA HORA (recordatorio recurrente,
//! no una sola vez). Los que se loguean después reciben el aviso al elevar su
//! nivel (ver `apply_level` en el crate de comandos) y en el siguiente tick.
//! Si el registry no responde, el fallo queda en `AppContext::update_check_error`
//! para que `/serverversion` y el panel lo muestren (nunca un falso "al día").

use std::sync::Arc;
use std::time::Duration;

use semver::Version;
use server_core::types::ILevel;
use server_core::AppContext;
use tracing::{debug, info};

/// Imagen cuyo listado de tags define la "última versión".
const IMAGE: &str = "bsjaramillo/astra";
/// Intervalo entre chequeos. El registry es anónimo y barato; 1 h permite
/// avisar por PM a admins/owners cada hora mientras haya una actualización.
const INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Página del listado de tags. Mayor que la cantidad actual (126) y por
/// encima del default de 100 que truncaba el resultado.
const TAGS_PAGE: u32 = 1000;

/// Loop del chequeo. Se spawnea desde `main` si `update_check` está activo.
/// El primer chequeo corre al arrancar y después cada hora; además escucha
/// `AppContext::trigger_update_check` (lo llama `/serverversion`) para
/// comprobar en el momento. Los errores de red se guardan en
/// `AppContext::update_check_error` — así el comando y el panel distinguen
/// "no hay nada nuevo" de "no se pudo comprobar" en vez de mentir con un
/// "estás al día" silencioso.
pub async fn check_loop(ctx: Arc<AppContext>) {
    let client = match reqwest::Client::builder()
        .user_agent(format!("astra/{}", server_core::VERSION))
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            debug!("update check deshabilitado: no se pudo crear el cliente HTTP: {e}");
            return;
        }
    };
    let current = match Version::parse(server_core::VERSION) {
        Ok(v) => v,
        Err(e) => {
            debug!("update check deshabilitado: versión propia no parseable: {e}");
            return;
        }
    };

    let mut interval = tokio::time::interval(INTERVAL);
    // El primer tick de `interval` dispara de inmediato, así que el primer
    // chequeo corre al arrancar sin esperar una hora.
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = ctx.update_check_notify.notified() => {}
        }
        check_once(&ctx, &client, &current).await;
    }
}

/// Corre una verificación y vuelca el resultado al `AppContext`:
/// `available_update` + limpieza de error si encontró una más nueva o si la
/// corriendo es la última; `update_check_error` + `update_check_last` para
/// que `/serverversion` y el panel no reporten un falso "al día".
async fn check_once(ctx: &AppContext, client: &reqwest::Client, current: &Version) {
    match fetch_latest(client).await {
        Ok(Some(latest)) => {
            *ctx.update_check_error.write() = None;
            *ctx.update_check_last.write() = Some(std::time::SystemTime::now());
            if latest <= *current {
                return;
            }
            let latest_str = latest.to_string();
            // Aviso por PM a admins/owners CADA HORA mientras haya una versión
            // más nueva (recordatorio recurrente; el panel también lo muestra).
            info!("nueva versión de Astra disponible: v{latest_str} (corriendo v{current})");
            *ctx.available_update.write() = Some(latest_str.clone());
            for user in ctx.user_pool.users() {
                if !user.logged_in || *user.level.read() < ILevel::Admin {
                    continue;
                }
                ctx.send_update_notice(&user, &latest_str);
            }
        }
        Ok(None) => {
            // El registry respondió pero no hay tags estables parseables: es
            // un chequeo "exitoso" (sin versión que comparar).
            *ctx.update_check_error.write() = None;
            *ctx.update_check_last.write() = Some(std::time::SystemTime::now());
        }
        Err(e) => {
            debug!("update check falló (se reintenta en el próximo tick): {e}");
            *ctx.update_check_error.write() = Some(e.to_string());
            *ctx.update_check_last.write() = Some(std::time::SystemTime::now());
        }
    }
}

/// Mayor versión ESTABLE publicada en el registry (descarta prereleases tipo
/// `-beta.NN` y `-rc.NN`; también los tags por arquitectura como `-amd64` y
/// `latest`). Pagina con el cursor `last` hasta agotar los tags (sin esto
/// ghcr.io devuelve solo 100 y el resultado queda truncado).
async fn fetch_latest(client: &reqwest::Client) -> anyhow::Result<Option<Version>> {
    // Token anónimo de pull: para imágenes públicas ghcr.io lo emite sin
    // credenciales, pero exige presentarlo igual en la API v2.
    let token: serde_json::Value = client
        .get(format!("https://ghcr.io/token?scope=repository:{IMAGE}:pull"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = token
        .get("token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("respuesta de token sin campo 'token'"))?
        .to_string();

    let mut tags: Vec<String> = Vec::new();
    let mut last: Option<String> = None;
    loop {
        let url = match &last {
            Some(l) => format!(
                "https://ghcr.io/v2/{IMAGE}/tags/list?n={TAGS_PAGE}&last={}",
                l
            ),
            None => format!("https://ghcr.io/v2/{IMAGE}/tags/list?n={TAGS_PAGE}"),
        };
        let page: serde_json::Value = client
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let page_tags: Vec<String> = page
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if page_tags.is_empty() {
            break;
        }
        tags.extend(page_tags.clone());
        // Si la página llegó completa, puede haber más: seguir desde el último.
        if page_tags.len() < TAGS_PAGE as usize {
            break;
        }
        last = page_tags.last().cloned();
    }

    Ok(tags.iter().filter_map(|t| parse_version_tag(t)).max())
}

/// Parsea un tag del registry como versión. Solo acepta RELEASES estables
/// (`1.2.3`): descarta prereleases (`-beta.NN`, `-rc.NN`) y las variantes por
/// arquitectura (`0.0.1-beta.33-amd64`), además de `latest`. Un pre-release
/// no debe marcar "hay una actualización" para una sala en producción.
fn parse_version_tag(tag: &str) -> Option<Version> {
    let v = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    if !v.pre.is_empty() {
        return None;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_tag_accepts_releases() {
        assert_eq!(
            parse_version_tag("v1.2.3"),
            Some(Version::parse("1.2.3").unwrap())
        );
        assert_eq!(
            parse_version_tag("0.1.31"),
            Some(Version::parse("0.1.31").unwrap())
        );
    }

    #[test]
    fn parse_version_tag_rejects_prereleases_and_arch_variants() {
        // Las prereleases NO deben contar como actualización.
        assert_eq!(parse_version_tag("0.0.1-beta.33"), None);
        assert_eq!(parse_version_tag("0.0.1-rc.1"), None);
        assert_eq!(parse_version_tag("0.0.1-beta.33-amd64"), None);
        assert_eq!(parse_version_tag("latest"), None);
    }

    #[test]
    fn stable_outranks_prerelease() {
        // Con la misma versión base, la estable debe elegirse sobre la beta
        // (aunque semver ordenaría la beta encima si ambas estuvieran).
        let stable = parse_version_tag("0.2.0").unwrap();
        let beta = parse_version_tag("0.2.0-beta.2");
        assert!(beta.is_none(), "la beta no debe parsear");
        assert!(stable > Version::parse("0.1.31").unwrap());
    }

    /// Integración real contra ghcr.io (requiere red). Correr con:
    /// `cargo test -p astra fetch_latest_real -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn fetch_latest_real() {
        let client = reqwest::Client::builder()
            .user_agent(format!("astra/{}", server_core::VERSION))
            .build()
            .unwrap();
        let latest = fetch_latest(&client).await.unwrap().expect("hay tags");
        // La última versión publicada nunca es menor que la corriendo.
        let current = Version::parse(server_core::VERSION).unwrap();
        assert!(latest >= current, "latest {latest} < current {current}");
    }
}
