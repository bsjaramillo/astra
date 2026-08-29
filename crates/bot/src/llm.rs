//! Cliente LLM (OpenAI-compatible, DeepSeek y Anthropic) para el bot.
//!
//! Expuesto tras un trait ([`LlmClient`]) para poder mockearlo en tests.

use std::time::Duration;

use async_trait::async_trait;

use crate::config::{LlmConfig, LlmProvider};
use crate::memory::ChatMessage;

/// Cliente LLM async. Mockeable en tests.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Envía `messages` (sin el system prompt) y devuelve el texto de la
    /// respuesta. `Err` ante error de red, timeout, status HTTP o parseo.
    async fn chat(&self, cfg: &LlmConfig, messages: &[ChatMessage]) -> Result<String, String>;
}

/// Cliente HTTP real con `reqwest`.
pub struct HttpLlm;

impl HttpLlm {
    fn client() -> &'static reqwest::Client {
        static CLIENT: std::sync::LazyLock<reqwest::Client> =
            std::sync::LazyLock::new(reqwest::Client::new);
        &CLIENT
    }
}

#[async_trait]
impl LlmClient for HttpLlm {
    async fn chat(&self, cfg: &LlmConfig, messages: &[ChatMessage]) -> Result<String, String> {
        if cfg.endpoint.is_empty() {
            return Err("llm.endpoint vacío".into());
        }
        if cfg.model.is_empty() {
            return Err("llm.model vacío".into());
        }
        if cfg.api_key.is_empty() {
            return Err("llm.api_key requerida".into());
        }
        let timeout = Duration::from_secs(cfg.timeout_secs.max(1));

        // Reintento ante errores TRANSITORIOS (429 / 5xx / respuesta vacía /
        // "insufficient_system_resource"): DeepSeek y otros providers devuelven
        // estos fallos cuando están sobrecargados y suelen resolverse solos.
        const MAX_RETRIES: usize = 2;
        let mut last_err = String::new();
        for attempt in 0..=MAX_RETRIES {
            match self.chat_once(cfg, messages, timeout).await {
                Ok(text) => return Ok(text),
                Err(e) => {
                    if attempt < MAX_RETRIES && is_transient(&e) {
                        let backoff_ms = 300 * (1 << attempt);
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        last_err = e;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Err(last_err)
    }
}

impl HttpLlm {
    /// Un intento de llamada (sin reintento).
    async fn chat_once(
        &self,
        cfg: &LlmConfig,
        messages: &[ChatMessage],
        timeout: Duration,
    ) -> Result<String, String> {
        match cfg.provider {
            LlmProvider::Openai | LlmProvider::Deepseek => {
                openai_chat(cfg, messages, timeout).await
            }
            LlmProvider::Anthropic => anthropic_chat(cfg, messages, timeout).await,
        }
    }
}

/// ¿El error es transitorio (merece reintento)? 429, 5xx, respuesta vacía y
/// el "insufficient_system_resource" de DeepSeek. Los 4xx restantes
/// (400/401/402/422) son permanentes (config inválida, saldo, etc.) y los
/// errores de red/timeout no se reintentan para no alargar la espera.
fn is_transient(e: &str) -> bool {
    if e.contains("insufficient_system_resource") {
        return true;
    }
    if let Some(rest) = e.strip_prefix("llm http ") {
        if let Some(code) = rest
            .split(':')
            .next()
            .and_then(|c| c.trim().parse::<u16>().ok())
        {
            return code == 429 || (500..=599).contains(&code);
        }
        return false;
    }
    e == "respuesta LLM vacía"
}

async fn openai_chat(
    cfg: &LlmConfig,
    messages: &[ChatMessage],
    timeout: Duration,
) -> Result<String, String> {
    let mut payload_messages = Vec::with_capacity(messages.len() + 1);
    payload_messages.push(serde_json::json!({
        "role": "system",
        "content": cfg.system_prompt,
    }));
    for m in messages {
        payload_messages.push(serde_json::json!({
            "role": m.role,
            "content": m.content,
        }));
    }

    let mut req = HttpLlm::client()
        .post(&cfg.endpoint)
        .timeout(timeout)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({
            "model": cfg.model,
            "messages": payload_messages,
            "temperature": cfg.temperature,
            "max_tokens": cfg.max_tokens,
        }));
    if !cfg.api_key.is_empty() {
        req = req.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", cfg.api_key),
        );
    }

    let resp = req.send().await.map_err(|e| format!("llm request: {}", e))?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| format!("llm read: {}", e))?;
    if !status.is_success() {
        let snippet = String::from_utf8_lossy(&bytes).chars().take(200).collect::<String>();
        return Err(format!("llm http {}: {}", status, snippet));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("llm json: {}", e))?;
    let content = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("respuesta LLM inesperada: {}", truncate(&bytes)))?
        .trim()
        .to_string();
    let finish_reason = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str());
    // DeepSeek: cuando el servidor está sobrecargado responde con
    // finish_reason "insufficient_system_resource" (a veces con content vacío).
    // Es transitorio → se reintenta.
    if finish_reason == Some("insufficient_system_resource") {
        return Err("llm http 503: insufficient_system_resource (servidor sobrecargado)".into());
    }
    if content.is_empty() {
        return Err("respuesta LLM vacía".into());
    }
    // finish_reason == "length": el modelo se quedó sin tokens a mitad de la
    // respuesta. Marcarlo con "…" para que no parezca un corte silencioso.
    if finish_reason == Some("length") {
        Ok(format!("{} …", content))
    } else {
        Ok(content)
    }
}

async fn anthropic_chat(
    cfg: &LlmConfig,
    messages: &[ChatMessage],
    timeout: Duration,
) -> Result<String, String> {
    // Anthropic: los roles válidos son user/assistant (el system va aparte).
    let mut body_messages = Vec::with_capacity(messages.len());
    for m in messages {
        let role = match m.role.as_str() {
            "assistant" => "assistant",
            _ => "user",
        };
        body_messages.push(serde_json::json!({ "role": role, "content": m.content }));
    }

    let mut req = HttpLlm::client()
        .post(&cfg.endpoint)
        .timeout(timeout)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": cfg.model,
            "max_tokens": cfg.max_tokens,
            "system": cfg.system_prompt,
            "messages": body_messages,
        }));
    if !cfg.api_key.is_empty() {
        req = req.header("x-api-key", cfg.api_key.as_str());
    }

    let resp = req.send().await.map_err(|e| format!("llm request: {}", e))?;
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| format!("llm read: {}", e))?;
    if !status.is_success() {
        let snippet = String::from_utf8_lossy(&bytes).chars().take(200).collect::<String>();
        return Err(format!("llm http {}: {}", status, snippet));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("llm json: {}", e))?;
    let content = v
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter().find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        })
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| format!("respuesta LLM inesperada: {}", truncate(&bytes)))?
        .trim()
        .to_string();
    if content.is_empty() {
        Err("respuesta LLM vacía".into())
    } else {
        // stop_reason == "max_tokens": respuesta cortada por el límite.
        let truncated = v
            .get("stop_reason")
            .and_then(|r| r.as_str())
            == Some("max_tokens");
        if truncated {
            Ok(format!("{} …", content))
        } else {
            Ok(content)
        }
    }
}

fn truncate(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_endpoint() {
        let cfg = LlmConfig {
            endpoint: String::new(),
            api_key: "k".into(),
            ..LlmConfig::default()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(HttpLlm.chat(&cfg, &[])).unwrap_err();
        assert!(err.contains("endpoint"));
    }

    #[test]
    fn rejects_empty_api_key() {
        let cfg = LlmConfig {
            endpoint: "https://x".into(),
            api_key: String::new(),
            ..LlmConfig::default()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(HttpLlm.chat(&cfg, &[])).unwrap_err();
        assert!(err.contains("api_key"));
    }

    #[test]
    fn transient_error_classification() {
        // Transitorios: 429, 5xx, respuesta vacía, resource insuficiente.
        assert!(is_transient("llm http 429: too many requests"));
        assert!(is_transient("llm http 503: service unavailable"));
        assert!(is_transient("llm http 500: boom"));
        assert!(is_transient("respuesta LLM vacía"));
        assert!(is_transient("llm http 503: insufficient_system_resource (servidor sobrecargado)"));
        // Permanentes: no se reintentan.
        assert!(!is_transient("llm http 400: bad request"));
        assert!(!is_transient("llm http 401: invalid key"));
        assert!(!is_transient("llm http 402: insufficient balance"));
        assert!(!is_transient("llm http 422: invalid model"));
        assert!(!is_transient("llm request: timeout (red)"));
        assert!(!is_transient("llm json: parse error"));
    }
}