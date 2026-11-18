//! Character movement and animation: the animation collection lookup, the
//! per-character animation model, the input rules and per-tick decisions,
//! and the identifiers they share. A behavioural reimplementation of Oni's
//! system — see `docs/MOVEMENT.md`.

pub mod collection;
pub mod decide;
pub mod input;
pub mod motion;
pub mod reaction;
pub mod rules;
pub mod types;
