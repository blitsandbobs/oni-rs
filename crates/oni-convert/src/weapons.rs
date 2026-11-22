//! Weapons: the weapon classes (level 0's `ONWC`
//! templates) → `common/weapons.json`, and each level's placed weapons and
//! powerups (`OT_Weapon` / `OT_PowerUps` objects) → `levels/NN/items.json`.
//!
//! A class is decoded by its template layout (`WPtWeaponClass`; `cargo run
//! -p oni-template --example layout -- ONWC` prints it). Of note: the recoil
//! base is already in internal units; an attachment with ammo used is a
//! *shooter*, and shooter index 0xFFFF follows every shooter; its matrix is
//! gun space (columns X, Y, Z, translation).

use std::path::Path;

use oni_dat::{Dat, FourCc};
use std::collections::BTreeMap;

use oni_schema::files;
#[cfg(test)]
use serde_json::json;

use crate::cstr;
use crate::fields::Fields;

const TAG_ONWC: FourCc = FourCc(u32::from_be_bytes(*b"ONWC"));
const TAG_M3GM: FourCc = FourCc(u32::from_be_bytes(*b"M3GM"));
#[cfg(test)]
const SIZE: usize = 1812;

/// Class flag bits (`WPcWeaponClassFlag_*`), bit index → name.
const FLAGS: [&str; 21] = [
    "big",
    "unstorable",
    "energy",
    "twoHanded",
    "recoilDirect",
    "autofire",
    "",
    "aiStun",
    "aiKnockdown",
    "aiSingleShot",
    "hasAlternate",
    "unreloadable",
    "walkOnly",
    "restartEachShot",
    "dontStopWhileFiring",
    "hasLaser",
    "noScale",
    "halfScale",
    "dontClipSight",
    "dontFadeOut",
    "useAmmoContinuously",
];

/// Placed powerup type codes (the object's 4 bytes) → names, in
/// `WPtPowerupType` order.
const POWERUPS: [(&[u8; 4], &str); 6] = [
    (b"AMMB", "ammoBallistic"),
    (b"AMME", "ammoEnergy"),
    (b"HYPO", "hypo"),
    (b"SHLD", "shieldBelt"),
    (b"INVI", "invisibility"),
    (b"ALSI", "lsi"),
];

fn anim_type(t: u16) -> oni_schema::files::AnimRef {
    oni_schema::ANIM_TYPES
        .get(t as usize)
        .map_or(oni_schema::files::AnimRef::Number(t), |s| {
            oni_schema::files::AnimRef::Name(s.to_string())
        })
}

/// The AI's aiming and shooting parameters for one shooter (the
/// `WPtAIParameters` at `p`: `ai_parameters`, …).
pub(crate) fn ai_params(f: Fields, p: &str) -> oni_schema::files::AiGunParams {
    let k = |n: &str| format!("{p}.{n}");
    oni_schema::files::AiGunParams {
        no_wild_shots: f.u32(&k("targeting_flags")) & 1 != 0,
        prediction_speed: f.f32(&k("prediction_speed")),
        burst_radius: f.f32(&k("burst_radius")),
        aim_radius: f.f32(&k("aim_radius")),
        detection_volume: f.f32(&k("detection_volume")),
        min_safe_distance: f.f32(&k("min_safe_distance")),
        max_range: f.f32(&k("max_range")),
        max_startle_misses: f.u16(&k("max_startle_misses")),
        shoot_skill_index: f.u16(&k("shootskill_index")),
        melee_assist_timer: f.u32(&k("melee_assist_timer")),
        ballistic_speed: f.f32(&k("ballistic_speed")),
        ballistic_gravity: f.f32(&k("ballistic_gravity")),
        dodge_pattern: [
            f.f32(&k("dodge_pattern_length")),
            f.f32(&k("dodge_pattern_base")),
            f.f32(&k("dodge_pattern_angle")),
        ],
        // Where the shot leaves the gun (gun space): its direction, its
        // offset off that line, and the turn from it onto the barrel.
        shooter_direction: f.vec3(&k("shooterdir_gunspace")),
        shooter_offset: f.vec3(&k("shooter_perpoffset_gunspace")),
        shooter_inverse: matrix(f, &k("shooter_inversematrix.m")),
    }
}

/// A 4×3 matrix field (`M3tMatrix4x3`'s `m`).
pub(crate) fn matrix(f: Fields, name: &str) -> [f32; 12] {
    let m = f.floats(name);
    std::array::from_fn(|k| m[k])
}

/// A template reference's instance, if it is one.
fn instance<'d>(dat: &'d Dat<'_>, r: u32) -> Option<&'d oni_dat::Instance> {
    (r & 1 != 0)
        .then(|| dat.instances.get((r >> 8) as usize))
        .flatten()
}

/// One weapon class as JSON. `model` is its geometry's path under `common/`.
pub fn class(f: Fields, model: Option<String>) -> files::WeaponClass {
    let flags = f.u32("flags");
    let count = (f.u16("attachment_count") as usize).min(16);
    let attachments = f
        .list("attachment")
        .into_iter()
        .take(count)
        .map(|a| {
            let shooter = a.u16("shooter_index");
            files::WeaponAttachment {
                class: a.text("classname"),
                matrix: matrix(a, "matrix.m"),
                ammo_used: a.u16("ammo_used"),
                chamber_time: a.u16("chamber_time"),
                cheat_chamber_time: a.u16("cheat_chamber_time"),
                min_fire_time: a.u16("minfire_time"),
                shooter_index: if shooter == 0xFFFF {
                    -1
                } else {
                    i32::from(shooter)
                },
                start_delay: a.u16("start_delay"),
            }
        })
        .collect();
    let empty = f.text("empty_soundname");
    files::WeaponClass {
        name: f.text("name"),
        model,
        flags: FLAGS
            .iter()
            .enumerate()
            .filter(|(b, n)| !n.is_empty() && flags & (1 << b) != 0)
            .map(|(_, n)| n.to_string())
            .collect(),
        max_ammo: f.u16("max_ammo"),
        shooter_count: f.u16("shooter_count"),
        reload_time: f.u16("reloadTime"),
        reload_delay_time: f.u16("reload_delay_time"),
        stop_chamber_threshold: f.u16("stopChamberThreshold"),
        recoil_anim_type: anim_type(f.u16("recoilAnimType")),
        reload_anim_type: anim_type(f.u16("reloadAnimType")),
        aiming_speed: f.f32("aimingSpeed"),
        recoil: files::WeaponRecoil {
            base: f.f32("private_recoil_info.base"),
            max: f.f32("private_recoil_info.max"),
            factor: f.f32("private_recoil_info.factor"),
            return_speed: f.f32("private_recoil_info.returnSpeed"),
            firing_return_speed: f.f32("private_recoil_info.firingReturnSpeed"),
        },
        sight: files::WeaponSight {
            position: f.vec3("sight.position"),
            vector: f.vec3("sight.vector"),
        },
        max_sight_distance: f.f32("maximum_sight_distance"),
        laser_color: format!("#{:08x}", f.u32("laser_color")),
        second_hand: files::SecondHand {
            position: f.vec3("secondHandPosition"),
            vector: f.vec3("secondHandVector"),
            up: f.vec3("secondHandUpVector"),
        },
        pickup_offset: f.vec3("pickup_offset"),
        display_y_offset: f.f32("display_yoffset"),
        empty_sound: (!empty.is_empty()).then_some(empty),
        ai: ai_params(f, "ai_parameters"),
        ai_alternate: (f.u16("shooter_count") > 1).then(|| ai_params(f, "ai_parameters_alt")),
        attachments,
        sphere_radius: None,
        cursor: None,
        cursor_targeted: None,
        tunnel: None,
        icon: None,
        hud_empty: None,
        hud_fill: None,
        instance: None,
    }
}

/// Writes `weapons.json` (every `ONWC` in `dat`, level 0) into `common`.
pub fn write_classes(
    dat: &Dat<'_>,
    registry: &oni_template::Registry,
    common: &Path,
) -> std::io::Result<usize> {
    let mut out = BTreeMap::new();
    for (inst, rec) in crate::fields::all(registry, dat, TAG_ONWC) {
        let f = Fields(&rec);
        // The geometry: a named model lives in common/models under its name,
        // an unnamed one under its tag and index (`asset_name`).
        let geometry = instance(dat, f.u32("geometry"));
        let model = geometry
            .filter(|g| g.tag == TAG_M3GM)
            .map(|g| format!("models/{}.glb", crate::asset_name(dat, g)));
        // Its physics sphere (`WPiMakeSphereTree`): a quarter of the largest
        // coordinate of its geometry's points.
        let radius = geometry
            .and_then(|g| oni_geom::extract_mesh(dat, g))
            .map(|m| {
                m.positions
                    .iter()
                    .flat_map(|p| p.iter().map(|c| c.abs()))
                    .fold(0.0f32, f32::max)
                    / 4.0
            });
        let mut c = class(f, model);
        c.sphere_radius = radius;
        let texture = |name: &str| instance(dat, f.u32(name)).map(|t| crate::asset_name(dat, t));
        // Its sprites (`WPtSprite`: texture, shade, scale): the reticle, the
        // reticle on a target, the sight's tunnel (with how many and how far
        // apart).
        let sprite = |name: &str| {
            texture(&format!("{name}.texture")).map(|t| files::WeaponSprite {
                texture: t,
                shade: format!("#{:08x}", f.u32(&format!("{name}.shade"))),
                scale: f.f32(&format!("{name}.scale")),
            })
        };
        c.cursor = sprite("cursor");
        c.cursor_targeted = sprite("cursor_targeted");
        c.tunnel = sprite("tunnel").map(|v| files::WeaponTunnel {
            texture: v.texture,
            shade: v.shade,
            scale: v.scale,
            count: f.u32("tunnel_count"),
            spacing: f.f32("tunnel_spacing"),
        });
        // Its HUD textures: the icon, the empty and the full ammo ring.
        c.icon = texture("icon");
        c.hud_empty = texture("hud_empty");
        c.hud_fill = texture("hud_fill");
        // (Its instance name: the key of its new-weapon message.)
        c.instance = inst.name.clone();
        out.insert(c.name.clone(), c);
    }
    let count = out.len();
    std::fs::create_dir_all(common)?;
    std::fs::write(
        common.join("weapons.json"),
        serde_json::to_string_pretty(&files::WeaponsFile { classes: out })?,
    )?;
    Ok(count)
}

/// Writes a level's placed weapons and powerups to `items.json`.
pub fn write_items(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    let weapons: Vec<oni_schema::files::PlacedWeapon> = crate::objects::read(dat, raw, "Weapon")
        .1
        .iter()
        .map(|o| oni_schema::files::PlacedWeapon {
            id: o.id,
            class: cstr(o.data, 0, 32),
            position: o.position,
            // The editor keeps a placed weapon's yaw in the X rotation.
            yaw: o.rotation[0],
        })
        .collect();
    let powerups: Vec<oni_schema::files::PlacedPowerup> = crate::objects::read(dat, raw, "PowerUp")
        .1
        .iter()
        .map(|o| {
            let code = o.data.get(..4).unwrap_or_default();
            let kind = POWERUPS
                .iter()
                .find(|(c, _)| c.as_slice() == code)
                .map_or("ammoBallistic", |(_, n)| n);
            oni_schema::files::PlacedPowerup {
                id: o.id,
                kind: kind.to_string(),
                position: o.position,
                yaw: o.rotation[1],
            }
        })
        .collect();
    if weapons.is_empty() && powerups.is_empty() {
        return Ok(());
    }
    std::fs::write(
        root.join("items.json"),
        serde_json::to_string_pretty(&oni_schema::files::ItemsFile { weapons, powerups })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_hand_built_class() {
        let mut d = vec![0u8; SIZE];
        d[92..98].copy_from_slice(b"w0_tst");
        d[192..194].copy_from_slice(&12u16.to_le_bytes());
        d[194..196].copy_from_slice(&1u16.to_le_bytes());
        d[196..198].copy_from_slice(&1u16.to_le_bytes());
        d[204..208].copy_from_slice(&((1u32 << 3) | (1 << 5)).to_le_bytes());
        let a = 464;
        d[a + 48..a + 53].copy_from_slice(b"w0_e1");
        d[a + 68..a + 70].copy_from_slice(&1u16.to_le_bytes());
        d[a + 70..a + 72].copy_from_slice(&12u16.to_le_bytes());
        d[a + 76..a + 78].copy_from_slice(&0u16.to_le_bytes());
        let registry = oni_template::Registry::load();
        let rec = oni_template::decode(
            registry.get(TAG_ONWC).unwrap(),
            &d,
            oni_dat::ByteOrder::Little,
        )
        .unwrap();
        let c = serde_json::to_value(class(Fields(&rec), Some("models/x.glb".into()))).unwrap();
        assert_eq!(c["name"], json!("w0_tst"));
        assert_eq!(c["flags"], json!(["twoHanded", "autofire"]));
        assert_eq!(c["maxAmmo"], json!(12));
        assert_eq!(c["attachments"][0]["class"], json!("w0_e1"));
        assert_eq!(c["attachments"][0]["chamberTime"], json!(12));
        assert_eq!(c["attachments"][0]["shooterIndex"], json!(0));
        assert!(c.get("aiAlternate").is_none());
    }
}
