//! ElevenLabs in der Oberfläche: Zugang unter Einstellungen → KI ([`Zugang`]) und der
//! Erzeuger für den Jingle-Assistenten ([`Erzeuger`]: Sprache, Effekt, Bett).
//!
//! Schlüsselbund und Netz nie im UI-Thread: jeder Schritt läuft in einem kurzen
//! Hintergrund-Thread, die Antwort kommt über einen Kanal. Der Schlüssel steht nie im
//! Fenster, nie im Log und nie in einer Meldung.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use woa_agent::elevenlabs::{
    self, Ablage, Art, BettAuftrag, EffektAuftrag, Herkunft, Klangerzeuger, Konto, Source,
    SprachAuftrag, Sprecher, Stimme, TtsModell,
};
use woa_secrets::Ort;

use crate::theme;

const ACCOUNT: &str = elevenlabs::KEY_ACCOUNT;

/// Ablegen, Nachsehen, Entfernen und Konto lesen; in Tests austauschbar.
#[derive(Clone, Copy)]
pub struct Ops {
    pub stored: fn(&Path, &str) -> Option<Ort>,
    pub save: fn(&Path, &str, &str) -> Result<Ort, String>,
    pub clear: fn(&Path, &str) -> Result<(), String>,
    /// Kontostand mit dem gespeicherten Schlüssel (`None`) oder einem frisch eingegebenen.
    pub konto: fn(&Path, Option<String>) -> Result<Konto, String>,
}

fn konto_real(data_dir: &Path, key: Option<String>) -> Result<Konto, String> {
    let src = match key {
        Some(k) => Source::with_key(elevenlabs::API, k),
        None => Source::from_data_dir(data_dir.to_path_buf()),
    };
    src.konto().map_err(|e| e.to_string())
}

impl Default for Ops {
    fn default() -> Self {
        fn none(_: &Path, _: &str) -> Option<Ort> {
            None
        }
        Self {
            // Tests sehen nie im Schlüsselbund des Rechners nach.
            stored: if cfg!(test) {
                none
            } else {
                woa_secrets::stored
            },
            save: woa_secrets::save,
            clear: woa_secrets::clear,
            konto: konto_real,
        }
    }
}

enum ZMsg {
    Stored(Option<Ort>),
    Saved(Result<Ort, String>),
    Cleared(Result<(), String>),
    Konto(Result<Konto, String>),
}

/// Zeile „ElevenLabs-Schlüssel“ unter Einstellungen → KI.
pub struct Zugang {
    input: String,
    stored: Option<Option<Ort>>,
    asked: bool,
    konto: Option<Result<Konto, String>>,
    busy: bool,
    ops: Ops,
    tx: Sender<ZMsg>,
    rx: Receiver<ZMsg>,
}

impl Default for Zugang {
    fn default() -> Self {
        Self::with_ops(Ops::default())
    }
}

impl Zugang {
    pub fn with_ops(ops: Ops) -> Self {
        let (tx, rx) = channel();
        Self {
            input: String::new(),
            stored: None,
            asked: false,
            konto: None,
            busy: false,
            ops,
            tx,
            rx,
        }
    }

    fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.busy = false;
            match m {
                ZMsg::Stored(o) => self.stored = Some(o),
                ZMsg::Saved(Ok(o)) => {
                    self.stored = Some(Some(o));
                    tracing::info!("elevenlabs key stored");
                }
                ZMsg::Saved(Err(e)) => {
                    tracing::warn!(error = %e, "elevenlabs key not stored");
                    self.konto = Some(Err(format!("Nicht gespeichert: {e}")));
                }
                ZMsg::Cleared(r) => {
                    self.stored = Some(None);
                    self.konto = r.err().map(|e| Err(format!("Nicht entfernt: {e}")));
                }
                ZMsg::Konto(r) => self.konto = Some(r),
            }
        }
    }

    fn spawn(&mut self, f: impl FnOnce() -> ZMsg + Send + 'static) {
        let tx = self.tx.clone();
        self.busy = true;
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }

    /// Speichert den eingegebenen Schlüssel und prüft ihn gleich am Konto.
    pub fn save(&mut self, data_dir: &Path) {
        let key = std::mem::take(&mut self.input).trim().to_string();
        if key.is_empty() {
            return;
        }
        let (dir, ops, tx) = (data_dir.to_path_buf(), self.ops, self.tx.clone());
        self.spawn(move || {
            let r = (ops.save)(&dir, ACCOUNT, &key);
            if r.is_ok() {
                let _ = tx.send(ZMsg::Konto((ops.konto)(&dir, Some(key))));
            }
            ZMsg::Saved(r)
        });
    }

    pub fn check(&mut self, data_dir: &Path) {
        let (dir, ops) = (data_dir.to_path_buf(), self.ops);
        self.spawn(move || ZMsg::Konto((ops.konto)(&dir, None)));
    }

    pub fn remove(&mut self, data_dir: &Path) {
        let (dir, ops) = (data_dir.to_path_buf(), self.ops);
        self.konto = None;
        self.spawn(move || ZMsg::Cleared((ops.clear)(&dir, ACCOUNT)));
    }

    /// Wartet (für Tests) bis keine Antwort mehr aussteht.
    #[cfg(test)]
    fn settle(&mut self) {
        let t0 = std::time::Instant::now();
        loop {
            self.drain();
            if !self.busy || t0.elapsed().as_secs() > 5 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        self.drain();
    }

    pub fn show(&mut self, ui: &mut egui::Ui, data_dir: &Path) {
        self.drain();
        if !self.asked {
            self.asked = true;
            let (dir, ops) = (data_dir.to_path_buf(), self.ops);
            self.spawn(move || ZMsg::Stored((ops.stored)(&dir, ACCOUNT)));
        }
        if self.busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        }
        let has = matches!(self.stored, Some(Some(_)));
        ui.horizontal(|ui| {
            ui.label("ElevenLabs-Schlüssel");
            let hint = match self.stored {
                None => "wird nachgesehen …".to_string(),
                Some(Some(o)) => format!("gespeichert {}", o.label()),
                Some(None) => "für Sprache, Effekte, Musik (optional)".to_string(),
            };
            ui.add(
                egui::TextEdit::singleline(&mut self.input)
                    .password(true)
                    .hint_text(hint)
                    .desired_width(220.0),
            );
            if ui
                .add_enabled(
                    !self.input.trim().is_empty() && !self.busy,
                    egui::Button::new("Speichern"),
                )
                .on_hover_text(
                    "Eigener Schlüssel aus deinem ElevenLabs-Konto (Profil → API Keys). \
                     Er bleibt im Schlüsselbund des Systems.",
                )
                .clicked()
            {
                self.save(data_dir);
            }
            if ui
                .add_enabled(has && !self.busy, egui::Button::new("Prüfen"))
                .on_hover_text("Fragt das Konto ab: Abo und verbleibende Credits")
                .clicked()
            {
                self.check(data_dir);
            }
            if ui
                .add_enabled(has && !self.busy, egui::Button::new("Entfernen"))
                .on_hover_text("Löscht den Schlüssel aus dem Schlüsselbund")
                .clicked()
            {
                self.remove(data_dir);
            }
        });
        match &self.konto {
            Some(Ok(k)) => {
                ui.label(egui::RichText::new(k.label()).small().color(theme::text()));
            }
            Some(Err(e)) => {
                ui.label(egui::RichText::new(e).small().color(theme::danger_text()));
            }
            None => {}
        }
        ui.label(
            egui::RichText::new(elevenlabs::DATENSCHUTZ)
                .small()
                .color(theme::text_dim()),
        );
    }
}

/// Ein fertig erzeugter Klang, bereit für die Spur.
#[derive(Debug, Clone, PartialEq)]
pub struct Fertig {
    pub art: Art,
    /// Datei im Aufnahmeordner des Projekts.
    pub path: PathBuf,
    pub seconds: f64,
}

#[derive(Debug, Clone)]
enum Auftrag {
    Sprache(SprachAuftrag, String),
    Effekt(EffektAuftrag),
    Bett(BettAuftrag),
}

impl Auftrag {
    fn art(&self) -> Art {
        match self {
            Self::Sprache(..) => Art::Sprache,
            Self::Effekt(_) => Art::Effekt,
            Self::Bett(_) => Art::Bett,
        }
    }

    fn schaetzung(&self) -> elevenlabs::Schaetzung {
        match self {
            Self::Sprache(a, _) => elevenlabs::schaetzung_sprache(&a.text),
            Self::Effekt(a) => elevenlabs::schaetzung_effekt(a.seconds),
            Self::Bett(a) => elevenlabs::schaetzung_bett(a.seconds),
        }
    }
}

enum EMsg {
    Key(bool),
    Voices(Result<Vec<Stimme>, String>),
    Done(Art, Result<Fertig, String>),
}

/// Erzeuger im Jingle-Assistenten.
pub struct Erzeuger {
    pub data_dir: PathBuf,
    /// Medienordner und Bibliothek; `None` in Tests.
    pub ablage: Option<Ablage>,
    /// Quelle; `None` = Schlüsselbund unter `data_dir`.
    pub source: Option<Source>,
    has_key: Option<bool>,
    asked: bool,
    voices: Option<Result<Vec<Stimme>, String>>,
    pub voice: usize,
    pub model: TtsModell,
    pub stability: f64,
    pub style: f64,
    pub text: String,
    pub fx_text: String,
    pub fx_seconds: f64,
    pub bett_text: String,
    confirm: Option<Auftrag>,
    running: Option<Art>,
    pub note: Option<String>,
    tx: Sender<EMsg>,
    rx: Receiver<EMsg>,
}

impl Default for Erzeuger {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            data_dir: woa_radio::data_dir(),
            // Tests fassen weder Schlüsselbund noch Bibliothek des Rechners an.
            ablage: if cfg!(test) {
                None
            } else {
                Some(Ablage::standard())
            },
            source: cfg!(test).then(|| Source {
                api: "http://127.0.0.1:9".into(),
                key: std::sync::Arc::new(|| None),
            }),
            has_key: None,
            asked: false,
            voices: None,
            voice: 0,
            model: TtsModell::default(),
            stability: 0.5,
            style: 0.0,
            text: String::new(),
            fx_text: "Rakete startet und explodiert".into(),
            fx_seconds: 3.0,
            bett_text: "fröhliches, sommerliches Radio-Bett, Gitarre und Schlagzeug".into(),
            confirm: None,
            running: None,
            note: None,
            tx,
            rx,
        }
    }
}

impl Erzeuger {
    fn source(&self) -> Source {
        self.source
            .clone()
            .unwrap_or_else(|| Source::from_data_dir(self.data_dir.clone()))
    }

    /// Fotomodus: tut so, als läge ein Schlüssel und zwei Stimmen; kein Netz, kein Schlüsselbund.
    pub fn demo(&mut self) {
        self.asked = true;
        self.has_key = Some(true);
        self.source = Some(Source {
            api: "http://127.0.0.1:9".into(),
            key: std::sync::Arc::new(|| None),
        });
        self.ablage = None;
        self.voices = Some(Ok(vec![
            Stimme {
                id: "demo1".into(),
                name: "Anna (Beispiel)".into(),
                category: "premade".into(),
            },
            Stimme {
                id: "demo2".into(),
                name: "Bert (Beispiel)".into(),
                category: "premade".into(),
            },
        ]));
        self.text = "Wir on Air – dein Radio für den Sommer!".into();
    }

    /// Liegt ein Schlüssel? `None`: wird nachgesehen.
    pub fn has_key(&mut self) -> Option<bool> {
        if !self.asked {
            self.asked = true;
            let (tx, src) = (self.tx.clone(), self.source());
            std::thread::spawn(move || {
                let _ = tx.send(EMsg::Key(src.has_key()));
            });
        }
        self.has_key
    }

    /// Nimmt Antworten an; gibt fertige Klänge für die Spur zurück.
    pub fn poll(&mut self, ctx: &egui::Context) -> Vec<Fertig> {
        let mut out = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            match m {
                EMsg::Key(k) => self.has_key = Some(k),
                EMsg::Voices(v) => self.voices = Some(v),
                EMsg::Done(art, r) => {
                    self.running = None;
                    match r {
                        Ok(f) => {
                            self.note = Some(format!(
                                "Fertig: {} ({:.1} s)",
                                f.path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                                f.seconds
                            ));
                            out.push(f);
                        }
                        Err(e) => {
                            tracing::warn!(art = art.as_str(), error = %e, "elevenlabs failed");
                            self.note = Some(e);
                        }
                    }
                }
            }
        }
        if self.running.is_some() || (self.asked && self.has_key.is_none()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        out
    }

    fn load_voices(&mut self) {
        if self.voices.is_some() {
            return;
        }
        self.voices = Some(Ok(Vec::new()));
        let (tx, src) = (self.tx.clone(), self.source());
        std::thread::spawn(move || {
            let _ = tx.send(EMsg::Voices(src.stimmen().map_err(|e| e.to_string())));
        });
    }

    /// Startet den bestätigten Auftrag; die Datei landet in `takes_dir`.
    fn start(&mut self, a: Auftrag, takes_dir: PathBuf) {
        let art = a.art();
        self.running = Some(art);
        self.note = None;
        let (tx, src, ablage) = (self.tx.clone(), self.source(), self.ablage.clone());
        std::thread::spawn(move || {
            let r = run(&src, &a, &takes_dir, ablage.as_ref());
            let _ = tx.send(EMsg::Done(art, r));
        });
    }

    /// Bestätigungskasten: Schätzung, Datenschutz, Erzeugen/Abbrechen.
    fn confirm_ui(&mut self, ui: &mut egui::Ui, takes_dir: &Path) {
        let Some(a) = self.confirm.clone() else {
            return;
        };
        egui::Frame::new()
            .fill(theme::well())
            .corner_radius(theme::RADIUS_MD)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(a.schaetzung().label()).strong());
                ui.label(
                    egui::RichText::new(elevenlabs::DATENSCHUTZ)
                        .small()
                        .color(theme::text_dim()),
                );
                ui.horizontal(|ui| {
                    if ui.button("Erzeugen").clicked() {
                        self.confirm = None;
                        self.start(a.clone(), takes_dir.to_path_buf());
                    }
                    if ui.button("Abbrechen").clicked() {
                        self.confirm = None;
                    }
                });
            });
    }

    fn busy_ui(&self, ui: &mut egui::Ui) -> bool {
        if let Some(art) = self.running {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "ElevenLabs erzeugt {} …",
                    match art {
                        Art::Sprache => "die Sprache",
                        Art::Effekt => "den Effekt",
                        Art::Bett => "das Bett",
                    }
                ));
            });
            return true;
        }
        if let Some(n) = &self.note {
            ui.label(egui::RichText::new(n).small().color(theme::text_dim()));
        }
        false
    }

    /// Schritt „Stimme“: Sprecher ElevenLabs. Nur aufrufen, wenn ein Schlüssel liegt.
    pub fn sprache_ui(&mut self, ui: &mut egui::Ui, takes_dir: &Path) {
        self.load_voices();
        let voices = match &self.voices {
            Some(Ok(v)) => v.clone(),
            Some(Err(e)) => {
                ui.label(egui::RichText::new(e).small().color(theme::danger_text()));
                if ui.button("Stimmen neu laden").clicked() {
                    self.voices = None;
                }
                return;
            }
            None => Vec::new(),
        };
        ui.add(
            egui::TextEdit::multiline(&mut self.text)
                .hint_text("Was die Stimme sagen soll, z. B. „Wir on Air – dein Radio!“")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        egui::Grid::new("el-tts").num_columns(2).show(ui, |ui| {
            ui.label("Stimme");
            if voices.is_empty() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Stimmen werden geladen …");
                });
            } else {
                self.voice = self.voice.min(voices.len() - 1);
                egui::ComboBox::from_id_salt("el-voice")
                    .selected_text(&voices[self.voice].name)
                    .show_ui(ui, |ui| {
                        for (i, v) in voices.iter().enumerate() {
                            ui.selectable_value(&mut self.voice, i, &v.name);
                        }
                    });
            }
            ui.end_row();
            ui.label("Modell");
            egui::ComboBox::from_id_salt("el-model")
                .selected_text(self.model.label())
                .show_ui(ui, |ui| {
                    for m in TtsModell::ALL {
                        ui.selectable_value(&mut self.model, m, m.label());
                    }
                });
            ui.end_row();
            ui.label("Stabilität");
            ui.add(egui::Slider::new(&mut self.stability, 0.0..=1.0))
                .on_hover_text("Weniger: lebendiger, mehr: gleichmäßiger");
            ui.end_row();
            ui.label("Stil");
            ui.add(egui::Slider::new(&mut self.style, 0.0..=1.0))
                .on_hover_text("Mehr: übertriebener Vortrag");
            ui.end_row();
        });
        if self.busy_ui(ui) {
            return;
        }
        if self.confirm.is_some() {
            self.confirm_ui(ui, takes_dir);
            return;
        }
        let ready = !self.text.trim().is_empty() && !voices.is_empty();
        if ui
            .add_enabled(ready, egui::Button::new("Sprache erzeugen …"))
            .on_hover_text("Zeigt erst die geschätzten Kosten; erzeugt erst nach Bestätigung")
            .clicked()
            && let Some(v) = voices.get(self.voice)
        {
            let mut a = SprachAuftrag::new(self.text.trim(), v.id.clone());
            a.model = self.model;
            a.stability = self.stability;
            a.style = self.style;
            self.confirm = Some(Auftrag::Sprache(a, v.name.clone()));
        }
    }

    /// Schritt „Schluss“: Effekt und Bett erzeugen. `bed_seconds`: Länge des Jingles.
    pub fn klang_ui(&mut self, ui: &mut egui::Ui, takes_dir: &Path, bed_seconds: f64) {
        ui.label(theme::panel_label("ELEVENLABS"));
        egui::Grid::new("el-klang").num_columns(3).show(ui, |ui| {
            ui.label("Effekt");
            ui.add(egui::TextEdit::singleline(&mut self.fx_text).desired_width(200.0));
            ui.add(
                egui::DragValue::new(&mut self.fx_seconds)
                    .range(elevenlabs::SFX_SECONDS.0..=elevenlabs::SFX_SECONDS.1)
                    .speed(0.1)
                    .suffix(" s"),
            );
            ui.end_row();
            ui.label("Bett");
            ui.add(egui::TextEdit::singleline(&mut self.bett_text).desired_width(200.0));
            ui.label(format!("{bed_seconds:.0} s"));
            ui.end_row();
        });
        if self.busy_ui(ui) {
            return;
        }
        if self.confirm.is_some() {
            self.confirm_ui(ui, takes_dir);
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.fx_text.trim().is_empty(), egui::Button::new("Effekt erzeugen …"))
                .on_hover_text("Z. B. Rakete, Böller, Whoosh; landet auf einer neuen Spur")
                .clicked()
            {
                self.confirm = Some(Auftrag::Effekt(EffektAuftrag {
                    text: self.fx_text.trim().to_string(),
                    seconds: Some(self.fx_seconds),
                }));
            }
            if ui
                .add_enabled(!self.bett_text.trim().is_empty(), egui::Button::new("Bett erzeugen …"))
                .on_hover_text("Instrumentale Musik nach Beschreibung, so lang wie der Jingle, auf neuer Spur „Bett“")
                .clicked()
            {
                self.confirm = Some(Auftrag::Bett(BettAuftrag {
                    text: self.bett_text.trim().to_string(),
                    seconds: bed_seconds,
                }));
            }
        });
    }
}

/// Erzeugen, ablegen (Projekt, Medienordner, Bibliothek), Länge messen. Hintergrund-Thread.
fn run(
    src: &Source,
    a: &Auftrag,
    takes_dir: &Path,
    ablage: Option<&Ablage>,
) -> Result<Fertig, String> {
    let (audio, mut h) = match a {
        Auftrag::Sprache(s, name) => {
            let mut h = Herkunft::new(src.anbieter(), Art::Sprache, &s.text);
            h.stimme = name.clone();
            h.modell = s.model.id().to_string();
            (src.sprechen(s), h)
        }
        Auftrag::Effekt(e) => (
            src.effekt(e),
            Herkunft::new("ElevenLabs", Art::Effekt, &e.text),
        ),
        Auftrag::Bett(b) => (src.bett(b), Herkunft::new("ElevenLabs", Art::Bett, &b.text)),
    };
    if h.modell.is_empty() {
        h.modell = match h.art {
            Art::Effekt => "eleven_text_to_sound_v2".into(),
            _ => "music".into(),
        };
    }
    let audio = audio.map_err(|e| e.to_string())?;
    let path = elevenlabs::ablegen(&audio, takes_dir, &elevenlabs::stem(h.art, &h.text), &h)?;
    // Bei 60 BPM sind Beats Sekunden.
    let seconds = woa_agent::audio::wav_length_beats(&path, 60.0)?;
    if let Some(ab) = ablage
        && let Err(e) = elevenlabs::in_bibliothek(&path, &ab.media_dir, &ab.db, &h)
    {
        tracing::warn!(error = %e, "elevenlabs result not in library");
    }
    Ok(Fertig {
        art: h.art,
        path,
        seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static SAVED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    fn none(_: &Path, _: &str) -> Option<Ort> {
        None
    }
    fn save(_: &Path, acc: &str, v: &str) -> Result<Ort, String> {
        SAVED.lock().unwrap().push((acc.into(), v.into()));
        Ok(Ort::Datei)
    }
    fn clear(_: &Path, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn konto(_: &Path, key: Option<String>) -> Result<Konto, String> {
        // Gespeichert (None) oder frisch eingegeben: beides ergibt dasselbe Konto.
        let _ = key;
        Ok(Konto {
            tier: "free".into(),
            character_count: 100,
            character_limit: 10_000,
            reset_unix: None,
        })
    }

    #[test]
    fn key_goes_to_its_own_entry_shows_credits_and_can_be_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut z = Zugang::with_ops(Ops {
            stored: none,
            save,
            clear,
            konto,
        });
        z.input = "  el-fake-key  ".into();
        z.save(tmp.path());
        assert!(z.input.is_empty(), "Feld ist nach dem Speichern leer");
        z.settle();
        assert_eq!(
            *SAVED.lock().unwrap(),
            [("elevenlabs".to_string(), "el-fake-key".to_string())]
        );
        assert_eq!(z.stored, Some(Some(Ort::Datei)));
        let label = z.konto.as_ref().unwrap().as_ref().unwrap().label();
        assert!(label.contains("9900"), "{label}");
        assert!(!label.contains("el-fake-key"));
        z.remove(tmp.path());
        z.settle();
        assert_eq!(z.stored, Some(None));
        assert!(z.konto.is_none());
    }

    #[test]
    fn confirmation_shows_the_estimate_before_anything_is_sent() {
        let a = Auftrag::Sprache(SprachAuftrag::new("Hallo Welt", "abc"), "Anna".into());
        assert!(a.schaetzung().label().contains("10 Credits"));
        let b = Auftrag::Bett(BettAuftrag {
            text: "x".into(),
            seconds: 60.0,
        });
        assert!(b.schaetzung().label().contains("0.150 $"));
    }
}
