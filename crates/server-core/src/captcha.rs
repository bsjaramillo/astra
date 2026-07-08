//! Captcha manager. Equivalente a `core/CaptchaManager.cs` del sb0t original.
//!
//! Mantiene challenges pendientes por user_id. Cada challenge tiene:
//! - La palabra correcta
//! - Bytes de la imagen PNG (para enviarla)
//! - Timestamp de creación
//! - Contador de intentos fallidos
//!
//! Los challenges expiran después de `expiration_secs`.

use parking_lot::Mutex;
use std::collections::HashMap;

use astra_captcha::Captcha;

/// Desafío de captcha pendiente.
#[derive(Debug, Clone)]
pub struct CaptchaChallenge {
    /// La palabra correcta (case-insensitive en la verificación).
    pub word: String,
    /// Bytes de la imagen PNG renderizada.
    pub png: Vec<u8>,
    /// Timestamp de creación (segundos unix).
    pub created_at: u64,
    /// ¿Fue completado?
    pub completed: bool,
    /// Intentos fallidos.
    pub failed_attempts: u32,
}

/// Manager de captchas. Thread-safe.
pub struct CaptchaManager {
    /// Captchas pendientes por user_id.
    pending: Mutex<HashMap<String, CaptchaChallenge>>,
    /// Segundos después de los cuales un challenge expira.
    expiration_secs: u64,
    /// Máx intentos fallidos antes de expulsar al user.
    max_attempts: u32,
}

/// Resultado de verificar una respuesta.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyResult {
    /// Respuesta correcta.
    Ok,
    /// Respuesta incorrecta pero aún tiene intentos.
    Wrong { remaining: u32 },
    /// El user no tiene challenge pendiente.
    NoChallenge,
    /// El challenge ya estaba completado.
    AlreadyCompleted,
    /// El challenge expiró.
    Expired,
    /// Demasiados intentos fallidos.
    TooManyAttempts,
}

impl CaptchaManager {
    /// Crea un manager con los parámetros dados.
    pub fn new(expiration_secs: u64, max_attempts: u32) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            expiration_secs,
            max_attempts,
        }
    }

    /// Crea un nuevo challenge para `user_id`. Reemplaza cualquier
    /// challenge anterior pendiente.
    pub fn create(&self, user_id: String) -> CaptchaChallenge {
        let captcha = Captcha::generate();
        let challenge = CaptchaChallenge {
            word: captcha.word().to_string(),
            png: captcha.png().to_vec(),
            created_at: crate::time::unix_time(),
            completed: false,
            failed_attempts: 0,
        };
        self.pending.lock().insert(user_id, challenge.clone());
        challenge
    }

    /// Verifica una respuesta. Marca el challenge como completado si es OK.
    pub fn verify(&self, user_id: &str, answer: &str) -> VerifyResult {
        let now = crate::time::unix_time();
        let mut pending = self.pending.lock();

        let Some(challenge) = pending.get_mut(user_id) else {
            return VerifyResult::NoChallenge;
        };
        if challenge.completed {
            return VerifyResult::AlreadyCompleted;
        }
        if now.saturating_sub(challenge.created_at) > self.expiration_secs {
            pending.remove(user_id);
            return VerifyResult::Expired;
        }
        if challenge.failed_attempts >= self.max_attempts {
            pending.remove(user_id);
            return VerifyResult::TooManyAttempts;
        }

        if challenge.word.eq_ignore_ascii_case(answer.trim()) {
            challenge.completed = true;
            return VerifyResult::Ok;
        }

        challenge.failed_attempts += 1;
        let remaining = self.max_attempts.saturating_sub(challenge.failed_attempts);
        if remaining == 0 {
            pending.remove(user_id);
            VerifyResult::TooManyAttempts
        } else {
            VerifyResult::Wrong { remaining }
        }
    }

    /// ¿El user tiene un challenge pendiente sin completar?
    pub fn has_pending(&self, user_id: &str) -> bool {
        let pending = self.pending.lock();
        pending
            .get(user_id)
            .map(|c| !c.completed)
            .unwrap_or(false)
    }

    /// Limpia el challenge de un user (cuando se completa, expira o kickea).
    pub fn clear(&self, user_id: &str) {
        self.pending.lock().remove(user_id);
    }

    /// Cantidad de challenges pendientes (para métricas).
    pub fn pending_count(&self) -> usize {
        self.pending.lock().len()
    }

    /// Expira challenges viejos que no se respondieron. Retorna la
    /// cantidad de users que quedaron sin challenge.
    pub fn expire_old(&self) -> usize {
        let now = crate::time::unix_time();
        let mut pending = self.pending.lock();
        let before = pending.len();
        pending.retain(|_, c| !c.completed && now.saturating_sub(c.created_at) <= self.expiration_secs);
        before - pending.len()
    }
}

impl Default for CaptchaManager {
    fn default() -> Self {
        Self::new(300, 3) // 5 min, 3 intentos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_verify_ok() {
        let m = CaptchaManager::default();
        let c = m.create("user1".to_string());
        assert_eq!(c.word.len(), 4);
        assert!(!c.png.is_empty());
        let r = m.verify("user1", &c.word);
        assert_eq!(r, VerifyResult::Ok);
    }

    #[test]
    fn verify_is_case_insensitive() {
        let m = CaptchaManager::default();
        let c = m.create("user1".to_string());
        assert_eq!(m.verify("user1", &c.word.to_lowercase()), VerifyResult::Ok);
    }

    #[test]
    fn verify_with_padding() {
        let m = CaptchaManager::default();
        let c = m.create("user1".to_string());
        assert_eq!(m.verify("user1", &format!("  {}  ", c.word)), VerifyResult::Ok);
    }

    #[test]
    fn wrong_answer_increments_attempts() {
        let m = CaptchaManager::new(300, 3);
        m.create("user1".to_string());
        let r = m.verify("user1", "WRONG");
        assert_eq!(r, VerifyResult::Wrong { remaining: 2 });
        let r = m.verify("user1", "ALSO");
        assert_eq!(r, VerifyResult::Wrong { remaining: 1 });
    }

    #[test]
    fn wrong_answer_three_times_kicks() {
        let m = CaptchaManager::new(300, 3);
        m.create("user1".to_string());
        assert!(matches!(m.verify("user1", "WRONG"), VerifyResult::Wrong { .. }));
        assert!(matches!(m.verify("user1", "WRONG"), VerifyResult::Wrong { .. }));
        let r = m.verify("user1", "WRONG");
        assert_eq!(r, VerifyResult::TooManyAttempts);
        assert!(!m.has_pending("user1"));
    }

    #[test]
    fn verify_no_challenge() {
        let m = CaptchaManager::default();
        assert_eq!(m.verify("nobody", "TEST"), VerifyResult::NoChallenge);
    }

    #[test]
    fn verify_already_completed() {
        let m = CaptchaManager::default();
        let c = m.create("user1".to_string());
        m.verify("user1", &c.word);
        let r = m.verify("user1", &c.word);
        assert_eq!(r, VerifyResult::AlreadyCompleted);
    }

    #[test]
    fn verify_expired() {
        let m = CaptchaManager::new(0, 3); // expira inmediatamente
        m.create("user1".to_string());
        // Necesitamos esperar al menos 1 segundo para que expire
        std::thread::sleep(std::time::Duration::from_secs(1));
        let r = m.verify("user1", "TEST");
        assert_eq!(r, VerifyResult::Expired);
    }

    #[test]
    fn has_pending_tracks_state() {
        let m = CaptchaManager::default();
        assert!(!m.has_pending("user1"));
        let c = m.create("user1".to_string());
        assert!(m.has_pending("user1"));
        m.verify("user1", &c.word);
        assert!(!m.has_pending("user1"));
    }

    #[test]
    fn clear_removes_challenge() {
        let m = CaptchaManager::default();
        m.create("user1".to_string());
        m.clear("user1");
        assert!(!m.has_pending("user1"));
    }

    #[test]
    fn expire_old_removes_expired() {
        let m = CaptchaManager::new(0, 3);
        m.create("user1".to_string());
        m.create("user2".to_string());
        std::thread::sleep(std::time::Duration::from_secs(1));
        let purged = m.expire_old();
        assert_eq!(purged, 2);
        assert_eq!(m.pending_count(), 0);
    }
}
