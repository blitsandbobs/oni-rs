//! `oni-convert` — convert an Oni GameDataFolder to the modern asset tree.
//!
//! Usage:
//!   cargo run -p oni-convert -- <input GameDataFolder> <output dir>

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: oni-convert <input GameDataFolder> <output dir>");
        return ExitCode::from(2);
    };

    match oni_convert::convert(Path::new(&input), Path::new(&output)) {
        Ok(report) => {
            let (mut t, mut s, mut m, mut c, mut a) = (0, 0, 0, 0, 0);
            for (root, st) in &report.levels {
                println!(
                    "{root:12} tex={:<5} snd={:<5} mesh={:<5} chars={:<4} anims={:<5}{}",
                    st.textures,
                    st.sounds,
                    st.meshes,
                    st.characters,
                    st.animations,
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
            }
            println!(
                "total: {t} textures, {s} sounds, {m} meshes, {c} characters, {a} animations -> {output}/oni.json"
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
