//! Character AI, first layer: the **executor** and a scripted route follower
//! (issue #1; behaviour per `docs/AI.md`).
//!
//! Oni's AI has no movement of its own. Each tick it decides where the
//! character should face and which way it should move (forward, backpedal,
//! sidestep, or stop) at what pace (run, walk, creep); the executor turns
//! that into the same control bits a player produces and turns the body —
//! and the character's ordinary movement system (`anim::decide`) does the
//! rest: start clips, loops, stops, collisions. Higher layers (routes now;
//! pathfinding, goals and combat later) only ever produce [`Orders`].

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::anim::input::key;
use crate::control::Facing;

/// The most an AI turns its body per tick (radians), running; half when
/// walking or creeping.
pub const MAX_TURN: f32 = TAU / 100.0;

/// Which way to move relative to the body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Stopped,
    Forward,
    Backpedal,
    SidestepLeft,
    SidestepRight,
}

/// How fast to move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pace {
    #[default]
    Run,
    Walk,
    /// Crouched movement.
    Creep,
}

/// What the AI wants this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Orders {
    pub direction: Direction,
    pub pace: Pace,
    /// Body yaw to turn toward (radians), if any.
    pub face: Option<f32>,
}

/// Turns orders into control bits and turns the body. Returns the bits.
pub fn execute(orders: Orders, facing: &mut Facing) -> u32 {
    let mut keys = match orders.direction {
        Direction::Stopped => 0,
        Direction::Forward => key::FORWARD,
        Direction::Backpedal => key::BACKWARD,
        Direction::SidestepLeft => key::STEP_LEFT,
        Direction::SidestepRight => key::STEP_RIGHT,
    };
    keys |= match orders.pace {
        Pace::Run => 0,
        Pace::Walk => key::WALK,
        Pace::Creep => key::CROUCH,
    };
    if let Some(target) = orders.face {
        let cap = if orders.pace == Pace::Run { MAX_TURN } else { MAX_TURN * 0.5 };
        let delta = relative(target - facing.facing);
        facing.rotate(delta.clamp(-cap, cap));
    }
    // The AI owns its facing: keep the "wanted" facing on the body so the
    // movement system neither slews it nor plays step-turn clips.
    facing.desired_facing = facing.facing;
    facing.facing_modifier = 0.0;
    keys
}

/// Folds an angle into `(-π, π]`.
fn relative(a: f32) -> f32 {
    let r = (a + PI).rem_euclid(TAU) - PI;
    if r <= -PI {
        r + TAU
    } else {
        r
    }
}

/// The body yaw that faces from `from` toward `to` (on the ground plane).
pub fn yaw_toward(from: Vec3, to: Vec3) -> f32 {
    let d = to - from;
    d.x.atan2(d.z)
}

/// Follows a list of points on the ground: turns toward the next one, runs
/// (or walks) to it, and moves on; stops at the last (or loops).
#[derive(Clone, Debug)]
pub struct Route {
    pub points: Vec<Vec3>,
    pub pace: Pace,
    pub looping: bool,
    /// Index of the point being headed for.
    pub next: usize,
    /// Within this distance a point counts as reached.
    pub arrive: f32,
}

/// A point counts as reached this close (Oni units, on the ground plane).
pub const ARRIVE_DISTANCE: f32 = 2.5;
/// A target more than this far off the body's facing is turned toward on the
/// spot before moving off.
const TURN_FIRST: f32 = PI / 2.0;

/// Roughly how fast each pace covers ground (units per tick, measured from
/// Konoko's clips) — with the turn cap this gives the turning circle.
fn ground_speed(pace: Pace) -> f32 {
    match pace {
        Pace::Run => 0.6,
        Pace::Walk | Pace::Creep => 0.25,
    }
}

/// The tightest circle the character can run/walk around at `pace`.
fn turning_radius(pace: Pace) -> f32 {
    let turn = if pace == Pace::Run { MAX_TURN } else { MAX_TURN * 0.5 };
    // A margin over the ideal circle: speed varies through start clips.
    1.25 * ground_speed(pace) / turn
}

impl Route {
    pub fn new(points: Vec<Vec3>, pace: Pace, looping: bool) -> Route {
        Route {
            points,
            pace,
            looping,
            next: 0,
            arrive: ARRIVE_DISTANCE,
        }
    }

    /// Whether the last point has been reached (never, when looping).
    pub fn finished(&self) -> bool {
        self.next >= self.points.len()
    }

    /// This tick's orders for a character standing at `feet` facing `yaw`.
    pub fn orders(&mut self, feet: Vec3, yaw: f32) -> Orders {
        loop {
            let Some(&target) = self.points.get(self.next) else {
                return Orders::default(); // done: stand still
            };
            let flat = Vec3::new(target.x - feet.x, 0.0, target.z - feet.z);
            let dist = flat.length();
            if dist > self.arrive {
                let face = yaw_toward(feet, target);
                let off = relative(face - yaw).abs();
                // Moving on, the body sweeps a circle of the turning radius:
                // a target inside it can't be reached by turning on the move
                // (the character would orbit it), so turn on the spot first.
                // The arc through the target tangent to the facing has radius
                // dist / (2 sin off).
                let arc = dist / (2.0 * off.sin().max(1e-3));
                let direction = if off > TURN_FIRST || arc < turning_radius(self.pace) {
                    Direction::Stopped
                } else {
                    Direction::Forward
                };
                return Orders {
                    direction,
                    pace: self.pace,
                    face: Some(face),
                };
            }
            self.next += 1;
            if self.finished() && self.looping && !self.points.is_empty() {
                self.next = 0;
            }
            if self.finished() {
                return Orders::default();
            }
        }
    }
}

/// What drives a non-player character.
#[derive(Clone, Debug)]
pub enum Brain {
    /// Walk a scripted route.
    Route(Route),
    /// Keep following fixed orders (scripted / test control).
    Hold(Orders),
}

impl Brain {
    /// This tick's control bits; turns `facing` as ordered.
    pub fn think(&mut self, feet: Vec3, facing: &mut Facing) -> u32 {
        let orders = match self {
            Brain::Route(r) => r.orders(feet, facing.facing),
            Brain::Hold(o) => *o,
        };
        execute(orders, facing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_become_control_bits() {
        let mut f = Facing::new(0.0);
        let o = |direction, pace| Orders { direction, pace, face: None };
        assert_eq!(execute(o(Direction::Forward, Pace::Run), &mut f), key::FORWARD);
        assert_eq!(execute(o(Direction::Backpedal, Pace::Walk), &mut f), key::BACKWARD | key::WALK);
        assert_eq!(
            execute(o(Direction::SidestepLeft, Pace::Creep), &mut f),
            key::STEP_LEFT | key::CROUCH
        );
        assert_eq!(execute(o(Direction::Stopped, Pace::Run), &mut f), 0);
    }

    #[test]
    fn turning_is_capped_and_owned_by_the_ai() {
        let mut f = Facing::new(0.0);
        let orders = Orders { face: Some(1.0), ..Orders::default() };
        execute(orders, &mut f);
        assert!((f.facing - MAX_TURN).abs() < 1e-5, "one capped step, got {}", f.facing);
        assert_eq!(f.desired_facing, f.facing, "nothing left for the movement system to slew");
        let walking = Orders { face: Some(1.0), pace: Pace::Walk, ..Orders::default() };
        let before = f.facing;
        execute(walking, &mut f);
        assert!((f.facing - before - MAX_TURN * 0.5).abs() < 1e-5, "half the rate walking");
    }

    #[test]
    fn route_heads_for_each_point_then_stops() {
        let pts = vec![Vec3::new(0.0, 0.0, 10.0), Vec3::new(10.0, 0.0, 10.0)];
        let mut r = Route::new(pts, Pace::Run, false);
        // At the start, facing the first point: run toward it.
        let o = r.orders(Vec3::ZERO, 0.0);
        assert_eq!(o.direction, Direction::Forward);
        assert!(o.face.unwrap().abs() < 1e-5);
        // Facing away from it: turn on the spot first.
        let o = r.orders(Vec3::ZERO, PI);
        assert_eq!(o.direction, Direction::Stopped);
        // Arrived at the first point: head for the second (+X = yaw +π/2).
        let o = r.orders(Vec3::new(0.0, 0.0, 10.0), 0.0);
        assert_eq!(r.next, 1);
        assert!((o.face.unwrap() - PI / 2.0).abs() < 1e-3);
        // Arrived at the last: stand still, finished.
        let o = r.orders(Vec3::new(9.5, 0.0, 10.0), PI / 2.0);
        assert_eq!(o, Orders::default());
        assert!(r.finished());
    }

    #[test]
    fn looping_route_wraps() {
        let pts = vec![Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO];
        let mut r = Route::new(pts, Pace::Walk, true);
        r.orders(Vec3::new(0.0, 0.0, 10.0), 0.0); // reach point 0
        r.orders(Vec3::ZERO, PI); // reach point 1 → wraps
        assert_eq!(r.next, 0);
        assert!(!r.finished());
    }
}
