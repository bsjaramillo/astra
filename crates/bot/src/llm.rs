//! Cliente LLM para el bot, basado en Rig (`rig-core`).
//!
//! [`RigLlm`] implementa [`LlmClient`] usando los clientes de proveedores de
//! Rig (OpenAI, DeepSeek, Anthropic). El trait sigue expuesto para poder
//! mockearlo en tests.
//!
//! El cliente se construye por llamada desde la [`crate::config::LlmConfig`]
//! vigente: así los cambios de proveedor/key/modelo en el panel admin se
//! aplican en vivo, sin reiniciar.

use std::time::Duration;

use async_trait::async_trait;
use rig_core::client::completion::CompletionClient;
use rig_core::completion::{
    AssistantContent, CompletionError, CompletionModel, FinishReason, Message,
};
use rig_core::providers::{anthropic, deepseek, openai};

use crate::config::{LlmConfig, LlmProvider};
use crate::memory::ChatMessage;

/// Cliente LLM async. Mockeable en tests.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Envía `messages` (sin el system prompt) y devuelve el texto de la
    /// respuesta. `Err` ante error de red, timeout, status HTTP o parseo.
    async fn chat(&self, cfg: &LlmConfig, messages: &[ChatMessage]) -> Result<String, String>;
}

/// Cliente LLM real basado en Rig.
pub struct RigLlm;

/// Cliente Rig de un proveedor, ya construido con la config vigente.
///
/// Se arma por llamada (ver doc del módulo); cada variante guarda el cliente
/// concreto del proveedor (los tipos difieren, por eso el enum).
enum RigClient {
    Openai(openai::CompletionsClient),
    Deepseek(deepseek::Client),
    Anthropic(anthropic::Client),
}

impl RigClient {
    /// Construye el cliente del proveedor indicado por `cfg`.
    fn build(cfg: &LlmConfig) -> Result<Self, String> {
        match cfg.provider {
            LlmProvider::Openai => openai::Client::builder()
                .api_key(cfg.api_key.clone())
                .build()
                .map(|c| Self::Openai(c.completions_api()))
                .map_err(|e| format!("rig openai: {}", e)),
            LlmProvider::Deepseek => deepseek::Client::builder()
                .api_key(cfg.api_key.clone())
                .build()
                .map(Self::Deepseek)
                .map_err(|e| format!("rig deepseek: {}", e)),
            LlmProvider::Anthropic => anthropic::Client::builder()
                .api_key(cfg.api_key.clone())
                .build()
                .map(Self::Anthropic)
                .map_err(|e| format!("rig anthropic: {}", e)),
        }
    }

    /// Una llamada de completación con el cliente de este proveedor.
    async fn completion(
        &self,
        cfg: &LlmConfig,
        messages: &[ChatMessage],
    ) -> Result<String, String> {
        match self {
            Self::Openai(c) => run_model(c.completion_model(&cfg.model), cfg, messages).await,
            Self::Deepseek(c) => run_model(c.completion_model(&cfg.model), cfg, messages).await,
            Self::Anthropic(c) => run_model(c.completion_model(&cfg.model), cfg, messages).await,
        }
    }
}

/// Ejecuta una completación con un modelo concreto de Rig.
///
/// El system prompt va como `preamble` (Rig lo inyecta como primer mensaje de
/// sistema) y el último mensaje de `messages` es el prompt actual; el resto
/// del historial se envía tal cual.
async fn run_model<M>(model: M, cfg: &LlmConfig, messages: &[ChatMessage]) -> Result<String, String>
where
    M: CompletionModel + Clone,
{
    let mut history: Vec<Message> = messages
        .iter()
        .map(|m| match m.role.as_str() {
            "assistant" => Message::assistant(m.content.clone()),
            _ => Message::user(m.content.clone()),
        })
        .collect();
    let prompt = if history.is_empty() {
        // Sin historial (ej. el saludo): el system prompt ya dirige la
        // respuesta. Rig exige al menos un mensaje, así que usamos un marcador
        // neutro (los providers rechazan contenido vacío).
        Message::user(".")
    } else {
        history.pop().expect("history no vacía")
    };

    let resp = model
        .completion_request(prompt)
        .preamble(cfg.system_prompt.clone())
        .messages(history)
        .temperature(cfg.temperature)
        .max_tokens(cfg.max_tokens.max(1) as u64)
        .send()
        .await
        .map_err(|e| rig_error_str(&e))?;

    let content = extract_text(&resp.choice);

    // DeepSeek: cuando el servidor está sobrecargado responde con
    // finish_reason "insufficient_system_resource" (a veces con content
    // vacío). Es transitorio → se reintenta.
    if let Some(FinishReason::Other(r)) = resp.finish_reason() {
        if r == "insufficient_system_resource" {
            return Err("llm http 503: insufficient_system_resource (servidor sobrecargado)".into());
        }
    }
    if content.is_empty() {
        return Err("respuesta LLM vacía".into());
    }
    // finish_reason == "length": el modelo se quedó sin tokens a mitad de la
    // respuesta. Marcarlo con "…" para que no parezca un corte silencioso.
    if resp.finish_reason() == Some(FinishReason::Length) {
        Ok(format!("{} …", content))
    } else {
        Ok(content)
    }
}

/// Concatena los bloques de texto de la respuesta del modelo.
fn extract_text(choice: &[AssistantContent]) -> String {
    choice
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

/// Formatea un [`CompletionError`] de Rig en el formato que entiende el bot.
///
/// Si el error lleva status HTTP (429/5xx), lo preserva en el prefijo
/// `llm http {code}:` para que [`is_transient`] siga clasificando los errores
/// transitorios igual que con el cliente HTTP anterior.
fn rig_error_str(e: &CompletionError) -> String {
    if let Some(status) = e.provider_response_status() {
        let body = e
            .provider_response_body()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        return format!("llm http {}: {}", status.as_u16(), body);
    }
    format!("llm rig: {}", e)
}

#[async_trait]
impl LlmClient for RigLlm {
    async fn chat(&self, cfg: &LlmConfig, messages: &[ChatMessage]) -> Result<String, String> {
        if cfg.api_key.is_empty() {
            return Err("llm.api_key requerida".into());
        }
        if cfg.model.is_empty() {
            return Err("llm.model vacío".into());
        }
        let client = RigClient::build(cfg)?;
        let timeout = Duration::from_secs(cfg.timeout_secs.max(1));

        // Reintento ante errores TRANSITORIOS (429 / 5xx / respuesta vacía /
        // "insufficient_system_resource"): DeepSeek y otros providers devuelven
        // estos fallos cuando están sobrecargados y suelen resolverse solos.
        const MAX_RETRIES: usize = 2;
        let mut last_err = String::new();
        for attempt in 0..=MAX_RETRIES {
            // Timeout sobre toda la llamada: el cliente HTTP interno de Rig no
            // expone timeout por petición, así que se corta la espera aquí para
            // respetar `timeout_secs` (no se reintenta: no es transitorio).
            let outcome = match tokio::time::timeout(timeout, client.completion(cfg, messages)).await
            {
                Ok(r) => r,
                Err(_) => return Err("llm request: timeout".into()),
            };
            match outcome {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_api_key() {
        let cfg = LlmConfig {
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            ..LlmConfig::default()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(RigLlm.chat(&cfg, &[])).unwrap_err();
        assert!(err.contains("api_key"));
    }

    #[test]
    fn rejects_empty_model() {
        let cfg = LlmConfig {
            api_key: "k".into(),
            model: String::new(),
            ..LlmConfig::default()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(RigLlm.chat(&cfg, &[])).unwrap_err();
        assert!(err.contains("model"));
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

    #[test]
    fn extract_text_joins_text_blocks() {
        let choice = vec![
            AssistantContent::text("Hola "),
            AssistantContent::text("mundo"),
        ];
        assert_eq!(extract_text(&choice), "Hola mundo");
        assert_eq!(extract_text(&[]), "");
    }

    #[test]
    fn rig_error_str_preserves_status() {
        let err = CompletionError::HttpError(
            rig_core::http_client::Error::InvalidStatusCodeWithMessage(
                http::StatusCode::TOO_MANY_REQUESTS,
                "too many requests".into(),
            ),
        );
        let s = rig_error_str(&err);
        assert!(s.starts_with("llm http 429:"), "got: {}", s);
        assert!(is_transient(&s));
    }
}