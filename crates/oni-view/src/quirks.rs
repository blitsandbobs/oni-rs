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

    /// Whether a body killed in the air flies on with its flight unturned:
    /// the shipped game keeps the flight in the body's own frame but moves it
    /// as if it were the world's (Q13).
    pub fn corpse_flight_unturned(self) -> bool {
        self.faithful
    }

    /// Whether a door counts as blocked only by its first leaf: the shipped
    /// game tests the first leaf's pause twice where it meant each leaf, so
    /// someone blocking only a double door's second leaf holds it shut for
    /// good — it never turns back (Q14).
    pub fn door_first_leaf_only(self) -> bool {
        self.faithful
    }

    /// Whether a melee technique ignores danger. The shipped game tests its
    /// "fearless" flag against the wrong set of bits — the ones holding the
    /// technique's start state — so it really depends on that state (Q5).
    pub fn fearless(self, flag: bool, start_state: u8) -> bool {
        if self.faithful {
            start_state & 0b100 != 0
        } else {
            flag
        }
    }

    /// Whether a melee AI weighs a throw against a target it can't throw
    /// (knockdown-resistant, invulnerable). The shipped game only notices at
    /// the last moment, so such throws keep their weight (Q6).
    pub fn weigh_unthrowable(self) -> bool {
        !self.faithful
    }

    /// How many times the target's radius comes off the run-in distance the
    /// melee AI plans with: the shipped game subtracts it twice (Q7).
    pub fn run_in_radius_times(self) -> f32 {
        if self.faithful {
            2.0
        } else {
            1.0
        }
    }

    /// Hit prediction's body-contact step: the shipped game adds the target's
    /// height offset to its head twice (Q8).
    pub fn contact_head_twice(self) -> bool {
        self.faithful
    }

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

/// Goals (`docs/GOALS.md`).
impl Quirks {
    /// Q10: how often an investigator checks its view of the spot it is
    /// heading for (meant every 15 ticks; shipped, every tick).
    pub fn investigate_check_every(self) -> u64 {
        if self.faithful {
            0
        } else {
            15
        }
    }

    /// Q11: losing sight of its target, combat pursues at once instead of
    /// after the class's lost-contact delay.
    pub fn pursue_lost_target_at_once(self) -> bool {
        self.faithful
    }

    /// Q12: a combat target's death never hands over to another enemy.
    pub fn no_new_target(self) -> bool {
        self.faithful
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melee_quirks_follow_the_mode() {
        let (fixed, faithful) = (Quirks::FIXED, Quirks::FAITHFUL);
        // Fearless: the flag, or (shipped) bit 2 of the start state.
        assert!(fixed.fearless(true, 1) && !fixed.fearless(false, 4));
        assert!(faithful.fearless(false, 4) && !faithful.fearless(true, 1));
        assert!(fixed.weigh_unthrowable() && !faithful.weigh_unthrowable());
        assert_eq!((fixed.run_in_radius_times(), faithful.run_in_radius_times()), (1.0, 2.0));
        assert!(!fixed.contact_head_twice() && faithful.contact_head_twice());
        assert_eq!((fixed.investigate_check_every(), faithful.investigate_check_every()), (15, 0));
        assert!(!fixed.pursue_lost_target_at_once() && faithful.pursue_lost_target_at_once());
        assert!(!fixed.no_new_target() && faithful.no_new_target());
    }
}
