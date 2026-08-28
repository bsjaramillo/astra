//! Memoria de conversación por usuario (historial de mensajes al LLM).
//!
//! Mantiene por nick un deque de `(timestamp, message)` acotado a
//! `memory_turns` y podado por TTL, para que la memoria no crezca sin límite.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Un mensaje del historial en el formato del LLM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// `system` | `user` | `assistant`.
    pub role: String,
    /// Contenido.
    pub content: String,
}

/// TTL default del historial por usuario (30 min de inactividad se olvidan).
const DEFAULT_TTL: Duration = Duration::from_secs(30 * 60);

struct UserHistory {
    entries: VecDeque<(Instant, ChatMessage)>,
}

/// Memoria de conversación thread-safe.
pub struct ConversationMemory {
    inner: Mutex<HashMap<String, UserHistory>>,
}

impl Default for ConversationMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationMemory {
    /// Crea una memoria vacía.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Agrega un mensaje al historial de `user`, podando por turns y TTL.
    pub fn push(&self, user: &str, role: &str, content: &str, max_turns: usize) {
        let mut map = self.inner.lock();
        let now = Instant::now();
        let hist = map.entry(user.to_string()).or_insert_with(|| UserHistory {
            entries: VecDeque::new(),
        });
        // Podar entradas vencidas.
        hist.entries.retain(|(ts, _)| now.duration_since(*ts) < DEFAULT_TTL);
        hist.entries.push_back((now, ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        }));
        // Acotar por turns (toma al menos los más recientes).
        while hist.entries.len() > max_turns.max(1) {
            hist.entries.pop_front();
        }
    }

    /// Historial de `user` (los `max_turns` más recientes), o `None` si no hay.
    pub fn history(&self, user: &str, max_turns: usize) -> Option<Vec<ChatMessage>> {
        let mut map = self.inner.lock();
        let now = Instant::now();
        let hist = map.get_mut(user)?;
        hist.entries.retain(|(ts, _)| now.duration_since(*ts) < DEFAULT_TTL);
        if hist.entries.is_empty() {
            return None;
        }
        let out: Vec<ChatMessage> = hist
            .entries
            .iter()
            .rev()
            .take(max_turns.max(1))
            .rev()
            .map(|(_, m)| m.clone())
            .collect();
        Some(out)
    }

    /// Olvida el historial de `user`.
    pub fn clear(&self, user: &str) {
        self.inner.lock().remove(user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_by_turns() {
        let m = ConversationMemory::new();
        for i in 0..20 {
            m.push("alice", "user", &format!("msg {}", i), 5);
        }
        let h = m.history("alice", 5).unwrap();
        assert_eq!(h.len(), 5);
        assert_eq!(h[4].content, "msg 19");
        assert_eq!(h[0].content, "msg 15");
    }

    #[test]
    fn no_history_for_unknown_user() {
        let m = ConversationMemory::new();
        assert!(m.history("bob", 5).is_none());
    }

    #[test]
    fn clear_forgets() {
        let m = ConversationMemory::new();
        m.push("carol", "user", "hi", 5);
        assert!(m.history("carol", 5).is_some());
        m.clear("carol");
        assert!(m.history("carol", 5).is_none());
    }

    #[test]
    fn preserves_order_oldest_to_newest() {
        let m = ConversationMemory::new();
        m.push("dave", "user", "uno", 5);
        m.push("dave", "assistant", "dos", 5);
        m.push("dave", "user", "tres", 5);
        let h = m.history("dave", 5).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!((h[0].role.as_str(), h[0].content.as_str()), ("user", "uno"));
        assert_eq!((h[1].role.as_str(), h[1].content.as_str()), ("assistant", "dos"));
        assert_eq!((h[2].role.as_str(), h[2].content.as_str()), ("user", "tres"));
    }
}