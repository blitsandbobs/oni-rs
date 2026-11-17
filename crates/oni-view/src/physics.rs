//! Kinematic character physics (engine milestone **E3**), via Rapier.
//!
//! Matches Oni's model (see `ENGINE_NOTES.md` §3–4): the character is a capsule
//! moved by a velocity = **horizontal intent (from animation root motion) +
//! gravity**, resolved by **move-and-slide** against the level mesh. We use
//! Rapier's `KinematicCharacterController` (the same move-and-slide + ground
//! detection category as Oni's `OBrPhysics_Update_Reflect`), with the static
//! level as a trimesh collider. The character is controller-driven (not a
//! dynamic body), so no rigid-body step is needed — just per-tick `move_shape`.

use glam::Vec3;
use rapier3d::control::{CharacterLength, KinematicCharacterController};
use rapier3d::na::{Isometry3, Point3, Translation3, Vector3};
use rapier3d::prelude::*;

/// Gravity acceleration (world units per tick²) — placeholder; the faithful
/// value comes from the character's `ONCC` air constants later.
const GRAVITY: f32 = 0.9;
/// Terminal fall speed (units/tick).
const MAX_FALL: f32 = 20.0;
/// Small downward "stick" velocity while grounded, so ground contact keeps
/// being detected (the collision stops it, so it doesn't actually sink).
const GROUND_STICK: f32 = 1.0;

pub struct Physics {
    bodies: RigidBodySet,
    colliders: ColliderSet,
    query: QueryPipeline,
    controller: KinematicCharacterController,
    capsule: Capsule,
    half_height: f32,
    /// Character **feet** position in world space.
    pub feet: Vec3,
    /// Downward velocity (units/tick), 0 when grounded.
    vel_y: f32,
    pub grounded: bool,
}

impl Physics {
    /// Builds the physics world from the level's triangles and a character of
    /// the given total height / radius, with feet starting at `feet`.
    pub fn new(
        level_positions: &[[f32; 3]],
        level_indices: &[u32],
        feet: Vec3,
        height: f32,
        radius: f32,
    ) -> Physics {
        let mut colliders = ColliderSet::new();
        let bodies = RigidBodySet::new();

        let verts: Vec<Point3<f32>> = level_positions
            .iter()
            .map(|v| Point3::new(v[0], v[1], v[2]))
            .collect();
        let idx: Vec<[u32; 3]> = level_indices
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        colliders.insert(ColliderBuilder::trimesh(verts, idx).build());

        let mut query = QueryPipeline::new();
        query.update(&colliders);

        // Vertical capsule: the cylinder part spans the body, caps at the ends.
        let half_height = (height * 0.5 - radius).max(radius);
        let capsule = Capsule::new(
            Point3::new(0.0, -half_height, 0.0),
            Point3::new(0.0, half_height, 0.0),
            radius,
        );

        // Snap to ground within a small distance so the character sticks to
        // floors/steps instead of hopping off edges.
        let controller = KinematicCharacterController {
            up: Vector3::y_axis(),
            snap_to_ground: Some(CharacterLength::Absolute(0.5)),
            offset: CharacterLength::Absolute(0.05),
            ..KinematicCharacterController::default()
        };

        Physics {
            bodies,
            colliders,
            query,
            controller,
            capsule,
            half_height,
            feet,
            vel_y: 0.0,
            grounded: false,
        }
    }

    /// The capsule centre (feet + half the capsule span) as an isometry.
    fn capsule_iso(&self) -> Isometry3<f32> {
        let c = self.feet + Vec3::new(0.0, self.half_height + self.capsule.radius, 0.0);
        Isometry3::from(Translation3::new(c.x, c.y, c.z))
    }

    /// Advances one 60 Hz tick: `horizontal` is the desired in-plane move this
    /// tick (from animation root motion / input); gravity and collision are
    /// applied. Returns the new feet position.
    pub fn step(&mut self, horizontal: Vec3) -> Vec3 {
        // Vertical velocity: a small down-stick when grounded (keeps ground
        // contact detected), else gravity accelerates the fall.
        self.vel_y = if self.grounded {
            -GROUND_STICK
        } else {
            (self.vel_y - GRAVITY).max(-MAX_FALL)
        };

        let desired = Vector3::new(horizontal.x, self.vel_y, horizontal.z);
        let movement = self.controller.move_shape(
            1.0, // dt in ticks: velocities are already per-tick
            &self.bodies,
            &self.colliders,
            &self.query,
            &self.capsule,
            &self.capsule_iso(),
            desired,
            QueryFilter::default(),
            |_| {},
        );

        self.feet += Vec3::new(
            movement.translation.x,
            movement.translation.y,
            movement.translation.z,
        );
        // Grounded when the controller says so, or when a downward move was
        // blocked by collision (we wanted to fall but barely moved down).
        let wanted_down = self.vel_y < 0.0;
        let blocked_down = wanted_down && movement.translation.y > self.vel_y * 0.5;
        self.grounded = movement.grounded || blocked_down;
        self.feet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A large flat floor quad at y=0 (two triangles), no assets needed.
    fn floor() -> (Vec<[f32; 3]>, Vec<u32>) {
        (
            vec![
                [-50.0, 0.0, -50.0],
                [50.0, 0.0, -50.0],
                [50.0, 0.0, 50.0],
                [-50.0, 0.0, 50.0],
            ],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    /// The floor plus a vertical wall at x = 5 (spanning z and y).
    fn floor_and_wall() -> (Vec<[f32; 3]>, Vec<u32>) {
        let (mut v, mut i) = floor();
        let b = v.len() as u32;
        v.extend_from_slice(&[
            [5.0, 0.0, -50.0],
            [5.0, 10.0, -50.0],
            [5.0, 10.0, 50.0],
            [5.0, 0.0, 50.0],
        ]);
        i.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        (v, i)
    }

    #[test]
    fn drops_and_lands_grounded() {
        let (v, i) = floor();
        let mut p = Physics::new(&v, &i, Vec3::new(0.0, 20.0, 0.0), 4.0, 0.7);
        for _ in 0..200 {
            p.step(Vec3::ZERO);
        }
        assert!(p.grounded, "settles grounded after falling");
        assert!(
            p.feet.y.abs() < 0.3,
            "feet rest at floor level, got {}",
            p.feet.y
        );
    }

    #[test]
    fn walks_freely_on_open_floor() {
        let (v, i) = floor();
        let mut p = Physics::new(&v, &i, Vec3::ZERO, 4.0, 0.7);
        for _ in 0..10 {
            p.step(Vec3::ZERO); // settle
        }
        let x0 = p.feet.x;
        for _ in 0..30 {
            p.step(Vec3::new(0.3, 0.0, 0.0)); // move +x across open floor
        }
        assert!(p.feet.x - x0 > 5.0, "moved freely, dx = {}", p.feet.x - x0);
        assert!(p.feet.y.abs() < 0.3, "stays on the floor while moving");
    }

    #[test]
    fn wall_blocks_and_does_not_tunnel() {
        let (v, i) = floor_and_wall(); // wall at x = 5
        let mut p = Physics::new(&v, &i, Vec3::ZERO, 4.0, 0.7);
        for _ in 0..10 {
            p.step(Vec3::ZERO);
        }
        for _ in 0..60 {
            p.step(Vec3::new(0.5, 0.0, 0.0)); // push hard into the +x wall (30u desired)
        }
        assert!(
            p.feet.x > 3.0,
            "advanced toward the wall, got x = {}",
            p.feet.x
        );
        assert!(
            p.feet.x < 6.0,
            "stopped at the wall, no tunnelling, got x = {}",
            p.feet.x
        );
    }
}
