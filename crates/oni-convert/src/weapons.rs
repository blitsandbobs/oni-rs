//! Weapons (`docs/WEAPONS.md`): the weapon classes (level 0's `ONWC`
//! templates) → `common/weapons.json`, and each level's placed weapons and
//! powerups (`OT_Weapon` / `OT_PowerUps` objects) → `levels/NN/items.json`.
//!
//! ## `WPtWeaponClass` (1812 bytes, offsets after the instance pre-data)
//!
//! | off | field |
//! |---|---|
//! | 0 | sight marker: position, vector (gun space) |
//! | 24 | maximum sight distance; 28 laser colour (ARGB) |
//! | 88 | geometry (`M3GM` ref) |
//! | 92 | name `char[32]` |
//! | 124 | aiming speed |
//! | 128 | recoil: base (already in internal units), max, factor, return speed, firing return speed |
//! | 184 | u16 free time (unused), recoil anim type, reload anim type, reload time, max ammo, attachment count, shooter count, reload delay time, stop-chamber threshold, pad |
//! | 204 | flags |
//! | 208 | AI parameters (128 bytes) for shooter 0; 336 for the alternate shooter |
//! | 464 | 16 particle attachments × 80 bytes |
//! | 1744 | empty sound name `char[32]` |
//! | 1788 | glow scale; 1796 pickup offset; 1808 display y offset |
//!
//! An attachment: a 4×3 matrix (gun space; columns X, Y, Z, translation),
//! the particle class name `char[16]` @48, then u16 ammo used @68 (non-zero
//! = a *shooter*), chamber time, cheat chamber time, minimum fire time,
//! shooter index (0xFFFF = follows every shooter) and start delay.

use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Map, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

const TAG_ONWC: FourCc = FourCc(u32::from_be_bytes(*b"ONWC"));
const TAG_M3GM: FourCc = FourCc(u32::from_be_bytes(*b"M3GM"));
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

fn vec3(d: &[u8], o: usize) -> [f32; 3] {
    [le_f32(d, o), le_f32(d, o + 4), le_f32(d, o + 8)]
}

fn anim_type(t: u16) -> Value {
    oni_schema::ANIM_TYPES
        .get(t as usize)
        .map_or(json!(t), |s| json!(s))
}

/// The AI's aiming and shooting parameters for one shooter.
pub(crate) fn ai_params(d: &[u8], o: usize) -> oni_schema::files::AiGunParams {
    oni_schema::files::AiGunParams {
        no_wild_shots: le_u32(d, o) & 1 != 0,
        prediction_speed: le_f32(d, o + 76),
        burst_radius: le_f32(d, o + 80),
        aim_radius: le_f32(d, o + 84),
        detection_volume: le_f32(d, o + 88),
        min_safe_distance: le_f32(d, o + 92),
        max_range: le_f32(d, o + 96),
        max_startle_misses: le_u16(d, o + 100),
        shoot_skill_index: le_u16(d, o + 102),
        melee_assist_timer: le_u32(d, o + 104),
        ballistic_speed: le_f32(d, o + 108),
        ballistic_gravity: le_f32(d, o + 112),
        dodge_pattern: [le_f32(d, o + 116), le_f32(d, o + 120), le_f32(d, o + 124)],
        // Where the shot leaves the gun (gun space): its direction, its
        // offset off that line, and the turn from it onto the barrel.
        shooter_direction: vec3(d, o + 52),
        shooter_offset: vec3(d, o + 64),
        shooter_inverse: std::array::from_fn(|k| le_f32(d, o + 4 + k * 4)),
    }
}

/// One weapon class as JSON. `model` is its geometry's path under `common/`.
pub fn class(d: &[u8], model: Option<String>) -> Option<Value> {
    if d.len() < SIZE {
        return None;
    }
    let flags = le_u32(d, 204);
    let names: Vec<&str> = FLAGS
        .iter()
        .enumerate()
        .filter(|(b, n)| !n.is_empty() && flags & (1 << b) != 0)
        .map(|(_, n)| *n)
        .collect();
    let count = (le_u16(d, 194) as usize).min(16);
    let attachments: Vec<Value> = (0..count)
        .map(|i| {
            let o = 464 + i * 80;
            let matrix: Vec<f32> = (0..12).map(|k| le_f32(d, o + k * 4)).collect();
            let shooter = le_u16(d, o + 76);
            json!({
                "class": cstr(d, o + 48, 16),
                "matrix": matrix,
                "ammoUsed": le_u16(d, o + 68),
                "chamberTime": le_u16(d, o + 70),
                "cheatChamberTime": le_u16(d, o + 72),
                "minFireTime": le_u16(d, o + 74),
                "shooterIndex": if shooter == 0xFFFF { -1 } else { i32::from(shooter) },
                "startDelay": le_u16(d, o + 78),
            })
        })
        .collect();
    let mut c = Map::new();
    c.insert("name".into(), json!(cstr(d, 92, 32)));
    if let Some(m) = model {
        c.insert("model".into(), json!(m));
    }
    c.insert("flags".into(), json!(names));
    c.insert("maxAmmo".into(), json!(le_u16(d, 192)));
    c.insert("shooterCount".into(), json!(le_u16(d, 196)));
    c.insert("reloadTime".into(), json!(le_u16(d, 190)));
    c.insert("reloadDelayTime".into(), json!(le_u16(d, 198)));
    c.insert("stopChamberThreshold".into(), json!(le_u16(d, 200)));
    c.insert("recoilAnimType".into(), anim_type(le_u16(d, 186)));
    c.insert("reloadAnimType".into(), anim_type(le_u16(d, 188)));
    c.insert("aimingSpeed".into(), json!(le_f32(d, 124)));
    c.insert(
        "recoil".into(),
        json!({
            "base": le_f32(d, 128),
            "max": le_f32(d, 132),
            "factor": le_f32(d, 136),
            "returnSpeed": le_f32(d, 140),
            "firingReturnSpeed": le_f32(d, 144),
        }),
    );
    c.insert(
        "sight".into(),
        json!({ "position": vec3(d, 0), "vector": vec3(d, 12) }),
    );
    c.insert("maxSightDistance".into(), json!(le_f32(d, 24)));
    c.insert(
        "laserColor".into(),
        json!(format!("#{:08x}", le_u32(d, 28))),
    );
    c.insert(
        "secondHand".into(),
        json!({ "position": vec3(d, 172), "vector": vec3(d, 148), "up": vec3(d, 160) }),
    );
    c.insert("pickupOffset".into(), json!(vec3(d, 1796)));
    c.insert("displayYOffset".into(), json!(le_f32(d, 1808)));
    let empty = cstr(d, 1744, 32);
    if !empty.is_empty() {
        c.insert("emptySound".into(), json!(empty));
    }
    c.insert("ai".into(), json!(ai_params(d, 208)));
    if le_u16(d, 196) > 1 {
        c.insert("aiAlternate".into(), json!(ai_params(d, 336)));
    }
    c.insert("attachments".into(), json!(attachments));
    Some(Value::Object(c))
}

/// Writes `weapons.json` (every `ONWC` in `dat`, level 0) into `common`.
pub fn write_classes(dat: &Dat<'_>, common: &Path) -> std::io::Result<usize> {
    let mut out = Map::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_ONWC) {
        let Some(d) = dat.instance_data(inst) else {
            continue;
        };
        // The geometry: a named model lives in common/models under its name,
        // an unnamed one under its tag and index (`asset_name`).
        let r = le_u32(d, 88);
        let model = (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .filter(|g| g.tag == TAG_M3GM)
            .map(|g| format!("models/{}.glb", crate::asset_name(dat, g)));
        // Its physics sphere (`WPiMakeSphereTree`): a quarter of the largest
        // coordinate of its geometry's points.
        let radius = (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .and_then(|g| oni_geom::extract_mesh(dat, g))
            .map(|m| {
                m.positions
                    .iter()
                    .flat_map(|p| p.iter().map(|c| c.abs()))
                    .fold(0.0f32, f32::max)
                    / 4.0
            });
        if let Some(mut c) = class(d, model) {
            if let Some(r) = radius {
                c["sphereRadius"] = json!(r);
            }
            // Its sprites (`WPtSprite`: texture, shade, scale): the
            // reticle, the reticle on a target, the sight's tunnel (with
            // how many and how far apart).
            let sprite = |at: usize| {
                let r = le_u32(d, at);
                let texture = (r & 1 != 0)
                    .then(|| dat.instances.get((r >> 8) as usize))
                    .flatten()
                    .map(|t| crate::asset_name(dat, t));
                texture.map(|t| json!({ "texture": t, "shade": format!("#{:08x}", le_u32(d, at + 4)), "scale": crate::le_f32(d, at + 8) }))
            };
            if let Some(v) = sprite(32) {
                c["cursor"] = v;
            }
            if let Some(v) = sprite(44) {
                c["cursorTargeted"] = v;
            }
            if let Some(mut v) = sprite(56) {
                v["count"] = json!(le_u32(d, 68));
                v["spacing"] = json!(crate::le_f32(d, 72));
                c["tunnel"] = v;
            }
            // Its HUD textures: the icon, the empty and the full ammo ring.
            for (key, at) in [("icon", 76), ("hudEmpty", 80), ("hudFill", 84)] {
                let r = le_u32(d, at);
                if let Some(t) = (r & 1 != 0)
                    .then(|| dat.instances.get((r >> 8) as usize))
                    .flatten()
                {
                    c[key] = json!(crate::asset_name(dat, t));
                }
            }
            // (Its instance name: the key of its new-weapon message.)
            if let Some(n) = &inst.name {
                c["instance"] = json!(n);
            }
            let name = c["name"].as_str().unwrap_or_default().to_string();
            out.insert(name, c);
        }
    }
    let count = out.len();
    std::fs::create_dir_all(common)?;
    std::fs::write(
        common.join("weapons.json"),
        serde_json::to_string_pretty(&json!({ "classes": out }))?,
    )?;
    Ok(count)
}

/// Writes a level's placed weapons and powerups to `items.json`.
pub fn write_items(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    let weapons: Vec<Value> = crate::objects::read(dat, raw, "Weapon")
        .1
        .iter()
        .map(|o| {
            json!({
                "id": o.id,
                "class": cstr(o.data, 0, 32),
                "position": o.position,
                // The editor keeps a placed weapon's yaw in the X rotation.
                "yaw": o.rotation[0],
            })
        })
        .collect();
    let powerups: Vec<Value> = crate::objects::read(dat, raw, "PowerUp")
        .1
        .iter()
        .map(|o| {
            let code = o.data.get(..4).unwrap_or_default();
            let kind = POWERUPS
                .iter()
                .find(|(c, _)| c.as_slice() == code)
                .map_or("ammoBallistic", |(_, n)| n);
            json!({ "id": o.id, "type": kind, "position": o.position, "yaw": o.rotation[1] })
        })
        .collect();
    if weapons.is_empty() && powerups.is_empty() {
        return Ok(());
    }
    std::fs::write(
        root.join("items.json"),
        serde_json::to_string_pretty(&json!({ "weapons": weapons, "powerups": powerups }))?,
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
        let c = class(&d, Some("models/x.glb".into())).unwrap();
        assert_eq!(c["name"], json!("w0_tst"));
        assert_eq!(c["flags"], json!(["twoHanded", "autofire"]));
        assert_eq!(c["maxAmmo"], json!(12));
        assert_eq!(c["attachments"][0]["class"], json!("w0_e1"));
        assert_eq!(c["attachments"][0]["chamberTime"], json!(12));
        assert_eq!(c["attachments"][0]["shooterIndex"], json!(0));
        assert!(c.get("aiAlternate").is_none());
    }
}
