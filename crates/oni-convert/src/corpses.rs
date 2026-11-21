//! Pre-placed corpses (`ONtCorpseArray`, `CRSA`; `docs/ASSET_FORMAT.md`,
//! *Corpses*): bodies the level designers left lying where they fell — a
//! character class and each of its 19 body parts' world matrix.
//! Written to `levels/NN/corpses.json`.

use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u32};

const TAG_CRSA: FourCc = FourCc(u32::from_be_bytes(*b"CRSA"));
/// `pad0[12]`, `static_corpses`, `next`, `max_corpses`, then the corpses.
const STATIC_COUNT: usize = 12;
const MAX_CORPSES: usize = 20;
const CORPSES: usize = 24;
/// `corpse_name[32]`, `node_list[32]`, then `ONtCorpse_Data`: the class
/// (an instance reference), `matricies[19]` (4 × 3 floats each: the X, Y
/// and Z axes, then the place), `corpse_bbox`.
const CORPSE_SIZE: usize = 1100;
const MATRICES: usize = 164;
const BBOX: usize = 1076;
const PARTS: usize = 19;

/// The level's static corpses (the first `static_corpses`; the rest of the
/// array is room for the dead of play).
pub fn corpses(dat: &Dat<'_>, registry: &oni_template::Registry) -> Vec<Value> {
    let Some(inst) = dat.instances.iter().find(|i| i.tag == TAG_CRSA) else { return Vec::new() };
    let (Some(d), Some(layout)) = (dat.instance_data(inst), registry.get(TAG_CRSA)) else { return Vec::new() };
    // (The class reference by name, as the template decoder resolves it.)
    let fields = oni_template::decode(layout, d, dat.header.endian).map(|r| crate::level_json::record_to_json(&r, dat)).unwrap_or_default();
    let count = le_u32(d, STATIC_COUNT) as usize;
    (0..count)
        .filter_map(|k| {
            let at = CORPSES + k * CORPSE_SIZE;
            d.get(at..at + CORPSE_SIZE)?;
            let class = fields["corpses"][k]["corpse_data"]["characterClass"]["$ref"].as_str()?;
            let matrices: Vec<Vec<f32>> = (0..PARTS).map(|p| (0..12).map(|i| le_f32(d, at + MATRICES + p * 48 + i * 4)).collect()).collect();
            let v = |o: usize| [le_f32(d, at + o), le_f32(d, at + o + 4), le_f32(d, at + o + 8)];
            Some(json!({
                "name": cstr(d, at, 32),
                "class": class,
                "matrices": matrices,
                "bbox": { "min": v(BBOX), "max": v(BBOX + 12) },
            }))
        })
        .collect()
}

/// The array's size (`max_corpses`): its static corpses and the room after
/// them for the dead of play. None without an array.
pub fn max_corpses(dat: &Dat<'_>) -> Option<u32> {
    let inst = dat.instances.iter().find(|i| i.tag == TAG_CRSA)?;
    let d = dat.instance_data(inst)?;
    (d.len() >= CORPSES).then(|| le_u32(d, MAX_CORPSES))
}

pub fn write(dat: &Dat<'_>, registry: &oni_template::Registry, root: &Path) -> std::io::Result<()> {
    let Some(max) = max_corpses(dat) else { return Ok(()) };
    let c = corpses(dat, registry);
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("corpses.json"), serde_json::to_string_pretty(&json!({ "max": max, "corpses": c }))?)
}
