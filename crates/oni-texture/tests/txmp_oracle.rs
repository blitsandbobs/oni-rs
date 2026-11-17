//! TXMP tests against the real level0 data, and (optionally) byte-exact
//! against OniSplit's DDS extraction.
//!
//! Defaults: `../original/gamedata/level0_Final.{dat,raw}`. Override the dat
//! with `ONI_DAT_LEVEL0`. If `ONI_DDS_DIR` points at a directory of OniSplit
//! `-extract:dds` output, the byte-exact comparison runs too. Absent fixtures
//! cause the test to skip, not fail.

use std::path::PathBuf;

use oni_dat::Dat;
use oni_texture::{textures, PixelType, TextureMap};

/// All non-placeholder TXMP instances carry pixel data in `.raw`.
const EXPECTED_TXMP_WITH_PIXELS: usize = 1957;
/// Of those, the ones OniSplit exports as standalone files (named, i.e. not a
/// unique sub-texture inlined into a parent instance).
const EXPECTED_TXMP_NAMED: usize = 1320;

fn workspace_rel(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(rel)
}

fn level0_dat() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONI_DAT_LEVEL0") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let p = workspace_rel("original/gamedata/level0_Final.dat");
    p.exists().then_some(p)
}

#[test]
fn level0_textures_parse_and_decode() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found (set ONI_DAT_LEVEL0)");
        return;
    };
    let raw_path = dat_path.with_extension("raw");
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let raw_bytes = std::fs::read(&raw_path).expect("read raw");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");

    let texs: Vec<TextureMap> = textures(&dat).collect();

    // Count of TXMP instances carrying pixel data in .raw.
    let with_pixels = texs.iter().filter(|t| t.raw_offset != 0).count();
    assert_eq!(
        with_pixels, EXPECTED_TXMP_WITH_PIXELS,
        "TXMP textures with raw pixels"
    );
    let named = texs
        .iter()
        .filter(|t| t.name.is_some() && t.raw_offset != 0)
        .count();
    assert_eq!(
        named, EXPECTED_TXMP_NAMED,
        "named (OniSplit-exported) TXMP textures"
    );

    for t in &texs {
        assert!(t.width > 0 && t.height > 0, "sane dims for {:?}", t.name);
        if t.raw_offset != 0 {
            // Full stored region (all mips) must lie within .raw.
            let px = t.raw_pixels(&raw_bytes);
            assert!(px.is_some(), "raw pixels in range for {:?}", t.name);
            assert_eq!(px.unwrap().len(), t.stored_size() as usize);
        }
    }

    // Decode one of each format we expect, as a smoke test.
    for want in [
        PixelType::Dxt1,
        PixelType::Argb4444,
        PixelType::Rgb555,
        PixelType::Rgb888,
    ] {
        if let Some(t) = texs.iter().find(|t| t.format == want && t.raw_offset != 0) {
            let rgba = t.decode_top_level(&raw_bytes).expect("decode");
            assert_eq!(rgba.len(), t.width as usize * t.height as usize * 4);
        }
    }
}

#[test]
fn level0_textures_byte_exact_vs_onisplit_dds() {
    let Ok(dds_dir) = std::env::var("ONI_DDS_DIR") else {
        eprintln!("SKIP: set ONI_DDS_DIR to OniSplit -extract:dds output to run byte-exact check");
        return;
    };
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let raw_bytes = std::fs::read(dat_path.with_extension("raw")).expect("read raw");
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");

    // Index our named textures by name.
    use std::collections::HashMap;
    let texs: Vec<_> = textures(&dat).collect();
    let by_name: HashMap<&str, &_> = texs
        .iter()
        .filter(|t| t.raw_offset != 0)
        .filter_map(|t| t.name.as_deref().map(|n| (n, t)))
        .collect();

    // Walk OniSplit's DDS output: filename is "TXMP" + percent-encoded(name).
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&dds_dir).expect("read dds dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("dds") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let Some(encoded) = stem.strip_prefix("TXMP") else {
            continue;
        };
        let name = percent_decode(encoded);
        let Some(t) = by_name.get(name.as_str()) else {
            continue;
        };

        let dds = std::fs::read(&path).expect("read dds");
        let payload = &dds[128..]; // skip the 128-byte DDS header
        let mine = t.raw_pixels(&raw_bytes).expect("raw pixels");
        // OniSplit's DDS may omit the final 1×1 mip; our region is a superset,
        // so compare on the payload length (the content it does include).
        assert!(
            mine.len() >= payload.len(),
            "our region should cover OniSplit's for {name}"
        );
        assert_eq!(
            &mine[..payload.len()],
            payload,
            "byte-exact pixels for {name}"
        );
        checked += 1;
    }
    assert!(
        checked > 1000,
        "expected to check most textures, only did {checked}"
    );
    eprintln!("byte-exact vs OniSplit DDS: {checked} textures matched");
}

/// Decodes `%XX` escapes (OniSplit's filename encoding) back to the raw name.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out.iter().map(|&c| c as char).collect()
}
