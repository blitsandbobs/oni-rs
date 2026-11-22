//! Placed characters → `levels/NN/characters.json` (//! *Data*; `oni_schema::files::CharactersFile`): every character the level's editor
//! placed — who, where, which team, what job, how alert, how it pursues.

use std::path::Path;

use oni_dat::Dat;
use oni_schema as sc;
use std::collections::BTreeMap;

use oni_schema::files;
use serde_json::Value;

use crate::{cstr, le_u16, le_u32};

/// The shipped object-file versions that change the record (`OT_Character.c`).
const V_HURT: u32 = 21;
const V_DEFEATED: u32 = 29;
const V_OUT_OF_AMMO: u32 = 34;
const V_NEUTRAL: u32 = 23;

/// A level's placed characters as JSON (None when it has none).
pub fn collect(dat: &Dat<'_>, raw: &[u8]) -> Option<files::CharactersFile> {
    let (version, objects) = crate::objects::read(dat, raw, "Character");
    if objects.is_empty() {
        return None;
    }
    let list: Vec<files::PlacedCharacter> = objects
        .iter()
        .filter_map(|o| character(o, version))
        .collect();
    Some(files::CharactersFile { characters: list })
}

fn character(o: &crate::objects::Object<'_>, version: u32) -> Option<files::PlacedCharacter> {
    let d = o.data;
    let flags_at = 0;
    let text = |at: usize, n: usize| (at + n <= d.len()).then(|| cstr(d, at, n));
    let class = text(4, 64)?;
    let name = text(68, 32)?;
    let weapon = text(100, 64)?;
    let mut at = 164usize;
    let mut scripts = BTreeMap::new();
    let mut script = |key: &str, at: &mut usize| {
        if *at + 32 <= d.len() {
            let s = cstr(d, *at, 32);
            if !s.is_empty() {
                scripts.insert(key.to_string(), s);
            }
        }
        *at += 32;
    };
    script("spawn", &mut at);
    for key in ["die", "combat", "alarm"] {
        script(key, &mut at);
    }
    if version >= V_HURT {
        script("hurt", &mut at);
    }
    if version >= V_DEFEATED {
        script("defeated", &mut at);
    }
    if version >= V_OUT_OF_AMMO {
        script("outOfAmmo", &mut at);
        script("noPath", &mut at);
    }
    let u32_at = |o: usize| (o + 4 <= d.len()).then(|| le_u32(d, o));
    let u16_at = |o: usize| (o + 2 <= d.len()).then(|| le_u16(d, o));
    let hit_points = u32_at(at)? as i32;
    let job = u32_at(at + 4)?;
    let job_id = u16_at(at + 8)?;
    let combat_id = u16_at(at + 10)?;
    let melee_id = u16_at(at + 12)?;
    let neutral_id = if version >= V_NEUTRAL {
        u16_at(at + 14)?
    } else {
        0
    };
    at += 16;
    let mut inventory = BTreeMap::new();
    for (k, kind) in sc::INVENTORY.iter().enumerate() {
        let (uses, drop) = (u16_at(at + k * 4)?, u16_at(at + k * 4 + 2)?);
        if uses != 0 || drop != 0 {
            inventory.insert(kind.to_string(), files::Carried { uses, drop });
        }
    }
    at += 24;
    let team = u32_at(at)?;
    let ammo = u32_at(at + 4)?;
    let alert =
        |o: usize| sc::name(&sc::ALERT_LEVELS, u32_at(o).unwrap_or(0) as u16).unwrap_or("lull");
    let pursue =
        |o: usize| sc::name(&sc::PURSUIT_MODES, u32_at(o).unwrap_or(0) as u16).unwrap_or("none");
    let alarm_groups: Vec<u32> = (0..32)
        .filter(|b| u32_at(at + 24).unwrap_or(0) & (1 << b) != 0)
        .collect();

    let name_of = |table: &[&'static str], v: u32, or: &str| {
        sc::name(table, v as u16).unwrap_or(or).to_string()
    };
    let id_of = |v: u16| (v != 0 && v != 0xFFFF).then_some(v);
    Some(files::PlacedCharacter {
        id: o.id,
        name,
        class,
        flags: sc::bit_names(&sc::CHARACTER_FLAGS, u32_at(flags_at)?)
            .into_iter()
            .map(String::from)
            .collect(),
        position: o.position,
        facing: o.rotation[1].to_radians(),
        team: name_of(&sc::TEAMS, team, "neutral"),
        job: name_of(&sc::JOBS, job, "none"),
        alert: files::AlertLevels {
            initial: alert(at + 8).to_string(),
            minimum: alert(at + 12).to_string(),
            job: alert(at + 16).to_string(),
            investigate: alert(at + 20).to_string(),
        },
        pursue: files::PursueModes {
            strong_unseen: pursue(at + 28).to_string(),
            weak_unseen: pursue(at + 32).to_string(),
            strong_seen: pursue(at + 36).to_string(),
            weak_seen: pursue(at + 40).to_string(),
            lost: name_of(&sc::PURSUIT_LOST, u32_at(at + 44).unwrap_or(0), "return"),
        },
        ammo_percent: ammo,
        weapon: Some(weapon).filter(|w| !w.is_empty()),
        scripts,
        hit_point_offset: (hit_points != 0).then_some(hit_points),
        // (0 is a real patrol / combat profile id.)
        job_id: (job_id != 0 || job == 3).then_some(job_id),
        combat_profile_id: combat_id,
        melee_profile_id: id_of(melee_id),
        neutral_id: id_of(neutral_id),
        inventory,
        alarm_groups,
    })
}

/// Writes `characters.json` into a level folder.
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<usize> {
    let Some(doc) = collect(dat, raw) else {
        return Ok(0);
    };
    let n = doc.characters.len();
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("characters.json"),
        serde_json::to_string_pretty(&doc)?,
    )?;
    Ok(n)
}

/// A level's character setups (`AISA`, `AItCharacterSetup`) as JSON: the
/// characters `chr_create` makes by script id — for the cutscenes — at their
/// default flag (`oni_schema::files::SetupsFile`).
pub fn setups(dat: &Dat<'_>, registry: &oni_template::Registry) -> Vec<files::CharacterSetup> {
    let tag = oni_dat::FourCc::from_bytes(*b"AISA");
    let Some(layout) = registry.get(tag) else {
        return Vec::new();
    };
    let text = |v: &Value| -> String {
        let bytes: Vec<u8> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| b.as_i64().unwrap_or(0) as u8)
            .take_while(|&b| b != 0)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let link = |v: &Value| v["$ref"].as_str().map(str::to_string);
    let mut out = Vec::new();
    for inst in dat.instances.iter().filter(|i| i.tag == tag) {
        let Some(data) = dat.instance_data(inst) else {
            continue;
        };
        let Ok(rec) = oni_template::decode(layout, data, dat.header.endian) else {
            continue;
        };
        let fields = crate::level_json::record_to_json(&rec, dat);
        for s in fields["characterSetups"].as_array().into_iter().flatten() {
            let mut scripts = BTreeMap::new();
            for (key, field) in [
                ("spawn", "spawn_name"),
                ("die", "die_name"),
                ("combat", "combat_name"),
                ("alarm", "alarm_name"),
                ("hurt", "hurt_name"),
                ("defeated", "defeated_name"),
                ("outOfAmmo", "outofammo_name"),
                ("noPath", "nopath_name"),
            ] {
                let name = text(&s[field]);
                if !name.is_empty() {
                    scripts.insert(key.to_string(), name);
                }
            }
            let nonempty = |t: String| Some(t).filter(|t| !t.is_empty());
            let weapon = link(&s["weapon"]);
            let c = files::CharacterSetup {
                script_id: s["defaultScriptID"].as_u64().unwrap_or(0) as u16,
                flag: s["defaultFlagID"].as_i64().unwrap_or(0) as i16,
                team: sc::name(&sc::TEAMS, s["teamNumber"].as_u64().unwrap_or(0) as u16)
                    .unwrap_or("neutral")
                    .to_string(),
                name: nonempty(text(&s["playerName"])),
                class: link(&s["characterClass"]),
                ammo: weapon
                    .as_ref()
                    .map(|_| s["ammo"].as_u64().unwrap_or(0) as u16),
                weapon,
                scripts,
                variable: nonempty(text(&s["variable"])),
            };
            out.push(c);
        }
    }
    out
}

/// Writes `setups.json` into a level folder (nothing when it has none).
pub fn write_setups(
    dat: &Dat<'_>,
    registry: &oni_template::Registry,
    root: &Path,
) -> std::io::Result<usize> {
    let list = setups(dat, registry);
    if list.is_empty() {
        return Ok(0);
    }
    let n = list.len();
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("setups.json"),
        serde_json::to_string_pretty(&files::SetupsFile { setups: list })?,
    )?;
    Ok(n)
}
