//! Small geometry shared across the engine: the data's 4×3 matrices and
//! object clips' placement.

use glam::{Mat4, Vec4};
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

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

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
