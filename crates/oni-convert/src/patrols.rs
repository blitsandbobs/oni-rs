//! Patrol paths, flags and combat profiles (`docs/GOALS.md`,
//! `docs/ASSET_FORMAT.md`): the waypoint programs patrolling characters
//! follow, the marker flags they (and scripts) refer to, and the combat
//! profiles placed characters fight by.

use std::path::Path;

use oni_dat::Dat;
use oni_schema as sc;
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

fn i16_at(d: &[u8], o: usize) -> i16 {
    le_u16(d, o) as i16
}

/// A level's patrol paths.
fn patrols(dat: &Dat<'_>, raw: &[u8]) -> Vec<Value> {
    let (version, objects) = crate::objects::read(dat, raw, "Patrol Path");
    objects
        .iter()
        .filter_map(|o| {
            let d = o.data;
            if d.len() < 36 {
                return None;
            }
            let n = le_u32(d, 32) as usize;
            let (id, flags, mut at) = match version {
                v if v >= 27 => (le_u16(d, 36), le_u16(d, 38), 40),
                v if v >= 5 => (le_u16(d, 36), 0, 38),
                _ => (0, 0, 36),
            };
            let mut waypoints = Vec::new();
            for _ in 0..n {
                if at + 4 > d.len() {
                    break;
                }
                let kind = le_u32(d, at);
                at += 4;
                let name = sc::name(&sc::PATROL_WAYPOINTS, kind as u16).unwrap_or("unknown");
                let mut w = json!({ "type": name });
                let point = |at: usize| json!([le_f32(d, at), le_f32(d, at + 4), le_f32(d, at + 8)]);
                let size = match kind {
                    0 | 3 | 5 => {
                        w["flag"] = json!(i16_at(d, at));
                        2
                    }
                    2 => {
                        w["ticks"] = json!(le_u32(d, at) as i32);
                        4
                    }
                    4 | 8 => {
                        w["point"] = point(at);
                        12
                    }
                    7 => {
                        w["mode"] = json!(sc::name(&sc::MOVEMENT_MODES, le_u32(d, at) as u16).unwrap_or("byAlertLevel"));
                        4
                    }
                    9 => {
                        w["direction"] = json!(sc::name(&sc::MOVE_DIRECTIONS, le_u32(d, at) as u16).unwrap_or("forward"));
                        4
                    }
                    10 => {
                        w["flag"] = json!(i16_at(d, at));
                        w["distance"] = json!(le_f32(d, at + 2));
                        6
                    }
                    11 => {
                        w["point"] = point(at);
                        w["distance"] = json!(le_f32(d, at + 12));
                        16
                    }
                    14 => {
                        w["flag"] = json!(i16_at(d, at));
                        w["ticks"] = json!(le_u32(d, at + 2));
                        6
                    }
                    15 if version >= 19 => {
                        w["flag"] = json!(i16_at(d, at));
                        w["distance"] = json!(le_f32(d, at + 2));
                        6
                    }
                    16 => {
                        w["waypoint"] = json!(le_u32(d, at));
                        4
                    }
                    17 => {
                        w["ticks"] = json!(i16_at(d, at));
                        w["angle"] = json!(le_f32(d, at + 2).to_radians());
                        6
                    }
                    19 => {
                        // Frames before the flag (the file order, not the struct's).
                        let ticks = i16_at(d, at);
                        if ticks != 0 {
                            w["ticks"] = json!(ticks);
                        }
                        w["flag"] = json!(i16_at(d, at + 2));
                        w["maxAngle"] = json!(le_f32(d, at + 4).min(180.0).to_radians());
                        8
                    }
                    20 | 21 => {
                        w["script"] = json!(format!("patrolscript{:04}", i16_at(d, at)));
                        2
                    }
                    22 => {
                        w["lock"] = json!(d.get(at).copied().unwrap_or(0) != 0);
                        1
                    }
                    23 => {
                        w["flag"] = json!(i16_at(d, at));
                        w["ticks"] = json!(i16_at(d, at + 2));
                        w["inaccuracy"] = json!(le_f32(d, at + 4));
                        8
                    }
                    _ => 0,
                };
                at += size;
                waypoints.push(w);
            }
            let mut p = json!({ "id": id, "name": cstr(d, 0, 32), "waypoints": waypoints });
            if flags & 1 != 0 {
                p["returnToNearest"] = json!(true);
            }
            Some(p)
        })
        .collect()
}

/// A level's flags: id, display name, position and facing.
fn flags(dat: &Dat<'_>, raw: &[u8]) -> Vec<Value> {
    crate::objects::read(dat, raw, "Flag")
        .1
        .iter()
        .filter(|o| o.data.len() >= 8)
        .map(|o| {
            let d = o.data;
            let (prefix, id) = (le_u16(d, 4), le_u16(d, 6));
            let (hi, lo) = ((prefix >> 8) as u8, (prefix & 0xFF) as u8);
            let name = match (hi, lo) {
                (0, 0) => format!("{id}"),
                (0, c) | (c, 0) => format!("{}-{id}", c as char),
                (a, b) => format!("{}{}-{id}", a as char, b as char),
            };
            // Facing: the Euler rotation's (static X, Y, Z) forward axis.
            let [a, b, c] = o.rotation.map(f32::to_radians);
            let zx = a.cos() * b.sin() * c.cos() + a.sin() * c.sin();
            let zz = a.cos() * b.cos();
            let facing = zx.atan2(zz).rem_euclid(std::f32::consts::TAU);
            let mut f = json!({ "id": id, "name": name, "position": o.position, "facing": facing });
            let note = if d.len() >= 136 { cstr(d, 8, 128) } else { String::new() };
            if !note.is_empty() {
                f["note"] = json!(note);
            }
            f
        })
        .collect()
}

/// Writes `patrols.json` and `flags.json` into a level folder.
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    let (p, f) = (patrols(dat, raw), flags(dat, raw));
    if p.is_empty() && f.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("patrols.json"), serde_json::to_string_pretty(&json!({ "patrols": p }))?)?;
    std::fs::write(root.join("flags.json"), serde_json::to_string_pretty(&json!({ "flags": f }))?)?;
    Ok(())
}

/// The combat profiles (level 0).
pub fn combat_profiles(dat: &Dat<'_>, raw: &[u8]) -> Vec<Value> {
    let (version, objects) = crate::objects::read(dat, raw, "Combat");
    objects
        .iter()
        .filter(|o| o.data.len() >= 88)
        .map(|o| {
            let d = o.data;
            let f = |at: usize, def: f32| if d.len() >= at + 4 { le_f32(d, at) } else { def };
            let u = |at: usize, def: u32| if d.len() >= at + 4 { le_u32(d, at) } else { def };
            let behavior = |k: usize| sc::name(&sc::COMBAT_BEHAVIORS, le_u32(d, 68 + k * 4) as u16).unwrap_or("none");
            let flags = if version >= 36 { le_u16(d, 66) } else { 0 };
            json!({
                "id": le_u16(d, 64),
                "name": cstr(d, 0, 64),
                "behavior": {
                    "long": behavior(0),
                    "medium": behavior(1),
                    "short": behavior(2),
                    "mediumRetreat": behavior(3),
                    "longRetreat": behavior(4),
                },
                "mediumRange": f(88, 200.0),
                "meleeWhen": sc::name(&sc::MELEE_WHEN, u(92, 3) as u16).unwrap_or("ifShortRange"),
                "noGun": sc::name(&sc::NO_GUN, u(96, 0) as u16).unwrap_or("melee"),
                "shortRange": f(100, 40.0),
                "pursuitDistance": f(104, 200.0),
                "panic": {
                    "melee": u(108, 600),
                    "gunfire": u(112, 900),
                    "sight": u(116, 600),
                    "hurt": u(120, 1200),
                },
                "alarm": {
                    "findDistance": f(124, 300.0),
                    "ignoreEnemyDistance": f(128, 80.0),
                    "chaseEnemyDistance": f(132, 0.0),
                    "damageThreshold": u(136, 30),
                    "fightTicks": u(140, 360),
                    "attackIfKnockedDown": flags & 1 != 0,
                },
            })
        })
        .collect()
}
