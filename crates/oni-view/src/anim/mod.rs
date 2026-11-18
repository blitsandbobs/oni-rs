//! Oni's character movement / animation system, ported: the animation
//! collection lookup, the per-character animation model, the input probe
//! chain, and the identifiers they share. See `docs/MOVEMENT.md`.

// Transitional: the identifier tables and pause/interpolation helpers are
// consumed by the character model and probe chain (next phases of the port).
#![allow(dead_code)]

pub mod collection;
pub mod types;
