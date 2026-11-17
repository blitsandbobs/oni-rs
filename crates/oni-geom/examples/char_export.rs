//! `char_export` — decode TRBS character bodies to glTF `.glb` (one node per part).
//!
//! Usage:
//!   cargo run -p oni-geom --example char_export -- <file.dat> <out_dir> [--limit N] [--filter SUBSTR]

use std::path::Path;
use std::process::ExitCode;

use oni_dat::Dat;
use oni_geom::character::{bodies, lod};
use oni_geom::glb::{scene_to_glb, SceneNode};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(dat_path), Some(out_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: char_export <file.dat> <out_dir> [--limit N] [--filter SUBSTR]");
        return ExitCode::from(2);
    };
    let mut limit = usize::MAX;
    let mut filter: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--limit" => limit = args.next().and_then(|s| s.parse().ok()).unwrap_or(limit),
            "--filter" => filter = args.next(),
            other => {
                eprintln!("unknown option: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let raw_bytes = std::fs::read(std::path::PathBuf::from(&dat_path).with_extension("raw")).ok();
    let dat = Dat::parse(&dat_bytes).expect("parse dat");
    std::fs::create_dir_all(&out_dir).expect("mkdir");

    // Default neutral pose: frame 0 of the unarmed standing idle.
    let idle_pose = raw_bytes.as_ref().and_then(|raw| {
        oni_geom::character::find_animation(&dat, "NINJAidle1")
            .and_then(|a| oni_geom::character::frame0_pose(&dat, a, raw))
    });

    let mut written = 0usize;
    for (idx, mut body) in bodies(&dat, lod::HIGH) {
        if let Some(pose) = &idle_pose {
            oni_geom::character::apply_pose(&mut body, pose, true);
        }
        if written >= limit {
            break;
        }
        let name = body.name.clone().unwrap_or_else(|| format!("body_{idx}"));
        if let Some(f) = &filter {
            if !name.contains(f.as_str()) {
                continue;
            }
        }
        let nodes: Vec<SceneNode> = body
            .parts
            .iter()
            .map(|p| SceneNode {
                name: p.bone.clone(),
                translation: p.translation,
                rotation: p.rotation,
                parent: p.parent,
                mesh: Some(&p.mesh),
                blend: false,
                texture_uri: p
                    .mesh
                    .base_texture
                    .as_ref()
                    .map(|t| format!("../textures/{t}.png")),
            })
            .collect();
        let glb = scene_to_glb(&nodes);
        let file = Path::new(&out_dir).join(format!("{}.glb", sanitize(&name)));
        std::fs::write(&file, &glb).expect("write glb");
        written += 1;
    }
    println!("wrote {written} character .glb(s) to {out_dir}");
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
