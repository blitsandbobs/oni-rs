//! Consoles (behaviour per `scripting/BSL.md`, *Consoles*; data
//! `levels/NN/consoles.json`): terminals a character uses with the Action
//! key — it walks up to the console's action marker, plays the console
//! animation, and on its action frame the console fires its event list
//! (scripts, other consoles, alarms, doors, …).

use std::f32::consts::{PI, TAU};
use std::path::Path;

use glam::Vec3;

/// Something an event list does (`Oni_Event`).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Script(String),
    ActivateConsole(u16),
    DeactivateConsole(u16),
    ActivateAlarm(u16),
    LockDoor(u16),
    UnlockDoor(u16),
    ActivateLaser(u16),
    DeactivateLaser(u16),
    /// Turrets: not modelled yet.
    Other(String, u16),
}

/// An event from its JSON form.
pub fn event(e: &serde_json::Value) -> Event {
    let id = e["id"].as_u64().unwrap_or(0) as u16;
    match e["type"].as_str().unwrap_or("") {
        "script" => Event::Script(e["script"].as_str().unwrap_or("").to_string()),
        "activateConsole" => Event::ActivateConsole(id),
        "deactivateConsole" => Event::DeactivateConsole(id),
        "activateAlarm" => Event::ActivateAlarm(id),
        "lockDoor" => Event::LockDoor(id),
        "unlockDoor" => Event::UnlockDoor(id),
        "activateTrigger" => Event::ActivateLaser(id),
        "deactivateTrigger" => Event::DeactivateLaser(id),
        other => Event::Other(other.to_string(), id),
    }
}

#[derive(Clone, Debug)]
pub struct Console {
    pub id: u16,
    pub class: String,
    pub position: Vec3,
    pub marker: Vec3,
    /// The way a user faces it.
    pub facing: f32,
    pub events: Vec<Event>,
    pub initial_active: bool,
    pub punch: bool,
    pub is_alarm: bool,
    pub active: bool,
    pub triggered: bool,
}

/// Every console (`0xFFFF` in an event list).
pub const ALL: u16 = 0xFFFF;

/// A user must stand within these of the marker: height, along its axis
/// (from 2 in front of it to 10 behind), sideways, facing.
const MARKER_HEIGHT: f32 = 8.0;
const MARKER_AHEAD: f32 = -2.0;
const MARKER_BEHIND: f32 = 10.0;
const MARKER_SIDE: f32 = 8.0;
const MARKER_FACING: f32 = 70.0 * PI / 180.0;
/// Walking in: this fast, and close enough within this.
const WALK_IN_STEP: f32 = 0.25;
const WALK_IN_DONE: f32 = 2.0;
const WALK_IN_TICKS: u64 = 180;

pub fn load(level_dir: &Path) -> Vec<Console> {
    let doc: serde_json::Value = std::fs::read_to_string(level_dir.join("consoles.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let v3 = |v: &serde_json::Value| {
        let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
        Vec3::new(f(0), f(1), f(2))
    };
    doc["consoles"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let has = |f: &str| c["flags"].as_array().is_some_and(|a| a.iter().any(|x| x == f));
            let events = c["events"].as_array().into_iter().flatten().map(event).collect();
            let initial_active = has("initialActive");
            Console {
                id: c["id"].as_u64().unwrap_or(0) as u16,
                class: c["class"].as_str().unwrap_or("").to_string(),
                position: v3(&c["position"]),
                marker: v3(&c["marker"]["position"]),
                facing: c["marker"]["facing"].as_f64().unwrap_or(0.0) as f32,
                events,
                initial_active,
                punch: has("punch"),
                is_alarm: has("isAlarm"),
                active: initial_active,
                triggered: false,
            }
        })
        .collect()
}

fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

impl Console {
    /// Whether a character with its pelvis at `pelvis`, facing `facing`, is
    /// in place to use this console (`generous`: twice the sideways room,
    /// half as much again of the facing — the AI's test).
    pub fn in_reach(&self, pelvis: Vec3, facing: f32, generous: bool) -> bool {
        if (pelvis.y - self.marker.y).abs() > MARKER_HEIGHT {
            return false;
        }
        // The marker's axis, pointing away from the console (toward a user).
        let away = Vec3::new(-self.facing.sin(), 0.0, -self.facing.cos());
        let d = Vec3::new(pelvis.x - self.marker.x, 0.0, pelvis.z - self.marker.z);
        let along = d.dot(away);
        let side = (d - away * along).length();
        let (side_max, facing_max) = if generous { (MARKER_SIDE * 2.0, MARKER_FACING * 1.5) } else { (MARKER_SIDE, MARKER_FACING) };
        (MARKER_AHEAD..=MARKER_BEHIND).contains(&along) && side <= side_max && wrap(facing - self.facing).abs() <= facing_max
    }

    /// Where a user ends up: at the marker, on the console's floor.
    pub fn stand(&self) -> Vec3 {
        Vec3::new(self.marker.x, self.position.y, self.marker.z)
    }

    /// The screen it shows: inactive, active or triggered.
    pub fn screen(&self) -> &'static str {
        match (self.active, self.triggered) {
            (false, _) => "inactive",
            (true, true) => "triggered",
            (true, false) => "active",
        }
    }
}

/// A character using a console.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Use {
    /// Waiting to come to a stop.
    Stopping(usize),
    /// Walking onto the marker (give up at this tick).
    Walking(usize, u64),
    /// Playing the console animation.
    Using(usize),
}

impl Use {
    pub fn console(self) -> usize {
        match self {
            Use::Stopping(c) | Use::Walking(c, _) | Use::Using(c) => c,
        }
    }
}

/// How far a walk-in moves this tick toward `to`.
pub fn walk_in_step(from: Vec3, to: Vec3) -> (Vec3, bool) {
    let d = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let len = d.length();
    if len <= WALK_IN_DONE {
        return (Vec3::ZERO, true);
    }
    (d / len * len.min(WALK_IN_STEP), false)
}

/// How long a walk-in may take.
pub fn walk_in_ticks() -> u64 {
    WALK_IN_TICKS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console() -> Console {
        Console {
            id: 1,
            class: "c".into(),
            position: Vec3::new(0.0, 0.0, 0.0),
            marker: Vec3::new(0.0, 11.7, 3.2),
            facing: PI,
            events: Vec::new(),
            initial_active: true,
            punch: false,
            is_alarm: false,
            active: true,
            triggered: false,
        }
    }

    #[test]
    fn reach_needs_place_height_and_facing() {
        let c = console();
        // In front of it (further from the console), facing it.
        assert!(c.in_reach(Vec3::new(0.0, 10.0, 8.0), PI, false));
        // Facing away.
        assert!(!c.in_reach(Vec3::new(0.0, 10.0, 8.0), 0.0, false));
        // Too far behind the marker, too far aside, wrong floor.
        assert!(!c.in_reach(Vec3::new(0.0, 10.0, 15.0), PI, false));
        assert!(!c.in_reach(Vec3::new(9.0, 10.0, 5.0), PI, false));
        assert!(c.in_reach(Vec3::new(9.0, 10.0, 5.0), PI, true), "the AI's test allows more aside");
        assert!(!c.in_reach(Vec3::new(0.0, 25.0, 5.0), PI, false));
    }
}
