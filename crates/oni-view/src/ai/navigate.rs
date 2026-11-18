//! Walking to a point through the level (behaviour per
//! `docs/PATHFINDING.md`, *Following a path*): a planned path is followed
//! waypoint by waypoint (each with its own arrival radius); being stuck,
//! straying off the path or a failed plan leads to planning again, and
//! repeated failure to giving up.

use glam::Vec3;

use super::{Direction, Orders, Pace, Route};
use crate::nav::plan::{Error, Path};

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
/// After a failed plan, wait this long before trying again.
const RETRY_AFTER: u64 = 90;
/// A destination is reached within at least this.
const MIN_ARRIVE: f32 = 3.0;

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
    stuck_from: (Vec3, u32),
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
            careful: false,
            stuck_from: (Vec3::ZERO, 0),
        }
    }

    /// The waypoints being followed (with arrival radii) and the index of
    /// the next one.
    pub fn waypoints(&self) -> (Vec<(Vec3, f32)>, usize) {
        match &self.route {
            Some(r) => (
                r.points.iter().enumerate().map(|(i, p)| (*p, r.radii.get(i).copied().unwrap_or(r.arrive))).collect(),
                r.next,
            ),
            None => (Vec::new(), 0),
        }
    }

    /// Whether the world should plan for it now.
    pub fn wants_plan(&self, now: u64) -> bool {
        self.state == State::Planning && now >= self.retry_at
    }

    /// Takes a plan (or its failure).
    pub fn take_plan(&mut self, plan: Result<Path, Error>, now: u64) {
        match plan {
            Ok(path) => {
                let n = path.points.len();
                let (points, mut radii): (Vec<Vec3>, Vec<f32>) = path.points.into_iter().unzip();
                if let Some(last) = radii.last_mut() {
                    *last = last.min(self.arrive).max(MIN_ARRIVE);
                }
                if self.careful {
                    radii.iter_mut().for_each(|r| *r = MIN_ARRIVE);
                }
                let mut route = Route::new(points, self.pace, false);
                route.radii = radii;
                self.route = (n > 0).then_some(route);
                self.state = State::Following;
            }
            Err(e) => {
                self.retry_at = now + RETRY_AFTER;
                self.error(now, &format!("{e:?}"));
            }
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
        self.state = if self.errors >= GIVE_UP { State::Failed(why.to_string()) } else { State::Planning };
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
        let Some(route) = self.route.as_mut() else {
            return Orders::default();
        };
        // Strayed off the path?
        let next = route.next.min(route.points.len().saturating_sub(1));
        if next > 0 {
            let (a, b) = (flat(route.points[next - 1]), flat(route.points[next]));
            let ab = b - a;
            let t = if ab.length_squared() > 1e-6 { ((flat(feet) - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
            if (flat(feet) - (a + ab * t)).length() > OFF_PATH {
                self.error(now, "off the path");
                return Orders::default();
            }
        }
        let orders = route.orders(feet, yaw);
        if route.finished() {
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
