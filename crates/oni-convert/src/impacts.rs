//! Impact effects (`docs/sound/ORIGINAL_TRIGGERS.md` §1): what a footstep,
//! a landing, a blow, a bullet hit sounds like (and which particles and
//! AI noise go with it), by impact type × surface material × weight →
//! `common/impacts.json`.
//!
//! - The **impact types** (`Impt`) and **materials** (`Mtrl`) are trees: each
//!   names its parent (a template reference at offset 8).
//! - The textures' materials come from level 0's `TMBD` table
//!   (`materials.rs`).
//! - The **table** is level 0's binary data `ONIE` ("impact_effects"): a
//!   header (version 2; counts of impact types, materials, particles, sounds,
//!   entries, material indices), translators from the file's impact and
//!   material numbers — their place in the list — to names (128-byte name,
//!   then 4 bytes of runtime numbers; 132 bytes each),
//!   a lookup per impact (8 bytes), particle records (84: class name[64],
//!   collision orientation u32, location type u32, location data 8 bytes
//!   after a 4-byte runtime slot), sound records (48: impulse name[32], 8
//!   runtime bytes, u16 flags — bit 0: AIs hear it —, u16 AI sound type, f32
//!   AI radius), entries (20: u16 impact, u16 material, u16 component — 0
//!   impact, 1 damage, 2 projectile —, u16 modifier — 0 any, 1 heavy, 2
//!   medium, 3 light —, u16 particle count, pad, u32 sound record or
//!   0xFFFFFFFF, u32 first particle), then the material indices.

use std::collections::BTreeMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};
use oni_schema::files;
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

const TAG_IMPT: FourCc = FourCc(u32::from_be_bytes(*b"Impt"));
const TAG_MTRL: FourCc = FourCc(u32::from_be_bytes(*b"Mtrl"));
const TAG_BINA: FourCc = FourCc(u32::from_be_bytes(*b"BINA"));

const COMPONENTS: [&str; 3] = ["impact", "damage", "projectile"];
const MODIFIERS: [&str; 4] = ["any", "heavy", "medium", "light"];
/// The AI's sound kinds.
pub const AI_SOUNDS: [&str; 5] = ["ignore", "interesting", "danger", "melee", "gunshot"];

/// A tree of named templates of `tag`: name → parent name.
fn tree(dat: &Dat<'_>, tag: FourCc) -> BTreeMap<String, Option<String>> {
    let prefix = tag.to_string();
    let name_of = |k: usize| {
        dat.instances
            .get(k)
            .and_then(|i| i.name.as_deref())
            .map(|n| n.trim_start_matches(prefix.as_str()).to_string())
    };
    dat.instances
        .iter()
        .filter(|i| i.tag == tag)
        .filter_map(|i| {
            let d = dat.instance_data(i)?;
            let name = i
                .name
                .as_deref()?
                .trim_start_matches(prefix.as_str())
                .to_string();
            let r = le_u32(d, 8);
            Some((
                name,
                (r & 1 != 0).then(|| name_of((r >> 8) as usize)).flatten(),
            ))
        })
        .collect()
}

/// The impact-effect table's entries, by name.
fn table(b: &[u8]) -> Option<Vec<files::ImpactEntry>> {
    if b.len() < 28 || le_u32(b, 0) != 2 {
        return None;
    }
    let n = |k: usize| le_u32(b, 4 + k * 4) as usize;
    let (impacts, materials, particles, sounds, entries) = (n(0), n(1), n(2), n(3), n(4));
    let mut at = 28;
    let mut names = |count: usize| {
        let mut by_id = BTreeMap::new();
        for k in 0..count {
            let o = at + k * 132;
            // (Entries name them by their place in this list.)
            by_id.insert(k as u16, cstr(b, o, 128));
        }
        at += count * 132;
        by_id
    };
    let impact_names = names(impacts);
    let material_names = names(materials);
    at += impacts * 8;
    let particle_at = at;
    at += particles * 84;
    let sound_at = at;
    at += sounds * 48;
    let entry_at = at;
    if b.len() < entry_at + entries * 20 {
        return None;
    }
    let particle = |k: usize| {
        let o = particle_at + k * 84;
        // Its orientation (`P3tEnumCollisionOrientation`), where it goes
        // (`P3tEffectLocationType`) and that place's parameters.
        let orientation = ["unchanged", "reversed", "reflected", "normal"]
            .get(le_u32(b, o + 68) as usize)
            .copied()
            .unwrap_or("unchanged");
        use files::ImpactLocation as L;
        files::ImpactParticle {
            class: cstr(b, o, 64),
            orientation: orientation.to_string(),
            location: match le_u32(b, o + 72) {
                1 => L::CollisionOffset {
                    offset: le_f32(b, o + 76),
                },
                3 => L::FindWall {
                    radius: le_f32(b, o + 76),
                    look_along: b[o + 80] != 0,
                },
                4 => L::Decal {
                    stays: b[o + 76] != 0,
                    random_rotation: b[o + 77] != 0,
                },
                5 => L::AttachedToObject,
                _ => L::Default,
            },
        }
    };
    Some(
        (0..entries)
            .map(|k| {
                let o = entry_at + k * 20;
                let s = le_u32(b, o + 12);
                let sound = ((s as usize) < sounds).then(|| {
                    let so = sound_at + s as usize * 48;
                    files::ImpactSound {
                        impulse: cstr(b, so, 32),
                        ai: (le_u16(b, so + 40) & 1 != 0).then(|| files::AiNoise {
                            kind: AI_SOUNDS
                                .get(le_u16(b, so + 42) as usize)
                                .copied()
                                .unwrap_or("ignore")
                                .to_string(),
                            radius: le_f32(b, so + 44),
                        }),
                    }
                });
                let (first, count) = (le_u32(b, o + 16) as usize, le_u16(b, o + 8) as usize);
                files::ImpactEntry {
                    impact: impact_names.get(&le_u16(b, o)).cloned().unwrap_or_default(),
                    material: material_names
                        .get(&le_u16(b, o + 2))
                        .cloned()
                        .unwrap_or_default(),
                    component: COMPONENTS
                        .get(le_u16(b, o + 4) as usize)
                        .copied()
                        .unwrap_or("impact")
                        .to_string(),
                    modifier: MODIFIERS
                        .get(le_u16(b, o + 6) as usize)
                        .copied()
                        .unwrap_or("any")
                        .to_string(),
                    sound,
                    particles: if count > 0 && first + count <= particles {
                        (first..first + count).map(particle).collect()
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect(),
    )
}

/// A character class's (`ONCC`) sound-impact fields: the impact types its
/// movements make (`animImpacts`: 15 names, in the order walk, run, crouch,
/// slide, land, land hard, land dead, knockdown, thrown, standing turn, run
/// start, one step, run stop, walk stop, sprint — and the weight of its
/// steps), its special attacks' impacts (`ONIA`: attack name → impact,
/// weight), and per body part (19, part order) the material struck and the
/// impact a blow with that part makes on a hit, a block, a kill (`CBPM`,
/// `CBPI`).
pub fn class_fields(dat: &Dat<'_>, d: &[u8]) -> Value {
    let at = |r: u32| {
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
    };
    let name_of = |r: u32, prefix: &str| {
        at(r)
            .and_then(|i| i.name.as_deref())
            .map(|n| n.trim_start_matches(prefix).to_string())
    };
    let mut out = json!({});
    if d.len() >= 1100 + 15 * 130 {
        let names: Vec<String> = (0..15).map(|k| cstr(d, 1100 + k * 130, 128)).collect();
        out["animImpacts"] =
            json!({ "modifier": cstr(d, 1084, 16).to_lowercase(), "impacts": names });
    }
    // How it cries out (`painConstants` @116): eleven u16 tunings, the
    // quietest grunt's volume, the four impulse names.
    if d.len() >= 272 {
        let u = |k: usize| le_u16(d, 116 + k * 2);
        out["pain"] = json!({
            "basePercentage": u(0), "maxPercentage": u(1), "percentageThreshold": u(2),
            "timer": u(3), "minTimer": u(4), "maxLight": u(5), "maxMedium": u(6),
            "deathChance": u(7), "volumeThreshold": u(8), "mediumThreshold": u(9), "heavyThreshold": u(10),
            "minVolume": crate::le_f32(d, 140),
            "light": cstr(d, 144, 32), "medium": cstr(d, 176, 32), "heavy": cstr(d, 208, 32), "death": cstr(d, 240, 32),
        });
    }
    // What its AI says (`ai2_behavior.vocalizations`): a chance (%) and an
    // ambient per kind.
    if d.len() >= 692 + 10 * 32 {
        const KINDS: [&str; 10] = [
            "taunt",
            "alert",
            "startle",
            "checkBody",
            "pursue",
            "cower",
            "superPunch",
            "superKick",
            "super3",
            "super4",
        ];
        out["vocalizations"] = json!(KINDS
            .iter()
            .enumerate()
            .map(|(k, kind)| json!({ "kind": kind, "chance": d[680 + k], "sound": cstr(d, 692 + k * 32, 32) }))
            .collect::<Vec<_>>());
    }
    if let Some(a) = at(le_u32(d, 1076)).and_then(|i| dat.instance_data(i)) {
        let n = (le_u32(a, 20) as usize).min(a.len().saturating_sub(24) / 164);
        out["attackImpacts"] = json!((0..n)
            .map(|k| {
                let o = 24 + k * 164;
                json!({ "attack": cstr(a, o, 16), "impact": cstr(a, o + 16, 128), "modifier": cstr(a, o + 144, 16).to_lowercase() })
            })
            .collect::<Vec<_>>());
    }
    if let Some(m) = at(le_u32(d, 3132)).and_then(|i| dat.instance_data(i)) {
        out["partMaterials"] = json!((0..19)
            .map(|k| name_of(le_u32(m, k * 4), "Mtrl"))
            .collect::<Vec<_>>());
    }
    if let Some(m) = at(le_u32(d, 3136)).and_then(|i| dat.instance_data(i)) {
        let list = |s: usize| {
            (0..19)
                .map(|k| name_of(le_u32(m, (s * 19 + k) * 4), "Impt"))
                .collect::<Vec<_>>()
        };
        out["partImpacts"] = json!({ "hit": list(0), "blocked": list(1), "killed": list(2) });
    }
    out
}

/// Writes `impacts.json` into `common` (from level 0).
pub fn write(dat: &Dat<'_>, raw: &[u8], common: &Path) -> std::io::Result<usize> {
    let Some(inst) = dat
        .instances
        .iter()
        .find(|i| i.tag == TAG_BINA && i.name.as_deref() == Some("EINOimpact_effects"))
    else {
        return Ok(0);
    };
    let Some(bd) = dat.instance_data(inst) else {
        return Ok(0);
    };
    let start = le_u32(bd, 4) as usize + 8;
    let Some(entries) = raw.get(start..).and_then(table) else {
        return Ok(0);
    };
    let textures: BTreeMap<String, String> = crate::materials::texture_materials(dat, raw)
        .into_iter()
        .map(|(t, m)| (crate::sanitize(&t), m))
        .collect();
    let n = entries.len();
    std::fs::create_dir_all(common)?;
    std::fs::write(
        common.join("impacts.json"),
        serde_json::to_string_pretty(&files::ImpactsFile {
            impacts: tree(dat, TAG_IMPT),
            materials: tree(dat, TAG_MTRL),
            textures,
            entries,
        })?,
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_hand_built_table() {
        // 1 impact, 1 material, 0 particles, 1 sound, 1 entry.
        let mut b = vec![0u8; 28 + 132 * 2 + 8 + 48 + 20];
        for (k, v) in [2u32, 1, 1, 0, 1, 1].iter().enumerate() {
            b[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[28..28 + 4].copy_from_slice(b"Kick");
        b[160..160 + 5].copy_from_slice(b"Flesh");
        let s = 28 + 264 + 8;
        b[s..s + 7].copy_from_slice(b"bdy_med");
        b[s + 40..s + 42].copy_from_slice(&1u16.to_le_bytes());
        b[s + 42..s + 44].copy_from_slice(&3u16.to_le_bytes());
        b[s + 44..s + 48].copy_from_slice(&150f32.to_le_bytes());
        let e = s + 48;
        b[e + 4..e + 6].copy_from_slice(&2u16.to_le_bytes());
        b[e + 6..e + 8].copy_from_slice(&2u16.to_le_bytes());
        let t = serde_json::to_value(table(&b).unwrap()).unwrap();
        assert_eq!(t[0]["impact"], json!("Kick"));
        assert_eq!(t[0]["material"], json!("Flesh"));
        assert_eq!(
            (t[0]["component"].as_str(), t[0]["modifier"].as_str()),
            (Some("projectile"), Some("medium"))
        );
        assert_eq!(t[0]["sound"]["impulse"], json!("bdy_med"));
        assert_eq!(
            t[0]["sound"]["ai"],
            json!({ "type": "melee", "radius": 150.0 })
        );
    }
}
