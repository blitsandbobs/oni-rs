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

fn is_false(b: &bool) -> bool {
    !*b
}

/// `levels/NN/consoles.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConsolesFile {
    pub consoles: Vec<Console>,
}

/// A console (`OBJtOSD_Console`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Console {
    pub id: u16,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// Where its user stands and faces.
    pub marker: Marker,
    pub events: Vec<Event>,
    /// Its screen's textures when inactive, active and used.
    pub screens: Screens,
    /// Its screen's model (a path under `common/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_model: Option<String>,
    /// `CONSOLE_FLAGS` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Its class prompts as an alarm console.
    #[serde(default, skip_serializing_if = "is_false")]
    pub alarm_prompt: bool,
}

/// A console's marker: where its user stands, and the way they face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub position: V3,
    pub facing: f32,
}

/// A console's screen textures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Screens {
    pub inactive: String,
    pub active: String,
    pub triggered: String,
}

/// `levels/NN/doors.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DoorsFile {
    pub doors: Vec<Door>,
}

/// A door (`OBJtOSD_Door`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Door {
    pub id: u16,
    /// Its object id (its level quads' tag).
    pub object_id: u32,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// How near someone must come to open it.
    pub radius: f32,
    /// The key it needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<u16>,
    /// `DOOR_FLAGS` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
    /// Its moving leaves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leaves: Vec<DoorLeaf>,
}

/// A door leaf: its object setup's origin (4×3, column by column).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DoorLeaf {
    pub origin: [f32; 12],
}

/// `common/door_classes.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DoorClassesFile {
    pub classes: BTreeMap<String, DoorClass>,
}

/// A door class (`DOOR`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoorClass {
    /// The impulses played opening and closing.
    pub open: String,
    pub close: String,
    /// Its leaves' animation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<ObjectAnimation>,
    /// What sound passes it while it isn't open (`all`, `shouting`,
    /// `gunfire`, `none`) and how much it muffles what does (1: all).
    pub sound_allow: String,
    pub sound_muffle: f32,
    /// The noise the AI hears it make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiNoise>,
}

/// A noise the AI hears: its kind (`AI_SOUNDS`) and how far.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiNoise {
    #[serde(rename = "type")]
    pub kind: String,
    pub radius: f32,
}

/// `common/laser_classes.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LaserClassesFile {
    pub classes: BTreeMap<String, LaserClassSounds>,
}

/// A laser class's sounds (`TRIG`): the ambient while active, the impulse
/// when tripped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaserClassSounds {
    pub active: String,
    pub trigger: String,
}

/// `object_animations.json` (common and per level).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectAnimationsFile {
    pub ticks_per_second: u32,
    pub count: usize,
    pub animations: Vec<ObjectAnimation>,
}

/// A scripted object animation (`OBAN`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectAnimation {
    pub name: Option<String>,
    /// `OBJECT_ANIMATION_FLAGS` names.
    pub flags: Vec<String>,
    pub ticks_per_frame: u16,
    pub num_frames: u16,
    pub door_open_frames: u16,
    /// 4×3, column by column.
    pub scale: [f32; 12],
    pub keyframes: Vec<ObjectAnimationKey>,
}

/// An object animation's key: its frame, translation and rotation
/// (a quaternion).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectAnimationKey {
    pub frame: i32,
    pub translation: V3,
    pub rotation: [f32; 4],
}

/// `common/turrets.json`: the turret classes (`TURR`), by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurretClassesFile {
    pub classes: BTreeMap<String, TurretClass>,
}

/// A turret class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurretClass {
    pub name: String,
    /// Its head's and barrel's models (paths under `common/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barrel: Option<String>,
    pub head_pivot: V3,
    pub barrel_pivot: V3,
    /// Ticks it stays on (0: for ever).
    pub timeout: u32,
    pub pitch: Limits,
    pub yaw: Limits,
    pub active_sound: String,
    pub ai: AiGunParams,
    pub targeting: Targeting,
    pub skill: ShootSkill,
    pub attachments: Vec<TurretAttachment>,
}

/// A turning part's limits (radians) and speed (radians a tick).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub min: f32,
    pub max: f32,
    pub speed: f32,
}

/// A turret's particle attachment: its class, which shooter it is (−1:
/// none), its chamber time (shooters only) and its matrix (4×3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurretAttachment {
    pub class: String,
    pub shooter: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamber: Option<u16>,
    pub matrix: [f32; 12],
}

/// How the AI uses a gun (`WPtAIParameters`; weapons' and turrets').
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGunParams {
    pub no_wild_shots: bool,
    pub prediction_speed: f32,
    pub burst_radius: f32,
    pub aim_radius: f32,
    pub detection_volume: f32,
    pub min_safe_distance: f32,
    pub max_range: f32,
    pub max_startle_misses: u16,
    pub shoot_skill_index: u16,
    pub melee_assist_timer: u32,
    pub ballistic_speed: f32,
    pub ballistic_gravity: f32,
    pub dodge_pattern: V3,
    /// Where the shot leaves the gun (gun space): its direction, its offset
    /// off that line, and the turn from it onto the barrel (4×3).
    pub shooter_direction: V3,
    pub shooter_offset: V3,
    pub shooter_inverse: [f32; 12],
}

impl Default for AiGunParams {
    /// None given: all zero, the shot leaving along the barrel.
    fn default() -> Self {
        AiGunParams {
            no_wild_shots: false,
            prediction_speed: 0.0,
            burst_radius: 0.0,
            aim_radius: 0.0,
            detection_volume: 0.0,
            min_safe_distance: 0.0,
            max_range: 0.0,
            max_startle_misses: 0,
            shoot_skill_index: 0,
            melee_assist_timer: 0,
            ballistic_speed: 0.0,
            ballistic_gravity: 0.0,
            dodge_pattern: [0.0; 3],
            shooter_direction: [0.0; 3],
            shooter_offset: [0.0; 3],
            shooter_inverse: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        }
    }
}

/// Targeting settings (`AI2tTargetingParams`; characters' and turrets').
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Targeting {
    pub startle_miss_ratio: f32,
    pub startle_miss_distance: f32,
    pub predict_amount: f32,
    pub predict_position_delay: u32,
    pub predict_delay: u32,
    pub predict_velocity: u32,
    pub predict_trend: u32,
}

/// A shooting skill (`AI2tShootingSkill`; characters' and turrets').
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShootSkill {
    pub recoil_compensation: f32,
    pub best_aiming_sine: f32,
    pub shot_error_percent: f32,
    pub shot_decay: f32,
    pub gun_inaccuracy: f32,
    pub delay_min: u16,
    pub delay_max: u16,
}

/// `levels/NN/turrets.json`: the level's placed turrets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurretsFile {
    pub turrets: Vec<Turret>,
}

/// A placed turret (`OBJtOSD_Turret`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turret {
    pub object_id: u32,
    pub id: u16,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// The teams it fires at (bits).
    pub target_teams: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// `levels/NN/corpses.json`: the level's corpse array (`CRSA`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CorpsesFile {
    /// Its size: the static corpses and room for the dead of play.
    pub max: u32,
    pub corpses: Vec<Corpse>,
}

/// A pre-placed corpse: its class, each body part's matrix (4×3) and its
/// bounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corpse {
    pub name: String,
    pub class: String,
    pub matrices: Vec<[f32; 12]>,
    pub bbox: Bounds,
}

/// A box's corners.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: V3,
    pub max: V3,
}

/// `levels/NN/flags.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FlagsFile {
    pub flags: Vec<Flag>,
}

/// A flag: id, display name, position, facing and its note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flag {
    pub id: u16,
    pub name: String,
    pub position: V3,
    pub facing: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// `levels/NN/triggers.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TriggersFile {
    pub triggers: Vec<TriggerVolume>,
}

/// A trigger volume (`OT_TriggerVolume`): a box from `position` (a corner)
/// `size` along its axes, the scripts it runs, who sets it off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TriggerVolume {
    pub id: i32,
    pub name: String,
    pub position: V3,
    pub axes: [V3; 3],
    pub size: V3,
    /// `TEAMS` names.
    pub teams: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<i32>,
    /// `TRIGGER_FLAGS` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// `levels/NN/patrols.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PatrolsFile {
    pub patrols: Vec<Patrol>,
}

/// A patrol path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patrol {
    pub id: u16,
    pub name: String,
    pub waypoints: Vec<Waypoint>,
    /// Joined (and rejoined) at its nearest waypoint.
    #[serde(default, skip_serializing_if = "is_false")]
    pub return_to_nearest: bool,
}

/// A patrol waypoint (`AI2tWaypoint`; `PATROL_WAYPOINTS`). Ticks keep the
/// data's own widths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Waypoint {
    MoveToFlag {
        flag: i16,
    },
    Stop,
    Pause {
        ticks: i32,
    },
    LookAtFlag {
        flag: i16,
    },
    LookAtPoint {
        point: V3,
    },
    MoveAndFaceFlag {
        flag: i16,
    },
    Loop,
    MovementMode {
        mode: String,
    },
    MoveToPoint {
        point: V3,
    },
    LookInDirection {
        direction: String,
    },
    MoveThroughFlag {
        flag: i16,
        distance: f32,
    },
    MoveThroughPoint {
        point: V3,
        distance: f32,
    },
    StopLooking,
    FreeFacing,
    GlanceAtFlag {
        flag: i16,
        ticks: u32,
    },
    /// (Old files keep neither.)
    MoveNearFlag {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flag: Option<i16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance: Option<f32>,
    },
    LoopToWaypoint {
        waypoint: u32,
    },
    Scan {
        ticks: i16,
        angle: f32,
    },
    StopScanning,
    Guard {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticks: Option<i16>,
        flag: i16,
        #[serde(rename = "maxAngle")]
        max_angle: f32,
    },
    TriggerScript {
        script: String,
    },
    PlayScript {
        script: String,
    },
    LockIntoPatrol {
        lock: bool,
    },
    ShootAtFlag {
        flag: i16,
        ticks: i16,
        inaccuracy: f32,
    },
    Unknown,
}

/// `levels/NN/characters.json`: the level's placed characters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharactersFile {
    pub characters: Vec<PlacedCharacter>,
}

/// A placed character (`OBJtOSD_Character`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedCharacter {
    pub id: u32,
    pub name: String,
    pub class: String,
    /// `CHARACTER_FLAGS` names.
    pub flags: Vec<String>,
    pub position: V3,
    pub facing: f32,
    pub team: String,
    pub job: String,
    pub alert: AlertLevels,
    pub pursue: PursueModes,
    pub ammo_percent: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<String>,
    /// Its event scripts (`spawn`, `die`, …), by event.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_point_offset: Option<i32>,
    /// Its patrol path or guard's id (0 is a real id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<u16>,
    pub combat_profile_id: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub melee_profile_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neutral_id: Option<u16>,
    /// What it carries and drops, by `INVENTORY` kind.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inventory: BTreeMap<String, Carried>,
    /// The alarm groups it answers (bit numbers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alarm_groups: Vec<u32>,
}

/// A character's alert levels (`ALERT_LEVELS` names).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertLevels {
    pub initial: String,
    pub minimum: String,
    pub job: String,
    pub investigate: String,
}

/// What a character does about a lost or unseen enemy (`PURSUIT_MODES`,
/// `PURSUIT_LOST` names).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PursueModes {
    pub strong_unseen: String,
    pub weak_unseen: String,
    pub strong_seen: String,
    pub weak_seen: String,
    pub lost: String,
}

/// How many of an item a character uses and drops.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Carried {
    #[serde(rename = "use")]
    pub uses: u16,
    pub drop: u16,
}

/// `levels/NN/setups.json`: the characters `chr_create` makes by script id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SetupsFile {
    pub setups: Vec<CharacterSetup>,
}

/// A character setup (`AItCharacterSetup`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSetup {
    pub script_id: u16,
    pub flag: i16,
    pub team: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo: Option<u16>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
}

/// `levels/NN/films.json`: cutscene recordings (`FILM`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilmsFile {
    pub ticks_per_second: u32,
    pub count: usize,
    pub films: Vec<Film>,
}

/// A film: its start, and its keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Film {
    pub name: Option<String>,
    pub initial_location: V3,
    pub initial_facing: f32,
    pub initial_desired_facing: f32,
    /// Aiming left-right and up-down.
    pub aiming: [f32; 2],
    pub length: u32,
    pub key_count: usize,
    pub keys: Vec<FilmKey>,
}

/// A film key: the turns and buttons from tick `time`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilmKey {
    #[serde(rename = "turnUD")]
    pub turn_ud: f32,
    #[serde(rename = "turnLR")]
    pub turn_lr: f32,
    pub buttons: u64,
    pub time: u32,
}

/// `common/combat_profiles.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CombatProfilesFile {
    pub profiles: Vec<CombatProfile>,
}

/// A combat profile (`OBJtOSD_Combat`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatProfile {
    pub id: u16,
    pub name: String,
    pub behavior: CombatBehaviors,
    pub medium_range: f32,
    pub melee_when: String,
    pub no_gun: String,
    pub short_range: f32,
    pub pursuit_distance: f32,
    pub panic: PanicTicks,
    pub alarm: AlarmSettings,
}

/// A combat profile's behaviour by range (`COMBAT_BEHAVIORS` names).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatBehaviors {
    pub long: String,
    pub medium: String,
    pub short: String,
    pub medium_retreat: String,
    pub long_retreat: String,
}

/// How long each kind of fright lasts (ticks).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanicTicks {
    pub melee: u32,
    pub gunfire: u32,
    pub sight: u32,
    pub hurt: u32,
}

/// A combat profile's alarm settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmSettings {
    pub find_distance: f32,
    pub ignore_enemy_distance: f32,
    pub chase_enemy_distance: f32,
    pub damage_threshold: u32,
    pub fight_ticks: u32,
    pub attack_if_knocked_down: bool,
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
