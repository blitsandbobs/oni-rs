//! Known bugs in the original game, and whether to reproduce them.
//!
//! By default the engine behaves as the designers evidently intended; with
//! `--faithful` (or Lua `faithful(true)`) it reproduces the shipped game's
//! behaviour, bugs included. Each quirk is one named method so the choice is
//! visible at the point of use; `docs/QUIRKS.md` describes each one.

use crate::anim::types::ty;

/// Which behaviour to follow where the original is known to be buggy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quirks {
    /// Reproduce the shipped game's bugs.
    pub faithful: bool,
}

impl Quirks {
    pub const FIXED: Quirks = Quirks { faithful: false };
    pub const FAITHFUL: Quirks = Quirks { faithful: true };

    /// The animation type a jump with step-right held asks for. The shipped
    /// game names the right-landing type here (and in the matching "already
    /// jumping" check), so that request never finds a takeoff clip and the
    /// jump falls through to a neutral one (Q1 in `docs/QUIRKS.md`).
    pub fn jump_right(self) -> u16 {
        if self.faithful {
            ty::LAND_RIGHT
        } else {
            ty::JUMP_RIGHT
        }
    }
}
