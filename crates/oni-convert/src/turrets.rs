//! Turrets: the turret classes (level 0's `TURR`
//! templates) → `common/turrets.json`, and each level's placed turrets
//! (`OT_Turret` objects) → `levels/NN/turrets.json`.
//!
//! A class is decoded by its template layout (`OBJtTurretClass`; `cargo
//! run -p oni-template --example layout -- TURR` prints it): its head and
//! barrel geometry (the base is baked into the level), their pivots (base
//! space, head space), attachments in barrel space (shooter index 0xFFFF
//! follows every shooter), a timeout in ticks (0 = forever), angle limits
//! in radians and speeds in radians a tick.
//!
//! A placed turret: the class name `char[63]`, then u16 script id @63 (not
//! unique), u16 flags @65 (0x2 initially active — never honoured), and the
//! u32 target-team mask @103 (bit n = team n).

use std::path::Path;

use oni_dat::{Dat, FourCc};
use oni_schema::files;

use crate::fields::Fields;
use crate::{cstr, le_u16, le_u32};

const TAG_TURR: FourCc = FourCc(u32::from_be_bytes(*b"TURR"));
const TAG_M3GM: FourCc = FourCc(u32::from_be_bytes(*b"M3GM"));
// (A hand-built class for the test: its size and attachments' place.)
#[cfg(test)]
const SIZE: usize = 1600;
#[cfg(test)]
const ATTACHMENTS: usize = 140;
#[cfg(test)]
const ATTACHMENT: usize = 76;
const NAME: usize = 63;

/// One turret class (`model` resolves a geometry reference).
pub fn class(f: Fields, model: &dyn Fn(u32) -> Option<String>) -> files::TurretClass {
    let n = (f.u16("attachment_count") as usize).min(16);
    let attachments = f
        .list("attachment")
        .into_iter()
        .take(n)
        .map(|a| {
            let shooter = a.u16("shooter_index");
            files::TurretAttachment {
                class: a.text("particle_class_name"),
                shooter: if shooter == 0xFFFF {
                    -1
                } else {
                    i32::from(shooter)
                },
                // (Only shooters' chamber times mean anything.)
                chamber: (shooter != 0xFFFF).then(|| a.u16("chamber_time")),
                matrix: crate::weapons::matrix(a, "matrix.m"),
            }
        })
        .collect();
    let t = |n: &str| format!("targeting_params.{n}");
    let s = |n: &str| format!("shooting_skill.{n}");
    files::TurretClass {
        name: f.text("name"),
        head: model(f.u32("turret_geometry")),
        barrel: model(f.u32("barrel_geometry")),
        head_pivot: f.vec3("turret_position"),
        barrel_pivot: f.vec3("barrel_position"),
        timeout: f.u32("timeout"),
        pitch: files::Limits {
            min: f.f32("min_vert_angle"),
            max: f.f32("max_vert_angle"),
            speed: f.f32("max_vert_speed"),
        },
        yaw: files::Limits {
            min: f.f32("min_horiz_angle"),
            max: f.f32("max_horiz_angle"),
            speed: f.f32("max_horiz_speed"),
        },
        active_sound: f.text("active_soundname"),
        ai: crate::weapons::ai_params(f, "ai_params"),
        targeting: files::Targeting {
            startle_miss_ratio: f.f32(&t("startle_miss_angle")),
            startle_miss_distance: f.f32(&t("startle_miss_distance")),
            predict_amount: f.f32(&t("predict_amount")),
            predict_position_delay: f.u32(&t("predict_positiondelayframes")),
            predict_delay: f.u32(&t("predict_delayframes")),
            predict_velocity: f.u32(&t("predict_velocityframes")),
            predict_trend: f.u32(&t("predict_trendframes")),
        },
        skill: files::ShootSkill {
            recoil_compensation: f.f32(&s("recoil_compensation")),
            best_aiming_sine: f.f32(&s("best_aiming_angle")),
            shot_error_percent: f.f32(&s("shot_error")),
            shot_decay: f.f32(&s("shot_decay")),
            gun_inaccuracy: f.f32(&s("gun_inaccuracy")),
            delay_min: f.u16(&s("delay_min")),
            delay_max: f.u16(&s("delay_max")),
        },
        attachments,
    }
}

/// Writes `turrets.json` (every `TURR` in `dat`, level 0) into `common`.
pub fn write_classes(
    dat: &Dat<'_>,
    registry: &oni_template::Registry,
    common: &Path,
) -> std::io::Result<usize> {
    let model = |r: u32| {
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .filter(|g| g.tag == TAG_M3GM)
            .map(|g| format!("models/{}.glb", crate::asset_name(dat, g)))
    };
    let mut file = files::TurretClassesFile::default();
    for (_, rec) in crate::fields::all(registry, dat, TAG_TURR) {
        let c = class(Fields(&rec), &model);
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
        let registry = oni_template::Registry::load();
        let rec = oni_template::decode(
            registry.get(TAG_TURR).unwrap(),
            &d,
            oni_dat::ByteOrder::Little,
        )
        .unwrap();
        let c = class(Fields(&rec), &|_| None);
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
