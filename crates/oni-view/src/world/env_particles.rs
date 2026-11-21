//! Environmental particles (`docs/PARTICLES.md`, *Environmental particles*):
//! the level's placed particle effects — floor targets, lights, steam, force
//! fields, creatures — from its `ENVP` arrays and particle objects
//! (`levels/NN/env_particles.json`). Each is created at the level's start
//! (unless flagged not to be) and scripts drive them by tag (`particle tag
//! create | kill | reset | do <event> | <event>`).

use std::path::Path;

use glam::{Mat3, Mat4, Vec3};
use serde_json::Value;

use super::weapons::ShotHost;
use super::World;
use crate::p3::{self, Handle};

/// The original's event names, in the particle system's event order.
const EVENT_NAMES: [&str; 16] = [
    "update",
    "pulse",
    "start",
    "stop",
    "bgfx_start",
    "bgfx_stop",
    "hit_wall",
    "hit_char",
    "lifetime",
    "explode",
    "brokenlink",
    "create",
    "die",
    "newattractor",
    "delay_start",
    "delay_stop",
];

/// How far behind a level decal its wall may be (`EPcDecal_FindWallDistance`).
const DECAL_FIND_WALL: f32 = 10.0;

pub(crate) struct EnvParticle {
    class: Option<p3::ClassId>,
    pub tag: String,
    matrix: Mat4,
    /// A decal's size (× its class's).
    decal_scale: [f32; 2],
    not_initially: bool,
    /// Its live particle, while created.
    handle: Option<Handle>,
    /// A placed object's (`EPcParticleOwner_Object`): the setup it belongs
    /// to and the object's matrix it rides (its own is in the object's
    /// space); None for the level's own.
    pub(crate) owner: Option<(usize, std::rc::Rc<std::cell::Cell<Mat4>>)>,
}

/// One particle of an `ENVP` array as `objects.json` / `env_particles.json`
/// write it: class, tag, matrix, decal scale, held back at the start.
pub(crate) fn parse(p: &Value, classes: &p3::Classes) -> EnvParticle {
    let f = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
    let m: Vec<f32> = p["matrix"].as_array().map(|a| a.iter().map(f).collect()).unwrap_or_default();
    let c = |k: usize| Vec3::new(m.get(k * 3).copied().unwrap_or(0.0), m.get(k * 3 + 1).copied().unwrap_or(0.0), m.get(k * 3 + 2).copied().unwrap_or(0.0));
    EnvParticle {
        class: p["class"].as_str().and_then(|n| classes.get(n)),
        tag: p["tag"].as_str().unwrap_or("").to_string(),
        matrix: Mat4::from_mat3_translation(Mat3::from_cols(c(0), c(1), c(2)), c(3)),
        decal_scale: [f(&p["decalScale"][0]), f(&p["decalScale"][1])],
        not_initially: p["notInitiallyCreated"].as_bool().unwrap_or(false),
        handle: None,
        owner: None,
    }
}

/// Loads a level's environmental particles (classes resolved against the
/// particle system's).
pub(crate) fn load(level_dir: &Path, classes: &p3::Classes) -> Vec<EnvParticle> {
    let Some(j) = std::fs::read_to_string(level_dir.join("env_particles.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return Vec::new();
    };
    j["particles"].as_array().into_iter().flatten().map(|p| parse(p, classes)).collect()
}

impl World {
    /// Creates environmental particle `k`'s particle (`EPrNewParticle`): at its
    /// place and orientation, still, owned by no one, its create event run.
    fn env_new(&mut self, k: usize) {
        let e = &self.env_particles[k];
        let Some(class) = e.class else { return };
        if self.armory.particles.class(class).has(p3::class::cflag::DECAL) {
            self.env_particles[k].handle = self.env_decal(k, class);
            return;
        }
        let m = e.matrix;
        let (tick, now) = (self.armory.particles.tick, self.now as u64);
        let time = f64::from(tick) / p3::HZ;
        let mut p = self.armory.particles.create(class, tick);
        p.position = m.w_axis.truncate();
        p.orientation = Mat3::from_mat4(m);
        p.velocity = Vec3::ZERO;
        p.offset = Vec3::ZERO;
        p.owner = 0;
        // (An object's rides its matrix, if the class takes one.)
        if let Some((_, cell)) = &e.owner {
            if self.armory.particles.class(class).has(p3::class::cflag::DYNAMIC_MATRIX) {
                p.matrix = Some(cell.clone());
            }
        }
        let super::weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        let h = particles.created(p, time, &mut host);
        self.env_particles[k].handle = h;
    }

    /// A decal instead (`EPrNewDecal`): on the wall found within 10 behind
    /// it (along its −Y), up along its Z, kept for good and never turned.
    fn env_decal(&mut self, k: usize, class: p3::ClassId) -> Option<Handle> {
        use crate::quads::flag;
        use p3::effect::{Cause, Data, Location, Orientation, Spec};
        let e = &self.env_particles[k];
        let (m, scale) = (e.matrix, e.decal_scale);
        let hit = self.quads.ray(m.w_axis.truncate(), m.y_axis.truncate() * -DECAL_FIND_WALL, flag::OBJ_COL_SKIP)?;
        let mut data = Data::new(self.armory.particles.tick, hit.point);
        data.cause = Cause::HitWall(Some(hit.quad));
        data.upvector = m.z_axis.truncate();
        data.decal_scale = scale;
        data.normal = self.quads.plane(self.quads.quads[hit.quad as usize].plane).0;
        let spec = Spec { class, orientation: Orientation::Normal, location: Location::Decal { is_static: true, random_rotation: false } };
        self.create_effect(&spec, &data)
    }

    /// A placed object's particles made (`OBrSetParticles`, `EPrArray_New`):
    /// each tagged after the object (`name_tag`), riding `cell`, created
    /// unless held back — for setup `setup`, unless they already are.
    pub(super) fn object_particles_new(&mut self, setup: usize, cell: std::rc::Rc<std::cell::Cell<Mat4>>) {
        if self.env_particles.iter().any(|e| e.owner.as_ref().is_some_and(|o| o.0 == setup)) {
            return;
        }
        let s = &self.object_setups[setup];
        let name = s.name.clone();
        let made: Vec<EnvParticle> = s.particles.iter().map(|v| parse(v, &self.armory.particles.classes)).collect();
        for mut e in made {
            e.tag = if name.is_empty() { e.tag } else { format!("{name}_{}", e.tag) };
            e.owner = Some((setup, cell.clone()));
            let held = e.not_initially;
            self.env_particles.push(e);
            if !held {
                self.env_new(self.env_particles.len() - 1);
            }
        }
    }

    /// A placed object's particles gone (`EPrArray_Delete`): hidden, killed.
    pub(super) fn object_particles_delete(&mut self, setup: usize) {
        let mine: Vec<usize> = (0..self.env_particles.len()).filter(|&k| self.env_particles[k].owner.as_ref().is_some_and(|o| o.0 == setup)).collect();
        for &k in &mine {
            self.env_kill(k);
        }
        let mut k = 0;
        self.env_particles.retain(|_| {
            k += 1;
            !mine.contains(&(k - 1))
        });
    }

    /// Kills environmental particle `k`'s particle, if it has one.
    fn env_kill(&mut self, k: usize) {
        if let Some(h) = self.env_particles[k].handle.take() {
            let now = self.now as u64;
            let super::weapons::Armory { particles, .. } = &mut self.armory;
            let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
            particles.kill(h, &mut host);
        }
    }

    /// A cutscene clearing the dangerous projectiles
    /// (`P3iRemoveDangerousProjectiles`, `p3_removedangerous`): every
    /// particle of a class marked to die on a cutscene is killed, of one
    /// marked to expire on a cutscene expires next tick — each marked as
    /// cut by the cutscene (its effects then behave accordingly); one
    /// already fading or chopped is left be.
    pub fn remove_dangerous_projectiles(&mut self) {
        use p3::class::cflag;
        use p3::pflag;
        let now = self.now as u64;
        let super::weapons::Armory { particles, .. } = &mut self.armory;
        let mut die = Vec::new();
        let mut expire = Vec::new();
        for (h, p) in particles.iter() {
            let c = particles.class(p.class);
            if p.flags & (pflag::FADE_OUT | pflag::CHOP) != 0 {
                continue;
            }
            if c.has(cflag::DIE_ON_CUTSCENE) {
                die.push(h);
            } else if c.has(cflag::EXPIRE_ON_CUTSCENE) {
                expire.push(h);
            }
        }
        for &h in die.iter().chain(&expire) {
            if let Some(p) = particles.get_mut(h) {
                p.flags |= pflag::CUTSCENE_KILLED;
            }
        }
        for h in expire {
            if let Some(p) = particles.get_mut(h) {
                p.lifetime = 0.01;
            }
        }
        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        for h in die {
            particles.kill(h, &mut host);
        }
    }

    /// The level's start: every environmental particle not held back is
    /// created.
    pub(super) fn env_particles_start(&mut self) {
        for k in 0..self.env_particles.len() {
            let e = &self.env_particles[k];
            // (Objects make their own as they are made.)
            if !e.not_initially && e.owner.is_none() && e.handle.is_none() {
                self.env_new(k);
            }
        }
    }

    /// Scripts' `particle tag command [event]` to every environmental
    /// particle tagged `tag`: `create`, `kill`, `reset` (killed, and made
    /// again unless held back), `do <event>` or `<event>` (sent to it).
    /// False: an unknown command.
    pub fn env_particle_command(&mut self, tag: &str, command: &str, event: Option<&str>) -> bool {
        let event_of = |n: &str| EVENT_NAMES.iter().position(|e| *e == n);
        let ev = match command {
            "create" | "kill" | "reset" => None,
            "do" => match event.and_then(event_of) {
                Some(e) => Some(e),
                None => return false,
            },
            other => match event_of(other) {
                Some(e) => Some(e),
                None => return false,
            },
        };
        let picked: Vec<usize> = (0..self.env_particles.len()).filter(|&k| self.env_particles[k].tag == tag).collect();
        for k in picked {
            match (command, ev) {
                ("create", _) => self.env_new(k),
                ("kill", _) => self.env_kill(k),
                ("reset", _) => {
                    self.env_kill(k);
                    if !self.env_particles[k].not_initially {
                        self.env_new(k);
                    }
                }
                (_, Some(e)) => {
                    let Some(h) = self.env_particles[k].handle else { continue };
                    if self.armory.particles.get(h).is_none() {
                        self.env_particles[k].handle = None;
                        continue;
                    }
                    let now = self.now as u64;
                    let time = f64::from(self.armory.particles.tick) / p3::HZ;
                    let super::weapons::Armory { particles, .. } = &mut self.armory;
                    let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
                    particles.send(h, e, time, &mut host);
                }
                _ => {}
            }
        }
        true
    }

    /// How many environmental particles tagged `tag` are alive.
    pub fn env_particles_alive(&self, tag: &str) -> usize {
        self.env_particles.iter().filter(|e| e.tag == tag && e.handle.is_some_and(|h| self.armory.particles.get(h).is_some())).count()
    }
}
