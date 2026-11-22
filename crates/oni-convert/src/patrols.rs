//! Patrol paths, flags and combat profiles (`docs/GOALS.md`,
//! `docs/ASSET_FORMAT.md`): the waypoint programs patrolling characters
//! follow, the marker flags they (and scripts) refer to, and the combat
//! profiles placed characters fight by.

use std::path::Path;

use oni_dat::Dat;
use oni_schema as sc;
use oni_schema::files;

use crate::{cstr, le_f32, le_u16, le_u32};

fn i16_at(d: &[u8], o: usize) -> i16 {
    le_u16(d, o) as i16
}

/// A level's patrol paths.
pub fn patrols(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::Patrol> {
    use files::Waypoint as W;
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
                let point = |at: usize| [le_f32(d, at), le_f32(d, at + 4), le_f32(d, at + 8)];
                let flag = |at: usize| i16_at(d, at);
                let (w, size) = match kind {
                    0 => (W::MoveToFlag { flag: flag(at) }, 2),
                    1 => (W::Stop, 0),
                    2 => (
                        W::Pause {
                            ticks: le_u32(d, at) as i32,
                        },
                        4,
                    ),
                    3 => (W::LookAtFlag { flag: flag(at) }, 2),
                    4 => (W::LookAtPoint { point: point(at) }, 12),
                    5 => (W::MoveAndFaceFlag { flag: flag(at) }, 2),
                    6 => (W::Loop, 0),
                    7 => (
                        W::MovementMode {
                            mode: sc::name(&sc::MOVEMENT_MODES, le_u32(d, at) as u16)
                                .unwrap_or("byAlertLevel")
                                .to_string(),
                        },
                        4,
                    ),
                    8 => (W::MoveToPoint { point: point(at) }, 12),
                    9 => (
                        W::LookInDirection {
                            direction: sc::name(&sc::MOVE_DIRECTIONS, le_u32(d, at) as u16)
                                .unwrap_or("forward")
                                .to_string(),
                        },
                        4,
                    ),
                    10 => (
                        W::MoveThroughFlag {
                            flag: flag(at),
                            distance: le_f32(d, at + 2),
                        },
                        6,
                    ),
                    11 => (
                        W::MoveThroughPoint {
                            point: point(at),
                            distance: le_f32(d, at + 12),
                        },
                        16,
                    ),
                    12 => (W::StopLooking, 0),
                    13 => (W::FreeFacing, 0),
                    14 => (
                        W::GlanceAtFlag {
                            flag: flag(at),
                            ticks: le_u32(d, at + 2),
                        },
                        6,
                    ),
                    15 if version >= 19 => (
                        W::MoveNearFlag {
                            flag: Some(flag(at)),
                            distance: Some(le_f32(d, at + 2)),
                        },
                        6,
                    ),
                    15 => (
                        W::MoveNearFlag {
                            flag: None,
                            distance: None,
                        },
                        0,
                    ),
                    16 => (
                        W::LoopToWaypoint {
                            waypoint: le_u32(d, at),
                        },
                        4,
                    ),
                    17 => (
                        W::Scan {
                            ticks: i16_at(d, at),
                            angle: le_f32(d, at + 2).to_radians(),
                        },
                        6,
                    ),
                    18 => (W::StopScanning, 0),
                    // Frames before the flag (the file order, not the struct's).
                    19 => (
                        W::Guard {
                            ticks: Some(i16_at(d, at)).filter(|&t| t != 0),
                            flag: i16_at(d, at + 2),
                            max_angle: le_f32(d, at + 4).min(180.0).to_radians(),
                        },
                        8,
                    ),
                    20 | 21 => {
                        let script = format!("patrolscript{:04}", i16_at(d, at));
                        let w = if kind == 20 {
                            W::TriggerScript { script }
                        } else {
                            W::PlayScript { script }
                        };
                        (w, 2)
                    }
                    22 => (
                        W::LockIntoPatrol {
                            lock: d.get(at).copied().unwrap_or(0) != 0,
                        },
                        1,
                    ),
                    23 => (
                        W::ShootAtFlag {
                            flag: flag(at),
                            ticks: i16_at(d, at + 2),
                            inaccuracy: le_f32(d, at + 4),
                        },
                        8,
                    ),
                    _ => (W::Unknown, 0),
                };
                at += size;
                waypoints.push(w);
            }
            Some(files::Patrol {
                id,
                name: cstr(d, 0, 32),
                waypoints,
                return_to_nearest: flags & 1 != 0,
            })
        })
        .collect()
}

/// A level's flags: id, display name, position and facing.
fn flags(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::Flag> {
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
            files::Flag {
                id,
                name,
                position: o.position,
                facing: zx.atan2(zz).rem_euclid(std::f32::consts::TAU),
                note: if d.len() >= 136 {
                    cstr(d, 8, 128)
                } else {
                    String::new()
                },
            }
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
    std::fs::write(
        root.join("patrols.json"),
        serde_json::to_string_pretty(&files::PatrolsFile { patrols: p })?,
    )?;
    std::fs::write(
        root.join("flags.json"),
        serde_json::to_string_pretty(&files::FlagsFile { flags: f })?,
    )?;
    let t = triggers(dat, raw);
    if !t.is_empty() {
        std::fs::write(
            root.join("triggers.json"),
            serde_json::to_string_pretty(&files::TriggersFile { triggers: t })?,
        )?;
    }
    Ok(())
}

/// The combat profiles (level 0).
pub fn combat_profiles(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::CombatProfile> {
    let (version, objects) = crate::objects::read(dat, raw, "Combat");
    objects
        .iter()
        .filter(|o| o.data.len() >= 88)
        .map(|o| {
            let d = o.data;
            let f = |at: usize, def: f32| {
                if d.len() >= at + 4 {
                    le_f32(d, at)
                } else {
                    def
                }
            };
            let u = |at: usize, def: u32| {
                if d.len() >= at + 4 {
                    le_u32(d, at)
                } else {
                    def
                }
            };
            let behavior = |k: usize| {
                sc::name(&sc::COMBAT_BEHAVIORS, le_u32(d, 68 + k * 4) as u16).unwrap_or("none")
            };
            let flags = if version >= 36 { le_u16(d, 66) } else { 0 };
            let name_of = |table: &[&'static str], v: u32, or: &str| {
                sc::name(table, v as u16).unwrap_or(or).to_string()
            };
            files::CombatProfile {
                id: le_u16(d, 64),
                name: cstr(d, 0, 64),
                behavior: files::CombatBehaviors {
                    long: behavior(0).to_string(),
                    medium: behavior(1).to_string(),
                    short: behavior(2).to_string(),
                    medium_retreat: behavior(3).to_string(),
                    long_retreat: behavior(4).to_string(),
                },
                medium_range: f(88, 200.0),
                melee_when: name_of(&sc::MELEE_WHEN, u(92, 3), "ifShortRange"),
                no_gun: name_of(&sc::NO_GUN, u(96, 0), "melee"),
                short_range: f(100, 40.0),
                pursuit_distance: f(104, 200.0),
                panic: files::PanicTicks {
                    melee: u(108, 600),
                    gunfire: u(112, 900),
                    sight: u(116, 600),
                    hurt: u(120, 1200),
                },
                alarm: files::AlarmSettings {
                    find_distance: f(124, 300.0),
                    ignore_enemy_distance: f(128, 80.0),
                    chase_enemy_distance: f(132, 0.0),
                    damage_threshold: u(136, 30),
                    fight_ticks: u(140, 360),
                    attack_if_knocked_down: flags & 1 != 0,
                },
            }
        })
        .collect()
}

/// The rotation matrix (columns) of an object's Euler angles (degrees,
/// static X, Y, Z order): `R = Rz · Ry · Rx`.
pub fn euler_axes(rotation: [f32; 3]) -> [[f32; 3]; 3] {
    let [a, b, c] = rotation.map(f32::to_radians);
    let (sa, ca, sb, cb, sc, cc) = (a.sin(), a.cos(), b.sin(), b.cos(), c.sin(), c.cos());
    [
        [cb * cc, cb * sc, -sb],
        [sa * sb * cc - ca * sc, sa * sb * sc + ca * cc, sa * cb],
        [ca * sb * cc + sa * sc, ca * sb * sc - sa * cc, ca * cb],
    ]
}

/// A level's trigger volumes (`OT_TriggerVolume`): boxes from their
/// position (a corner) to `size` along their rotated axes.
pub fn triggers(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::TriggerVolume> {
    let (version, objects) = crate::objects::read(dat, raw, "Trigger Volume");
    objects
        .iter()
        .filter(|o| o.data.len() >= 63 + 96 + 4 + 12 + 4)
        .map(|o| {
            let d = o.data;
            let name = cstr(d, 0, 63);
            let script = |at: usize| cstr(d, at, 32);
            let mut at = 63 + 96;
            let teams = le_u32(d, at);
            at += 4;
            let size = [le_f32(d, at), le_f32(d, at + 4), le_f32(d, at + 8)];
            at += 12;
            let id = le_u32(d, at) as i32;
            at += 4;
            let parent = if version >= 22 {
                at += 4;
                le_u32(d, at - 4) as i32
            } else {
                0
            };
            let note = cstr(d, at, 128);
            let flags = le_u32(d, at + 128);
            let script = |at: usize| Some(script(at)).filter(|s| !s.is_empty());
            files::TriggerVolume {
                id,
                name,
                position: o.position,
                axes: euler_axes(o.rotation),
                size,
                teams: sc::bit_names(&sc::TEAMS, teams & 0xFF)
                    .into_iter()
                    .map(String::from)
                    .collect(),
                entry: script(63),
                inside: script(95),
                exit: script(127),
                parent: (parent != 0).then_some(parent),
                flags: sc::bit_names(&sc::TRIGGER_FLAGS, flags)
                    .into_iter()
                    .map(String::from)
                    .collect(),
                note,
            }
        })
        .collect()
}
