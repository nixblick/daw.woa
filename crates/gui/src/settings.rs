//! Einstellungen (ScreenSettings + SettingsFilter, Design Fassung 7; Gliederung André
//! 03.11.2026): ein Fenster (Strg+,) mit sieben festen Bereichen. Eine Liste ([`REGISTRY`])
//! speist Fenster, Filter und die Abkürzungen im Menü „Einstellungen“.
//!
//! **Einsortier-Regel** (auch in `CLAUDE.md` und `docs/design/umsetzung.md`): betrifft eine
//! Einstellung nur eine Ansicht, gehört sie in deren Bereich (Radio, Party, Produzent); braucht
//! sie ein fremdes Konto oder einen Schlüssel, gehört sie unter „Dienste und Konten“ in die
//! Gruppe ihres Zwecks (fehlt der Zweck, kommt eine neue Gruppe in [`SERVICE_GROUPS`]); sonst
//! nach Allgemein bzw. Audio und Geräte. Eine neue Einstellung ist ein Eintrag hier, kein Umbau.

/// Bereich links. Reihenfolge = Anzeige.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    Allgemein,
    Audio,
    Radio,
    Party,
    Produzent,
    Dienste,
    /// Nur im Entwickler-Modus sichtbar.
    Entwickler,
}

impl Area {
    pub const ALL: [Area; 7] = [
        Area::Allgemein,
        Area::Audio,
        Area::Radio,
        Area::Party,
        Area::Produzent,
        Area::Dienste,
        Area::Entwickler,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Area::Allgemein => "Allgemein",
            Area::Audio => "Audio und Geräte",
            Area::Radio => "Radio",
            Area::Party => "Party",
            Area::Produzent => "Produzent",
            Area::Dienste => "Dienste und Konten",
            Area::Entwickler => "Entwickler",
        }
    }

    pub fn icon(self) -> &'static str {
        use egui_phosphor::regular as ic;
        match self {
            Area::Allgemein => ic::GEAR_SIX,
            Area::Audio => ic::SPEAKER_HIGH,
            Area::Radio => ic::RADIO,
            Area::Party => ic::CONFETTI,
            Area::Produzent => ic::PIANO_KEYS,
            Area::Dienste => ic::PLUGS_CONNECTED,
            Area::Entwickler => ic::CODE,
        }
    }

    /// Nur im Entwickler-Modus zu sehen.
    pub fn dev_only(self) -> bool {
        self == Area::Entwickler
    }

    /// Bereiche, die man mit diesem Entwickler-Stand sieht.
    pub fn visible(dev: bool) -> impl Iterator<Item = Area> {
        Area::ALL.into_iter().filter(move |a| dev || !a.dev_only())
    }
}

/// Gruppen unter „Dienste und Konten“, nach Zweck (nicht nach Technik).
pub const SERVICE_GROUPS: [&str; 4] = [
    "Assistent",
    "Stimmen und Klänge",
    "Steuerung von außen",
    "Sendung",
];

/// Kleine Technik-Marke auf einer Dienst-Karte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Api,
    Mcp,
}

impl Mark {
    pub fn label(self) -> &'static str {
        match self {
            Mark::Api => "API",
            Mark::Mcp => "MCP",
        }
    }
}

/// Zeile (Einstellung) oder Karte (Dienst mit Status).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Row,
    Card(Option<Mark>),
}

/// Was eine Zeile bedient (der Aufrufer zeichnet das Bedienelement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingId {
    // Allgemein
    Theme,
    VuView,
    Folders,
    AutoUpdate,
    CheckUpdates,
    Telemetry,
    Developer,
    // Audio und Geräte
    Outputs,
    ReconnectOutput,
    Pfl,
    Mic,
    Phone,
    MidiInput,
    Controls,
    // Radio
    Stream,
    Pult,
    Crossfade,
    Ducking,
    DuckTimes,
    CartDuck,
    StopFade,
    FadeLength,
    BusProcessing,
    MicChain,
    WishFolder,
    // Party
    DjCurve,
    PartyPanels,
    // Produzent
    MonitorVolume,
    SoundBanks,
    PianoRollWindow,
    Plugins,
    // Dienste und Konten
    /// Index in `ProviderConfig::presets()`.
    Provider(u8),
    Assistant,
    Tools,
    ElevenLabs,
    Freesound,
    McpLive,
    McpHttp,
    StreamRef,
    // Entwickler
    Abnahme,
    Fragen,
    DevEnd,
}

/// Eine Einstellung: Bereich, Gruppe, Titel (wie im Menü), Beschreibung, Suchwörter
/// (Umgangssprache und Synonyme), Menü-Abkürzung, die `GuiConfig`-Felder, die sie bedient.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub id: SettingId,
    pub area: Area,
    pub group: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub keys: &'static str,
    /// Steht als Abkürzung im Menü „Einstellungen“.
    pub menu: bool,
    pub fields: &'static [&'static str],
    pub kind: Kind,
}

const fn row(
    id: SettingId,
    area: Area,
    group: &'static str,
    name: &'static str,
    desc: &'static str,
    keys: &'static str,
) -> Entry {
    Entry {
        id,
        area,
        group,
        name,
        desc,
        keys,
        menu: false,
        fields: &[],
        kind: Kind::Row,
    }
}

impl Entry {
    const fn menu(mut self) -> Self {
        self.menu = true;
        self
    }
    const fn fields(mut self, f: &'static [&'static str]) -> Self {
        self.fields = f;
        self
    }
    const fn card(mut self, m: Option<Mark>) -> Self {
        self.kind = Kind::Card(m);
        self
    }
}

use Area as A;
use SettingId as S;

/// Alle Einstellungen, nach Bereich geordnet (die Reihenfolge entscheidet den ersten Treffer).
pub const REGISTRY: &[Entry] = &[
    // ── Allgemein
    row(
        S::Theme,
        A::Allgemein,
        "Erscheinungsbild",
        "Erscheinungsbild",
        "Hell oder Dunkel; die Akzentfarbe folgt der Ansicht",
        "thema farbe hell dunkel dark mode light aussehen design",
    )
    .fields(&["theme"]),
    row(
        S::VuView,
        A::Allgemein,
        "Erscheinungsbild",
        "Pegelansicht (Analyse-Fenster)",
        "Analog (VU), Modern (Balken) oder aus",
        "pegel vu meter balken analog anzeige",
    )
    .fields(&["vu_view"]),
    row(
        S::Folders,
        A::Allgemein,
        "Speicherorte",
        "Speicherorte",
        "Wo WOA Projekte, Mitschnitte und Exporte ablegt",
        "ordner pfad projekte mitschnitt export ablage verzeichnis",
    )
    .menu()
    .fields(&["ordner"]),
    row(
        S::AutoUpdate,
        A::Allgemein,
        "Updates",
        "Beim Start nach Updates suchen (einmal am Tag)",
        "Sucht beim Start höchstens einmal am Tag nach einer neuen Version",
        "update aktualisieren neue version start",
    )
    .fields(&["auto_update"]),
    row(
        S::CheckUpdates,
        A::Allgemein,
        "Updates",
        "Jetzt nach Updates suchen",
        "Sofort nach einer neuen Version suchen",
        "update aktualisieren neue version",
    ),
    row(
        S::Telemetry,
        A::Allgemein,
        "Datenschutz",
        "Fehlercodes anonym senden (ab nächstem Start)",
        "Nur Fehlercodes, keine Inhalte; ab dem nächsten Start",
        "telemetrie fehler anonym datenschutz privatsphäre tracking",
    )
    .menu()
    .fields(&["telemetry_enabled"]),
    row(
        S::Developer,
        A::Allgemein,
        "Entwickler",
        "Entwickler-Modus",
        "Für alle, die mitentwickeln: per Kennwort freischalten",
        "entwickler kennwort abnahme fragen developer",
    )
    .menu()
    .fields(&["entwickler"]),
    // ── Audio und Geräte
    row(
        S::Outputs,
        A::Audio,
        "Soundkarten",
        "Audio-Geräte",
        "Der eine Ort für Soundkarten: Ausgänge, Vorhören, Mikrofon, Telefon und Pult",
        "ausgang eingang gerät kopfhörer lautsprecher audio interface pult soundkarte boxen \
         mikro mikrofon mic quelle externes mikro usb codec telefon pfl",
    )
    .menu()
    .fields(&["outputs"]),
    row(
        S::ReconnectOutput,
        A::Audio,
        "Soundkarten",
        "Ausgabe neu verbinden",
        "Öffnet alle Ausgänge neu auf dem aktuellen Standardgerät, z. B. nach dem Einstecken der Kopfhörer",
        "kopfhörer neu verbinden ton weg kein ton",
    )
    .menu(),
    row(
        S::Mic,
        A::Audio,
        "Soundkarten",
        "Mikrofon",
        "Welches Mikro auf Sendung geht; gewählt in Audio-Geräte",
        "mikro mikrofon mic eingang quelle extern externes usb sprechen",
    )
    .fields(&["mic"]),
    row(
        S::Pfl,
        A::Audio,
        "Soundkarten",
        "Vorhören (PFL)",
        "Kopfhörer zum Vorhören; gewählt in Audio-Geräte",
        "pfl cue vorhören kopfhörer",
    )
    .fields(&["pfl_output"]),
    row(
        S::Phone,
        A::Audio,
        "Soundkarten",
        "Telefon (Eingang für den Anrufer)",
        "Eingang für Anrufer in der Sendung; gewählt in Audio-Geräte",
        "anruf telefon hybrid gast anrufer",
    )
    .fields(&["phone"]),
    row(
        S::MidiInput,
        A::Audio,
        "MIDI-Controller",
        "MIDI-Eingang",
        "Gerät, das die scharf geschaltete MIDI-Spur spielt",
        "keyboard klaviatur gerät midi",
    )
    .menu()
    .fields(&["midi_input"]),
    row(
        S::Controls,
        A::Audio,
        "MIDI-Controller",
        "Controller und MIDI-Learn",
        "Tasten und Regler eines Controllers zuordnen",
        "controller learn fader tasten zuordnen midi",
    )
    .menu()
    .fields(&["controller_input", "controller_maps"]),
    // ── Radio
    row(
        S::Stream,
        A::Radio,
        "Sendung",
        "Sendung: Stream-Server und Kennwort",
        "Shoutcast oder Icecast: Adresse, Port, Kennwort, Verbindung testen",
        "stream shoutcast icecast server port senden streaming",
    )
    .menu()
    .fields(&["stream"]),
    row(
        S::Pult,
        A::Radio,
        "Sendung",
        "Pult-Modus",
        "Die Sendung kommt vom Eingang des externen Mischpults; gewählt in Audio-Geräte",
        "pult mischpult extern mixer",
    )
    .fields(&["pult"]),
    row(
        S::Crossfade,
        A::Radio,
        "Übergänge (Rack im Radio)",
        "Crossfade-Blende",
        "Länge der Überblendung zwischen Titeln",
        "crossfade blende überblendung übergang fade",
    ),
    row(
        S::StopFade,
        A::Radio,
        "Übergänge (Rack im Radio)",
        "Stopp blendet aus",
        "Stopp an Sende-Player und Decks blendet über die eingestellte Dauer aus",
        "stopp ausblenden fade decks",
    ),
    row(
        S::FadeLength,
        A::Radio,
        "Übergänge (Rack im Radio)",
        "Fade-Taste: Dauer",
        "Wie lange die Fade-Tasten in Sende-Deck, Decks und Carts ausblenden (1 bis 10 s)",
        "fade ausblenden dauer sekunden runterregeln leiser machen",
    ),
    row(
        S::Ducking,
        A::Radio,
        "Ducking",
        "Musik-Ducking",
        "Absenkung der Musik, wenn das Mikro offen ist",
        "ducking mikro leiser absenken",
    ),
    row(
        S::DuckTimes,
        A::Radio,
        "Ducking",
        "Ducking: Rückkehr und Einblenden",
        "Musik kommt nach so viel Stille im Mikro zurück (Vorgabe 0,5 s) und blendet so lange ein (1 s); Decks und Carts werden mit abgesenkt",
        "ducking stille rückkehr einblenden release halten sprechpause",
    ),
    row(
        S::CartDuck,
        A::Radio,
        "Ducking",
        "Ducking auch bei Carts",
        "Musik leiser, solange ein Cart spielt",
        "ducking cart jingle",
    ),
    row(
        S::BusProcessing,
        A::Radio,
        "Processing",
        "Sendebus-Processing",
        "Lautheitsregelung, Kompressor und Limiter auf der Sendung",
        "processing lautheit lufs kompressor limiter",
    ),
    row(
        S::MicChain,
        A::Radio,
        "Processing",
        "Mikro-Kette",
        "Hochpass, Präsenz, Gate und Kompressor fürs Mikro",
        "mikro mikrofon gate kompressor processing",
    ),
    row(
        S::WishFolder,
        A::Radio,
        "Wunschtitel",
        "Wunschordner",
        "Ordner, aus dem Wunschtitel kommen (im Radio bei den Wunschtiteln)",
        "wunsch wunschtitel ordner hörer",
    )
    .fields(&["wish_folder"]),
    // ── Party
    row(
        S::DjCurve,
        A::Party,
        "DJ",
        "Übergang weich oder hart",
        "Kurve des Crossfaders im DJ-Mixer (unter dem Crossfader)",
        "dj crossfader kurve weich hart übergang crossfade",
    ),
    row(
        S::PartyPanels,
        A::Party,
        "Ansicht",
        "Wunschliste und Fernbedienung zeigen",
        "Karten rechts in der Party ein- oder ausblenden; die Bibliothek nimmt den freien Platz. \
         Eine laufende Fernbedienung bleibt an",
        "wunsch wunschliste fernbedienung handy qr ausblenden einblenden verstecken platz \
         bibliothek größer ansicht",
    )
    .fields(&["party_panels"]),
    // ── Produzent
    row(
        S::MonitorVolume,
        A::Produzent,
        "Abhören",
        "Abhörlautstärke",
        "Regler neben dem PEGEL in der Transportleiste; nur der Lautsprecher, Mix und Export \
         bleiben gleich",
        "lautstärke abhören leiser lauter monitor lautsprecher volume regler",
    )
    .fields(&["monitor_volume"]),
    row(
        S::SoundBanks,
        A::Produzent,
        "Klänge und Bänke",
        "Klangbank je Spur",
        "Im Rack der MIDI-Spur; GeneralUser GS ist eingebaut, weitere Bänke kommen beim ersten Gebrauch",
        "klang bank soundfont instrument sound",
    ),
    row(
        S::PianoRollWindow,
        A::Produzent,
        "Piano-Roll",
        "Piano-Roll als eigenes Fenster",
        "Aus: im Studio-Dock; an: eigenes Fenster",
        "piano roll noten fenster dock",
    )
    .fields(&["piano_roll_window"]),
    row(
        S::Plugins,
        A::Produzent,
        "Plugins",
        "Plugins neu suchen",
        "Liest die Plugin-Ordner noch einmal; gesperrte Plugins stehen im Rack unter „+ Plugin“",
        "plugin clap vst effekt instrument scan",
    ),
    // ── Dienste und Konten · Assistent
    row(
        S::Provider(0),
        A::Dienste,
        "Assistent",
        "Claude (Anthropic)",
        "Chat-Assistent mit eigenem Schlüssel",
        "ki ai claude anthropic chat assistent sprachmodell llm schlüssel",
    )
    .card(Some(Mark::Api))
    .fields(&["ki"]),
    row(
        S::Provider(1),
        A::Dienste,
        "Assistent",
        "ChatGPT (OpenAI)",
        "Chat-Assistent mit eigenem Schlüssel",
        "ki ai chatgpt gpt openai chat assistent sprachmodell llm schlüssel",
    )
    .card(Some(Mark::Api)),
    row(
        S::Provider(2),
        A::Dienste,
        "Assistent",
        "OpenRouter",
        "Viele Modelle über ein Konto, etwa Mistral oder Llama",
        "ki ai openrouter mistral llama gemini chat assistent sprachmodell schlüssel",
    )
    .card(Some(Mark::Api)),
    row(
        S::Provider(3),
        A::Dienste,
        "Assistent",
        "Ollama (lokal)",
        "Sprachmodell auf diesem Rechner, ohne Konto",
        "ki ai ollama lokal offline chat assistent sprachmodell",
    )
    .card(Some(Mark::Api)),
    row(
        S::Provider(4),
        A::Dienste,
        "Assistent",
        "LM Studio (lokal)",
        "Sprachmodell auf diesem Rechner, ohne Konto",
        "ki ai lm studio lokal offline chat assistent sprachmodell",
    )
    .card(Some(Mark::Api)),
    row(
        S::Assistant,
        A::Dienste,
        "Assistent",
        "Assistent (Chat)",
        "Chat mit dem gewählten Sprachmodell, das WOA bedienen kann",
        "ki ai chat assistent sprachmodell fragen",
    )
    .menu(),
    row(
        S::Tools,
        A::Dienste,
        "Assistent",
        "Werkzeuge für den Assistenten",
        "Andere Dienste als Werkzeuge anbinden und WOAs eigene Werkzeuge an/aus",
        "ki ai werkzeuge tools server extern",
    )
    .card(Some(Mark::Mcp)),
    // ── Dienste und Konten · Stimmen und Klänge
    row(
        S::ElevenLabs,
        A::Dienste,
        "Stimmen und Klänge",
        "ElevenLabs",
        "Stimmen, Soundeffekte und Musik mit deinem eigenen Konto; Schlüssel nur im Schlüsselbund",
        "elevenlabs eleven labs 11 labs 11labs stimme stimmen tts text to speech sprecher \
         sprachausgabe vorlesen ki ai voice soundeffekt",
    )
    .card(Some(Mark::Api)),
    row(
        S::Freesound,
        A::Dienste,
        "Stimmen und Klänge",
        "Freesound",
        "Samples suchen mit eigenem, kostenlosem Schlüssel",
        "freesound samples geräusche klänge sounds schlüssel",
    )
    .card(Some(Mark::Api)),
    // ── Dienste und Konten · Steuerung von außen
    row(
        S::McpLive,
        A::Dienste,
        "Steuerung von außen",
        "Claude Desktop und Claude Code",
        "Ein Agent bedient WOA live in diesem Fenster",
        "mcp agent claude desktop code steuerung extern ki ai fernsteuerung",
    )
    .card(Some(Mark::Mcp)),
    row(
        S::McpHttp,
        A::Dienste,
        "Steuerung von außen",
        "Hintergrund-Server",
        "WOA wartet mit Zugriffstoken auf andere Programme (nur auf Wunsch, bis WOA schließt)",
        "mcp api http server token port agent netzwerk",
    )
    .card(Some(Mark::Mcp)),
    // ── Dienste und Konten · Sendung (Querverweis)
    row(
        S::StreamRef,
        A::Dienste,
        "Sendung",
        "Shoutcast und Icecast",
        "Wird unter Radio › Sendung eingerichtet",
        "stream shoutcast icecast server senden",
    )
    .card(None),
    // ── Entwickler (nur im Entwickler-Modus)
    row(
        S::Abnahme,
        A::Entwickler,
        "Werkzeuge",
        "Abnahme",
        "Was abzunehmen ist, je Ansicht",
        "abnahme prüfpunkte test",
    ),
    row(
        S::Fragen,
        A::Entwickler,
        "Werkzeuge",
        "Fragen und To-dos",
        "Inhaltliche Entscheidungen mit Vorschlägen",
        "fragen todo entscheidung",
    ),
    row(
        S::DevEnd,
        A::Entwickler,
        "Werkzeuge",
        "Entwickler-Modus beenden",
        "Blendet Menü und Bereich Entwickler wieder aus",
        "entwickler beenden aus",
    ),
];

/// `GuiConfig`-Felder ohne eigene Oberfläche, mit Grund (Test: jedes Feld ist registriert
/// oder steht hier).
pub const NO_UI_FIELDS: &[(&str, &str)] = &[
    ("feedback_url", "nur config.json"),
    ("install_id", "zufällige Kennung, intern"),
    ("telemetry_url", "nur config.json"),
    (
        "hoerer_url",
        "nur config.json (Statistik-Server, Vorgabe eigener Server)",
    ),
    ("radio_buffer_frames", "nur config.json"),
    ("last_update_check", "intern"),
    (
        "settings_size",
        "gezogene Größe des Fensters Einstellungen, wird gemerkt (#379)",
    ),
    ("mode", "Modus-Umschalter in der Kopfleiste, wird gemerkt"),
];

/// Für die Suche: klein, Umlaute wie Grundbuchstabe (auch „ae“, „oe“, „ue“), ß wie ss.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push('a'),
            'ö' => out.push('o'),
            'ü' => out.push('u'),
            'ß' => out.push_str("ss"),
            c => out.push(c),
        }
    }
    out.replace("ae", "a").replace("oe", "o").replace("ue", "u")
}

/// Der Eintrag zu `id`.
pub fn entry(id: SettingId) -> Option<&'static Entry> {
    REGISTRY.iter().find(|e| e.id == id)
}

/// Treffer für `query` (ohne Groß/Klein, Umlaute wie Grundbuchstabe), leer = alle. Ohne
/// Entwickler-Modus fehlt der Bereich Entwickler.
pub fn filter(query: &str, dev: bool) -> Vec<&'static Entry> {
    let q = fold(query.trim());
    REGISTRY
        .iter()
        .filter(|e| dev || !e.area.dev_only())
        .filter(|e| {
            q.is_empty()
                || [e.name, e.desc, e.keys, e.group, e.area.name()]
                    .iter()
                    .any(|t| fold(t).contains(&q))
        })
        .collect()
}

/// Abkürzungen im Menü „Einstellungen“: je Bereich alphabetisch.
pub fn menu_entries(dev: bool) -> Vec<(Area, Vec<&'static Entry>)> {
    Area::visible(dev)
        .map(|a| {
            let mut v: Vec<_> = REGISTRY.iter().filter(|e| e.menu && e.area == a).collect();
            v.sort_by_key(|e| sort_key(e.name));
            (a, v)
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

fn sort_key(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            c => c,
        })
        .collect()
}

use eframe::egui;

use crate::theme;

/// Dauer der Markierung nach Enter (s).
pub const MARK_SECONDS: f64 = 2.0;

/// Fenster „Einstellungen“.
#[derive(Debug)]
pub struct SettingsWindow {
    pub open: bool,
    pub maximized: bool,
    pub area: Area,
    pub query: String,
    /// Entwickler-Modus an (setzt die App je Bild).
    pub dev: bool,
    /// Markierte Zeile und Startzeit (egui-Zeit).
    pub marked: Option<(SettingId, f64)>,
    /// Größe des Fensters (nicht maximiert), vom Nutzer gezogen; die App merkt sie sich in
    /// `config.json` (#379). `None` = [`DEFAULT_SIZE`].
    pub size: Option<[f32; 2]>,
    /// Zahl fehlender Geräte: gelber Chip am Bereich „Audio und Geräte“ (Fassung 8).
    pub missing_devices: usize,
}

/// Größe beim ersten Öffnen (#379): so breit, dass Beschreibung und Knöpfe nebeneinander
/// passen (vorher 780 px, dann lief der Text unter die Knöpfe).
pub const DEFAULT_SIZE: [f32; 2] = [1400.0, 930.0];
/// Kleinste Breite, bei der eine Zeile noch Text links und Knöpfe rechts zeigt.
pub const MIN_WIDTH: f32 = 720.0;

/// Gemerkte Größe auf den Bildschirm `screen` begrenzt (ein kleinerer Monitor schneidet sonst ab).
/// Bereichsknopf links (Fassung 8): aktiv `surface` mit Kante `accent-2` links, nicht rot;
/// `missing` > 0 zeigt rechts einen gelben Chip „n fehlt“.
fn area_button(
    ui: &mut egui::Ui,
    on: bool,
    enabled: bool,
    text: &str,
    missing: Option<usize>,
) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(
        egui::vec2(200.0, 30.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let p = ui.painter();
    if on || (enabled && r.hovered()) {
        p.rect_filled(
            rect,
            theme::RADIUS_SM,
            if on {
                theme::surface()
            } else {
                theme::mix(theme::surface(), theme::panel(), 0.5)
            },
        );
    }
    if on {
        let edge = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
        p.rect_filled(edge, 1.5, theme::accent_2());
    }
    let ink = match (enabled, on) {
        (false, _) => theme::text_dim().gamma_multiply(0.5),
        (true, true) => theme::text(),
        (true, false) => theme::text_dim(),
    };
    p.text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(13.0),
        ink,
    );
    if let Some(n) = missing.filter(|n| *n > 0) {
        let t = format!("{n} fehlt");
        let g = p.layout_no_wrap(t, egui::FontId::proportional(11.0), theme::text());
        let chip = egui::Rect::from_min_size(
            egui::pos2(rect.right() - g.size().x - 18.0, rect.center().y - 9.0),
            egui::vec2(g.size().x + 12.0, 18.0),
        );
        p.rect_filled(chip, 9.0, theme::mix(theme::warn(), theme::panel(), 0.84));
        p.rect_stroke(
            chip,
            9.0,
            egui::Stroke::new(1.0, theme::warn()),
            egui::StrokeKind::Inside,
        );
        p.galley(
            chip.min + egui::vec2(6.0, 9.0 - g.size().y / 2.0),
            g,
            theme::text(),
        );
    }
    let label = text.to_string();
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, &label));
    r
}

pub fn start_size(saved: Option<[f32; 2]>, screen: egui::Vec2) -> egui::Vec2 {
    let [w, h] = saved.unwrap_or(DEFAULT_SIZE);
    let max = (screen - egui::vec2(40.0, 40.0)).max(egui::vec2(MIN_WIDTH, 320.0));
    egui::vec2(w.max(MIN_WIDTH), h.max(320.0)).min(max)
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Self {
            open: false,
            maximized: false,
            area: Area::Allgemein,
            query: String::new(),
            dev: false,
            marked: None,
            size: None,
            missing_devices: 0,
        }
    }
}

impl SettingsWindow {
    /// Enter im Filter: zum ersten Treffer springen (Bereich wechseln) und ihn markieren.
    pub fn jump(&mut self, now: f64) -> Option<SettingId> {
        let e = filter(&self.query, self.dev).first().copied()?;
        self.area = e.area;
        self.marked = Some((e.id, now));
        Some(e.id)
    }

    /// Menü-Abkürzung: Fenster auf, Bereich von `id`, Filter leer, Zeile 2 s markiert.
    pub fn goto(&mut self, id: SettingId, now: f64) {
        let Some(e) = entry(id) else {
            return;
        };
        self.open = true;
        self.query.clear();
        self.area = e.area;
        self.marked = Some((id, now));
    }

    /// Zahnrad einer Ansicht: Fenster beim Bereich dieser Ansicht.
    pub fn open_area(&mut self, area: Area) {
        self.open = true;
        self.query.clear();
        self.area = area;
        self.marked = None;
    }

    /// Ist `id` gerade markiert?
    pub fn is_marked(&self, id: SettingId, now: f64) -> bool {
        self.marked
            .is_some_and(|(m, t0)| m == id && (0.0..MARK_SECONDS).contains(&(now - t0)))
    }

    /// Zeichnet das Fenster. `row` zeichnet rechts das Bedienelement einer Einstellung bzw.
    /// Status und Aktionen einer Dienst-Karte (Aufrufer).
    pub fn show(&mut self, ctx: &egui::Context, mut row: impl FnMut(&mut egui::Ui, SettingId)) {
        if !self.open {
            return;
        }
        if !self.dev && self.area.dev_only() {
            self.area = Area::Allgemein;
        }
        let mut open = true;
        let now = ctx.input(|i| i.time);
        let mut max = self.maximized;
        let mut win = crate::fragen::fenster(ctx, "Einstellungen", max);
        if !max {
            win = win
                .default_size(start_size(self.size, ctx.content_rect().size()))
                .min_width(MIN_WIDTH);
        }
        let shown = win.show(ctx, |ui| {
            crate::fragen::kopf(
                ui,
                egui_phosphor::regular::GEAR_SIX,
                "Einstellungen",
                "Filter findet jede Einstellung · Enter springt hin",
                &mut open,
                &mut max,
            );
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(format!(
                        "{} Einstellung suchen",
                        egui_phosphor::regular::MAGNIFYING_GLASS
                    ))
                    .desired_width(ui.available_width())
                    .min_size(egui::vec2(0.0, 32.0)),
            );
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Einstellung suchen")
            });
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.jump(now);
            }
            ui.add_space(theme::SPACE_2);
            let hits = filter(&self.query, self.dev);
            let filtering = !self.query.trim().is_empty();
            ui.horizontal_top(|ui| {
                // Bereiche links, mit Trefferzahl beim Filtern.
                ui.vertical(|ui| {
                    ui.set_width(200.0);
                    for a in Area::visible(self.dev) {
                        let n = hits.iter().filter(|e| e.area == a).count();
                        let text = if filtering {
                            format!("{} {}  ({n})", a.icon(), a.name())
                        } else {
                            format!("{} {}", a.icon(), a.name())
                        };
                        let missing = (a == Area::Audio).then_some(self.missing_devices);
                        if area_button(ui, self.area == a, !filtering || n > 0, &text, missing)
                            .clicked()
                        {
                            self.area = a;
                        }
                    }
                });
                ui.separator();
                // Eigene senkrechte Spalte: die ScrollArea erbte sonst das `horizontal_top`.
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        let shown: Vec<_> = if filtering {
                            hits.clone()
                        } else {
                            REGISTRY.iter().filter(|e| e.area == self.area).collect()
                        };
                        if shown.is_empty() {
                            ui.weak("Keine Einstellung gefunden");
                        }
                        let mut last_group = None;
                        for e in shown {
                            let head = (e.area, e.group);
                            if last_group != Some(head) {
                                if last_group.is_some() {
                                    ui.add_space(theme::SPACE_2);
                                }
                                let title = if filtering {
                                    format!("{} › {}", e.area.name(), e.group)
                                } else {
                                    e.group.to_string()
                                };
                                ui.label(theme::panel_label(title));
                                ui.add_space(theme::SPACE_1);
                                last_group = Some(head);
                            }
                            self.entry_ui(ui, ctx, e, now, &mut row);
                            ui.add_space(theme::SPACE_1);
                        }
                        if !filtering && self.area == Area::Dienste {
                            ui.add_space(theme::SPACE_2);
                            ui.label(
                                egui::RichText::new(
                                    "Weitere Stimmen, etwa Piper auf diesem Rechner, kommen \
                                     unter „Stimmen und Klänge“ dazu.",
                                )
                                .small()
                                .color(theme::text_dim()),
                            );
                        }
                    })
                });
            });
        });
        // Gezogene Größe merken (#379), nicht im maximierten Zustand.
        if !self.maximized
            && let Some(r) = &shown
        {
            let sz = r.response.rect.size();
            let new = [sz.x.round(), sz.y.round()];
            if self.size != Some(new) && (self.size.is_some() || self.size_touched(new)) {
                self.size = Some(new);
            }
        }
        self.maximized = max;
        if !open {
            self.open = false;
        }
    }

    /// Ohne gemerkte Größe zählt nur eine Größe, die vom Start-Wert abweicht (also gezogen).
    fn size_touched(&self, new: [f32; 2]) -> bool {
        let d = DEFAULT_SIZE;
        (new[0] - d[0]).abs() > 2.0 || (new[1] - d[1]).abs() > 2.0
    }

    fn entry_ui(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        e: &Entry,
        now: f64,
        row: &mut impl FnMut(&mut egui::Ui, SettingId),
    ) {
        let marked = self.is_marked(e.id, now);
        let card = matches!(e.kind, Kind::Card(_));
        let fr = egui::Frame::new()
            .fill(if marked {
                theme::mix(theme::cue(), theme::panel(), 0.82)
            } else if card {
                theme::panel()
            } else {
                theme::surface()
            })
            .stroke(egui::Stroke::new(
                if marked { 1.5 } else { 1.0 },
                if marked {
                    theme::cue()
                } else {
                    theme::border()
                },
            ))
            .corner_radius(theme::RADIUS_MD)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Breite der Bedienelemente aus dem letzten Bild (#378): der Text links bekommt
                // nur den Rest und bricht um, statt unter die Knöpfe zu laufen.
                let ctrl_id = ui.id().with(("setting_ctrl_w", e.id));
                let ctrl_w: f32 = ctx.data(|d| d.get_temp(ctrl_id)).unwrap_or(240.0);
                let text_w = (ui.available_width() - ctrl_w - 16.0).max(160.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(text_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(egui::RichText::new(e.name).strong().color(theme::text()));
                            if let Kind::Card(Some(m)) = e.kind {
                                crate::widgets::StateChip::new(
                                    m.label(),
                                    crate::widgets::ChipTone::Neutral,
                                )
                                .show(ui)
                                .on_hover_text("Technik, über die WOA den Dienst anspricht");
                            }
                        });
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(e.desc).small().color(theme::text_dim()),
                            )
                            .wrap(),
                        );
                    });
                    // Bedienelement rechts, so hoch wie die Zeile (sonst wächst sie).
                    let used = ui
                        .allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 36.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                row(ui, e.id);
                                ui.min_rect().width()
                            },
                        )
                        .inner;
                    if (used - ctrl_w).abs() > 1.0 {
                        ctx.data_mut(|d| d.insert_temp(ctrl_id, used));
                        ctx.request_repaint();
                    }
                });
            });
        if marked {
            fr.response.scroll_to_me(Some(egui::Align::Center));
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(q: &str) -> Vec<&'static str> {
        filter(q, false).iter().map(|e| e.name).collect()
    }

    fn first(q: &str) -> SettingId {
        filter(q, false).first().map(|e| e.id).expect(q)
    }

    #[test]
    fn enter_jumps_to_the_first_hit_and_marks_it_two_seconds() {
        let mut w = SettingsWindow {
            query: "icecast".into(),
            ..Default::default()
        };
        assert_eq!(w.jump(10.0), Some(SettingId::Stream));
        assert_eq!(w.area, Area::Radio);
        assert!(w.is_marked(SettingId::Stream, 11.9));
        assert!(!w.is_marked(SettingId::Stream, 12.0), "nach 2 s aus");
        w.query = "xyzzy".into();
        assert_eq!(w.jump(20.0), None);
    }

    #[test]
    fn seven_fixed_areas_in_order() {
        let n: Vec<_> = Area::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(
            n,
            [
                "Allgemein",
                "Audio und Geräte",
                "Radio",
                "Party",
                "Produzent",
                "Dienste und Konten",
                "Entwickler"
            ]
        );
        assert!(!n.contains(&"KI"), "Bereich KI ist aufgelöst");
        assert_eq!(Area::visible(false).count(), 6);
        assert_eq!(Area::visible(true).count(), 7);
    }

    #[test]
    fn every_area_has_entries_and_registry_is_ordered_by_area() {
        for a in Area::ALL {
            assert!(REGISTRY.iter().any(|e| e.area == a), "{} leer", a.name());
        }
        let pos = |a: Area| Area::ALL.iter().position(|x| *x == a);
        assert!(
            REGISTRY
                .windows(2)
                .all(|w| pos(w[0].area) <= pos(w[1].area)),
            "REGISTRY nach Bereich ordnen"
        );
    }

    #[test]
    fn ids_and_titles_are_unique() {
        for (i, a) in REGISTRY.iter().enumerate() {
            for b in &REGISTRY[i + 1..] {
                assert_ne!(a.id, b.id, "{} doppelt", a.name);
                assert_ne!(a.name, b.name, "Titel doppelt");
            }
        }
    }

    #[test]
    fn services_sit_in_purpose_groups_and_only_there_are_cards() {
        for e in REGISTRY {
            if e.area == Area::Dienste {
                assert!(SERVICE_GROUPS.contains(&e.group), "{}: {}", e.name, e.group);
            } else {
                assert_eq!(e.kind, Kind::Row, "{} ist keine Dienst-Karte", e.name);
            }
        }
        assert_eq!(
            entry(S::ElevenLabs).unwrap().kind,
            Kind::Card(Some(Mark::Api))
        );
        assert_eq!(entry(S::McpLive).unwrap().kind, Kind::Card(Some(Mark::Mcp)));
        // Shoutcast bleibt unter Radio, unter Dienste nur als Querverweis.
        assert_eq!(entry(S::Stream).unwrap().area, Area::Radio);
        assert_eq!(entry(S::StreamRef).unwrap().kind, Kind::Card(None));
    }

    #[test]
    fn colloquial_queries_find_the_right_place() {
        assert_eq!(first("11 labs"), S::ElevenLabs);
        assert_eq!(first("11labs"), S::ElevenLabs);
        assert_eq!(first("Eleven Labs"), S::ElevenLabs);
        assert_eq!(first("stimme"), S::ElevenLabs);
        assert_eq!(first("TTS"), S::ElevenLabs);
        assert_eq!(first("shoutcast"), S::Stream);
        assert_eq!(first("stream"), S::Stream);
        assert_eq!(first("mcp"), S::McpLive);
        assert!(names("ki").contains(&"ElevenLabs"));
        assert!(names("chatgpt").contains(&"ChatGPT (OpenAI)"));
        assert!(names("kennwort").contains(&"Sendung: Stream-Server und Kennwort"));
        assert!(names("soundkarte").contains(&"Audio-Geräte"));
        // André 03.11.: „wo stell ich denn die mikro quelle ein?“
        for q in ["mikro", "mikrofon", "externes mikro", "eingang"] {
            assert!(names(q).contains(&"Audio-Geräte"), "{q}");
        }
        assert!(
            names("KOPFHOERER").contains(&"Ausgabe neu verbinden"),
            "Umlaute egal"
        );
        assert!(names("midi").contains(&"Controller und MIDI-Learn"));
        assert!(names("datenschutz").contains(&"Fehlercodes anonym senden (ab nächstem Start)"));
        assert!(filter("xyzzy", false).is_empty());
    }

    #[test]
    fn developer_area_only_in_developer_mode_but_unlock_always_reachable() {
        assert!(filter("", false).iter().all(|e| e.area != Area::Entwickler));
        assert!(filter("abnahme", true).iter().any(|e| e.id == S::Abnahme));
        assert_eq!(entry(S::Developer).unwrap().area, Area::Allgemein);
        let mut w = SettingsWindow {
            query: "beenden".into(),
            ..Default::default()
        };
        assert_eq!(w.jump(0.0), None);
        w.dev = true;
        assert_eq!(w.jump(0.0), Some(S::DevEnd));
    }

    #[test]
    fn menu_shortcut_opens_the_window_at_its_place_and_marks_it() {
        let mut w = SettingsWindow {
            query: "midi".into(),
            ..Default::default()
        };
        w.goto(S::Stream, 5.0);
        assert!(w.open);
        assert!(w.query.is_empty());
        assert_eq!(w.area, Area::Radio);
        assert!(w.is_marked(S::Stream, 6.5));
        w.open_area(Area::Party);
        assert_eq!(w.area, Area::Party);
    }

    #[test]
    fn menu_shortcuts_come_from_the_registry_sorted_per_area() {
        let m = menu_entries(false);
        assert!(m.iter().all(|(a, _)| !a.dev_only()));
        for (_, v) in &m {
            let keys: Vec<_> = v.iter().map(|e| sort_key(e.name)).collect();
            let mut s = keys.clone();
            s.sort();
            assert_eq!(keys, s);
        }
        let all: Vec<_> = m
            .iter()
            .flat_map(|(_, v)| v.iter().map(|e| e.name))
            .collect();
        for n in [
            "Sendung: Stream-Server und Kennwort",
            "Audio-Geräte",
            "Controller und MIDI-Learn",
            "Assistent (Chat)",
            "Entwickler-Modus",
        ] {
            assert!(all.contains(&n), "{n} fehlt im Menü");
        }
        // Ein Einstieg für Soundkarten (André 03.11.: „doppelt gemoppelt“).
        for n in [
            "Vorhören (PFL)",
            "Telefon (Eingang für den Anrufer)",
            "Mikrofon",
        ] {
            assert!(!all.contains(&n), "{n} doppelt im Menü");
        }
    }

    /// Feldnamen von `GuiConfig`, aus dem Quelltext (serde lässt Vorgaben aus).
    fn config_fields() -> Vec<String> {
        let src = include_str!("feedback.rs").replace("\r\n", "\n");
        let body = src
            .split("pub struct GuiConfig {")
            .nth(1)
            .and_then(|s| s.split("\n}\n").next())
            .expect("GuiConfig");
        body.lines()
            .filter_map(|l| l.trim().strip_prefix("pub "))
            .filter_map(|l| l.split(':').next())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn every_config_field_with_ui_is_registered() {
        let fields = config_fields();
        assert!(fields.len() >= 20, "{fields:?}");
        for f in &fields {
            let reg = REGISTRY.iter().any(|e| e.fields.contains(&f.as_str()));
            let skip = NO_UI_FIELDS.iter().any(|(n, _)| n == f);
            assert!(
                reg ^ skip,
                "Feld {f}: registrieren oder in NO_UI_FIELDS (genau eins)"
            );
        }
        for e in REGISTRY {
            for f in e.fields {
                assert!(fields.iter().any(|x| x == f), "{}: kein Feld {f}", e.name);
            }
        }
    }

    /// #378/#379: Das Fenster öffnet breit genug, Beschreibung und Knopf überlappen nicht,
    /// auch nicht bei der kleinsten Breite (Text bricht um).
    #[test]
    fn rows_never_run_under_their_buttons() {
        use egui_kittest::kittest::Queryable;
        for size in [None, Some([MIN_WIDTH, 600.0])] {
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1920.0, 1009.0))
                .build_ui_state(
                    |ui, w: &mut SettingsWindow| {
                        let ctx = ui.ctx().clone();
                        if crate::theme::install_fonts(&ctx) {
                            crate::theme::apply(
                                &ctx,
                                crate::theme::Theme::Dunkel,
                                crate::theme::Mode::Radio,
                            );
                            return;
                        }
                        w.show(&ctx, |ui, _| {
                            let _ = ui.button("Im Produzenten zeigen");
                        });
                    },
                    SettingsWindow {
                        open: true,
                        area: Area::Produzent,
                        size,
                        ..Default::default()
                    },
                );
            h.run_steps(6);
            let desc = entry(SettingId::SoundBanks).unwrap().desc;
            let text = h.get_by_label(desc).rect();
            let buttons: Vec<_> = h
                .query_all_by_label("Im Produzenten zeigen")
                .map(|n: egui_kittest::Node<'_>| n.rect())
                .collect();
            assert!(!buttons.is_empty());
            for b in buttons {
                assert!(
                    !text.intersects(b) || text.max.y <= b.min.y || b.max.y <= text.min.y,
                    "{size:?}: Text {text:?} unter Knopf {b:?}"
                );
            }
            if size.is_none() {
                let win = h.get_by_label("Einstellung suchen").rect();
                assert!(win.width() >= 900.0, "Fenster zu schmal: {win:?}");
            }
        }
        assert_eq!(
            start_size(None, egui::vec2(1366.0, 700.0)),
            egui::vec2(1326.0, 660.0)
        );
        assert_eq!(
            start_size(Some([1500.0, 900.0]), egui::vec2(1920.0, 1009.0)),
            egui::vec2(1500.0, 900.0)
        );
    }
}
