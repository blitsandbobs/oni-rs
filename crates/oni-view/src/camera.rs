//! A simple free-fly perspective camera.

use glam::{Mat4, Vec3};

pub struct Camera {
    pub eye: Vec3,
    pub yaw: f32,   // radians, around +Y
    pub pitch: f32, // radians
    pub fov_y: f32, // radians
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn new(aspect: f32) -> Self {
        Self {
            eye: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 70f32.to_radians(),
            aspect,
            near: 0.5,
            far: 20_000.0,
        }
    }

    /// Forward direction from yaw/pitch.
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    /// Look from `eye` toward `target`.
    pub fn look_at(&mut self, eye: Vec3, target: Vec3) {
        self.eye = eye;
        let d = (target - eye).normalize_or_zero();
        self.pitch = d.y.clamp(-0.999, 0.999).asin();
        self.yaw = d.z.atan2(d.x);
    }

    /// Combined view-projection matrix (reverse-Z not used; standard depth).
    pub fn view_proj(&self) -> Mat4 {
        let proj = Mat4::perspective_rh(self.fov_y, self.aspect, self.near, self.far);
        let view = Mat4::look_to_rh(self.eye, self.forward(), Vec3::Y);
        proj * view
    }
}
