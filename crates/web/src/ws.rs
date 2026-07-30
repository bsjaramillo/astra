//! WebSocket server (RFC 6455) para clientes ib0t (HTML5).
//!
//! Acepta conexiones TCP en un puerto separado, hace el handshake
//! HTTP/1.1 → Upgrade, y luego pasa al modo WebSocket (frames de texto/binario).

use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine;
use bytes::{Buf, BytesMut};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

use server_core::AppContext;

use crate::handler::handle_connection;

/// GUID mágica del estándar WebSocket (RFC 6455).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Maneja una conexión TCP potencialmente WebSocket: handshake HTTP y luego
/// frames WS. `scripting` se propaga para poder despachar eventos de los
/// clientes web a los scripts (onJoin/onPart/onPublic/onCommand/...).
pub async fn handle_stream(
    ctx: Arc<AppContext>,
    stream: TcpStream,
    peer: SocketAddr,
    scripting: astra_scripting::ScriptHandle,
) -> anyhow::Result<()> {
    handle_ws_connection(ctx, stream, peer, scripting).await
}

/// Maneja una conexión WS entrante: hace el handshake y delega a `handle_connection`.
async fn handle_ws_connection(
    ctx: Arc<AppContext>,
    mut stream: TcpStream,
    peer: SocketAddr,
    scripting: astra_scripting::ScriptHandle,
) -> anyhow::Result<()> {
    // "HTTP" y no "WS": hasta leer el request no sabemos si es un handshake
    // WebSocket (cliente de sala) o un GET normal (panel). Los logs de más
    // abajo ("WS conectado" / "sirviendo panel HTTP") lo aclaran, para que sea
    // fácil distinguir qué es cada conexión en el log.
    debug!("nueva conexión HTTP desde {}", peer);

    // 1) Leer el handshake HTTP
    let request = read_http_request(&mut stream).await?;
    debug!("WS handshake request de {}: {} headers", peer, request.headers.len());

    // 1.5) Rutas del panel de administración (HTTP, no WebSocket).
    if request.path == "/admin" || request.path.starts_with("/admin/") || request.path.starts_with("/admin?") {
        // La IP real del cliente (no la del reverse proxy) para el rate-limit
        // de `/admin/login`: detrás de Caddy todas las peticiones llegan con la
        // IP del contenedor proxy, y sin resolver el forwarded header un solo
        // atacante bloquearía a todos los administradores.
        let admin_ip = resolve_client_ip(&ctx, peer.ip(), &request.headers);
        return handle_admin_route(&ctx, &mut stream, &request, admin_ip).await;
    }

    // 2) Extraer la clave
    let key = request
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))
        .map(|(_, v)| v.clone());

    let key = match key {
        Some(k) => k,
        None => {
            // HTTP plano (sin upgrade a WebSocket): servir el panel HTML
            // en GET /; cualquier otra cosa es un 400.
            if request.method.eq_ignore_ascii_case("GET") {
                // GET normal (navegador abriendo la URL): NO es un cliente de
                // sala, es el panel HTML. Se loguea distinto para que no se
                // confunda con un intento de entrar a la sala.
                info!("HTTP GET {} de {}: sirviendo panel (no es cliente de sala)", request.path, peer);
                send_http_html(&mut stream, crate::panel::INDEX_HTML).await?;
            } else {
                warn!("HTTP {} de {} sin Sec-WebSocket-Key (no es WS): 400", request.method, peer);
                send_http_error(&mut stream, 400, "Missing Sec-WebSocket-Key").await?;
            }
            return Ok(());
        }
    };

    // 2.5) Rate-limit de conexiones por IP — SOLO para handshakes WebSocket
    // de clientes de sala (no para el panel HTTP, que hace polling `fetch`
    // cada 5s y NO debe contar como "conexión nueva": si contara, el propio
    // administrador se auto-banearía). Paridad con el path Ares nativo. Se
    // exime a proxies reversos confiables y loopback (detrás de un proxy
    // todos los usuarios web comparten la IP, así que ahí no se puede
    // limitar por IP).
    if !ctx.trusted_proxies.is_trusted(peer.ip()) {
        if let Some(reason) = ctx.security.conn_flood.check(peer.ip()) {
            warn!(
                "REJECTED WS (rate limit de conexiones por IP): {} — {}",
                peer,
                reason.message()
            );
            // Cerrar sin completar el upgrade (el cliente verá conexión rechazada).
            let _ = send_http_error(&mut stream, 429, "Too many connections").await;
            return Ok(());
        }
    }

    // 3) Calcular Sec-WebSocket-Accept
    let accept = compute_accept_key(&key);

    // 4) Enviar respuesta 101 Switching Protocols
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         \r\n",
        accept
    );
    stream.write_all(response.as_bytes()).await?;
    debug!("WS handshake completado con {}", peer);

    // 5) Pasar al modo WebSocket. Si `peer` es un reverse proxy confiable
    // (paridad `ib0tClient.ApplyForwardedIP` de sb0t), resolvemos la IP real
    // del cliente desde `X-Real-IP`/`X-Forwarded-For` aquí — es el único
    // punto donde tenemos los headers HTTP del handshake.
    let resolved_ip = resolve_client_ip(&ctx, peer.ip(), &request.headers);
    handle_connection(ctx, stream, peer, resolved_ip, scripting).await
}

/// Resuelve la IP "real" de un cliente WS, confiando en `X-Real-IP`/
/// `X-Forwarded-For` solo si el peer directo (`peer_ip`) está en la lista
/// de proxies confiables (o es loopback). Si no se confía, o los headers
/// no están/no parsean, retorna `peer_ip` sin cambios.
///
/// Paridad `ib0tClient.ApplyForwardedIP` de sb0t: `X-Real-IP` gana si está
/// presente y parsea; si no, se usa el primer valor (el más a la izquierda,
/// el cliente original) de `X-Forwarded-For`.
fn resolve_client_ip(
    ctx: &AppContext,
    peer_ip: std::net::IpAddr,
    headers: &[(String, String)],
) -> std::net::IpAddr {
    if !ctx.trusted_proxies.is_trusted(peer_ip) {
        return peer_ip;
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    if let Some(real) = header("x-real-ip").and_then(|v| v.trim().parse::<std::net::IpAddr>().ok()) {
        return real;
    }
    if let Some(xff) = header("x-forwarded-for") {
        if let Some(first) = xff.split(',').next().and_then(|s| s.trim().parse::<std::net::IpAddr>().ok()) {
            return first;
        }
    }
    peer_ip
}

/// Lee un HTTP request completo (headers + body si hay Content-Length).
async fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<HttpRequest> {
    let mut buf = vec![0u8; 65536];
    let mut total = 0;

    // 1) Leer hasta el fin de los headers (\r\n\r\n).
    let header_end;
    loop {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            anyhow::bail!("conexión cerrada antes del handshake");
        }
        total += n;
        if let Some(idx) = find_double_crlf(&buf[..total]) {
            header_end = idx;
            break;
        }
        if total >= buf.len() {
            anyhow::bail!("HTTP request demasiado largo");
        }
    }

    let raw = std::str::from_utf8(&buf[..header_end])?;
    let mut req = parse_http_request(raw);

    // 2) Si hay Content-Length, leer el body (para POST del panel admin).
    let content_len: usize = req
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if content_len > 0 && content_len <= 1_048_576 {
        let mut body = buf[header_end..total].to_vec();
        while body.len() < content_len {
            let n = stream.read(&mut buf[..]).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
        body.truncate(content_len);
        req.body = String::from_utf8_lossy(&body).into_owned();
    }

    Ok(req)
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(3) {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Token del header `Authorization: Bearer <token>`.
    fn bearer_token(&self) -> &str {
        self.header("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("")
            .trim()
    }
}

fn parse_http_request(raw: &str) -> HttpRequest {
    let mut lines = raw.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }

    HttpRequest {
        method,
        path,
        headers,
        body: String::new(),
    }
}

/// Rutea las peticiones del panel de administración.
///
/// - `GET /admin`           → HTML del panel (login incluido en la página).
/// - `POST /admin/login`    → `{password}` → `{token}` (o 401).
/// - `GET  /admin/state`    → snapshot JSON (requiere Bearer token).
/// - `POST /admin/cmd`      → `{cmd}` → `{output:[...]}` (requiere Bearer token).
///
/// `client_ip` es la IP real del cliente (resuelta contra los proxies
/// confiables); se usa para el rate-limit de intentos de login.
async fn handle_admin_route(
    ctx: &Arc<AppContext>,
    stream: &mut TcpStream,
    req: &HttpRequest,
    client_ip: std::net::IpAddr,
) -> anyhow::Result<()> {
    let path = req.path.split('?').next().unwrap_or("");

    // El panel HTML no requiere token (el login se hace desde la página).
    if path == "/admin" || path == "/admin/" {
        send_http_html(stream, crate::panel::ADMIN_HTML).await?;
        return Ok(());
    }

    if !crate::admin::is_enabled(ctx) {
        send_http_json(stream, 403, "{\"error\":\"admin panel disabled (no owner password set)\"}").await?;
        return Ok(());
    }

    if path == "/admin/login" && req.method.eq_ignore_ascii_case("POST") {
        let password = json_field(&req.body, "password").unwrap_or_default();
        match crate::admin::authenticate(ctx, client_ip, &password) {
            Ok(token) => {
                info!("panel admin: login exitoso desde {}", client_ip);
                let body = format!("{{\"token\":\"{}\"}}", token);
                send_http_json(stream, 200, &body).await?;
            }
            Err(crate::admin::AuthError::Invalid) => {
                warn!("panel admin: login FALLIDO desde {}", client_ip);
                send_http_json(stream, 401, "{\"error\":\"invalid password\"}").await?;
            }
            Err(crate::admin::AuthError::Throttled) => {
                warn!(
                    "panel admin: login bloqueado por demasiados intentos desde {}",
                    client_ip
                );
                send_http_json(
                    stream,
                    429,
                    "{\"error\":\"too many failed attempts, try again later\"}",
                )
                .await?;
            }
        }
        return Ok(());
    }

    // A partir de aquí se requiere token válido.
    if !crate::admin::validate(req.bearer_token()) {
        send_http_json(stream, 401, "{\"error\":\"unauthorized\"}").await?;
        return Ok(());
    }

    match (req.method.as_str(), path) {
        ("GET", "/admin/state") => {
            let json = crate::admin::state_json(ctx);
            send_http_json(stream, 200, &json).await?;
        }
        (m, "/admin/cmd") if m.eq_ignore_ascii_case("POST") => {
            let cmd = json_field(&req.body, "cmd").unwrap_or_default();
            let lines = crate::admin::run_command(ctx, &cmd);
            let arr: Vec<String> = lines.iter().map(|l| format!("\"{}\"", json_escape(l))).collect();
            let body = format!("{{\"output\":[{}]}}", arr.join(","));
            send_http_json(stream, 200, &body).await?;
        }
        ("GET", "/admin/settings") => {
            let toml = crate::admin::read_settings(ctx);
            let body = format!("{{\"toml\":\"{}\"}}", json_escape(&toml));
            send_http_json(stream, 200, &body).await?;
        }
        (m, "/admin/settings") if m.eq_ignore_ascii_case("POST") => {
            let toml = json_field(&req.body, "toml").unwrap_or_default();
            match crate::admin::write_settings(ctx, &toml) {
                Ok(()) => send_http_json(stream, 200, "{\"ok\":true}").await?,
                Err(e) => {
                    let body = format!("{{\"error\":\"{}\"}}", json_escape(&e));
                    send_http_json(stream, 400, &body).await?;
                }
            }
        }
        ("GET", "/admin/config") => {
            let json = crate::admin::settings_json(ctx);
            send_http_json(stream, 200, &json).await?;
        }
        (m, "/admin/config") if m.eq_ignore_ascii_case("POST") => {
            match crate::admin::write_settings_json(ctx, &req.body) {
                Ok(()) => send_http_json(stream, 200, "{\"ok\":true}").await?,
                Err(e) => {
                    let body = format!("{{\"error\":\"{}\"}}", json_escape(&e));
                    send_http_json(stream, 400, &body).await?;
                }
            }
        }
        (m, "/admin/proxy/add") if m.eq_ignore_ascii_case("POST") => {
            let ip = json_field(&req.body, "ip").unwrap_or_default();
            let body = format!("{{\"ok\":{}}}", crate::admin::add_trusted_proxy(ctx, &ip));
            send_http_json(stream, 200, &body).await?;
        }
        (m, "/admin/proxy/remove") if m.eq_ignore_ascii_case("POST") => {
            let ip = json_field(&req.body, "ip").unwrap_or_default();
            let body = format!("{{\"ok\":{}}}", crate::admin::remove_trusted_proxy(ctx, &ip));
            send_http_json(stream, 200, &body).await?;
        }
        (m, "/admin/avatar") if m.eq_ignore_ascii_case("POST") => {
            let kind = json_field(&req.body, "kind").unwrap_or_default();
            let data_b64 = json_field(&req.body, "data_b64").unwrap_or_default();
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(data_b64.as_bytes()) {
                Ok(bytes) => match crate::admin::set_avatar(ctx, &kind, bytes) {
                    Ok(()) => send_http_json(stream, 200, "{\"ok\":true}").await?,
                    Err(e) => {
                        let body = format!("{{\"error\":\"{}\"}}", json_escape(&e));
                        send_http_json(stream, 400, &body).await?;
                    }
                },
                Err(_) => {
                    send_http_json(stream, 400, "{\"error\":\"invalid base64\"}").await?;
                }
            }
        }
        ("GET", "/admin/motd") => {
            let body = format!("{{\"text\":\"{}\"}}", json_escape(&ctx.motd.text()));
            send_http_json(stream, 200, &body).await?;
        }
        (m, "/admin/motd") if m.eq_ignore_ascii_case("POST") => {
            let text = json_field(&req.body, "text").unwrap_or_default();
            ctx.motd.set(&text);
            send_http_json(stream, 200, "{\"ok\":true}").await?;
        }
        ("GET", "/admin/template") => {
            let body = format!("{{\"text\":\"{}\"}}", json_escape(&ctx.templates.export_text()));
            send_http_json(stream, 200, &body).await?;
        }
        (m, "/admin/template") if m.eq_ignore_ascii_case("POST") => {
            let text = json_field(&req.body, "text").unwrap_or_default();
            let n = ctx.templates.apply_bulk(&text);
            let body = format!("{{\"ok\":true,\"applied\":{}}}", n);
            send_http_json(stream, 200, &body).await?;
        }
        ("GET", "/admin/avatar/server") => {
            match crate::admin::get_avatar_bytes(ctx, "server") {
                Some(bytes) => send_http_bytes(stream, 200, &bytes).await?,
                None => send_http_bytes(stream, 404, b"").await?,
            }
        }
        ("GET", "/admin/avatar/default") => {
            match crate::admin::get_avatar_bytes(ctx, "default") {
                Some(bytes) => send_http_bytes(stream, 200, &bytes).await?,
                None => send_http_bytes(stream, 404, b"").await?,
            }
        }
        _ => {
            send_http_json(stream, 404, "{\"error\":\"not found\"}").await?;
        }
    }
    Ok(())
}

/// Extrae un campo string de nivel superior de un JSON simple (sin parser
/// completo: busca `"campo":"valor"` con escapes básicos).
fn json_field(body: &str, field: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get(field)?.as_str().map(|s| s.to_string())
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Responde JSON. Estas respuestas son todas del panel `/admin`, así que
/// deliberadamente NO llevan `Access-Control-Allow-Origin`: con el wildcard,
/// cualquier página web podía hacer `fetch("https://tu-sala/admin/login")` con
/// passwords y LEER la respuesta (el token) desde el navegador del admin. Sin
/// el header, el navegador bloquea la lectura cross-origin.
async fn send_http_json(stream: &mut TcpStream, code: u16, body: &str) -> anyhow::Result<()> {
    let status = match code {
        200 => "200 OK",
        401 => "401 Unauthorized",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        429 => "429 Too Many Requests",
        _ => "400 Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        status,
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn compute_accept_key(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    let digest = hasher.finalize();
    base64::engine::general_purpose::STANDARD.encode(digest)
}

async fn send_http_error(
    stream: &mut TcpStream,
    code: u16,
    msg: &str,
) -> anyhow::Result<()> {
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: text/plain\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {}",
        code, msg, msg.len(), msg
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

/// Responde 200 OK con bytes crudos (imagen), adivinando el Content-Type
/// por los magic bytes. Usado para servir avatares (`GET /admin/avatar/*`).
async fn send_http_bytes(stream: &mut TcpStream, code: u16, bytes: &[u8]) -> anyhow::Result<()> {
    let (status, content_type): (&str, &str) = if code != 200 {
        ("404 Not Found", "text/plain")
    } else if bytes.starts_with(b"\x89PNG") {
        ("200 OK", "image/png")
    } else if bytes.starts_with(b"\xFF\xD8") {
        ("200 OK", "image/jpeg")
    } else {
        ("200 OK", "application/octet-stream")
    };
    let header = format!(
        "HTTP/1.1 {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\
         \r\n",
        status,
        content_type,
        bytes.len()
    );
    stream.write_all(header.as_bytes()).await?;
    if code == 200 {
        stream.write_all(bytes).await?;
    }
    Ok(())
}

/// Responde 200 OK con un body HTML y cierra la conexión.
async fn send_http_html(stream: &mut TcpStream, html: &str) -> anyhow::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        html.len(),
        html
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

// ============================================================================
// WebSocket frame reader/writer (RFC 6455)
// ============================================================================

/// Opcode de un frame WebSocket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WsOpcode {
    /// Frame de continuación
    Continuation = 0x0,
    /// Frame de texto
    Text = 0x1,
    /// Frame binario
    Binary = 0x2,
    /// Close
    Close = 0x8,
    /// Ping
    Ping = 0x9,
    /// Pong
    Pong = 0xA,
}

impl WsOpcode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x0 => Some(Self::Continuation),
            0x1 => Some(Self::Text),
            0x2 => Some(Self::Binary),
            0x8 => Some(Self::Close),
            0x9 => Some(Self::Ping),
            0xA => Some(Self::Pong),
            _ => None,
        }
    }
}

/// Lee un frame WebSocket del stream.
pub async fn read_frame(
    stream: &mut TcpStream,
    buf: &mut BytesMut,
) -> anyhow::Result<Option<(WsOpcode, Vec<u8>)>> {
    loop {
        if buf.len() < 2 {
            // Leer más bytes
            let mut tmp = [0u8; 4096];
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Ok(None);
            }
            buf.extend_from_slice(&tmp[..n]);
            continue;
        }

        let b1 = buf[0];
        let b2 = buf[1];

        let fin = (b1 & 0x80) != 0;
        let opcode = WsOpcode::from_u8(b1 & 0x0F)
            .ok_or_else(|| anyhow::anyhow!("opcode WebSocket desconocido: {}", b1 & 0x0F))?;
        let masked = (b2 & 0x80) != 0;
        let mut len = (b2 & 0x7F) as usize;

        let mut header_len = 2;
        if len == 126 {
            // Extended length (2 bytes)
            if buf.len() < 4 {
                let mut tmp = [0u8; 4096];
                let n = stream.read(&mut tmp).await?;
                if n == 0 {
                    return Ok(None);
                }
                buf.extend_from_slice(&tmp[..n]);
                continue;
            }
            len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
            header_len = 4;
        } else if len == 127 {
            // Extended length (8 bytes)
            if buf.len() < 10 {
                let mut tmp = [0u8; 4096];
                let n = stream.read(&mut tmp).await?;
                if n == 0 {
                    return Ok(None);
                }
                buf.extend_from_slice(&tmp[..n]);
                continue;
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&buf[2..10]);
            len = u64::from_be_bytes(bytes) as usize;
            header_len = 10;
        }

        let mask_len = if masked { 4 } else { 0 };
        let total_len = header_len + mask_len + len;

        if buf.len() < total_len {
            // Leer más
            let mut tmp = [0u8; 4096];
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Ok(None);
            }
            buf.extend_from_slice(&tmp[..n]);
            continue;
        }

        // Copiar payload
        let mut payload = buf[header_len + mask_len..total_len].to_vec();

        if masked {
            let mask = &buf[header_len..header_len + 4];
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }

        // Avanzar el buffer
        buf.advance(total_len);

        if !fin {
            // Fragmentado: por ahora no soportamos. Cerramos.
            return Err(anyhow::anyhow!("frames fragmentados no soportados"));
        }

        return Ok(Some((opcode, payload)));
    }
}

/// Escribe un frame de texto WebSocket. Genérico sobre cualquier tipo
/// que implemente `AsyncWriteExt`.
pub async fn write_text_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    text: &str,
) -> anyhow::Result<()> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    // IMPORTANTE (RFC 6455 §5.1): el servidor NO debe enmascarar los frames
    // que envía al cliente. El bit de máscara (0x80) va en 0 y no se escribe
    // mask key. (Los browsers cierran la conexión si el server enmascara.)
    let mut header = Vec::with_capacity(10);
    header.push(0x81); // FIN=1, opcode=text
    if len < 126 {
        header.push(len as u8);
    } else if len < 65536 {
        header.push(126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    writer.write_all(&header).await?;
    writer.write_all(bytes).await?;
    Ok(())
}

/// Escribe un close frame (sin máscara, como corresponde al server).
pub async fn write_close_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
) -> anyhow::Result<()> {
    let header = [0x88, 0x00];
    writer.write_all(&header).await?;
    Ok(())
}

/// Escribe un frame Pong (opcode 0xA) con el payload del Ping recibido
/// (RFC 6455 §5.5.3: el Pong DEBE llevar el mismo application data que el
/// Ping). Sin máscara (server→cliente). El payload de un frame de control
/// es ≤125 bytes, así que el header corto alcanza.
pub async fn write_pong_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> anyhow::Result<()> {
    let len = payload.len().min(125);
    let header = [0x8A, len as u8]; // FIN=1, opcode=pong
    writer.write_all(&header).await?;
    writer.write_all(&payload[..len]).await?;
    Ok(())
}

/// Escribe un frame Ping (opcode 0x9) sin payload, sin máscara
/// (server→cliente). Es el keepalive del server: un browser responde el Pong
/// automáticamente, así que su ausencia identifica una conexión muerta
/// (equivalente al FastPing del path Ares nativo).
pub async fn write_ping_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
) -> anyhow::Result<()> {
    let header = [0x89, 0x00]; // FIN=1, opcode=ping, sin payload
    writer.write_all(&header).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_accept_key_rfc_example() {
        // Ejemplo del RFC 6455 §1.3
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let accept = compute_accept_key(key);
        assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[tokio::test]
    async fn server_frames_are_not_masked() {
        // RFC 6455 §5.1: el server no debe enmascarar. El segundo byte (que
        // lleva el bit de máscara en 0x80) debe tener ese bit apagado.
        let mut buf: Vec<u8> = Vec::new();
        write_text_frame(&mut buf, "hola").await.unwrap();
        assert_eq!(buf[0], 0x81); // FIN + text
        assert_eq!(buf[1] & 0x80, 0, "el bit de máscara debe estar apagado");
        assert_eq!(buf[1] & 0x7F, 4, "len = 4");
        assert_eq!(&buf[2..], b"hola"); // sin mask key, payload directo

        // Frame largo (>125): usa extended length de 2 bytes, sin máscara.
        let mut buf2: Vec<u8> = Vec::new();
        let long = "x".repeat(200);
        write_text_frame(&mut buf2, &long).await.unwrap();
        assert_eq!(buf2[1] & 0x80, 0);
        assert_eq!(buf2[1] & 0x7F, 126);
        assert_eq!(u16::from_be_bytes([buf2[2], buf2[3]]), 200);
        assert_eq!(&buf2[4..], long.as_bytes());
    }

    #[tokio::test]
    async fn ping_frame_is_empty_and_unmasked() {
        // Keepalive del server: Ping vacío, sin máscara (si el server enmascara,
        // el browser cierra la conexión — justo lo contrario de lo que se busca).
        let mut buf: Vec<u8> = Vec::new();
        write_ping_frame(&mut buf).await.unwrap();
        assert_eq!(buf, vec![0x89, 0x00]);
    }

    #[test]
    fn parse_basic_request() {
        let raw = "GET /ws HTTP/1.1\r\nHost: example.com\r\nSec-WebSocket-Key: dGVzdA==\r\n\r\n";
        let req = parse_http_request(raw);
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/ws");
        assert_eq!(req.headers.len(), 2);
    }
}
