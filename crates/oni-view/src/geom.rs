//! Small geometry shared across the engine: the data's 4×3 matrices and
//! object clips' placement.

use glam::{Mat4, Vec3, Vec4};
use oni_geom::objanim::ObjectAnimation;

/// `OBcAnimFlags_Localized`: an object clip played from its object's origin.
pub const CLIP_LOCALIZED: u32 = 0x10;

/// A 4×3 matrix stored column by column, translation last (the data's and
/// the original's `M3tMatrix4x3`); missing entries read 0.
pub fn mat4_4x3(m: &[f32]) -> Mat4 {
    let at = |i: usize| m.get(i).copied().unwrap_or(0.0);
    let col = |c: usize| {
        Vec4::new(
            at(c * 3),
            at(c * 3 + 1),
            at(c * 3 + 2),
            if c == 3 { 1.0 } else { 0.0 },
        )
    };
    Mat4::from_cols(col(0), col(1), col(2), col(3))
}

/// A 4×3 matrix from its JSON form (12 numbers, as `mat4_4x3`).
pub fn mat4_json(v: &serde_json::Value) -> Mat4 {
    let m: Vec<f32> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|x| x.as_f64().unwrap_or(0.0) as f32)
        .collect();
    mat4_4x3(&m)
}

/// An object clip's matrix at `frame`.
pub fn clip_matrix(a: &ObjectAnimation, frame: i32) -> Mat4 {
    mat4_4x3(&a.matrix(frame))
}

/// An object clip's matrix at `frame` for an object placed at `origin`
/// (`PHrPhysics_MaintainMatrix`): from the origin for a localized clip,
/// else the clip's own.
pub fn placed_clip_matrix(a: &ObjectAnimation, frame: i32, origin: Mat4) -> Mat4 {
    let m = clip_matrix(a, frame);
    if a.flags & CLIP_LOCALIZED != 0 {
        origin * m
    } else {
        m
    }
}

/// Where a ray meets a sphere: it starts inside (or on it), or enters at
/// `t` (in multiples of the ray, ≥ 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entry {
    Inside,
    At(f32),
}

/// Where a ray from `from` along `ray` first enters the sphere `centre`,
/// `r` — the one test behind the lasers', shots', particles' and objects'
/// sphere checks; each caller bounds `t` and reads `Inside` its own way.
/// Heading away from it misses.
pub fn ray_sphere(from: Vec3, ray: Vec3, centre: Vec3, r: f32) -> Option<Entry> {
    let oc = from - centre;
    let c = oc.length_squared() - r * r;
    if c <= 0.0 {
        return Some(Entry::Inside);
    }
    let a = ray.length_squared();
    let b = oc.dot(ray);
    if a < 1e-12 || b > 0.0 {
        return None;
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    Some(Entry::At((-b - disc.sqrt()) / a))
}

/// Whether the segment from `a` to `b` meets the sphere (`CLrSphere_Line`):
/// either end inside it, or its nearest point between them.
pub fn segment_meets_sphere(a: Vec3, b: Vec3, centre: Vec3, r: f32) -> bool {
    let r2 = r * r;
    if a.distance_squared(centre) < r2 || b.distance_squared(centre) < r2 {
        return true;
    }
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= 0.0 {
        return false;
    }
    let u = (centre - a).dot(ab) / len2;
    u > 0.0 && u < 1.0 && (a + ab * u).distance_squared(centre) < r2
}

/// The slab test behind every box check (door boxes, shots against
/// objects' boxes, trigger volumes): where the segment `from → from + ray`
/// overlaps the box spanning `lo..hi` along each of `axes` from `origin` —
/// the fraction it enters at (0–1) and the outward normal of the face it
/// enters by (`None` when it starts inside).
pub fn segment_box(
    from: Vec3,
    ray: Vec3,
    origin: Vec3,
    axes: [Vec3; 3],
    lo: Vec3,
    hi: Vec3,
) -> Option<(f32, Option<Vec3>)> {
    let d = from - origin;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let mut normal = None;
    for k in 0..3 {
        let (o, v) = (d.dot(axes[k]), ray.dot(axes[k]));
        if v.abs() < 1e-9 {
            if o < lo[k] || o > hi[k] {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((lo[k] - o) / v, (hi[k] - o) / v);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a > t0 {
            t0 = a;
            normal = Some(-axes[k] * v.signum());
        }
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, normal))
}

/// The world's axes, for an axis-aligned `segment_box`.
pub const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_enter_boxes_by_a_face() {
        let (lo, hi) = (Vec3::splat(-1.0), Vec3::splat(1.0));
        let hit = segment_box(
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::ZERO,
            AXES,
            lo,
            hi,
        );
        assert_eq!(hit, Some((0.5, Some(-Vec3::X))));
        let inside = segment_box(Vec3::ZERO, Vec3::X, Vec3::ZERO, AXES, lo, hi);
        assert_eq!(inside, Some((0.0, None)));
        assert_eq!(
            segment_box(
                Vec3::new(-3.0, 2.0, 0.0),
                Vec3::X * 6.0,
                Vec3::ZERO,
                AXES,
                lo,
                hi
            ),
            None
        );
    }

    #[test]
    fn rays_enter_spheres_ahead_only() {
        let c = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::X, c, 2.0),
            Some(Entry::At(8.0))
        );
        assert_eq!(ray_sphere(Vec3::ZERO, -Vec3::X, c, 2.0), None);
        assert_eq!(
            ray_sphere(Vec3::new(9.0, 0.0, 0.0), Vec3::X, c, 2.0),
            Some(Entry::Inside)
        );
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::X, Vec3::new(10.0, 3.0, 0.0), 2.0),
            None
        );
    }

    #[test]
    fn a_4x3_matrix_is_column_by_column_translation_last() {
        let m = mat4_4x3(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 5.0, 6.0, 7.0]);
        assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(
            mat4_4x3(&[]),
            Mat4::from_cols(Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::W)
        );
    }
}
