//! Line of sight through the level (Rapier), and what looking ahead along
//! the floor finds. (Characters move against the level by the original's
//! rules: `chrphys.rs`.)

use glam::Vec3;
use rapier3d::na::{Point3, Vector3};
use rapier3d::prelude::*;

/// What lies ahead along the floor (see [`crate::chrphys::probe`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// Distance the body could move before a wall.
    pub free: f32,
    /// Distance at which the floor drops away, if within `free`.
    pub ledge: Option<f32>,
}

/// Line of sight through the level: the opaque geometry only (glass and
/// other transparent surfaces don't block sight, as in the original).
pub struct Sight {
    bodies: RigidBodySet,
    colliders: ColliderSet,
    query: QueryPipeline,
    /// Doors' closed leaves (opaque only while closed).
    doors: Vec<Option<ColliderHandle>>,
}

impl Sight {
    pub fn new(positions: &[[f32; 3]], indices: &[u32]) -> Sight {
        let mut colliders = ColliderSet::new();
        let verts: Vec<Point3<f32>> = positions.iter().map(|v| Point3::new(v[0], v[1], v[2])).collect();
        let idx: Vec<[u32; 3]> = indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        if !idx.is_empty() {
            colliders.insert(ColliderBuilder::trimesh(verts, idx).build());
        }
        let mut query = QueryPipeline::new();
        query.update(&colliders);
        Sight { bodies: RigidBodySet::new(), colliders, query, doors: Vec::new() }
    }

    /// Adds a door's closed leaf (opaque to begin with).
    pub fn add_door(&mut self, positions: &[[f32; 3]], indices: &[u32]) {
        let verts: Vec<Point3<f32>> = positions.iter().map(|v| Point3::new(v[0], v[1], v[2])).collect();
        let idx: Vec<[u32; 3]> = indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
        let h = (!idx.is_empty()).then(|| self.colliders.insert(ColliderBuilder::trimesh(verts, idx).build()));
        self.query.update(&self.colliders);
        self.doors.push(h);
    }

    /// Makes door `k` opaque (closed) or not.
    pub fn set_door(&mut self, k: usize, closed: bool) {
        if let Some(c) = self.doors.get(k).copied().flatten().and_then(|h| self.colliders.get_mut(h)) {
            if c.is_enabled() != closed {
                c.set_enabled(closed);
                self.query.update(&self.colliders);
            }
        }
    }

    /// How many opaque surfaces (at most `max`) lie between `from` and `to`.
    pub fn hits(&self, from: Vec3, to: Vec3, max: usize) -> usize {
        let d = to - from;
        let len = d.length();
        if len < 1e-3 {
            return 0;
        }
        let dir = d / len;
        let (mut at, mut n) = (0.0, 0);
        while n < max {
            let o = from + dir * at;
            let ray = Ray::new(Point3::new(o.x, o.y, o.z), Vector3::new(dir.x, dir.y, dir.z));
            match self.query.cast_ray(&self.bodies, &self.colliders, &ray, len - at, true, QueryFilter::default()) {
                Some((_, t)) => {
                    n += 1;
                    at += t + 0.1;
                    if at >= len {
                        break;
                    }
                }
                None => break,
            }
        }
        n
    }

    /// Whether nothing opaque lies between `from` and `to`.
    pub fn clear(&self, from: Vec3, to: Vec3) -> bool {
        let d = to - from;
        let len = d.length();
        if len < 1e-3 {
            return true;
        }
        let ray = Ray::new(Point3::new(from.x, from.y, from.z), Vector3::new(d.x / len, d.y / len, d.z / len));
        self.query
            .cast_ray(&self.bodies, &self.colliders, &ray, len, true, QueryFilter::default())
            .is_none()
    }
}
