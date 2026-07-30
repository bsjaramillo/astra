//! Publicación de la sala en el directorio público.
//!
//! Cada pocos minutos se manda la ficha de la sala —nombre, topic,
//! descripción, tags— para que aparezca en el catálogo web y su dueño pueda
//! gestionarla. Es **opt-in**: sin `[directory] enabled = true` no sale nada
//! de aquí.
//!
//! ## Lo que NUNCA se envía
//!
//! - El **`guid`** del servidor. Es el secreto con el que un leaf se autentica
//!   contra un hub (`credentials = SHA1(reverse(name ++ guid))`): publicarlo
//!   permitiría a cualquiera hacerse pasar por esta sala en el Link. Se manda
//!   un identificador derivado, `sha256("astra-directory-v1:" ++ guid)`, que
//!   sirve para reconocer la sala entre reinicios y no revela nada.
//! - La `owner_password`, las claves de Supabase, los trusted leaves.
//! - Cualquier dato de los usuarios conectados: ni nicks, ni IPs, ni cuentas.
//!
//! El único dato "de la sala" que viaja es cuánta gente hay, y el directorio
//! ni siquiera se fía de él: lo contrasta con su propio sondeo.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use server_core::settings::Settings;
use server_core::AppContext;
use tracing::{debug, info, warn};

/// Cadencia por defecto. El directorio puede pedir otra en su respuesta.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Techo del backoff cuando el directorio no responde.
const MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);

/// Bucle de publicación. Se lanza desde `main` si está activado.
pub async fn heartbeat_loop(ctx: Arc<AppContext>) {
    let client = match reqwest::Client::builder()
        .user_agent(format!("astra/{}", server_core::VERSION))
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            debug!("directorio deshabilitado: no se pudo crear el cliente HTTP: {e}");
            return;
        }
    };

    // Un arranque escalonado. Sin esto, un `docker compose up` con varias
    // salas —o un reinicio del directorio— las hace llamar todas a la vez.
    let jitter = {
        use rand::Rng;
        Duration::from_millis(rand::thread_rng().gen_range(0..60_000))
    };
    tokio::time::sleep(jitter).await;

    let mut interval = DEFAULT_INTERVAL;
    let mut backoff = DEFAULT_INTERVAL;

    loop {
        match send_once(&ctx, &client).await {
            Ok(resp) => {
                backoff = interval;
                if let Some(secs) = resp.interval_secs {
                    // Se acota: un directorio comprometido no puede convertir
                    // a las salas en una fuente de tráfico contra nadie.
                    interval = Duration::from_secs(secs.clamp(60, 3600));
                }
                if let Some(token) = resp.token {
                    persist_token(&ctx, &token);
                }
                if let Some(url) = resp.url {
                    let previo = ctx.directory_listing();
                    if previo.as_deref() != Some(url.as_str()) {
                        info!("sala publicada en el directorio: {url}");
                        *ctx.directory_listing.write() = Some(url);
                    }
                }
            }
            Err(e) => {
                debug!("directorio: no se pudo publicar ({e:#}); se reintenta");
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

/// Respuesta del directorio.
struct Response {
    token: Option<String>,
    url: Option<String>,
    interval_secs: Option<u64>,
}

async fn send_once(ctx: &AppContext, client: &reqwest::Client) -> anyhow::Result<Response> {
    let cfg = &ctx.settings.directory;
    let url = format!("{}/api/v1/heartbeat", cfg.url.trim_end_matches('/'));

    let mut req = client.post(&url).json(&payload(ctx));
    // La credencial vive en el config si se pudo guardar; si no, el directorio
    // reconoce la sala por su identificador y su IP y emite una nueva.
    let token = ctx
        .directory_token()
        .filter(|t| !t.is_empty())
        .or_else(|| Some(cfg.token.clone()).filter(|t| !t.is_empty()));
    if let Some(t) = &token {
        req = req.bearer_auth(t);
    }

    let body: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
    Ok(Response {
        token: body.get("token").and_then(|v| v.as_str()).map(str::to_string),
        url: body.get("url").and_then(|v| v.as_str()).map(str::to_string),
        interval_secs: body.get("interval_secs").and_then(|v| v.as_u64()),
    })
}

/// La ficha que se publica.
fn payload(ctx: &AppContext) -> serde_json::Value {
    let cfg = &ctx.settings.directory;
    json!({
        "public_id": public_room_id(&ctx.settings.guid),
        "name": ctx.settings.room_name,
        "topic": ctx.current_room_topic(),
        "description": cfg.description,
        "website": cfg.website,
        "tags": cfg.tags,
        "host": cfg.public_host,
        "port": ctx.settings.port,
        "tls": cfg.tls,
        "language": ctx.settings.language,
        "server": format!("Astra {}", server_core::VERSION),
        "web_enabled": ctx.settings.web_enabled,
        "roomsearch": ctx.settings.roomsearch,
        "users": ctx.user_pool.len(),
        "listed": cfg.listed,
    })
}

/// Identificador público de la sala, derivado del guid.
///
/// El guid **no** se envía: es el secreto de autenticación del Link. Este
/// derivado solo sirve para que el directorio reconozca la sala entre
/// reinicios, y de él no se puede volver al original.
pub fn public_room_id(guid: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("astra-directory-v1:{guid}").as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// Guarda la credencial en el `astra.toml`.
///
/// Puede fallar, y no es excepcional: en Docker el fichero se monta a menudo
/// de solo lectura. Se avisa una vez y se sigue funcionando — el directorio
/// contempla ese caso y reemite la credencial en cada arranque.
fn persist_token(ctx: &AppContext, token: &str) {
    *ctx.directory_token.write() = Some(token.to_string());

    let Some(path) = ctx.config_path() else { return };
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<Settings>(&text).unwrap_or_else(|_| (*ctx.settings).clone()),
        Err(_) => (*ctx.settings).clone(),
    };
    if settings.directory.token == token {
        return;
    }
    settings.directory.token = token.to_string();
    if let Err(e) = settings.save(&path) {
        warn!(
            "no se pudo guardar la credencial del directorio en {}: {e}. \
             La sala sigue publicándose; se pedirá una nueva en cada arranque.",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_id_publico_no_revela_el_guid() {
        let guid = "6034ed7e0df26ad0b6e9adfa2f12064a";
        let id = public_room_id(guid);
        assert_eq!(id.len(), 32);
        assert!(!id.contains(guid), "el guid no puede aparecer en el derivado");
        // Estable entre arranques, para que la sala se reconozca.
        assert_eq!(id, public_room_id(guid));
        // Y distinto para otra sala.
        assert_ne!(id, public_room_id("otro-guid"));
    }
}
