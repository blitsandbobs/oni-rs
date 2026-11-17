//! `txmp_extract` — decode TXMP textures from a `.dat`/`.raw` pair to PNG.
//!
//! Usage:
//!   cargo run -p oni-texture --example txmp_extract -- <file.dat> <out_dir> [--limit N] [--filter SUBSTR]
//!
//! The `.raw` file is found by swapping the `.dat` extension.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oni_dat::Dat;
use oni_texture::{textures, PixelType};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(dat_path), Some(out_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: txmp_extract <file.dat> <out_dir> [--limit N] [--filter SUBSTR]");
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

    let raw_path = PathBuf::from(&dat_path).with_extension("raw");
    let dat_bytes = match std::fs::read(&dat_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {dat_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let raw_bytes = match std::fs::read(&raw_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {}: {e}", raw_path.display());
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
    let mut skipped = 0usize;
    for tex in textures(&dat) {
        if written >= limit {
            break;
        }
        let name = tex.name.clone().unwrap_or_else(|| "<unnamed>".into());
        if let Some(f) = &filter {
            if !name.contains(f.as_str()) {
                continue;
            }
        }
        let rgba = match tex.decode_top_level(&raw_bytes) {
            Ok(px) => px,
            Err(e) => {
                eprintln!(
                    "skip {name} ({:?} {}x{}): {e}",
                    tex.format, tex.width, tex.height
                );
                skipped += 1;
                continue;
            }
        };
        // Index keeps filenames unique even for unnamed/duplicate textures.
        let file = Path::new(&out_dir).join(format!("TXMP_{:05}_{}.png", written, sanitize(&name)));
        if let Err(e) = write_png(&file, tex.width as u32, tex.height as u32, &rgba) {
            eprintln!("write {}: {e}", file.display());
            skipped += 1;
            continue;
        }
        written += 1;
    }

    // Small format tally so the run is self-describing.
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for tex in textures(&dat) {
        *counts.entry(fmt_name(tex.format)).or_insert(0) += 1;
    }
    println!("wrote {written} PNG(s) to {out_dir} (skipped {skipped})");
    println!("TXMP formats present: {counts:?}");
    ExitCode::SUCCESS
}

fn fmt_name(p: PixelType) -> &'static str {
    match p {
        PixelType::Dxt1 => "DXT1",
        PixelType::Argb4444 => "ARGB4444",
        PixelType::Rgb555 => "RGB555",
        PixelType::Argb1555 => "ARGB1555",
        PixelType::Rgb888 => "RGB888",
        _ => "other",
    }
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

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let w_buf = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(w_buf, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(rgba)
        .map_err(std::io::Error::other)?;
    Ok(())
}
