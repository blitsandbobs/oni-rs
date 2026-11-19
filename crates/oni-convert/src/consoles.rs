//! Consoles (`OT_Console`; `docs/ASSET_FORMAT.md`, *Consoles*): each
//! level's consoles with their event lists and where a character stands to
//! use them (the action marker, from the console's class in level 0).

use std::collections::HashMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

use crate::{cstr, le_u16};

const TAG_CONS: FourCc = FourCc(u32::from_be_bytes(*b"CONS"));
/// Fixed name fields are this long.
const NAME: usize = 63;

/// A console class: where its action marker is and which way it faces (in
/// the console's own space), and its flags.
#[derive(Clone, Debug, Default)]
pub struct Class {
    pub position: [f32; 3],
    pub vector: [f32; 3],
    pub flags: u32,
    /// The screen's model (drawn at the console, textured by its state: the
    /// console's own screen textures — every shipped class's are `NONE`).
    pub screen_model: Option<String>,
}

/// The console classes (level 0's `CONS` templates), by name.
pub fn classes(dat: &Dat<'_>, registry: &oni_template::Registry) -> HashMap<String, Class> {
    let Some(layout) = registry.get(TAG_CONS) else { return HashMap::new() };
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_CONS)
        .filter_map(|inst| {
            let name = inst.name.as_deref()?.trim_start_matches("CONS").to_string();
            let rec = oni_template::decode(layout, dat.instance_data(inst)?, dat.header.endian).ok()?;
            let f = |k: &str| match rec.get(k).map(|e| &e.value) {
                Some(oni_template::Value::Float(v)) => *v,
                _ => 0.0,
            };
            let class = Class {
                position: [f("action_position.x"), f("action_position.y"), f("action_position.z")],
                vector: [f("action_vector.x"), f("action_vector.y"), f("action_vector.z")],
                flags: rec.int("flags").unwrap_or(0) as u32,
                screen_model: match rec.get("screen_geometry").map(|e| &e.value) {
                    Some(oni_template::Value::Ref(_, r)) => {
                        let r = *r;
                        (r & 1 != 0)
                            .then(|| dat.instances.get((r >> 8) as usize))
                            .flatten()
                            .map(|g| format!("models/{}.glb", crate::asset_name(dat, g)))
                    }
                    _ => None,
                },
            };
            Some((name, class))
        })
        .collect()
}

/// An event list (`Oni_Event`): a count, then each event's type and
/// parameter. Returns the events and the bytes read.
pub fn events(d: &[u8], at: usize) -> (Vec<Value>, usize) {
    let mut out = Vec::new();
    let n = le_u16(d, at) as usize;
    let mut p = at + 2;
    for _ in 0..n {
        if p + 2 > d.len() {
            break;
        }
        let kind = le_u16(d, p);
        p += 2;
        let name = oni_schema::name(&oni_schema::EVENT_TYPES, kind).unwrap_or("none");
        match kind {
            1 => {
                out.push(json!({ "type": name, "script": cstr(d, p, 32) }));
                p += 32;
            }
            2..=11 => {
                out.push(json!({ "type": name, "id": le_u16(d, p) }));
                p += 2;
            }
            _ => out.push(json!({ "type": name })),
        }
    }
    (out, p - at)
}

/// A level's consoles.
pub fn consoles(dat: &Dat<'_>, raw: &[u8], classes: &HashMap<String, Class>) -> Vec<Value> {
    let (_, objects) = crate::objects::read(dat, raw, "Console");
    objects
        .iter()
        .filter(|o| o.data.len() >= NAME + 4 + 3 * NAME + 2)
        .map(|o| {
            let d = o.data;
            let class_name = cstr(d, 0, NAME);
            let id = le_u16(d, NAME);
            let flags = (le_u16(d, NAME + 2) & 0x68) as u32;
            let screen = |k: usize| cstr(d, NAME + 4 + k * NAME, NAME);
            let (events, _) = events(d, NAME + 4 + 3 * NAME);
            let axes = crate::patrols::euler_axes(o.rotation);
            let class = classes.get(&class_name).cloned().unwrap_or_default();
            let rotate = |v: [f32; 3]| -> [f32; 3] {
                std::array::from_fn(|k| axes[0][k] * v[0] + axes[1][k] * v[1] + axes[2][k] * v[2])
            };
            let at = rotate(class.position);
            let v = rotate(class.vector);
            let marker = [o.position[0] + at[0], o.position[1] + at[1], o.position[2] + at[2]];
            let facing = (-v[0]).atan2(-v[2]).rem_euclid(std::f32::consts::TAU);
            let mut c = json!({
                "id": id,
                "class": class_name,
                "position": o.position,
                "axes": axes,
                "marker": { "position": marker, "facing": facing },
                "events": events,
                "screens": { "inactive": screen(0), "active": screen(1), "triggered": screen(2) },
            });
            if let Some(m) = &class.screen_model {
                c["screenModel"] = json!(m);
            }
            let f = oni_schema::bit_names(&oni_schema::CONSOLE_FLAGS, flags);
            if !f.is_empty() {
                c["flags"] = json!(f);
            }
            if class.flags & 1 != 0 {
                c["alarmPrompt"] = json!(true);
            }
            c
        })
        .collect()
}

/// Writes `consoles.json` into a level folder.
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path, classes: &HashMap<String, Class>) -> std::io::Result<()> {
    let c = consoles(dat, raw, classes);
    if c.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("consoles.json"), serde_json::to_string_pretty(&json!({ "consoles": c }))?)
}
