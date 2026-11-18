//! Laser triggers (`OT_Trigger`; `docs/ASSET_FORMAT.md`, *Laser triggers*):
//! each level's laser emitters with their timing and event lists, and each
//! class's emitter track — where the beam starts and which way it points at
//! every frame of the class's animation (from level 0's `TRIG` / `TRGE` /
//! `OBAN` templates).

use std::collections::BTreeMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

const TAG_TRIG: FourCc = FourCc(u32::from_be_bytes(*b"TRIG"));
const TAG_TRGE: FourCc = FourCc(u32::from_be_bytes(*b"TRGE"));
const NAME: usize = 63;
/// The flags a level keeps (initially active, reversed, ping-pong).
const PERSISTENT_FLAGS: u16 = 0x38;

/// A laser class's track: per animation frame, the beam's origin and unit
/// direction in the trigger's own space.
pub type Track = Vec<[f32; 6]>;

/// The laser classes (level 0's `TRIG` templates), by name.
pub fn classes(dat: &Dat<'_>, registry: &oni_template::Registry) -> BTreeMap<String, Track> {
    let (Some(trig), Some(trge)) = (registry.get(TAG_TRIG), registry.get(TAG_TRGE)) else {
        return BTreeMap::new();
    };
    let instance = |r: Option<i64>| {
        let v = r? as u32;
        (v & 1 != 0).then(|| dat.instances.get((v >> 8) as usize)).flatten()
    };
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_TRIG)
        .filter_map(|inst| {
            let name = inst.name.as_deref()?.trim_start_matches("TRIG").to_string();
            let rec = oni_template::decode(trig, dat.instance_data(inst)?, dat.header.endian).ok()?;
            let em_inst = instance(rec.int("emitter"))?;
            let em = oni_template::decode(trge, dat.instance_data(em_inst)?, dat.header.endian).ok()?;
            let f = |k: &str| match em.get(k).map(|e| &e.value) {
                Some(oni_template::Value::Float(v)) => *v,
                _ => 0.0,
            };
            let at = [f("emit_position.x"), f("emit_position.y"), f("emit_position.z")];
            let v = [f("emit_vector.x"), f("emit_vector.y"), f("emit_vector.z")];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
            let v = v.map(|c| c / len);
            let anim = instance(rec.int("animation")).and_then(|i| oni_geom::objanim::decode_object_animation(dat, i));
            let frames = anim.as_ref().map_or(1, |a| a.num_frames.max(1) as i32);
            let track = (0..frames)
                .map(|fr| {
                    let m = anim.as_ref().map_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0], |a| a.matrix(fr));
                    let apply = |p: [f32; 3], w: f32| -> [f32; 3] {
                        std::array::from_fn(|i| m[i] * p[0] + m[3 + i] * p[1] + m[6 + i] * p[2] + m[9 + i] * w)
                    };
                    let o = apply(at, 1.0);
                    let d = apply(v, 0.0);
                    let dl = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                    [o[0], o[1], o[2], d[0] / dl, d[1] / dl, d[2] / dl]
                })
                .collect();
            Some((name, track))
        })
        .collect()
}

/// A level's laser triggers.
pub fn lasers(dat: &Dat<'_>, raw: &[u8]) -> Vec<Value> {
    let (_, objects) = crate::objects::read(dat, raw, "Trigger");
    objects
        .iter()
        .filter(|o| o.data.len() >= NAME + 22)
        .map(|o| {
            let d = o.data;
            let flags = le_u16(d, NAME + 2) & PERSISTENT_FLAGS;
            let argb = le_u32(d, NAME + 4);
            let byte = |s: u32| ((argb >> s) & 0xFF) as f32 / 255.0;
            let (events, _) = crate::consoles::events(d, NAME + 22);
            let mut l = json!({
                "id": le_u16(d, NAME),
                "class": cstr(d, 0, NAME),
                "position": o.position,
                "axes": crate::patrols::euler_axes(o.rotation),
                "color": [byte(16), byte(8), byte(0)],
                "startOffset": le_f32(d, NAME + 8).clamp(0.0, 1.0),
                "speed": le_f32(d, NAME + 12).clamp(0.0, 100.0),
                "emitters": le_u16(d, NAME + 16).clamp(1, 32),
                "timeOn": le_u16(d, NAME + 18),
                "timeOff": le_u16(d, NAME + 20),
                "events": events,
            });
            let f = oni_schema::bit_names(&oni_schema::LASER_FLAGS, flags as u32);
            if !f.is_empty() {
                l["flags"] = json!(f);
            }
            l
        })
        .collect()
}

/// Writes `lasers.json` into a level folder: its lasers, and the tracks of
/// the classes they use.
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path, classes: &BTreeMap<String, Track>) -> std::io::Result<()> {
    let l = lasers(dat, raw);
    if l.is_empty() {
        return Ok(());
    }
    let used: BTreeMap<&String, &Track> =
        classes.iter().filter(|(n, _)| l.iter().any(|x| x["class"].as_str() == Some(n.as_str()))).collect();
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("lasers.json"), serde_json::to_string_pretty(&json!({ "lasers": l, "classes": used }))?)
}
