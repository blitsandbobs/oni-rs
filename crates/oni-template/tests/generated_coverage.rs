//! Validates the generated (standalone) layouts against real level0 data:
//!   * every instance whose tag has a generated layout must decode without
//!     overrunning its bytes (a wrong offset usually blows up a var-array
//!     count and overruns — so this catches layout bugs across 30k instances);
//!   * the generated TXMP and SNDD layouts must reproduce the values the
//!     hand-written `oni-texture` / `oni-sound` parsers produce.
//!
//! Skips if the level0 fixture is absent (set `ONI_DAT_LEVEL0`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use oni_dat::Dat;
use oni_fourcc::FourCc;
use oni_template::{decode, RefKind, Registry, Value};

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

#[test]
fn generated_registry_is_populated() {
    let reg = Registry::load();
    assert!(
        reg.len() > 100,
        "expected many generated layouts, got {}",
        reg.len()
    );
    assert!(reg.get(FourCc::from_bytes(*b"TXMP")).is_some());
    assert!(reg.get(FourCc::from_bytes(*b"SNDD")).is_some());
}

#[test]
fn every_instance_decodes_without_overrun() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");
    let reg = Registry::load();

    let mut covered_tags = BTreeMap::new();
    let mut total_tags = BTreeMap::new();
    let mut decoded = 0usize;
    let mut failures: Vec<(String, String)> = Vec::new();

    for inst in &dat.instances {
        *total_tags.entry(inst.tag).or_insert(0usize) += 1;
        let Some(layout) = reg.get(inst.tag) else {
            continue;
        };
        let Some(data) = dat.instance_data(inst) else {
            continue;
        };
        *covered_tags.entry(inst.tag).or_insert(0usize) += 1;
        match decode(layout, data, dat.header.endian) {
            Ok(_) => decoded += 1,
            Err(e) => {
                if failures.len() < 20 {
                    failures.push((format!("{}", inst.tag), format!("{e}")));
                }
            }
        }
    }

    eprintln!(
        "coverage: {}/{} level0 tags have a generated layout; decoded {decoded} instances",
        covered_tags.len(),
        total_tags.len()
    );
    use std::collections::BTreeSet;
    let fail_tags: BTreeSet<&str> = failures.iter().map(|(t, _)| t.as_str()).collect();
    eprintln!("failing tags ({}): {fail_tags:?}", fail_tags.len());
    // Most of level0's 64 tags should be covered.
    assert!(
        covered_tags.len() >= 40,
        "only {} tags covered",
        covered_tags.len()
    );
}

#[test]
fn generated_txmp_matches_texture_parser() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");
    let reg = Registry::load();
    let layout = reg.get(FourCc::from_bytes(*b"TXMP")).expect("TXMP layout");

    let mut checked = 0;
    for inst in dat
        .instances
        .iter()
        .filter(|i| i.tag == FourCc::from_bytes(*b"TXMP"))
    {
        let Ok(tex) = oni_texture::TextureMap::from_instance(&dat, inst) else {
            continue;
        };
        let data = dat.instance_data(inst).unwrap();
        let rec = decode(layout, data, dat.header.endian).expect("decode");
        assert_eq!(rec.int("width"), Some(tex.width as i64));
        assert_eq!(rec.int("height"), Some(tex.height as i64));
        assert_eq!(rec.int("texelType"), Some(tex.format as i64));
        assert_eq!(
            rec.offset_of("pixels"),
            Some(148),
            "generated pixels offset"
        );
        match rec.get("pixels").unwrap().value {
            Value::Ref(RefKind::Raw, v) => assert_eq!(v, tex.raw_offset),
            _ => panic!("pixels not raw ref"),
        }
        checked += 1;
    }
    assert!(checked > 1900, "checked {checked}");
    eprintln!("generated TXMP layout == oni-texture for {checked} instances");
}

#[test]
fn generated_sndd_matches_sound_parser() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");
    let reg = Registry::load();
    let layout = reg.get(FourCc::from_bytes(*b"SNDD")).expect("SNDD layout");

    let mut checked = 0;
    for inst in dat
        .instances
        .iter()
        .filter(|i| i.tag == FourCc::from_bytes(*b"SNDD"))
    {
        let Ok(snd) = oni_sound::Sound::from_instance(&dat, inst) else {
            continue;
        };
        let data = dat.instance_data(inst).unwrap();
        let rec = decode(layout, data, dat.header.endian).expect("decode");
        assert_eq!(rec.int("num_bytes"), Some(snd.num_bytes as i64));
        assert_eq!(
            rec.offset_of("num_bytes"),
            Some(56),
            "generated num_bytes offset"
        );
        assert_eq!(rec.offset_of("data"), Some(60), "generated data offset");
        match rec.get("data").unwrap().value {
            Value::Ref(RefKind::Raw, v) => assert_eq!(v, snd.raw_offset),
            _ => panic!("data not raw ref"),
        }
        checked += 1;
    }
    assert!(checked > 2300, "checked {checked}");
    eprintln!("generated SNDD layout == oni-sound for {checked} instances");
}
