//! Cover-Bilder für das Radio-Layout. Auslesen (symphonia) und Dekodieren (`image`) laufen
//! in einem eigenen Hintergrund-Thread; die Oberfläche holt nur fertige Texturen ab.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use eframe::egui;
use woa_engine::cover::{MAX_COVER_BYTES, read_cover};

/// Größte Bildkante, die dekodiert wird (Speicherschutz).
pub const MAX_DIM: u32 = 8_000;
/// Kante des großen Covers neben der Uhr.
pub const BIG: u32 = 512;
/// Kante des Vorschaubilds in der Playlist (doppelt für scharfe Darstellung).
pub const THUMB: u32 = 64;
/// Dauer der Überblendung beim Titelwechsel.
const FADE_S: f32 = 0.4;

/// Groß und klein aus einem Bild, fertig für egui.
pub struct Images {
    pub big: egui::ColorImage,
    pub thumb: egui::ColorImage,
}

/// Dekodiert ein Cover und skaliert es herunter. `None` bei zu großem oder kaputtem Bild.
pub fn decode_image(bytes: &[u8]) -> Option<Images> {
    if bytes.len() > MAX_COVER_BYTES {
        return None;
    }
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIM);
    limits.max_image_height = Some(MAX_DIM);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?.into_rgba8();
    let fit = |edge: u32| {
        let s = (edge as f32 / w.max(h) as f32).min(1.0);
        let (nw, nh) = (
            ((w as f32 * s).round() as u32).max(1),
            ((h as f32 * s).round() as u32).max(1),
        );
        let small = image::imageops::thumbnail(&img, nw, nh);
        egui::ColorImage::from_rgba_unmultiplied(
            [nw as usize, nh as usize],
            small.as_flat_samples().as_slice(),
        )
    };
    Some(Images {
        big: fit(BIG),
        thumb: fit(THUMB),
    })
}

enum Slot {
    Pending,
    Missing,
    Ready {
        thumb: egui::TextureHandle,
        big: Option<egui::ColorImage>,
    },
}

/// Cache der Cover je Datei. Kleine Vorschaubilder bleiben, große Texturen nur für den
/// laufenden und den vorigen Titel (Überblendung).
pub struct Covers {
    slots: HashMap<PathBuf, Slot>,
    ask: Sender<PathBuf>,
    done: Receiver<(PathBuf, Option<Images>)>,
    shown: Option<(PathBuf, egui::TextureHandle)>,
    previous: Option<egui::TextureHandle>,
    /// Pfad, auf den zuletzt umgeschaltet wurde (auch ohne Bild). Ohne ihn hätte ein
    /// Titel ohne Cover die Überblendung jeden Frame neu gestartet: Dauer-Repaint,
    /// 100 % CPU im UI-Thread (Paket 8, 29.10.2026).
    target: Option<PathBuf>,
    switched_at: Instant,
}

impl Covers {
    pub fn new(ctx: &egui::Context) -> Self {
        let (ask, todo) = channel::<PathBuf>();
        let (send, done) = channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("woa-cover".into())
            .spawn(move || {
                for p in todo {
                    let imgs = read_cover(&p).and_then(|c| decode_image(&c.data));
                    if send.send((p, imgs)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("Cover-Thread startet nicht: {e}");
        }
        Self {
            slots: HashMap::new(),
            ask,
            done,
            shown: None,
            previous: None,
            target: None,
            switched_at: Instant::now(),
        }
    }

    /// Fertige Bilder abholen und als Texturen anlegen.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((p, imgs)) = self.done.try_recv() {
            let slot = match imgs {
                Some(i) => Slot::Ready {
                    thumb: ctx.load_texture(
                        format!("cover-thumb:{}", p.display()),
                        i.thumb,
                        egui::TextureOptions::LINEAR,
                    ),
                    big: Some(i.big),
                },
                None => Slot::Missing,
            };
            self.slots.insert(p, slot);
        }
    }

    fn request(&mut self, path: &Path) -> Option<&mut Slot> {
        if !self.slots.contains_key(path) {
            self.slots.insert(path.to_path_buf(), Slot::Pending);
            let _ = self.ask.send(path.to_path_buf());
        }
        self.slots.get_mut(path)
    }

    /// Vorschaubild der Datei, sobald es da ist.
    pub fn thumb(&mut self, path: &Path) -> Option<egui::TextureHandle> {
        match self.request(path)? {
            Slot::Ready { thumb, .. } => Some(thumb.clone()),
            _ => None,
        }
    }

    /// Großes Cover des laufenden Titels und die Überblendung: (neu, alt, Anteil neu 0..1).
    pub fn big(
        &mut self,
        ctx: &egui::Context,
        path: Option<&Path>,
    ) -> (
        Option<egui::TextureHandle>,
        Option<egui::TextureHandle>,
        f32,
    ) {
        let now_path = path.map(Path::to_path_buf);
        if self.target != now_path {
            let tex = path.and_then(|p| {
                if let Some(Slot::Ready { big, .. }) = self.request(p) {
                    big.clone()
                        .map(|img| ctx.load_texture("cover-big", img, egui::TextureOptions::LINEAR))
                } else {
                    None
                }
            });
            // Erst umschalten, wenn das Bild da ist oder sicher fehlt.
            let settled = !matches!(path.and_then(|p| self.slots.get(p)), Some(Slot::Pending));
            if settled {
                self.previous = self.shown.take().map(|(_, t)| t);
                self.target.clone_from(&now_path);
                if let (Some(p), Some(t)) = (now_path, tex) {
                    self.shown = Some((p, t));
                } else {
                    self.shown = None;
                }
                self.switched_at = Instant::now();
            }
        }
        let t = (self.switched_at.elapsed().as_secs_f32() / FADE_S).min(1.0);
        if t < 1.0 {
            // ~60 Bilder/s nur während der kurzen Überblendung.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else {
            self.previous = None;
        }
        (
            self.shown.as_ref().map(|(_, t)| t.clone()),
            self.previous.clone(),
            t,
        )
    }
}

/// Zeichnet ein Cover-Quadrat: Bild mit Überblendung oder Platzhalter mit Note.
pub fn paint_square(
    ui: &mut egui::Ui,
    size: f32,
    now: Option<&egui::TextureHandle>,
    before: Option<&egui::TextureHandle>,
    t: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let r = egui::CornerRadius::same((size / 16.0).clamp(2.0, 10.0) as u8);
    painter.rect_filled(rect, r, crate::theme::surface());
    let draw = |tex: &egui::TextureHandle, alpha: f32| {
        let [w, h] = tex.size();
        let s = size / w.max(h).max(1) as f32;
        let img =
            egui::Rect::from_center_size(rect.center(), egui::vec2(w as f32 * s, h as f32 * s));
        let tint = egui::Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0) as u8);
        egui::Image::new(tex)
            .corner_radius(r)
            .tint(tint)
            .paint_at(ui, img);
    };
    if let Some(b) = before {
        draw(b, 1.0 - t);
    }
    match now {
        Some(n) => draw(n, t),
        None => {
            let c = crate::theme::mix(
                crate::theme::text_dim(),
                crate::theme::surface(),
                1.0 - t * 0.6,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                crate::theme::ic::MUSIC_NOTE,
                egui::FontId::proportional(size * 0.45),
                c,
            );
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        let data: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 251) as u8, 80, 200])
            .collect();
        wr.write_image_data(&data).unwrap();
        wr.finish().unwrap();
        out
    }

    /// Titel ohne Cover: die Überblendung startet einmal, nicht jeden Frame neu
    /// (sonst Dauer-Repaint, 100 % CPU im UI-Thread).
    #[test]
    fn missing_cover_does_not_restart_fade() {
        let ctx = crate::theme::test_context();
        let mut c = Covers::new(&ctx);
        let p = PathBuf::from("/kein/cover.mp3");
        c.slots.insert(p.clone(), Slot::Missing);
        let _ = c.big(&ctx, Some(&p));
        let first = c.switched_at;
        std::thread::sleep(std::time::Duration::from_millis(5));
        for _ in 0..3 {
            let _ = c.big(&ctx, Some(&p));
        }
        assert_eq!(c.switched_at, first);
        let _ = c.big(&ctx, None);
        assert!(c.switched_at > first, "Wechsel auf None blendet um");
    }

    #[test]
    fn png_is_scaled_to_fit() {
        let i = decode_image(&png(1024, 512)).unwrap();
        assert_eq!(i.big.size, [512, 256]);
        assert_eq!(i.thumb.size, [64, 32]);
        let s = decode_image(&png(100, 100)).unwrap();
        assert_eq!(
            s.big.size,
            [100, 100],
            "kleine Bilder werden nicht vergrößert"
        );
    }

    #[test]
    fn huge_or_broken_images_are_refused() {
        assert!(decode_image(&png(MAX_DIM + 1, 1)).is_none());
        assert!(decode_image(b"kein bild").is_none());
        assert!(decode_image(&vec![0u8; MAX_COVER_BYTES + 1]).is_none());
    }

    #[test]
    fn jpeg_is_decoded() {
        let mut out = Vec::new();
        let img = image::RgbImage::from_pixel(40, 30, image::Rgb([200, 30, 30]));
        image::codecs::jpeg::JpegEncoder::new(&mut out)
            .encode_image(&img)
            .unwrap();
        let i = decode_image(&out).unwrap();
        assert_eq!(i.big.size, [40, 30]);
    }
}
