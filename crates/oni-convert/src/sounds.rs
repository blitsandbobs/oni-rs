//! Sound definitions (`docs/AUDIO.md`): the ambient sounds and sound groups
//! (level 0's `OSBD` instances, each a binary blob in the `.raw`) →
//! `common/sounds.json`. The sounds themselves are the WAVs under `audio/`.
//!
//! An `OSBD` instance holds the blob's size @0 and its `.raw` offset @4; the
//! blob starts with its class (`OSAm` ambient, `OSGr` group, `OSIm` impulse —
//! stored byte-reversed) and size, then the definition, which begins with
//! its format version (1–6; older versions lack later fields, which then
//! take the loader's defaults). Names are 32-byte strings.
//!
//! Ambient: priority, flags (1 interrupt on stop, 2 play once, 4 pan),
//! sphere radius, detail time min/max (seconds), max- and min-volume
//! distances (feet), the five group names (detail, base 1, base 2, in, out),
//! threshold (copies at once; from version 5, else 3), min occlusion (from
//! version 6, else 0).
//!
//! Impulse (a one-shot): its group's name, priority (0 low … 3 highest),
//! max- and min-volume distances (feet), three angle fields the game never
//! uses, then from version 4 the alt threshold and alt impulse's name, from
//! 5 the impact velocity, from 6 the min occlusion.
//!
//! Group: volume (from version 2, else 1), pitch (from 3, else 1), u16 flags
//! (1 prevent repeats) and u16 flag data (from 6; before, prevent repeats
//! with 4 or more permutations), channels, permutation count, then per
//! permutation: weight, volume min/max, pitch min/max, the sound's name.

use std::collections::BTreeMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

use crate::{cstr, le_f32, le_u16, le_u32};

const TAG_OSBD: FourCc = FourCc(u32::from_be_bytes(*b"OSBD"));
const NAME: usize = 32;

/// One definition blob's class, version and body (after the version).
fn blob<'a>(dat: &Dat<'a>, raw: &'a [u8], inst: &oni_dat::Instance) -> Option<(&'a [u8], u32, &'a [u8])> {
    let d = dat.instance_data(inst)?;
    let at = le_u32(d, 4) as usize;
    let size = le_u32(raw.get(at..at + 8)?, 4) as usize;
    let b = raw.get(at..at + 8 + size)?;
    (size >= 4).then(|| (&b[..4], le_u32(b, 8), &b[12..]))
}

fn name(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// An ambient sound's definition (format `version`).
pub fn ambient(version: u32, b: &[u8]) -> Option<Value> {
    if b.len() < 188 + 4 * (u32::from(version >= 5) + u32::from(version >= 6)) as usize {
        return None;
    }
    let mut a = json!({
        "priority": le_u32(b, 0),
        "sphereRadius": le_f32(b, 8),
        "detailTime": [le_f32(b, 12), le_f32(b, 16)],
        "maxVolumeDistance": le_f32(b, 20),
        "minVolumeDistance": le_f32(b, 24),
        "threshold": if version >= 5 { le_u32(b, 188) } else { 3 },
        "minOcclusion": if version >= 6 { le_f32(b, 192) } else { 0.0 },
    });
    let flags = le_u32(b, 4);
    let names: Vec<&str> =
        [(1, "interruptOnStop"), (2, "playOnce"), (4, "pan")].iter().filter(|(m, _)| flags & m != 0).map(|(_, n)| *n).collect();
    if !names.is_empty() {
        a["flags"] = json!(names);
    }
    for (k, key) in ["detail", "base1", "base2", "in", "out"].iter().enumerate() {
        if let Some(n) = name(cstr(b, 28 + k * NAME, NAME)) {
            a[*key] = json!(n);
        }
    }
    Some(a)
}

/// An impulse sound's definition (format `version`).
pub fn impulse(version: u32, b: &[u8]) -> Option<Value> {
    if b.len() < 56 {
        return None;
    }
    let mut i = json!({
        "group": cstr(b, 0, NAME),
        "priority": le_u32(b, 32),
        "maxVolumeDistance": le_f32(b, 36),
        "minVolumeDistance": le_f32(b, 40),
    });
    if version >= 4 && b.len() >= 92 {
        let alt = cstr(b, 60, NAME);
        if le_u32(b, 56) > 0 && !alt.is_empty() {
            i["altThreshold"] = json!(le_u32(b, 56));
            i["alt"] = json!(alt);
        }
    }
    if version >= 5 && b.len() >= 96 && le_f32(b, 92) > 0.0 {
        i["impactVelocity"] = json!(le_f32(b, 92));
    }
    if version >= 6 && b.len() >= 100 && le_f32(b, 96) > 0.0 {
        i["minOcclusion"] = json!(le_f32(b, 96));
    }
    Some(i)
}

/// A sound group's definition (format `version`; `sound`: the WAV's name
/// under `audio/`).
pub fn group(version: u32, b: &[u8]) -> Option<Value> {
    // (Fields join the format over versions: each absent one shifts the rest.)
    let mut o = 0;
    let mut next = |present: bool, len: usize| {
        let at = o;
        if present {
            o += len;
        }
        present.then_some(at)
    };
    let volume = next(version >= 2, 4);
    let pitch = next(version >= 3, 4);
    let flags = next(version >= 6, 4);
    let channels = next(true, 4).unwrap_or(0);
    let count = next(true, 4).unwrap_or(0);
    let start = o;
    if b.len() < start {
        return None;
    }
    let f = |at: Option<usize>| at.map_or(1.0, |a| le_f32(b, a));
    let n = le_u32(b, count) as usize;
    let perms: Vec<Value> = (0..n)
        .map_while(|i| {
            let o = start + i * (20 + NAME);
            (o + 20 + NAME <= b.len()).then(|| {
                json!({
                    "weight": le_u32(b, o),
                    "volume": [le_f32(b, o + 4), le_f32(b, o + 8)],
                    "pitch": [le_f32(b, o + 12), le_f32(b, o + 16)],
                    "sound": crate::sanitize(&cstr(b, o + 20, NAME)),
                })
            })
        })
        .collect();
    let mut g = json!({
        "volume": f(volume),
        "pitch": f(pitch),
        "channels": le_u32(b, channels),
        "permutations": perms,
    });
    let prevent = match flags {
        Some(at) => le_u16(b, at) & 1 != 0,
        None => n >= 4,
    };
    if prevent {
        g["preventRepeats"] = json!(true);
    }
    Some(g)
}

/// A level's sound objects: spheres (heard within their own min distance,
/// placed) and boxes (heard inside, not placed). The record: the ambient's
/// name, the shape (`SPHR`: max / min distance in feet; `VLME`: min and max
/// corners relative to the object, unrotated), then right after the shape
/// pitch (never used) and volume (a sphere's record is padded to a box's
/// size after them).
pub fn objects(dat: &Dat<'_>, raw: &[u8]) -> Vec<Value> {
    const SPHERE: u32 = u32::from_be_bytes(*b"SPHR");
    const BOX: u32 = u32::from_be_bytes(*b"VLME");
    crate::objects::read(dat, raw, "Sound")
        .1
        .iter()
        .filter(|o| o.data.len() >= NAME + 4 + 8 + 8)
        .filter_map(|o| {
            let d = o.data;
            let (shape, size) = match le_u32(d, NAME) {
                SPHERE => (json!({ "sphere": { "max": le_f32(d, NAME + 4), "min": le_f32(d, NAME + 8) } }), 8),
                BOX if d.len() >= NAME + 4 + 24 + 8 => {
                    let v = |o: usize| [le_f32(d, o), le_f32(d, o + 4), le_f32(d, o + 8)];
                    (json!({ "box": { "min": v(NAME + 4), "max": v(NAME + 16) } }), 24)
                }
                _ => return None,
            };
            let mut s = json!({
                "objectId": o.id,
                "ambient": cstr(d, 0, NAME),
                "position": o.position,
                "volume": le_f32(d, NAME + 4 + size + 4),
            });
            s.as_object_mut()?.extend(shape.as_object()?.clone());
            Some(s)
        })
        .collect()
}

/// Writes a level's `sounds.json` (its sound objects), if it has any.
pub fn write_objects(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<usize> {
    let o = objects(dat, raw);
    if !o.is_empty() {
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("sounds.json"), serde_json::to_string_pretty(&json!({ "objects": o }))?)?;
    }
    Ok(o.len())
}

/// Writes `sounds.json` (ambients and groups) into `common`.
pub fn write(dat: &Dat<'_>, raw: &[u8], common: &Path) -> std::io::Result<(usize, usize)> {
    let mut ambients = BTreeMap::new();
    let mut groups = BTreeMap::new();
    let mut impulses = BTreeMap::new();
    for inst in dat.instances.iter().filter(|i| i.tag == TAG_OSBD) {
        let Some(full) = inst.name.as_deref() else { continue };
        let Some((class, version, body)) = blob(dat, raw, inst) else { continue };
        let key = full.rsplit_once('.').map_or(full, |(n, _)| n).to_string();
        match class {
            b"mASO" => {
                if let Some(a) = ambient(version, body) {
                    ambients.insert(key, a);
                }
            }
            b"rGSO" => {
                if let Some(g) = group(version, body) {
                    groups.insert(key, g);
                }
            }
            b"mISO" => {
                if let Some(i) = impulse(version, body) {
                    impulses.insert(key, i);
                }
            }
            _ => {}
        }
    }
    let counts = (ambients.len(), groups.len());
    if counts != (0, 0) {
        std::fs::create_dir_all(common)?;
        std::fs::write(
            common.join("sounds.json"),
            serde_json::to_string_pretty(&json!({ "ambients": ambients, "groups": groups, "impulses": impulses }))?,
        )?;
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_hand_built_definitions() {
        let mut a = vec![0u8; 196];
        a[4..8].copy_from_slice(&5u32.to_le_bytes());
        a[20..24].copy_from_slice(&10f32.to_le_bytes());
        a[24..28].copy_from_slice(&50f32.to_le_bytes());
        a[28 + NAME..28 + NAME + 4].copy_from_slice(b"loop");
        a[188..192].copy_from_slice(&3u32.to_le_bytes());
        let a = ambient(6, &a).unwrap();
        assert_eq!(a["flags"], json!(["interruptOnStop", "pan"]));
        assert_eq!(a["base1"], json!("loop"));
        assert!(a.get("detail").is_none());
        assert_eq!(a["minVolumeDistance"], json!(50.0));

        let mut g = vec![0u8; 20 + 52];
        g[0..4].copy_from_slice(&1f32.to_le_bytes());
        g[16..20].copy_from_slice(&1u32.to_le_bytes());
        g[20..24].copy_from_slice(&10u32.to_le_bytes());
        g[40..49].copy_from_slice(b"hum_x.aif");
        let g6 = group(6, &g).unwrap();
        assert_eq!(g6["permutations"][0]["sound"], json!("hum_x_aif"));
        assert_eq!(g6["permutations"][0]["weight"], json!(10));
        // Version 1: no volume, pitch or flags — the count sits at 4.
        let mut g1 = vec![0u8; 8 + 52];
        g1[4..8].copy_from_slice(&1u32.to_le_bytes());
        g1[8 + 20..8 + 25].copy_from_slice(b"a.aif");
        let g1 = group(1, &g1).unwrap();
        assert_eq!(g1["volume"], json!(1.0));
        assert_eq!(g1["permutations"][0]["sound"], json!("a_aif"));

        let mut i = vec![0u8; 100];
        i[0..4].copy_from_slice(b"bang");
        i[32..36].copy_from_slice(&2u32.to_le_bytes());
        i[40..44].copy_from_slice(&40f32.to_le_bytes());
        i[56..60].copy_from_slice(&3u32.to_le_bytes());
        i[60..63].copy_from_slice(b"alt");
        i[92..96].copy_from_slice(&25f32.to_le_bytes());
        let i6 = impulse(6, &i).unwrap();
        assert_eq!((i6["group"].as_str(), i6["priority"].as_u64()), (Some("bang"), Some(2)));
        assert_eq!((i6["alt"].as_str(), i6["altThreshold"].as_u64()), (Some("alt"), Some(3)));
        assert_eq!(i6["impactVelocity"], json!(25.0));
        let i3 = impulse(3, &i).unwrap();
        assert!(i3.get("alt").is_none() && i3.get("impactVelocity").is_none(), "older versions lack them");
    }
}
