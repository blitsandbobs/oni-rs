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
    /// What sound passes it while it isn't open, and how much it muffles
    /// that (`sound_allow`, `sound_muffle_fraction`).
    pub sound_allow: SoundAllow,
    pub sound_muffle: f32,
}

/// The sounds a door lets through while it isn't open (`AI2tSoundAllowType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoundAllow {
    All,
    /// Shouts: danger, melee and gunshots.
    Shouting,
    #[default]
    Gunfire,
    None,
}

impl Door {
    /// Whether sound of kind `s` gets past it (`PHrGeneratePath`): open (or
    /// opening, or held open), anything; otherwise what its class allows,
    /// each step beyond counting `1 / (1 − muffle)` times. None: not at all.
    pub fn sound_through(&self, s: crate::ai::perception::Stimulus) -> Option<f32> {
        use crate::ai::perception::Stimulus as S;
        if matches!(self.state, State::Opening(_) | State::Open(_)) {
            return Some(1.0);
        }
        let passes = match self.sound_allow {
            SoundAllow::All => true,
            SoundAllow::Shouting => matches!(s, S::SoundDanger | S::SoundMelee | S::SoundGunshot),
            SoundAllow::Gunfire => s == S::SoundGunshot,
            SoundAllow::None => false,
        };
        if !passes || self.sound_muffle > 0.99 {
            return None;
        }
        Some(if self.sound_muffle > 0.0 {
            1.0 / (1.0 - self.sound_muffle)
        } else {
            1.0
        })
    }
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
        crate::geom::placed_clip_matrix(a, frame, origin)
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
            .min_by(|&a, &b| {
                p.distance_squared(self.leaves[a].w_axis.truncate())
                    .total_cmp(&p.distance_squared(self.leaves[b].w_axis.truncate()))
            })
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
        let (test, ahead) = if frame < step {
            (frame + step, true)
        } else {
            (frame - step, false)
        };
        let last = i32::from(self.anim.as_ref().map_or(LAST_FRAME + 1, |a| a.num_frames));
        if test >= last {
            return;
        }
        // The step's motion (from this frame to the other).
        let delta = self.leaf_matrix(k, test) * self.leaf_matrix(k, frame).inverse();
        let moved = if ahead {
            delta.transform_point3(point) - point
        } else {
            point - delta.transform_point3(point)
        };
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
        let blocked = if leaf0_only {
            self.paused[0] > 0
        } else {
            self.paused.iter().any(|&p| p > 0)
        };
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
                    self.state = if f + 1 >= OPEN_FRAME {
                        State::Open(HOLD_OPEN)
                    } else {
                        State::Opening(f + 1)
                    };
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
    let doc: serde_json::Value = crate::json::load(level_dir.join("doors.json"));
    use crate::json::vec3 as v3;
    // The classes' leaf animations (`common/door_classes.json`).
    let classes: serde_json::Value =
        crate::json::load(level_dir.join("../../common/door_classes.json"));
    let mut anims: std::collections::HashMap<String, Option<Rc<ObjectAnimation>>> =
        Default::default();
    doc["doors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            let has = |f: &str| {
                d["flags"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x == f))
            };
            let events = d["events"]
                .as_array()
                .into_iter()
                .flatten()
                .map(crate::consoles::event)
                .collect();
            let initial_locked = has("initialLocked");
            let class = d["class"].as_str().unwrap_or("").to_string();
            let anim = anims
                .entry(class.clone())
                .or_insert_with(|| object_animation(&classes["classes"][&class]["animation"]))
                .clone();
            let leaves = d["leaves"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| crate::geom::mat4_json(&l["origin"]))
                .collect();
            let c = &classes["classes"][&class];
            let sound_allow = match c["soundAllow"].as_str() {
                Some("all") => SoundAllow::All,
                Some("shouting") => SoundAllow::Shouting,
                Some("none") => SoundAllow::None,
                _ => SoundAllow::Gunfire,
            };
            let sound_muffle = c["soundMuffle"].as_f64().unwrap_or(0.5) as f32;
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
                sound_allow,
                sound_muffle,
            }
        })
        .collect()
}

/// An object animation from `object_animations.json`'s form.
pub fn object_animation(j: &serde_json::Value) -> Option<Rc<ObjectAnimation>> {
    let keys = j["keyframes"].as_array()?;
    let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
    let flags = j["flags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| {
            oni_schema::OBJECT_ANIMATION_FLAGS
                .iter()
                .position(|k| Some(*k) == n.as_str())
        })
        .fold(0, |acc, b| acc | (1 << b));
    let scale = j["scale"]
        .as_array()
        .map(|a| std::array::from_fn(|i| a.get(i).map_or(0.0, f)))
        .unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
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
    let by_object: std::collections::HashMap<u32, &Door> =
        doors.iter().map(|d| (d.object_id, d)).collect();
    let mut out = Vec::with_capacity(scene.prims.len());
    for p in std::mem::take(&mut scene.prims) {
        let Some(d) = p
            .door
            .and_then(|id| by_object.get(&id))
            .filter(|d| d.leaves.len() > 1)
        else {
            out.push(p);
            continue;
        };
        let mut parts: [Vec<u32>; 2] = Default::default();
        for t in p.indices.chunks_exact(3) {
            let c = t.iter().fold(Vec3::ZERO, |a, &i| {
                a + Vec3::from(p.vertices[i as usize].pos)
            }) / 3.0;
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
            sound_allow: SoundAllow::Gunfire,
            sound_muffle: 0.5,
        }
    }

    #[test]
    fn locking_sounds_only_on_a_change_and_manual_doors_are_picked_by_facing() {
        let mut doors = Doors {
            list: vec![
                door(),
                Door {
                    id: 2,
                    position: Vec3::new(0.0, 0.0, -10.0),
                    ..door()
                },
            ],
            ..Default::default()
        };
        assert_eq!(doors.set(1, Some(true), None), [("doorLock", Vec3::ZERO)]);
        assert!(doors.set(1, Some(true), None).is_empty(), "already locked");
        assert_eq!(
            doors.set(-1, Some(false), None).len(),
            1,
            "every door: one changed"
        );
        for d in &mut doors.list {
            d.manual = true;
        }
        // Facing +Z from just behind door 1: it, not door 2 behind.
        let at = Vec3::new(0.0, 0.0, -5.0);
        assert_eq!(doors.manual_in_reach(at, 0.0, at), Some((0, true)));
        doors.list[0].locked = true;
        assert_eq!(
            doors.manual_in_reach(at, 0.0, at),
            Some((0, false)),
            "locked, still the one faced"
        );
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
        assert_eq!(
            d.state,
            State::Closing(OPEN_FRAME + 1),
            "someone near doesn't turn a closing door back"
        );
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
        assert_eq!(
            reopen,
            Some(REVERSE_FRAMES as usize - 1),
            "after 90 ticks it opens for them"
        );
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
        assert_eq!(
            d.state,
            State::Closing(60),
            "the second leaf still holds it"
        );
    }

    #[test]
    fn locks_and_one_way_sides() {
        let mut d = door();
        d.locked = true;
        assert!(!d.may_open(Vec3::Z, false));
        assert!(d.may_open(Vec3::Z, true), "a script ignores locks");
        d.one_way = true;
        assert!(
            d.may_open(Vec3::Z, false) != d.may_open(-Vec3::Z, false),
            "one side only"
        );
        d.jammed = true;
        assert!(!d.may_open(Vec3::Z, false));
    }
}

/// A door leaf's box in its own frame (at its closed position, frame 0).
#[derive(Clone, Copy, Debug)]
struct LeafBox {
    door: usize,
    leaf: usize,
    lo: Vec3,
    hi: Vec3,
}

/// The level's doors and what is derived from them: their closed leaves'
/// geometry, their leaves' boxes (in their own frames, and placed where
/// they stand this tick), their flat shades, and their links to the
/// pathfinding graph's connections.
#[derive(Default)]
pub struct Doors {
    pub list: Vec<Door>,
    /// Each door's closed leaves (its level geometry: vertices, indices).
    meshes: Vec<(Vec<[f32; 3]>, Vec<u32>)>,
    /// Each door's flat shade (`ONrLevel_ObjectGunk_GetShade`), by object id.
    pub shades: std::collections::HashMap<u32, [f32; 3]>,
    /// The leaves' boxes where they stand this tick (solid while their door
    /// isn't closed).
    pub boxes: Vec<crate::chrphys::DoorBox>,
    /// Each leaf's box in its own frame (what `boxes` places).
    leaf_boxes: Vec<LeafBox>,
    /// Each door's connections in the pathfinding graph.
    links: Vec<Vec<(usize, usize)>>,
    /// Whether each door's quads were last marked solid.
    applied: Vec<bool>,
}

impl Doors {
    /// The level's doors (`doors.json`), nothing derived yet.
    pub fn load(level_dir: &Path) -> Doors {
        Doors {
            list: load(level_dir),
            ..Default::default()
        }
    }

    /// Gives the doors their closed leaves (by object id, from the level's
    /// geometry): while a door is closed they are its level quads; while it
    /// moves or stands open each leaf is a box in its own frame (`OBrInit`'s
    /// bounds: the geometry's), moving with it.
    pub fn attach(
        &mut self,
        mut meshes: std::collections::HashMap<u32, (Vec<[f32; 3]>, Vec<u32>)>,
        quads: &mut crate::quads::Quads,
    ) {
        self.meshes = self
            .list
            .iter()
            .map(|d| meshes.remove(&d.object_id).unwrap_or_default())
            .collect();
        self.leaf_boxes.clear();
        for (k, (d, (v, idx))) in self.list.iter().zip(&self.meshes).enumerate() {
            for l in 0..d.leaves.len() {
                let inv = d.leaf_matrix(l, 0).inverse();
                // (Each triangle to the leaf nearest its middle, as drawn.)
                let pts: Vec<Vec3> = idx
                    .chunks_exact(3)
                    .map(|t| [0, 1, 2].map(|j| Vec3::from(v[t[j] as usize])))
                    .filter(|t| d.leaf_of((t[0] + t[1] + t[2]) / 3.0) == l)
                    .flatten()
                    .map(|p| inv.transform_point3(p))
                    .collect();
                if pts.is_empty() {
                    continue;
                }
                let (lo, hi) = pts.iter().fold(
                    (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                    |(lo, hi), p| (lo.min(*p), hi.max(*p)),
                );
                self.leaf_boxes.push(LeafBox {
                    door: k,
                    leaf: l,
                    lo,
                    hi,
                });
            }
        }
        self.place_boxes();
        self.mark_quads(quads);
    }

    /// Each door's flat shade from the level's quads (once they are loaded).
    pub fn shade_from(&mut self, quads: &crate::quads::Quads) {
        self.shades = self
            .list
            .iter()
            .map(|d| (d.object_id, shade(quads, d.object_id)))
            .collect();
    }

    /// The leaves' boxes where they stand this tick: solid while their door
    /// isn't closed (its leaf objects exist).
    pub fn place_boxes(&mut self) {
        self.boxes = self
            .leaf_boxes
            .iter()
            .map(|b| {
                let d = &self.list[b.door];
                let m = d.leaf_matrix(b.leaf, d.frame());
                let axes = [
                    m.x_axis.truncate(),
                    m.y_axis.truncate(),
                    m.z_axis.truncate(),
                ];
                let len = Vec3::new(axes[0].length(), axes[1].length(), axes[2].length());
                crate::chrphys::DoorBox {
                    center: m.transform_point3((b.lo + b.hi) * 0.5),
                    axes: axes.map(|a| a.normalize_or_zero()),
                    half: (b.hi - b.lo) * 0.5 * len,
                    solid: d.state != State::Closed,
                }
            })
            .collect();
    }

    /// The leaves (object id, leaf) of the doors that aren't closed whose
    /// boxes the segment from `a` to `b` meets.
    pub fn leaves_met(&self, a: Vec3, b: Vec3) -> Vec<(u32, u8)> {
        self.leaf_boxes
            .iter()
            .zip(&self.boxes)
            .filter(|(_, w)| w.solid && w.meets_segment(a, b))
            .map(|(l, _)| (self.list[l.door].object_id, l.leaf as u8))
            .collect()
    }

    /// Door `id`'s leaves this tick: each one's box (centre, half sizes),
    /// whether it is solid, and the ticks it stays paused.
    pub fn leaves(&self, id: u16) -> Vec<(Vec3, Vec3, bool, u16)> {
        self.leaf_boxes
            .iter()
            .zip(&self.boxes)
            .filter(|(b, _)| self.list[b.door].id == id)
            .map(|(b, w)| {
                (
                    w.center,
                    w.half,
                    w.solid,
                    self.list[b.door].paused[b.leaf.min(1)],
                )
            })
            .collect()
    }

    /// How each moving door leaf (object id, leaf) has moved from closed,
    /// for drawing.
    pub fn motions(&self) -> std::collections::HashMap<(u32, u8), Mat4> {
        self.list
            .iter()
            .filter(|d| d.state != State::Closed)
            .flat_map(|d| {
                (0..d.leaves.len()).map(move |l| ((d.object_id, l as u8), d.leaf_motion(l)))
            })
            .collect()
    }

    /// Characters touched door leaves this tick (`OBrNotifyPhysicsCollision`;
    /// `feet`: where each toucher stands): a leaf moving into one in front of
    /// it pauses.
    pub fn touched(
        &mut self,
        touches: &[(usize, crate::chrphys::Touch)],
        feet: impl Fn(usize) -> Vec3,
    ) {
        for &(who, t) in touches {
            let Some(b) = t.door.and_then(|k| self.leaf_boxes.get(k)) else {
                continue;
            };
            let d = &mut self.list[b.door];
            let was = d.paused[b.leaf.min(1)];
            d.touched(b.leaf, t.point, t.normal, feet(who));
            if d.paused[b.leaf.min(1)] > was {
                d.blocker = Some(who);
            }
        }
    }

    /// Each door's own level quads — its closed leaves, the "gunk" — as its
    /// state has them (`OBJrDoor_HideShowGunk`): solid and seen while it is
    /// closed; invisible and passed through from the moment it starts to open
    /// until it has shut. (Exported without collision, so the pathfinding grid
    /// ignores them; a door's reset shows them.)
    pub fn mark_quads(&mut self, quads: &mut crate::quads::Quads) {
        let shown: std::collections::HashMap<u32, bool> =
            self.list.iter().map(|d| (d.object_id, d.solid())).collect();
        let hide = crate::quads::flag::NO_COLLISION | crate::quads::flag::INVISIBLE;
        for q in &mut quads.quads {
            if q.object_tag >> 24 != 3 {
                continue;
            }
            if let Some(&show) = shown.get(&(q.object_tag & 0x00FF_FFFF)) {
                q.flags = if show {
                    q.flags & !hide
                } else {
                    q.flags | hide
                };
            }
        }
        self.applied = self.list.iter().map(|d| d.solid()).collect();
    }

    /// Whether door `k`'s quads no longer match its state.
    pub fn quads_stale(&self, k: usize) -> bool {
        self.applied
            .get(k)
            .is_some_and(|&was| was != self.list[k].solid())
    }

    /// Each door's portals, linked as the original links them as it builds
    /// the graph (`OBJrDoor_MakeConnectionLink`; `docs/PATHFINDING.md`,
    /// *Doors*): each connection (room by room, portal by portal) takes the
    /// first door whose base is within half the connection's width of the
    /// connection's base midpoint (in 3D) and that will have it. A door keeps
    /// two slots — not two connections the same way; a nearer one (by 10 %)
    /// or a narrower one about as near replaces a slot's; the dual of the
    /// other slot about as near and wide too. One replaced keeps its link to
    /// the door (the original never clears it): so a door can gate more than
    /// two connections, and some only one way.
    pub fn portals(&mut self, nav: &crate::nav::Navigation) -> &Vec<Vec<(usize, usize)>> {
        if self.links.len() != self.list.len() {
            type Slot = Option<((usize, usize), f32, f32)>; // (connection, dist², width)
            let mut slots: Vec<[Slot; 2]> = vec![[None, None]; self.list.len()];
            let mut links: Vec<Vec<(usize, usize)>> = vec![Vec::new(); self.list.len()];
            let ends = |rk: (usize, usize)| (rk.0, nav.rooms[rk.0].portals[rk.1].to);
            for (r, room) in nav.rooms.iter().enumerate() {
                for (k, p) in room.portals.iter().enumerate() {
                    let conn = (r, k);
                    let (from, to) = (r, p.to);
                    for (di, d) in self.list.iter().enumerate() {
                        let dist_sq = d.position.distance_squared(p.mid);
                        if dist_sq > (p.width * 0.5).powi(2) {
                            continue;
                        }
                        let s = &slots[di];
                        let same = |o: &Slot| o.is_some_and(|(c, _, _)| ends(c) == (from, to));
                        let dual = |o: &Slot| o.is_some_and(|(c, _, _)| ends(c) == (to, from));
                        let (ex0, ex1) = (same(&s[1]), same(&s[0]));
                        let field = |i: usize| s[i].map_or((0.0, 0.0), |(_, ds, w)| (ds, w));
                        let ((d0, w0), (d1, w1)) = (field(0), field(1));
                        let index = if !ex0 && s[0].is_none() {
                            Some(0)
                        } else if !ex1 && s[1].is_none() {
                            Some(1)
                        } else if !ex0 && d0 > 1.1 * dist_sq {
                            Some(0)
                        } else if !ex1 && d1 > 1.1 * dist_sq {
                            Some(1)
                        } else if !ex0 && d0 > 0.9 * dist_sq && w0 > p.width {
                            Some(0)
                        } else if !ex1 && d1 > 0.9 * dist_sq && w1 > p.width {
                            Some(1)
                        } else if !ex0 && d0 > 0.9 * dist_sq && w0 > 0.9 * p.width && dual(&s[1]) {
                            Some(0)
                        } else if !ex1 && d1 > 0.9 * dist_sq && w1 > 0.9 * p.width && dual(&s[0]) {
                            Some(1)
                        } else {
                            None
                        };
                        let Some(index) = index else { continue }; // this door won't: the next may
                        slots[di][index] = Some((conn, dist_sq, p.width));
                        links[di].push(conn);
                        break; // a connection links to one door
                    }
                }
            }
            self.links = links;
        }
        &self.links
    }

    /// The closed manual door in reach of someone with their pelvis at
    /// `pelvis`, facing `facing`, feet at `feet`, that they face most —
    /// preferring one they may open (`OBJrDoor_TryDoorAction`): its index
    /// and whether they may open it.
    pub fn manual_in_reach(&self, pelvis: Vec3, facing: f32, feet: Vec3) -> Option<(usize, bool)> {
        self.list
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.manual && d.state == State::Closed && feet.distance(d.position) < d.radius
            })
            .filter_map(|(k, d)| {
                let off =
                    crate::angle::relative(crate::angle::yaw_of(d.position - pelvis) - facing)
                        .abs();
                (off <= MANUAL_FACING).then_some((k, off, d.may_open(feet, false)))
            })
            .min_by(|a, b| b.2.cmp(&a.2).then(a.1.total_cmp(&b.1)))
            .map(|(k, _, may)| (k, may))
    }

    /// Locks / unlocks / jams / unjams door `id` (`ALL` / -1: every door);
    /// the sounds this makes (a lock or unlock that changes something).
    pub fn set(
        &mut self,
        id: i64,
        lock: Option<bool>,
        jam: Option<bool>,
    ) -> Vec<(&'static str, Vec3)> {
        let mut sounds = Vec::new();
        for d in self
            .list
            .iter_mut()
            .filter(|d| id < 0 || id == 0xFFFF || i64::from(d.id) == id)
        {
            if let Some(l) = lock {
                if d.locked != l {
                    sounds.push((if l { "doorLock" } else { "doorUnlock" }, d.position));
                }
                d.locked = l;
            }
            if let Some(j) = jam {
                d.jammed = j;
            }
        }
        sounds
    }
}

/// A door's flat shade (`ONrLevel_ObjectGunk_GetShade`): the average of
/// its level quads' corner shades (a triangle's three), counting only
/// corners with a component over 0x3F, rounded; none such, 0x3F grey;
/// no quads, white.
fn shade(quads: &crate::quads::Quads, object_id: u32) -> [f32; 3] {
    const MIN: u32 = 0x3F;
    let tag = (3 << 24) | (object_id & 0x00FF_FFFF);
    let quads: Vec<_> = quads.quads.iter().filter(|q| q.object_tag == tag).collect();
    if quads.is_empty() {
        return [1.0; 3];
    }
    let (mut sum, mut n) = ([0u32; 3], 0u32);
    for q in quads {
        let corners = if q.flags & crate::quads::flag::TRIANGLE != 0 {
            3
        } else {
            4
        };
        for &c in &q.shades[..corners] {
            let rgb = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF];
            if rgb.iter().any(|&v| v > MIN) {
                for k in 0..3 {
                    sum[k] += rgb[k];
                }
                n += 1;
            }
        }
    }
    let avg = if n > 0 {
        sum.map(|v| (v + n / 2) / n)
    } else {
        [MIN; 3]
    };
    avg.map(|v| v as f32 / 255.0)
}
