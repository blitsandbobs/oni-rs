//! Player facing — a faithful port of Oni's three-field facing model
//! (`ONtCharacter.facing`/`desiredFacing`/`facingModifier`).
//!
//! The body yaw (`facing`, what we render) does not snap to input; it chases
//! the input target (`desired_facing`) at a capped rate each tick
//! ([`Facing::turn_toward`], Oni's `HandleTurning`). `facing_modifier` biases
//! the "forward" reference so a forward-moving clip can play while the body is
//! turned (strafe). Angles are radians; `facing`/`desired_facing` are kept in
//! `[0, 2π)` (Oni's `UUmTrig_Clip`).

use std::f32::consts::{PI, TAU};

/// Body turn cap per tick while running (Oni `iGetTurnPerFrame` `medTurn`).
pub const MED_TURN: f32 = PI * 1.5 / 60.0;
/// Body turn cap per tick while walking (Oni `slowTurn`).
pub const SLOW_TURN: f32 = PI * 1.5 / 120.0;
/// Pending-turn threshold past which a turn-in-place clip plays
/// (Oni `ONgAimingWidth` = 70°); smaller turns just slew.
pub const TURN_DEADZONE: f32 = PI * 70.0 / 180.0;
/// Wraps an angle into `[0, 2π)` (Oni's `UUmTrig_Clip`).
fn wrap_2pi(a: f32) -> f32 {
    let m = a % TAU;
    if m < 0.0 {
        m + TAU
    } else {
        m
    }
}

/// Folds an angle difference into `[-π, π]` (Oni's `angle_to_relative_angle`;
/// one correction, assuming the input is already within `[-2π, 2π]`).
fn rel_angle(a: f32) -> f32 {
    if a > PI {
        a - TAU
    } else if a < -PI {
        a + TAU
    } else {
        a
    }
}

/// The player's facing state.
#[derive(Clone, Copy, Debug)]
pub struct Facing {
    /// Body yaw actually rendered/used for movement, `[0, 2π)`.
    pub facing: f32,
    /// Where input wants to point, `[0, 2π)`.
    pub desired_facing: f32,
    /// Offset so the movement "forward" reference can differ from body facing
    /// (strafe). Re-established each tick by the winning input (0 = none).
    pub facing_modifier: f32,
}

impl Facing {
    /// Starts facing (and desiring) `yaw`.
    pub fn new(yaw: f32) -> Self {
        let y = wrap_2pi(yaw);
        Facing {
            facing: y,
            desired_facing: y,
            facing_modifier: 0.0,
        }
    }

    /// Adds `delta` to the desired facing (Oni's `AdjustDesiredFacing`); the
    /// body then chases it over frames. `delta` should already be sanity-clamped
    /// to `±π` by the caller (Oni clamps the per-tick mouse turn).
    pub fn adjust_desired(&mut self, delta: f32) {
        self.desired_facing = wrap_2pi(self.desired_facing + delta);
    }

    /// Signed shortest-arc angle the body still has to turn to reach
    /// `desired_facing`, corrected by `facing_modifier` so the direction is
    /// chosen consistently (Oni's `ONrCharacter_GetDesiredFacingOffset`).
    pub fn desired_offset(&self) -> f32 {
        let forward_angle = self.desired_facing - self.facing_modifier;
        let forward_to_desired = rel_angle(self.desired_facing - forward_angle);
        let facing_to_forward = rel_angle(forward_angle - self.facing);
        let mut facing_to_desired = self.desired_facing - self.facing;

        if forward_to_desired > 0.0 && facing_to_forward > 0.0 {
            if facing_to_desired < 0.0 {
                facing_to_desired += TAU;
            }
        } else if forward_to_desired < 0.0 && facing_to_forward < 0.0 {
            if facing_to_desired > 0.0 {
                facing_to_desired -= TAU;
            }
        } else {
            facing_to_desired = rel_angle(facing_to_desired);
        }
        facing_to_desired
    }

    /// Turns the body by `amount` radians (positive = left).
    pub fn rotate(&mut self, amount: f32) {
        self.facing = wrap_2pi(self.facing + amount);
    }

    /// Sets the facing modifier: the body is asked to face `modifier` away
    /// from the aim direction (a diagonal run leans the body ±45°, a turned
    /// sidestep ±90°). The aim direction itself does not move.
    pub fn set_modifier(&mut self, modifier: f32) {
        let delta = modifier - self.facing_modifier;
        self.facing_modifier = modifier;
        self.adjust_desired(delta);
    }

    /// Where the player is aiming: the desired facing without the modifier.
    /// The follow camera tracks this, so diagonal runs don't swing the view.
    pub fn aim(&self) -> f32 {
        wrap_2pi(self.desired_facing - self.facing_modifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One capped slew toward the desired facing (what the decision step's
    /// steer does for a free-turning clip).
    fn slew(f: &mut Facing, cap: f32) {
        let off = f.desired_offset();
        f.rotate(if off > 0.0 {
            off.min(cap)
        } else {
            off.max(-cap)
        });
    }

    fn approx(a: f32, b: f32) -> bool {
        rel_angle(a - b).abs() < 1e-4
    }

    #[test]
    fn body_slews_toward_desired_at_the_cap() {
        let mut f = Facing::new(0.0);
        f.adjust_desired(1.0); // desire 1 rad to the left
        assert!((f.desired_facing - 1.0).abs() < 1e-5);
        // One capped step moves part-way, not all the way.
        slew(&mut f, 0.3);
        assert!(approx(f.facing, 0.3), "facing {}", f.facing);
        // Several steps converge exactly onto the target.
        for _ in 0..10 {
            slew(&mut f, 0.3);
        }
        assert!(approx(f.facing, 1.0), "facing {}", f.facing);
    }

    #[test]
    fn turns_the_short_way_across_the_wrap() {
        let mut f = Facing::new(0.1);
        // Desire just below 2π: shortest arc is a small NEGATIVE turn.
        f.desired_facing = TAU - 0.1;
        let off = f.desired_offset();
        assert!(off < 0.0 && off.abs() < 0.5, "offset {off}");
        slew(&mut f, 0.05);
        // Moved negative (wrapped), toward the target the short way.
        assert!(approx(f.facing, 0.05), "facing {}", f.facing);
    }

    #[test]
    fn modifier_leans_the_body_but_not_the_aim() {
        let mut f = Facing::new(0.0);
        f.set_modifier(PI / 4.0);
        assert!(
            approx(f.desired_facing, PI / 4.0),
            "body asked to lean 45° left"
        );
        assert!(approx(f.aim(), 0.0), "aim unchanged");
        f.set_modifier(0.0);
        assert!(
            approx(f.desired_facing, 0.0),
            "clearing the modifier undoes the lean"
        );
    }

    #[test]
    fn reaches_target_without_overshoot() {
        let mut f = Facing::new(0.0);
        f.adjust_desired(0.02);
        slew(&mut f, 0.1); // cap larger than the remaining offset
        assert!(approx(f.facing, 0.02), "facing {}", f.facing);
    }
}
