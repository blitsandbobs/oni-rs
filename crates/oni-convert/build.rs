//! The converter's build: a hash of every source that shapes its output
//! (this crate and the format crates it uses), so a converted tree can say
//! which converter made it and an install can tell it is out of date
//! (`oni_convert::BUILD`, `oni.json`'s `build`).

use std::path::{Path, PathBuf};

/// The crates whose code decides what the converter writes.
const CRATES: [&str; 9] = [
    "oni-convert",
    "oni-geom",
    "oni-texture",
    "oni-sound",
    "oni-template",
    "oni-dat",
    "oni-bytes",
    "oni-fourcc",
    "oni-schema",
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x == "rs" || x == "json" || x == "toml")
        {
            out.push(p);
        }
    }
}

fn main() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut all = Vec::new();
    for c in CRATES {
        let src = crates.join(c).join("src");
        println!("cargo:rerun-if-changed={}", src.display());
        files(&src, &mut all);
    }
    all.sort();
    // FNV-1a over each file's path (from the crates folder) and bytes.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for p in &all {
        let rel = p.strip_prefix(&crates).unwrap_or(p);
        eat(rel.to_string_lossy().replace('\\', "/").as_bytes());
        eat(&std::fs::read(p).unwrap_or_default());
    }
    println!("cargo:rustc-env=ONI_CONVERT_BUILD={h:016x}");
}
