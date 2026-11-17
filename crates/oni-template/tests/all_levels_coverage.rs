//! Whole-corpus coverage: decode every instance in every level `.dat` with the
//! generated registry, reporting tag coverage and asserting nothing overruns.
//!
//! Scans `<repo>/original/gamedata/*.dat` by default (override the directory
//! with `ONI_GAMEDATA_DIR`); skips if absent.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use oni_dat::Dat;
use oni_template::{decode, Registry};

fn gamedata_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONI_GAMEDATA_DIR") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("original/gamedata");
    p.is_dir().then_some(p)
}

#[test]
fn all_levels_decode_without_overrun() {
    let Some(dir) = gamedata_dir() else {
        eprintln!("SKIP: gamedata dir not found (set ONI_GAMEDATA_DIR)");
        return;
    };
    let reg = Registry::load();

    let mut dats: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read gamedata")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("dat"))
        .collect();
    dats.sort();
    assert!(!dats.is_empty(), "no .dat files in {}", dir.display());

    let mut all_tags: BTreeMap<String, usize> = BTreeMap::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut total_instances = 0usize;
    let mut decoded = 0usize;
    let mut failures: Vec<(String, String, String)> = Vec::new(); // (level, tag, err)

    for path in &dats {
        let bytes = std::fs::read(path).expect("read dat");
        let dat = match Dat::parse(&bytes) {
            Ok(d) => d,
            Err(e) => panic!("parse {}: {e}", path.display()),
        };
        let level = path.file_stem().unwrap().to_string_lossy().to_string();
        for inst in &dat.instances {
            total_instances += 1;
            let tag = inst.tag.to_string();
            *all_tags.entry(tag.clone()).or_insert(0) += 1;
            let Some(layout) = reg.get(inst.tag) else {
                continue;
            };
            covered.insert(tag.clone());
            let Some(data) = dat.instance_data(inst) else {
                continue;
            };
            match decode(layout, data, dat.header.endian) {
                Ok(_) => decoded += 1,
                Err(e) => {
                    if failures.len() < 30 {
                        failures.push((level.clone(), tag.clone(), format!("{e}")));
                    }
                }
            }
        }
    }

    let uncovered: Vec<&String> = all_tags.keys().filter(|t| !covered.contains(*t)).collect();
    eprintln!("levels: {}", dats.len());
    eprintln!("distinct tags across all data: {}", all_tags.len());
    eprintln!("tags covered by a layout: {}", covered.len());
    eprintln!("total instances: {total_instances}, decoded: {decoded}");
    eprintln!("UNCOVERED tags ({}): {uncovered:?}", uncovered.len());
    if !failures.is_empty() {
        eprintln!("overrun failures ({}):", failures.len());
        for (l, t, e) in &failures {
            eprintln!("  [{l}] {t}: {e}");
        }
    }

    assert!(
        failures.is_empty(),
        "decode overruns across levels: see report above"
    );
}
