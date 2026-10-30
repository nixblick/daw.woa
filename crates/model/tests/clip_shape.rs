//! Paket 21: Fade-Kurven und Clip-Gain im Modell: Kurventabellen, Befehle mit Undo und
//! Redo, Prüfung der Werte, Projektformat.

use woa_model::{
    Clip, ClipId, Command, FadeCurve, MAX_GAIN_DB, MIN_GAIN_DB, ModelError, Session, TrackKind,
    store,
};

fn session_with_clip() -> (Session, ClipId) {
    let mut s = Session::new("shape");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "A", TrackKind::Audio))
        .expect("track");
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(c, "c", 0.0, 8.0, Some("a.wav".into())),
        index: None,
    })
    .expect("clip");
    (s, c)
}

fn clip(s: &Session, c: ClipId) -> Clip {
    s.project().clip(c).expect("clip").1.clone()
}

#[test]
fn curve_tables() {
    // (t, linear, equal power, s-curve)
    let table = [
        (0.0f32, 0.0f32, 0.0f32, 0.0f32),
        (0.25, 0.25, 0.382_683_43, 0.146_446_6),
        (0.5, 0.5, std::f32::consts::FRAC_1_SQRT_2, 0.5),
        (0.75, 0.75, 0.923_879_5, 0.853_553_4),
        (1.0, 1.0, 1.0, 1.0),
    ];
    for (t, lin, eq, s) in table {
        assert!((FadeCurve::Linear.gain(t) - lin).abs() < 1e-6, "linear {t}");
        assert!(
            (FadeCurve::EqualPower.gain(t) - eq).abs() < 1e-6,
            "equal power {t}"
        );
        assert!((FadeCurve::SCurve.gain(t) - s).abs() < 1e-6, "s-curve {t}");
    }
    // Außerhalb 0..1 wird begrenzt.
    for c in FadeCurve::ALL {
        assert_eq!(c.gain(-3.0), 0.0);
        assert_eq!(c.gain(7.0), 1.0);
        assert!(c.gain(f32::NAN).is_finite() || c.gain(f32::NAN).is_nan());
    }
}

#[test]
fn curves_are_monotonic_and_pairs_keep_their_sum() {
    for c in FadeCurve::ALL {
        let mut prev = -1.0f32;
        for k in 0..=1000 {
            let g = c.gain(k as f32 / 1000.0);
            assert!(g >= prev, "{c:?} falls at {k}");
            prev = g;
        }
    }
    for k in 0..=100 {
        let t = k as f32 / 100.0;
        // Gleiche Leistung: Quadrate der beiden Seiten ergeben 1.
        let p = FadeCurve::EqualPower.gain(t).powi(2) + FadeCurve::EqualPower.gain(1.0 - t).powi(2);
        assert!((p - 1.0).abs() < 1e-5, "power {p} at {t}");
        // S-Kurve und Linear: die Summe der beiden Seiten ergibt 1.
        for c in [FadeCurve::SCurve, FadeCurve::Linear] {
            let a = c.gain(t) + c.gain(1.0 - t);
            assert!((a - 1.0).abs() < 1e-5, "{c:?} sum {a} at {t}");
        }
    }
}

#[test]
fn curve_and_gain_commands_undo_and_redo() {
    let (mut s, c) = session_with_clip();
    s.apply(Command::SetClipFadeCurves {
        clip: c,
        fade_in: FadeCurve::SCurve,
        fade_out: FadeCurve::EqualPower,
    })
    .expect("curves");
    s.apply(Command::SetClipGain {
        clip: c,
        gain_db: 6.0,
    })
    .expect("gain");
    let k = clip(&s, c);
    assert_eq!(
        (k.fade_in_curve, k.fade_out_curve, k.gain_db),
        (FadeCurve::SCurve, FadeCurve::EqualPower, 6.0)
    );
    s.undo().expect("undo gain");
    assert_eq!(clip(&s, c).gain_db, 0.0);
    s.undo().expect("undo curves");
    let k = clip(&s, c);
    assert_eq!(
        (k.fade_in_curve, k.fade_out_curve),
        (FadeCurve::Linear, FadeCurve::Linear)
    );
    s.redo().expect("redo");
    s.redo().expect("redo");
    assert_eq!(clip(&s, c).gain_db, 6.0);
    assert_eq!(clip(&s, c).fade_out_curve, FadeCurve::EqualPower);
}

#[test]
fn gain_is_checked_and_unknown_clips_are_rejected() {
    let (mut s, c) = session_with_clip();
    for bad in [
        MAX_GAIN_DB + 0.1,
        MIN_GAIN_DB - 0.1,
        f64::NAN,
        f64::INFINITY,
    ] {
        let r = s.apply(Command::SetClipGain {
            clip: c,
            gain_db: bad,
        });
        assert!(r.is_err(), "{bad} must be refused");
    }
    assert_eq!(clip(&s, c).gain_db, 0.0, "refused commands change nothing");
    assert!(matches!(
        s.apply(Command::SetClipGain {
            clip: ClipId(9999),
            gain_db: 0.0
        }),
        Err(ModelError::ClipNotFound(_))
    ));
    assert!(matches!(
        s.apply(Command::SetClipFadeCurves {
            clip: ClipId(9999),
            fade_in: FadeCurve::Linear,
            fade_out: FadeCurve::Linear
        }),
        Err(ModelError::ClipNotFound(_))
    ));
}

#[test]
fn project_format_stays_v2_without_shape_and_roundtrips_with_it() {
    let (mut s, c) = session_with_clip();
    assert_eq!(store::required_version(s.project()), 2);
    let json = store::to_json(s.project()).expect("json");
    assert!(!json.contains("fade_in_curve"));
    assert_eq!(json.matches("gain_db").count(), 1, "only the track gain");
    // Eine Datei aus 0.15 (ohne die neuen Felder) lädt unverändert.
    let old = store::from_json(&json, std::path::Path::new("project.json")).expect("old file");
    assert_eq!(&old, s.project());

    s.apply(Command::SetClipGain {
        clip: c,
        gain_db: -2.5,
    })
    .expect("gain");
    s.apply(Command::SetClipFadeCurves {
        clip: c,
        fade_in: FadeCurve::EqualPower,
        fade_out: FadeCurve::SCurve,
    })
    .expect("curves");
    assert_eq!(
        store::required_version(s.project()),
        3,
        "older builds refuse instead of dropping"
    );
    let json = store::to_json(s.project()).expect("json");
    assert!(json.contains("\"equal_power\"") && json.contains("\"s_curve\""));
    let back = store::from_json(&json, std::path::Path::new("project.json")).expect("reload");
    assert_eq!(&back, s.project());
}
