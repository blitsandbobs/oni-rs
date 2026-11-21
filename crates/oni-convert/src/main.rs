//! `oni-convert` — convert an Oni GameDataFolder to the modern asset tree.
//!
//! Usage:
//!   cargo run -p oni-convert -- <input GameDataFolder> <output dir> [--scripts <dir>]
//!
//! `--scripts` names the folder holding the levels' original scripts
//! (`<Level>/*.bsl`); by default the extracted Anniversary Edition's
//! vanilla scripts next to the game data are used when present.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: oni-convert <input GameDataFolder> <output dir>");
        return ExitCode::from(2);
    };

    let rest: Vec<String> = args.collect();
    let scripts = rest
        .iter()
        .position(|a| a == "--scripts")
        .and_then(|i| rest.get(i + 1));
    let result = match scripts {
        Some(dir) => {
            oni_convert::convert_with(Path::new(&input), Path::new(&output), Some(Path::new(dir)))
        }
        None => oni_convert::convert(Path::new(&input), Path::new(&output)),
    };
    match result {
        Ok(report) => {
            let (mut t, mut s, mut m, mut c, mut a, mut p) = (0, 0, 0, 0, 0, 0);
            for (root, st) in &report.levels {
                println!(
                    "{root:12} tex={:<5} snd={:<5} mesh={:<5} chars={:<4} anims={:<5} parts={:<4}{}",
                    st.textures,
                    st.sounds,
                    st.meshes,
                    st.characters,
                    st.animations,
                    st.particles,
                    if st.warnings.is_empty() {
                        String::new()
                    } else {
                        format!("  ({} warnings)", st.warnings.len())
                    }
                );
                t += st.textures;
                s += st.sounds;
                m += st.meshes;
                c += st.characters;
                a += st.animations;
                p += st.particles;
            }
            println!(
                "total: {t} textures, {s} sounds, {m} meshes, {c} characters, {a} animations, {p} particles -> {output}/oni.json"
            );
            println!(
                "references: {} resolved, {} dangling",
                report.refs_resolved, report.refs_dangling
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("convert failed: {e}");
            ExitCode::FAILURE
        }
    }
}
