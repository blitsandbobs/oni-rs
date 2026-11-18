//! Doors (behaviour per `docs/DOORS.md`; data `levels/NN/doors.json`): a
//! door opens for anyone allowed through who comes within its activation
//! radius (a manual one only by the Action key), stays open at least 3 s and
//! while someone allowed is near, then closes. Locked doors open for nobody
//! (one-way doors from their open side); jammed ones stay as they are.
//! While closed its leaf is solid; it takes 50 ticks to open or close.

use std::path::Path;

use glam::Vec3;

use crate::consoles::Event;

/// Door animations: open at this frame, closed again at the last.
pub const OPEN_FRAME: u16 = 50;
pub const LAST_FRAME: u16 = 100;
/// It stays open at least this long.
const HOLD_OPEN: u32 = 180;
/// A manual door is used within this angle of where its user faces.
pub const MANUAL_FACING: f32 = 1.4;

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
}

impl Door {
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

    /// One tick. `someone`: a character allowed through is within reach.
    /// True if it just started opening (its events are due).
    pub fn tick(&mut self, someone: bool) -> bool {
        match self.state {
            State::Closed => {
                if someone && !self.manual && !self.jammed {
                    self.state = State::Opening(0);
                    return true;
                }
            }
            State::Opening(f) => {
                self.state = if f + 1 >= OPEN_FRAME { State::Open(HOLD_OPEN) } else { State::Opening(f + 1) };
            }
            State::Open(t) if t > 0 => self.state = State::Open(t - 1),
            State::Open(_) => {
                if !someone && !self.jammed {
                    self.state = State::Closing(OPEN_FRAME);
                }
            }
            State::Closing(f) => {
                if someone && !self.jammed {
                    self.open();
                } else if f + 1 >= LAST_FRAME {
                    self.state = State::Closed;
                } else {
                    self.state = State::Closing(f + 1);
                }
            }
        }
        false
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
            Door {
                id: d["id"].as_u64().unwrap_or(0) as u16,
                object_id: d["objectId"].as_u64().unwrap_or(0) as u32,
                class: d["class"].as_str().unwrap_or("").to_string(),
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
            }
        })
        .collect()
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
        }
    }

    #[test]
    fn opens_holds_and_closes() {
        let mut d = door();
        assert!(d.tick(true), "someone near: it opens");
        for _ in 0..OPEN_FRAME {
            d.tick(true);
        }
        assert_eq!(d.state, State::Open(HOLD_OPEN));
        for _ in 0..HOLD_OPEN {
            d.tick(false);
        }
        assert_eq!(d.state, State::Open(0), "held open 3 s");
        d.tick(true);
        assert_eq!(d.state, State::Open(0), "and while someone is near");
        d.tick(false);
        assert_eq!(d.state, State::Closing(OPEN_FRAME));
        for _ in 0..OPEN_FRAME {
            d.tick(false);
        }
        assert!(d.solid(), "closed again");
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
