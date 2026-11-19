//! Doors (`OT_Door`; `docs/ASSET_FORMAT.md`, *Doors*): each level's doors —
//! which open for whom, and the event lists they run. Their closed geometry
//! is the environment's `door_0x<object id>` nodes.

use std::path::Path;

use oni_dat::Dat;
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16};

const NAME: usize = 63;

/// The door classes' sounds (`DOOR`): the impulse played opening and
/// closing (names[32] @28, @60) and the noise the AI hears (u32 type @20,
/// −1 none; f32 radius @24).
pub fn class_sounds(dat: &Dat<'_>) -> serde_json::Map<String, Value> {
    const TAG_DOOR: oni_dat::FourCc = oni_dat::FourCc(u32::from_be_bytes(*b"DOOR"));
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_DOOR)
        .filter_map(|i| {
            let d = dat.instance_data(i)?;
            if d.len() < 92 {
                return None;
            }
            let name = i.name.as_deref()?.trim_start_matches("DOOR").to_string();
            let mut c = json!({ "open": cstr(d, 28, 32), "close": cstr(d, 60, 32) });
            let kind = crate::le_u32(d, 20);
            if kind < crate::impacts::AI_SOUNDS.len() as u32 {
                c["ai"] = json!({ "type": crate::impacts::AI_SOUNDS[kind as usize], "radius": le_f32(d, 24) });
            }
            Some((name, c))
        })
        .collect()
}

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
