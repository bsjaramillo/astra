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

use server_core::app::{AppContext, HistoryEntry};
use server_core::bot::Bot;
use server_core::db::Database;
use server_core::text_effects::strip_colors;
use server_core::user_pool::{AresUser, UserPool};

use crate::config::{BotConfig, TriggerMode};
use crate::llm::{HttpLlm, LlmClient};
use crate::memory::ConversationMemory;

/// Motor del bot.
pub struct BotEngine {
    db: Arc<Database>,
    config: Arc<RwLock<BotConfig>>,
    memory: Arc<ConversationMemory>,
    llm: Arc<dyn LlmClient>,
    /// Handle al motor de scripting (para los side-effects de los comandos).
    scripting: astra_scripting::ScriptHandle,
    /// Última vez que el bot respondió a cada usuario (cooldown).
    cooldown: Arc<Mutex<HashMap<String, Instant>>>,
    /// Usuarios con una llamada al LLM en curso.
    in_flight: Arc<Mutex<HashMap<String, ()>>>,
    /// Total de llamadas al LLM en curso (tope global).
    in_flight_count: Arc<AtomicUsize>,
}

impl BotEngine {
    /// Crea el motor cargando la config desde la DB.
    pub fn new(db: Arc<Database>, scripting: astra_scripting::ScriptHandle) -> Arc<Self> {
        Self::with_llm(db, scripting, Arc::new(HttpLlm))
    }

    /// Como [`new`](Self::new) pero con un cliente LLM propio (tests).
    pub fn with_llm(
        db: Arc<Database>,
        scripting: astra_scripting::ScriptHandle,
        llm: Arc<dyn LlmClient>,
    ) -> Arc<Self> {
        let config = Arc::new(RwLock::new(BotConfig::load(&db)));
        Arc::new(Self {
            db,
            config,
            memory: Arc::new(ConversationMemory::new()),
            llm,
            scripting,
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
    fn spawn_greet(&self, ctx: &Arc<AppContext>, name: &str) {
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
    fn spawn_reply(&self, ctx: &Arc<AppContext>, from: &str, text: &str, is_pm: bool) {
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
        // Snapshot del historial público reciente (configurable; 0 = off).
        let recent = if cfg.recent_history_lines > 0 {
            ctx.recent_messages(cfg.recent_history_lines)
        } else {
            Vec::new()
        };
        let ctx = ctx.clone();
        let from = from.to_string();
        let text = text.to_string();
        let memory = self.memory.clone();
        let llm = self.llm.clone();
        let config = self.config.clone();
        let scripting = self.scripting.clone();
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

            // Prompt enriquecido con el contexto de la sala (usuarios, topic,
            // comandos) y el historial público reciente.
            let mut llm_cfg = cfg.llm.clone();
            llm_cfg.system_prompt =
                build_system_prompt(&cfg, &pool, &room_name, &topic, &server_bot_name, &recent);

            let reply = match llm.chat(&llm_cfg, &history).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("bot: error LLM para '{}': {}", from, e);
                    cfg.fallback_response.clone()
                }
            };
            let reply = normalize_reply(&reply);

            // Si la ejecución de comandos está habilitada, el LLM puede pedir
            // ejecutar uno con `[CMD] ... [/CMD]`. Se ejecuta CON EL NIVEL DEL
            // SOLICITANTE (no con el del bot), así aplican las validaciones de
            // permisos del comando (ejecutor vs objetivo).
            let (clean, mut cmds) = extract_commands(&reply);
            let mut final_text = clean;
            if cmds.is_empty() {
                // Fallback: si el LLM no usó la directiva pero respondió con
                // un comando directo ("/topic x"), tratarlo como tal.
                let t = final_text.trim_start().to_string();
                if t.starts_with('/') || t.starts_with('#') {
                    cmds.push(t);
                    final_text = String::new();
                }
            }
            if cfg.execute_commands {
                let mut extras: Vec<String> = Vec::new();
                for cmd in cmds {
                    extras.extend(execute_as_user(
                        &ctx,
                        &pool,
                        &from,
                        &cmd,
                        &cfg.allowed_commands,
                        &scripting,
                    ));
                }
                if !extras.is_empty() {
                    if !final_text.is_empty() {
                        final_text.push(' ');
                    }
                    final_text.push_str(&extras.join(" | "));
                }
            } else {
                // Ejecución deshabilitada: no se procesa ninguna directiva.
                final_text = reply.clone();
            }
            let final_text = final_text.trim().to_string();
            let final_text = if final_text.is_empty() {
                cfg.fallback_response.clone()
            } else {
                final_text
            };

            if !final_text.is_empty() {
                if is_pm {
                    if let Some(u) = pool.get_by_name(&from) {
                        for chunk in split_chunks(&final_text, MAX_MSG_LEN) {
                            let _ = u.send_pvt(&cfg.name, &chunk);
                        }
                    }
                } else {
                    for chunk in split_chunks(&final_text, MAX_MSG_LEN) {
                        broadcast_public(&pool, &cfg.name, &chunk);
                    }
                }
                if cfg.conversation_memory {
                    memory.push(&from, "assistant", &final_text, cfg.memory_turns);
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
    fn on_join(&self, ctx: &Arc<AppContext>, name: &str) {
        let cfg = self.config.read().clone();
        if !cfg.enabled || !cfg.greet_on_join {
            return;
        }
        if name.is_empty() || name == cfg.name || name == ctx.settings.bot_name {
            return;
        }
        self.spawn_greet(ctx, name);
    }

    fn on_public(&self, ctx: &Arc<AppContext>, from: &str, text: &str) {
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

    fn on_private(&self, ctx: &Arc<AppContext>, from: &str, text: &str) {
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

/// ¿El mensaje dispara una respuesta del bot? El texto se limpia de códigos
/// de color/formato antes de validar, para que un color (o un carácter
/// zero-width unicode) antes del prefijo no rompa el trigger.
fn trigger_matches(cfg: &BotConfig, text: &str) -> bool {
    let text = strip_colors(text);
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

/// Formatea el historial público reciente como líneas legibles para el LLM
/// (más recientes al final). Si no hay, lo dice explícitamente.
fn recent_history_str(entries: &[HistoryEntry]) -> String {
    if entries.is_empty() {
        return "sin mensajes recientes".to_string();
    }
    entries
        .iter()
        .map(|e| {
            if e.is_emote {
                format!("- * {} {} *", e.name, e.text)
            } else {
                format!("- {}: {}", e.name, e.text)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prompt de sistema con el contexto de la sala inyectado: el bot sabe quién
/// está conectado, qué comandos existen y lo último que se habló en público.
/// Si `execute_commands` está activo, se le indica cómo pedir ejecutar un
/// comando (`[CMD] ... [/CMD]`).
fn build_system_prompt(
    cfg: &BotConfig,
    pool: &UserPool,
    room_name: &str,
    topic: &str,
    server_bot_name: &str,
    recent: &[HistoryEntry],
) -> String {
    let base = cfg.llm.system_prompt.trim();
    let mut exec = String::new();
    if cfg.execute_commands {
        let scope = if cfg.allowed_commands.is_empty() {
            "cualquier comando que sus permisos permitan".to_string()
        } else {
            format!("solo: /{}", cfg.allowed_commands.join(", /"))
        };
        exec = format!(
            "\n\nPuedes EJECUTAR comandos de la sala si el usuario te lo pide ({}). \
             El comando se ejecutará con los PERMISOS del usuario que lo pide, no con los tuyos. \
             Para ejecutar uno, responde únicamente el comando entre [CMD] y [/CMD], \
             por ejemplo: [CMD]/topic Nuevo tema[/CMD].",
            scope
        );
    }
    format!(
        "{}\n\n=== Contexto actual de la sala ===\n{}\n{}\n\n=== Historial reciente de la sala ===\n{}{}",
        base,
        room_context(pool, cfg, room_name, topic, server_bot_name),
        commands_context(),
        recent_history_str(recent),
        exec,
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

/// Busca `needle` en `haystack` desde `from`, case-insensitive (ASCII).
/// Devuelve el byte offset, o `None`.
fn find_ci(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || from > h.len() {
        return None;
    }
    let mut i = from;
    while i + n.len() <= h.len() {
        if h[i..i + n.len()]
            .iter()
            .zip(n.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Extrae las directivas `[CMD] ... [/CMD]` de la respuesta del LLM (si la
/// ejecución de comandos está habilitada). Devuelve el texto limpio (sin las
/// directivas) y la lista de comandos propuestos.
fn extract_commands(reply: &str) -> (String, Vec<String>) {
    const START: &str = "[CMD]";
    const END: &str = "[/CMD]";
    let mut clean = String::new();
    let mut cmds: Vec<String> = Vec::new();
    let mut rest = reply;
    loop {
        let Some(i) = find_ci(rest, START, 0) else {
            clean.push_str(rest);
            break;
        };
        clean.push_str(&rest[..i]);
        let after = &rest[i + START.len()..];
        match find_ci(after, END, 0) {
            Some(j) => {
                let cmd = after[..j].trim();
                if !cmd.is_empty() {
                    cmds.push(cmd.to_string());
                }
                rest = &after[j + END.len()..];
            }
            None => {
                // Directiva sin cerrar: preservar el texto original tal cual.
                clean.push_str(START);
                clean.push_str(after);
                break;
            }
        }
    }
    (clean, cmds)
}

/// Ejecuta un comando de la sala CON LA IDENTIDAD DEL SOLICITANTE (su nombre,
/// nivel, GUID e IP), no con la del bot. Así las validaciones de nivel de cada
/// comando (ejecutor vs objetivo) se aplican igual que si lo corriera el
/// propio usuario: un Regular no puede banear a un Admin, etc. Devuelve las
/// líneas de respuesta capturadas (lo que habría visto el usuario).
fn execute_as_user(
    ctx: &AppContext,
    pool: &UserPool,
    requester: &str,
    command: &str,
    allowed: &[String],
    scripting: &astra_scripting::ScriptHandle,
) -> Vec<String> {
    let line = command.trim().trim_start_matches('/').trim();
    if line.is_empty() {
        return vec!["Comando vacío.".to_string()];
    }
    let (cmd, args) = match line.split_once(char::is_whitespace) {
        Some((c, a)) => (c, a.trim()),
        None => (line, ""),
    };

    // Allowlist: si está vacía, cualquier comando que el nivel permita.
    if !allowed.is_empty() && !allowed.iter().any(|a| a.eq_ignore_ascii_case(cmd)) {
        return vec![format!("No tengo permiso para ejecutar /{}.", cmd)];
    }

    let Some(requester_user) = pool.get_by_name(requester) else {
        return vec![format!("No encuentro a '{}'.", requester)];
    };

    // Sintético con la identidad del solicitante + canal de captura.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();
    let mut u = AresUser::new(requester_user.id, requester_user.external_ip, requester_user.guid);
    u.logged_in = true;
    *u.name.write() = requester_user.name.read().clone();
    *u.level.write() = *requester_user.level.read();
    u.sender = Some(tx);
    let u = Arc::new(u);

    let (handled, events) = astra_commands::dispatch_builtin(ctx, scripting, &u, cmd, args);
    for ev in events {
        scripting.dispatch(ev);
    }

    let mut out = Vec::new();
    while let Ok(pkt) = rx.try_recv() {
        if !pkt.is_empty() {
            let op = pkt[0];
            if op == proto_ares::TcpMsg::ServerNosuch as u8 {
                let mut r = proto_ares::PacketReader::new(&pkt[1..]);
                if let Ok(text) = r.read_string_nt() {
                    out.push(text);
                }
            } else if op == proto_ares::TcpMsg::Pmt as u8 {
                let mut r = proto_ares::PacketReader::new(&pkt[1..]);
                let _ = r.read_string_nt();
                if let Ok(text) = r.read_string_nt() {
                    out.push(text);
                }
            }
        }
    }
    if out.is_empty() {
        if !handled {
            out.push(format!("Comando desconocido: /{}", cmd));
        } else {
            out.push(format!("Listo (/{}).", cmd));
        }
    }
    out
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
            astra_scripting::ScriptHandle::dummy(),
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
        let prompt = build_system_prompt(&cfg, &ctx.user_pool, "Mi Sala", "topic de prueba", "Astra", &[]);
        assert!(prompt.contains("Mi Sala"));
        assert!(prompt.contains("topic de prueba"));
        assert!(prompt.contains("erin"), "debe listar a los usuarios conectados");
        assert!(prompt.contains("/kick"), "debe listar los comandos");
    }

    #[test]
    fn build_system_prompt_includes_recent_history() {
        let hist = vec![
            HistoryEntry {
                name: "alice".into(),
                text: "hola a todos".into(),
                is_emote: false,
                time_secs: 0,
            },
            HistoryEntry {
                name: "bob".into(),
                text: "baila".into(),
                is_emote: true,
                time_secs: 0,
            },
        ];
        let s = recent_history_str(&hist);
        assert!(s.contains("alice: hola a todos"), "got: {}", s);
        assert!(s.contains("* bob baila *"), "got: {}", s);

        let db = Database::in_memory().unwrap();
        let bot = engine(db, "hola");
        let cfg = bot.config_snapshot();
        let (ctx, _rx) = ctx_with_user("erin");
        let prompt =
            build_system_prompt(&cfg, &ctx.user_pool, "Mi Sala", "t", "Astra", &hist);
        assert!(prompt.contains("Historial reciente de la sala"));
        assert!(prompt.contains("alice: hola a todos"));
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

    #[test]
    fn trigger_contains_ignores_color_codes() {
        let mut cfg = BotConfig::default();
        cfg.name = "Nova".into();
        cfg.trigger = TriggerMode::Contains;
        assert!(trigger_matches(&cfg, "hola \x0302Nova"));
        assert!(trigger_matches(&cfg, "\u{00AD}Nova\u{00AD}"));
    }

    #[test]
    fn trigger_prefix_ignores_color_codes() {
        let mut cfg = BotConfig::default();
        cfg.name = "Nova".into();
        cfg.trigger = TriggerMode::Prefix;
        cfg.trigger_prefix = "!".into();
        // Color Ares antes del prefijo.
        assert!(trigger_matches(&cfg, "\x03!hola"));
        assert!(trigger_matches(&cfg, "\x0303!hola"));
        assert!(trigger_matches(&cfg, "\x02!hola"));
        // Soft-hyphen unicode y zero-width antes del prefijo.
        assert!(trigger_matches(&cfg, "\u{00AD}!hola"));
        assert!(trigger_matches(&cfg, "\u{200B}!hola"));
        assert!(trigger_matches(&cfg, "\u{FEFF}!hola"));
        // Sin prefijo no dispara.
        assert!(!trigger_matches(&cfg, "hola"));
    }

    #[test]
    fn strip_colors_keeps_text() {
        assert_eq!(strip_colors("hola \x0302mundo"), "hola mundo");
        assert_eq!(strip_colors("\x0501"), ""); // código de color completo
        assert_eq!(strip_colors("\x05ab"), "ab"); // \x05 sin dígitos: solo el marcador
        assert_eq!(strip_colors("!ping"), "!ping");
    }

    #[test]
    fn extract_commands_parses_directives() {
        let (clean, cmds) = extract_commands("Listo [CMD]/topic nuevo[/CMD] y ya");
        assert_eq!(clean, "Listo  y ya");
        assert_eq!(cmds, vec!["/topic nuevo"]);

        let (clean, cmds) = extract_commands("[cmd]/kick bob[/cmd]");
        assert_eq!(clean, "");
        assert_eq!(cmds, vec!["/kick bob"]);

        let (clean, cmds) = extract_commands("sin comandos");
        assert_eq!(clean, "sin comandos");
        assert!(cmds.is_empty());

        // Directiva sin cerrar: se deja como texto.
        let (clean, cmds) = extract_commands("hola [CMD]/topic");
        assert_eq!(clean, "hola [CMD]/topic");
        assert!(cmds.is_empty());
    }

    fn ctx_with_user_level(name: &str, level: server_core::ILevel) -> Arc<AppContext> {
        let (ctx, _rx) = ctx_with_user(name);
        if let Some(u) = ctx.user_pool.get_by_name(name) {
            *u.level.write() = level;
        }
        ctx
    }

    #[test]
    fn execute_as_user_uses_requester_level() {
        // Moderator+ puede /topic.
        let ctx = ctx_with_user_level("mod", server_core::ILevel::Moderator);
        let dummy = astra_scripting::ScriptHandle::dummy();
        let out = execute_as_user(&ctx, &ctx.user_pool, "mod", "/topic hola", &[], &dummy);
        assert!(
            out.iter().any(|l| l.contains("Topic updated.")),
            "out: {:?}",
            out
        );
        assert_eq!(ctx.current_room_topic(), "hola");
    }

    #[test]
    fn execute_as_user_rejects_low_level() {
        // Regular NO puede /topic → la validación del comando lo rechaza
        // usando el nivel del SOLICITANTE.
        let ctx = ctx_with_user_level("regular", server_core::ILevel::Regular);
        let dummy = astra_scripting::ScriptHandle::dummy();
        let out = execute_as_user(&ctx, &ctx.user_pool, "regular", "/topic x", &[], &dummy);
        assert!(
            out.iter().any(|l| l.contains("Access denied")),
            "out: {:?}",
            out
        );
    }

    #[test]
    fn execute_as_user_enforces_allowlist() {
        let ctx = ctx_with_user_level("owner", server_core::ILevel::Owner);
        let dummy = astra_scripting::ScriptHandle::dummy();
        // Allowlist sin "topic" → rechazado aunque el nivel permita.
        let out = execute_as_user(
            &ctx,
            &ctx.user_pool,
            "owner",
            "/topic x",
            &["ban".into()],
            &dummy,
        );
        assert!(
            out.iter().any(|l| l.contains("No tengo permiso")),
            "out: {:?}",
            out
        );
        // Allowlist vacía = cualquier comando permitido por nivel.
        let out = execute_as_user(&ctx, &ctx.user_pool, "owner", "/topic y", &[], &dummy);
        assert!(
            out.iter().any(|l| l.contains("Topic updated.")),
            "out: {:?}",
            out
        );
    }

    #[test]
    fn execute_as_user_unknown_command() {
        let ctx = ctx_with_user_level("owner", server_core::ILevel::Owner);
        let dummy = astra_scripting::ScriptHandle::dummy();
        let out = execute_as_user(&ctx, &ctx.user_pool, "owner", "/noexiste", &[], &dummy);
        assert!(
            out.iter().any(|l| l.contains("noexiste")),
            "out: {:?}",
            out
        );
    }

    #[tokio::test]
    async fn full_execute_flow_on_public() {
        // El flujo completo: on_public → LLM responde con [CMD] → se ejecuta
        // con el nivel del solicitante → el topic cambia.
        let (ctx, mut rx) = ctx_with_user("owner");
        if let Some(u) = ctx.user_pool.get_by_name("owner") {
            *u.level.write() = server_core::ILevel::Owner;
        }
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "[CMD]/topic hola mundo[/CMD]");
        let mut cfg = bot.config_snapshot();
        cfg.execute_commands = true;
        cfg.reply_in_room = true;
        *bot.config.write() = cfg;

        bot.on_public(&ctx, "owner", "Nova cambia el topic");

        // El /topic primero difunde el TOPIC a la sala; luego llega la
        // respuesta del bot con el resultado. Consumimos hasta encontrarla.
        let mut found = false;
        for _ in 0..5 {
            let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                .await
                .expect("timeout esperando respuesta")
                .unwrap();
            if msg.starts_with("PUBLIC:") && msg.contains("Topic updated.") {
                found = true;
                break;
            }
        }
        assert!(found, "no llegó la respuesta 'Topic updated.'");
        assert_eq!(ctx.current_room_topic(), "hola mundo");
    }

    #[tokio::test]
    async fn full_execute_flow_bare_command_fallback() {
        // El LLM responde con un comando directo (sin la directiva [CMD]):
        // el fallback lo ejecuta igual.
        let (ctx, _rx) = ctx_with_user("owner");
        if let Some(u) = ctx.user_pool.get_by_name("owner") {
            *u.level.write() = server_core::ILevel::Owner;
        }
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "/topic directo");
        let mut cfg = bot.config_snapshot();
        cfg.execute_commands = true;
        *bot.config.write() = cfg;

        bot.on_public(&ctx, "owner", "Nova cambia el topic");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(ctx.current_room_topic(), "directo");
    }

    #[tokio::test]
    async fn full_execute_flow_case_insensitive_name() {
        // El nick del solicitante con MAYÚSCULAS se resuelve igual aunque el
        // bot reciba el nombre en minúsculas (get_by_name es case-insensitive).
        let (ctx, _rx) = ctx_with_user("Owner");
        if let Some(u) = ctx.user_pool.get_by_name("Owner") {
            *u.level.write() = server_core::ILevel::Owner;
        }
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "[CMD]/status hola[/CMD]");
        let mut cfg = bot.config_snapshot();
        cfg.execute_commands = true;
        *bot.config.write() = cfg;
        // "owner" (minúsculas) vs nick "Owner".
        bot.on_public(&ctx, "owner", "Nova cambia el status");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(ctx.room_status(), "hola");
    }

    #[tokio::test]
    async fn full_execute_flow_colored_nick() {
        // El nick del solicitante tiene un código de color (\x03Owner) pero el
        // bot recibe/usa el nombre "limpio" (Owner): la resolución lo encuentra
        // igual y el comando se ejecuta con su nivel.
        let (ctx, _rx) = ctx_with_user("\x03Owner");
        if let Some(u) = ctx.user_pool.get_by_name("Owner") {
            *u.level.write() = server_core::ILevel::Owner;
        }
        let db = Database::in_memory().unwrap();
        let bot = engine(db, "[CMD]/status hola[/CMD]");
        let mut cfg = bot.config_snapshot();
        cfg.execute_commands = true;
        *bot.config.write() = cfg;

        bot.on_public(&ctx, "Owner", "Nova cambia el status");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(ctx.room_status(), "hola");
    }
}
