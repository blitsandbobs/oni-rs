//! The render camera: where it is, which way it looks, and its projection.
//! (What drives it in play is the game camera, `world/camera.rs`.)

use glam::{Mat4, Vec3};

pub struct Camera {
    pub eye: Vec3,
    pub yaw: f32,   // radians, around +Y
    pub pitch: f32, // radians
    /// Its up direction (a camera clip can roll it).
    pub up: Vec3,
    pub fov_y: f32, // radians
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

/// The game's field of view (`ONgMotoko_FieldOfView`: 45° vertically), near
/// plane (`ONcMotoko_NearPlane`) and far plane (`ONgMotoko_FarPlane`).
pub const GAME_FOV_Y: f32 = 45.0 * std::f32::consts::PI / 180.0;
pub const GAME_NEAR: f32 = 2.0;
pub const GAME_FAR: f32 = 10_000.0;

impl Camera {
    pub fn new(aspect: f32) -> Self {
        Self {
            eye: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            up: Vec3::Y,
            fov_y: 70f32.to_radians(),
            aspect,
            near: 0.5,
            far: 20_000.0,
        }
    }

    /// A camera with the game's projection.
    pub fn game(aspect: f32) -> Self {
        Self { fov_y: GAME_FOV_Y, near: GAME_NEAR, far: GAME_FAR, ..Self::new(aspect) }
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

    /// World-space right/up vectors of the camera plane, for orienting
    /// camera-facing billboards (`right = forward × up`, `up = right × forward`).
    pub fn billboard_basis(&self) -> (Vec3, Vec3) {
        let fwd = self.forward();
        let right = fwd.cross(self.up).normalize_or_zero();
        let up = right.cross(fwd).normalize_or_zero();
        (right, up)
    }

    /// Look from `eye` toward `target`.
    pub fn look_at(&mut self, eye: Vec3, target: Vec3) {
        self.look_to(eye, target - eye, Vec3::Y);
    }

    /// Look from `eye` along `dir`, with `up` overhead.
    pub fn look_to(&mut self, eye: Vec3, dir: Vec3, up: Vec3) {
        self.eye = eye;
        let d = dir.normalize_or_zero();
        self.pitch = d.y.clamp(-0.999, 0.999).asin();
        self.yaw = d.z.atan2(d.x);
        self.up = up.normalize_or(Vec3::Y);
    }

    /// The view matrix.
    pub fn view(&self) -> Mat4 {
        Mat4::look_to_rh(self.eye, self.forward(), self.up)
    }

    /// Combined view-projection matrix (reverse-Z not used; standard depth).
    pub fn view_proj(&self) -> Mat4 {
        let proj = Mat4::perspective_rh(self.fov_y, self.aspect, self.near, self.far);
        proj * self.view()
    }
}
