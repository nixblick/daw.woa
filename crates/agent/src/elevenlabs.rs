//! Anbindung an ElevenLabs: Sprache (Text-to-Speech), Soundeffekte und Musikbetten mit dem
//! eigenen API-Schlüssel des Nutzers.
//!
//! Der Schlüssel liegt im Schlüsselbund (Eintrag [`KEY_ACCOUNT`] über `woa_secrets`), nie in
//! `config.json`, nie im Projekt, nie im Log und nie in einer Antwort an ein Modell. Ohne
//! Schlüssel ändert sich nichts: jede Anfrage endet vor dem Netz mit [`Fehler::KeinSchluessel`].
//!
//! Alle Aufrufe blockieren; sie gehören in einen Hintergrund-Thread (Oberfläche) oder in
//! `spawn_blocking` (MCP), nie in den Audio- oder UI-Thread.
//!
//! Endpunkte und Felder nach der offiziellen API-Doku, abgerufen am 02.11.2026 (die Seiten
//! tragen kein Datum und keine Versionsnummer):
//! - `POST /v1/text-to-speech/{voice_id}` – elevenlabs.io/docs/api-reference/text-to-speech/convert
//! - `POST /v1/sound-generation` – elevenlabs.io/docs/api-reference/text-to-sound-effects/convert
//! - `POST /v1/music` – elevenlabs.io/docs/api-reference/music/compose
//! - `GET /v1/user/subscription` – elevenlabs.io/docs/api-reference/user/subscription/get
//! - `GET /v2/voices` – elevenlabs.io/docs/api-reference/voices/search
//! - Fehlerform `{"detail": {"code": …}}` – elevenlabs.io/docs/developers/resources/errors
//! - Preise – elevenlabs.io/pricing/api
//!
//! Die Schnittstelle ist in zwei Traits geschnitten: [`Sprecher`] (Text → Sprache) kann später
//! auch ein lokaler Anbieter (Piper, `docs/klang-plan.md` B1) erfüllen; [`Klangerzeuger`]
//! (Effekt, Bett) bleibt ElevenLabs vorbehalten.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Eintrag im Schlüsselbund für den ElevenLabs-API-Schlüssel.
pub const KEY_ACCOUNT: &str = "elevenlabs";

/// Adresse der API.
pub const API: &str = "https://api.elevenlabs.io";

/// Ausgabeformat für alles: MP3 44,1 kHz 128 kbit/s. PCM, WAV und höhere Bitraten verlangen
/// laut Doku je nach Abo eine höhere Stufe; dieses Format geht mit jedem Konto.
pub const OUTPUT_FORMAT: &str = "mp3_44100_128";

/// Dateiendung zu [`OUTPUT_FORMAT`].
pub const EXT: &str = "mp3";

/// Größte Antwort mit Audio.
const MAX_AUDIO_BYTES: u64 = 64 * 1024 * 1024;

/// Effektdauer laut Doku: 0,5 bis 30 Sekunden.
pub const SFX_SECONDS: (f64, f64) = (0.5, 30.0);

/// Musiklänge laut Doku: 3 000 bis 600 000 ms.
pub const MUSIC_MS: (u32, u32) = (3_000, 600_000);

/// Höchstlänge eines Sprachtexts (eleven_v3: 5 000 Zeichen laut Modellübersicht).
pub const MAX_TTS_CHARS: usize = 5_000;

/// Hinweis für die Oberfläche: was an ElevenLabs geht.
pub const DATENSCHUTZ: &str = "Der eingegebene Text (Sprache, Effekt- oder Musikbeschreibung) geht \
     an ElevenLabs (USA) und wird dort verarbeitet. Nichts davon geht an WOA.";

/// Was schiefging, deutsch und ohne Schlüssel.
#[derive(Debug, Clone, PartialEq)]
pub enum Fehler {
    KeinSchluessel,
    /// HTTP 401: Schlüssel falsch, abgelaufen oder ohne Recht.
    SchluesselAbgelehnt,
    /// HTTP 402: Guthaben (Credits) aufgebraucht.
    Kontingent,
    /// HTTP 429: zu viele Anfragen oder zu viele gleichzeitig.
    ZuViele,
    /// HTTP 400/422: ElevenLabs lehnt die Eingabe ab.
    Eingabe(String),
    /// Nicht erreichbar (Netz, DNS, Zeitüberschreitung).
    Netz(String),
    /// Anderer HTTP-Status.
    Server(u16, String),
    /// Antwort nicht lesbar.
    Antwort(String),
}

impl std::fmt::Display for Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeinSchluessel => write!(
                f,
                "Kein ElevenLabs-Schlüssel. Leg deinen eigenen unter Einstellungen → KI → \
                 „ElevenLabs-Schlüssel“ ab."
            ),
            Self::SchluesselAbgelehnt => write!(
                f,
                "ElevenLabs lehnt den Schlüssel ab (HTTP 401). Bitte unter Einstellungen → KI \
                 neu eintragen."
            ),
            Self::Kontingent => write!(
                f,
                "ElevenLabs: Guthaben aufgebraucht (HTTP 402). Credits im ElevenLabs-Konto \
                 aufstocken oder bis zum nächsten Monat warten."
            ),
            Self::ZuViele => write!(
                f,
                "ElevenLabs: zu viele Anfragen gerade (HTTP 429). Kurz warten und noch einmal."
            ),
            Self::Eingabe(m) => write!(f, "ElevenLabs lehnt die Eingabe ab: {m}"),
            Self::Netz(m) => write!(f, "ElevenLabs nicht erreichbar: {m}"),
            Self::Server(s, m) if m.is_empty() => write!(f, "ElevenLabs: HTTP {s}"),
            Self::Server(s, m) => write!(f, "ElevenLabs: HTTP {s} ({m})"),
            Self::Antwort(m) => write!(f, "ElevenLabs: Antwort unlesbar ({m})"),
        }
    }
}

impl std::error::Error for Fehler {}

/// Kontostand aus `GET /v1/user/subscription`.
#[derive(Debug, Clone, PartialEq)]
pub struct Konto {
    pub tier: String,
    pub character_count: i64,
    pub character_limit: i64,
    pub reset_unix: Option<i64>,
}

impl Konto {
    /// Verbleibende Credits (Zeichen) im laufenden Abrechnungszeitraum.
    pub fn remaining(&self) -> i64 {
        (self.character_limit - self.character_count).max(0)
    }

    pub fn label(&self) -> String {
        format!(
            "{} von {} Credits übrig (Abo „{}“)",
            self.remaining(),
            self.character_limit,
            self.tier
        )
    }
}

/// Eine Stimme aus dem Konto.
#[derive(Debug, Clone, PartialEq)]
pub struct Stimme {
    pub id: String,
    pub name: String,
    pub category: String,
}

/// Sprachmodell für Text-to-Speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TtsModell {
    /// `eleven_multilingual_v2`: Vorgabe der API, 29 Sprachen inkl. Deutsch. Kennt laut Doku
    /// kein `language_code`; die Sprache folgt dem Text.
    #[default]
    MultilingualV2,
    /// `eleven_v3`: ausdrucksstärker, 70+ Sprachen, `language_code` erlaubt, höchstens 5 000 Zeichen.
    V3,
}

impl TtsModell {
    pub const ALL: [TtsModell; 2] = [TtsModell::MultilingualV2, TtsModell::V3];

    pub fn id(self) -> &'static str {
        match self {
            Self::MultilingualV2 => "eleven_multilingual_v2",
            Self::V3 => "eleven_v3",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MultilingualV2 => "Multilingual v2 (ruhig, verlässlich)",
            Self::V3 => "v3 (ausdrucksstark)",
        }
    }

    /// Darf `language_code` mit?
    pub fn takes_language(self) -> bool {
        matches!(self, Self::V3)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
}

/// Auftrag Text → Sprache.
#[derive(Debug, Clone, PartialEq)]
pub struct SprachAuftrag {
    pub text: String,
    pub voice_id: String,
    pub model: TtsModell,
    /// 0..1, Vorgabe 0,5.
    pub stability: f64,
    /// 0..1, Vorgabe 0.
    pub style: f64,
    /// Ähnlichkeit zur Originalstimme, 0..1, Vorgabe 0,75 (JingleVoice Fassung 7).
    pub similarity: f64,
    /// Sprache (ISO 639-1), nur für Modelle mit [`TtsModell::takes_language`].
    pub language: String,
}

impl SprachAuftrag {
    pub fn new(text: impl Into<String>, voice_id: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            voice_id: voice_id.into(),
            model: TtsModell::default(),
            stability: 0.5,
            style: 0.0,
            similarity: 0.75,
            language: "de".into(),
        }
    }
}

/// Auftrag Text → Soundeffekt.
#[derive(Debug, Clone, PartialEq)]
pub struct EffektAuftrag {
    pub text: String,
    /// `None`: Länge bestimmt ElevenLabs aus dem Text.
    pub seconds: Option<f64>,
}

/// Auftrag Text → Musikbett (immer instrumental).
#[derive(Debug, Clone, PartialEq)]
pub struct BettAuftrag {
    pub text: String,
    pub seconds: f64,
}

/// Erzeugtes Audio.
#[derive(Clone, PartialEq)]
pub struct Audio {
    pub bytes: Vec<u8>,
    pub ext: &'static str,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Audio({} Bytes .{})", self.bytes.len(), self.ext)
    }
}

/// Text → Sprache. ElevenLabs heute, ein lokaler Anbieter (Piper) später.
pub trait Sprecher: Send + Sync {
    /// Anzeigename des Anbieters (steht auch in der Herkunft).
    fn anbieter(&self) -> &'static str;
    fn stimmen(&self) -> Result<Vec<Stimme>, Fehler>;
    fn sprechen(&self, a: &SprachAuftrag) -> Result<Audio, Fehler>;
}

/// Text → Soundeffekt und Musikbett.
pub trait Klangerzeuger: Send + Sync {
    fn effekt(&self, a: &EffektAuftrag) -> Result<Audio, Fehler>;
    fn bett(&self, a: &BettAuftrag) -> Result<Audio, Fehler>;
}

/// Zugang zur API: Adresse und Schlüssel-Quelle. Tests setzen eine lokale Adresse und einen
/// festen Schlüssel, damit sie weder Netz noch Schlüsselbund anfassen. Kein `Debug`: der
/// Schlüssel soll nie in einer Ausgabe landen können.
#[derive(Clone)]
pub struct Source {
    pub api: String,
    pub key: Arc<dyn Fn() -> Option<String> + Send + Sync>,
}

impl Default for Source {
    fn default() -> Self {
        Self::from_data_dir(woa_radio::data_dir())
    }
}

impl Source {
    /// Schlüssel aus dem Schlüsselbund (Rückfall: geschützte Datei in `data_dir`).
    pub fn from_data_dir(data_dir: PathBuf) -> Self {
        Self {
            api: API.to_string(),
            key: Arc::new(move || {
                woa_secrets::reveal(&data_dir, KEY_ACCOUNT)
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
            }),
        }
    }

    /// Fester Schlüssel (zum Prüfen eines frisch eingegebenen, noch nicht gespeicherten).
    pub fn with_key(api: &str, key: String) -> Self {
        Self {
            api: api.to_string(),
            key: Arc::new(move || Some(key.clone())),
        }
    }

    /// Ist ein Schlüssel hinterlegt?
    pub fn has_key(&self) -> bool {
        (self.key)().is_some()
    }

    fn key(&self) -> Result<String, Fehler> {
        (self.key)().ok_or(Fehler::KeinSchluessel)
    }

    /// Kontostand (prüft zugleich den Schlüssel).
    pub fn konto(&self) -> Result<Konto, Fehler> {
        let v = self.get_json("/v1/user/subscription", &[])?;
        parse_konto(&v).ok_or_else(|| Fehler::Antwort("Felder fehlen".into()))
    }

    fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, Fehler> {
        let key = self.key()?;
        let url = format!("{}{path}", self.api);
        let mut req = http(&url)?.get(&url).header("xi-api-key", &key);
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let mut resp = req.call().map_err(|e| net_error(&e, &key))?;
        check(&mut resp, &key)?;
        resp.body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_json()
            .map_err(|e| Fehler::Antwort(scrub(&e.to_string(), &key)))
    }

    fn post_audio(&self, path: &str, body: &serde_json::Value) -> Result<Audio, Fehler> {
        let key = self.key()?;
        let url = format!("{}{path}", self.api);
        let mut resp = http(&url)?
            .post(&url)
            .header("xi-api-key", &key)
            .header("Content-Type", "application/json")
            .header("Accept", "audio/mpeg")
            .query("output_format", OUTPUT_FORMAT)
            .send(body.to_string())
            .map_err(|e| net_error(&e, &key))?;
        check(&mut resp, &key)?;
        let mut bytes = Vec::new();
        resp.body_mut()
            .with_config()
            .limit(MAX_AUDIO_BYTES)
            .reader()
            .read_to_end(&mut bytes)
            .map_err(|e| Fehler::Netz(scrub(&e.to_string(), &key)))?;
        if bytes.is_empty() {
            return Err(Fehler::Antwort("kein Audio".into()));
        }
        Ok(Audio { bytes, ext: EXT })
    }
}

impl Sprecher for Source {
    fn anbieter(&self) -> &'static str {
        "ElevenLabs"
    }

    /// Alle Stimmen des Kontos, seitenweise (`has_more`/`next_page_token`), höchstens 500.
    fn stimmen(&self) -> Result<Vec<Stimme>, Fehler> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..5 {
            let mut q = vec![("page_size", "100"), ("include_total_count", "false")];
            if let Some(t) = token.as_deref() {
                q.push(("next_page_token", t));
            }
            let v = self.get_json("/v2/voices", &q)?;
            out.extend(parse_voices(&v));
            let more = v.get("has_more").and_then(serde_json::Value::as_bool) == Some(true);
            token = v
                .get("next_page_token")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            if !more || token.is_none() {
                break;
            }
        }
        Ok(out)
    }

    fn sprechen(&self, a: &SprachAuftrag) -> Result<Audio, Fehler> {
        let text = a.text.trim();
        if text.is_empty() {
            return Err(Fehler::Eingabe("Text ist leer".into()));
        }
        if text.chars().count() > MAX_TTS_CHARS {
            return Err(Fehler::Eingabe(format!(
                "Text ist länger als {MAX_TTS_CHARS} Zeichen"
            )));
        }
        if !valid_voice_id(&a.voice_id) {
            return Err(Fehler::Eingabe("keine gültige Stimme gewählt".into()));
        }
        let body = tts_body(a);
        self.post_audio(&format!("/v1/text-to-speech/{}", a.voice_id), &body)
    }
}

impl Klangerzeuger for Source {
    fn effekt(&self, a: &EffektAuftrag) -> Result<Audio, Fehler> {
        if a.text.trim().is_empty() {
            return Err(Fehler::Eingabe("Beschreibung ist leer".into()));
        }
        self.post_audio("/v1/sound-generation", &sfx_body(a))
    }

    fn bett(&self, a: &BettAuftrag) -> Result<Audio, Fehler> {
        if a.text.trim().is_empty() {
            return Err(Fehler::Eingabe("Beschreibung ist leer".into()));
        }
        self.post_audio("/v1/music", &music_body(a))
    }
}

/// Körper für `POST /v1/text-to-speech/{voice_id}`.
pub fn tts_body(a: &SprachAuftrag) -> serde_json::Value {
    let mut body = serde_json::json!({
        "text": a.text.trim(),
        "model_id": a.model.id(),
        "voice_settings": {
            "stability": a.stability.clamp(0.0, 1.0),
            "similarity_boost": a.similarity.clamp(0.0, 1.0),
            "style": a.style.clamp(0.0, 1.0),
        },
    });
    if a.model.takes_language() && !a.language.is_empty() {
        body["language_code"] = a.language.clone().into();
    }
    body
}

/// Körper für `POST /v1/sound-generation` (Modell `eleven_text_to_sound_v2`, die Vorgabe;
/// v3 ist laut Doku nur eingeschränkt zugänglich).
pub fn sfx_body(a: &EffektAuftrag) -> serde_json::Value {
    let mut body = serde_json::json!({
        "text": a.text.trim(),
        "model_id": "eleven_text_to_sound_v2",
    });
    if let Some(s) = a.seconds.filter(|s| s.is_finite()) {
        body["duration_seconds"] = s.clamp(SFX_SECONDS.0, SFX_SECONDS.1).into();
    }
    body
}

/// Körper für `POST /v1/music`: immer instrumental, Länge geklemmt.
pub fn music_body(a: &BettAuftrag) -> serde_json::Value {
    serde_json::json!({
        "prompt": a.text.trim(),
        "music_length_ms": music_ms(a.seconds),
        "force_instrumental": true,
    })
}

/// Sekunden → `music_length_ms` im erlaubten Bereich.
pub fn music_ms(seconds: f64) -> u32 {
    let ms = if seconds.is_finite() {
        (seconds * 1000.0).round().max(0.0) as u64
    } else {
        0
    };
    (ms.min(u64::from(u32::MAX)) as u32).clamp(MUSIC_MS.0, MUSIC_MS.1)
}

/// Stimmen-Kennungen sind kurze alphanumerische Zeichenketten; nichts anderes kommt in den Pfad.
pub fn valid_voice_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub fn parse_konto(v: &serde_json::Value) -> Option<Konto> {
    Some(Konto {
        tier: v.get("tier")?.as_str()?.to_string(),
        character_count: v.get("character_count")?.as_i64()?,
        character_limit: v.get("character_limit")?.as_i64()?,
        reset_unix: v
            .get("next_character_count_reset_unix")
            .and_then(serde_json::Value::as_i64),
    })
}

pub fn parse_voices(v: &serde_json::Value) -> Vec<Stimme> {
    v.get("voices")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let id = s.get("voice_id")?.as_str()?.to_string();
                    valid_voice_id(&id).then(|| Stimme {
                        name: s
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&id)
                            .to_string(),
                        category: s
                            .get("category")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Sucht eine Stimme nach Kennung oder Name (ohne Groß/klein).
pub fn find_voice<'a>(list: &'a [Stimme], wanted: &str) -> Option<&'a Stimme> {
    let w = wanted.trim();
    list.iter()
        .find(|s| s.id == w)
        .or_else(|| list.iter().find(|s| s.name.eq_ignore_ascii_case(w)))
}

/// Was eine Erzeugung ungefähr kostet.
#[derive(Debug, Clone, PartialEq)]
pub struct Schaetzung {
    /// Text-to-Speech: Zeichen (= Credits im Abo, so zählt `character_count`).
    pub credits: Option<u64>,
    /// Nach der API-Preisliste vom 02.11.2026 in US-Dollar.
    pub usd: f64,
}

impl Schaetzung {
    pub fn label(&self) -> String {
        match self.credits {
            Some(c) => format!("≈ {c} Credits (Zeichen), API-Preis ≈ {:.3} $", self.usd),
            None => format!(
                "API-Preis ≈ {:.3} $ (Schätzung; Credits je Erzeugung nennt ElevenLabs nicht)",
                self.usd
            ),
        }
    }
}

/// TTS: 0,08 $ je 1 000 Zeichen (v2 Multilingual und v3, Preisliste 02.11.2026).
pub fn schaetzung_sprache(text: &str) -> Schaetzung {
    let n = text.trim().chars().count() as u64;
    Schaetzung {
        credits: Some(n),
        usd: n as f64 * 0.08 / 1000.0,
    }
}

/// Soundeffekt: 0,12 $ je Minute; ohne Länge mit der Höchstdauer gerechnet.
pub fn schaetzung_effekt(seconds: Option<f64>) -> Schaetzung {
    let s = seconds
        .filter(|s| s.is_finite())
        .map_or(SFX_SECONDS.1, |s| s.clamp(SFX_SECONDS.0, SFX_SECONDS.1));
    Schaetzung {
        credits: None,
        usd: s / 60.0 * 0.12,
    }
}

/// Musik: 0,15 $ je Minute.
pub fn schaetzung_bett(seconds: f64) -> Schaetzung {
    let s = f64::from(music_ms(seconds)) / 1000.0;
    Schaetzung {
        credits: None,
        usd: s / 60.0 * 0.15,
    }
}

/// Art der Erzeugung (für Dateinamen, Bibliothek und Herkunft).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Sprache,
    Effekt,
    Bett,
}

impl Art {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sprache => "sprache",
            Self::Effekt => "effekt",
            Self::Bett => "bett",
        }
    }

    pub fn library_kind(self) -> woa_library::Kind {
        match self {
            Self::Sprache | Self::Effekt => woa_library::Kind::Sample,
            Self::Bett => woa_library::Kind::Music,
        }
    }
}

/// Herkunft eines erzeugten Klangs; steht als `<datei>.herkunft.json` daneben.
#[derive(Debug, Clone, PartialEq)]
pub struct Herkunft {
    pub anbieter: String,
    pub art: Art,
    pub text: String,
    /// Stimme (Name) oder leer.
    pub stimme: String,
    pub modell: String,
    pub erzeugt_unix: u64,
}

impl Herkunft {
    pub fn new(anbieter: &str, art: Art, text: &str) -> Self {
        Self {
            anbieter: anbieter.to_string(),
            art,
            text: text.trim().to_string(),
            stimme: String::new(),
            modell: String::new(),
            erzeugt_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "anbieter": self.anbieter,
            "art": self.art.as_str(),
            "text": self.text,
            "stimme": self.stimme,
            "modell": self.modell,
            "erzeugt_unix": self.erzeugt_unix,
        })
    }
}

/// Pfad der Herkunftsdatei zu `audio`.
pub fn herkunft_path(audio: &Path) -> PathBuf {
    let mut s = audio.as_os_str().to_owned();
    s.push(".herkunft.json");
    PathBuf::from(s)
}

/// Kurzer Dateistamm aus Art und Text: `elevenlabs-effekt-rakete-startet`.
pub fn stem(art: Art, text: &str) -> String {
    let mut words = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            'ß' => 's',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            words.push(c);
        } else if !words.ends_with('-') && !words.is_empty() {
            words.push('-');
        }
        if words.len() >= 32 {
            break;
        }
    }
    let words = words.trim_end_matches('-');
    if words.is_empty() {
        format!("elevenlabs-{}", art.as_str())
    } else {
        format!("elevenlabs-{}-{words}", art.as_str())
    }
}

/// Schreibt `audio` als neue Datei `<stamm>[-n].<ext>` in `dir` samt Herkunftsdatei.
/// Erst unter Zwischennamen, dann umbenannt: nie eine halbe Datei.
pub fn ablegen(audio: &Audio, dir: &Path, stem: &str, h: &Herkunft) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut path = dir.join(format!("{stem}.{}", audio.ext));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{}", audio.ext));
        n += 1;
    }
    let part = path.with_extension(format!("{}.part", audio.ext));
    let r = std::fs::write(&part, &audio.bytes)
        .and_then(|()| std::fs::rename(&part, &path))
        .map_err(|e| format!("{}: {e}", path.display()));
    if let Err(e) = r {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let meta = serde_json::to_string_pretty(&h.json()).unwrap_or_default();
    std::fs::write(herkunft_path(&path), meta)
        .map_err(|e| format!("{}: {e}", herkunft_path(&path).display()))?;
    Ok(path)
}

/// Kopiert eine abgelegte Datei in den Medienordner (Jingle-Ordner), trägt sie in die
/// Bibliothek ein (Art je nach Erzeugung, Schlagworte `elevenlabs` und die Art) und gibt
/// Pfad und Bibliotheksnummer zurück.
pub fn in_bibliothek(
    file: &Path,
    media_dir: &Path,
    db: &Path,
    h: &Herkunft,
) -> Result<(PathBuf, i64), String> {
    let name = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("elevenlabs");
    let audio = Audio {
        bytes: std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?,
        ext: EXT,
    };
    let path = ablegen(&audio, media_dir, name, h)?;
    let lib = woa_library::Library::open(db).map_err(|e| e.to_string())?;
    let new = woa_library::import::probe(&path, h.art.library_kind())?;
    let id = match lib.add_track(&new).map_err(|e| e.to_string())? {
        woa_library::AddOutcome::Added(id)
        | woa_library::AddOutcome::Updated(id)
        | woa_library::AddOutcome::Duplicate(id) => id,
    };
    for tag in ["elevenlabs", h.art.as_str()] {
        lib.add_tag(id, tag).map_err(|e| e.to_string())?;
    }
    Ok((path, id))
}

/// Wohin erzeugte Klänge zusätzlich zum Projekt gehen: Medienordner und Bibliothek.
#[derive(Debug, Clone, PartialEq)]
pub struct Ablage {
    pub media_dir: PathBuf,
    pub db: PathBuf,
}

impl Ablage {
    /// Jingle-Ordner aus den Einstellungen und die Bibliothek im Datenordner.
    pub fn standard() -> Self {
        Self {
            media_dir: woa_radio::ordner::configured_dir(woa_radio::ordner::Art::Jingles),
            db: woa_radio::data_dir().join(woa_library::DB_FILE),
        }
    }
}

/// Nur https, außer für die lokale Testadresse.
fn local(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

fn http(url: &str) -> Result<ureq::Agent, Fehler> {
    if !url.starts_with("https://") && !local(url) {
        return Err(Fehler::Netz("nur https erlaubt".into()));
    }
    Ok(ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        // Musik kann dauern.
        .timeout_global(Some(Duration::from_secs(300)))
        .https_only(!local(url))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into())
}

/// Entfernt den Schlüssel aus einem Text, falls er doch darin steht.
fn scrub(text: &str, key: &str) -> String {
    if key.len() >= 4 {
        text.replace(key, "***")
    } else {
        text.to_string()
    }
}

fn net_error(e: &ureq::Error, key: &str) -> Fehler {
    Fehler::Netz(scrub(&e.to_string(), key))
}

/// Liest den Fehlercode aus `detail` (Objekt mit `code`, älter `status`; bei 422 eine Liste
/// mit `msg`).
pub fn detail_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return String::new();
    };
    let d = &v["detail"];
    if let Some(list) = d.as_array() {
        return list
            .iter()
            .filter_map(|e| e.get("msg").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("; ");
    }
    if let Some(s) = d.as_str() {
        return s.to_string();
    }
    let code = d
        .get("code")
        .or_else(|| d.get("status"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let msg = d
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match (code.is_empty(), msg.is_empty()) {
        (true, _) => msg.to_string(),
        (false, true) => code.to_string(),
        (false, false) => format!("{code}: {msg}"),
    }
}

fn check(resp: &mut ureq::http::Response<ureq::Body>, key: &str) -> Result<(), Fehler> {
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let body = resp
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .unwrap_or_default();
    let detail = scrub(&detail_message(&body), key);
    let code_quota = detail.contains("quota_exceeded") || detail.contains("insufficient_credits");
    Err(match status {
        401 if code_quota => Fehler::Kontingent,
        401 | 403 => Fehler::SchluesselAbgelehnt,
        402 => Fehler::Kontingent,
        429 => Fehler::ZuViele,
        400 | 422 => Fehler::Eingabe(if detail.is_empty() {
            format!("HTTP {status}")
        } else {
            detail
        }),
        s => Fehler::Server(s, detail),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_code_only_for_v3() {
        let mut a = SprachAuftrag::new("Hallo", "abc123");
        assert!(tts_body(&a).get("language_code").is_none());
        a.model = TtsModell::V3;
        assert_eq!(tts_body(&a)["language_code"], "de");
        assert_eq!(tts_body(&a)["model_id"], "eleven_v3");
        assert_eq!(tts_body(&a)["voice_settings"]["similarity_boost"], 0.75);
        a.similarity = 2.0;
        assert_eq!(
            tts_body(&a)["voice_settings"]["similarity_boost"],
            1.0,
            "auf 0..1 begrenzt"
        );
    }

    #[test]
    fn ranges_are_clamped() {
        let b = sfx_body(&EffektAuftrag {
            text: "Böller".into(),
            seconds: Some(99.0),
        });
        assert_eq!(b["duration_seconds"], 30.0);
        let b = sfx_body(&EffektAuftrag {
            text: "x".into(),
            seconds: Some(0.1),
        });
        assert_eq!(b["duration_seconds"], 0.5);
        assert!(
            sfx_body(&EffektAuftrag {
                text: "x".into(),
                seconds: None
            })
            .get("duration_seconds")
            .is_none()
        );
        assert_eq!(music_ms(1.0), 3_000);
        assert_eq!(music_ms(15.0), 15_000);
        assert_eq!(music_ms(10_000.0), 600_000);
        assert_eq!(
            music_body(&BettAuftrag {
                text: "x".into(),
                seconds: 20.0
            })["force_instrumental"],
            true
        );
    }

    #[test]
    fn detail_shapes() {
        assert_eq!(
            detail_message(r#"{"detail":{"code":"invalid_api_key","message":"bad"}}"#),
            "invalid_api_key: bad"
        );
        assert_eq!(
            detail_message(r#"{"detail":{"status":"quota_exceeded"}}"#),
            "quota_exceeded"
        );
        assert_eq!(
            detail_message(
                r#"{"detail":[{"loc":["body","text"],"msg":"field required","type":"x"}]}"#
            ),
            "field required"
        );
        assert_eq!(detail_message("kein json"), "");
    }

    #[test]
    fn voice_ids_cannot_escape_the_path() {
        assert!(valid_voice_id("21m00Tcm4TlvDq8ikWAM"));
        assert!(!valid_voice_id("../user"));
        assert!(!valid_voice_id("a?b"));
        assert!(!valid_voice_id(""));
    }

    #[test]
    fn stems_are_file_safe() {
        assert_eq!(
            stem(Art::Effekt, "Rakete, die startet!"),
            "elevenlabs-effekt-rakete-die-startet"
        );
        assert_eq!(stem(Art::Sprache, "  "), "elevenlabs-sprache");
        assert_eq!(stem(Art::Bett, "Größe"), "elevenlabs-bett-grose");
    }

    #[test]
    fn estimates() {
        assert_eq!(schaetzung_sprache("Hallo Welt").credits, Some(10));
        assert!((schaetzung_effekt(Some(5.0)).usd - 0.01).abs() < 1e-9);
        assert!((schaetzung_bett(60.0).usd - 0.15).abs() < 1e-9);
    }
}
