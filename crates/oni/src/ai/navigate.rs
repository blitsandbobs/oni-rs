//! Walking to a point through the level:
//! a path through the rooms is planned, then each room's stretch on
//! entering it (`AI2iPath_NewNode`), followed waypoint by waypoint (each
//! with its own arrival radius); being stuck, straying off the path or a
//! failed plan leads to planning again, and repeated failure to giving up.
//! A new order soon after, to near the same place, may keep the path
//! (`AI2iPath_MakePath`'s reuse).

use glam::Vec3;

use super::{Direction, Orders, Pace, Route};
use crate::nav::plan::{Error, Global, Leg};

/// Stuck: moved less than this…
const STUCK_DISTANCE: f32 = 0.5;
/// …over this many ticks while trying to move.
const STUCK_TICKS: u32 = 30;
/// More than this far off the current path segment: replan.
const OFF_PATH: f32 = 14.0;
/// After this many errors, give up.
const GIVE_UP: u32 = 5;
/// An error is forgotten after this many quiet ticks.
const ERROR_DECAY: u64 = 300;
/// After an error, no new one is noticed for this long.
const MIN_REPATH: u64 = 30;
/// After a failed plan, wait this long before trying again.
const RETRY_AFTER: u64 = 90;
/// A destination is reached within at least this.
const MIN_ARRIVE: f32 = 3.0;
/// A new destination this far from the last resets the error count
/// (`AI2cNewDestination_Distance`).
const NEW_DESTINATION: f32 = 15.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Needs a (new) plan.
    Planning,
    Following,
    Arrived,
    /// Couldn't get there.
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Navigator {
    pub goal: Vec3,
    pub arrive: f32,
    pub pace: Pace,
    pub state: State,
    route: Option<Route>,
    errors: u32,
    last_error: u64,
    retry_at: u64,
    /// Plan without smoothing (after an error).
    pub careful: bool,
    /// The last plan found no route (the original's path request tells its
    /// caller so at once; this navigator meanwhile waits to try again).
    pub plan_failed: bool,
    /// A character it may path through (`curPathIgnoreChar`): whom it
    /// follows or goes up to.
    pub ignore: Option<usize>,
    /// A character it follows (`AI2cDestinationType_FollowCharacter`):
    /// each new path heads for where it is then.
    pub follow: Option<usize>,
    stuck_from: (Vec3, u32),
    /// The path through the rooms, the room it is in along it, and when and
    /// for where it was planned (for reuse).
    pub global: Option<Global>,
    pub leg: usize,
    pub planned: (u64, Vec3),
    /// The current room's stretch (its start and end, for reuse).
    pub leg_ends: (Vec3, Vec3),
    /// It has reached the next room: its stretch there needs planning.
    pub leg_due: bool,
    /// Given a new destination while following: keep the path if it may.
    pub retarget: bool,
}

impl Navigator {
    pub fn new(goal: Vec3, arrive: f32, pace: Pace) -> Navigator {
        Navigator {
            goal,
            arrive: arrive.max(MIN_ARRIVE),
            pace,
            state: State::Planning,
            route: None,
            errors: 0,
            last_error: 0,
            retry_at: 0,
            plan_failed: false,
            careful: false,
            ignore: None,
            follow: None,
            stuck_from: (Vec3::ZERO, 0),
            global: None,
            leg: 0,
            planned: (0, Vec3::ZERO),
            leg_ends: (Vec3::ZERO, Vec3::ZERO),
            leg_due: false,
            retarget: false,
        }
    }

    /// Changes the pace (also of a path being followed).
    pub fn set_pace(&mut self, pace: Pace) {
        self.pace = pace;
        if let Some(r) = self.route.as_mut() {
            r.pace = pace;
        }
    }

    /// The waypoints being followed (with arrival radii) and the index of
    /// the next one.
    pub fn waypoints(&self) -> (Vec<(Vec3, f32)>, usize) {
        match &self.route {
            Some(r) => (
                r.points
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, r.radii.get(i).copied().unwrap_or(r.arrive)))
                    .collect(),
                r.next,
            ),
            None => (Vec::new(), 0),
        }
    }

    /// Whether the world should plan its whole path now.
    pub fn wants_plan(&self, now: u64) -> bool {
        self.state == State::Planning && now >= self.retry_at
    }

    /// Whether the room it has come into needs its stretch planned.
    pub fn wants_leg(&self) -> bool {
        self.state == State::Following && self.leg_due && self.global.is_some()
    }

    /// A new destination (`AI2rPath_GoToPoint`): while following, the world
    /// decides whether the path can be kept ([`Self::retarget`]); otherwise
    /// it plans afresh. One far from the last starts its error count over.
    pub fn new_goal(&mut self, goal: Vec3, arrive: f32, pace: Pace) {
        if goal.distance(self.goal) > NEW_DESTINATION {
            self.errors = 0;
            self.careful = false;
        }
        self.goal = goal;
        self.arrive = arrive.max(MIN_ARRIVE);
        self.set_pace(pace);
        match self.state {
            State::Following => self.retarget = true,
            State::Planning => {}
            _ => {
                self.state = State::Planning;
                self.retry_at = 0;
                self.plan_failed = false;
            }
        }
    }

    /// Takes a whole new path: the rooms, and the first room's stretch (or
    /// the failure).
    pub fn take_plan(&mut self, plan: Result<(Global, Leg), Error>, now: u64) {
        self.retarget = false;
        match plan {
            Ok((g, first)) => {
                self.global = Some(g);
                self.leg = 0;
                self.planned = (now, self.goal);
                self.plan_failed = false;
                self.take_leg(Ok(first), now);
            }
            Err(e) => {
                self.plan_failed = true;
                self.retry_at = now + RETRY_AFTER;
                self.error(now, &format!("{e:?}"));
            }
        }
    }

    /// Takes the current room's stretch (or the failure).
    pub fn take_leg(&mut self, leg: Result<Leg, Error>, now: u64) {
        self.leg_due = false;
        match leg {
            Ok(leg) => {
                let n = leg.points.len();
                let (points, mut radii): (Vec<Vec3>, Vec<f32>) = leg.points.into_iter().unzip();
                let last_leg = self
                    .global
                    .as_ref()
                    .is_none_or(|g| self.leg + 1 >= g.legs());
                if let Some(last) = radii.last_mut().filter(|_| last_leg) {
                    *last = last.min(self.arrive).max(MIN_ARRIVE);
                }
                if self.careful {
                    radii.iter_mut().for_each(|r| *r = MIN_ARRIVE);
                }
                let mut route = Route::new(points, self.pace, false);
                route.radii = radii;
                self.route = (n > 0).then_some(route);
                self.leg_ends = (leg.start, leg.end);
                self.state = State::Following;
            }
            Err(e) => self.error(now, &format!("{e:?}")),
        }
    }

    /// The current room's stretch kept for a moved destination: its last
    /// waypoint moved to the stretch's new end.
    pub fn keep_leg(&mut self, end: Vec3) {
        self.retarget = false;
        if let Some(g) = self.global.as_mut() {
            g.to = self.goal;
        }
        self.leg_ends.1 = end;
        if let Some(p) = self.route.as_mut().and_then(|r| r.points.last_mut()) {
            *p = end;
        }
    }

    fn error(&mut self, now: u64, why: &str) {
        if now.saturating_sub(self.last_error) > ERROR_DECAY {
            self.errors = self.errors.saturating_sub(1);
        }
        self.errors += 1;
        self.last_error = now;
        self.careful = true;
        self.route = None;
        self.global = None;
        self.leg_due = false;
        self.retarget = false;
        self.state = if self.errors >= GIVE_UP {
            State::Failed(why.to_string())
        } else {
            State::Planning
        };
    }

    /// Wedged against something its movement keeps meeting
    /// (`AI2rMovementState_CheckFailure`, "collision stuck"): plan again.
    pub fn collision_stuck(&mut self, now: u64) {
        // (Not again within 30 ticks of the last error, `AI2cRepath_MinFrames`.)
        if self.state == State::Following && now.saturating_sub(self.last_error) >= MIN_REPATH {
            self.error(now, "collision stuck");
        }
    }

    /// This tick's orders for a body at `feet` facing `yaw`.
    pub fn orders(&mut self, feet: Vec3, yaw: f32, now: u64) -> Orders {
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
        if flat(self.goal - feet).length() <= self.arrive {
            self.state = State::Arrived;
            self.route = None;
        }
        if self.state != State::Following {
            return Orders::default();
        }
        if self.leg_due {
            // (Waiting on the next room's stretch: on as it was going.)
            return Orders {
                direction: Direction::Forward,
                pace: self.pace,
                face: Some(yaw),
            };
        }
        let Some(route) = self.route.as_mut() else {
            return Orders::default();
        };
        // Strayed off the path?
        let next = route.next.min(route.points.len().saturating_sub(1));
        if next > 0 {
            let (a, b) = (flat(route.points[next - 1]), flat(route.points[next]));
            let ab = b - a;
            let t = if ab.length_squared() > 1e-6 {
                ((flat(feet) - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if (flat(feet) - (a + ab * t)).length() > OFF_PATH {
                self.error(now, "off the path");
                return Orders::default();
            }
        }
        let orders = route.orders(feet, yaw);
        if route.finished() {
            // Into the next room: its stretch is planned before the next
            // tick; meanwhile it keeps on as it was going.
            if self
                .global
                .as_ref()
                .is_some_and(|g| self.leg + 1 < g.legs())
            {
                self.leg += 1;
                self.leg_due = true;
                return Orders {
                    direction: Direction::Forward,
                    pace: self.pace,
                    face: Some(yaw),
                };
            }
            self.state = State::Arrived;
            return Orders::default();
        }
        // Stuck?
        if orders.direction != Direction::Stopped {
            let (at, ticks) = self.stuck_from;
            if flat(feet - at).length() > STUCK_DISTANCE {
                self.stuck_from = (feet, 0);
            } else if ticks + 1 >= STUCK_TICKS {
                self.stuck_from = (feet, 0);
                self.error(now, "stuck");
                return Orders::default();
            } else {
                self.stuck_from.1 = ticks + 1;
            }
        }
        orders
    }
}
