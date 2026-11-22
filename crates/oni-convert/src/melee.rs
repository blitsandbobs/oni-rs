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
use oni_schema::files;

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
    pub file: files::MeleeProfile,
}

/// The melee profiles stored in this file (empty unless it's level 0).
pub fn collect(dat: &Dat<'_>, raw: Option<&[u8]>) -> Vec<Profile> {
    let Some(raw) = raw else { return Vec::new() };
    crate::objects::read(dat, raw, "Melee Profile")
        .1
        .into_iter()
        .filter_map(|o| profile(o.data))
        .collect()
}

/// One profile from its object chunk.
fn profile(d: &[u8]) -> Option<Profile> {
    if d.len() < 204 {
        return None;
    }
    let u = |o: usize| le_u32(d, o);
    let f = |o: usize| le_f32(d, o);
    let (actions, reactions, spacing, moves) = (
        u(188) as usize,
        u(192) as usize,
        u(196) as usize,
        u(200) as usize,
    );
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
    let technique = |k: usize, spacing: bool| -> files::Technique {
        let o = 204 + k * TECHNIQUE_SIZE;
        let (n, first) = (u(o + 80) as usize, u(o + 84) as usize);
        let moves: Vec<files::MeleeMove> = pool
            .get(first..first + n)
            .unwrap_or(&[])
            .iter()
            .map(|&(m, p)| move_json(m, p, spacing))
            .collect();
        files::Technique {
            name: cstr(d, o, 64),
            weight: u(o + 68),
            importance: u(o + 72),
            repeat_delay: u(o + 76),
            moves,
            flags: sc::bit_names(&sc::TECHNIQUE_FLAGS, u(o + 64))
                .into_iter()
                .map(String::from)
                .collect(),
        }
    };
    let id = u(0);
    let name = cstr(d, 4, 64);
    let file = files::MeleeProfile {
        id,
        name: name.clone(),
        class: cstr(d, 68, 64),
        attack_notice_percent: u(132),
        dodge: files::MeleeDodge {
            base_percent: u(136),
            additional_percent: u(140),
            damage_threshold: u(144),
        },
        block: files::MeleeBlock {
            skill_percent: u(148),
            group_percent: u(152),
        },
        weights: files::MeleeWeights {
            target_not_blocking: f(156),
            target_wrong_stance: f(160),
            blocked_but_unblockable: f(164),
            blocked_but_staggers: f(168),
            blocked_but_block_stun: f(172),
            blocked: f(176),
            throw_into_danger: f(180),
        },
        dazed: files::MeleeDazed {
            min: le_u16(d, 184),
            max: le_u16(d, 186),
        },
        actions: (0..actions).map(|k| technique(k, false)).collect(),
        reactions: (actions..actions + reactions)
            .map(|k| technique(k, false))
            .collect(),
        spacing: (actions + reactions..techniques)
            .map(|k| technique(k, true))
            .collect(),
    };
    Some(Profile { id, name, file })
}

/// A move as `{type, move, …named parameters}`. Times become ticks, angles
/// radians.
fn move_json(code: u32, p: [f32; 3], spacing: bool) -> files::MeleeMove {
    let index = (code & 0x0FFF_FFFF) as usize;
    let name =
        |table: &[&'static str]| Some(table.get(index).copied().unwrap_or("unknown").to_string());
    let ticks = |s: f32| Some((s * 60.0).round() as u32);
    let of = |kind: &str, name| files::MeleeMove {
        kind: kind.to_string(),
        name,
        ..Default::default()
    };
    match code >> 28 {
        0 => of("attack", name(&sc::MELEE_ATTACKS)),
        1 => {
            let mut m = of("position", name(&sc::MELEE_POSITIONS));
            if DELAYING_POSITIONS.contains(&index) {
                m.min_run_in = Some(p[0]);
                m.max_run_in = Some(p[1]);
                m.tolerance = Some(p[2]);
            }
            m
        }
        2 => {
            let mut m = of("maneuver", name(&sc::MELEE_MANEUVERS));
            match (index, spacing) {
                (8, _) => m.chance_percent = Some(p[0]), // random stop
                (13, _) => m.max_range = Some(p[0]),     // Barabas' wave
                (0, false) => {
                    m.duration = ticks(p[0]);
                    m.min_range = Some(p[1]);
                    m.threshold = Some(p[2]);
                }
                (1, false) => {
                    m.duration = ticks(p[0]);
                    m.max_range = Some(p[1]);
                    m.threshold = Some(p[2]);
                }
                (0 | 1, true) => {
                    m.duration = ticks(p[0]);
                    m.target_range = Some(p[1]);
                    m.tolerance = Some(p[2]);
                }
                (2 | 3, false) => {
                    m.duration = ticks(p[0]);
                    m.min_angle = Some(p[1].to_radians());
                    m.max_angle = Some(p[2].to_radians());
                }
                (2 | 3, true) => {
                    m.duration = ticks(p[0]);
                    m.closeness_angle = Some(p[1].to_radians());
                }
                _ => m.duration = ticks(p[0]),
            }
            m
        }
        3 => of("evade", name(&sc::MELEE_EVADES)),
        4 => of("throw", name(&sc::MELEE_THROWS)),
        _ => files::MeleeMove {
            kind: "unknown".into(),
            code: Some(code),
            ..Default::default()
        },
    }
}
