//! Steering round what an AI bumps into (`Oni_AI2_MovementState.c`: the
//! collision records, their ageing, and the "badness" ring).
//! Each wall, door leaf or character the
//! character's movement meets is noted; one met for a while weighs against
//! moving toward it, and the AI moves in the least bad direction near the one
//! it wants. Met for 30 frames running, the AI is stuck against it and its
//! path fails.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

/// At most this many records (`AI2cMovementState_MaxCollisions`).
const MAX_COLLISIONS: usize = 16;
/// Planes steeper than this (low ceilings) are left out.
const MAX_VERTICAL: f32 = 0.9;
/// Met fewer frames than this, a record weighs nothing.
const IGNORE_FRAMES: u32 = 3;
/// It weighs most (this much) once met this many frames.
const MAX_URGENCY_FRAMES: u32 = 30;
const MAX_URGENCY_WEIGHT: f32 = 0.5;
/// Met this many frames, still being met: stuck.
const ERROR_FRAMES: u32 = 30;
/// A record no longer met fades from this far away and this long after,
/// and is gone by these.
const START_FORGET_DISTANCE: f32 = 10.0;
const TOTALLY_FORGET_DISTANCE: f32 = 20.0;
const START_FORGET_FRAMES: u32 = 30;
const TOTALLY_FORGET_FRAMES: u32 = 90;
/// Gone once this far behind its plane; fading out beyond the body's radius
/// to this distance in front.
const INSIDE_FORGET_DISTANCE: f32 = 5.0;
const COMFORT_DISTANCE: f32 = 10.0;
/// Moving away from a plane by more than this (the dot of the move with its
/// normal), its record weighs nothing.
const PARALLEL_TOLERANCE: f32 = 0.2;
/// A record weighs fully within 30° of its direction, fading to nothing at
/// 90°.
const BADNESS_ANGLE: f32 = 30.0 * PI / 180.0;
const BADNESS_MAX_ANGLE: f32 = 90.0 * PI / 180.0;
/// How much turning away from the wanted direction counts, per radian.
const HEURISTIC_THETA: f32 = 0.2;
/// At most this many points on the badness ring.
const MAX_BADNESS_VALUES: usize = 4 * MAX_COLLISIONS;
/// An AI's leaf sphere (`ONrCharacter_GetLeafSphereRadius`).
const AI_LEAF_RADIUS: f32 = 3.0;

/// What a collision was with (`AI2tCollisionIdentifier`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum With {
    /// A level quad's plane (its index; the top bit: the reversed side).
    Wall(u32),
    /// A moving object (a door leaf's box).
    Object(usize),
    /// Another character.
    Character(usize),
}

#[derive(Clone, Copy, Debug)]
struct Collision {
    with: With,
    hit: bool,
    miss: bool,
    serious: bool,
    frames: u32,
    total_hit_frames: u32,
    /// The plane (normal, d).
    normal: Vec3,
    d: f32,
    /// Toward the obstacle (the plane's reversed normal), radians.
    direction: f32,
    weight: f32,
    last_hit_point: Vec3,
    last_collision_point: Vec3,
}

/// One character's collision records (`AI2tMovementState`'s).
#[derive(Clone, Debug, Default)]
pub struct Avoid {
    records: Vec<Collision>,
    /// The way it means to move this tick (radians), if it moves.
    move_direction: Option<f32>,
    /// Wedged against something: its path should fail.
    pub stuck: bool,
}

use crate::angle::wrap_2pi as wrap;

/// The yaw of a horizontal direction (x, z).
fn yaw(x: f32, z: f32) -> f32 {
    wrap(x.atan2(z))
}

impl Avoid {
    /// Its movement collided with `with` at `point`, the plane's normal
    /// `normal` (toward the character) and `d` (`AI2rMovementState_Notify*`):
    /// not when it isn't moving, nor for near-horizontal planes, nor for
    /// planes it is behind.
    pub fn notify(&mut self, with: With, point: Vec3, normal: Vec3, d: f32, feet: Vec3) {
        if self.move_direction.is_none()
            || normal.y.abs() > MAX_VERTICAL
            || feet.dot(normal) + d < 0.0
        {
            return;
        }
        // (A wall is the same record by its plane; its quad may change.)
        let k = match self.records.iter().position(|c| c.with == with) {
            Some(k) => {
                let c = &mut self.records[k];
                if c.miss {
                    c.miss = false;
                    c.frames = 0;
                }
                k
            }
            None => {
                if self.records.len() >= MAX_COLLISIONS {
                    return;
                }
                self.records.push(Collision {
                    with,
                    hit: false,
                    miss: false,
                    serious: false,
                    frames: 0,
                    total_hit_frames: 0,
                    normal,
                    d,
                    direction: yaw(-normal.x, -normal.z),
                    weight: 0.0,
                    last_hit_point: feet,
                    last_collision_point: point,
                });
                self.records.len() - 1
            }
        };
        let c = &mut self.records[k];
        c.hit = true;
        c.last_hit_point = feet;
        c.last_collision_point = point;
        c.total_hit_frames += 1;
    }

    /// Ages the records and reweighs them (`AI2iMovementState_AgeCollisions`).
    fn age(&mut self, feet: Vec3) {
        let move_direction = self.move_direction;
        let mut stuck = false;
        self.records.retain_mut(|c| {
            if !c.hit && !c.miss {
                c.miss = true;
                c.frames = 0;
            }
            c.frames += 1;
            let (dist_weight, time_weight) = if c.hit {
                (1.0, 1.0)
            } else {
                let dist = feet.distance(c.last_hit_point);
                if dist > TOTALLY_FORGET_DISTANCE || c.frames >= TOTALLY_FORGET_FRAMES {
                    return false;
                }
                let dw = if dist > START_FORGET_DISTANCE {
                    (TOTALLY_FORGET_DISTANCE - dist)
                        / (TOTALLY_FORGET_DISTANCE - START_FORGET_DISTANCE)
                } else {
                    1.0
                };
                let tw = if c.frames >= START_FORGET_FRAMES {
                    (TOTALLY_FORGET_FRAMES - c.frames) as f32
                        / (TOTALLY_FORGET_FRAMES - START_FORGET_FRAMES) as f32
                } else {
                    1.0
                };
                (dw, tw)
            };
            let urgency = if c.total_hit_frames <= IGNORE_FRAMES {
                0.0
            } else {
                if c.hit {
                    c.serious = true;
                }
                if c.total_hit_frames < MAX_URGENCY_FRAMES {
                    MAX_URGENCY_WEIGHT * (c.total_hit_frames - IGNORE_FRAMES) as f32
                        / (MAX_URGENCY_FRAMES - IGNORE_FRAMES) as f32
                } else {
                    if c.hit && c.total_hit_frames >= ERROR_FRAMES {
                        stuck = true;
                    }
                    MAX_URGENCY_WEIGHT
                }
            };
            c.weight = urgency * dist_weight * time_weight;
            if c.weight > 0.0 && !c.hit {
                // (Its distance from the plane, measured at the height it was
                // touched.)
                let p = Vec3::new(feet.x, c.last_collision_point.y, feet.z);
                let dist = p.dot(c.normal) + c.d;
                if dist < -INSIDE_FORGET_DISTANCE {
                    return false;
                }
                if dist > 0.0 {
                    let comfort = (dist - AI_LEAF_RADIUS) / COMFORT_DISTANCE;
                    if comfort > 1.0 {
                        c.weight = 0.0;
                    } else if comfort > 0.0 {
                        match move_direction {
                            None => c.weight = 0.0,
                            Some(m)
                                if m.sin() * c.normal.x + m.cos() * c.normal.z
                                    > PARALLEL_TOLERANCE =>
                            {
                                c.weight = 0.0
                            }
                            Some(_) => c.weight *= 1.0 - comfort,
                        }
                    }
                } else {
                    c.weight *= 1.0 + dist / INSIDE_FORGET_DISTANCE;
                }
            }
            true
        });
        if stuck {
            self.stuck = true;
        }
    }

    /// One tick of movement (`AI2rMovementState_Update`): ages the records,
    /// then — moving with something in the way — turns the way it moves to
    /// the least bad direction near the one it wants. `wanted`: the way it
    /// means to move (radians), if it moves. Returns the way to move. (The
    /// original also steers round what suppresses its movement modifiers,
    /// which aren't modelled.)
    pub fn steer(&mut self, wanted: Option<f32>, feet: Vec3) -> Option<f32> {
        self.move_direction = wanted.map(wrap);
        self.age(feet);
        if !self.records.is_empty() {
            if let Some(m) = self.move_direction {
                let mut ring = Ring::new();
                for c in &self.records {
                    ring.add(c);
                }
                self.move_direction = Some(ring.least_bad(m));
            }
        }
        for c in &mut self.records {
            c.hit = false;
        }
        self.move_direction
    }

    /// How many records it holds, and how many are serious.
    pub fn count(&self) -> (usize, usize) {
        (
            self.records.len(),
            self.records.iter().filter(|c| c.serious).count(),
        )
    }
}

/// A point on the badness ring (`AI2tBadnessValue`).
#[derive(Clone, Copy, Debug)]
struct Value {
    angle: f32,
    value: f32,
    increment: f32,
    next: usize,
}

/// The badness of each direction, piecewise linear round the circle
/// (`AI2iMovementState_*Badness*`): a ring of points sorted by angle, from
/// 0 to 2π.
struct Ring {
    values: Vec<Value>,
}

impl Ring {
    fn new() -> Ring {
        Ring {
            values: vec![
                Value {
                    angle: 0.0,
                    value: 0.0,
                    increment: 0.0,
                    next: 1,
                },
                Value {
                    angle: TAU,
                    value: 0.0,
                    increment: 0.0,
                    next: 0,
                },
            ],
        }
    }

    /// Adds a record's badness: rising from 90° before its direction to its
    /// weight at 30° before, level to 30° after, falling to nothing at 90°.
    fn add(&mut self, c: &Collision) {
        if c.weight == 0.0 {
            return;
        }
        let clip_low = |a: f32| if a < 0.0 { a + TAU } else { a };
        let clip_high = |a: f32| if a >= TAU { a - TAU } else { a };
        let begin = clip_low(c.direction - BADNESS_MAX_ANGLE);
        let begincap = clip_low(c.direction - BADNESS_ANGLE);
        let endcap = clip_high(c.direction + BADNESS_ANGLE);
        let end = clip_high(c.direction + BADNESS_MAX_ANGLE);
        let w = c.weight;
        let mut at = 0;
        at = self.create(at, begin, 0.0, 0.0);
        at = self.create(at, begincap, 0.0, w);
        at = self.create(at, endcap, w, w);
        self.create(at, end, w, 0.0);
        for v in &mut self.values {
            v.value += v.increment;
            v.increment = 0.0;
        }
    }

    /// Walks from point `start` to `end_angle`, ramping the increment from
    /// `from` to `to` on the way, and puts a point at the end
    /// (`AI2iMovementState_CreateBadnessValue`). Returns that point.
    fn create(&mut self, start: usize, end_angle: f32, from: f32, to: f32) -> usize {
        let start_angle = self.values[start].angle;
        let mut wrap_first = start_angle > end_angle;
        let range = if wrap_first {
            TAU + end_angle - start_angle
        } else {
            end_angle - start_angle
        };
        let delta = to - from;
        let mut at = start;
        loop {
            if (self.values[at].angle - end_angle).abs() < 1e-6 {
                self.values[at].increment = to;
                return at;
            }
            let remaining = if wrap_first {
                TAU + end_angle - self.values[at].angle
            } else {
                end_angle - self.values[at].angle
            };
            self.values[at].increment = if range != 0.0 {
                to - delta * remaining / range
            } else {
                to
            };
            let next = self.values[at].next;
            if wrap_first {
                if self.values[next].angle < end_angle {
                    wrap_first = false;
                }
            } else if self.values[next].angle > end_angle {
                if self.values.len() >= MAX_BADNESS_VALUES {
                    return at;
                }
                let (a, b) = (self.values[at], self.values[next]);
                let t = (end_angle - a.angle) / (b.angle - a.angle);
                self.values.push(Value {
                    angle: end_angle,
                    value: a.value + t * (b.value - a.value),
                    increment: to,
                    next,
                });
                let new = self.values.len() - 1;
                self.values[at].next = new;
                return new;
            }
            at = next;
            if at == start {
                return at;
            }
        }
    }

    /// The direction near `direction` least bad to move in
    /// (`AI2iMovementState_LeastBadDirection`): each point's badness plus
    /// 0.2 a radian turned away.
    fn least_bad(&mut self, direction: f32) -> f32 {
        self.create(0, direction, 0.0, 0.0);
        let (mut best, mut best_angle) = (1e9f32, 0.0);
        let mut at = 0;
        loop {
            let v = self.values[at];
            if v.next == 0 {
                break;
            }
            let delta = crate::angle::wrap_pi(v.angle - direction);
            let this = v.value + HEURISTIC_THETA * delta.abs();
            if this < best {
                best = this;
                best_angle = v.angle;
            }
            at = v.next;
        }
        best_angle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall ahead (+z), facing back at the character.
    fn wall_ahead(a: &mut Avoid, feet: Vec3) {
        a.notify(
            With::Wall(7),
            feet + Vec3::Z * 4.0,
            -Vec3::Z,
            feet.z + 4.0,
            feet,
        );
    }

    #[test]
    fn a_wall_met_a_while_turns_it_aside() {
        let mut a = Avoid::default();
        let feet = Vec3::ZERO;
        // At first it keeps going straight on.
        let dir = a.steer(Some(0.0), feet).unwrap();
        wall_ahead(&mut a, feet);
        assert!(dir.abs() < 1e-4);
        // Met a few frames it still goes on (turning away costs more than
        // the wall weighs); met nearly 30, it moves off at an angle.
        let mut last = 0.0;
        for k in 0..28 {
            last = a.steer(Some(0.0), feet).unwrap();
            wall_ahead(&mut a, feet);
            if k == 10 {
                assert!(last.abs() < 1e-4, "still straight on");
            }
        }
        let off = crate::angle::wrap_pi(last);
        assert!(off.abs() > 0.5, "turned aside by {off}");
        assert!(!a.stuck);
        for _ in 0..30 {
            a.steer(Some(0.0), feet);
            wall_ahead(&mut a, feet);
        }
        a.steer(Some(0.0), feet);
        assert!(a.stuck, "wedged 30 frames: stuck");
    }

    #[test]
    fn records_are_kept_only_while_moving_and_in_front() {
        let mut a = Avoid::default();
        a.steer(None, Vec3::ZERO);
        wall_ahead(&mut a, Vec3::ZERO);
        assert_eq!(a.count().0, 0, "standing still: not noted");
        a.steer(Some(0.0), Vec3::ZERO);
        a.notify(With::Wall(1), Vec3::ZERO, Vec3::Y, 0.0, Vec3::ZERO);
        assert_eq!(a.count().0, 0, "a floor or ceiling: not noted");
        a.notify(
            With::Wall(2),
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::Z,
            2.0 - 5.0,
            Vec3::ZERO,
        );
        assert_eq!(a.count().0, 0, "behind its plane: not noted");
        wall_ahead(&mut a, Vec3::ZERO);
        assert_eq!(a.count().0, 1);
        // Walked away and left alone, it is forgotten.
        for k in 0..=TOTALLY_FORGET_FRAMES {
            a.steer(Some(PI), Vec3::new(0.0, 0.0, -(k as f32) * 0.1));
        }
        assert_eq!(a.count().0, 0);
    }
}
