//! SNDD tests against the real level0 data, and (optionally) byte-exact
//! against OniSplit's WAV extraction.
//!
//! Defaults: `<repo>/original/gamedata/level0_Final.{dat,raw}` (override the dat
//! with `ONI_DAT_LEVEL0`). If `ONI_WAV_DIR` points at OniSplit `-extract:wav`
//! output, the byte-exact `data`-chunk comparison runs. Absent fixtures skip.

use std::collections::HashMap;
use std::path::PathBuf;

use oni_dat::Dat;
use oni_sound::sounds;

const EXPECTED_SNDD: usize = 2380;

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
fn level0_sounds_parse() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found (set ONI_DAT_LEVEL0)");
        return;
    };
    let raw_bytes = std::fs::read(dat_path.with_extension("raw")).expect("read raw");
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");

    let snds: Vec<_> = sounds(&dat).collect();
    let named = snds.iter().filter(|s| s.name.is_some()).count();
    assert_eq!(named, EXPECTED_SNDD, "named SNDD sounds");

    for s in &snds {
        // Format blob is well-formed and audio lies within .raw.
        assert_eq!(s.format.len(), 50);
        assert!(matches!(s.format_tag(), 1 | 2), "PCM or ADPCM");
        assert!(s.channels() == 1 || s.channels() == 2, "mono/stereo");
        assert!(
            (8000..=48000).contains(&s.sample_rate()),
            "sane sample rate: {}",
            s.sample_rate()
        );
        let audio = s.raw_audio(&raw_bytes).expect("audio in range");
        assert_eq!(audio.len(), s.num_bytes as usize);
        // WAV wrapper is well-formed.
        let wav = s.to_wav(&raw_bytes).expect("wav");
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
    }
}

#[test]
fn level0_sounds_byte_exact_vs_onisplit_wav() {
    let Ok(wav_dir) = std::env::var("ONI_WAV_DIR") else {
        eprintln!("SKIP: set ONI_WAV_DIR to OniSplit -extract:wav output to run byte-exact check");
        return;
    };
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let raw_bytes = std::fs::read(dat_path.with_extension("raw")).expect("read raw");
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");

    let snds: Vec<_> = sounds(&dat).collect();
    let by_name: HashMap<&str, &_> = snds
        .iter()
        .filter_map(|s| s.name.as_deref().map(|n| (n, s)))
        .collect();

    let mut checked = 0usize;
    for entry in std::fs::read_dir(&wav_dir).expect("read wav dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let Some(encoded) = stem.strip_prefix("SNDD") else {
            continue;
        };
        let name = percent_decode(encoded);
        let Some(s) = by_name.get(name.as_str()) else {
            continue;
        };

        let wav = std::fs::read(&path).expect("read wav");
        let data_pos = find_chunk(&wav, b"data").expect("data chunk");
        let dlen = u32::from_le_bytes(wav[data_pos + 4..data_pos + 8].try_into().unwrap()) as usize;
        let onisplit_audio = &wav[data_pos + 8..data_pos + 8 + dlen];

        let mine = s.raw_audio(&raw_bytes).expect("audio");
        assert_eq!(mine, onisplit_audio, "byte-exact audio for {name}");
        checked += 1;
    }
    assert!(
        checked > 2000,
        "expected to check most sounds, only did {checked}"
    );
    eprintln!("byte-exact vs OniSplit WAV: {checked} sounds matched");
}

fn find_chunk(wav: &[u8], id: &[u8; 4]) -> Option<usize> {
    let mut i = 12;
    while i + 8 <= wav.len() {
        let cid = &wav[i..i + 4];
        let sz = u32::from_le_bytes(wav[i + 4..i + 8].try_into().unwrap()) as usize;
        if cid == id {
            return Some(i); // position of the chunk id
        }
        i += 8 + sz + (sz & 1);
    }
    None
}

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

#[test]
fn level0_sounds_decode_to_pcm() {
    let Some(dat_path) = level0_dat() else {
        eprintln!("SKIP: level0 fixture not found");
        return;
    };
    let Ok(raw) = std::fs::read(dat_path.with_extension("raw")) else {
        eprintln!("SKIP: .raw not found");
        return;
    };
    let dat_bytes = std::fs::read(&dat_path).expect("read dat");
    let dat = Dat::parse(&dat_bytes).expect("parse dat");

    let mut decoded = 0usize;
    for snd in sounds(&dat).take(300) {
        let pcm = snd.decode_pcm(&raw).expect("decode pcm");
        assert!(!pcm.is_empty(), "non-empty pcm for {:?}", snd.name);
        // Decoded length is a whole number of channel-interleaved frames.
        assert_eq!(pcm.len() % snd.channels().max(1) as usize, 0);
        // A PCM WAV wraps it with a 44-byte header.
        let wav = snd.to_pcm_wav(&raw).expect("wav");
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1, "PCM format tag");
        decoded += 1;
    }
    assert!(decoded > 100, "decoded {decoded} sounds");
    eprintln!("decoded {decoded} sounds to PCM");
}
