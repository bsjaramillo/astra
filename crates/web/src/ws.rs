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
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

use server_core::AppContext;

use crate::handler::handle_connection;

/// GUID mágica del estándar WebSocket (RFC 6455).
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// WebSocket server.
pub struct WsServer {
    ctx: Arc<AppContext>,
    port: u16,
}

/// Maneja una conexión TCP potencialmente WebSocket: handshake HTTP y luego frames WS.
pub async fn handle_stream(
    ctx: Arc<AppContext>,
    stream: TcpStream,
    peer: SocketAddr,
) -> anyhow::Result<()> {
    handle_ws_connection(ctx, stream, peer).await
}

impl WsServer {
    /// Crea un nuevo WebSocket server.
    pub fn new(ctx: Arc<AppContext>, port: u16) -> Self {
        Self { ctx, port }
    }

    /// Inicia el loop principal. Escucha en `0.0.0.0:port`.
    pub async fn serve(self) -> anyhow::Result<()> {
        let bind_addr: SocketAddr = format!("0.0.0.0:{}", self.port).parse()?;
        let listener = TcpListener::bind(bind_addr).await?;
        info!("WebSocket server escuchando en {} (clientes ib0t/HTML5)", bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let ctx = self.ctx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_ws_connection(ctx, stream, peer).await {
                            debug!("ws connection {} cerrada: {}", peer, e);
                        }
                    });
                }
                Err(e) => {
                    error!("ws accept error: {}", e);
                }
            }
        }
    }
}

/// Maneja una conexión WS entrante: hace el handshake y delega a `handle_connection`.
async fn handle_ws_connection(
    ctx: Arc<AppContext>,
    mut stream: TcpStream,
    peer: SocketAddr,
) -> anyhow::Result<()> {
    info!("nueva conexión WS desde {}", peer);

    // 1) Leer el handshake HTTP
    let request = read_http_request(&mut stream).await?;
    debug!("WS handshake request de {}: {} headers", peer, request.headers.len());

    // 2) Extraer la clave
    let key = request
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))
        .map(|(_, v)| v.clone());

    let key = match key {
        Some(k) => k,
        None => {
            warn!("WS handshake sin Sec-WebSocket-Key");
            send_http_error(&mut stream, 400, "Missing Sec-WebSocket-Key").await?;
            return Ok(());
        }
    };

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

    // 5) Pasar al modo WebSocket
    handle_connection(ctx, stream, peer).await
}

/// Lee un HTTP request completo (hasta \r\n\r\n).
async fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<HttpRequest> {
    let mut buf = vec![0u8; 8192];
    let mut total = 0;

    // Leer hasta encontrar \r\n\r\n
    loop {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            anyhow::bail!("conexión cerrada antes del handshake");
        }
        total += n;
        if let Some(idx) = find_double_crlf(&buf[..total]) {
            let raw = std::str::from_utf8(&buf[..idx])?;
            return Ok(parse_http_request(raw));
        }
        if total >= buf.len() {
            anyhow::bail!("HTTP request demasiado largo");
        }
    }
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
    #[allow(dead_code)]
    method: String,
    #[allow(dead_code)]
    path: String,
    headers: Vec<(String, String)>,
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
    }
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

    let mut header = Vec::with_capacity(10);
    header.push(0x81); // FIN=1, opcode=text
    if len < 126 {
        header.push(0x80 | len as u8);
    } else if len < 65536 {
        header.push(0x80 | 126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(0x80 | 127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    // Mask = 0x00000000 (no enmascarar)
    header.extend_from_slice(&[0, 0, 0, 0]);

    writer.write_all(&header).await?;
    writer.write_all(bytes).await?;
    Ok(())
}

/// Escribe un close frame. Genérico sobre cualquier `AsyncWriteExt`.
pub async fn write_close_frame<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
) -> anyhow::Result<()> {
    let header = [0x88, 0x80, 0, 0, 0, 0];
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

    #[test]
    fn parse_basic_request() {
        let raw = "GET /ws HTTP/1.1\r\nHost: example.com\r\nSec-WebSocket-Key: dGVzdA==\r\n\r\n";
        let req = parse_http_request(raw);
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/ws");
        assert_eq!(req.headers.len(), 2);
    }
}
