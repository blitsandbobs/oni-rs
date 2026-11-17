//! `m3gm_export` — decode M3GM geometry from a `.dat`/`.raw` to glTF `.glb`.
//!
//! Usage:
//!   cargo run -p oni-geom --example m3gm_export -- <file.dat> <out_dir> [--limit N] [--min-tris N]

use std::path::Path;
use std::process::ExitCode;

use oni_dat::Dat;
use oni_geom::{glb, meshes};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(dat_path), Some(out_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: m3gm_export <file.dat> <out_dir> [--limit N] [--min-tris N]");
        return ExitCode::from(2);
    };
    let mut limit = usize::MAX;
    let mut min_tris = 0usize;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--limit" => limit = args.next().and_then(|s| s.parse().ok()).unwrap_or(limit),
            "--min-tris" => min_tris = args.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            other => {
                eprintln!("unknown option: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let dat_bytes = match std::fs::read(&dat_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {dat_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let dat = match Dat::parse(&dat_bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {out_dir}: {e}");
        return ExitCode::FAILURE;
    }

    let mut written = 0usize;
    let mut total_tris = 0usize;
    for (idx, mesh) in meshes(&dat) {
        if written >= limit {
            break;
        }
        if mesh.triangle_count() < min_tris {
            continue;
        }
        let tex_uri = mesh
            .base_texture
            .as_ref()
            .map(|n| format!("../textures/{n}.png"));
        let glb_bytes = glb::to_glb(&mesh, tex_uri.as_deref());
        let name = mesh
            .base_texture
            .clone()
            .unwrap_or_else(|| format!("mesh_{idx}"));
        let file = Path::new(&out_dir).join(format!("M3GM_{idx:05}_{}.glb", sanitize(&name)));
        if let Err(e) = std::fs::write(&file, &glb_bytes) {
            eprintln!("write {}: {e}", file.display());
            continue;
        }
        written += 1;
        total_tris += mesh.triangle_count();
    }
    println!("wrote {written} .glb mesh(es) to {out_dir}; {total_tris} triangles total");
    ExitCode::SUCCESS
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
