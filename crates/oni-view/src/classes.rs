//! Character classes: everything needed to spawn a given kind of character
//! (Konoko, a Striker, a Ninja, …), read from the converter's
//! `common/characters/classes.json` (`docs/ASSET_FORMAT.md`).
//!
//! A class names a body model, its own skin (one texture per body part —
//! several classes share a body with differently coloured skins), an
//! animation collection, and its tuning. Collections are shared between the
//! classes that use them; each loaded class becomes a [`CharKind`] once, with
//! its model uploaded for rendering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::anim::collection::AnimCollection;
use crate::anim::motion::ClassTuning;
use crate::skel::{self, Character};
use crate::world::CharKind;

/// The classes of one converted game, loaded on demand.
pub struct Classes {
    /// `common/characters/`.
    dir: PathBuf,
    anim_dir: PathBuf,
    /// The level being played (clips introduced later are unavailable).
    level: u16,
    entries: Vec<serde_json::Value>,
    collections: HashMap<String, Rc<AnimCollection>>,
    kinds: HashMap<String, CharKind>,
    /// Clips loaded on their own (`clip`).
    singles: HashMap<String, Rc<AnimCollection>>,
    /// AI melee profiles by name.
    melee: HashMap<String, Rc<crate::ai::melee::profile::Profile>>,
    /// The game settings (`common/game_settings.json`).
    settings: serde_json::Value,
}

impl Classes {
    /// Opens the class list under the converted-asset root `oni_root`.
    pub fn open(oni_root: &Path, level: u16) -> std::io::Result<Classes> {
        let dir = oni_root.join("common/characters");
        let melee =
            crate::ai::melee::profile::load(&dir.join("melee_profiles.json")).unwrap_or_default();
        let text = std::fs::read_to_string(dir.join("classes.json"))?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| std::io::Error::other(format!("classes.json: {e}")))?;
        Ok(Classes {
            dir,
            anim_dir: oni_root.join("common/animations"),
            level,
            entries: doc["classes"].as_array().cloned().unwrap_or_default(),
            collections: HashMap::new(),
            kinds: HashMap::new(),
            singles: HashMap::new(),
            melee,
            settings: crate::json::load(oni_root.join("common/game_settings.json")),
        })
    }

    /// The spawnable kind for class `name`, loading it on first use;
    /// `upload` registers its model with the renderer and returns the model
    /// index.
    pub fn kind(
        &mut self,
        name: &str,
        mut upload: impl FnMut(&Character) -> usize,
    ) -> Result<CharKind, String> {
        if let Some(k) = self.kinds.get(name) {
            return Ok(k.clone());
        }
        let c = self
            .entries
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .cloned()
            .ok_or_else(|| format!("no character class {name:?}"))?;

        let model = c["model"]
            .as_str()
            .ok_or_else(|| format!("{name}: no model"))?;
        let mut character = skel::load_character(&self.dir.join(model))
            .map_err(|e| format!("{name}: model {model}: {e}"))?;
        // The class's own skin over the shared body.
        if let Some(skin) = c["partTextures"].as_array() {
            for part in &mut character.parts {
                if let Some(t) = skin.get(part.bone).and_then(|t| t.as_str()) {
                    part.texture = Some(self.dir.join(t));
                }
            }
        }

        let coll_name = c["animationCollection"]
            .as_str()
            .ok_or_else(|| format!("{name}: no animation collection"))?
            .to_string();
        let anims = match self.collections.get(&coll_name) {
            Some(a) => a.clone(),
            None => {
                let a = AnimCollection::load(&self.anim_dir, &coll_name, self.level)
                    .map_err(|e| format!("{name}: collection {coll_name}: {e}"))?;
                if a.is_empty() {
                    return Err(format!("{name}: collection {coll_name} has no clips"));
                }
                eprintln!(
                    "class {name} → {coll_name}: {} clips (level {})",
                    a.len(),
                    self.level
                );
                let a = Rc::new(a);
                self.collections.insert(coll_name, a.clone());
                a
            }
        };

        let daze = (
            c["dazed"]["min"].as_u64().unwrap_or(60) as u32,
            c["dazed"]["max"].as_u64().unwrap_or(100) as u32,
        );
        // Its low body, which magic effects are drawn over.
        let low = self.dir.join(model).with_file_name("low.glb");
        let magic_model = skel::load_character(&low).ok().map(|c| upload(&c));
        let kind = CharKind {
            model: upload(&character),
            magic_model,
            character: Rc::new(character),
            anims,
            tuning: tuning(&c),
            melee: c["meleeProfile"]
                .as_str()
                .and_then(|n| self.melee.get(n))
                .cloned(),
            daze,
            perception: crate::ai::perception::Tuning::from_json(&c["perception"]),
            class_combat: crate::ai::goals::ClassCombat::from_json(&c["perception"]),
            death_delete: c["stats"]["deathDeleteDelay"].as_u64().unwrap_or(0) as u32,
            aiming: Rc::new(crate::anim::aim::load(&c["aiming"], &self.anim_dir)),
            shooting: Rc::new(
                c["perception"]["shootingSkill"]
                    .as_array()
                    .map(|a| a.iter().map(crate::ai::ranged::Skill::from_json).collect())
                    .unwrap_or_default(),
            ),
            targeting: crate::ai::ranged::Params::from_json(&c["perception"]["targeting"]),
            sounds: Rc::new(crate::impacts::ClassSounds::from_json(&c)),
            shadow: crate::decal::Shadow::from_json(&c["shadow"]).map(Rc::new),
            // (Its super particles: those named `super…`, on a body part.)
            super_particles: Rc::new(
                crate::world::anim_particles::class_particles(&c)
                    .into_iter()
                    .filter(|p| p.name.len() >= 5 && p.name[..5].eq_ignore_ascii_case("super"))
                    .filter_map(|p| match p.part {
                        crate::world::anim_particles::ClassPart::Part(k) if k < 19 => {
                            Some((p.class, k))
                        }
                        _ => None,
                    })
                    .collect(),
            ),
            class_particles: Rc::new(crate::world::anim_particles::class_particles(&c)),
        };
        self.kinds.insert(name.to_string(), kind.clone());
        Ok(kind)
    }
}

impl Classes {
    /// The class a placement takes (`ONrGameState_NewCharacter`): with a
    /// random costume or upgraded difficulty, one of the classes of its
    /// variant — on hard, of the variant it upgrades to — picked by `roll`
    /// (0–1). (Upgraded below hard, it still takes a random one of its own
    /// variant: one test covers both flags.)
    pub fn costume(
        &self,
        class: &str,
        random: bool,
        upgrade: bool,
        hard: bool,
        roll: f32,
    ) -> String {
        if !random && !upgrade {
            return class.to_string();
        }
        let Some(c) = self
            .entries
            .iter()
            .find(|c| c["name"].as_str() == Some(class))
        else {
            return class.to_string();
        };
        let Some(variant) = c["variantName"].as_str() else {
            return class.to_string();
        };
        let desired = match c["variantUpgrade"].as_str() {
            Some(u) if upgrade && hard && !u.is_empty() => u,
            _ => variant,
        };
        let pool: Vec<&str> = self
            .entries
            .iter()
            .filter(|e| e["variantName"].as_str() == Some(desired))
            .filter_map(|e| e["name"].as_str())
            .collect();
        if pool.is_empty() {
            eprintln!("classes: {class} (variant {variant}) found no classes of variant {desired}");
            return class.to_string();
        }
        let k = ((pool.len() as f32 * roll) as usize).min(pool.len() - 1);
        pool[k].to_string()
    }

    /// A class's default melee profile's id (`ai2_behavior.default_melee_id`).
    pub fn default_melee_id(&self, class: &str) -> Option<u16> {
        let c = self
            .entries
            .iter()
            .find(|c| c["name"].as_str() == Some(class))?;
        self.melee.get(c["meleeProfile"].as_str()?).map(|p| p.id)
    }

    /// A single clip by name, from any class (loaded once).
    pub fn clip(&mut self, name: &str) -> Option<Rc<AnimCollection>> {
        if let Some(c) = self.singles.get(name) {
            return Some(c.clone());
        }
        let c = Rc::new(AnimCollection::single(&self.anim_dir, name).ok()?);
        self.singles.insert(name.to_string(), c.clone());
        Some(c)
    }

    /// A melee profile by id (what a placed character names).
    pub fn melee_by_id(&self, id: u16) -> Option<Rc<crate::ai::melee::profile::Profile>> {
        self.melee.values().find(|p| p.id == id).cloned()
    }

    /// The AI's difficulty multipliers for `level` ("easy", "normal", "hard").
    pub fn difficulty(&self, level: &str) -> crate::ai::melee::brain::Difficulty {
        let d = &self.settings["difficulty"];
        let i = d["levels"]
            .as_array()
            .and_then(|l| l.iter().position(|v| v.as_str() == Some(level)));
        let pick = |k: &str| i.and_then(|i| d[k][i].as_f64()).map_or(1.0, |v| v as f32);
        crate::ai::melee::brain::Difficulty {
            notice: pick("noticeMultiplier"),
            dodge: pick("dodgeMultiplier"),
            block: pick("blockMultiplier"),
            player_hp: pick("playerHpMultiplier"),
            enemy_hp: pick("enemyHpMultiplier"),
            inaccuracy: pick("inaccuracyMultiplier"),
            level: i.map_or(1, |i| i as i64),
        }
    }
}

/// The movement model's tuning from a class entry's `stats` and `air`.
fn tuning(c: &serde_json::Value) -> ClassTuning {
    let mut t = ClassTuning::default();
    let (stats, air) = (&c["stats"], &c["air"]);
    let u = |v: &serde_json::Value| v.as_u64();
    let f = |v: &serde_json::Value| v.as_f64().map(|x| x as f32);
    if let Some(v) = u(&stats["fightModeDuration"]) {
        t.fight_mode_duration = v.min(u64::from(u16::MAX)) as u16;
    }
    if let Some(v) = u(&stats["idleDelay"]) {
        t.idle_delay = v as u32;
    }
    if let Some(v) = u(&stats["idleRestartDelay"]) {
        t.idle_restart_delay = v as u32;
    }
    if let Some(v) = u(&stats["maxHitPoints"]) {
        t.max_hit_points = v as i32;
    }
    if let Some(v) = stats["knockdownResistant"].as_bool() {
        t.knockdown_resistant = v;
    }
    if let Some(v) = stats["leftHanded"].as_bool() {
        t.left_handed = v;
    }
    if let Some(v) = stats["superHypo"].as_bool() {
        t.super_hypo = v;
    }
    if let Some(v) = stats["superShield"].as_bool() {
        t.super_shield = v;
    }
    for (k, name) in [
        "normal",
        "minorStun",
        "majorStun",
        "minorKnockdown",
        "majorKnockdown",
        "blownUp",
        "pickUp",
    ]
    .iter()
    .enumerate()
    {
        if let Some(v) = f(&stats["damageResistance"][name]) {
            t.damage_resistance[k] = v;
        }
    }
    if let Some(v) = f(&stats["bossShield"]) {
        t.boss_shield = v;
    }
    if let Some(v) = u(&c["hypoTime"]) {
        t.hypo_time = v as u16;
    }
    if let Some(v) = f(&air["jumpGravity"]) {
        t.jump_gravity = v;
    }
    if let Some(v) = f(&air["fallGravity"]) {
        t.fall_gravity = v;
    }
    if let Some(v) = f(&air["jumpAmount"]) {
        t.jump_amount = v;
    }
    if let Some(v) = f(&air["maxVelocity"]) {
        t.max_velocity = v;
    }
    if let Some(v) = f(&air["jumpAccelAmount"]) {
        t.jump_accel_amount = v;
    }
    if let Some(v) = u(&air["numJumpAccelFrames"]) {
        t.num_jump_accel_frames = v as u16;
    }
    if let Some(v) = f(&air["damageStart"]) {
        t.damage_start = v;
    }
    if let Some(v) = f(&air["damageEnd"]) {
        t.damage_end = v;
    }
    t
}
