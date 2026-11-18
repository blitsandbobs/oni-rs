//! Doors (`OT_Door`; `docs/ASSET_FORMAT.md`, *Doors*): each level's doors —
//! which open for whom, and the event lists they run. Their closed geometry
//! is the environment's `door_0x<object id>` nodes.

use std::path::Path;

use oni_dat::Dat;
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16};

const NAME: usize = 63;

/// A level's doors.
pub fn doors(dat: &Dat<'_>, raw: &[u8]) -> Vec<Value> {
    let (version, objects) = crate::objects::read(dat, raw, "Door");
    objects
        .iter()
        .filter(|o| o.data.len() >= NAME + 22)
        .map(|o| {
            let d = o.data;
            let flags = (le_u16(d, NAME + 4) & 0x1795) as u32;
            let radius = le_f32(d, NAME + 18).max(0.0).sqrt();
            let events_at = if version >= 16 { NAME + 22 + 2 * NAME } else { NAME + 22 };
            let (events, _) = crate::consoles::events(d, events_at);
            let mut door = json!({
                "id": le_u16(d, NAME),
                "objectId": o.id,
                "class": cstr(d, 0, NAME),
                "position": o.position,
                "axes": crate::patrols::euler_axes(o.rotation),
                "radius": radius,
            });
            let key = le_u16(d, NAME + 2);
            if key != 0 {
                door["key"] = json!(key);
            }
            let f = oni_schema::bit_names(&oni_schema::DOOR_FLAGS, flags);
            if !f.is_empty() {
                door["flags"] = json!(f);
            }
            if !events.is_empty() {
                door["events"] = json!(events);
            }
            door
        })
        .collect()
}

/// Writes `doors.json` into a level folder.
pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    let d = doors(dat, raw);
    if d.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(root.join("doors.json"), serde_json::to_string_pretty(&json!({ "doors": d }))?)
}
