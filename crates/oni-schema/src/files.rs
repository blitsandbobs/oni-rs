//! The converted files' formats, shared by the converter (which writes
//! them) and the engine (which reads them): a key renamed on one side is
//! renamed on both. Keys are camelCase; optional parts are left out when
//! empty. `docs/ASSET_FORMAT.md` describes each file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A point or direction.
pub type V3 = [f32; 3];

/// An event a console, door or laser trigger fires (`OBJtEvent`;
/// `EVENT_TYPES`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    None,
    Script { script: String },
    ActivateTurret { id: u16 },
    DeactivateTurret { id: u16 },
    ActivateConsole { id: u16 },
    DeactivateConsole { id: u16 },
    ActivateAlarm { id: u16 },
    DeactivateAlarm { id: u16 },
    ActivateTrigger { id: u16 },
    DeactivateTrigger { id: u16 },
    LockDoor { id: u16 },
    UnlockDoor { id: u16 },
}

/// `levels/NN/lasers.json`: the level's laser triggers, and the tracks and
/// emitter eyes of the classes they use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LasersFile {
    pub lasers: Vec<Laser>,
    /// Per class: per animation frame, the beam's origin and unit direction
    /// in the laser's own space.
    pub classes: BTreeMap<String, Vec<[f32; 6]>>,
    pub eyes: BTreeMap<String, LaserEye>,
}

/// A laser trigger (`OBJtOSD_Trigger`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Laser {
    pub id: u16,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// RGB, 0–1.
    pub color: V3,
    /// Where on its track it starts (0–1).
    pub start_offset: f32,
    /// Track frames per tick.
    pub speed: f32,
    pub emitters: u16,
    /// Ticks.
    pub time_on: u16,
    pub time_off: u16,
    pub events: Vec<Event>,
    /// `LASER_FLAGS` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// A laser class's emitter eye: its model (a path under `common/`) and, per
/// animation frame, the emitter's 4×3 matrix in the laser's own space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaserEye {
    pub model: String,
    pub frames: Vec<[f32; 12]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_read_and_write_by_their_type_name() {
        let events = vec![
            Event::Script {
                script: "go".into(),
            },
            Event::LockDoor { id: 3 },
            Event::None,
        ];
        let text = serde_json::to_string(&events).unwrap();
        assert_eq!(
            text,
            r#"[{"type":"script","script":"go"},{"type":"lockDoor","id":3},{"type":"none"}]"#
        );
        assert_eq!(serde_json::from_str::<Vec<Event>>(&text).unwrap(), events);
    }
}
