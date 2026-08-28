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
        let timeout = Duration::from_secs(cfg.timeout_secs.max(1));
        match cfg.provider {
            LlmProvider::Openai | LlmProvider::Deepseek => {
                openai_chat(cfg, messages, timeout).await
            }
            LlmProvider::Anthropic => anthropic_chat(cfg, messages, timeout).await,
        }
    }
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
    if content.is_empty() {
        Err("respuesta LLM vacía".into())
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
        Ok(content)
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
            ..LlmConfig::default()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(HttpLlm.chat(&cfg, &[])).unwrap_err();
        assert!(err.contains("endpoint"));
    }
}