//! Animation sounds (`docs/sound/ORIGINAL_TRIGGERS.md` §2): what a
//! character's moves sound like — per character **variant** (`ONCV`: a
//! tree, each naming its parent), rows of animation-sound type × weight →
//! impulse, or a named clip → impulse on a frame — → `common/anim_sounds.json`.
//!
//! The rows are level 0's binary data `SABD`, one per variant (named
//! `DBAS<variant>`): tagged chunks (tag u32, length u32, data): `SAFT`
//! (version), `SAVT` (the variant's name[32]), then `SASA` rows — frame u32,
//! weight name[32], type name[32], clip name[32], impulse name[32].

use std::collections::BTreeMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};
use oni_schema::files;

use crate::{cstr, le_u32};

const TAG_ONCV: FourCc = FourCc(u32::from_be_bytes(*b"ONCV"));
const TAG_BINA: FourCc = FourCc(u32::from_be_bytes(*b"BINA"));

/// One variant's rows.
fn rows(b: &[u8]) -> Vec<files::AnimSound> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= b.len() {
        let (tag, len) = (&b[at..at + 4], le_u32(b, at + 4) as usize);
        let body = &b[(at + 8).min(b.len())..(at + 8 + len).min(b.len())];
        if (tag == b"ASAS" || tag == b"SASA") && body.len() >= 132 {
            let clip = Some(cstr(body, 68, 32)).filter(|c| !c.is_empty());
            out.push(files::AnimSound {
                kind: cstr(body, 36, 32),
                modifier: cstr(body, 4, 32).to_lowercase(),
                impulse: cstr(body, 100, 32),
                frame: clip.as_ref().map(|_| le_u32(body, 0)),
                clip,
            });
        }
        at += 8 + len;
    }
    out
}

/// Writes `anim_sounds.json` into `common` (from level 0): the variant tree
/// (by name), the variants' instance names (as classes name them) and each
/// variant's rows.
pub fn write(dat: &Dat<'_>, raw: &[u8], common: &Path) -> std::io::Result<usize> {
    let at = |r: u32| {
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
    };
    let variants: BTreeMap<String, Option<String>> = dat
        .instances
        .iter()
        .filter(|i| i.tag == TAG_ONCV)
        .filter_map(|i| {
            let d = dat.instance_data(i)?;
            let parent = at(le_u32(d, 0))
                .and_then(|p| dat.instance_data(p))
                .map(|p| cstr(p, 4, 32));
            Some((cstr(d, 4, 32), parent))
        })
        .collect();
    // (Classes name their variant by its instance name.)
    let instances: BTreeMap<String, String> = dat
        .instances
        .iter()
        .filter(|i| i.tag == TAG_ONCV)
        .filter_map(|i| {
            Some((
                i.name.as_deref()?.trim_start_matches("ONCV").to_string(),
                cstr(dat.instance_data(i)?, 4, 32),
            ))
        })
        .collect();
    let mut sounds = BTreeMap::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_BINA) {
        let Some(name) = inst.name.as_deref().and_then(|n| n.strip_prefix("DBAS")) else {
            continue;
        };
        let Some(bd) = dat.instance_data(inst) else {
            continue;
        };
        let (start, size) = (le_u32(bd, 4) as usize + 8, le_u32(bd, 0) as usize);
        let Some(b) = raw.get(start..start + size.saturating_sub(8)) else {
            continue;
        };
        let r = rows(b);
        if !r.is_empty() {
            sounds.insert(name.to_string(), r);
        }
    }
    let n = sounds.values().map(Vec::len).sum();
    std::fs::create_dir_all(common)?;
    std::fs::write(
        common.join("anim_sounds.json"),
        serde_json::to_string_pretty(&files::AnimSoundsFile {
            variants,
            instances,
            sounds,
        })?,
    )?;
    Ok(n)
}
