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
}

impl Classes {
    /// Opens the class list under the converted-asset root `oni_root`.
    pub fn open(oni_root: &Path, level: u16) -> std::io::Result<Classes> {
        let dir = oni_root.join("common/characters");
        let text = std::fs::read_to_string(dir.join("classes.json"))?;
        let doc: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| std::io::Error::other(format!("classes.json: {e}")))?;
        Ok(Classes {
            dir,
            anim_dir: oni_root.join("common/animations"),
            level,
            entries: doc["classes"].as_array().cloned().unwrap_or_default(),
            collections: HashMap::new(),
            kinds: HashMap::new(),
        })
    }

    /// The spawnable kind for class `name`, loading it on first use;
    /// `upload` registers its model with the renderer and returns the model
    /// index.
    pub fn kind(
        &mut self,
        name: &str,
        upload: impl FnOnce(&Character) -> usize,
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

        let model = c["model"].as_str().ok_or_else(|| format!("{name}: no model"))?;
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
                eprintln!("class {name} → {coll_name}: {} clips (level {})", a.len(), self.level);
                let a = Rc::new(a);
                self.collections.insert(coll_name, a.clone());
                a
            }
        };

        let kind = CharKind {
            model: upload(&character),
            character: Rc::new(character),
            anims,
            tuning: tuning(&c),
        };
        self.kinds.insert(name.to_string(), kind.clone());
        Ok(kind)
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
    if let Some(v) = f(&air["jumpGravity"]) {
        t.jump_gravity = v;
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
