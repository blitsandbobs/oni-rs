//! Emits `level.json` — a level's semantic objects, decoded from the generic
//! template registry ([`oni_template`]) and serialized with references
//! resolved to asset links.
//!
//! We decode a curated set of gameplay/level object tags (not the bulk asset
//! arrays like TXMP/M3GM/geometry, which become files). Template references
//! become `{"$ref": "<name>"}` so the JSON links objects to each other and to
//! assets.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use oni_dat::{Dat, FourCc, Instance};
use oni_template::{decode, Record, Registry, Value};

/// Object tags worth surfacing as semantic level data.
const OBJECT_TAGS: &[[u8; 4]] = &[
    *b"ONLV", // level descriptor
    *b"AGDB", // AI graph / spawn database
    *b"OBDC", // object collection
    *b"ONSA", // sound area array
    *b"AITR", // AI trigger
    *b"AISA", // AI script array
    *b"DOOR", // doors
    *b"TRIG", // triggers
    *b"TURR", // turrets
    *b"CONS", // consoles
    *b"IPge", // input pages / level UI
    *b"OPge", *b"ENVP", // environment particles
    *b"CRSA", // corpses
];

/// The `OBOA` (Starting Object Array) tag — the placed-object list.
const TAG_OBOA: FourCc = FourCc(0x4F42_4F41);
const OBOA_STRIDE: usize = 240; // sizeof(OBtObjectSetup)

fn le_u16(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .unwrap_or(0)
}
fn le_u32(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0)
}
fn le_f32(d: &[u8], o: usize) -> f32 {
    f32::from_bits(le_u32(d, o))
}
fn cstr(d: &[u8], o: usize, n: usize) -> String {
    let s = d.get(o..o + n).unwrap_or(&[]);
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

/// Decodes `OBOA` into the authored placement list: each object's name,
/// transform (`position`/`orientation`/`scale`), script id and door flag.
fn decode_placed_objects(dat: &Dat<'_>) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let Some(inst) = dat.instances.iter().find(|i| i.tag == TAG_OBOA) else {
        return out;
    };
    let Some(d) = dat.instance_data(inst) else {
        return out;
    };
    // OBtObjectSetupArray: pad[22], numObjects u16 @22, objects @24.
    let count = le_u16(d, 22) as usize;
    for k in 0..count {
        let o = 24 + k * OBOA_STRIDE;
        if o + OBOA_STRIDE > d.len() {
            break;
        }
        let door_script = le_u32(d, o + 20);
        out.push(serde_json::json!({
            "name": cstr(d, o + 112, 64),
            "file": cstr(d, o + 176, 64),
            "position": { "x": le_f32(d, o + 32), "y": le_f32(d, o + 36), "z": le_f32(d, o + 40) },
            "orientation": {
                "x": le_f32(d, o + 44), "y": le_f32(d, o + 48),
                "z": le_f32(d, o + 52), "w": le_f32(d, o + 56),
            },
            "scale": le_f32(d, o + 60),
            "scriptId": le_u32(d, o + 28),
            "isDoor": door_script != 0,
        }));
    }
    out
}

/// Decodes the level's objects and writes `<root>/level.json`. Returns the
/// number of objects written.
pub fn write_level_json(
    dat: &Dat<'_>,
    root: &Path,
    registry: &Registry,
    env_objects: &[crate::EnvObjectInfo],
) -> std::io::Result<usize> {
    // Inventory of every tag in the file.
    let mut inventory: BTreeMap<String, usize> = BTreeMap::new();
    for inst in &dat.instances {
        *inventory.entry(inst.tag.to_string()).or_insert(0) += 1;
    }

    let want: Vec<FourCc> = OBJECT_TAGS.iter().map(|b| FourCc::from_bytes(*b)).collect();
    let mut objects = Vec::new();
    for inst in &dat.instances {
        if !want.contains(&inst.tag) {
            continue;
        }
        let Some(layout) = registry.get(inst.tag) else {
            continue;
        };
        let Some(data) = dat.instance_data(inst) else {
            continue;
        };
        let Ok(rec) = decode(layout, data, dat.header.endian) else {
            continue;
        };
        objects.push(serde_json::json!({
            "type": inst.tag.to_string(),
            "name": inst.name,
            "id": inst.index,
            "fields": record_to_json(&rec, dat),
        }));
    }

    // Authored placed objects (OBOA) with transforms.
    let placed = decode_placed_objects(dat);

    // Baked-geometry objects in environment.glb (cross-link by node name).
    let env_geometry: Vec<serde_json::Value> = env_objects
        .iter()
        .map(|e| {
            serde_json::json!({
                "node": e.node,
                "type": e.type_name,
                "tag": format!("{:#010x}", e.tag),
                "pivot": { "x": e.pivot[0], "y": e.pivot[1], "z": e.pivot[2] },
                "triangles": e.triangles,
            })
        })
        .collect();

    let level = serde_json::json!({
        "inventory": inventory,
        "objectCount": objects.len(),
        "objects": objects,
        "placedObjects": placed,
        "environmentGeometry": {
            "glb": "environment.glb",
            "objectCount": env_geometry.len(),
            "objects": env_geometry,
        },
    });

    std::fs::create_dir_all(root)?;
    let mut f = std::fs::File::create(root.join("level.json"))?;
    f.write_all(serde_json::to_string_pretty(&level)?.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(objects.len())
}

pub(crate) fn record_to_json(rec: &Record, dat: &Dat<'_>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for e in &rec.entries {
        if e.name.is_empty() || e.name.starts_with("_pad") {
            continue;
        }
        if let Some(v) = value_to_json(&e.value, dat) {
            // The generic decoder flattens nested structs to dotted names
            // (`position.x`); rebuild them as nested objects (`{position:{x}}`).
            insert_nested(&mut map, &e.name, v);
        }
    }
    serde_json::Value::Object(map)
}

/// Inserts `value` at a dotted path (`a.b.c`) into `map`, creating intermediate
/// objects as needed.
fn insert_nested(
    map: &mut serde_json::Map<String, serde_json::Value>,
    path: &str,
    value: serde_json::Value,
) {
    match path.split_once('.') {
        None => {
            map.insert(path.to_string(), value);
        }
        Some((head, rest)) => {
            let child = map
                .entry(head.to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(obj) = child {
                insert_nested(obj, rest, value);
            }
        }
    }
}

fn value_to_json(val: &Value, dat: &Dat<'_>) -> Option<serde_json::Value> {
    use oni_template::RefKind;
    Some(match val {
        Value::Int(i) => serde_json::json!(i),
        Value::Float(f) => serde_json::json!(f),
        Value::Ref(RefKind::Template, 0) => serde_json::Value::Null,
        Value::Ref(RefKind::Template, v) => {
            let idx = (v >> 8) as usize;
            match dat.instances.get(idx) {
                Some(inst) => serde_json::json!({ "$ref": ref_name(inst) }),
                None => serde_json::json!({ "$ref": format!("#{idx}") }),
            }
        }
        // Raw/separate offsets are into asset payloads; keep as numbers.
        Value::Ref(_, v) => serde_json::json!(v),
        Value::Pad(_) => return None,
        Value::Array(items) => {
            let arr: Vec<serde_json::Value> = items
                .iter()
                .map(|r| {
                    if r.entries.len() == 1 {
                        value_to_json(&r.entries[0].value, dat).unwrap_or(serde_json::Value::Null)
                    } else {
                        record_to_json(r, dat)
                    }
                })
                .collect();
            serde_json::Value::Array(arr)
        }
    })
}

fn ref_name(inst: &Instance) -> String {
    match &inst.name {
        Some(n) if !n.is_empty() => n.clone(),
        _ => format!("{}#{}", inst.tag, inst.index),
    }
}
