//! The particles animations put on the body (`docs/PARTICLES.md`,
//! *Animation particles*; `ONrCharacter_NewAnimationHook`,
//! `_OldAnimationHook`, `ONrCharacter_Update`): each clip names some of its
//! class's particles, each on a body part, active over a run of its frames
//! — punch and kick trails, dust, glows, the super ninja's fireball, Mutant
//! Muro's thunderbolt.

use glam::{Mat3, Vec3};

use super::World;
use crate::p3::{effect, event, owner, Handle};

/// Where a class particle goes (`override_partindex`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassPart {
    /// On this body part, whatever the animation says (the super ones).
    Part(usize),
    /// On the part the animation names.
    Animation,
    /// A hit flash: started as the others, but on a thrown character who
    /// is dead it makes a "killed" impact instead
    /// (`ONcCharacterParticle_HitFlash`).
    HitFlash,
}

/// One of a class's particles (`ONtCharacterParticle`).
#[derive(Clone, Debug)]
pub struct ClassParticle {
    pub name: String,
    pub class: String,
    pub part: ClassPart,
}

/// An animation's particle, made for the clip playing
/// (`ONtCharacterParticleInstance`).
#[derive(Clone, Debug)]
pub struct AnimParticle {
    /// The class particle (none: not found — it keeps its place in the
    /// clip's list).
    pub def: Option<usize>,
    pub part: usize,
    pub handle: Option<Handle>,
}

/// A fighter's animation particles: those of the clip it plays, which of
/// them are on, and which clip they were made for.
#[derive(Clone, Debug, Default)]
pub struct AnimParticles {
    pub list: Vec<AnimParticle>,
    pub active: u16,
    pub made_for: Option<u32>,
}

impl World {
    /// One tick of every fighter's animation particles.
    pub(super) fn anim_particles_tick(&mut self) {
        for who in 0..self.fighters.len() {
            self.anim_particles_of(who);
        }
    }

    fn anim_particles_of(&mut self, who: usize) {
        let started = self.fighters[who].motion.started;
        let gone = self.fighters[who].gone;
        // A new clip: the last one's go (`_OldAnimationHook`), this one's
        // are made (`_NewAnimationHook`).
        if gone || self.fighters[who].anim_particles.made_for != Some(started) {
            self.anim_particles_clear(who);
            if gone {
                return;
            }
            self.anim_particles_make(who);
            self.fighters[who].anim_particles.made_for = Some(started);
        }
        if self.fighters[who].anim_particles.list.is_empty() {
            return;
        }
        // Which are on at this frame.
        let f = &self.fighters[who];
        let frame = f.motion.frame;
        let now_active = f
            .motion
            .current()
            .particles
            .iter()
            .enumerate()
            .take(16)
            .filter(|(_, p)| frame >= p.start && frame <= p.stop)
            .fold(0u16, |m, (k, _)| m | (1 << k));
        let was = f.anim_particles.active;
        if now_active == was {
            return;
        }
        let tint = self.anim_tint(who);
        let t = f64::from(self.armory.particles.tick) / 60.0;
        let list = self.fighters[who].anim_particles.list.clone();
        for (k, p) in list.iter().enumerate() {
            let Some(def) = p.def else { continue };
            let bit = 1u16 << k;
            let starting = now_active & bit != 0 && was & bit == 0;
            // A hit flash starting on a thrown character who is dead: its
            // "killed" flash instead, once for that character.
            if starting && self.fighters[who].class_particles[def].part == ClassPart::HitFlash && self.killed_flash(who, k, p) {
                continue;
            }
            let Some(h) = p.handle.filter(|&h| self.armory.particles.alive(h)) else { continue };
            self.armory.particles.set_tint(h, tint);
            if starting {
                self.send_particle(h, event::START, t);
            } else if now_active & bit == 0 && was & bit != 0 {
                self.send_particle(h, event::STOP, t);
            }
        }
        self.fighters[who].anim_particles.active = now_active;
    }

    /// The colour the particles take (`ONiCharacter_GetAnimParticleTint`):
    /// the health of whom it throws, else its own.
    fn anim_tint(&self, who: usize) -> u32 {
        let f = &self.fighters[who];
        let c = f.throwing.and_then(|t| self.fighters.get(t)).unwrap_or(f);
        self.health_shade(c.hp, c.max_hp)
    }

    /// Makes the current clip's particles, each attached to its body part.
    fn anim_particles_make(&mut self, who: usize) {
        use effect::{Data, Location, Orientation, Spec};
        let wanted = self.fighters[who].motion.current().particles.clone();
        if wanted.is_empty() {
            return;
        }
        let tint = self.anim_tint(who);
        self.armory.particles.global_tint = Some(tint);
        let mut list = Vec::new();
        for ap in wanted.iter().take(16) {
            let f = &self.fighters[who];
            let def = f.class_particles.iter().position(|c| c.name == ap.name);
            let part = usize::from(ap.part);
            let mut handle = None;
            match def {
                None => eprintln!("anim particles: fighter {who}: no particle '{}' for {}", ap.name, f.motion.name()),
                Some(d) => {
                    let class_name = f.class_particles[d].class.clone();
                    let centre = f.character.parts.iter().find(|p| p.bone == part).map(|p| (p.bound_box.0 + p.bound_box.1) / 2.0);
                    if let (Some(class), Some(attach)) = (self.armory.particles.classes.get(&class_name), self.fighters[who].part_matrix(part)) {
                        let spec = Spec { class, orientation: Orientation::Unchanged, location: Location::Default };
                        let mut data = Data::new(self.armory.particles.tick, centre.unwrap_or(Vec3::ZERO));
                        data.owner = owner::CHARACTER_MELEE | who as u32;
                        data.parent_orientation = Some(Mat3::IDENTITY);
                        data.attach = Some(attach);
                        handle = self.create_effect(&spec, &data);
                    }
                }
            }
            list.push(AnimParticle { def, part, handle });
        }
        self.armory.particles.global_tint = None;
        let ap = &mut self.fighters[who].anim_particles;
        ap.list = list;
        ap.active = 0;
    }

    /// The last clip's particles stopped (if on) and killed.
    fn anim_particles_clear(&mut self, who: usize) {
        let ap = std::mem::take(&mut self.fighters[who].anim_particles);
        let t = f64::from(self.armory.particles.tick) / 60.0;
        for (k, p) in ap.list.iter().enumerate() {
            let Some(h) = p.handle.filter(|&h| self.armory.particles.alive(h)) else { continue };
            if ap.active & (1 << k) != 0 {
                self.send_particle(h, event::STOP, t);
            }
            self.kill_particle(h);
        }
    }

    /// The "killed" flash of a hit-flash particle (`Self_Damage_Killed`) on
    /// a dead thrown character, in its place; true if it took the place.
    fn killed_flash(&mut self, who: usize, k: usize, p: &AnimParticle) -> bool {
        let f = &self.fighters[who];
        let (thrown, other) = match (f.throwing, f.thrown_by) {
            (Some(t), _) => (t, who),
            (None, Some(by)) if f.motion.current().has_flag(crate::anim::types::flag::THROW_TARGET) => (who, by),
            _ => return false,
        };
        if self.fighters.get(thrown).is_none_or(|t| t.alive() && !t.motion.dying) {
            return false;
        }
        if let Some(h) = p.handle {
            self.kill_particle(h);
        }
        self.fighters[who].anim_particles.list[k].handle = None;
        if self.fighters[thrown].final_flash {
            return true;
        }
        let Some(impacts) = self.impacts.clone() else { return true };
        let f = &self.fighters[who];
        let material = f.sounds.part_materials.get(p.part).cloned().flatten().and_then(|m| impacts.material(&m)).or_else(|| impacts.material("Character"));
        let (Some(impact), Some(material)) = (impacts.impact("Self_Damage_Killed"), material) else { return true };
        let at = f.bones().get(p.part).map_or(f.feet(), |m| m.w_axis.truncate());
        let facing = Vec3::new(f.facing.facing.sin(), 0.0, f.facing.facing.cos());
        let mut data = effect::Data::new(self.armory.particles.tick, at);
        data.cause = effect::Cause::MeleeHit;
        data.owner = owner::CHARACTER_MELEE | who as u32;
        data.normal = facing;
        data.parent_velocity = Some(facing);
        self.impact_effect(impact, material, crate::impacts::MEDIUM, data, 1.0, Some(thrown), Some(other));
        self.fighters[thrown].final_flash = true;
        true
    }

    /// A blow that landed: the attacker's animation particles on the parts
    /// that struck get a pulse (`ONiCharacter_HandleHit…`).
    pub(super) fn anim_particles_pulse(&mut self, who: usize, parts: u32) {
        let ap = &self.fighters[who].anim_particles;
        let pulsed: Vec<Handle> = ap
            .list
            .iter()
            .enumerate()
            .filter(|(k, p)| ap.active & (1 << k) != 0 && parts & (1 << p.part) != 0)
            .filter_map(|(_, p)| p.handle)
            .filter(|&h| self.armory.particles.alive(h))
            .collect();
        let t = f64::from(self.armory.particles.tick) / 60.0;
        for h in pulsed {
            self.send_particle(h, event::PULSE, t);
        }
    }

    fn send_particle(&mut self, h: Handle, ev: usize, t: f64) {
        let now = self.now as u64;
        let super::weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = super::weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        particles.send(h, ev, t, &mut host);
    }

    fn kill_particle(&mut self, h: Handle) {
        let now = self.now as u64;
        let super::weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = super::weapons::ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        particles.kill(h, &mut host);
    }
}

/// A class's particles from its `classes.json` entry.
pub fn class_particles(c: &serde_json::Value) -> Vec<ClassParticle> {
    c["particles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let part = match &p["part"] {
                serde_json::Value::String(s) if s == "hitFlash" => ClassPart::HitFlash,
                serde_json::Value::Number(n) => ClassPart::Part(n.as_u64()? as usize),
                _ => ClassPart::Animation,
            };
            Some(ClassParticle { name: p["name"].as_str()?.to_string(), class: p["class"].as_str()?.to_string(), part })
        })
        .collect()
}
