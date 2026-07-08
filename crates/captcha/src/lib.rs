//! Generación de captchas para Ares Galaxy.
//!
//! Cada captcha es una palabra de 4 letras (A-Z) elegida al azar de un
//! wordlist de ~250 palabras, junto con una imagen PNG que la dibuja con
//! un bitmap font 5x7 + ruido aleatorio.
//!
//! Inspirado en `core/CaptchaManager.cs` del sb0t original.
//!
//! ## Uso
//!
//! ```rust
//! use astra_captcha::Captcha;
//!
//! let captcha = Captcha::generate();
//! assert_eq!(captcha.word().len(), 4);
//! let png_bytes = captcha.png();
//! assert!(!png_bytes.is_empty());
//! ```

#![warn(missing_docs)]

mod font;
mod image;
mod wordlist;

use rand::seq::SliceRandom;

/// Un desafío de captcha listo para enviar al usuario.
#[derive(Debug, Clone)]
pub struct Captcha {
    /// La palabra correcta (4 letras, A-Z). Comparación case-insensitive.
    word: String,
    /// Bytes de la imagen PNG.
    png: Vec<u8>,
}

impl Captcha {
    /// Genera un captcha nuevo: palabra al azar + imagen PNG.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let word = wordlist::WORDS
            .choose(&mut rng)
            .copied()
            .expect("wordlist should be non-empty")
            .to_string();
        let png = image::render_png(&word);
        Self { word, png }
    }

    /// Genera un captcha con la palabra forzada (útil para tests).
    pub fn with_word(word: &str) -> Self {
        let png = image::render_png(word);
        Self {
            word: word.to_uppercase(),
            png,
        }
    }

    /// La palabra correcta.
    pub fn word(&self) -> &str {
        &self.word
    }

    /// Bytes de la imagen PNG renderizada.
    pub fn png(&self) -> &[u8] {
        &self.png
    }

    /// Verifica una respuesta del usuario (case-insensitive).
    pub fn verify(&self, answer: &str) -> bool {
        self.word.eq_ignore_ascii_case(answer.trim())
    }
}

/// Verifica que el wordlist del crate es válido. Útil para chequeos
/// al inicio del server.
pub fn validate_wordlist() -> Result<(), &'static str> {
    wordlist::validate_wordlist()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_4_letter_word() {
        let c = Captcha::generate();
        assert_eq!(c.word().len(), 4);
        assert!(c.word().chars().all(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn generate_produces_png() {
        let c = Captcha::generate();
        assert!(c.png().len() > 100, "PNG demasiado pequeño: {}", c.png().len());
        assert_eq!(&c.png()[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn verify_is_case_insensitive() {
        let c = Captcha::with_word("HELLO");
        assert!(c.verify("HELLO"));
        assert!(c.verify("hello"));
        assert!(c.verify("Hello"));
        assert!(c.verify("  hello  "));
        assert!(!c.verify("world"));
    }

    #[test]
    fn with_word_uppercases_input() {
        let c = Captcha::with_word("test");
        assert_eq!(c.word(), "TEST");
    }

    #[test]
    fn generated_captchas_are_random() {
        // Generar 10 captchas y verificar que no todos sean iguales.
        let words: Vec<String> = (0..10).map(|_| Captcha::generate().word().to_string()).collect();
        let unique: std::collections::HashSet<_> = words.iter().collect();
        assert!(unique.len() >= 2, "10 captchas generaron palabras: {:?}", words);
    }

    #[test]
    fn validate_wordlist_passes() {
        validate_wordlist().unwrap();
    }
}
