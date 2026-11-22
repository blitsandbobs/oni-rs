//! Angles: yaw in radians, 0 facing +Z, turning toward +X.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

/// An angle into `[0, 2π)` (`UUmTrig_Clip`).
pub fn wrap_2pi(a: f32) -> f32 {
    a.rem_euclid(TAU)
}

/// An angle into `[-π, π)`: a signed turn.
pub fn wrap_pi(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// An angle into `(-π, π]`.
pub fn relative(a: f32) -> f32 {
    let r = wrap_pi(a);
    if r <= -PI {
        r + TAU
    } else {
        r
    }
}

/// The yaw of a direction (its horizontal part).
pub fn yaw_of(d: Vec3) -> f32 {
    d.x.atan2(d.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps() {
        assert!((wrap_2pi(-0.5) - (TAU - 0.5)).abs() < 1e-6);
        assert!((wrap_pi(PI + 0.5) - (-PI + 0.5)).abs() < 1e-6);
        assert_eq!(relative(-PI), PI);
        assert!((yaw_of(Vec3::X) - PI / 2.0).abs() < 1e-6);
    }
}
