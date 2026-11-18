//! Placed characters → `levels/NN/characters.json` (`docs/PERCEPTION.md`,
//! *Data*; `docs/ASSET_FORMAT.md`): every character the level's editor
//! placed — who, where, which team, what job, how alert, how it pursues.

use std::path::Path;

use oni_dat::Dat;
use oni_schema as sc;
use serde_json::{json, Value};

use crate::{cstr, le_u16, le_u32};

/// The shipped object-file versions that change the record (`OT_Character.c`).
const V_HURT: u32 = 21;
const V_DEFEATED: u32 = 29;
const V_OUT_OF_AMMO: u32 = 34;
const V_NEUTRAL: u32 = 23;

/// A level's placed characters as JSON (None when it has none).
pub fn collect(dat: &Dat<'_>, raw: &[u8]) -> Option<Value> {
    let (version, objects) = crate::objects::read(dat, raw, "Character");
    if objects.is_empty() {
        return None;
    }
    let list: Vec<Value> = objects.iter().filter_map(|o| character(o, version)).collect();
    Some(json!({ "characters": list }))
}

fn character(o: &crate::objects::Object<'_>, version: u32) -> Option<Value> {
    let d = o.data;
    let flags_at = 0;
    let text = |at: usize, n: usize| (at + n <= d.len()).then(|| cstr(d, at, n));
    let class = text(4, 64)?;
    let name = text(68, 32)?;
    let weapon = text(100, 64)?;
    let mut at = 164usize;
    let mut scripts = serde_json::Map::new();
    let mut script = |key: &str, at: &mut usize| {
        if *at + 32 <= d.len() {
            let s = cstr(d, *at, 32);
            if !s.is_empty() {
                scripts.insert(key.into(), json!(s));
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
    let neutral_id = if version >= V_NEUTRAL { u16_at(at + 14)? } else { 0 };
    at += 16;
    let mut inventory = serde_json::Map::new();
    for (k, kind) in sc::INVENTORY.iter().enumerate() {
        let (use_, drop) = (u16_at(at + k * 4)?, u16_at(at + k * 4 + 2)?);
        if use_ != 0 || drop != 0 {
            inventory.insert((*kind).into(), json!({ "use": use_, "drop": drop }));
        }
    }
    at += 24;
    let team = u32_at(at)?;
    let ammo = u32_at(at + 4)?;
    let alert = |o: usize| sc::name(&sc::ALERT_LEVELS, u32_at(o).unwrap_or(0) as u16).unwrap_or("lull");
    let pursue = |o: usize| sc::name(&sc::PURSUIT_MODES, u32_at(o).unwrap_or(0) as u16).unwrap_or("none");
    let alarm_groups: Vec<u32> = (0..32).filter(|b| u32_at(at + 24).unwrap_or(0) & (1 << b) != 0).collect();

    let mut c = json!({
        "id": o.id,
        "name": name,
        "class": class,
        "flags": sc::bit_names(&sc::CHARACTER_FLAGS, u32_at(flags_at)?),
        "position": o.position,
        "facing": o.rotation[1].to_radians(),
        "team": sc::name(&sc::TEAMS, team as u16).unwrap_or("neutral"),
        "job": sc::name(&sc::JOBS, job as u16).unwrap_or("none"),
        "alert": {
            "initial": alert(at + 8),
            "minimum": alert(at + 12),
            "job": alert(at + 16),
            "investigate": alert(at + 20),
        },
        "pursue": {
            "strongUnseen": pursue(at + 28),
            "weakUnseen": pursue(at + 32),
            "strongSeen": pursue(at + 36),
            "weakSeen": pursue(at + 40),
            "lost": sc::name(&sc::PURSUIT_LOST, u32_at(at + 44).unwrap_or(0) as u16).unwrap_or("return"),
        },
        "ammoPercent": ammo,
    });
    let set = |c: &mut Value, k: &str, v: Value| c[k] = v;
    if !weapon.is_empty() {
        set(&mut c, "weapon", json!(weapon));
    }
    if !scripts.is_empty() {
        set(&mut c, "scripts", Value::Object(scripts));
    }
    if hit_points != 0 {
        set(&mut c, "hitPointOffset", json!(hit_points));
    }
    // (0 is a real patrol / combat profile id.)
    if job_id != 0 || job == 3 {
        set(&mut c, "jobId", json!(job_id));
    }
    set(&mut c, "combatProfileId", json!(combat_id));
    for (k, v) in [("meleeProfileId", melee_id), ("neutralId", neutral_id)] {
        if v != 0 && v != 0xFFFF {
            set(&mut c, k, json!(v));
        }
    }
    if !inventory.is_empty() {
        set(&mut c, "inventory", Value::Object(inventory));
    }
    if !alarm_groups.is_empty() {
        set(&mut c, "alarmGroups", json!(alarm_groups));
    }
    Some(c)
}

/// Writes `characters.json` into a level folder.
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<usize> {
    let Some(doc) = collect(dat, raw) else { return Ok(0) };
    let n = doc["characters"].as_array().map_or(0, Vec::len);
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("characters.json"), serde_json::to_string_pretty(&doc)?)?;
    Ok(n)
}
