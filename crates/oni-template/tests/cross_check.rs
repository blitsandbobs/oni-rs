//! Proves the generic walker reproduces the hand-written `oni-texture` and
//! `oni-sound` parsers on real level0 data: same field values, same offsets,
//! for every TXMP and SNDD instance.
//!
//! Default data path: `<repo>/original/gamedata/level0_Final.{dat,raw}`
//! (override with `ONI_DAT_LEVEL0`); skips if absent.

use std::path::PathBuf;

use oni_dat::Dat;
use oni_fourcc::FourCc;
use oni_template::{decode, Field, Kind, Layout, RefKind, Value};

fn level0_dat() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONI_DAT_LEVEL0") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("original/gamedata/level0_Final.dat");
    p.exists().then_some(p)
}

fn txmp_layout() -> Layout {
    use Kind::*;
    Layout {
        tag: FourCc::from_bytes(*b"TXMP"),
        name: "Texture Map".into(),
        fields: vec![
            Field::new("debugName", Pad(128)),
            Field::new("flags", U32),
            Field::new("width", U16),
            Field::new("height", U16),
            Field::new("texelType", Enum),
            Field::new("animation", TemplateRef),
            Field::new("envMap", TemplateRef),
            Field::new("pixels", RawPtr),
            Field::new("pixelStorage", SeparateIndex),
        ],
    }
}

fn sndd_layout() -> Layout {
    use Kind::*;
    let coef = vec![Field::new("iCoef1", I16), Field::new("iCoef2", I16)];
    Layout {
        tag: FourCc::from_bytes(*b"SNDD"),
        name: "Sound Data".into(),
        fields: vec![
            Field::new("flags", U32),
            Field::new("wFormatTag", U16),
            Field::new("nChannels", U16),
            Field::new("nSamplesPerSec", U32),
            Field::new("nAvgBytesPerSec", U32),
            Field::new("nBlockAlign", U16),
            Field::new("wBitsPerSample", U16),
            Field::new("cbSize", U16),
            Field::new("wSamplesPerBlock", U16),
            Field::new("wNumCoef", U16),
            Field::new("aCoef", Array { len: 7, elem: coef }),
            Field::new("duration", U16),
            Field::new("num_bytes", U32),
            Field::new("data", RawPtr),
        ],
    }
}

#[test]
fn generic_walker_matches_texture_parser() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");
    let layout = txmp_layout();

    let mut checked = 0usize;
    for inst in dat
        .instances
        .iter()
        .filter(|i| i.tag == FourCc::from_bytes(*b"TXMP"))
    {
        let Ok(tex) = oni_texture::TextureMap::from_instance(&dat, inst) else {
            continue;
        };
        let data = dat.instance_data(inst).expect("data");
        let rec = decode(&layout, data, dat.header.endian).expect("decode");

        assert_eq!(
            rec.int("width"),
            Some(tex.width as i64),
            "width for {:?}",
            inst.name
        );
        assert_eq!(rec.int("height"), Some(tex.height as i64), "height");
        assert_eq!(rec.int("texelType"), Some(tex.format as i64), "format");
        assert_eq!(rec.int("flags"), Some(tex.flags as i64), "flags");
        match rec.get("pixels").unwrap().value {
            Value::Ref(RefKind::Raw, v) => assert_eq!(v, tex.raw_offset, "pixels offset"),
            _ => panic!("pixels not a raw ref"),
        }
        // The walker's offsets must match the hand-found ones.
        assert_eq!(rec.offset_of("width"), Some(132));
        assert_eq!(rec.offset_of("pixels"), Some(148));
        checked += 1;
    }
    assert!(checked > 1900, "checked {checked} TXMP");
    eprintln!("generic walker == oni-texture for {checked} TXMP instances");
}

#[test]
fn generic_walker_matches_sound_parser() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");
    let layout = sndd_layout();

    let mut checked = 0usize;
    for inst in dat
        .instances
        .iter()
        .filter(|i| i.tag == FourCc::from_bytes(*b"SNDD"))
    {
        let Ok(snd) = oni_sound::Sound::from_instance(&dat, inst) else {
            continue;
        };
        let data = dat.instance_data(inst).expect("data");
        let rec = decode(&layout, data, dat.header.endian).expect("decode");

        assert_eq!(
            rec.int("num_bytes"),
            Some(snd.num_bytes as i64),
            "num_bytes for {:?}",
            inst.name
        );
        assert_eq!(
            rec.int("wFormatTag"),
            Some(snd.format_tag() as i64),
            "format tag"
        );
        assert_eq!(
            rec.int("nChannels"),
            Some(snd.channels() as i64),
            "channels"
        );
        assert_eq!(
            rec.int("nSamplesPerSec"),
            Some(snd.sample_rate() as i64),
            "rate"
        );
        match rec.get("data").unwrap().value {
            Value::Ref(RefKind::Raw, v) => assert_eq!(v, snd.raw_offset, "data offset"),
            _ => panic!("data not a raw ref"),
        }
        assert_eq!(rec.offset_of("num_bytes"), Some(56));
        assert_eq!(rec.offset_of("data"), Some(60));
        checked += 1;
    }
    assert!(checked > 2300, "checked {checked} SNDD");
    eprintln!("generic walker == oni-sound for {checked} SNDD instances");
}
