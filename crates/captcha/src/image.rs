//! Renderizado de la imagen PNG del captcha.
//!
//! Genera una imagen en escala de grises con la palabra dibujada usando
//! el bitmap font de [`font`]. Se añade un ruido mínimo (puntos sueltos)
//! para entorpecer OCRs simples sin destruir la legibilidad.

use image::{GrayImage, Luma};
use rand::Rng;

use super::font;

/// Genera una imagen PNG (bytes) con la palabra `word` dibujada.
///
/// La imagen mide `(5+1)*len + 4` × `9` píxeles. Se añade ruido mínimo
/// (3-5 píxeles grises) para que el OCR simple falle, pero el humano
/// puede leer la palabra sin problemas.
pub fn render_png(word: &str) -> Vec<u8> {
    let upper: String = word.chars().flat_map(|c| c.to_uppercase()).collect();
    let chars: Vec<char> = upper.chars().collect();

    let char_w = font::GLYPH_W;
    let char_h = font::GLYPH_H;
    let kern = font::KERNING;
    let pad_y = font::V_PADDING;

    let total_w: u32 = (chars.len() * (char_w + kern) + 4) as u32;
    let total_h: u32 = (char_h + pad_y * 2) as u32;

    let mut img = GrayImage::from_pixel(total_w, total_h, Luma([255u8]));

    let mut rng = rand::thread_rng();

    let mut x = 2u32;
    for &c in &chars {
        if let Some(rows) = font::glyph(c) {
            // rows[row] = u8 con 5 bits, bit col = pixel(col, row)
            for (row, &row_bits) in rows.iter().enumerate() {
                for col in 0..char_w {
                    let bit = (row_bits >> col) & 1;
                    if bit == 1 {
                        let px = x + col as u32;
                        let py = pad_y as u32 + row as u32;
                        img.put_pixel(px, py, Luma([0u8]));
                    }
                }
            }
        }
        x += (char_w + kern) as u32;
    }

    // Ruido: 3-5 píxeles grises (no negros, para no destruir el texto).
    let noise_count = 3 + (rng.gen_range(0..3));
    for _ in 0..noise_count {
        let nx = rng.gen_range(0..total_w);
        let ny = rng.gen_range(0..total_h);
        let v: u8 = rng.gen_range(100..200);
        img.put_pixel(nx, ny, Luma([v]));
    }

    let mut buf = Vec::new();
    let dyn_img = image::DynamicImage::ImageLuma8(img);
    dyn_img
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("PNG encoding should not fail");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_produces_valid_png() {
        let png = render_png("HELLO");
        assert!(png.len() > 8);
        assert_eq!(&png[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn render_handles_lowercase() {
        let png = render_png("test");
        assert!(png.len() > 8);
        assert_eq!(&png[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn render_handles_short_words() {
        let png = render_png("AB");
        assert!(png.len() > 8);
    }

    #[test]
    fn render_dimensions_match_font() {
        // Para "HELLO" (5 chars): width = 5*6 + 4 = 34, height = 7 + 2 = 9
        let png = render_png("HELLO");
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!(img.width(), 34);
        assert_eq!(img.height(), 9);
    }
}
