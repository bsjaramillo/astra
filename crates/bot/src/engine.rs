//! Motor del bot agente: recibe los eventos de sala y conversa vía LLM.
//!
//! Implementa el trait [`server_core::bot::Bot`]. El binario lo construye y
//! lo cuelga en `AppContext.bot`; los hooks de TCP/web invocan
//! `on_join`/`on_public`/`on_private`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};

use server_core::app::AppContext;
use server_core::bot::Bot;
use server_core::db::Database;
use server_core::user_pool::UserPool;

use crate::config::{BotConfig, TriggerMode};
use crate::llm::{HttpLlm, LlmClient};
use crate::memory::ConversationMemory;

/// Motor del bot.
pub struct BotEngine {
    db: Arc<Database>,
    config: Arc<RwLock<BotConfig>>,
    memory: Arc<ConversationMemory>,
    llm: Arc<dyn LlmClient>,
    /// Última vez que el bot respondió a cada usuario (cooldown).
    cooldown: Arc<Mutex<HashMap<String, Instant>>>,
    /// Usuarios con una llamada al LLM en curso.
    in_flight: Arc<Mutex<HashMap<String, ()>>>,
    /// Total de llamadas al LLM en curso (tope global).
    in_flight_count: Arc<AtomicUsize>,
}

impl BotEngine {
    /// Crea el motor cargando la config desde la DB.
    pub fn new(db: Arc<Database>) -> Arc<Self> {
        Self::with_llm(db, Arc::new(HttpLlm))
    }

    /// Como [`new`](Self::new) pero con un cliente LLM propio (tests).
    pub fn with_llm(db: Arc<Database>, llm: Arc<dyn LlmClient>) -> Arc<Self> {
        let config = Arc::new(RwLock::new(BotConfig::load(&db)));
        Arc::new(Self {
            db,
            config,
            memory: Arc::new(ConversationMemory::new()),
            llm,
            cooldown: Arc::new(Mutex::new(HashMap::new())),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            in_flight_count: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Snapshot de la config actual.
    pub fn config_snapshot(&self) -> BotConfig {
        self.config.read().clone()
    }

    /// ¿Bot activo?
    pub fn is_enabled(&self) -> bool {
        self.config.read().enabled
    }

    /// Nombre actual del bot (para la userlist fantasma).
    pub fn bot_name(&self) -> String {
        self.config.read().name.clone()
    }

    /// Reemplaza y persiste la config (aplica en vivo).
    pub fn set_config(&self, cfg: BotConfig) -> Result<(), String> {
        cfg.save(&self.db)?;
        *self.config.write() = cfg;
        Ok(())
    }

    /// Saludo de bienvenida en background: generado por el LLM si
    /// `greet_llm` está activo (con la pista de cómo invocar al bot según el
    /// trigger); si no (o si el LLM falla), usa el template `greet_message` +
    /// la misma pista.
    fn spawn_greet(&self, ctx: &AppContext, name: &str) {
        let cfg = self.config.read().clone();
        let pool = ctx.user_pool.clone();
        let llm = self.llm.clone();
        let config = self.config.clone();
        let name = name.to_string();
        let room = ctx.settings.room_name.clone();
        let bot_name = cfg.name.clone();
        let use_llm = cfg.greet_llm;
        let fallback = cfg.greet_message.clone();
        let greet_as_pm = cfg.greet_as_pm;
        let hint = trigger_hint(&cfg);

        tokio::spawn(async move {
            let cfg = config.read().clone();
            if !cfg.enabled {
                return;
            }
            let text = if use_llm {
                let mut llm_cfg = cfg.llm.clone();
                llm_cfg.system_prompt = format!(
                    "Eres {}, el bot de la sala '{}'.\n\
                     Un usuario llamado '{}' acaba de entrar.\n\
                     Salúdalo con 1 o 2 frases cálidas y dile cómo invocarte: {}.\n\
                     Responde en español, breve y natural.",
                    bot_name, room, name, hint
                );
                match llm.chat(&llm_cfg, &[]).await {
                    Ok(r) => normalize_reply(&r),
                    Err(e) => {
                        tracing::warn!("bot: error LLM en saludo para '{}': {}", name, e);
                        format!("{} {}", render_greet(&fallback, &name, &room), hint)
                    }
                }
            } else {
                let t = render_greet(&fallback, &name, &room);
                if t.is_empty() {
                    return;
                }
                format!("{} {}", t, hint)
            };
            if text.is_empty() {
                return;
            }
            if greet_as_pm {
                if let Some(u) = pool.get_by_name(&name) {
                    for chunk in split_chunks(&text, MAX_MSG_LEN) {
                        let _ = u.send_pvt(&bot_name, &chunk);
                    }
                }
            } else {
                for chunk in split_chunks(&text, MAX_MSG_LEN) {
                    broadcast_public(&pool, &bot_name, &chunk);
                }
            }
        });
    }

    /// Lanza una respuesta (público o PM) en background, respetando cooldown,
    /// in-flight y tope global.
    fn spawn_reply(&self, ctx: &AppContext, from: &str, text: &str, is_pm: bool) {
        let cfg = self.config.read().clone();

        // Cooldown por usuario.
        {
            let mut cd = self.cooldown.lock();
            if let Some(last) = cd.get(from) {
                if last.elapsed() < Duration::from_secs(cfg.cooldown_secs.max(1)) {
                    return;
                }
            }
            cd.insert(from.to_string(), Instant::now());
        }
        // In-flight por usuario + tope global.
        {
            let mut inf = self.in_flight.lock();
            if inf.contains_key(from)
                || self.in_flight_count.load(Ordering::Relaxed) >= cfg.max_in_flight.max(1)
            {
                return;
            }
            inf.insert(from.to_string(), ());
            self.in_flight_count.fetch_add(1, Ordering::Relaxed);
        }

        let pool = ctx.user_pool.clone();
        let room_name = ctx.settings.room_name.clone();
        let topic = ctx.current_room_topic();
        let server_bot_name = ctx.settings.bot_name.clone();
        let from = from.to_string();
        let text = text.to_string();
        let memory = self.memory.clone();
        let llm = self.llm.clone();
        let config = self.config.clone();
        let cooldown = self.cooldown.clone();
        let in_flight = self.in_flight.clone();
        let in_flight_count = self.in_flight_count.clone();

        tokio::spawn(async move {
            let cfg = config.read().clone();
            // Re-chequear enable: la config pudo cambiar mientras se lanzaba.
            if !cfg.enabled {
                release(&in_flight, &in_flight_count, &cooldown, &from);
                return;
            }

            if cfg.conversation_memory {
                memory.push(&from, "user", &text, cfg.memory_turns);
            }
            let history = if cfg.conversation_memory {
                memory.history(&from, cfg.memory_turns).unwrap_or_default()
            } else {
                Vec::new()
            };

            // Prompt enriquecido con el contexto de la sala (usuarios, topic).
            let mut llm_cfg = cfg.llm.clone();
            llm_cfg.system_prompt =
                build_system_prompt(&cfg, &pool, &room_name, &topic, &server_bot_name);

            let reply = match llm.chat(&llm_cfg, &history).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("bot: error LLM para '{}': {}", from, e);
                    cfg.fallback_response.clone()
                }
            };
            let reply = normalize_reply(&reply);

            if !reply.is_empty() {
                if is_pm {
                    if let Some(u) = pool.get_by_name(&from) {
                        for chunk in split_chunks(&reply, MAX_MSG_LEN) {
                            let _ = u.send_pvt(&cfg.name, &chunk);
                        }
                    }
                } else {
                    for chunk in split_chunks(&reply, MAX_MSG_LEN) {
                        broadcast_public(&pool, &cfg.name, &chunk);
                    }
                }
                if cfg.conversation_memory {
                    memory.push(&from, "assistant", &reply, cfg.memory_turns);
                }
            }

            release(&in_flight, &in_flight_count, &cooldown, &from);
        });
    }
}

/// Máximo de caracteres por mensaje de chat (paridad con el corte de sb0t/
/// Astra en `truncate_message`). Respuestas más largas se dividen en varias.
const MAX_MSG_LEN: usize = 300;

/// Divide `text` en trozos de hasta `max` caracteres, cortando de preferencia
/// en el último espacio para no partir palabras. Devuelve un solo trozo si el
/// texto ya cabe.
fn split_chunks(text: &str, max: usize) -> Vec<String> {
    if max == 0 || text.is_empty() {
        return vec![text.to_string()];
    }

    if text.chars().count() <= max {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        // Byte index del corte: los primeros `max` chars (o el final).
        let limit = rest
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        let window = &rest[..limit];
        // Cortar en el último espacio dentro de la ventana (evita partir palabras).
        let cut = window
            .rfind(' ')
            .filter(|&i| i > 0)
            .map(|i| i + 1)
            .unwrap_or(limit);
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    out
}

fn normalize_reply(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn release(
    in_flight: &Mutex<HashMap<String, ()>>,
    in_flight_count: &AtomicUsize,
    cooldown: &Mutex<HashMap<String, Instant>>,
    from: &str,
) {
    in_flight.lock().remove(from);
    in_flight_count.fetch_sub(1, Ordering::Relaxed);
    cooldown.lock().insert(from.to_string(), Instant::now());
}

impl Bot for BotEngine {
    fn on_join(&self, ctx: &AppContext, name: &str) {
        let cfg = self.config.read().clone();
        if !cfg.enabled || !cfg.greet_on_join {
            return;
        }
        if name.is_empty() || name == cfg.name || name == ctx.settings.bot_name {
            return;
        }
        self.spawn_greet(ctx, name);
    }

    fn on_public(&self, ctx: &AppContext, from: &str, text: &str) {
        let cfg = self.config.read().clone();
        if !cfg.enabled || !cfg.reply_in_room {
            return;
        }
        if from.is_empty() || text.is_empty() || from == cfg.name || from == ctx.settings.bot_name {
            return;
        }
        if !trigger_matches(&cfg, text) {
            return;
        }
        self.spawn_reply(ctx, from, text, false);
    }

    fn on_private(&self, ctx: &AppContext, from: &str, text: &str) {
        let cfg = self.config.read().clone();
        if !cfg.enabled || !cfg.reply_by_pm {
            return;
        }
        if from.is_empty() || text.is_empty() || from == cfg.name || from == ctx.settings.bot_name {
            return;
        }
        self.spawn_reply(ctx, from, text, true);
    }

    fn is_enabled(&self) -> bool {
        self.config.read().enabled
    }

    fn bot_name(&self) -> String {
        self.config.read().name.clone()
    }

    fn config_json(&self) -> String {
        serde_json::to_string(&self.config.read().clone()).unwrap_or_default()
    }

    fn set_config_json(&self, json: &str) -> Result<(), String> {
        let cfg: BotConfig = serde_json::from_str(json).map_err(|e| format!("json: {}", e))?;
        self.set_config(cfg)
    }
}

/// ¿El mensaje dispara una respuesta del bot?
fn trigger_matches(cfg: &BotConfig, text: &str) -> bool {
    match cfg.trigger {
        TriggerMode::Always => true,
        TriggerMode::Prefix => text.trim_start().starts_with(&cfg.trigger_prefix),
        TriggerMode::Contains => {
            let name = cfg.name_lower();
            !name.is_empty() && text.to_lowercase().contains(&name)
        }
    }
}

/// Cómo invoca el usuario al bot, según el trigger configurado (para el
/// saludo y para que el bot lo explique).
fn trigger_hint(cfg: &BotConfig) -> String {
    match cfg.trigger {
        TriggerMode::Contains => format!("mencionando mi nombre ('{}')", cfg.name),
        TriggerMode::Prefix => format!("escribiendo '{}' al inicio de tu mensaje", cfg.trigger_prefix),
        TriggerMode::Always => "escribiéndome cualquier mensaje".to_string(),
    }
}

/// Lista de usuarios conectados (hasta 30) para el contexto del prompt.
fn room_context(
    pool: &UserPool,
    cfg: &BotConfig,
    room_name: &str,
    topic: &str,
    server_bot_name: &str,
) -> String {
    let names: Vec<String> = pool
        .users()
        .iter()
        .filter(|u| u.logged_in)
        .map(|u| u.name.read().clone())
        .filter(|n| n != &cfg.name && n != server_bot_name)
        .take(30)
        .collect();
    format!(
        "Sala: '{}' | Topic: '{}' | Usuarios conectados ({}): {}",
        room_name,
        topic,
        pool.len(),
        if names.is_empty() {
            "ninguno".to_string()
        } else {
            names.join(", ")
        }
    )
}

/// Comandos de la sala que el bot conoce (para responder dudas del usuario).
/// La EJECUCIÓN de comandos por el bot está planificada (ver docs/ROADMAP-V2).
fn commands_context() -> String {
    "Comandos de la sala (para informar al usuario): /kick <nick>, \
     /muzzle <nick>, /unmuzzle <nick>, /ban <nick> [razón], /topic <texto>, \
     /status <texto>, /help"
        .to_string()
}

/// Prompt de sistema con el contexto de la sala inyectado (point 8: el bot
/// sabe quién está conectado y qué comandos existen).
fn build_system_prompt(
    cfg: &BotConfig,
    pool: &UserPool,
    room_name: &str,
    topic: &str,
    server_bot_name: &str,
) -> String {
    let base = cfg.llm.system_prompt.trim();
    format!(
        "{}\n\n=== Contexto actual de la sala ===\n{}\n{}",
        base,
        room_context(pool, cfg, room_name, topic, server_bot_name),
        commands_context()
    )
}

/// Sustituye los placeholders del saludo (`+n` → nick, `+rn` → sala).
fn render_greet(template: &str, name: &str, room_name: &str) -> String {
    template
        .replace("+n", name)
        .replace("+rn", room_name)
        .trim()
        .to_string()
}

/// Difunde un mensaje público como `from` a toda la sala (nativos + web).
pub(crate) fn broadcast_public(pool: &UserPool, from: &str, text: &str) {
    for u in pool.users() {
        if u.logged_in {
            let _ = u.send_public(from, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LlmConfig, LlmProvider};
    use crate::memory::ChatMessage;
    use async_trait::async_trait;
    use server_core::settings::Settings;
    use std::net::{IpAddr, Ipv4Addr};

    struct MockLlm {
        reply: String,
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn chat(
            &self,
            _cfg: &LlmConfig,
            _messages: &[ChatMessage],
        ) -> Result<String, String> {
            Ok(self.reply.clone())
        }
    }

    fn engine(db: Arc<Database>, reply: &str) -> Arc<BotEngine> {
        let e = BotEngine::with_llm(
            db,
            Arc::new(MockLlm {
                reply: reply.into(),
            }),
        );
        let mut cfg = BotConfig::default();
        cfg.enabled = true;
        cfg.name = "Nova".into();
        cfg.cooldown_secs = 0;
        // El helper usa el saludo estático (no-LLM) para que los tests de
        // `on_join` sean deterministas; el path LLM se cubre aparte.
        cfg.greet_llm = false;
        cfg.llm = LlmConfig {
            provider: LlmProvider::Openai,
            ..LlmConfig::default()
        };
        *e.config.write() = cfg;
        e
    }

    fn ctx_with_user(
        name: &str,
    ) -> (
        Arc<AppContext>,
        tokio::sync::mpsc::UnboundedReceiver<String>,
    ) {
        let settings = Settings::default();
        let db = Database::in_memory().unwrap();
        let ctx = Arc::new(AppContext::new(settings, db));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut u =
            server_core::user_pool::AresUser::new(1, IpAddr::V4(Ipv4Addr::LOCALHOST), [0u8; 16]);
        *u.name.write() = name.to_string();
        u.ws_text_sender = Some(tx);
        u.logged_in = true;
        ctx.user_pool.add(Arc::new(u));
        (ctx, rx)
    }

    #[test]
    fn trigger_modes() {
        let mut cfg = BotConfig::default();
        cfg.name = "Nova".into();
        cfg.trigger = TriggerMode::Contains;
        assert!(trigger_matches(&cfg, "hola Nova"));
        assert!(trigger_matches(&cfg, "NOVA!!"));
        assert!(!trigger_matches(&cfg, "hola mundo"));

        cfg.trigger = TriggerMode::Prefix;
        cfg.trigger_prefix = "!".into();
        assert!(trigger_matches(&cfg, "!ping"));
        assert!(!trigger_matches(&cfg, "ping"));

        cfg.trigger = TriggerMode::Always;
        assert!(trigger_matches(&cfg, "cualquier cosa"));
    }

    #[test]
    fn render_greet_placeholders() {
        assert_eq!(
            render_greet("hola +n en +rn", "Ana", "Mi Sala"),
            "hola Ana en Mi Sala"
        );
        assert_eq!(render_greet("solo hola", "Ana", "Mi Sala"), "solo hola");
    }

    #[test]
    fn split_chunks_short_text() {
        assert_eq!(split_chunks("hola", 300), vec!["hola".to_string()]);
        assert_eq!(split_chunks("", 300), vec![String::new()]);
    }

    #[test]
    fn split_chunks_respects_max() {
        let text = "uno dos tres cuatro cinco seis siete ocho nueve diez";
        let chunks = split_chunks(text, 10);
        assert!(chunks.len() > 1, "debería dividirse");
        for c in &chunks {
            assert!(
                c.chars().count() <= 10,
                "chunk de {} > 10: '{}'",
                c.chars().count(),
                c
            );
        }
        // Se conserva el contenido completo.
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn split_chunks_breaks_on_words() {
        let text = "palabra_a palabra_b palabra_c palabra_d";
        let chunks = split_chunks(text, 15);
        assert!(
            chunks.iter().all(|c| !c.starts_with(' ')),
            "ningún chunk debe empezar con espacio"
        );
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn split_chunks_hard_cuts_long_word() {
        // Sin espacios: corta duro en el límite.
        let text = "x".repeat(100);
        let chunks = split_chunks(&text, 30);
        assert_eq!(chunks.len(), 4);
        assert!(chunks.iter().all(|c| c.chars().count() <= 30));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn normalize_reply_flattens_whitespace() {
        assert_eq!(
            normalize_reply("  primera línea\n\n segunda\t línea  "),
            "primera línea segunda línea"
        );
    }

    #[tokio::test]
    async fn on_join_sends_pm_greet() {
        let (ctx, mut rx) = ctx_with_user("alice");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "hola");
        bot.on_join(&ctx, "alice");
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout esperando saludo")
            .unwrap();
        assert!(msg.starts_with("PM:"), "esperaba PM, got {:?}", msg);
        assert!(msg.contains("alice"));
    }

    #[tokio::test]
    async fn on_join_llm_greeting_uses_llm() {
        let (ctx, mut rx) = ctx_with_user("dana");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "¡Bienvenida dana! Menciona 'Nova' para hablarme.");
        // Activar el saludo LLM: usa la respuesta del mock (con la pista).
        let mut cfg = bot.config_snapshot();
        cfg.greet_llm = true;
        *bot.config.write() = cfg;
        bot.on_join(&ctx, "dana");
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout esperando saludo LLM")
            .unwrap();
        assert!(msg.starts_with("PM:"), "esperaba PM, got {:?}", msg);
        assert!(msg.contains("Bienvenida dana"));
    }

    #[test]
    fn trigger_hint_modes() {
        let mut cfg = BotConfig::default();
        cfg.name = "Nova".into();
        cfg.trigger = TriggerMode::Contains;
        assert!(trigger_hint(&cfg).contains("Nova"));
        cfg.trigger = TriggerMode::Prefix;
        cfg.trigger_prefix = "!".into();
        assert!(trigger_hint(&cfg).contains("'!'"));
        cfg.trigger = TriggerMode::Always;
        assert!(trigger_hint(&cfg).contains("cualquier mensaje"));
    }

    #[test]
    fn build_system_prompt_includes_room_context() {
        let (ctx, _rx) = ctx_with_user("erin");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "hola");
        let cfg = bot.config_snapshot();
        let prompt = build_system_prompt(
            &cfg,
            &ctx.user_pool,
            "Mi Sala",
            "topic de prueba",
            "Astra",
        );
        assert!(prompt.contains("Mi Sala"));
        assert!(prompt.contains("topic de prueba"));
        assert!(prompt.contains("erin"), "debe listar a los usuarios conectados");
        assert!(prompt.contains("/kick"), "debe listar los comandos");
    }

    #[test]
    fn ignores_self_and_server_bot() {
        let (ctx, _rx) = ctx_with_user("Nova");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "hola");
        // El bot no responde a mensajes con su propio nombre.
        let mut cfg = bot.config_snapshot();
        cfg.reply_in_room = true;
        cfg.reply_by_pm = true;
        *bot.config.write() = cfg;
        // Sin panic y sin envío (no hay user del server bot tampoco).
        bot.on_public(&ctx, "Nova", "hola Nova");
        bot.on_private(&ctx, "Nova", "hola");
    }

    #[tokio::test]
    async fn on_public_replies_with_llm_output() {
        let (ctx, mut rx) = ctx_with_user("bob");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "¡hola bob!");
        bot.on_public(&ctx, "bob", "qué tal Nova?");
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout esperando respuesta")
            .unwrap();
        assert!(msg.starts_with("PUBLIC:"), "esperaba PUBLIC, got {:?}", msg);
        assert!(msg.contains("¡hola bob!"));
    }

    #[tokio::test]
    async fn on_private_replies_by_pm() {
        let (ctx, mut rx) = ctx_with_user("carol");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "respuesta privada");
        bot.on_private(&ctx, "carol", "me escribes?");
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout esperando PM")
            .unwrap();
        assert!(msg.starts_with("PM:"));
        assert!(msg.contains("respuesta privada"));
    }

    #[tokio::test]
    async fn cooldown_drops_rapid_retrigger() {
        let (ctx, mut rx) = ctx_with_user("dave");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "respuesta");
        bot.on_public(&ctx, "dave", "hola Nova");
        bot.on_public(&ctx, "dave", "hola Nova de nuevo");
        // Solo debe llegar una respuesta (la 2ª la descarta el cooldown).
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timeout")
            .unwrap();
        assert!(first.starts_with("PUBLIC:"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
                .await
                .is_err(),
            "no debía haber una segunda respuesta inmediata"
        );
    }
}
