//! The level's placed objects (behaviour per `docs/OBJECTS.md`; data
//! `levels/NN/objects.json`): forklifts, trucks, windows — none exist until a
//! script creates them (`obj_create`, by script id); each is drawn with its
//! models where its clip (or its placement) puts it, and characters collide
//! with its sphere. Scripts start, swap and stop the clips (`env_anim`,
//! `env_setanim`), hide and kill them.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};
use oni_geom::objanim::ObjectAnimation;

use super::World;

/// `OBcFlags_*` the engine reads.
mod oflag {
    pub const INVISIBLE: u32 = 0x0100;
    pub const IN_USE: u32 = 0x0200;
    pub const NO_COLLISION: u32 = 0x0400;
}
/// `OBcAnimFlags_*`.
mod aflag {
    pub const LOOP: u32 = 0x1;
    pub const PINGPONG: u32 = 0x2;
    pub const AUTOSTART: u32 = 0x8;
    pub const LOCALIZED: u32 = 0x10;
}

/// One placed object as the level authored it (`OBtObjectSetup`).
#[derive(Clone, Debug)]
pub struct Setup {
    pub name: String,
    pub script_id: u32,
    pub flags: u32,
    pub position: Vec3,
    /// Its turn (as the original's quaternion-to-matrix reads it).
    pub rotation: Quat,
    pub scale: f32,
    pub origin: Mat4,
    pub animation: Option<String>,
    pub models: Vec<PathBuf>,
    /// Its collision sphere's radius (as shipped: spec quirk 16, kept in
    /// both modes — the "largest coordinate" the code meant would wall off
    /// whole aisles round a truck).
    pub radius: f32,
}

/// A live object (`OBtObject` with its physics' animation context).
#[derive(Clone, Debug)]
pub struct Object {
    pub setup: usize,
    pub anim: Option<Rc<ObjectAnimation>>,
    pub frame: i32,
    pub step: i32,
    /// The frame it stops at (0: none).
    pub stop: i32,
    pub animating: bool,
    pub flags: u32,
}

/// Loads a level's object setups (`objects.json`).
pub fn load(level_dir: &Path) -> Vec<Setup> {
    let doc: serde_json::Value = std::fs::read_to_string(level_dir.join("objects.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let common = level_dir.join("../../common");
    let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
    doc["objects"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| {
            let flags = o["flags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| oni_schema::OBJECT_FLAGS.iter().position(|k| !k.is_empty() && Some(*k) == n.as_str()))
                .fold(0, |acc, b| acc | (1 << b));
            let q = &o["orientation"];
            let m: Vec<f32> = o["origin"].as_array().map(|a| a.iter().map(f).collect()).unwrap_or_default();
            let col = |c: usize| glam::Vec4::new(m.get(c * 3).copied().unwrap_or(0.0), m.get(c * 3 + 1).copied().unwrap_or(0.0), m.get(c * 3 + 2).copied().unwrap_or(0.0), if c == 3 { 1.0 } else { 0.0 });
            Setup {
                name: o["name"].as_str().unwrap_or("").to_string(),
                script_id: o["scriptId"].as_u64().unwrap_or(0) as u32,
                flags,
                position: Vec3::new(f(&o["position"][0]), f(&o["position"][1]), f(&o["position"][2])),
                // (Oni's quaternion-to-matrix is the transpose of the usual.)
                rotation: Quat::from_xyzw(f(&q[0]), f(&q[1]), f(&q[2]), f(&q[3])).normalize().conjugate(),
                scale: f(&o["scale"]),
                origin: Mat4::from_cols(col(0), col(1), col(2), col(3)),
                animation: o["animation"].as_str().map(str::to_string),
                models: o["geometry"].as_array().into_iter().flatten().filter_map(|g| g.as_str()).map(|g| common.join(g)).collect(),
                radius: f(&o["radius"]["shipped"]).max(1.0),
            }
        })
        .collect()
}

impl Object {
    /// Starts its clip from the beginning (`OBrAnim_Start`).
    fn start(&mut self) {
        self.animating = true;
        self.frame = 0;
        self.step = 1;
    }

    /// Stops it where it is (`OBrAnim_Stop`).
    fn halt(&mut self) {
        if let Some(a) = &self.anim {
            self.frame = self.frame.clamp(0, i32::from(a.num_frames) - 1);
        }
        self.animating = false;
    }

    /// One tick of its clip (`PHrPhysics_Update_Animation`): a frame a tick
    /// (the clip's ticks-per-frame never holds it back), round again for a
    /// looping one, back and forth for a ping-pong one, else it stops at the
    /// end; an autostart clip starts by itself.
    fn tick(&mut self) {
        let Some(a) = self.anim.clone() else { return };
        if !self.animating && a.flags & aflag::AUTOSTART != 0 {
            self.start();
        }
        if !self.animating {
            return;
        }
        self.frame += self.step;
        if self.frame >= i32::from(a.num_frames) || (self.stop > 0 && self.frame >= self.stop) {
            if a.flags & aflag::PINGPONG != 0 {
                self.frame = i32::from(a.num_frames) - 1;
                self.step = -1;
            } else if a.flags & aflag::LOOP != 0 {
                self.frame = 0;
            } else {
                self.halt();
            }
        }
        if self.frame < 0 {
            if a.flags & aflag::PINGPONG != 0 {
                self.frame = 0;
                self.step = 1;
            } else {
                self.halt();
            }
        }
    }

    /// Whether its clip is running and not yet at its last frame (what the
    /// `_block` commands wait for).
    fn playing(&self) -> bool {
        self.animating && self.anim.as_ref().is_some_and(|a| self.frame + 1 < i32::from(a.num_frames))
    }
}

impl World {
    /// Object `k`'s matrix (`PHrPhysics_MaintainMatrix`): its clip's (from
    /// its origin, for a localized one), else its placement.
    pub fn object_matrix(&self, k: usize) -> Mat4 {
        let o = &self.objects[k];
        let s = &self.object_setups[o.setup];
        match &o.anim {
            Some(a) => {
                let m = a.matrix(o.frame);
                let col = |c: usize| glam::Vec4::new(m[c * 3], m[c * 3 + 1], m[c * 3 + 2], if c == 3 { 1.0 } else { 0.0 });
                let m = Mat4::from_cols(col(0), col(1), col(2), col(3));
                if a.flags & aflag::LOCALIZED != 0 {
                    s.origin * m
                } else {
                    m
                }
            }
            None => Mat4::from_scale_rotation_translation(Vec3::splat(s.scale), s.rotation, s.position),
        }
    }

    /// The live objects with script ids in `lo..=hi` (`AIiObjectsFromScript`).
    fn objects_by_id(&self, a: u32, b: u32) -> Vec<usize> {
        let (lo, hi) = (a.min(b), a.max(b));
        (0..self.objects.len()).filter(|&k| (lo..=hi).contains(&self.object_setups[self.objects[k].setup].script_id)).collect()
    }

    /// `obj_create a b`: a new object for each setup in use with a script id
    /// in the range (even one already made).
    pub fn obj_create(&mut self, a: u32, b: u32) {
        let (lo, hi) = (a.min(b), a.max(b));
        for (i, s) in self.object_setups.iter().enumerate() {
            if s.flags & oflag::IN_USE == 0 || !(lo..=hi).contains(&s.script_id) {
                continue;
            }
            let anim = s.animation.as_deref().and_then(|n| self.object_anim(n));
            self.objects.push(Object { setup: i, anim, frame: 0, step: 1, stop: 0, animating: false, flags: s.flags });
        }
    }

    /// `obj_kill a b`: those objects are gone.
    pub fn obj_kill(&mut self, a: u32, b: u32) {
        let gone = self.objects_by_id(a, b);
        let mut k = 0;
        self.objects.retain(|_| {
            k += 1;
            !gone.contains(&(k - 1))
        });
    }

    /// `obj_show` / `obj_hide`: drawn and solid, or neither (`OBrShow`).
    pub fn obj_show(&mut self, a: u32, b: u32, show: bool) {
        for k in self.objects_by_id(a, b) {
            let o = &mut self.objects[k];
            if show {
                o.flags &= !oflag::INVISIBLE;
            } else {
                o.flags |= oflag::INVISIBLE;
            }
        }
    }

    /// `env_anim a b`: their clips start from the beginning. `_block`: while
    /// one plays on, it waits (as shipped, only when given a second id).
    /// Returns whether it waits.
    pub fn env_anim(&mut self, a: u32, b: Option<u32>, block: bool) -> bool {
        let found = self.objects_by_id(a, b.unwrap_or(a));
        if block && b.is_some_and(|b| b != 0) && found.iter().any(|&k| self.objects[k].playing()) {
            return true;
        }
        for k in found {
            self.objects[k].start();
        }
        false
    }

    /// `env_setanim id clip`: those objects take the named clip and start it
    /// (`OBrSetAnimationByName`). `_block` with one still playing: as
    /// shipped, nothing happens (it neither waits nor swaps). False if no
    /// such object or clip.
    pub fn env_setanim(&mut self, id: u32, clip: &str, block: bool) -> bool {
        let found = self.objects_by_id(id, id);
        if found.is_empty() {
            return false;
        }
        if block && found.iter().any(|&k| self.objects[k].playing()) {
            return true;
        }
        let Some(anim) = self.object_anim(clip) else { return false };
        for k in found {
            let o = &mut self.objects[k];
            o.anim = Some(anim.clone());
            o.start();
            o.stop = i32::from(anim.num_frames);
        }
        true
    }

    /// One tick of every object's clip.
    pub(super) fn objects_tick(&mut self) {
        for o in &mut self.objects {
            o.tick();
        }
    }

    /// The objects' collision spheres this tick (centre on its matrix's
    /// place, `OBiUpdate`): not hidden, not without collision.
    pub fn object_spheres(&self) -> Vec<(Vec3, f32)> {
        (0..self.objects.len())
            .filter(|&k| self.objects[k].flags & (oflag::INVISIBLE | oflag::NO_COLLISION) == 0)
            .map(|k| (self.object_matrix(k).w_axis.truncate(), self.object_setups[self.objects[k].setup].radius))
            .collect()
    }

    /// What to draw for the objects: each shown one's models and matrix —
    /// all but those see-through this frame ([`World::object_jello_draws`]).
    pub fn object_draws(&self) -> Vec<(PathBuf, Mat4)> {
        self.object_models(false)
    }

    /// The objects see-through this frame (the camera's jello: drawn at
    /// 31 of 255, `OBrDraw`).
    pub fn object_jello_draws(&self) -> Vec<(PathBuf, Mat4)> {
        self.object_models(true)
    }

    fn object_models(&self, see_through: bool) -> Vec<(PathBuf, Mat4)> {
        (0..self.objects.len())
            .filter(|&k| self.objects[k].flags & oflag::INVISIBLE == 0 && self.jello_objects.contains(&k) == see_through)
            .flat_map(|k| {
                let m = self.object_matrix(k);
                self.object_setups[self.objects[k].setup].models.iter().map(move |p| (p.clone(), m))
            })
            .collect()
    }

    /// The live objects: name, place, frame and whether its clip runs.
    pub fn object_states(&self) -> Vec<(String, Vec3, i32, bool)> {
        (0..self.objects.len())
            .map(|k| {
                let o = &self.objects[k];
                (self.object_setups[o.setup].name.clone(), self.object_matrix(k).w_axis.truncate(), o.frame, o.animating)
            })
            .collect()
    }
}
