//! Turrets: the turret classes (level 0's `TURR`
//! templates) → `common/turrets.json`, and each level's placed turrets
//! (`OT_Turret` objects) → `levels/NN/turrets.json`.
//!
//! ## `OBJtTurretClass` (1600 bytes, offsets after the pre-data)
//!
//! | off | field |
//! |---|---|
//! | 0 | name `char[32]` |
//! | 78 | u16 attachment count; 80 shooter count |
//! | 100 / 108 | head and barrel geometry (`M3GM` refs; the base is baked into the level) |
//! | 116 / 128 | head pivot (base space), barrel pivot (head space) |
//! | 140 | 16 attachments × 76 bytes: particle class `char[16]`, u16 chamber time @20, u16 shooter index @22 (0xFFFF = follows all), 4×3 matrix @24 (barrel space) |
//! | 1356 | the AI's weapon parameters (as a gun's) |
//! | 1484 | targeting parameters; 1512 the shooting skill |
//! | 1536 | timeout (ticks an activation lasts; 0 = forever) |
//! | 1540 | pitch limits (min, max), yaw limits (min, max) — radians |
//! | 1556 | pitch and yaw speed (radians a tick) |
//!
//! A placed turret: the class name `char[63]`, then u16 script id @63 (not
//! unique), u16 flags @65 (0x2 initially active — never honoured), and the
//! u32 target-team mask @103 (bit n = team n).

use std::path::Path;

use oni_dat::{Dat, FourCc};
use oni_schema::files;

use crate::{cstr, le_f32, le_u16, le_u32, le_vec3};

const TAG_TURR: FourCc = FourCc(u32::from_be_bytes(*b"TURR"));
const TAG_M3GM: FourCc = FourCc(u32::from_be_bytes(*b"M3GM"));
const SIZE: usize = 1600;
const ATTACHMENTS: usize = 140;
const ATTACHMENT: usize = 76;
const NAME: usize = 63;

/// One turret class (`model` resolves a geometry reference).
pub fn class(d: &[u8], model: &dyn Fn(u32) -> Option<String>) -> Option<files::TurretClass> {
    if d.len() < SIZE {
        return None;
    }
    let n = (le_u16(d, 78) as usize).min(16);
    let attachments = (0..n)
        .map(|i| {
            let o = ATTACHMENTS + i * ATTACHMENT;
            let shooter = le_u16(d, o + 22);
            files::TurretAttachment {
                class: cstr(d, o, 16),
                shooter: if shooter == 0xFFFF {
                    -1
                } else {
                    i32::from(shooter)
                },
                // (Only shooters' chamber times mean anything.)
                chamber: (shooter != 0xFFFF).then(|| le_u16(d, o + 20)),
                matrix: std::array::from_fn(|k| le_f32(d, o + 24 + k * 4)),
            }
        })
        .collect();
    let t = 1484;
    let s = 1512;
    Some(files::TurretClass {
        name: cstr(d, 0, 32),
        head: model(le_u32(d, 100)),
        barrel: model(le_u32(d, 108)),
        head_pivot: le_vec3(d, 116),
        barrel_pivot: le_vec3(d, 128),
        timeout: le_u32(d, 1536),
        pitch: files::Limits {
            min: le_f32(d, 1540),
            max: le_f32(d, 1544),
            speed: le_f32(d, 1556),
        },
        yaw: files::Limits {
            min: le_f32(d, 1548),
            max: le_f32(d, 1552),
            speed: le_f32(d, 1560),
        },
        active_sound: cstr(d, 1564, 32),
        ai: crate::weapons::ai_params(d, 1356),
        targeting: files::Targeting {
            startle_miss_ratio: le_f32(d, t),
            startle_miss_distance: le_f32(d, t + 4),
            predict_amount: le_f32(d, t + 8),
            predict_position_delay: le_u32(d, t + 12),
            predict_delay: le_u32(d, t + 16),
            predict_velocity: le_u32(d, t + 20),
            predict_trend: le_u32(d, t + 24),
        },
        skill: files::ShootSkill {
            recoil_compensation: le_f32(d, s),
            best_aiming_sine: le_f32(d, s + 4),
            shot_error_percent: le_f32(d, s + 8),
            shot_decay: le_f32(d, s + 12),
            gun_inaccuracy: le_f32(d, s + 16),
            delay_min: le_u16(d, s + 20),
            delay_max: le_u16(d, s + 22),
        },
        attachments,
    })
}

/// Writes `turrets.json` (every `TURR` in `dat`, level 0) into `common`.
pub fn write_classes(dat: &Dat<'_>, common: &Path) -> std::io::Result<usize> {
    let model = |r: u32| {
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .filter(|g| g.tag == TAG_M3GM)
            .map(|g| format!("models/{}.glb", crate::asset_name(dat, g)))
    };
    let mut file = files::TurretClassesFile::default();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_TURR) {
        let Some(c) = dat.instance_data(inst).and_then(|d| class(d, &model)) else {
            continue;
        };
        file.classes.insert(c.name.clone(), c);
    }
    let count = file.classes.len();
    if count > 0 {
        std::fs::create_dir_all(common)?;
        std::fs::write(
            common.join("turrets.json"),
            serde_json::to_string_pretty(&file)?,
        )?;
    }
    Ok(count)
}

/// A level's placed turrets.
pub fn turrets(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::Turret> {
    crate::objects::read(dat, raw, "Turret")
        .1
        .iter()
        .filter(|o| o.data.len() >= 107)
        .map(|o| {
            let d = o.data;
            files::Turret {
                object_id: o.id,
                id: le_u16(d, NAME),
                class: cstr(d, 0, NAME),
                position: o.position,
                axes: crate::patrols::euler_axes(o.rotation),
                target_teams: le_u32(d, 103),
                flags: if le_u16(d, NAME + 2) & 0x2 != 0 {
                    vec!["initialActive".into()]
                } else {
                    Vec::new()
                },
            }
        })
        .collect()
}

/// Writes `turrets.json` into a level folder (only if it places any).
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    let turrets = turrets(dat, raw);
    if turrets.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("turrets.json"),
        serde_json::to_string_pretty(&files::TurretsFile { turrets })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_hand_built_class() {
        let mut d = vec![0u8; SIZE];
        d[..10].copy_from_slice(b"x_turret_w");
        d[78..80].copy_from_slice(&2u16.to_le_bytes());
        let a0 = ATTACHMENTS;
        d[a0..a0 + 4].copy_from_slice(b"w0_e");
        d[a0 + 20..a0 + 22].copy_from_slice(&5u16.to_le_bytes());
        let a1 = ATTACHMENTS + ATTACHMENT;
        d[a1 + 22..a1 + 24].copy_from_slice(&0xFFFFu16.to_le_bytes());
        d[a1 + 20..a1 + 22].copy_from_slice(&0xDEADu16.to_le_bytes());
        d[1536..1540].copy_from_slice(&900u32.to_le_bytes());
        let c = class(&d, &|_| None).unwrap();
        assert_eq!(c.name, "x_turret_w");
        assert_eq!(c.timeout, 900);
        assert_eq!(c.attachments[0].chamber, Some(5));
        assert_eq!(c.attachments[0].shooter, 0);
        assert_eq!(c.attachments[1].shooter, -1);
        assert_eq!(
            c.attachments[1].chamber, None,
            "junk chamber of a follower dropped"
        );
    }
}
