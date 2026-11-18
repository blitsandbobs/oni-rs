//! AI melee profiles → `common/characters/melee_profiles.json`
//! (`docs/MELEE_AI.md`, `docs/ASSET_FORMAT.md`).
//!
//! Profiles are objects of the "Melee Profile" group, stored once (in level
//! 0) as a binary-data blob: an object file of size-prefixed chunks, one per
//! profile. Each profile holds its tuning and its techniques; a technique is a
//! slice of the profile's move pool, and a move names an entry of one of the
//! original's move tables (named in `oni-schema`).

use oni_dat::Dat;
use oni_schema as sc;
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

const TECHNIQUE_SIZE: usize = 88;
const MOVE_SIZE: usize = 16;
/// Position moves that can delay (run in) and so take parameters: the run,
/// close and run-jump moves.
const DELAYING_POSITIONS: [usize; 12] = [0, 1, 2, 3, 12, 13, 14, 15, 16, 17, 18, 19];

/// A decoded profile and the id classes refer to it by.
pub struct Profile {
    pub id: u32,
    pub name: String,
    pub json: Value,
}

/// The melee profiles stored in this file (empty unless it's level 0).
pub fn collect(dat: &Dat<'_>, raw: Option<&[u8]>) -> Vec<Profile> {
    let Some(raw) = raw else { return Vec::new() };
    crate::objects::read(dat, raw, "Melee Profile").1.into_iter().filter_map(|o| profile(o.data)).collect()
}

/// One profile from its object chunk.
fn profile(d: &[u8]) -> Option<Profile> {
    if d.len() < 204 {
        return None;
    }
    let u = |o: usize| le_u32(d, o);
    let f = |o: usize| le_f32(d, o);
    let (actions, reactions, spacing, moves) = (u(188) as usize, u(192) as usize, u(196) as usize, u(200) as usize);
    let techniques = actions + reactions + spacing;
    let moves_at = 204 + techniques * TECHNIQUE_SIZE;
    if d.len() < moves_at + moves * MOVE_SIZE {
        return None;
    }
    let pool: Vec<(u32, [f32; 3])> = (0..moves)
        .map(|k| {
            let o = moves_at + k * MOVE_SIZE;
            (u(o), [f(o + 4), f(o + 8), f(o + 12)])
        })
        .collect();
    let technique = |k: usize, spacing: bool| -> Value {
        let o = 204 + k * TECHNIQUE_SIZE;
        let (n, first) = (u(o + 80) as usize, u(o + 84) as usize);
        let moves: Vec<Value> = pool
            .get(first..first + n)
            .unwrap_or(&[])
            .iter()
            .map(|&(m, p)| move_json(m, p, spacing))
            .collect();
        let mut t = json!({
            "name": cstr(d, o, 64),
            "weight": u(o + 68),
            "importance": u(o + 72),
            "repeatDelay": u(o + 76),
            "moves": moves,
        });
        let flags = sc::bit_names(&sc::TECHNIQUE_FLAGS, u(o + 64));
        if !flags.is_empty() {
            t["flags"] = json!(flags);
        }
        t
    };
    let id = u(0);
    let name = cstr(d, 4, 64);
    let json = json!({
        "id": id,
        "name": name,
        "class": cstr(d, 68, 64),
        "attackNoticePercent": u(132),
        "dodge": {
            "basePercent": u(136),
            "additionalPercent": u(140),
            "damageThreshold": u(144),
        },
        "block": { "skillPercent": u(148), "groupPercent": u(152) },
        "weights": {
            "targetNotBlocking": f(156),
            "targetWrongStance": f(160),
            "blockedButUnblockable": f(164),
            "blockedButStaggers": f(168),
            "blockedButBlockStun": f(172),
            "blocked": f(176),
            "throwIntoDanger": f(180),
        },
        "dazed": { "min": le_u16(d, 184), "max": le_u16(d, 186) },
        "actions": (0..actions).map(|k| technique(k, false)).collect::<Vec<_>>(),
        "reactions": (actions..actions + reactions).map(|k| technique(k, false)).collect::<Vec<_>>(),
        "spacing": (actions + reactions..techniques).map(|k| technique(k, true)).collect::<Vec<_>>(),
    });
    Some(Profile { id, name, json })
}

/// A move as `{type, move, …named parameters}`. Times become ticks, angles
/// radians.
fn move_json(code: u32, p: [f32; 3], spacing: bool) -> Value {
    let index = (code & 0x0FFF_FFFF) as usize;
    let name = |table: &[&'static str]| table.get(index).copied().unwrap_or("unknown");
    let ticks = |s: f32| (s * 60.0).round() as u32;
    match code >> 28 {
        0 => json!({ "type": "attack", "move": name(&sc::MELEE_ATTACKS) }),
        1 => {
            let mut m = json!({ "type": "position", "move": name(&sc::MELEE_POSITIONS) });
            if DELAYING_POSITIONS.contains(&index) {
                m["minRunIn"] = json!(p[0]);
                m["maxRunIn"] = json!(p[1]);
                m["tolerance"] = json!(p[2]);
            }
            m
        }
        2 => {
            let mut m = json!({ "type": "maneuver", "move": name(&sc::MELEE_MANEUVERS) });
            match (index, spacing) {
                (8, _) => m["chancePercent"] = json!(p[0]), // random stop
                (13, _) => m["maxRange"] = json!(p[0]),     // Barabas' wave
                (0, false) => {
                    m["duration"] = json!(ticks(p[0]));
                    m["minRange"] = json!(p[1]);
                    m["threshold"] = json!(p[2]);
                }
                (1, false) => {
                    m["duration"] = json!(ticks(p[0]));
                    m["maxRange"] = json!(p[1]);
                    m["threshold"] = json!(p[2]);
                }
                (0 | 1, true) => {
                    m["duration"] = json!(ticks(p[0]));
                    m["targetRange"] = json!(p[1]);
                    m["tolerance"] = json!(p[2]);
                }
                (2 | 3, false) => {
                    m["duration"] = json!(ticks(p[0]));
                    m["minAngle"] = json!(p[1].to_radians());
                    m["maxAngle"] = json!(p[2].to_radians());
                }
                (2 | 3, true) => {
                    m["duration"] = json!(ticks(p[0]));
                    m["closenessAngle"] = json!(p[1].to_radians());
                }
                _ => m["duration"] = json!(ticks(p[0])),
            }
            m
        }
        3 => json!({ "type": "evade", "move": name(&sc::MELEE_EVADES) }),
        4 => json!({ "type": "throw", "move": name(&sc::MELEE_THROWS) }),
        _ => json!({ "type": "unknown", "code": code }),
    }
}
