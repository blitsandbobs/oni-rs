//! Known bugs in the original game, and whether to reproduce them.
//!
//! By default the engine behaves as the designers evidently intended; with
//! `--faithful` (or Lua `faithful(true)`) it reproduces the shipped game's
//! behaviour, bugs included. Each quirk is one named method so the choice is
//! visible at the point of use; `docs/deviations.md` describes each one.

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

    /// Whether hit prediction turns a colliding target's movement into its
    /// own frame with `dx` in place of `dz` across (Q15).
    pub fn colliding_velocity_slip(self) -> bool {
        self.faithful
    }

    /// Whether the camera's see-through test reads the feet's side of a
    /// wall twice where it meant the head's: a wall between the camera and
    /// the player's head, but not their feet, stays solid (Q16).
    pub fn jello_plane_test_feet_twice(self) -> bool {
        self.faithful
    }

    /// Whether the camera's see-through test on objects reads its ray's
    /// vector as the far end (`PHrCollision_Point` hands `CLrSphere_Line` a
    /// vector for a point): the segment runs from the camera toward the
    /// world's origin, so objects between the camera and the player rarely
    /// go see-through (Q17).
    pub fn jello_object_ray_as_point(self) -> bool {
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
    /// jump falls through to a neutral one (Q1 in `docs/deviations.md`).
    pub fn jump_right(self) -> u16 {
        if self.faithful {
            ty::LAND_RIGHT
        } else {
            ty::JUMP_RIGHT
        }
    }
}

/// Goals.
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

    /// Q12: a combat target's death never hands over to another enemy.
    pub fn no_new_target(self) -> bool {
        self.faithful
    }

    /// Q18: Space's skip of a cutscene is commented out — cutscenes can't
    /// be skipped.
    pub fn cutscenes_unskippable(self) -> bool {
        self.faithful
    }

    /// Q19: the options' Invert Mouse box is titled "Off" (ticked: inverted).
    pub fn invert_mouse_titled_off(self) -> bool {
        self.faithful
    }

    /// Q22: the melee AI weighs a rifle disarm as for a pistol (then gives
    /// it up): it never takes a rifle.
    pub fn rifle_disarm_wants_pistol(self) -> bool {
        self.faithful
    }

    /// Q21: the corpse array holds 20 bodies, the level's own among them.
    pub fn corpse_limit_20(self) -> bool {
        self.faithful
    }

    /// Q20: a teleport's fade back in (its teleport-out clip) never runs —
    /// the character pops back into view.
    pub fn teleport_pops_in(self) -> bool {
        self.faithful
    }

    /// Q23: letting go of a gun zeroes its fire and reload delays
    /// (`WPrRelease`), so dropping it and picking it up skips them — the
    /// community's "cooldown exploit".
    pub fn drop_resets_cooldown(self) -> bool {
        self.faithful
    }

    /// Q24: the projectile dodge measures the trajectory's closest point
    /// from the world's origin, not from the character, so AIs almost never
    /// dodge projectiles.
    pub fn projectiles_from_origin(self) -> bool {
        self.faithful
    }

    /// Q25: a throw takes the nearest foe in reach and arc with no look for
    /// a wall between (`AttemptThrow`): someone on the other side of a thin
    /// wall can be thrown.
    pub fn throw_through_walls(self) -> bool {
        self.faithful
    }

    /// Q26: a reload takes its clip when it starts and fills the gun at its
    /// clip's action frame; knocked down in between, the clip is lost.
    pub fn interrupted_reload_loses_clip(self) -> bool {
        self.faithful
    }

    /// Q27: the cutscene inserts are scaled from their 4:3 virtual screen on
    /// each axis alone, so a wide screen stretches them.
    pub fn portraits_stretched(self) -> bool {
        self.faithful
    }

    /// Q28: a fading lens flare beyond the fog's end is drawn at full
    /// strength (`gl_calculate_fog_factor` gives 1 there, where its own
    /// linear ramp falls to 0).
    pub fn flares_unfogged_past_fog_end(self) -> bool {
        self.faithful
    }

    /// Whether the level scripts run as shipped, without our fixes to their
    /// most glaring mistakes (`docs/deviations.md`).
    pub fn scripts_as_shipped(self) -> bool {
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
        assert_eq!(
            (fixed.run_in_radius_times(), faithful.run_in_radius_times()),
            (1.0, 2.0)
        );
        assert!(!fixed.contact_head_twice() && faithful.contact_head_twice());
        assert_eq!(
            (
                fixed.investigate_check_every(),
                faithful.investigate_check_every()
            ),
            (15, 0)
        );
        assert!(!fixed.no_new_target() && faithful.no_new_target());
    }
}
