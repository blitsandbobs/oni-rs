//! `sndd_extract` — extract SNDD sounds from a `.dat`/`.raw` pair to WAV.
//!
//! Usage:
//!   cargo run -p oni-sound --example sndd_extract -- <file.dat> <out_dir> [--limit N] [--filter SUBSTR]
//!
//! Produces RIFF/WAVE files whose `data` chunk is byte-identical to OniSplit's
//! `-extract:wav`. MS-ADPCM playback works in QuickTime/VLC/`afplay`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oni_dat::Dat;
use oni_sound::sounds;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(dat_path), Some(out_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: sndd_extract <file.dat> <out_dir> [--limit N] [--filter SUBSTR]");
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
    let mut adpcm = 0usize;
    for snd in sounds(&dat) {
        if written >= limit {
            break;
        }
        let name = snd.name.clone().unwrap_or_else(|| "unnamed".into());
        if let Some(f) = &filter {
            if !name.contains(f.as_str()) {
                continue;
            }
        }
        let wav = match snd.to_wav(&raw_bytes) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("skip {name}: {e}");
                skipped += 1;
                continue;
            }
        };
        if snd.is_compressed() {
            adpcm += 1;
        }
        let file = Path::new(&out_dir).join(format!("SNDD_{:05}_{}.wav", written, sanitize(&name)));
        if let Err(e) = std::fs::write(&file, &wav) {
            eprintln!("write {}: {e}", file.display());
            skipped += 1;
            continue;
        }
        written += 1;
    }
    println!("wrote {written} WAV(s) to {out_dir} (skipped {skipped}; {adpcm} MS-ADPCM)");
    ExitCode::SUCCESS
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
