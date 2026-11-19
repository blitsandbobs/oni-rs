//! Doors (behaviour per `docs/DOORS.md`; data `levels/NN/doors.json`): a
//! door opens for anyone allowed through who comes within its activation
//! radius (a manual one only by the Action key), stays open at least 3 s and
//! while someone allowed is near, then closes. Locked doors open for nobody
//! (one-way doors from their open side); jammed ones stay as they are.
//! While closed its own level quads are its leaves; while it moves and stands
//! open, its leaves are objects animated by the class's clip, each a box that
//! characters collide with and that pauses when one is in its way.

use std::path::Path;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use oni_geom::objanim::ObjectAnimation;

use crate::consoles::Event;

/// Door animations: open at this frame, closed again at the last.
pub const OPEN_FRAME: u16 = 50;
pub const LAST_FRAME: u16 = 100;
/// It stays open at least this long.
const HOLD_OPEN: u32 = 180;
/// A manual door is used within this angle of where its user faces.
pub const MANUAL_FACING: f32 = 1.4;
/// A leaf a character is in the way of stays still this long after
/// (`OBcMinPausedFrames`).
pub const PAUSE_FRAMES: u16 = 5;
/// Blocked this long, a door turns back (`OBcDoorReverseFrames`) and stops
/// sensing who is near for a while (`OBcDoor_CollisionProxDisable`); blocked
/// this long while opening, it gives up being blocked
/// (`OBcDoorTotallyStuckFrames`).
const REVERSE_FRAMES: u32 = 90;
const PROXIMITY_OFF: u32 = 180;
const STUCK_FRAMES: u32 = 420;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Closed,
    /// Opening, at this animation frame (0 → 50).
    Opening(u16),
    /// Open; held for this many more ticks.
    Open(u32),
    /// Closing, at this frame (50 → 100).
    Closing(u16),
}

#[derive(Clone, Debug)]
pub struct Door {
    pub id: u16,
    /// Its object's id (which names its geometry).
    pub object_id: u32,
    pub class: String,
    pub position: Vec3,
    /// Its rotation (unit columns).
    pub axes: [Vec3; 3],
    pub radius: f32,
    /// The key that opens it locked (nobody carries keys yet).
    pub key: Option<u16>,
    pub manual: bool,
    pub one_way: bool,
    pub reverse: bool,
    pub locked: bool,
    pub jammed: bool,
    pub events: Vec<Event>,
    pub state: State,
    /// Its moving leaves' origins (one, or two for double doors).
    pub leaves: Vec<Mat4>,
    /// The class's leaf animation (`OBtAnimation`).
    pub anim: Option<Rc<ObjectAnimation>>,
    /// Ticks each leaf stays paused by a character in its way.
    pub paused: [u16; 2],
    /// Ticks it has been blocked in a row.
    pub blocked: u32,
    /// Blocked too long opening: nothing pauses it until it is open.
    pub give_up: bool,
    /// Ticks it doesn't sense who is near.
    pub proximity_off: u32,
    /// Who last held a leaf still (`pausing_context`).
    pub blocker: Option<usize>,
}

/// What a door's tick asks of the world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tick {
    /// It began to open (its events are due).
    pub started: bool,
    /// Closing and blocked too long: it opens for whoever blocks it, if they
    /// may open it.
    pub reopen: bool,
}

impl Door {
    /// Its leaves' animation frame (`animationFrame`): 0 closed, the open
    /// frame held open.
    pub fn frame(&self) -> i32 {
        i32::from(match self.state {
            State::Closed => 0,
            State::Opening(f) | State::Closing(f) => f,
            State::Open(_) => OPEN_FRAME,
        })
    }

    /// Whether its leaves are moving (their animation running).
    pub fn moving(&self) -> bool {
        matches!(self.state, State::Opening(_) | State::Closing(_))
    }

    /// Leaf `k`'s matrix at `frame` (`PHrPhysics_MaintainMatrix`): the
    /// clip's, from its origin for a localized one.
    pub fn leaf_matrix(&self, k: usize, frame: i32) -> Mat4 {
        let origin = self.leaves.get(k).copied().unwrap_or(Mat4::IDENTITY);
        let Some(a) = &self.anim else { return origin };
        let m = a.matrix(frame);
        let col = |c: usize| glam::Vec4::new(m[c * 3], m[c * 3 + 1], m[c * 3 + 2], if c == 3 { 1.0 } else { 0.0 });
        let m = Mat4::from_cols(col(0), col(1), col(2), col(3));
        if a.flags & 0x10 != 0 {
            origin * m
        } else {
            m
        }
    }

    /// How leaf `k` has moved from where it stands closed (frame 0) to
    /// `frame`. (Its closed leaf in the level is the same geometry there.)
    pub fn leaf_motion_at(&self, k: usize, frame: i32) -> Mat4 {
        self.leaf_matrix(k, frame) * self.leaf_matrix(k, 0).inverse()
    }

    /// How leaf `k` has moved now.
    pub fn leaf_motion(&self, k: usize) -> Mat4 {
        self.leaf_motion_at(k, self.frame())
    }

    /// The leaf a point of the closed door belongs to: the one whose origin
    /// is nearest.
    pub fn leaf_of(&self, p: Vec3) -> usize {
        (0..self.leaves.len())
            .min_by(|&a, &b| p.distance_squared(self.leaves[a].w_axis.truncate()).total_cmp(&p.distance_squared(self.leaves[b].w_axis.truncate())))
            .unwrap_or(0)
    }

    /// A character at `at` touched leaf `k` at `point`, the face's normal
    /// `normal` toward it (`OBrNotifyPhysicsCollision`): if that part of the
    /// leaf is moving toward it (within 60° of the normal) and it stands in
    /// front of the face, the leaf pauses — unless the door has given up.
    pub fn touched(&mut self, k: usize, point: Vec3, normal: Vec3, at: Vec3) {
        if !self.moving() || self.give_up || k >= self.paused.len() {
            return;
        }
        let frame = self.frame();
        let step = 1;
        let (test, ahead) = if frame < step { (frame + step, true) } else { (frame - step, false) };
        let last = i32::from(self.anim.as_ref().map_or(LAST_FRAME + 1, |a| a.num_frames));
        if test >= last {
            return;
        }
        // The step's motion (from this frame to the other).
        let delta = self.leaf_matrix(k, test) * self.leaf_matrix(k, frame).inverse();
        let moved = if ahead { delta.transform_point3(point) - point } else { point - delta.transform_point3(point) };
        if moved.length_squared() < 1e-4 {
            return;
        }
        if moved.normalize().dot(normal) >= 0.5 && (at - point).dot(normal) >= 0.0 {
            self.paused[k] = PAUSE_FRAMES;
        }
    }
    /// Whether its leaf blocks (closed, or not yet moving).
    pub fn solid(&self) -> bool {
        self.state == State::Closed
    }

    /// Whether a character at `at` may open it (`ignore_locks`: a script's
    /// order). Jammed: no; unlocked: yes; one-way: from its open side; a key:
    /// not modelled (nobody carries one yet).
    pub fn may_open(&self, at: Vec3, ignore_locks: bool) -> bool {
        if ignore_locks {
            return true;
        }
        if self.jammed {
            return false;
        }
        if !self.locked {
            return true;
        }
        if self.one_way {
            let side = (at - self.position).dot(self.axes[2]) > 0.0;
            return side != self.reverse;
        }
        let _no_keys_yet = self.key;
        false
    }

    /// Its state by name, for scripts and tests.
    pub fn state_name(&self) -> &'static str {
        match self.state {
            State::Closed => "closed",
            State::Opening(_) => "opening",
            State::Open(_) => "open",
            State::Closing(_) => "closing",
        }
    }

    /// Starts opening (from wherever it is).
    pub fn open(&mut self) {
        self.state = match self.state {
            State::Closed => State::Opening(0),
            // Reversing mid-close: the mirrored frame.
            State::Closing(f) => State::Opening(LAST_FRAME + 1 - f),
            s => s,
        };
    }

    /// Starts closing.
    pub fn close(&mut self) {
        self.state = match self.state {
            State::Open(_) => State::Closing(OPEN_FRAME),
            State::Opening(f) => State::Closing(2 * OPEN_FRAME - f),
            s => s,
        };
    }

    /// One tick (`OBJrDoor_Update` with its leaves' animation). `someone`:
    /// a character allowed through is within reach. `leaf0_only`: the
    /// original counts a door blocked only by its first leaf (it tests that
    /// one twice: Q-door-leaf).
    pub fn tick(&mut self, someone: bool, leaf0_only: bool) -> Tick {
        let mut out = Tick::default();
        // Blocked: a while, and it turns back (closing: for whoever is in
        // the way); a long while opening, and it gives up being blocked.
        let blocked = if leaf0_only { self.paused[0] > 0 } else { self.paused.iter().any(|&p| p > 0) };
        if self.state != State::Closed {
            if blocked {
                self.blocked += 1;
                let opening = matches!(self.state, State::Opening(_) | State::Open(_));
                if self.blocked >= STUCK_FRAMES && opening {
                    self.give_up = true;
                } else if self.blocked >= REVERSE_FRAMES {
                    self.proximity_off = PROXIMITY_OFF;
                    if opening {
                        self.close();
                    } else {
                        out.reopen = true;
                    }
                }
            } else {
                self.blocked = 0;
            }
        }
        // The leaves: paused (either holds both, one following the other),
        // else a frame on.
        let paused = self.paused.iter().any(|&p| p > 0);
        for p in &mut self.paused {
            *p = p.saturating_sub(1);
        }
        // While moving, nothing but the animation; open (after the hold) or
        // closed, it senses who is near.
        let busy = match self.state {
            State::Opening(f) => {
                if !paused {
                    self.state = if f + 1 >= OPEN_FRAME { State::Open(HOLD_OPEN) } else { State::Opening(f + 1) };
                }
                true
            }
            State::Open(t) if t > 0 => {
                self.give_up = false;
                self.state = State::Open(t - 1);
                true
            }
            State::Closing(f) => {
                if paused {
                    true
                } else if f + 1 >= LAST_FRAME {
                    self.state = State::Closed;
                    self.give_up = false;
                    false
                } else {
                    self.state = State::Closing(f + 1);
                    true
                }
            }
            _ => false,
        };
        if busy {
            return out;
        }
        if self.proximity_off > 0 {
            self.proximity_off -= 1;
        } else if !self.jammed {
            match self.state {
                State::Closed if someone && !self.manual => {
                    self.state = State::Opening(0);
                    out.started = true;
                }
                State::Open(_) if !someone => self.state = State::Closing(OPEN_FRAME),
                _ => {}
            }
        }
        out
    }
}

pub fn load(level_dir: &Path) -> Vec<Door> {
    let doc: serde_json::Value = std::fs::read_to_string(level_dir.join("doors.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let v3 = |v: &serde_json::Value| {
        let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
        Vec3::new(f(0), f(1), f(2))
    };
    // The classes' leaf animations (`common/door_classes.json`).
    let classes: serde_json::Value = std::fs::read_to_string(level_dir.join("../../common/door_classes.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut anims: std::collections::HashMap<String, Option<Rc<ObjectAnimation>>> = Default::default();
    doc["doors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            let has = |f: &str| d["flags"].as_array().is_some_and(|a| a.iter().any(|x| x == f));
            let events = d["events"]
                .as_array()
                .into_iter()
                .flatten()
                .map(crate::consoles::event)
                .collect();
            let initial_locked = has("initialLocked");
            let class = d["class"].as_str().unwrap_or("").to_string();
            let anim = anims.entry(class.clone()).or_insert_with(|| object_animation(&classes["classes"][&class]["animation"])).clone();
            let leaves = d["leaves"].as_array().into_iter().flatten().map(|l| matrix(&l["origin"])).collect();
            Door {
                id: d["id"].as_u64().unwrap_or(0) as u16,
                object_id: d["objectId"].as_u64().unwrap_or(0) as u32,
                class,
                position: v3(&d["position"]),
                axes: [v3(&d["axes"][0]), v3(&d["axes"][1]), v3(&d["axes"][2])],
                radius: d["radius"].as_f64().unwrap_or(30.0) as f32,
                key: d["key"].as_u64().map(|k| k as u16),
                manual: has("manual"),
                one_way: has("oneWay"),
                reverse: has("reverse"),
                locked: initial_locked,
                jammed: false,
                events,
                state: State::Closed,
                leaves,
                anim,
                paused: [0; 2],
                blocked: 0,
                give_up: false,
                proximity_off: 0,
                blocker: None,
            }
        })
        .collect()
}

/// A 4×3 column-major matrix (`[f32; 12]` in JSON).
fn matrix(v: &serde_json::Value) -> Mat4 {
    let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    let col = |c: usize| glam::Vec4::new(f(c * 3), f(c * 3 + 1), f(c * 3 + 2), if c == 3 { 1.0 } else { 0.0 });
    Mat4::from_cols(col(0), col(1), col(2), col(3))
}

/// An object animation from `object_animations.json`'s form.
pub fn object_animation(j: &serde_json::Value) -> Option<Rc<ObjectAnimation>> {
    let keys = j["keyframes"].as_array()?;
    let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
    let flags = j["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| oni_schema::OBJECT_ANIMATION_FLAGS.iter().position(|k| Some(*k) == n.as_str()))
        .fold(0, |acc, b| acc | (1 << b));
    let scale = j["scale"].as_array().map(|a| std::array::from_fn(|i| a.get(i).map_or(0.0, f))).unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    Some(Rc::new(ObjectAnimation {
        name: j["name"].as_str().map(str::to_string),
        flags,
        ticks_per_frame: j["ticksPerFrame"].as_u64().unwrap_or(1) as u16,
        num_frames: j["numFrames"].as_u64().unwrap_or(0) as u16,
        door_open_frames: j["doorOpenFrames"].as_u64().unwrap_or(0) as u16,
        keys: keys
            .iter()
            .map(|k| oni_geom::objanim::ObjAnimKey {
                frame: k["frame"].as_i64().unwrap_or(0) as i32,
                translation: std::array::from_fn(|i| f(&k["translation"][i])),
                rotation: std::array::from_fn(|i| f(&k["rotation"][i])),
            })
            .collect(),
        scale,
    }))
}

/// Splits each double door's closed leaves in the level's geometry into its
/// two leaves (each triangle to the leaf it is nearest), so each can be drawn
/// where it moves.
pub fn split_leaves(scene: &mut crate::gltf::Scene, doors: &[Door]) {
    let by_object: std::collections::HashMap<u32, &Door> = doors.iter().map(|d| (d.object_id, d)).collect();
    let mut out = Vec::with_capacity(scene.prims.len());
    for p in std::mem::take(&mut scene.prims) {
        let Some(d) = p.door.and_then(|id| by_object.get(&id)).filter(|d| d.leaves.len() > 1) else {
            out.push(p);
            continue;
        };
        let mut parts: [Vec<u32>; 2] = Default::default();
        for t in p.indices.chunks_exact(3) {
            let c = t.iter().fold(Vec3::ZERO, |a, &i| a + Vec3::from(p.vertices[i as usize].pos)) / 3.0;
            parts[d.leaf_of(c).min(1)].extend_from_slice(t);
        }
        for (k, indices) in parts.into_iter().enumerate().filter(|(_, i)| !i.is_empty()) {
            let mut q = p.clone();
            q.indices = indices;
            q.leaf = k as u8;
            out.push(q);
        }
    }
    scene.prims = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door() -> Door {
        Door {
            id: 1,
            object_id: 1,
            class: "d".into(),
            position: Vec3::ZERO,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            radius: 30.0,
            key: None,
            manual: false,
            one_way: false,
            reverse: false,
            locked: false,
            jammed: false,
            events: Vec::new(),
            state: State::Closed,
            leaves: Vec::new(),
            anim: None,
            paused: [0; 2],
            blocked: 0,
            give_up: false,
            proximity_off: 0,
            blocker: None,
        }
    }

    #[test]
    fn opens_holds_and_closes() {
        let mut d = door();
        assert!(d.tick(true, false).started, "someone near: it opens");
        for _ in 0..OPEN_FRAME {
            d.tick(true, false);
        }
        assert_eq!(d.state, State::Open(HOLD_OPEN));
        for _ in 0..HOLD_OPEN {
            d.tick(false, false);
        }
        assert_eq!(d.state, State::Open(0), "held open 3 s");
        d.tick(true, false);
        assert_eq!(d.state, State::Open(0), "and while someone is near");
        d.tick(false, false);
        assert_eq!(d.state, State::Closing(OPEN_FRAME));
        d.tick(true, false);
        assert_eq!(d.state, State::Closing(OPEN_FRAME + 1), "someone near doesn't turn a closing door back");
        for _ in 0..OPEN_FRAME {
            d.tick(false, false);
        }
        assert!(d.solid(), "closed again");
    }

    #[test]
    fn a_blocked_door_waits_then_turns_back() {
        let mut d = door();
        d.state = State::Closing(60);
        // Someone in the way of its leaf, every tick.
        let mut reopen = None;
        for t in 0..100 {
            d.paused[0] = PAUSE_FRAMES;
            if d.tick(false, false).reopen {
                reopen = Some(t);
                break;
            }
        }
        assert_eq!(d.state, State::Closing(60), "held still while blocked");
        assert_eq!(reopen, Some(REVERSE_FRAMES as usize - 1), "after 90 ticks it opens for them");
        assert_eq!(d.proximity_off, PROXIMITY_OFF);
        // Opening, it closes again instead; a long while, it gives up.
        let mut d = door();
        d.state = State::Opening(20);
        for _ in 0..REVERSE_FRAMES {
            d.paused[0] = PAUSE_FRAMES;
            d.tick(false, false);
        }
        assert!(matches!(d.state, State::Closing(_)), "{:?}", d.state);
        // (The original counts only the first leaf.)
        let mut d = door();
        d.state = State::Closing(60);
        for _ in 0..200 {
            d.paused[1] = PAUSE_FRAMES;
            assert!(!d.tick(false, true).reopen);
        }
        assert_eq!(d.state, State::Closing(60), "the second leaf still holds it");
    }

    #[test]
    fn locks_and_one_way_sides() {
        let mut d = door();
        d.locked = true;
        assert!(!d.may_open(Vec3::Z, false));
        assert!(d.may_open(Vec3::Z, true), "a script ignores locks");
        d.one_way = true;
        assert!(d.may_open(Vec3::Z, false) != d.may_open(-Vec3::Z, false), "one side only");
        d.jammed = true;
        assert!(!d.may_open(Vec3::Z, false));
    }
}
