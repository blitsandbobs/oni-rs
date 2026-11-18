//! Character collision against the level (engine milestone **E3**), via Rapier.
//!
//! This layer only *resolves* motion: the character model decides how far to
//! move each tick (animation root motion on the ground; its own air velocity
//! and gravity when airborne — see `fighter.rs`), and `slide` moves the capsule
//! by that amount, sliding along walls and floors (Rapier's
//! `KinematicCharacterController`, the same move-and-slide category as Oni's
//! physics). `ground_below` answers how far the floor is under the feet, which
//! the character uses to stick to slopes and to notice walking off a ledge.

use glam::Vec3;
use rapier3d::control::{CharacterLength, KinematicCharacterController};
use rapier3d::na::{Isometry3, Point3, Translation3, Vector3};
use rapier3d::prelude::*;

pub struct Physics {
    bodies: RigidBodySet,
    colliders: ColliderSet,
    query: QueryPipeline,
    controller: KinematicCharacterController,
    capsule: Capsule,
    half_height: f32,
    /// Character **feet** position in world space.
    pub feet: Vec3,
    /// Whether the last `slide` ended standing on something.
    pub grounded: bool,
}

impl Physics {
    /// Builds the collision world from the level's triangles and a character of
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

        // No snap-to-ground: the character model supplies its own downward
        // stick each grounded tick (so walking off a ledge is detectable).
        let controller = KinematicCharacterController {
            up: Vector3::y_axis(),
            snap_to_ground: None,
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
            grounded: false,
        }
    }

    /// The capsule centre (feet + half the capsule span) as an isometry.
    fn capsule_iso(&self) -> Isometry3<f32> {
        let c = self.feet + Vec3::new(0.0, self.half_height + self.capsule.radius, 0.0);
        Isometry3::from(Translation3::new(c.x, c.y, c.z))
    }

    /// Moves the character by `delta` (world units this tick, vertical
    /// included), sliding along the level. Returns whether it ended grounded:
    /// standing on a floor, or a downward move that the floor stopped.
    pub fn slide(&mut self, delta: Vec3) -> bool {
        let movement = self.controller.move_shape(
            1.0, // dt in ticks: the delta is already per-tick
            &self.bodies,
            &self.colliders,
            &self.query,
            &self.capsule,
            &self.capsule_iso(),
            Vector3::new(delta.x, delta.y, delta.z),
            QueryFilter::default(),
            |_| {},
        );
        self.feet += Vec3::new(
            movement.translation.x,
            movement.translation.y,
            movement.translation.z,
        );
        let blocked_down = delta.y < 0.0 && movement.translation.y > delta.y * 0.5;
        self.grounded = movement.grounded || blocked_down;
        self.grounded
    }

    /// Distance from the feet straight down to the level, if within `max`.
    pub fn ground_below(&self, max: f32) -> Option<f32> {
        // Start slightly above the feet so a floor we are standing on counts.
        let lift = 0.5;
        let origin = Point3::new(self.feet.x, self.feet.y + lift, self.feet.z);
        let ray = Ray::new(origin, Vector3::new(0.0, -1.0, 0.0));
        self.query
            .cast_ray(
                &self.bodies,
                &self.colliders,
                &ray,
                max + lift,
                true,
                QueryFilter::default(),
            )
            .map(|(_, toi)| (toi - lift).max(0.0))
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

    /// Falls under a simple per-tick gravity until grounded.
    fn settle(p: &mut Physics) {
        let mut vy = 0.0f32;
        for _ in 0..300 {
            vy = (vy - 0.06).max(-4.0);
            if p.slide(Vec3::new(0.0, vy, 0.0)) {
                vy = 0.0;
            }
        }
    }

    #[test]
    fn drops_and_lands_grounded() {
        let (v, i) = floor();
        let mut p = Physics::new(&v, &i, Vec3::new(0.0, 20.0, 0.0), 4.0, 0.7);
        settle(&mut p);
        assert!(p.grounded, "settles grounded after falling");
        assert!(p.feet.y.abs() < 0.3, "feet rest at floor level, got {}", p.feet.y);
    }

    #[test]
    fn walks_freely_on_open_floor() {
        let (v, i) = floor();
        let mut p = Physics::new(&v, &i, Vec3::ZERO, 4.0, 0.7);
        settle(&mut p);
        let x0 = p.feet.x;
        for _ in 0..30 {
            p.slide(Vec3::new(0.3, -0.5, 0.0)); // move +x with a ground stick
        }
        assert!(p.feet.x - x0 > 5.0, "moved freely, dx = {}", p.feet.x - x0);
        assert!(p.feet.y.abs() < 0.3, "stays on the floor while moving");
        assert!(p.grounded);
    }

    #[test]
    fn upward_move_leaves_the_ground() {
        let (v, i) = floor();
        let mut p = Physics::new(&v, &i, Vec3::ZERO, 4.0, 0.7);
        settle(&mut p);
        assert!(!p.slide(Vec3::new(0.0, 1.0, 0.0)), "moving up is not grounded");
        assert!(p.feet.y > 0.5, "rose off the floor");
        let below = p.ground_below(4.0).expect("floor within reach");
        assert!((below - p.feet.y).abs() < 0.2, "ground distance {below} ≈ height {}", p.feet.y);
    }

    #[test]
    fn wall_blocks_and_does_not_tunnel() {
        let (v, i) = floor_and_wall(); // wall at x = 5
        let mut p = Physics::new(&v, &i, Vec3::ZERO, 4.0, 0.7);
        settle(&mut p);
        for _ in 0..60 {
            p.slide(Vec3::new(0.5, -0.5, 0.0)); // push hard into the +x wall
        }
        assert!(p.feet.x > 3.0, "advanced toward the wall, got x = {}", p.feet.x);
        assert!(p.feet.x < 6.0, "stopped at the wall, no tunnelling, got x = {}", p.feet.x);
    }
}
