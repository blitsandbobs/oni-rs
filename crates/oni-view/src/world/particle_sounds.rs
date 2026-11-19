//! The particles' sounds (`docs/AUDIO.md`, *Particles*): their impulse and
//! ambient sounds, their impact effects (bullets, shells, explosions on a
//! surface), and projectiles whizzing past the listener.

use glam::Vec3;

use super::World;
use crate::p3::{self, SoundEvent};

/// Projectiles followed for flybys at once.
const FLYBYS: usize = 24;

/// Where a projectile stepping `prev → at` passes closest to `ear`, if it
/// does so during this step.
fn flyby_point(prev: Vec3, at: Vec3, ear: Vec3) -> Option<Vec3> {
    let step = at - prev;
    let (t0, t1) = ((prev - ear).dot(step), (at - ear).dot(step));
    (t0 * t1 < 0.0).then(|| if (t1 - t0).abs() > 1e-6 { prev * (t1 / (t1 - t0)) + at * (-t0 / (t1 - t0)) } else { (prev + at) * 0.5 })
}

impl World {
    /// Plays what the particles asked for this tick, moves their ambient
    /// sounds with them, and checks the flybys.
    pub(super) fn particle_sounds(&mut self) {
        for ev in std::mem::take(&mut self.armory.particles.sounds) {
            match ev {
                SoundEvent::Impulse { name, at } => {
                    self.audio.impulse_play(&name, Some(at), None, self.sight.as_ref());
                }
                SoundEvent::AmbientStart { particle, name, at } => {
                    if let Some(a) = self.audio.ambient_start(&name, at, self.sight.as_ref()) {
                        self.particle_ambients.insert(particle, a);
                    }
                }
                SoundEvent::AmbientStop { particle } => {
                    if let Some(a) = self.particle_ambients.remove(&particle) {
                        self.audio.ambient_stop(a);
                    }
                }
                SoundEvent::Impact { impact, modifier, at, velocity, orientation, hit, owner, flags } => {
                    let Some(impacts) = self.impacts.clone() else { continue };
                    let Some(impact) = impacts.impact(&impact) else { continue };
                    let material = match hit {
                        Some(k) if k.env && k.id > 0 => impacts.material("Glass"),
                        Some(k) if k.env => k.triangle.and_then(|t| self.floor_materials.get(t as usize).copied().flatten()),
                        Some(k) => self.struck_material(k.id as usize, k.point),
                        None => None,
                    }
                    .or_else(|| impacts.material("Default"));
                    if let Some(material) = material {
                        use p3::effect::{Cause, Data};
                        let who = p3::owner::character(owner);
                        let mut data = Data::new(self.armory.particles.tick, at);
                        data.owner = owner;
                        data.parent_velocity = velocity;
                        data.parent_orientation = orientation;
                        data.parent_flags = flags;
                        // (A particle's impact lays decals at their class's
                        // size.)
                        data.decal_scale = [1.0, 1.0];
                        data.cause = match hit {
                            Some(k) if k.env => Cause::HitWall(self.struck_quad(k.point, k.normal)),
                            Some(_) => Cause::HitPhysics,
                            None if flags & p3::pflag::CUTSCENE_KILLED != 0 => Cause::CutsceneKilled,
                            None => Cause::None,
                        };
                        if let Some(k) = hit {
                            data.normal = k.normal;
                        }
                        self.impact_effect(impact, material, modifier as u8, data, 1.0, who, None);
                    }
                }
                SoundEvent::Flyby { particle } => {
                    if self.flybys.len() < FLYBYS {
                        if let Some(p) = self.armory.particles.get(particle) {
                            let at = self.armory.particles.world_position(p);
                            self.flybys.push((particle, at));
                        }
                    }
                }
            }
        }
        // Ambient sounds follow their particles (stopped if one went).
        let follow: Vec<(p3::Handle, usize)> = self.particle_ambients.iter().map(|(h, a)| (*h, *a)).collect();
        for (h, a) in follow {
            match self.armory.particles.get(h).map(|p| self.armory.particles.world_position(p)) {
                Some(at) => self.audio.ambient_move(a, at),
                None => {
                    self.particle_ambients.remove(&h);
                    self.audio.ambient_stop(a);
                }
            }
        }
        // A projectile crossing its closest approach to the listener this
        // tick whizzes there.
        let ear = self.audio.ear();
        let mut k = 0;
        while k < self.flybys.len() {
            let (h, prev) = self.flybys[k];
            let Some(p) = self.armory.particles.get(h) else {
                self.flybys.swap_remove(k);
                continue;
            };
            let at = self.armory.particles.world_position(p);
            let sound = self.armory.particles.classes.list[p.class].flyby.clone();
            self.flybys[k].1 = at;
            if let (Some(point), Some(s)) = (flyby_point(prev, at, ear), sound) {
                self.audio.impulse_play(&s, Some(point), None, self.sight.as_ref());
            }
            k += 1;
        }
    }

    /// The level quad a particle struck at `point` (facing `normal`): what
    /// objects collide with, just behind the point.
    pub(super) fn struck_quad(&self, point: Vec3, normal: Vec3) -> Option<u32> {
        struck_quad(&self.quads, point, normal)
    }

    /// The material a shot struck on fighter `who` at `point`: its shield's,
    /// else the closest body part's.
    fn struck_material(&self, who: usize, point: Vec3) -> Option<u16> {
        let impacts = self.impacts.as_ref()?;
        let f = self.fighters.get(who)?;
        if f.inventory.shield > 0 {
            return impacts.material("Shield");
        }
        let part = f
            .bones()
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.w_axis.truncate().distance_squared(point).total_cmp(&b.1.w_axis.truncate().distance_squared(point)))
            .map_or(0, |(k, _)| k);
        f.sounds.part_materials.get(part).cloned().flatten().and_then(|m| impacts.material(&m)).or_else(|| impacts.material("Character"))
    }
}

/// The quad just behind `point` along `-normal` (skipping what objects
/// pass through).
pub(super) fn struck_quad(quads: &crate::quads::Quads, point: Vec3, normal: Vec3) -> Option<u32> {
    quads.ray(point + normal * 0.5, normal * -1.0, crate::quads::flag::OBJ_COL_SKIP).map(|h| h.quad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flyby_sounds_where_the_shot_passes_closest() {
        let ear = Vec3::new(0.0, 0.0, 0.0);
        // Passing 5 units to the side, from z −10 to z +10: closest at z 0.
        let p = flyby_point(Vec3::new(5.0, 0.0, -10.0), Vec3::new(5.0, 0.0, 10.0), ear).unwrap();
        assert!((p - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4, "{p}");
        // Not yet there, or already past: nothing.
        assert!(flyby_point(Vec3::new(5.0, 0.0, -20.0), Vec3::new(5.0, 0.0, -10.0), ear).is_none());
        assert!(flyby_point(Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 20.0), ear).is_none());
    }
}
