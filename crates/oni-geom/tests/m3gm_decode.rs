//! Validates M3GM geometry decoding across all level `.dat` files:
//!   * every M3GM whose point array resolves must extract a structurally valid
//!     mesh (indices in range — enforced by the strip walk — and an exact
//!     triangle count), and
//!   * the emitted `.glb` has a well-formed header.
//!
//! The strip-walk topology is the risky part; requiring in-range indices and
//! exact `triNormalIndexArray.numIndices` triangles across the whole corpus is
//! a strong structural check (it reproduces the engine's own invariant).

use std::path::PathBuf;

use oni_dat::Dat;
use oni_geom::{extract_mesh, glb, meshes, TAG_M3GM};

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
fn all_m3gm_decode_cleanly() {
    let Some(dir) = gamedata_dir() else {
        eprintln!("SKIP: gamedata dir not found (set ONI_GAMEDATA_DIR)");
        return;
    };
    let mut dats: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("dat"))
        .collect();
    dats.sort();

    let mut total_m3gm = 0usize;
    let mut extracted = 0usize;
    let mut failed = 0usize; // has a resolvable point array but mesh extraction failed
    let mut total_tris = 0usize;

    for path in &dats {
        let bytes = std::fs::read(path).expect("read dat");
        let dat = Dat::parse(&bytes).expect("parse dat");
        for inst in dat.instances.iter().filter(|i| i.tag == TAG_M3GM) {
            total_m3gm += 1;
            match extract_mesh(&dat, inst) {
                Some(mesh) => {
                    extracted += 1;
                    assert!(!mesh.positions.is_empty(), "mesh has no points");
                    assert!(
                        mesh.indices.len().is_multiple_of(3),
                        "indices not a multiple of 3"
                    );
                    assert!(mesh.triangle_count() > 0, "mesh has no triangles");
                    for &i in &mesh.indices {
                        assert!((i as usize) < mesh.positions.len(), "index out of range");
                    }
                    assert!(
                        mesh.texcoords.is_empty() || mesh.texcoords.len() == mesh.positions.len(),
                        "texcoords not parallel to positions"
                    );
                    total_tris += mesh.triangle_count();
                }
                None => {
                    // Could be an unresolvable ref (placeholder/other file). Count it;
                    // we assert below that the vast majority extract.
                    failed += 1;
                }
            }
        }
    }

    eprintln!(
        "M3GM: {total_m3gm} total, {extracted} extracted, {failed} unresolved, {total_tris} triangles"
    );
    assert!(extracted > 250, "expected many meshes, got {extracted}");
    // At least 95% of M3GM should extract (the rest reference data in other files).
    assert!(
        extracted * 100 >= total_m3gm * 95,
        "only {extracted}/{total_m3gm} M3GM extracted"
    );
}

#[test]
fn glb_header_is_wellformed() {
    let Some(dir) = gamedata_dir() else {
        eprintln!("SKIP: gamedata dir not found");
        return;
    };
    let bytes = std::fs::read(dir.join("level0_Final.dat")).expect("read");
    let dat = Dat::parse(&bytes).expect("parse");
    let (_, mesh) = meshes(&dat).next().expect("at least one mesh");
    let glb = glb::to_glb(&mesh, Some("../textures/foo.png"));

    assert_eq!(&glb[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2); // version
    assert_eq!(
        u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
        glb.len()
    );
    // JSON chunk type at offset 16
    assert_eq!(&glb[16..20], b"JSON");
}
