//! Golden test: parse `level0_Final.dat` and check it against OniSplit.
//!
//! OniSplit `-export <dir> level0_Final.dat` writes one `.oni` file per
//! *named, non-placeholder* instance, named `<TAG><name>`. We compare our
//! parse to two frozen oracle artifacts produced from that export:
//!
//!   * the total instance count and other header invariants, and
//!   * the per-tag histogram of named, non-placeholder instances
//!     (`level0_export_tag_histogram.txt`, one `"<count> <TAG>"` per line).
//!
//! The data and oracle live outside the workspace (they are copyrighted game
//! assets). Paths default to `../original/...` / `../generated/...` but can be overridden with the
//! `ONI_DAT_LEVEL0` and `ONI_ORACLE_HISTOGRAM` environment variables. When the
//! fixtures are absent the test is skipped so the suite still passes on a
//! machine without the game data.

use std::collections::BTreeMap;
use std::path::PathBuf;

use oni_dat::{ByteOrder, Dat};

/// Ground-truth counts for the Anniversary Edition `level0_Final.dat`.
const EXPECTED_TOTAL_INSTANCES: usize = 30527;
const EXPECTED_NAMED_INSTANCES: usize = 11032;
const EXPECTED_TEMPLATES: usize = 64;
/// Named instances minus placeholders == OniSplit's exported `.oni` count.
const EXPECTED_EXPORTED: usize = 10988;

fn fixture(env: &str, default_rel: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env) {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    // Default: <repo>/original or generated — the repo root is two levels above the
    // workspace (CARGO_MANIFEST_DIR is <repo>/oni-rs/crates/<crate>).
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p = manifest.join("../../..").join(default_rel);
    p.exists().then(|| p.canonicalize().unwrap_or(p))
}

fn load_oracle_histogram(path: &PathBuf) -> BTreeMap<String, usize> {
    let text = std::fs::read_to_string(path).expect("read oracle histogram");
    let mut h = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        // Format: "<count> <TAG>" where TAG is exactly 4 chars (may contain '_').
        let (count, tag) = line.split_once(' ').expect("count<space>tag");
        h.insert(
            tag.to_string(),
            count.trim().parse().expect("count is a number"),
        );
    }
    h
}

#[test]
fn level0_matches_onisplit() {
    let Some(dat_path) = fixture("ONI_DAT_LEVEL0", "original/gamedata/level0_Final.dat") else {
        eprintln!("SKIP: level0_Final.dat fixture not found (set ONI_DAT_LEVEL0)");
        return;
    };
    let Some(oracle_path) = fixture(
        "ONI_ORACLE_HISTOGRAM",
        "generated/oracle/level0_export_tag_histogram.txt",
    ) else {
        eprintln!("SKIP: oracle histogram not found (set ONI_ORACLE_HISTOGRAM)");
        return;
    };

    let bytes = std::fs::read(&dat_path).expect("read level0_Final.dat");
    let dat = Dat::parse(&bytes).expect("parse level0_Final.dat");

    // --- Header invariants ---
    assert_eq!(
        dat.header.endian,
        ByteOrder::Little,
        "AE data is PC/little-endian"
    );
    assert_eq!(
        dat.instances.len(),
        dat.header.num_instances as usize,
        "parsed instance count must equal header count"
    );
    assert_eq!(
        dat.instances.len(),
        EXPECTED_TOTAL_INSTANCES,
        "total instances"
    );
    assert_eq!(
        dat.named_count(),
        EXPECTED_NAMED_INSTANCES,
        "named instances"
    );
    assert_eq!(
        dat.templates.len(),
        EXPECTED_TEMPLATES,
        "template descriptors"
    );

    // Every tag that actually appears must be printable, and index must be dense.
    for (i, inst) in dat.instances.iter().enumerate() {
        assert_eq!(inst.index as usize, i, "instance index is its position");
        assert!(
            inst.tag.is_printable(),
            "instance {i} has non-printable tag {:?}",
            inst.tag
        );
    }

    // --- Oracle comparison: named, non-placeholder, grouped by tag ---
    let mut mine: BTreeMap<String, usize> = BTreeMap::new();
    let mut exported = 0usize;
    for inst in dat
        .instances
        .iter()
        .filter(|i| i.is_named() && !i.is_placeholder())
    {
        exported += 1;
        *mine.entry(inst.tag.to_string()).or_insert(0) += 1;
    }
    assert_eq!(
        exported, EXPECTED_EXPORTED,
        "named non-placeholder == OniSplit export count"
    );

    let oracle = load_oracle_histogram(&oracle_path);
    assert_eq!(
        mine, oracle,
        "per-tag histogram of exportable instances must match OniSplit exactly"
    );

    // --- Data access sanity: a non-placeholder named instance yields bytes ---
    let sample = dat
        .instances
        .iter()
        .find(|i| i.is_named() && !i.is_placeholder() && i.size > 0)
        .expect("at least one sized named instance");
    let data = dat.instance_data(sample).expect("instance data in range");
    assert_eq!(
        data.len(),
        sample.size as usize,
        "returned slice length == recorded size"
    );
}
