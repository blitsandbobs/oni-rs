//! Laser triggers (behaviour per `docs/LASERS.md`; data
//! `levels/NN/lasers.json`): emitters sliding along their class's track,
//! blinking on and off, whose beams — when the trigger is active, on and
//! shown — trip on the player and fire the trigger's event list.

use std::path::Path;
use std::rc::Rc;

use glam::Vec3;

use crate::consoles::Event;

/// How far a beam reaches.
pub const BEAM_LENGTH: f32 = 500.0;
/// A drawn beam stops this short of what it hits.
const BEAM_SHORT: f32 = 2.0;
/// Every laser (`-1` in a script).
pub const ALL: i64 = -1;

#[derive(Clone, Debug)]
pub struct Emitter {
    /// Where along the track (frames; the whole part is used).
    pub frame: f32,
    /// +1 or -1.
    pub step: f32,
}

#[derive(Clone, Debug)]
pub struct Laser {
    pub id: u16,
    /// Its class (its sounds).
    pub class: String,
    pub position: Vec3,
    pub axes: [Vec3; 3],
    pub color: [f32; 3],
    start_offset: f32,
    /// Track frames per tick (authored), and as a script has scaled it.
    speed: f32,
    pub current_speed: f32,
    time_on: u32,
    time_off: u32,
    change_time: u32,
    pub laser_on: bool,
    pub active: bool,
    pub hidden: bool,
    pub triggered: bool,
    initial_active: bool,
    reverse: bool,
    ping_pong: bool,
    pub emitters: Vec<Emitter>,
    pub events: Vec<Event>,
    /// Per track frame: the beam's origin and direction (own space).
    track: Rc<Vec<[f32; 6]>>,
    /// Per track frame: how far the beam reaches before the level.
    reach: Vec<Option<f32>>,
    /// Its emitters' eye (`OBJiTrigger_Draw`): the model, and per track
    /// frame the emitter's matrix in the trigger's space.
    eye: Option<Rc<(std::path::PathBuf, Vec<glam::Mat4>)>>,
}

impl Laser {
    fn frames(&self) -> f32 {
        self.track.len().max(1) as f32
    }

    /// Back as the level starts it: emitters spread along the track from
    /// the start offset, laser on, untriggered, active if authored so.
    pub fn reset(&mut self) {
        self.laser_on = true;
        self.change_time = if self.time_on > 0 || self.time_off > 0 {
            self.time_on
        } else {
            0
        };
        let n = self.frames();
        let space = n / self.emitters.len().max(1) as f32;
        for (i, e) in self.emitters.iter_mut().enumerate() {
            e.frame = if self.start_offset == 1.0 {
                n - 1.0
            } else {
                (space * i as f32 + self.start_offset * (n - 1.0)).rem_euclid((n - 1.0).max(1.0))
            };
            e.step = if self.reverse { -1.0 } else { 1.0 };
        }
        self.triggered = false;
        if self.initial_active {
            self.activate();
        } else {
            self.active = false;
        }
        // (The original then updates once and moves the emitters again.)
        self.tick();
        if self.active {
            self.advance();
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.laser_on = true;
        self.change_time = if self.time_on > 0 || self.time_off > 0 {
            self.time_on
        } else {
            0
        };
    }

    fn advance(&mut self) {
        let n = self.frames();
        for e in &mut self.emitters {
            e.frame += e.step * self.current_speed;
            if e.frame >= n {
                if self.ping_pong {
                    e.frame = n - 1.0;
                    e.step = -1.0;
                } else {
                    e.frame = 0.0;
                }
            }
            if e.frame < 0.0 {
                if self.ping_pong {
                    e.frame = 0.0;
                    e.step = 1.0;
                } else {
                    e.frame = n - 1.0;
                }
            }
        }
    }

    /// One tick: the emitters move (while active) and the laser blinks.
    pub fn tick(&mut self) {
        if self.active {
            self.advance();
        }
        if self.change_time > 0 {
            self.change_time -= 1;
        } else if self.laser_on && self.time_off > 0 {
            self.laser_on = false;
            self.change_time = self.time_off;
        } else if !self.laser_on && self.time_on > 0 {
            self.laser_on = true;
            self.change_time = self.time_on;
        }
    }

    /// Whether its beams are out (active, on, shown).
    pub fn beaming(&self) -> bool {
        self.active && self.laser_on && !self.hidden
    }

    /// Its emitters' eyes to draw (unless hidden): the model and, for each
    /// emitter, its matrix in the world — the trigger's placement times its
    /// frame's (`OBJiTrigger_Draw`).
    pub fn eye_draws(&self) -> Vec<(std::path::PathBuf, glam::Mat4)> {
        let Some(eye) = self.eye.as_ref().filter(|_| !self.hidden) else {
            return Vec::new();
        };
        let place = glam::Mat4::from_cols(
            self.axes[0].extend(0.0),
            self.axes[1].extend(0.0),
            self.axes[2].extend(0.0),
            self.position.extend(1.0),
        );
        self.emitters
            .iter()
            .filter_map(|e| {
                let f = (e.frame.max(0.0) as usize).min(eye.1.len().checked_sub(1)?);
                Some((eye.0.clone(), place * eye.1[f]))
            })
            .collect()
    }

    /// Emitter `i`'s beam in the world: origin, direction, and its track
    /// frame (whose reach is cached).
    pub fn beam(&self, i: usize) -> (Vec3, Vec3, usize) {
        let f = (self.emitters[i].frame.max(0.0) as usize).min(self.track.len().saturating_sub(1));
        let t = self
            .track
            .get(f)
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, -1.0, 0.0, 0.0]);
        let to_world =
            |v: [f32; 3]| self.axes[0] * v[0] + self.axes[1] * v[1] + self.axes[2] * v[2];
        (
            self.position + to_world([t[0], t[1], t[2]]),
            to_world([t[3], t[4], t[5]]).normalize_or_zero(),
            f,
        )
    }

    /// The level's reach at track frame `f`, worked out once.
    pub fn reach(&mut self, f: usize, cast: impl FnOnce() -> f32) -> f32 {
        if self.reach.len() <= f {
            self.reach.resize(f + 1, None);
        }
        *self.reach[f].get_or_insert_with(cast)
    }
}

/// The drawn part of a beam that reaches `len`.
pub fn drawn(len: f32) -> f32 {
    (len - BEAM_SHORT).max(0.0)
}

/// Where a ray from `from` along unit `dir` first enters a sphere, within
/// `max` (a start inside counts as 0).
pub fn ray_sphere(from: Vec3, dir: Vec3, max: f32, centre: Vec3, radius: f32) -> Option<f32> {
    let m = from - centre;
    let c = m.length_squared() - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    let b = m.dot(dir);
    let disc = b * b - c;
    if b > 0.0 || disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t <= max).then_some(t)
}

pub fn load(level_dir: &Path) -> Vec<Laser> {
    let doc: serde_json::Value = std::fs::read_to_string(level_dir.join("lasers.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let v3 = |v: &serde_json::Value| {
        let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
        Vec3::new(f(0), f(1), f(2))
    };
    let common = level_dir.join("../../common");
    let mut eyes: std::collections::HashMap<String, Rc<(std::path::PathBuf, Vec<glam::Mat4>)>> =
        Default::default();
    if let Some(e) = doc["eyes"].as_object() {
        for (name, v) in e {
            let Some(model) = v["model"].as_str() else {
                continue;
            };
            let frames = v["frames"]
                .as_array()
                .into_iter()
                .flatten()
                .map(crate::geom::mat4_json)
                .collect();
            eyes.insert(name.clone(), Rc::new((common.join(model), frames)));
        }
    }
    let mut tracks: std::collections::HashMap<String, Rc<Vec<[f32; 6]>>> = Default::default();
    if let Some(cl) = doc["classes"].as_object() {
        for (name, t) in cl {
            let track = t
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| std::array::from_fn(|i| r[i].as_f64().unwrap_or(0.0) as f32))
                .collect();
            tracks.insert(name.clone(), Rc::new(track));
        }
    }
    doc["lasers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            let has = |f: &str| {
                l["flags"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x == f))
            };
            let speed = l["speed"].as_f64().unwrap_or(1.0) as f32;
            let n = l["emitters"].as_u64().unwrap_or(1).clamp(1, 32) as usize;
            let mut laser = Laser {
                id: l["id"].as_u64().unwrap_or(0) as u16,
                position: v3(&l["position"]),
                axes: [v3(&l["axes"][0]), v3(&l["axes"][1]), v3(&l["axes"][2])],
                color: std::array::from_fn(|i| l["color"][i].as_f64().unwrap_or(1.0) as f32),
                start_offset: l["startOffset"].as_f64().unwrap_or(0.0) as f32,
                speed,
                current_speed: speed,
                time_on: l["timeOn"].as_u64().unwrap_or(0) as u32,
                time_off: l["timeOff"].as_u64().unwrap_or(0) as u32,
                change_time: 0,
                laser_on: true,
                active: false,
                hidden: false,
                triggered: false,
                initial_active: has("initialActive"),
                reverse: has("reverseAnim"),
                ping_pong: has("pingPong"),
                emitters: vec![
                    Emitter {
                        frame: 0.0,
                        step: 1.0
                    };
                    n
                ],
                events: l["events"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(crate::consoles::event)
                    .collect(),
                track: tracks
                    .get(l["class"].as_str().unwrap_or(""))
                    .cloned()
                    .unwrap_or_default(),
                reach: Vec::new(),
                eye: eyes.get(l["class"].as_str().unwrap_or("")).cloned(),
                class: l["class"].as_str().unwrap_or("").to_string(),
            };
            laser.reset();
            laser
        })
        .collect()
}

impl Laser {
    /// Scales its speed (`trig_speed`).
    pub fn set_speed(&mut self, factor: f32) {
        self.current_speed = self.speed * factor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laser(ping_pong: bool) -> Laser {
        let track: Vec<[f32; 6]> = (0..101)
            .map(|f| [0.0, 0.0, f as f32 * 0.3, -1.0, 0.0, 0.0])
            .collect();
        let mut l = Laser {
            class: String::new(),
            id: 1,
            position: Vec3::ZERO,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            color: [1.0, 0.0, 0.0],
            start_offset: 0.0,
            speed: 0.5,
            current_speed: 0.5,
            time_on: 0,
            time_off: 0,
            change_time: 0,
            laser_on: true,
            active: false,
            hidden: false,
            triggered: false,
            initial_active: false,
            reverse: false,
            ping_pong,
            emitters: vec![Emitter {
                frame: 0.0,
                step: 1.0,
            }],
            events: Vec::new(),
            track: Rc::new(track),
            reach: Vec::new(),
            eye: None,
        };
        l.reset();
        l
    }

    #[test]
    fn emitters_sweep_only_while_active_and_ping_pong() {
        let mut l = laser(true);
        assert!(!l.active);
        l.tick();
        assert_eq!(l.emitters[0].frame, 0.0, "inactive: still");
        l.activate();
        for _ in 0..200 {
            l.tick();
        }
        assert!(
            l.emitters[0].frame > 99.0,
            "half a frame a tick: across in 200 ticks"
        );
        l.tick();
        l.tick();
        assert_eq!(l.emitters[0].step, -1.0, "and back");
        let mut w = laser(false);
        w.activate();
        for _ in 0..203 {
            w.tick();
        }
        assert!(w.emitters[0].frame < 2.0, "without ping-pong it wraps");
    }

    #[test]
    fn blinks_on_and_off() {
        let mut l = laser(true);
        l.time_on = 3;
        l.time_off = 2;
        l.activate();
        let seq: Vec<bool> = (0..12)
            .map(|_| {
                l.tick();
                l.laser_on
            })
            .collect();
        assert_eq!(
            seq,
            [true, true, true, false, false, false, true, true, true, true, false, false]
        );
    }

    #[test]
    fn rays_hit_spheres_in_front_only() {
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(10.0, 0.0, 0.0), 2.0),
            Some(8.0)
        );
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::X, 5.0, Vec3::new(10.0, 0.0, 0.0), 2.0),
            None
        );
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(-10.0, 0.0, 0.0), 2.0),
            None
        );
        assert_eq!(
            ray_sphere(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(10.0, 3.0, 0.0), 2.0),
            None
        );
    }
}
