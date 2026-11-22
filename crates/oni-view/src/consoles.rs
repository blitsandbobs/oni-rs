//! Consoles (behaviour per `scripting/BSL.md`, *Consoles*; data
//! `levels/NN/consoles.json`): terminals a character uses with the Action
//! key — it walks up to the console's action marker, plays the console
//! animation, and on its action frame the console fires its event list
//! (scripts, other consoles, alarms, doors, …).

use std::f32::consts::PI;
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
    ActivateTurret(u16),
    DeactivateTurret(u16),
    /// Anything else (not modelled).
    Other(String, u16),
}

/// An event from its JSON form.
impl From<&oni_schema::files::Event> for Event {
    fn from(e: &oni_schema::files::Event) -> Event {
        use oni_schema::files::Event as F;
        match e {
            F::Script { script } => Event::Script(script.clone()),
            F::ActivateConsole { id } => Event::ActivateConsole(*id),
            F::DeactivateConsole { id } => Event::DeactivateConsole(*id),
            F::ActivateAlarm { id } => Event::ActivateAlarm(*id),
            F::LockDoor { id } => Event::LockDoor(*id),
            F::UnlockDoor { id } => Event::UnlockDoor(*id),
            F::ActivateTrigger { id } => Event::ActivateLaser(*id),
            F::DeactivateTrigger { id } => Event::DeactivateLaser(*id),
            F::ActivateTurret { id } => Event::ActivateTurret(*id),
            F::DeactivateTurret { id } => Event::DeactivateTurret(*id),
            F::DeactivateAlarm { id } => Event::Other("deactivateAlarm".into(), *id),
            F::None => Event::Other("none".into(), 0),
        }
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
    /// Its orientation, its screen's model (under `common/`) and the screen's
    /// texture names when inactive, active, used.
    pub rotation: glam::Mat3,
    pub screen_model: Option<String>,
    pub screens: [String; 3],
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
    let doc: oni_schema::files::ConsolesFile =
        crate::json::read_file(level_dir.join("consoles.json"));
    doc.consoles
        .iter()
        .map(|c| {
            let has = |f: &str| c.flags.iter().any(|x| x == f);
            let initial_active = has("initialActive");
            Console {
                id: c.id,
                class: c.class.clone(),
                position: Vec3::from(c.position),
                marker: Vec3::from(c.marker.position),
                facing: c.marker.facing,
                events: c.events.iter().map(Into::into).collect(),
                initial_active,
                punch: has("punch"),
                is_alarm: has("isAlarm"),
                active: initial_active,
                triggered: false,
                rotation: glam::Mat3::from_cols(
                    Vec3::from(c.axes[0]),
                    Vec3::from(c.axes[1]),
                    Vec3::from(c.axes[2]),
                ),
                screen_model: c.screen_model.clone(),
                screens: [
                    c.screens.inactive.clone(),
                    c.screens.active.clone(),
                    c.screens.triggered.clone(),
                ],
            }
        })
        .collect()
}

use crate::angle::wrap_pi as wrap;

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
        let (side_max, facing_max) = if generous {
            (MARKER_SIDE * 2.0, MARKER_FACING * 1.5)
        } else {
            (MARKER_SIDE, MARKER_FACING)
        };
        (MARKER_AHEAD..=MARKER_BEHIND).contains(&along)
            && side <= side_max
            && wrap(facing - self.facing).abs() <= facing_max
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
            rotation: glam::Mat3::IDENTITY,
            screen_model: None,
            screens: Default::default(),
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
        assert!(
            c.in_reach(Vec3::new(9.0, 10.0, 5.0), PI, true),
            "the AI's test allows more aside"
        );
        assert!(!c.in_reach(Vec3::new(0.0, 25.0, 5.0), PI, false));
    }
}
