//! Impact effects (`docs/AUDIO.md`, *Impact effects*;
//! `docs/sound/ORIGINAL_TRIGGERS.md` §1): what a footstep, landing or blow
//! sounds like, the noise the AI hears from it, by impact type × material ×
//! weight (`common/impacts.json`), and each character class's say in it.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::ai::perception::Stimulus;

/// Weights ("modifiers"): any matches every weight.
pub const ANY: u8 = 0;
pub const HEAVY: u8 = 1;
pub const MEDIUM: u8 = 2;
pub const LIGHT: u8 = 3;

pub fn modifier(name: &str) -> u8 {
    match name {
        "heavy" => HEAVY,
        "medium" => MEDIUM,
        "light" => LIGHT,
        _ => ANY,
    }
}

/// A blow's weight by its damage (≥15 heavy, ≥10 medium, >0 light).
pub fn damage_modifier(damage: u16) -> u8 {
    match damage {
        d if d >= 15 => HEAVY,
        d if d >= 10 => MEDIUM,
        0 => ANY,
        _ => LIGHT,
    }
}

/// An entry's sound: an impulse (maybe none) and the AI's noise.
pub struct Sound {
    pub impulse: String,
    pub ai: Option<(Stimulus, f32)>,
}

pub struct Entry {
    material: u16,
    /// Its slot: 0 impact, 1 damage, 2 projectile.
    component: usize,
    modifier: u8,
    pub sound: Option<Sound>,
    /// The particles it makes, in turn (`docs/PARTICLES.md`, *Effects*).
    pub particles: Vec<EffectParticle>,
}

/// One of an entry's particles: its class (by name: three named in the
/// shipped table don't exist, and are skipped), how it is turned and where
/// it goes.
pub struct EffectParticle {
    pub class: String,
    pub orientation: crate::p3::effect::Orientation,
    pub location: crate::p3::effect::Location,
}

fn effect_particle(p: &Value) -> EffectParticle {
    use crate::p3::effect::{Location, Orientation};
    let l = &p["location"];
    let location = match l["type"].as_str().unwrap_or("") {
        "collisionOffset" => Location::CollisionOffset(l["offset"].as_f64().unwrap_or(0.0) as f32),
        "findWall" => Location::FindWall,
        "decal" => Location::Decal {
            is_static: l["static"].as_bool().unwrap_or(false),
            random_rotation: l["randomRotation"].as_bool().unwrap_or(false),
        },
        "attachedToObject" => Location::AttachedToObject,
        _ => Location::Default,
    };
    EffectParticle {
        class: p["class"].as_str().unwrap_or("").to_string(),
        orientation: Orientation::from_name(p["orientation"].as_str().unwrap_or("")),
        location,
    }
}

#[derive(Default)]
struct Tree {
    ids: HashMap<String, u16>,
    parent: Vec<Option<u16>>,
    depth: Vec<u16>,
}

impl Tree {
    fn load(j: &Value) -> Tree {
        let mut t = Tree::default();
        let names: Vec<&String> = j
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        for (k, n) in names.iter().enumerate() {
            t.ids.insert((*n).clone(), k as u16);
        }
        t.parent = names
            .iter()
            .map(|n| j[n.as_str()].as_str().and_then(|p| t.ids.get(p).copied()))
            .collect();
        t.depth = (0..names.len())
            .map(|k| t.chain(k as u16).count() as u16 - 1)
            .collect();
        t
    }

    /// `k`, its parent, … up to the root.
    fn chain(&self, k: u16) -> impl Iterator<Item = u16> + '_ {
        std::iter::successors(Some(k), |&k| self.parent.get(k as usize).copied().flatten()).take(64)
    }
}

pub struct Impacts {
    impacts: Tree,
    materials: Tree,
    /// Impact types at or under `Blunt`: entries picked by the deepest
    /// material rather than the most specific impact.
    by_material: Vec<bool>,
    textures: HashMap<String, u16>,
    entries: Vec<Entry>,
    /// (impact, material) → its entries, in the table's order.
    pairs: HashMap<(u16, u16), Vec<usize>>,
}

fn stimulus(name: &str) -> Stimulus {
    match name {
        "interesting" => Stimulus::SoundInteresting,
        "danger" => Stimulus::SoundDanger,
        "melee" => Stimulus::SoundMelee,
        "gunshot" => Stimulus::SoundGunshot,
        _ => Stimulus::SoundIgnore,
    }
}

impl Impacts {
    pub fn load(common: &Path) -> Option<Impacts> {
        let j: Value =
            serde_json::from_str(&std::fs::read_to_string(common.join("impacts.json")).ok()?)
                .ok()?;
        Some(Impacts::from_json(&j))
    }

    pub fn from_json(j: &Value) -> Impacts {
        let impacts = Tree::load(&j["impacts"]);
        let materials = Tree::load(&j["materials"]);
        let blunt = impacts.ids.get("Blunt").copied();
        let by_material = (0..impacts.parent.len())
            .map(|k| blunt.is_some_and(|b| impacts.chain(k as u16).any(|c| c == b)))
            .collect();
        let textures = j["textures"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(t, m)| Some((t.clone(), *materials.ids.get(m.as_str()?)?)))
            .collect();
        let mut entries = Vec::new();
        let mut pairs: HashMap<(u16, u16), Vec<usize>> = HashMap::new();
        for e in j["entries"].as_array().into_iter().flatten() {
            let (Some(&impact), Some(&material)) = (
                impacts.ids.get(e["impact"].as_str().unwrap_or("")),
                materials.ids.get(e["material"].as_str().unwrap_or("")),
            ) else {
                continue;
            };
            let sound = e["sound"].is_object().then(|| Sound {
                impulse: e["sound"]["impulse"].as_str().unwrap_or("").to_string(),
                ai: e["sound"]["ai"].is_object().then(|| {
                    (
                        stimulus(e["sound"]["ai"]["type"].as_str().unwrap_or("")),
                        e["sound"]["ai"]["radius"].as_f64().unwrap_or(0.0) as f32,
                    )
                }),
            });
            let component = match e["component"].as_str() {
                Some("damage") => 1,
                Some("projectile") => 2,
                _ => 0,
            };
            pairs
                .entry((impact, material))
                .or_default()
                .push(entries.len());
            let particles = e["particles"]
                .as_array()
                .into_iter()
                .flatten()
                .map(effect_particle)
                .collect();
            entries.push(Entry {
                material,
                component,
                modifier: modifier(e["modifier"].as_str().unwrap_or("")),
                sound,
                particles,
            });
        }
        Impacts {
            impacts,
            materials,
            by_material,
            textures,
            entries,
            pairs,
        }
    }

    pub fn impact(&self, name: &str) -> Option<u16> {
        self.impacts.ids.get(name).copied()
    }

    pub fn material(&self, name: &str) -> Option<u16> {
        self.materials.ids.get(name).copied()
    }

    /// A texture's material (by its file's name).
    pub fn texture_material(&self, texture: &str) -> Option<u16> {
        self.textures.get(texture).copied()
    }

    pub fn entry(&self, k: usize) -> &Entry {
        &self.entries[k]
    }

    /// The entries an impact makes on a material at a weight, one at most
    /// per slot: up the impact's types, and for each up the material's — the
    /// first found per slot; or, under `Blunt`, the one of the deepest
    /// material (the more specific impact on a tie).
    pub fn lookup(&self, impact: u16, material: u16, modifier: u8) -> [Option<usize>; 3] {
        let mut found = [None; 3];
        let mut depth = [0u16; 3];
        let by_material = self
            .by_material
            .get(impact as usize)
            .copied()
            .unwrap_or(false);
        for i in self.impacts.chain(impact) {
            for m in self.materials.chain(material) {
                for &k in self.pairs.get(&(i, m)).into_iter().flatten() {
                    let e = &self.entries[k];
                    if modifier != ANY && e.modifier != ANY && modifier != e.modifier {
                        continue;
                    }
                    let c = e.component;
                    if by_material {
                        let d = self.materials.depth[e.material as usize];
                        if found[c].is_some() && depth[c] >= d {
                            continue;
                        }
                        depth[c] = d;
                        found[c] = Some(k);
                    } else if found[c].is_none() {
                        found[c] = Some(k);
                        if found.iter().all(Option::is_some) {
                            return found;
                        }
                    }
                }
            }
        }
        found
    }
}

/// The animation impact kinds, in the order a class lists its impacts.
pub mod anim {
    pub const WALK: usize = 0;
    pub const RUN: usize = 1;
    pub const CROUCH: usize = 2;
    pub const SLIDE: usize = 3;
    pub const LAND: usize = 4;
    pub const LAND_HARD: usize = 5;
    pub const LAND_DEAD: usize = 6;
    pub const KNOCKDOWN: usize = 7;
    pub const THROWN: usize = 8;
    pub const STANDING_TURN: usize = 9;
    pub const RUN_START: usize = 10;
    pub const ONE_STEP: usize = 11;
    pub const RUN_STOP: usize = 12;
    pub const WALK_STOP: usize = 13;
    pub const SPRINT: usize = 14;
}

/// The impact kind an animation type makes on its footsteps (the
/// original's table; attacks land flying attackers).
pub fn anim_impact(kind: &str) -> Option<usize> {
    use anim::*;
    Some(match kind {
        "standingTurnLeft" | "standingTurnRight" => STANDING_TURN,
        "walk" | "walkStart" | "walkSidestepLeft" | "walkSidestepRight" | "walkBackwards"
        | "consoleWalk" => WALK,
        "walkStop" => WALK_STOP,
        "run" | "runBackwards" | "runSidestepLeft" | "runSidestepRight" => RUN,
        "runStart" | "runBackwardsStart" | "runSidestepLeftStart" | "runSidestepRightStart" => {
            RUN_START
        }
        "runStop" => RUN_STOP,
        "oneStepStop" => ONE_STEP,
        k if k.starts_with("crouchRun") || k.starts_with("crouchWalk") => CROUCH,
        "slide" => SLIDE,
        k if k.starts_with("blownup") || k.starts_with("knockdown") => KNOCKDOWN,
        k if k.starts_with("thrown") => THROWN,
        "land" | "landForward" | "landRight" | "landLeft" | "landBack" => LAND,
        "landHard" | "landForwardHard" | "landRightHard" | "landLeftHard" | "landBackHard" => {
            LAND_HARD
        }
        "landDead" => LAND_DEAD,
        k if is_attack(k) => LAND,
        _ => return None,
    })
}

/// The attack animation types (the original's list of those that land).
fn is_attack(k: &str) -> bool {
    matches!(
        k,
        "kick"
            | "punch"
            | "kickForward"
            | "kickLeft"
            | "kickRight"
            | "kickBack"
            | "kickLow"
            | "punchForward"
            | "punchLeft"
            | "punchRight"
            | "punchBack"
            | "punchLow"
            | "kick2"
            | "kick3"
            | "kick3Forward"
            | "punch2"
            | "punch3"
            | "punch4"
            | "ppk"
            | "pkk"
            | "pkp"
            | "kpk"
            | "kpp"
            | "kkp"
            | "pk"
            | "kp"
            | "ppkk"
            | "ppkkk"
            | "ppkkkk"
            | "punchHeavy"
            | "kickHeavy"
            | "punchForwardHeavy"
            | "kickForwardHeavy"
            | "runKick"
            | "runPunch"
            | "runBackPunch"
            | "runBackKick"
            | "sidestepLeftKick"
            | "sidestepLeftPunch"
            | "sidestepRightKick"
            | "sidestepRightPunch"
            | "throw"
            | "throwForwardPunch"
            | "throwForwardKick"
            | "throwBehindPunch"
            | "throwBehindKick"
            | "runThrowForwardPunch"
            | "runThrowBehindPunch"
            | "runThrowForwardKick"
            | "runThrowBehindKick"
            | "pfPf"
            | "pfPfPf"
            | "plPl"
            | "plPlPl"
            | "prPr"
            | "prPrPr"
            | "pbPb"
            | "pbPbPb"
            | "pdPd"
            | "pdPdPd"
            | "kfKf"
            | "kfKfKf"
            | "klKl"
            | "klKlKl"
            | "krKr"
            | "krKrKr"
            | "kbKb"
            | "kbKbKb"
            | "kdKd"
            | "kdKdKd"
    )
}

/// A character class's say in its impacts (`classes.json`).
#[derive(Default)]
pub struct ClassSounds {
    /// Its steps' weight, and the impact type of each animation impact kind.
    pub modifier: u8,
    pub anim: Vec<String>,
    /// Special attacks: attack name → (impact, weight).
    pub attacks: HashMap<String, (String, u8)>,
    /// Per body part: the material struck, and the impact a blow with it
    /// makes on a hit, a block, a kill.
    pub part_materials: Vec<Option<String>>,
    pub part_impacts: [Vec<Option<String>>; 3],
    /// Its variant (instance name): whose animation sounds it makes.
    pub variant: Option<String>,
    /// How it cries out when hurt.
    pub pain: Pain,
    /// What its AI says, per vocalization kind: a chance (%) and an ambient.
    pub vocalizations: Vec<(u8, String)>,
}

/// A class's pain tunings (`painConstants`) and grunts (impulses).
#[derive(Default, Clone)]
pub struct Pain {
    pub base_percentage: u32,
    pub max_percentage: u32,
    pub percentage_threshold: u32,
    pub timer: u32,
    pub min_timer: u32,
    pub max_light: u32,
    pub max_medium: u32,
    pub death_chance: u32,
    pub volume_threshold: u32,
    pub medium_threshold: u32,
    pub heavy_threshold: u32,
    pub min_volume: f32,
    /// Light, medium, heavy, death.
    pub sounds: [String; 4],
}

impl ClassSounds {
    pub fn from_json(c: &Value) -> ClassSounds {
        let names = |j: &Value| {
            j.as_array()
                .map(|a| a.iter().map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        ClassSounds {
            modifier: modifier(c["animImpacts"]["modifier"].as_str().unwrap_or("")),
            anim: c["animImpacts"]["impacts"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().unwrap_or("").to_string())
                        .collect()
                })
                .unwrap_or_default(),
            attacks: c["attackImpacts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    Some((
                        a["attack"].as_str()?.to_string(),
                        (
                            a["impact"].as_str()?.to_string(),
                            modifier(a["modifier"].as_str().unwrap_or("")),
                        ),
                    ))
                })
                .collect(),
            part_materials: names(&c["partMaterials"]),
            part_impacts: [
                names(&c["partImpacts"]["hit"]),
                names(&c["partImpacts"]["blocked"]),
                names(&c["partImpacts"]["killed"]),
            ],
            variant: c["variant"].as_str().map(str::to_string),
            pain: {
                let p = &c["pain"];
                let u = |k: &str| p[k].as_u64().unwrap_or(0) as u32;
                let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
                Pain {
                    base_percentage: u("basePercentage"),
                    max_percentage: u("maxPercentage"),
                    percentage_threshold: u("percentageThreshold"),
                    timer: u("timer"),
                    min_timer: u("minTimer"),
                    max_light: u("maxLight"),
                    max_medium: u("maxMedium"),
                    death_chance: u("deathChance"),
                    volume_threshold: u("volumeThreshold"),
                    medium_threshold: u("mediumThreshold"),
                    heavy_threshold: u("heavyThreshold"),
                    min_volume: p["minVolume"].as_f64().unwrap_or(0.0) as f32,
                    sounds: [s("light"), s("medium"), s("heavy"), s("death")],
                }
            },
            vocalizations: c["vocalizations"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| {
                    (
                        v["chance"].as_u64().unwrap_or(0) as u8,
                        v["sound"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect(),
        }
    }
}

/// The part a set of body-part bits stands for: the highest (none: the
/// pelvis).
pub fn primary_part(bits: u32) -> usize {
    if bits == 0 {
        0
    } else {
        31 - bits.leading_zeros() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table() -> Impacts {
        Impacts::from_json(&json!({
            "impacts": { "Default": null, "Blunt": "Default", "Footstep": "Blunt", "Footstep_Run": "Footstep", "Melee": "Default", "Punch": "Melee" },
            "materials": { "Base": null, "Stone": "Base", "Concrete": "Stone", "Character": "Base", "Flesh": "Character" },
            "textures": { "floor01": "Concrete" },
            "entries": [
                { "impact": "Footstep", "material": "Concrete", "component": "impact", "modifier": "any", "sound": { "impulse": "step_con" } },
                { "impact": "Footstep_Run", "material": "Base", "component": "impact", "modifier": "light", "sound": { "impulse": "run_any" } },
                { "impact": "Footstep_Run", "material": "Base", "component": "damage", "modifier": "any", "sound": { "impulse": "", "ai": { "type": "ignore", "radius": 80 } } },
                { "impact": "Melee", "material": "Character", "component": "projectile", "modifier": "any", "sound": { "impulse": "punch_any" } },
                { "impact": "Punch", "material": "Flesh", "component": "projectile", "modifier": "heavy", "sound": { "impulse": "face_hvy" } },
                { "impact": "Punch", "material": "Flesh", "component": "projectile", "modifier": "light", "sound": { "impulse": "face_lite" } }
            ]
        }))
    }

    fn impulse(t: &Impacts, k: Option<usize>) -> &str {
        k.and_then(|k| t.entry(k).sound.as_ref())
            .map_or("", |s| s.impulse.as_str())
    }

    #[test]
    fn under_blunt_the_deepest_material_wins() {
        let t = table();
        let (run, concrete) = (
            t.impact("Footstep_Run").unwrap(),
            t.texture_material("floor01").unwrap(),
        );
        // A run on concrete: the more general Footstep's concrete entry beats
        // the run's own on the base material.
        let found = t.lookup(run, concrete, LIGHT);
        assert_eq!(impulse(&t, found[0]), "step_con");
        // The AI noise slot is filled from the run's own entry.
        assert_eq!(
            t.entry(found[1].unwrap()).sound.as_ref().unwrap().ai,
            Some((Stimulus::SoundIgnore, 80.0))
        );
    }

    #[test]
    fn elsewhere_the_most_specific_impact_wins_and_weights_filter() {
        let t = table();
        let (punch, flesh) = (t.impact("Punch").unwrap(), t.material("Flesh").unwrap());
        assert_eq!(impulse(&t, t.lookup(punch, flesh, HEAVY)[2]), "face_hvy");
        assert_eq!(impulse(&t, t.lookup(punch, flesh, LIGHT)[2]), "face_lite");
        // No medium entry for a punch on flesh: up the trees, Melee on
        // Character.
        assert_eq!(impulse(&t, t.lookup(punch, flesh, MEDIUM)[2]), "punch_any");
    }

    #[test]
    fn animation_types_map_to_impact_kinds() {
        assert_eq!(anim_impact("run"), Some(anim::RUN));
        assert_eq!(anim_impact("crouchWalkBackwards"), Some(anim::CROUCH));
        assert_eq!(anim_impact("kickForwardHeavy"), Some(anim::LAND));
        assert_eq!(anim_impact("stand"), None);
        assert_eq!(primary_part(0b1000_0100), 7);
        assert_eq!(damage_modifier(12), MEDIUM);
    }
}
