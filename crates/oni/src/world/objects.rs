//! The level's placed objects (data
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
pub(crate) mod oflag {
    pub const FORCE_DRAW: u32 = 0x0008;
    pub const INVISIBLE: u32 = 0x0100;
    pub const IN_USE: u32 = 0x0200;
    pub const NO_COLLISION: u32 = 0x0400;
}
/// `OBcAnimFlags_*`.
mod aflag {
    pub const LOOP: u32 = 0x1;
    pub const PINGPONG: u32 = 0x2;
    pub const AUTOSTART: u32 = 0x8;
}

/// The level's placed objects: as authored, those scripts have made, and
/// the object animations scripts name.
#[derive(Default)]
pub struct Placed {
    pub setups: Vec<Setup>,
    pub list: Vec<Object>,
    /// The object animations scripts name (`OBAN`, the level's and the
    /// common ones), by name.
    pub anims: std::collections::HashMap<String, Rc<oni_geom::objanim::ObjectAnimation>>,
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
    /// Its attached particles (`particleArray`), as `objects.json` has them.
    pub particles: Vec<oni_schema::files::EnvParticle>,
    /// Its collision sphere's radius (as shipped: spec quirk 16, kept in
    /// both modes — the "largest coordinate" the code meant would wall off
    /// whole aisles round a truck).
    pub radius: f32,
    /// Its first geometry's texture (what a particle striking it is made
    /// of, `P3iPhysicsGetMaterialType`).
    pub texture: Option<String>,
}

/// A model's first texture, by name (the glTF's first image).
fn first_texture(path: &std::path::Path) -> Option<String> {
    let b = std::fs::read(path).ok()?;
    let n = u32::from_le_bytes(b.get(12..16)?.try_into().ok()?) as usize;
    let j: serde_json::Value = serde_json::from_slice(b.get(20..20 + n)?).ok()?;
    let uri = j["images"][0]["uri"].as_str()?;
    Some(
        std::path::Path::new(uri)
            .file_stem()?
            .to_string_lossy()
            .into_owned(),
    )
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
    /// Lit flat in this colour (`obj_shade`).
    pub shade: Option<[f32; 3]>,
    /// Its matrix as its particles ride it (kept up each tick).
    pub cell: std::rc::Rc<std::cell::Cell<Mat4>>,
}

/// Loads a level's object setups (`objects.json`).
pub fn load(level_dir: &Path) -> Vec<Setup> {
    let doc: oni_schema::files::ObjectsFile =
        crate::json::read_file(level_dir.join("objects.json"));
    let common = level_dir.join("../../common");
    doc.objects
        .iter()
        .map(|o| {
            let flags = o
                .flags
                .iter()
                .filter_map(|n| {
                    oni_schema::OBJECT_FLAGS
                        .iter()
                        .position(|k| !k.is_empty() && k == n)
                })
                .fold(0, |acc, b| acc | (1 << b));
            let [x, y, z, w] = o.orientation;
            Setup {
                name: o.name.clone(),
                script_id: o.script_id,
                flags,
                position: Vec3::from(o.position),
                // (Oni's quaternion-to-matrix is the transpose of the usual.)
                rotation: Quat::from_xyzw(x, y, z, w).normalize().conjugate(),
                scale: o.scale,
                origin: crate::geom::mat4_4x3(&o.origin),
                animation: o.animation.clone(),
                models: o.geometry.iter().map(|g| common.join(g)).collect(),
                radius: o.radius.shipped.max(1.0),
                particles: o.particles.clone(),
                texture: o
                    .geometry
                    .first()
                    .and_then(|g| first_texture(&common.join(g))),
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
        self.animating
            && self
                .anim
                .as_ref()
                .is_some_and(|a| self.frame + 1 < i32::from(a.num_frames))
    }
}

impl World {
    /// Object `k`'s matrix (`PHrPhysics_MaintainMatrix`): its clip's (from
    /// its origin, for a localized one), else its placement.
    pub fn object_matrix(&self, k: usize) -> Mat4 {
        let o = &self.placed.list[k];
        let s = &self.placed.setups[o.setup];
        match &o.anim {
            Some(a) => crate::geom::placed_clip_matrix(a, o.frame, s.origin),
            None => {
                Mat4::from_scale_rotation_translation(Vec3::splat(s.scale), s.rotation, s.position)
            }
        }
    }

    /// The live objects with script ids in `lo..=hi` (`AIiObjectsFromScript`).
    fn objects_by_id(&self, a: u32, b: u32) -> Vec<usize> {
        let (lo, hi) = (a.min(b), a.max(b));
        (0..self.placed.list.len())
            .filter(|&k| {
                (lo..=hi).contains(&self.placed.setups[self.placed.list[k].setup].script_id)
            })
            .collect()
    }

    /// `obj_create a b`: a new object for each setup in use with a script id
    /// in the range (even one already made).
    pub fn obj_create(&mut self, a: u32, b: u32) {
        let (lo, hi) = (a.min(b), a.max(b));
        for i in 0..self.placed.setups.len() {
            let s = &self.placed.setups[i];
            if s.flags & oflag::IN_USE == 0 || !(lo..=hi).contains(&s.script_id) {
                continue;
            }
            let flags = s.flags;
            let anim = s.animation.clone().and_then(|n| self.object_anim(&n));
            let cell = std::rc::Rc::new(std::cell::Cell::new(Mat4::IDENTITY));
            self.placed.list.push(Object {
                setup: i,
                anim,
                frame: 0,
                step: 1,
                stop: 0,
                animating: false,
                flags,
                shade: None,
                cell: cell.clone(),
            });
            let k = self.placed.list.len() - 1;
            cell.set(self.object_matrix(k));
            // Its particles (`OBrSetParticles`), unless hidden.
            if flags & oflag::INVISIBLE == 0 {
                self.object_particles_new(i, cell);
            }
        }
    }

    /// `obj_kill a b`: those objects are gone.
    pub fn obj_kill(&mut self, a: u32, b: u32) {
        let gone = self.objects_by_id(a, b);
        for &k in &gone {
            self.object_particles_delete(self.placed.list[k].setup);
        }
        let mut k = 0;
        self.placed.list.retain(|_| {
            k += 1;
            !gone.contains(&(k - 1))
        });
        // (The stamps name objects by index, which has just shifted.)
        if !gone.is_empty() {
            self.obstructions.clear();
        }
    }

    /// `obj_show` / `obj_hide`: drawn and solid, or neither (`OBrShow`).
    pub fn obj_show(&mut self, a: u32, b: u32, show: bool) {
        for k in self.objects_by_id(a, b) {
            let o = &mut self.placed.list[k];
            if show {
                o.flags &= !oflag::INVISIBLE;
            } else {
                o.flags |= oflag::INVISIBLE;
            }
            // Its particles come and go with it (`OBrSetParticles`).
            let (setup, cell) = (o.setup, o.cell.clone());
            if show {
                self.object_particles_new(setup, cell);
            } else {
                self.object_particles_delete(setup);
            }
        }
    }

    /// `env_anim a b`: their clips start from the beginning. `_block`: while
    /// one plays on, it waits (as shipped, only when given a second id).
    /// Returns whether it waits.
    pub fn env_anim(&mut self, a: u32, b: Option<u32>, block: bool) -> bool {
        let found = self.objects_by_id(a, b.unwrap_or(a));
        if block
            && b.is_some_and(|b| b != 0)
            && found.iter().any(|&k| self.placed.list[k].playing())
        {
            return true;
        }
        for k in found {
            self.placed.list[k].start();
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
        if block && found.iter().any(|&k| self.placed.list[k].playing()) {
            return true;
        }
        let Some(anim) = self.object_anim(clip) else {
            return false;
        };
        for k in found {
            let o = &mut self.placed.list[k];
            o.anim = Some(anim.clone());
            o.start();
            o.stop = i32::from(anim.num_frames);
        }
        true
    }

    /// One tick of every object's clip.
    pub(super) fn objects_tick(&mut self) {
        for o in &mut self.placed.list {
            o.tick();
        }
        for k in 0..self.placed.list.len() {
            self.placed.list[k].cell.set(self.object_matrix(k));
        }
    }

    /// The objects' collision spheres this tick (centre on its matrix's
    /// place, `OBiUpdate`): not hidden, not without collision.
    /// The setup of the `n`th object in [`World::object_spheres`].
    pub(crate) fn object_spheres_index(&self, n: usize) -> Option<usize> {
        (0..self.placed.list.len())
            .filter(|&k| self.placed.list[k].flags & (oflag::INVISIBLE | oflag::NO_COLLISION) == 0)
            .nth(n)
            .map(|k| self.placed.list[k].setup)
    }

    pub fn object_spheres(&self) -> Vec<(Vec3, f32)> {
        (0..self.placed.list.len())
            .filter(|&k| self.placed.list[k].flags & (oflag::INVISIBLE | oflag::NO_COLLISION) == 0)
            .map(|k| {
                (
                    self.object_matrix(k).w_axis.truncate(),
                    self.placed.setups[self.placed.list[k].setup].radius,
                )
            })
            .collect()
    }

    /// `obj_shade a b r g b`: objects `a`–`b` lit flat in this colour (each
    /// 0–1, kept to 8 bits as the original keeps it).
    pub fn obj_shade(&mut self, a: u32, b: u32, rgb: [f32; 3]) {
        let shade = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() / 255.0);
        for k in self.objects_by_id(a, b) {
            self.placed.list[k].shade = Some(shade);
        }
    }

    /// `obj_force_draw a [b]`: drawn even out of view (`OBcFlags_ForceDraw`)
    /// — every object is already (the port doesn't cull them), so it is
    /// only kept.
    pub fn obj_force_draw(&mut self, a: u32, b: u32) {
        for k in self.objects_by_id(a, b) {
            self.placed.list[k].flags |= oflag::FORCE_DRAW;
        }
    }

    /// The live objects: name, place, frame and whether its clip runs.
    pub fn object_states(&self) -> Vec<(String, Vec3, i32, bool)> {
        (0..self.placed.list.len())
            .map(|k| {
                let o = &self.placed.list[k];
                (
                    self.placed.setups[o.setup].name.clone(),
                    self.object_matrix(k).w_axis.truncate(),
                    o.frame,
                    o.animating,
                )
            })
            .collect()
    }
}
