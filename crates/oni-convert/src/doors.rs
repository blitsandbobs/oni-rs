//! Doors (`OT_Door`; `oni_schema::files::DoorsFile`): each level's doors —
//! which open for whom, and the event lists they run. Their closed geometry
//! is the environment's `door_0x<object id>` nodes.

use std::path::Path;

use oni_dat::Dat;
use std::collections::BTreeMap;

use oni_schema::files;

use crate::{cstr, le_f32, le_u16};

const NAME: usize = 63;

/// The door classes' sounds (`DOOR`): the impulse played opening and
/// closing (names[32] @28, @60) and the noise the AI hears (u32 type @20,
/// −1 none; f32 radius @24).
pub fn class_sounds(dat: &Dat<'_>) -> BTreeMap<String, files::DoorClass> {
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
            // Its leaves' animation (`OBtAnimation` ref @8).
            let r = crate::le_u32(d, 8);
            let animation = (r & 1 != 0)
                .then(|| dat.instances.get((r >> 8) as usize))
                .flatten()
                .and_then(|a| oni_geom::objanim::decode_object_animation(dat, a))
                .map(|a| crate::object_animation_file(&a));
            // What sound passes it while it isn't open (`sound_allow` @16:
            // all, shouting, gunfire, none) and how much it muffles what does
            // (`sound_muffle_fraction` @12; 1: nothing passes).
            const ALLOW: [&str; 4] = ["all", "shouting", "gunfire", "none"];
            let kind = crate::le_u32(d, 20);
            Some((
                name,
                files::DoorClass {
                    open: cstr(d, 28, 32),
                    close: cstr(d, 60, 32),
                    animation,
                    sound_allow: ALLOW
                        .get(crate::le_u32(d, 16) as usize)
                        .copied()
                        .unwrap_or("none")
                        .to_string(),
                    sound_muffle: le_f32(d, 12),
                    ai: (kind < crate::impacts::AI_SOUNDS.len() as u32).then(|| files::AiNoise {
                        kind: crate::impacts::AI_SOUNDS[kind as usize].to_string(),
                        radius: le_f32(d, 24),
                    }),
                    bounds: leaf_bounds(dat, d),
                },
            ))
        })
        .collect()
}

/// A door class's first leaf geometry's bounding box: `geometry_array[0]`
/// (ref @0) → `OFGA` (`num_furn_geoms` @20, `furn_geom[0].geometry` @28) →
/// `M3GM` (`pointArray` @4) → `PNTA` (`minmax_boundingBox` @12: min, max).
fn leaf_bounds(dat: &Dat<'_>, door: &[u8]) -> Option<[[f32; 3]; 2]> {
    let at = |d: &[u8], o: usize| {
        let r = crate::le_u32(d, o);
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .and_then(|i| dat.instance_data(i))
    };
    let furn = at(door, 0).filter(|f| f.len() >= 32 && crate::le_u32(f, 20) > 0)?;
    let points = at(at(furn, 28).filter(|g| g.len() >= 8)?, 4).filter(|p| p.len() >= 36)?;
    let v = |o: usize| {
        [
            le_f32(points, o),
            le_f32(points, o + 4),
            le_f32(points, o + 8),
        ]
    };
    Some([v(12), v(24)])
}

/// A level's doors.
pub fn doors(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::Door> {
    let (version, objects) = crate::objects::read(dat, raw, "Door");
    let setups = leaf_setups(dat);
    objects
        .iter()
        .filter(|o| o.data.len() >= NAME + 22)
        .map(|o| {
            let d = o.data;
            let flags = (le_u16(d, NAME + 4) & 0x1795) as u32;
            let events_at = if version >= 16 {
                NAME + 22 + 2 * NAME
            } else {
                NAME + 22
            };
            let (events, _) = crate::consoles::events(d, events_at);
            // Its moving leaves: the level's object setups made for it (the
            // second of double doors at id | 0x1000), each with its origin.
            let id = u32::from(le_u16(d, NAME));
            let leaves = [id, id | 0x1000]
                .iter()
                .filter_map(|k| setups.iter().find(|s| s.0 == *k))
                .map(|s| files::DoorLeaf { origin: s.1 })
                .collect();
            let key = le_u16(d, NAME + 2);
            files::Door {
                id: le_u16(d, NAME),
                object_id: o.id,
                class: cstr(d, 0, NAME),
                position: o.position,
                axes: crate::patrols::euler_axes(o.rotation),
                radius: le_f32(d, NAME + 18).max(0.0).sqrt(),
                key: (key != 0).then_some(key),
                flags: oni_schema::bit_names(&oni_schema::DOOR_FLAGS, flags)
                    .into_iter()
                    .map(String::from)
                    .collect(),
                events,
                leaves,
            }
        })
        .collect()
}

/// The level's door-leaf object setups (`OBOA`, `OBtObjectSetup`, 240
/// bytes each after a 24-byte header): `doorScriptID` @20 and the origin
/// matrix `debugOrigMatrix` @64 (4×3, column-major).
fn leaf_setups(dat: &Dat<'_>) -> Vec<(u32, [f32; 12])> {
    const TAG_OBOA: oni_dat::FourCc = oni_dat::FourCc(u32::from_be_bytes(*b"OBOA"));
    let Some(d) = dat
        .instances
        .iter()
        .find(|i| i.tag == TAG_OBOA)
        .and_then(|i| dat.instance_data(i))
    else {
        return Vec::new();
    };
    (0..le_u16(d, 22) as usize)
        .map(|k| 24 + k * 240)
        .take_while(|o| o + 240 <= d.len())
        .filter(|&o| crate::le_u32(d, o + 20) != 0)
        .map(|o| {
            (
                crate::le_u32(d, o + 20),
                std::array::from_fn(|i| le_f32(d, o + 64 + 4 * i)),
            )
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
    std::fs::write(
        root.join("doors.json"),
        serde_json::to_string_pretty(&files::DoorsFile { doors: d })?,
    )
}
