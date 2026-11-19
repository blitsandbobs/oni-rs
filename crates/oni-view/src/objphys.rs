//! Physics for loose things — guns and powerups lying about or falling
//! (`BFW_Physics.c`, the linear level of `OBiPhysics_Update`; behaviour per
//! `docs/OBJECTS.md`, *Loose things*): a sphere pulled down by gravity,
//! moving its velocity a tick, reflected off the level's quads one plane a
//! pass (then halted if it still pushes into something), and slowed by
//! friction only once it has touched something.

use glam::Vec3;

use crate::quads::{flag, Quads};

/// Gravity, per tick per tick (`UUcGravity`).
pub const GRAVITY: f32 = 0.294;

/// A loose thing's physics context.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    /// Its sphere's centre.
    pub position: Vec3,
    pub velocity: Vec3,
    /// Pushes waiting for the next tick (`PHrPhysics_Accelerate`).
    pub acceleration: Vec3,
    pub radius: f32,
    pub static_friction: f32,
    pub dynamic_friction: f32,
}

/// A level surface it met: where, and the plane's normal (toward it).
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub quad: u32,
    pub point: Vec3,
    pub normal: Vec3,
}

impl Body {
    pub fn new(position: Vec3, radius: f32, static_friction: f32, dynamic_friction: f32) -> Body {
        Body { position, velocity: Vec3::ZERO, acceleration: Vec3::ZERO, radius, static_friction, dynamic_friction }
    }

    /// What the sphere meets moving along `v` (`PHrPhysics_Callback_FindEnvCollisions`,
    /// with the characters' skip flags as loose things use them; two-sided
    /// quads both ways), less what it moves away from or is already behind
    /// (`PHiRemoveBackfacingCollisions`).
    fn gather(&self, quads: &Quads, v: Vec3) -> Vec<Contact> {
        let mut out = Vec::new();
        for h in quads.sphere(self.position, self.radius, v, flag::CHR_COL_SKIP) {
            let q = &quads.quads[h.quad as usize];
            let (n, _) = quads.plane(q.plane);
            out.push(Contact { quad: h.quad, point: h.point, normal: n });
            if q.flags & flag::AK_2SIDED != 0 {
                out.push(Contact { quad: h.quad, point: h.point, normal: -n });
            }
        }
        out.retain(|c| !(v.dot(c.normal) > 0.0 || (c.point - self.position).dot(c.normal) > 0.0));
        out
    }

    /// One tick (`PHrPhysicsContext_Update` for this context): gravity and
    /// pending pushes, then a reflecting pass (nearest first: the first
    /// plane it pushes into takes that push off) — `on_pass` sees what it
    /// met — and a halt if it reflected, the move, and friction if it
    /// touched anything. Returns whether it touched anything.
    pub fn tick(&mut self, quads: &Quads, mut on_pass: impl FnMut(&Body, &[Contact])) -> bool {
        self.velocity.y -= GRAVITY;
        self.velocity += self.acceleration;
        self.acceleration = Vec3::ZERO;
        let mut v = self.velocity;
        // The reflecting pass (`OBrPhysics_Update_Reflect`).
        let mut met = self.gather(quads, v);
        met.sort_by(|a, b| a.point.distance_squared(self.position).total_cmp(&b.point.distance_squared(self.position)));
        let mut reflected = false;
        for c in &met {
            let d = v.dot(c.normal);
            if d < 0.0 {
                v -= c.normal * d;
                reflected = true;
                break;
            }
        }
        on_pass(self, &met);
        // The halt (`OBrPhysics_Update_ReflectHalt`): still pushing into
        // something, it stops.
        if reflected && self.gather(quads, v).iter().any(|c| v.dot(c.normal) < 0.0) {
            v = Vec3::ZERO;
        }
        self.position += v;
        self.velocity = v;
        // Friction (`PHrPhysics_Callback_Friction`), only having touched
        // something (no midair friction).
        if reflected {
            let len = self.velocity.length();
            if len < 1e-6 {
                self.velocity = Vec3::ZERO;
            } else {
                let to = (len - self.static_friction).max(0.0).min(len * self.dynamic_friction);
                self.velocity *= to / len;
            }
        }
        reflected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quads::tests::room;

    fn floor() -> [Vec3; 4] {
        [Vec3::new(-50.0, 0.0, -50.0), Vec3::new(-50.0, 0.0, 50.0), Vec3::new(50.0, 0.0, 50.0), Vec3::new(50.0, 0.0, -50.0)]
    }

    #[test]
    fn it_falls_lands_and_slides_to_rest() {
        let q = room(&[(floor(), 0)]);
        let mut b = Body::new(Vec3::new(0.0, 20.0, 0.0), 1.0, 0.0025, 0.75);
        b.velocity = Vec3::new(1.0, 0.0, 0.0);
        let mut landed = None;
        for t in 0..200 {
            let mut hit = false;
            b.tick(&q, |_, met| hit |= !met.is_empty());
            if hit && landed.is_none() {
                landed = Some(t);
            }
        }
        assert!(landed.is_some_and(|t| t < 15), "falls fast: {landed:?}");
        assert!((b.position.y - 1.0).abs() < 0.5, "resting on the floor: {}", b.position);
        assert!(b.velocity.length() < 1e-3, "and stopped: {}", b.velocity);
        assert!(b.position.x > 1.0 && b.position.x < 20.0, "having slid a little: {}", b.position);
    }
}
