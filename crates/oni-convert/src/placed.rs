//! The level's placed objects (`OBOA`, `OBtObjectSetup`; `docs/ASSET_FORMAT.md`,
//! *Placed objects*): what scripts create, kill and animate by script id —
//! forklifts, trucks, windows — each with its models, clip and placement.
//! (Door leaves are exported with their doors.)

use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

const TAG_OBOA: FourCc = FourCc(u32::from_be_bytes(*b"OBOA"));
const TAG_M3GA: FourCc = FourCc(u32::from_be_bytes(*b"M3GA"));
const TAG_OBAN: FourCc = FourCc(u32::from_be_bytes(*b"OBAN"));
/// `sizeof(OBtObjectSetup)`, after the array's 24-byte header.
const STRIDE: usize = 240;

/// The instance a local reference names.
fn local<'a>(dat: &'a Dat<'_>, r: u32) -> Option<&'a oni_dat::Instance> {
    (r & 1 != 0).then(|| dat.instances.get((r >> 8) as usize)).flatten()
}

/// The placed objects: `geometry_array` @0 (`M3GA`: count @20, `M3GM` refs
/// @24), `animation` @4, `flags` @12, `doorScriptID` @20, `physicsLevel`
/// @24, `scriptID` @28, `position` @32, `orientation` @44 (x, y, z, w),
/// `scale` @60, `debugOrigMatrix` @64, `objName` @112.
pub fn objects(dat: &Dat<'_>) -> Vec<Value> {
    let Some(d) = dat.instances.iter().find(|i| i.tag == TAG_OBOA).and_then(|i| dat.instance_data(i)) else { return Vec::new() };
    let mut out = Vec::new();
    for k in 0..le_u16(d, 22) as usize {
        let o = 24 + k * STRIDE;
        if o + STRIDE > d.len() {
            break;
        }
        if le_u32(d, o + 20) != 0 {
            continue; // a door's leaf
        }
        let parts: Vec<&oni_dat::Instance> = local(dat, le_u32(d, o))
            .filter(|g| g.tag == TAG_M3GA)
            .and_then(|g| dat.instance_data(g))
            .map(|g| (0..le_u32(g, 20) as usize).filter_map(|i| local(dat, le_u32(g, 24 + 4 * i))).collect())
            .unwrap_or_default();
        let geometry: Vec<String> = parts.iter().map(|m| format!("models/{}.glb", crate::asset_name(dat, m))).collect();
        // Its collision sphere's radius (`OBiMakeSphereTree`): the largest
        // coordinate of its geometry's points (local). As shipped, each test
        // is against the point less the world position, so the last point
        // to pass wins (spec quirk 16).
        let position = [le_f32(d, o + 32), le_f32(d, o + 36), le_f32(d, o + 40)];
        let (mut shipped, mut fixed) = (-1e9f32, 0.0f32);
        for m in parts.iter().filter_map(|m| oni_geom::extract_mesh(dat, m)) {
            for p in &m.positions {
                for c in 0..3 {
                    if (p[c] - position[c]).abs() > shipped {
                        shipped = p[c].abs();
                    }
                    fixed = fixed.max(p[c].abs());
                }
            }
        }
        let mut obj = json!({
            "index": k,
            "name": cstr(d, o + 112, 64),
            "scriptId": le_u32(d, o + 28),
            "flags": oni_schema::bit_names(&oni_schema::OBJECT_FLAGS, le_u32(d, o + 12)),
            "physicsLevel": oni_schema::PHYSICS_LEVELS.get(le_u32(d, o + 24) as usize).copied().unwrap_or("none"),
            "position": position,
            "orientation": [le_f32(d, o + 44), le_f32(d, o + 48), le_f32(d, o + 52), le_f32(d, o + 56)],
            "scale": le_f32(d, o + 60),
            "origin": (0..12).map(|i| le_f32(d, o + 64 + 4 * i)).collect::<Vec<_>>(),
            "geometry": geometry,
            "radius": { "shipped": shipped.max(1.0), "fixed": fixed.max(1.0) },
        });
        if let Some(a) = local(dat, le_u32(d, o + 4)).filter(|a| a.tag == TAG_OBAN) {
            obj["animation"] = json!(a.name.clone().unwrap_or_else(|| crate::asset_name(dat, a)));
        }
        out.push(obj);
    }
    out
}

/// Writes `objects.json` into a level folder.
pub fn write(dat: &Dat<'_>, root: &Path) -> std::io::Result<()> {
    let o = objects(dat);
    if o.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("objects.json"), serde_json::to_string_pretty(&json!({ "objects": o }))?)
}
