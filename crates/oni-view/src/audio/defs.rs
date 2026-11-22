//! The sound definitions (`common/sounds.json`): groups and their
//! permutations, ambient sounds, impulse sounds.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

pub(super) struct Permutation {
    pub weight: u32,
    pub volume: (f32, f32),
    pub pitch: (f32, f32),
    pub sound: String,
}

pub(super) struct Group {
    pub volume: f32,
    pub pitch: f32,
    /// 2: stereo (its ambient channel, and alt-impulse counting, take the
    /// stereo pool — whatever its sounds really are).
    pub channels: u32,
    pub prevent_repeats: bool,
    /// The last permutation picked.
    pub last: usize,
    pub permutations: Vec<Permutation>,
}

pub(super) const INTERRUPT_ON_STOP: u32 = 1;
pub(super) const PLAY_ONCE: u32 = 2;
pub(super) const PAN: u32 = 4;

#[derive(Default)]
pub(super) struct AmbientDef {
    pub priority: u8,
    pub flags: u32,
    pub sphere_radius: f32,
    pub detail_time: (f32, f32),
    pub max_distance: f32,
    pub min_distance: f32,
    pub detail: Option<String>,
    pub base: Option<String>,
    pub base2: Option<String>,
    pub in_sound: Option<String>,
    pub out_sound: Option<String>,
    pub threshold: usize,
    pub min_occlusion: f32,
}

#[derive(Clone)]
pub(super) struct ImpulseDef {
    pub group: String,
    pub priority: u8,
    pub max_distance: f32,
    pub min_distance: f32,
    /// Past this many copies of its group playing, play this impulse instead.
    pub alt: Option<(usize, String)>,
    /// A hit this fast or faster is at full volume (0: always full).
    pub impact_velocity: f32,
    pub min_occlusion: f32,
}

#[derive(Default)]
pub(super) struct Defs {
    pub ambients: HashMap<String, AmbientDef>,
    pub groups: HashMap<String, Group>,
    pub impulses: HashMap<String, ImpulseDef>,
}

impl Defs {
    /// The ambient a script names, as the original finds it
    /// (`OSrAmbient_GetByName`): its hash table compares names ignoring case,
    /// but finds only those in the name's hash bucket — and its hash
    /// (`AUrString_GetHashValue`, which keeps little more than the last few
    /// letters) mostly agrees across case. So `c00_01_106Shinatama` finds
    /// `c00_01_106shinatama`. The exact name first; else one equal ignoring
    /// case with the same hash.
    pub fn ambient_name(&self, name: &str) -> Option<String> {
        if self.ambients.contains_key(name) {
            return Some(name.to_string());
        }
        let h = original_hash(name);
        self.ambients
            .keys()
            .find(|k| k.eq_ignore_ascii_case(name) && original_hash(k) == h)
            .cloned()
    }
}

/// `AUrString_GetHashValue` as shipped: the Dragon Book's hash, but masking
/// with the top bits where it meant to clear them.
fn original_hash(s: &str) -> u32 {
    let mut v: u32 = 0;
    for &c in s.as_bytes() {
        v = (v << 4).wrapping_add(u32::from(c));
        let i = v & 0xf000_0000;
        if i != 0 {
            v = (v ^ (i >> 24)) & i;
        }
    }
    v
}

fn f(j: &Value) -> f32 {
    j.as_f64().unwrap_or(0.0) as f32
}

fn pair(j: &Value) -> (f32, f32) {
    (f(&j[0]), f(&j[1]))
}

fn name(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

impl Defs {
    pub fn load(common: &Path) -> Defs {
        let mut d = Defs::default();
        let Some(j) = crate::json::read(common.join("sounds.json")) else {
            return d;
        };
        for (k, a) in j["ambients"].as_object().into_iter().flatten() {
            let flags = a["flags"].as_array().into_iter().flatten().fold(0, |m, f| {
                m | match f.as_str() {
                    Some("interruptOnStop") => INTERRUPT_ON_STOP,
                    Some("playOnce") => PLAY_ONCE,
                    Some("pan") => PAN,
                    _ => 0,
                }
            });
            d.ambients.insert(
                k.clone(),
                AmbientDef {
                    priority: a["priority"].as_u64().unwrap_or(1).min(3) as u8,
                    flags,
                    sphere_radius: f(&a["sphereRadius"]),
                    detail_time: pair(&a["detailTime"]),
                    max_distance: f(&a["maxVolumeDistance"]),
                    min_distance: f(&a["minVolumeDistance"]),
                    detail: name(&a["detail"]),
                    base: name(&a["base1"]),
                    base2: name(&a["base2"]),
                    in_sound: name(&a["in"]),
                    out_sound: name(&a["out"]),
                    threshold: a["threshold"].as_u64().unwrap_or(3) as usize,
                    min_occlusion: f(&a["minOcclusion"]),
                },
            );
        }
        for (k, g) in j["groups"].as_object().into_iter().flatten() {
            d.groups.insert(
                k.clone(),
                Group {
                    volume: f(&g["volume"]),
                    pitch: f(&g["pitch"]),
                    channels: g["channels"].as_u64().unwrap_or(1) as u32,
                    prevent_repeats: g["preventRepeats"].as_bool().unwrap_or(false),
                    last: 0,
                    permutations: g["permutations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|p| Permutation {
                            weight: p["weight"].as_u64().unwrap_or(0) as u32,
                            volume: pair(&p["volume"]),
                            pitch: pair(&p["pitch"]),
                            sound: p["sound"].as_str().unwrap_or_default().to_string(),
                        })
                        .collect(),
                },
            );
        }
        for (k, i) in j["impulses"].as_object().into_iter().flatten() {
            let Some(group) = name(&i["group"]) else {
                continue;
            };
            d.impulses.insert(
                k.clone(),
                ImpulseDef {
                    group,
                    priority: i["priority"].as_u64().unwrap_or(1).min(3) as u8,
                    max_distance: f(&i["maxVolumeDistance"]),
                    min_distance: f(&i["minVolumeDistance"]),
                    alt: name(&i["alt"])
                        .map(|a| (i["altThreshold"].as_u64().unwrap_or(0) as usize, a)),
                    impact_velocity: f(&i["impactVelocity"]),
                    min_occlusion: f(&i["minOcclusion"]),
                },
            );
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_found_as_the_original_finds_them() {
        // The shipped hash keeps little of a long name's start: the case of
        // an early letter doesn't change it.
        assert_eq!(
            original_hash("c00_01_106Shinatama"),
            original_hash("c00_01_106shinatama")
        );
        assert_eq!(original_hash("c00_01_106shinatama"), 0x0068_a831);
        // A late letter's case does.
        assert_ne!(original_hash("abc"), original_hash("abC"));
        let mut defs = Defs::default();
        for name in ["c00_01_106shinatama", "abc"] {
            defs.ambients
                .insert(name.to_string(), AmbientDef::default());
        }
        assert_eq!(
            defs.ambient_name("c00_01_106shinatama").as_deref(),
            Some("c00_01_106shinatama")
        );
        assert_eq!(
            defs.ambient_name("c00_01_106Shinatama").as_deref(),
            Some("c00_01_106shinatama")
        );
        assert_eq!(
            defs.ambient_name("abC"),
            None,
            "another bucket: not found, as shipped"
        );
        assert_eq!(defs.ambient_name("nothing"), None);
    }
}
