//! QR-Code für die Adresse der Fernbedienung. Gezeichnet wird in der Oberfläche; hier nur die
//! Matrix.

use qrcode::{Color, QrCode};

/// Quadratische Matrix aus dunklen und hellen Modulen.
#[derive(Debug, Clone, PartialEq)]
pub struct QrMatrix {
    pub size: usize,
    modules: Vec<bool>,
}

impl QrMatrix {
    /// `true` = dunkel. Außerhalb: hell.
    pub fn dark(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }
}

/// Baut den Code; `None`, wenn der Text nicht hineinpasst.
pub fn qr(text: &str) -> Option<QrMatrix> {
    let code = QrCode::new(text.as_bytes()).ok()?;
    let size = code.width();
    let modules = code
        .to_colors()
        .into_iter()
        .map(|c| c == Color::Dark)
        .collect();
    Some(QrMatrix { size, modules })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_square_with_finder_patterns() {
        let m = qr("http://192.0.2.1:8787/?pin=123456").expect("qr");
        assert!(m.size >= 21);
        // Suchmuster: Ecken des Rahmens oben links sind dunkel.
        assert!(m.dark(0, 0) && m.dark(6, 0) && m.dark(0, 6));
        assert!(!m.dark(m.size + 1, 0));
    }

    #[test]
    fn too_long_text_gives_none() {
        assert!(qr(&"x".repeat(5000)).is_none());
    }
}
