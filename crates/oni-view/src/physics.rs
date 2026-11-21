//! What looking ahead along the floor finds. (Characters move against the
//! level by the original's rules, `chrphys.rs`; lines of sight and sound
//! run on the level's quads, `quads.rs`.)

/// What lies ahead along the floor (see [`crate::chrphys::probe`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// Distance the body could move before a wall.
    pub free: f32,
    /// Distance at which the floor drops away, if within `free`.
    pub ledge: Option<f32>,
}
