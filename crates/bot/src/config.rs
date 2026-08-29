//! Configuración del bot agente inteligente.
//!
//! Persistida como JSON en la tabla `kv` de la DB SQLite (clave
//! [`BOT_CONFIG_KV_KEY`]). Cargada al arrancar y editable en vivo desde el
//! panel admin (`/admin/bot`).

use serde::{Deserialize, Serialize};

use server_core::db::Database;

/// Clave en la tabla `kv` donde vive la config del bot.
pub const BOT_CONFIG_KV_KEY: &str = "bot.config";

/// Modo de disparo para responder en público.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerMode {
    /// Responder si el mensaje contiene el nombre del bot (case-insensitive).
    #[default]
    Contains,
    /// Responder si el mensaje empieza con [`BotConfig::trigger_prefix`].
    Prefix,
    /// Responder a todo mensaje público.
    Always,
}

/// Proveedor de LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    /// API OpenAI-compatible (`POST {endpoint}` con `Authorization: Bearer`).
    /// Cubre OpenAI, Ollama, Groq, LM Studio, vLLM, Mistral, etc.
    #[default]
    Openai,
    /// API de DeepSeek, compatible con el formato de OpenAI.
    Deepseek,
    /// API de Anthropic (`POST {endpoint}` con `x-api-key`).
    Anthropic,
}

/// Configuración del proveedor LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Proveedor.
    pub provider: LlmProvider,
    /// Endpoint COMPLETO de la llamada de chat.
    /// - openai: `https://api.openai.com/v1/chat/completions` (o el de
    ///   Ollama/Groq/DeepSeek/etc.)
    /// - anthropic: `https://api.anthropic.com/v1/messages`
    pub endpoint: String,
    /// API key. OBLIGATORIA (todos los providers la requieren).
    pub api_key: String,
    /// Modelo (ej. `gpt-4o-mini`, `claude-haiku-4-5`, `deepseek-v4-flash`).
    pub model: String,
    /// Temperatura (0-2).
    pub temperature: f64,
    /// Máximo de tokens de la respuesta.
    pub max_tokens: u32,
    /// Prompt de sistema que define la personalidad del bot.
    pub system_prompt: String,
    /// Timeout de la llamada en segundos.
    pub timeout_secs: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Openai,
            endpoint: "https://api.openai.com/v1/chat/completions".into(),
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            temperature: 0.7,
            max_tokens: 400,
            system_prompt: "Eres Nova, un asistente amable y cercano en una sala de chat.\
             Responde de forma breve y natural, en el idioma del usuario.".into(),
            timeout_secs: 30,
        }
    }
}

/// Configuración completa del bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BotConfig {
    /// ¿Bot activo?
    pub enabled: bool,
    /// Nombre del bot. Debe ser DISTINTO de `settings.bot_name` (el "bot" del
    /// servidor) para que sea una identidad propia.
    pub name: String,
    /// Saludar a quien entra a la sala.
    pub greet_on_join: bool,
    /// Saludo por PM (true) o en público (false).
    pub greet_as_pm: bool,
    /// Generar el saludo con el LLM (true). Si es `false` (o el LLM falla),
    /// se usa [`Self::greet_message`].
    pub greet_llm: bool,
    /// Mensaje de saludo (fallback / modo no-LLM). Placeholder `+n` = nick,
    /// `+rn` = nombre de sala.
    pub greet_message: String,
    /// Responder menciones en público.
    pub reply_in_room: bool,
    /// Responder PMs dirigidos al bot.
    pub reply_by_pm: bool,
    /// Modo de disparo de las respuestas en público.
    pub trigger: TriggerMode,
    /// Prefijo usado por [`TriggerMode::Prefix`].
    pub trigger_prefix: String,
    /// Recordar la conversación con cada usuario.
    pub conversation_memory: bool,
    /// Turns de historial por usuario que se envían al LLM.
    pub memory_turns: usize,
    /// Cuántos mensajes públicos recientes de la sala se inyectan al contexto
    /// del prompt (`0` = desactivado).
    pub recent_history_lines: usize,
    /// Permitir que el bot EJECUTE comandos de la sala cuando un usuario se
    /// lo pide. El comando se ejecuta con el nivel del usuario que lo pide
    /// (no con el del bot), así aplican las validaciones de permisos reales.
    /// Default OFF (vector de riesgo: el LLM puede malinterpretar una petición).
    pub execute_commands: bool,
    /// Allowlist de comandos que el bot puede ejecutar (sin `/`). Vacía =
    /// todos los que el nivel del solicitante permita.
    pub allowed_commands: Vec<String>,
    /// Segundos de enfriamiento por usuario (anti-spam de llamadas al LLM).
    pub cooldown_secs: u64,
    /// Máximo de llamadas al LLM en vuelo simultáneas.
    pub max_in_flight: usize,
    /// Configuración del LLM.
    pub llm: LlmConfig,
    /// Respuesta cuando el LLM falla o hace timeout.
    pub fallback_response: String,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            name: "Nova".into(),
            greet_on_join: true,
            greet_as_pm: true,
            greet_llm: true,
            greet_message: "¡Hola +n! Bienvenido a +rn. 🙂".into(),
            reply_in_room: true,
            reply_by_pm: true,
            trigger: TriggerMode::Contains,
            trigger_prefix: "!".into(),
            conversation_memory: true,
            memory_turns: 12,
            recent_history_lines: 15,
            execute_commands: false,
            allowed_commands: Vec::new(),
            cooldown_secs: 3,
            max_in_flight: 4,
            llm: LlmConfig::default(),
            fallback_response: "Hmm, ahora mismo no puedo responder. Inténtalo en un momento.".into(),
        }
    }
}

impl BotConfig {
    /// Carga la config desde la DB (o defaults si no existe).
    pub fn load(db: &Database) -> Self {
        match db.get_kv(BOT_CONFIG_KV_KEY) {
            Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
            _ => Self::default(),
        }
    }

    /// Persiste la config en la DB.
    pub fn save(&self, db: &Database) -> Result<(), String> {
        let raw = serde_json::to_string(self).map_err(|e| format!("serialize: {}", e))?;
        db.set_kv(BOT_CONFIG_KV_KEY, &raw).map_err(|e| format!("db: {}", e))
    }

    /// Nombre en minúsculas (para comparaciones de trigger).
    pub fn name_lower(&self) -> String {
        self.name.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = BotConfig::default();
        assert!(!c.enabled);
        assert!(!c.name.is_empty());
        assert!(c.memory_turns > 0);
        assert!(c.llm.timeout_secs > 0);
    }

    #[test]
    fn roundtrip_json() {
        let c = BotConfig {
            enabled: true,
            name: "Luna".into(),
            ..BotConfig::default()
        };
        let raw = serde_json::to_string(&c).unwrap();
        let back: BotConfig = serde_json::from_str(&raw).unwrap();
        assert!(back.enabled);
        assert_eq!(back.name, "Luna");
        assert_eq!(back.trigger, TriggerMode::Contains);
        assert_eq!(back.llm.provider, LlmProvider::Openai);
    }

    #[test]
    fn missing_fields_default() {
        let raw = r#"{"enabled":true,"name":"X"}"#;
        let c: BotConfig = serde_json::from_str(raw).unwrap();
        assert!(c.enabled);
        assert_eq!(c.trigger, TriggerMode::Contains);
        assert_eq!(c.llm.model, "gpt-4o-mini");
    }

    #[test]
    fn deepseek_provider_roundtrips() {
        let raw = r#"{"provider":"deepseek"}"#;
        let c: LlmConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(c.provider, LlmProvider::Deepseek);
        assert_eq!(serde_json::to_value(c.provider).unwrap(), "deepseek");
    }

    #[test]
    fn kv_roundtrip() {
        let db = Database::in_memory().unwrap();
        let mut c = BotConfig::default();
        c.enabled = true;
        c.name = "Zeta".into();
        c.save(&db).unwrap();
        let loaded = BotConfig::load(&db);
        assert!(loaded.enabled);
        assert_eq!(loaded.name, "Zeta");
    }
}