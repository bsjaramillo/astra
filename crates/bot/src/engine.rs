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

            let reply = match llm.chat(&cfg.llm, &history).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("bot: error LLM para '{}': {}", from, e);
                    cfg.fallback_response.clone()
                }
            };

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
        let text = render_greet(&cfg.greet_message, name, &ctx.settings.room_name);
        if text.is_empty() {
            return;
        }
        if cfg.greet_as_pm {
            if let Some(u) = ctx.user_pool.get_by_name(name) {
                for chunk in split_chunks(&text, MAX_MSG_LEN) {
                    let _ = u.send_pvt(&cfg.name, &chunk);
                }
            }
        } else {
            for chunk in split_chunks(&text, MAX_MSG_LEN) {
                broadcast_public(&ctx.user_pool, &cfg.name, &chunk);
            }
        }
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
        let e = BotEngine::with_llm(db, Arc::new(MockLlm { reply: reply.into() }));
        let mut cfg = BotConfig::default();
        cfg.enabled = true;
        cfg.name = "Nova".into();
        cfg.cooldown_secs = 0;
        cfg.llm = LlmConfig {
            provider: LlmProvider::Openai,
            ..LlmConfig::default()
        };
        *e.config.write() = cfg;
        e
    }

    fn ctx_with_user(name: &str) -> (Arc<AppContext>, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let settings = Settings::default();
        let db = Database::in_memory().unwrap();
        let ctx = Arc::new(AppContext::new(settings, db));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut u = server_core::user_pool::AresUser::new(
            1,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            [0u8; 16],
        );
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
        assert_eq!(render_greet("hola +n en +rn", "Ana", "Mi Sala"), "hola Ana en Mi Sala");
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
            assert!(c.chars().count() <= 10, "chunk de {} > 10: '{}'", c.chars().count(), c);
        }
        // Se conserva el contenido completo.
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn split_chunks_breaks_on_words() {
        let text = "palabra_a palabra_b palabra_c palabra_d";
        let chunks = split_chunks(text, 15);
        assert!(chunks.iter().all(|c| !c.starts_with(' ')), "ningún chunk debe empezar con espacio");
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
    fn on_join_sends_pm_greet() {
        let (ctx, mut rx) = ctx_with_user("alice");
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "hola");
        bot.on_join(&ctx, "alice");
        let msg = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { rx.recv().await.unwrap() });
        assert!(msg.starts_with("PM:"), "esperaba PM, got {:?}", msg);
        assert!(msg.contains("alice"));
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