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

pub mod avoid;
pub mod bosses;
pub mod goals;
pub mod level;
pub mod melee;
pub mod navigate;
pub mod perception;
pub mod ranged;

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

/// What the AI movement system has the player do while a script holds her
/// (`ai2_takecontrol`, `docs/AI.md`, *Scripted movement*): walk to a point
/// (`ai2_movetoflag`), then turn to a facing; or turn toward a point
/// (`ai2_lookatchar`).
#[derive(Clone, Debug, Default)]
pub struct Steer {
    pub nav: Option<navigate::Navigator>,
    /// The way to face once there (or now, with nowhere to go).
    pub face: Option<f32>,
    pub mode: goals::MoveMode,
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
        let cap = if orders.pace == Pace::Run {
            MAX_TURN
        } else {
            MAX_TURN * 0.5
        };
        let delta = relative(target - facing.facing);
        facing.rotate(delta.clamp(-cap, cap));
    }
    // The AI owns its facing: keep the "wanted" facing on the body so the
    // movement system neither slews it nor plays step-turn clips.
    facing.desired_facing = facing.facing;
    facing.facing_modifier = 0.0;
    keys
}

pub(crate) use crate::angle::relative;

/// The body yaw that faces from `from` toward `to` (on the ground plane).
pub fn yaw_toward(from: Vec3, to: Vec3) -> f32 {
    crate::angle::yaw_of(to - from)
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
    /// Within this distance a point counts as reached…
    pub arrive: f32,
    /// …or within its own radius, where given.
    pub radii: Vec<f32>,
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
    let turn = if pace == Pace::Run {
        MAX_TURN
    } else {
        MAX_TURN * 0.5
    };
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
            radii: Vec::new(),
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
            let arrive = self.radii.get(self.next).copied().unwrap_or(self.arrive);
            if dist > arrive {
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

/// What a character's AI knows this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Senses {
    pub feet: Vec3,
    /// Lying on the ground, the knockdown itself over.
    pub lying: bool,
    /// In a hit reaction (no control anyway).
    pub reeling: bool,
    /// In an attack clip.
    pub attacking: bool,
    /// Attack clips started so far (combo progress).
    pub attacks_started: u32,
    /// Where its opponent stands, if it has a living one.
    pub target: Option<Vec3>,
    /// Game time (ticks).
    pub now: u64,
}

/// What drives a non-player character.
#[derive(Clone, Debug)]
pub enum Brain {
    /// Walk a scripted route.
    Route(Route),
    /// Keep following fixed orders (scripted / test control).
    Hold(Orders),
    /// Fight an opponent.
    Spar(Spar),
    /// Walk to a point through the level (planned by the world).
    GoTo(Box<navigate::Navigator>),
    /// Fight an opponent hand to hand with the class's melee AI (driven by
    /// the world, which gives it a view of both fighters).
    Melee(Box<melee::brain::Melee>),
    /// The AI movement system driving the player for a script
    /// (`ai2_takecontrol`): no goals, only where to go and which way to face.
    Steer(Box<Steer>),
    /// Goals and a job (`docs/GOALS.md`; updated by the world).
    Agent(Box<goals::Agent>),
}

impl Brain {
    /// This tick's control bits; turns `facing` as ordered.
    pub fn think(&mut self, senses: &Senses, facing: &mut Facing, avoid: &mut avoid::Avoid) -> u32 {
        // The AI's own movement steers round what it keeps bumping into
        // (`ai/avoid.rs`); wedged, its path fails.
        let steer = |orders: Orders, avoid: &mut avoid::Avoid, facing: &Facing| {
            let wanted = match orders.direction {
                Direction::Stopped => None,
                Direction::Forward => Some(orders.face.unwrap_or(facing.facing)),
                Direction::Backpedal => Some(facing.facing + std::f32::consts::PI),
                Direction::SidestepLeft => Some(facing.facing + std::f32::consts::FRAC_PI_2),
                Direction::SidestepRight => Some(facing.facing - std::f32::consts::FRAC_PI_2),
            };
            let way = avoid.steer(wanted, senses.feet);
            // Steered off its way: the move relative to the body
            // (`AI2iMovementState_FindMovementDirection`) — ahead within 45°,
            // behind past 135°, else a sidestep — still facing as it was.
            match (orders.direction, way, wanted) {
                (Direction::Forward, Some(w), Some(m)) if (w - m).abs() > 1e-4 => {
                    let delta = relative(facing.facing - w);
                    let direction = if delta.abs() < std::f32::consts::FRAC_PI_4 {
                        Direction::Forward
                    } else if delta.abs() > 3.0 * std::f32::consts::FRAC_PI_4 {
                        Direction::Backpedal
                    } else if delta > 0.0 {
                        Direction::SidestepRight
                    } else {
                        Direction::SidestepLeft
                    };
                    Orders {
                        direction,
                        ..orders
                    }
                }
                _ => orders,
            }
        };
        match self {
            Brain::Route(r) => execute(
                steer(r.orders(senses.feet, facing.facing), avoid, facing),
                facing,
            ),
            Brain::Hold(o) => execute(*o, facing),
            Brain::Spar(s) => s.think(senses, facing),
            Brain::Melee(_) => 0, // decided by the world (`World::step`)
            Brain::Agent(a) => {
                let (orders, press) = a.orders(senses.feet, facing.facing, senses.now);
                let orders = steer(orders, avoid, facing);
                if std::mem::take(&mut avoid.stuck) {
                    if let Some(n) = a.nav.as_mut() {
                        n.collision_stuck(senses.now);
                    }
                }
                execute(orders, facing) | press
            }
            Brain::Steer(s) => {
                let now = senses.now;
                let mut o = match s.nav.as_mut() {
                    Some(n) => steer(n.orders(senses.feet, facing.facing, now), avoid, facing),
                    None => Orders::default(),
                };
                if std::mem::take(&mut avoid.stuck) {
                    if let Some(n) = s.nav.as_mut() {
                        n.collision_stuck(now);
                    }
                }
                let walking = s.nav.as_ref().is_some_and(|n| {
                    matches!(
                        n.state,
                        navigate::State::Following | navigate::State::Planning
                    )
                });
                if o.direction == Direction::Stopped && !walking {
                    o.face = s.face;
                }
                execute(o, facing)
            }
            Brain::GoTo(n) => {
                let now = senses.now;
                let orders = steer(n.orders(senses.feet, facing.facing, now), avoid, facing);
                if std::mem::take(&mut avoid.stuck) {
                    n.collision_stuck(now);
                }
                execute(orders, facing)
            }
        }
    }

    /// Whether it is about its everyday business (no alert, no fight): a
    /// casual AI always cries out when hurt.
    pub fn casual(&self) -> bool {
        match self {
            Brain::Agent(a) => a.casual(),
            _ => false,
        }
    }

    /// Whether it is taunting over a body it checked (its taunt line is
    /// not said then).
    pub fn body_taunted(&self) -> bool {
        match self {
            Brain::Agent(a) => a.body_taunted(),
            _ => false,
        }
    }

    /// The fighter this brain is fighting, if any.
    pub fn opponent(&self) -> Option<usize> {
        match self {
            Brain::Spar(s) => Some(s.target),
            Brain::Melee(m) => Some(m.target),
            Brain::Agent(a) => a.opponent(),
            _ => None,
        }
    }

    /// The navigator walking it somewhere, if any.
    pub fn navigator_mut(&mut self) -> Option<&mut navigate::Navigator> {
        match self {
            Brain::GoTo(n) => Some(n),
            Brain::Steer(s) => s.nav.as_mut(),
            Brain::Agent(a) => a.nav.as_mut(),
            _ => None,
        }
    }

    /// The melee AI driving it this tick, if any.
    pub fn melee_mut(&mut self) -> Option<&mut melee::brain::Melee> {
        match self {
            Brain::Melee(m) => Some(m),
            Brain::Agent(a) => a.melee_mut(),
            _ => None,
        }
    }
    pub fn melee(&self) -> Option<&melee::brain::Melee> {
        match self {
            Brain::Melee(m) => Some(m),
            Brain::Agent(a) => a.melee(),
            _ => None,
        }
    }
}

// --- sparring ------------------------------------------------------------------

use crate::anim::input::key::{FORWARD, KICK, PUNCH};

/// Closer than this, the sparring partner stops approaching (bodies touch at
/// roughly 6 units, and walking into the opponent is blocked anyway)…
const ENGAGE_DISTANCE: f32 = 7.0;
/// …and within this it may start a combo (well inside melee reach).
const STRIKE_DISTANCE: f32 = 8.5;
/// Beyond this it runs in; nearer, it walks.
const RUN_DISTANCE: f32 = 14.0;
/// Ticks between combos (a guard stance in between), min..max.
const PAUSE: (u32, u32) = (30, 90);
/// Ticks lying dazed after a knockdown before getting up, min..max.
const DAZE: (u32, u32) = (30, 90);
/// A combo whose next attack hasn't started this long after the last is over.
const COMBO_STALL: u32 = 45;

/// Button sequences the sparring partner strikes with (each a combo the
/// move rules chain: P, PP, PPP, K, KK, KKK, PPK, PK, KP).
const PATTERNS: &[&[u32]] = &[
    &[PUNCH],
    &[KICK],
    &[PUNCH, PUNCH],
    &[KICK, KICK],
    &[PUNCH, PUNCH, PUNCH],
    &[KICK, KICK, KICK],
    &[PUNCH, PUNCH, KICK],
    &[PUNCH, KICK],
    &[KICK, PUNCH],
];

/// A stand-in melee AI (issue #5 will replace its choices with the class's
/// melee profile): it closes in on its opponent, strikes with combo patterns
/// using the same move rules as the player, stands guard between combos
/// (blocking is automatic for a character standing still facing its
/// attacker), and after a knockdown lies dazed for a moment, then gets up.
#[derive(Clone, Debug)]
pub struct Spar {
    /// The fighter index it fights.
    pub target: usize,
    combo: Option<Combo>,
    /// Ticks left before the next combo may start.
    pause: u32,
    /// Ticks left lying down before getting up (None = not counting).
    daze: Option<u32>,
    tick: u32,
    rng: u32,
}

#[derive(Clone, Debug)]
struct Combo {
    buttons: &'static [u32],
    /// `attacks_started` when the combo began.
    base: u32,
    /// Attacks of this combo started so far, as of last tick.
    seen: usize,
    /// Ticks since the last attack started (or the combo began).
    idle: u32,
}

impl Spar {
    pub fn new(target: usize, seed: u32) -> Spar {
        Spar {
            target,
            combo: None,
            pause: PAUSE.0,
            daze: None,
            tick: 0,
            rng: seed.wrapping_mul(2_654_435_761).max(1),
        }
    }

    fn random(&mut self, lo: u32, hi: u32) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        lo + x % (hi - lo + 1)
    }

    fn think(&mut self, s: &Senses, facing: &mut Facing) -> u32 {
        self.tick = self.tick.wrapping_add(1);
        if s.reeling {
            self.combo = None;
            return execute(Orders::default(), facing);
        }
        // Knocked down: lie dazed, then press forward to get up.
        if s.lying {
            self.combo = None;
            let left = match self.daze {
                Some(n) => n,
                None => self.random(DAZE.0, DAZE.1),
            };
            if left == 0 {
                self.daze = None;
                return FORWARD;
            }
            self.daze = Some(left - 1);
            return 0;
        }
        self.daze = None;

        let Some(target) = s.target else {
            self.combo = None;
            return execute(Orders::default(), facing); // nobody left to fight
        };
        let face = yaw_toward(s.feet, target);
        let flat = Vec3::new(target.x - s.feet.x, 0.0, target.z - s.feet.z);
        let dist = flat.length();
        let off = relative(face - facing.facing).abs();

        // Mid-combo: keep pressing the next button (on alternate ticks, so
        // each press registers) until its attack starts; then the next.
        if let Some(c) = &mut self.combo {
            let done = s.attacks_started.wrapping_sub(c.base) as usize;
            let mut keys = execute(
                Orders {
                    face: Some(face),
                    ..Orders::default()
                },
                facing,
            );
            if done != c.seen {
                c.seen = done;
                c.idle = 0;
            }
            if done < c.buttons.len() {
                c.idle += 1;
                if c.idle > COMBO_STALL {
                    self.combo = None; // the chain broke (blocked, out of reach…)
                    self.pause = PAUSE.0;
                } else if self.tick.is_multiple_of(2) {
                    keys |= c.buttons[done];
                }
                return keys;
            }
            if !s.attacking {
                self.combo = None;
                self.pause = self.random(PAUSE.0, PAUSE.1);
            }
            return keys;
        }

        // Close in.
        if dist > ENGAGE_DISTANCE {
            let direction = if off > TURN_FIRST {
                Direction::Stopped
            } else {
                Direction::Forward
            };
            let pace = if dist > RUN_DISTANCE {
                Pace::Run
            } else {
                Pace::Walk
            };
            return execute(
                Orders {
                    direction,
                    pace,
                    face: Some(face),
                },
                facing,
            );
        }

        // In range: guard (stand facing the opponent) until it's time to strike.
        self.pause = self.pause.saturating_sub(1);
        let guard = execute(
            Orders {
                face: Some(face),
                ..Orders::default()
            },
            facing,
        );
        if self.pause == 0 && dist <= STRIKE_DISTANCE && off < 0.35 {
            let pick = self.random(0, PATTERNS.len() as u32 - 1) as usize;
            self.combo = Some(Combo {
                buttons: PATTERNS[pick],
                base: s.attacks_started,
                seen: 0,
                idle: 0,
            });
        }
        guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_become_control_bits() {
        let mut f = Facing::new(0.0);
        let o = |direction, pace| Orders {
            direction,
            pace,
            face: None,
        };
        assert_eq!(
            execute(o(Direction::Forward, Pace::Run), &mut f),
            key::FORWARD
        );
        assert_eq!(
            execute(o(Direction::Backpedal, Pace::Walk), &mut f),
            key::BACKWARD | key::WALK
        );
        assert_eq!(
            execute(o(Direction::SidestepLeft, Pace::Creep), &mut f),
            key::STEP_LEFT | key::CROUCH
        );
        assert_eq!(execute(o(Direction::Stopped, Pace::Run), &mut f), 0);
    }

    #[test]
    fn turning_is_capped_and_owned_by_the_ai() {
        let mut f = Facing::new(0.0);
        let orders = Orders {
            face: Some(1.0),
            ..Orders::default()
        };
        execute(orders, &mut f);
        assert!(
            (f.facing - MAX_TURN).abs() < 1e-5,
            "one capped step, got {}",
            f.facing
        );
        assert_eq!(
            f.desired_facing, f.facing,
            "nothing left for the movement system to slew"
        );
        let walking = Orders {
            face: Some(1.0),
            pace: Pace::Walk,
            ..Orders::default()
        };
        let before = f.facing;
        execute(walking, &mut f);
        assert!(
            (f.facing - before - MAX_TURN * 0.5).abs() < 1e-5,
            "half the rate walking"
        );
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
