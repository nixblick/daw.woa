//! Ein kleines CLAP-Plugin nur für Tests: Sinus-Synth (Noten) plus Gain (Eingang), zwei Regler
//! und ein Zustand. Gebaut mit `clack-plugin`, geladen über den echten Host-Weg
//! (`PluginEntry`, `PluginInstance`), also dasselbe Verfahren wie bei einem fremden Plugin,
//! nur ohne Bibliotheksdatei.
//!
//! Als Instrument (keine Eingabe) klingt es bei Noten; als Effekt (Eingabe, keine Noten)
//! verstärkt es das Signal um den Regler `GAIN`.

use std::sync::atomic::{AtomicU64, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPortsImpl,
};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams,
};
use clack_extensions::state::PluginStateImpl;
use clack_host::entry::PluginEntry;
use clack_plugin::clack_export_entry;
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use std::io::{Read, Write};

/// Kennung des Test-Plugins.
pub const ID: &str = "de.woa.test-plugin";
/// Regler: Verstärkung des Eingangs (0..2, Grundwert 1).
pub const GAIN: u32 = 1;
/// Regler: Lautstärke des Synths (0..1, Grundwert 0.5).
pub const LEVEL: u32 = 2;

/// Die Reglerwerte, vom Audio- und vom Control-Thread gemeinsam gelesen (als Bits eines `f64`).
pub struct TestShared {
    gain: AtomicU64,
    level: AtomicU64,
}

impl TestShared {
    fn new() -> Self {
        Self {
            gain: AtomicU64::new(1.0f64.to_bits()),
            level: AtomicU64::new(0.5f64.to_bits()),
        }
    }

    fn cell(&self, id: u32) -> Option<&AtomicU64> {
        match id {
            GAIN => Some(&self.gain),
            LEVEL => Some(&self.level),
            _ => None,
        }
    }

    fn get(&self, id: u32) -> Option<f64> {
        self.cell(id).map(|c| f64::from_bits(c.load(Ordering::Relaxed)))
    }

    fn set(&self, id: u32, v: f64) {
        let max = if id == GAIN { 2.0 } else { 1.0 };
        if let Some(c) = self.cell(id) {
            c.store(v.clamp(0.0, max).to_bits(), Ordering::Relaxed);
        }
    }
}

impl PluginShared<'_> for TestShared {}

/// Das Plugin.
pub struct TestPlugin;

impl Plugin for TestPlugin {
    type AudioProcessor<'a> = TestProcessor<'a>;
    type Shared<'a> = TestShared;
    type MainThread<'a> = TestMain<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&TestShared>) {
        builder
            .register::<clack_extensions::audio_ports::PluginAudioPorts>()
            .register::<clack_extensions::note_ports::PluginNotePorts>()
            .register::<clack_extensions::params::PluginParams>()
            .register::<clack_extensions::state::PluginState>();
    }
}

impl DefaultPluginFactory for TestPlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::{AUDIO_EFFECT, INSTRUMENT, STEREO};
        PluginDescriptor::new(ID, "WOA Test Plugin")
            .with_vendor("WOA")
            .with_features([INSTRUMENT, AUDIO_EFFECT, STEREO])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<TestShared, PluginError> {
        Ok(TestShared::new())
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a TestShared,
    ) -> Result<TestMain<'a>, PluginError> {
        Ok(TestMain { shared })
    }
}

/// Control-Thread-Teil.
pub struct TestMain<'a> {
    shared: &'a TestShared,
}

impl<'a> PluginMainThread<'a, TestShared> for TestMain<'a> {}

impl PluginAudioPortsImpl for TestMain<'_> {
    fn count(&self, _is_input: bool) -> u32 {
        1
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: if is_input { b"In" } else { b"Out" },
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginNotePortsImpl for TestMain<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(is_input)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if index == 0 && is_input {
            writer.set(&NotePortInfo {
                id: ClapId::new(0),
                name: b"Notes",
                supported_dialects: NoteDialects::CLAP,
                preferred_dialect: Some(NoteDialect::Clap),
            });
        }
    }
}

impl PluginMainThreadParams for TestMain<'_> {
    fn count(&self) -> u32 {
        2
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        let (id, name, max, default): (u32, &[u8], f64, f64) = match index {
            0 => (GAIN, b"Gain", 2.0, 1.0),
            1 => (LEVEL, b"Level", 1.0, 0.5),
            _ => return,
        };
        info.set(&ParamInfo {
            id: ClapId::new(id),
            flags: ParamInfoFlags::IS_AUTOMATABLE,
            cookie: Default::default(),
            name,
            module: b"",
            min_value: 0.0,
            max_value: max,
            default_value: default,
        });
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        self.shared.get(id.get())
    }

    fn value_to_text(
        &self,
        _id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> core::fmt::Result {
        use core::fmt::Write as _;
        write!(writer, "{value:.2}")
    }

    fn text_to_value(&self, _id: ClapId, text: &core::ffi::CStr) -> Option<f64> {
        text.to_str().ok()?.trim().parse().ok()
    }

    fn flush(
        &self,
        input: &clack_plugin::events::io::InputEvents,
        _output: &mut clack_plugin::events::io::OutputEvents,
    ) {
        for e in input {
            if let Some(CoreEventSpace::ParamValue(p)) = e.as_core_event()
                && let Some(id) = p.param_id()
            {
                self.shared.set(id.get(), p.value());
            }
        }
    }
}

impl PluginStateImpl for TestMain<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        let g = self.shared.get(GAIN).unwrap_or(1.0);
        let l = self.shared.get(LEVEL).unwrap_or(0.5);
        output.write_all(&g.to_le_bytes())?;
        output.write_all(&l.to_le_bytes())?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut b = [0u8; 8];
        input.read_exact(&mut b)?;
        let g = f64::from_le_bytes(b);
        input.read_exact(&mut b)?;
        let l = f64::from_le_bytes(b);
        self.shared.set(GAIN, g);
        self.shared.set(LEVEL, l);
        Ok(())
    }
}

const VOICES: usize = 8;

#[derive(Clone, Copy)]
struct Voice {
    key: u16,
    phase: f64,
    step: f64,
    amp: f64,
}

/// Audio-Thread-Teil: acht Sinus-Stimmen ohne Hüllkurve.
pub struct TestProcessor<'a> {
    shared: &'a TestShared,
    voices: [Option<Voice>; VOICES],
    rate: f64,
}

impl<'a> PluginAudioProcessor<'a, TestShared, TestMain<'a>> for TestProcessor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main: &TestMain<'a>,
        shared: &'a TestShared,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            voices: [None; VOICES],
            rate: config.sample_rate,
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let Some(mut port) = audio.port_pair(0) else {
            return Ok(ProcessStatus::Continue);
        };
        let Some(mut channels) = port.channels()?.into_f32() else {
            return Ok(ProcessStatus::Continue);
        };
        let frames = channels.frames_count() as usize;
        let mut cursor = 0usize;
        let mut input = events.input.into_iter().peekable();
        for frame in 0..frames {
            while let Some(e) = input.peek() {
                if e.header().time() as usize > frame {
                    break;
                }
                match e.as_core_event() {
                    Some(CoreEventSpace::NoteOn(n)) => {
                        if let clack_plugin::events::Match::Specific(key) = n.key() {
                            let hz = 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2();
                            let v = Voice {
                                key,
                                phase: 0.0,
                                step: hz / self.rate,
                                amp: n.velocity(),
                            };
                            if let Some(slot) = self.voices.iter_mut().find(|s| s.is_none()) {
                                *slot = Some(v);
                            }
                        }
                    }
                    Some(CoreEventSpace::NoteOff(n)) => {
                        for s in &mut self.voices {
                            let hit = match n.key() {
                                clack_plugin::events::Match::Specific(k) => {
                                    s.is_some_and(|v| v.key == k)
                                }
                                clack_plugin::events::Match::All => true,
                            };
                            if hit {
                                *s = None;
                            }
                        }
                    }
                    Some(CoreEventSpace::ParamValue(p)) => {
                        if let Some(id) = p.param_id() {
                            self.shared.set(id.get(), p.value());
                        }
                    }
                    _ => {}
                }
                input.next();
            }
            let gain = self.shared.get(GAIN).unwrap_or(1.0) as f32;
            let level = self.shared.get(LEVEL).unwrap_or(0.5);
            let mut synth = 0.0f64;
            for v in self.voices.iter_mut().flatten() {
                synth += (v.phase * std::f64::consts::TAU).sin() * v.amp * level * 0.25;
                v.phase = (v.phase + v.step).fract();
            }
            let synth = synth as f32;
            for ch in 0..2usize {
                let sample = channels.channel_pair(ch).map(|c| match c {
                    ChannelPair::InputOutput(i, o) => {
                        o[frame] = i[frame] * gain + synth;
                    }
                    ChannelPair::InPlace(b) => {
                        b[frame] = b[frame] * gain + synth;
                    }
                    ChannelPair::OutputOnly(o) => {
                        o[frame] = synth;
                    }
                    ChannelPair::InputOnly(_) => {}
                });
                let _ = sample;
            }
            cursor += 1;
        }
        let _ = cursor;
        Ok(ProcessStatus::Continue)
    }
}

impl PluginAudioProcessorParams for TestProcessor<'_> {
    fn flush(
        &mut self,
        input: &clack_plugin::events::io::InputEvents,
        _output: &mut clack_plugin::events::io::OutputEvents,
    ) {
        for e in input {
            if let Some(CoreEventSpace::ParamValue(p)) = e.as_core_event()
                && let Some(id) = p.param_id()
            {
                self.shared.set(id.get(), p.value());
            }
        }
    }
}

clack_export_entry!(clack_plugin::entry::SinglePluginEntry<TestPlugin>);

/// Eine Plugin-Liste mit nur dem Test-Plugin.
pub fn catalog() -> crate::Catalog {
    let mut cat = crate::Catalog::empty();
    cat.add_loaded(
        crate::PluginInfo {
            clap_id: ID.to_string(),
            name: "WOA Test Plugin".to_string(),
            vendor: "WOA".to_string(),
            features: vec![
                "instrument".to_string(),
                "audio-effect".to_string(),
                "stereo".to_string(),
            ],
            path: std::path::PathBuf::from("/woa/test-plugin"),
        },
        entry(),
    );
    cat
}

/// Lädt das Test-Plugin im eigenen Prozess (ohne Bibliotheksdatei) über den Host-Weg.
pub fn entry() -> PluginEntry {
    PluginEntry::load_from_clack::<clack_plugin::entry::SinglePluginEntry<TestPlugin>>(
        c"/woa/test-plugin",
    )
    .expect("test plugin entry")
}
