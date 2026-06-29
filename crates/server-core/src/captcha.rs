//! Captcha manager. Equivalente a `core/CaptchaManager.cs`.

#![allow(dead_code)]

use parking_lot::RwLock;
use std::collections::HashMap;

/// Manager de captchas.
pub struct CaptchaManager {
    /// Captchas pendientes por IP.
    pending: RwLock<HashMap<String, CaptchaChallenge>>,
}

#[derive(Debug, Clone)]
/// Desafío de captcha pendiente.
pub struct CaptchaChallenge {
    /// Palabra a tipear.
    pub word: String,
    /// Timestamp de creación.
    pub created_at: u64,
    /// ¿Fue completado?
    pub completed: bool,
}

impl Default for CaptchaManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptchaManager {
    /// Crea un manager vacío.
    pub fn new() -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
        }
    }

    /// Crea un nuevo captcha para un usuario.
    pub fn create(&self, user_id: String, word: String) {
        let challenge = CaptchaChallenge {
            word,
            created_at: crate::time::unix_time(),
            completed: false,
        };
        self.pending.write().insert(user_id, challenge);
    }

    /// Verifica una respuesta.
    pub fn verify(&self, user_id: &str, answer: &str) -> bool {
        let mut pending = self.pending.write();
        if let Some(challenge) = pending.get_mut(user_id) {
            if !challenge.completed && challenge.word.eq_ignore_ascii_case(answer) {
                challenge.completed = true;
                return true;
            }
        }
        false
    }

    /// Limpia el captcha de un usuario (cuando se completa o expira).
    pub fn clear(&self, user_id: &str) {
        self.pending.write().remove(user_id);
    }
}
