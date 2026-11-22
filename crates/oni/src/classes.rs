//! Character classes: everything needed to spawn a given kind of character
//! (Konoko, a Striker, a Ninja, …), read from the converter's
//! `common/characters/classes.json`
//! (`oni_schema::files::CharacterClassesFile`).
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
    entries: Vec<oni_schema::files::CharacterClass>,
    collections: HashMap<String, Rc<AnimCollection>>,
    kinds: HashMap<String, CharKind>,
    /// Clips loaded on their own (`clip`).
    singles: HashMap<String, Rc<AnimCollection>>,
    /// AI melee profiles by name.
    melee: HashMap<String, Rc<crate::ai::melee::profile::Profile>>,
    /// The game settings (`common/game_settings.json`).
    settings: oni_schema::files::GameSettingsFile,
}

impl Classes {
    /// Opens the class list under the converted-asset root `oni_root`.
    pub fn open(oni_root: &Path, level: u16) -> std::io::Result<Classes> {
        let dir = oni_root.join("common/characters");
        let melee =
            crate::ai::melee::profile::load(&dir.join("melee_profiles.json")).unwrap_or_default();
        let text = std::fs::read_to_string(dir.join("classes.json"))?;
        let doc: oni_schema::files::CharacterClassesFile = serde_json::from_str(&text)
            .map_err(|e| std::io::Error::other(format!("classes.json: {e}")))?;
        Ok(Classes {
            dir,
            anim_dir: oni_root.join("common/animations"),
            level,
            entries: doc.classes,
            collections: HashMap::new(),
            kinds: HashMap::new(),
            singles: HashMap::new(),
            melee,
            settings: crate::json::read_file(oni_root.join("common/game_settings.json")),
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
            .find(|c| c.name.as_deref() == Some(name))
            .cloned()
            .ok_or_else(|| format!("no character class {name:?}"))?;

        let model = c
            .model
            .as_deref()
            .ok_or_else(|| format!("{name}: no model"))?;
        let mut character = skel::load_character(&self.dir.join(model))
            .map_err(|e| format!("{name}: model {model}: {e}"))?;
        // The class's own skin over the shared body.
        for part in &mut character.parts {
            if let Some(Some(t)) = c.part_textures.get(part.bone) {
                part.texture = Some(self.dir.join(t));
            }
        }

        let coll_name = c
            .animation_collection
            .clone()
            .ok_or_else(|| format!("{name}: no animation collection"))?;
        let anims = self.collection(name, &coll_name)?;

        let daze = c
            .dazed
            .as_ref()
            .map_or((60, 100), |d| (d.min.max(0) as u32, d.max.max(0) as u32));
        // Its low body, which magic effects are drawn over.
        let low = self.dir.join(model).with_file_name("low.glb");
        let magic_model = skel::load_character(&low).ok().map(|c| upload(&c));
        let kind = CharKind {
            class: name.into(),
            model: upload(&character),
            magic_model,
            character: Rc::new(character),
            anims,
            tuning: tuning(&c),
            melee: c
                .melee_profile
                .as_ref()
                .and_then(|n| self.melee.get(n))
                .cloned(),
            daze,
            perception: crate::ai::perception::Tuning::from_class(c.perception.as_ref()),
            class_combat: crate::ai::goals::ClassCombat::from_class(c.perception.as_ref()),
            death_delete: c
                .stats
                .as_ref()
                .map_or(0, |s| u32::from(s.death_delete_delay)),
            aiming: Rc::new(crate::anim::aim::load(&c.aiming, &self.anim_dir)),
            shooting: Rc::new(
                c.perception
                    .iter()
                    .flat_map(|p| &p.shooting_skill)
                    .map(crate::ai::ranged::Skill::from_file)
                    .collect(),
            ),
            targeting: c
                .perception
                .as_ref()
                .map(|p| crate::ai::ranged::Params::from_file(&p.targeting))
                .unwrap_or_default(),
            sounds: Rc::new(crate::impacts::ClassSounds::from_class(&c)),
            shadow: crate::decal::Shadow::from_class(c.shadow.as_ref()).map(Rc::new),
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
        if let Some(p) = &kind.melee {
            self.plan(p);
        }
        self.kinds.insert(name.to_string(), kind.clone());
        Ok(kind)
    }

    /// Animation collection `coll_name` (class `name`'s), loaded once.
    fn collection(&mut self, name: &str, coll_name: &str) -> Result<Rc<AnimCollection>, String> {
        if let Some(a) = self.collections.get(coll_name) {
            return Ok(a.clone());
        }
        let a = AnimCollection::load(&self.anim_dir, coll_name, self.level)
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
        self.collections.insert(coll_name.to_string(), a.clone());
        Ok(a)
    }

    /// Gives melee profile `p` what it plans with: its class's animations
    /// and jumping (`AI2rMelee_PrepareForUse`), once. A class not found (or
    /// without animations) leaves the fighter's own to plan with.
    fn plan(&mut self, p: &Rc<crate::ai::melee::profile::Profile>) {
        if p.plan.get().is_some() {
            return;
        }
        let Some(c) = self
            .entries
            .iter()
            .find(|c| c.name.as_deref() == Some(p.class.as_str()))
            .cloned()
        else {
            return;
        };
        let Some(coll_name) = c.animation_collection.as_deref() else {
            return;
        };
        if let Ok(coll) = self.collection(&p.class, coll_name) {
            let _ = p.plan.set(Rc::new(crate::ai::melee::profile::Planning {
                coll,
                tuning: tuning(&c),
            }));
        }
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
            .find(|c| c.name.as_deref() == Some(class))
        else {
            return class.to_string();
        };
        let Some(variant) = c.variant_name.as_deref() else {
            return class.to_string();
        };
        let desired = match c.variant_upgrade.as_deref() {
            Some(u) if upgrade && hard && !u.is_empty() => u,
            _ => variant,
        };
        let pool: Vec<&str> = self
            .entries
            .iter()
            .filter(|e| e.variant_name.as_deref() == Some(desired))
            .filter_map(|e| e.name.as_deref())
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
            .find(|c| c.name.as_deref() == Some(class))?;
        self.melee.get(c.melee_profile.as_ref()?).map(|p| p.id)
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
    pub fn melee_by_id(&mut self, id: u16) -> Option<Rc<crate::ai::melee::profile::Profile>> {
        let p = self.melee.values().find(|p| p.id == id).cloned()?;
        self.plan(&p);
        Some(p)
    }

    /// The AI's difficulty multipliers for `level` ("easy", "normal", "hard").
    pub fn difficulty(&self, level: &str) -> crate::ai::melee::brain::Difficulty {
        let d = &self.settings.difficulty;
        let i = d.levels.iter().position(|v| v == level);
        let pick = |m: &[f32]| i.and_then(|i| m.get(i).copied()).unwrap_or(1.0);
        crate::ai::melee::brain::Difficulty {
            notice: pick(&d.notice_multiplier),
            dodge: pick(&d.dodge_multiplier),
            block: pick(&d.block_multiplier),
            player_hp: pick(&d.player_hp_multiplier),
            enemy_hp: pick(&d.enemy_hp_multiplier),
            inaccuracy: pick(&d.inaccuracy_multiplier),
            level: i.map_or(1, |i| i as i64),
        }
    }
}

/// The movement model's tuning from a class entry's `stats` and `air`.
fn tuning(c: &oni_schema::files::CharacterClass) -> ClassTuning {
    let mut t = ClassTuning::default();
    if let Some(s) = &c.stats {
        t.fight_mode_duration = s.fight_mode_duration.min(u32::from(u16::MAX)) as u16;
        t.idle_delay = s.idle_delay;
        t.idle_restart_delay = s.idle_restart_delay;
        t.max_hit_points = s.max_hit_points as i32;
        t.knockdown_resistant = s.knockdown_resistant;
        t.left_handed = s.left_handed;
        t.super_hypo = s.super_hypo;
        t.super_shield = s.super_shield;
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
            if let Some(&v) = s.damage_resistance.get(*name) {
                t.damage_resistance[k] = v;
            }
        }
        t.boss_shield = s.boss_shield;
    }
    if let Some(v) = c.hypo_time {
        t.hypo_time = v as u16;
    }
    if let Some(a) = &c.air {
        t.jump_gravity = a.jump_gravity;
        t.fall_gravity = a.fall_gravity;
        t.jump_amount = a.jump_amount;
        t.max_velocity = a.max_velocity;
        t.jump_accel_amount = a.jump_accel_amount;
        t.num_jump_accel_frames = a.num_jump_accel_frames;
        t.damage_start = a.damage_start;
        t.damage_end = a.damage_end;
    }
    t
}
