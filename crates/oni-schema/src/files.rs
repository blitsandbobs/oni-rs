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

/// `levels/NN/env_particles.json`: the level's environmental particles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvParticlesFile {
    pub particles: Vec<EnvParticle>,
}

/// An environmental particle (an `ENVP` array's or a particle object's):
/// its class, its matrix (4×3, in its owner's space), its decal scale,
/// where it came from (`env`, `object`), the tag scripts name it by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvParticle {
    pub class: String,
    pub matrix: [f32; 12],
    pub decal_scale: [f32; 2],
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Held back at the start.
    #[serde(default, skip_serializing_if = "is_false")]
    pub not_initially_created: bool,
}

/// `levels/NN/particles.json`: the static viewer's sprites for the level's
/// environmental particles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParticlesFile {
    pub count: usize,
    pub particles: Vec<StaticParticle>,
}

/// A static environmental particle sprite.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticParticle {
    pub class: String,
    pub texture: Option<String>,
    /// Its texture's file, relative to the level's folder.
    pub texture_path: Option<String>,
    pub position: Xyz,
    pub size: f32,
    pub additive: bool,
    pub self_illuminated: bool,
}

/// A point as an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Xyz {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// `levels/NN/items.json`: the level's placed weapons and powerups.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemsFile {
    pub weapons: Vec<PlacedWeapon>,
    pub powerups: Vec<PlacedPowerup>,
}

/// A placed weapon: its class and where (yaw in degrees).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedWeapon {
    pub id: u32,
    pub class: String,
    pub position: V3,
    pub yaw: f32,
}

/// A placed powerup: its kind and where (yaw in degrees).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedPowerup {
    pub id: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub position: V3,
    pub yaw: f32,
}

/// `common/texture_flags.json`: the textures drawn additively and those
/// lit by themselves (by file name).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureFlagsFile {
    pub additive: Vec<String>,
    pub self_illuminated: Vec<String>,
}

/// `levels/NN/sky.json`: the level's sky (`ONSK`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkyFile {
    pub name: String,
    /// The box's textures: top, left, right, front, back.
    pub textures: Vec<Option<String>>,
    pub planets: Vec<Planet>,
    pub star_count: u32,
}

/// A sky's planet (the sun, moons).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Planet {
    pub texture: String,
    pub size: [f32; 2],
    pub azimuth: f32,
    pub bearing: f32,
}

/// `levels/NN/objects.json`: the level's placed objects (`OBOA`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectsFile {
    pub objects: Vec<PlacedObject>,
}

/// A placed object (`OBtObjectSetup`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedObject {
    pub index: usize,
    pub name: String,
    pub script_id: u32,
    /// `OBJECT_FLAGS` names.
    pub flags: Vec<String>,
    pub physics_level: String,
    pub position: V3,
    /// A quaternion.
    pub orientation: [f32; 4],
    pub scale: f32,
    /// 4×3.
    pub origin: [f32; 12],
    /// Its models (paths under `common/`).
    pub geometry: Vec<String>,
    pub radius: ObjectRadius,
    /// Its particles, in its own space.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<EnvParticle>,
    /// Its object animation's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
}

/// A placed object's collision sphere radius: as shipped, and fixed
/// (`QUIRKS.md`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectRadius {
    pub shipped: f32,
    pub fixed: f32,
}

/// `levels/NN/neutrals.json`: the level's neutral behaviours.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NeutralsFile {
    pub neutrals: Vec<Neutral>,
}

/// A neutral behaviour (`OBJtOSD_Neutral`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neutral {
    pub id: u16,
    pub name: String,
    pub flags: Vec<String>,
    pub trigger_range: f32,
    pub conversation_range: f32,
    pub follow_range: f32,
    pub abort_enemy_range: f32,
    pub trigger_sound: String,
    pub abort_sound: String,
    pub enemy_sound: String,
    pub end_script: String,
    pub give: NeutralGive,
    pub lines: Vec<NeutralLine>,
}

/// What a neutral gives the player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeutralGive {
    pub weapon: String,
    pub ballistic_ammo: u8,
    pub energy_ammo: u8,
    pub hypos: u8,
    pub flags: Vec<String>,
}

/// A neutral conversation's line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeutralLine {
    pub flags: Vec<String>,
    pub anim: Option<AnimRef>,
    pub other_anim: Option<AnimRef>,
    pub sound: String,
}

/// An animation type: by its `ANIM_TYPES` name, or its number when it has
/// none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnimRef {
    Name(String),
    Number(u16),
}

/// `common/sounds.json`: the sound definitions (`OSBD`), by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundsFile {
    pub ambients: BTreeMap<String, AmbientSound>,
    pub groups: BTreeMap<String, SoundGroup>,
    pub impulses: BTreeMap<String, ImpulseSound>,
}

/// An ambient sound (`SStAmbient`): its parts by group name and how it is
/// heard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbientSound {
    pub priority: u32,
    pub sphere_radius: f32,
    /// Seconds between details, at least and at most.
    pub detail_time: [f32; 2],
    pub max_volume_distance: f32,
    pub min_volume_distance: f32,
    pub threshold: u32,
    pub min_occlusion: f32,
    /// `interruptOnStop`, `playOnce`, `pan`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base2: Option<String>,
    #[serde(rename = "in", default, skip_serializing_if = "Option::is_none")]
    pub in_sound: Option<String>,
    #[serde(rename = "out", default, skip_serializing_if = "Option::is_none")]
    pub out_sound: Option<String>,
}

/// An impulse sound (`SStImpulse`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpulseSound {
    pub group: String,
    pub priority: u32,
    pub max_volume_distance: f32,
    pub min_volume_distance: f32,
    /// Played this many at once, its `alt` plays instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_threshold: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_velocity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_occlusion: Option<f32>,
}

/// A sound group (`SStGroup`): its permutations, one picked each play.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundGroup {
    pub volume: f32,
    pub pitch: f32,
    pub channels: u32,
    pub permutations: Vec<Permutation>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub prevent_repeats: bool,
}

/// A sound group's permutation: its weight, volume and pitch ranges, and
/// its sound (a file under `audio/`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Permutation {
    pub weight: u32,
    pub volume: [f32; 2],
    pub pitch: [f32; 2],
    pub sound: String,
}

/// `levels/NN/sounds.json`: the level's sound objects.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SoundObjectsFile {
    pub objects: Vec<SoundObject>,
}

/// A sound object: its ambient, where, how loud, and its shape — a sphere
/// (heard within, placed) or a box (heard inside).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundObject {
    pub object_id: u32,
    pub ambient: String,
    pub position: V3,
    pub volume: f32,
    /// Its distances (feet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sphere: Option<SoundSphere>,
    /// Its corners, relative to the object, unrotated.
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
}

/// A sound sphere's distances.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundSphere {
    pub max: f32,
    pub min: f32,
}

/// `common/impacts.json`: the impact and material trees, the textures'
/// materials, and the impact-effect table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImpactsFile {
    /// Each impact type's parent.
    pub impacts: BTreeMap<String, Option<String>>,
    /// Each material's parent.
    pub materials: BTreeMap<String, Option<String>>,
    /// Each texture's material.
    pub textures: BTreeMap<String, String>,
    pub entries: Vec<ImpactEntry>,
}

/// An impact effect: what it is for (impact, material, component,
/// modifier), the sound it makes and the particles it sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImpactEntry {
    pub impact: String,
    pub material: String,
    pub component: String,
    pub modifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<ImpactSound>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<ImpactParticle>,
}

/// An impact effect's sound, and the noise the AI hears.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImpactSound {
    pub impulse: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiNoise>,
}

/// An impact effect's particle: its class, orientation and where it goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImpactParticle {
    pub class: String,
    pub orientation: String,
    pub location: ImpactLocation,
}

/// Where an impact effect's particle goes (`P3tEffectLocationType`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ImpactLocation {
    Default,
    CollisionOffset {
        offset: f32,
    },
    FindWall {
        radius: f32,
        #[serde(rename = "lookAlong")]
        look_along: bool,
    },
    Decal {
        #[serde(rename = "static")]
        stays: bool,
        #[serde(rename = "randomRotation")]
        random_rotation: bool,
    },
    AttachedToObject,
}

/// `common/anim_sounds.json`: the variant tree, the variants by instance
/// name, and each variant's animation sounds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnimSoundsFile {
    /// Each variant's parent.
    pub variants: BTreeMap<String, Option<String>>,
    /// Each variant's name, by its instance name (as classes name it).
    pub instances: BTreeMap<String, String>,
    pub sounds: BTreeMap<String, Vec<AnimSound>>,
}

/// An animation sound: for an animation type (and modifier), its impulse,
/// at a frame of a clip when it names one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimSound {
    #[serde(rename = "type")]
    pub kind: String,
    pub modifier: String,
    pub impulse: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
}

/// `common/game_settings.json`: the game's settings (`ONGS`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSettingsFile {
    /// The powerups' looks, by kind.
    pub powerups: BTreeMap<String, PowerupLook>,
    pub magic_textures: MagicTextures,
    pub autoprompts: Vec<Autoprompt>,
    /// A character's health colour ramp (melee flashes, the HUD).
    pub health_color: Vec<HealthColor>,
    /// The game's event and condition sounds (empty: silent), by name.
    pub event_sounds: BTreeMap<String, String>,
    pub condition_sounds: BTreeMap<String, String>,
    pub hypo: HypoSettings,
    pub boosted_health: f32,
    pub difficulty: DifficultySettings,
}

/// A powerup's look: its model (a path under `common/`), the glow sprite
/// and its size and height, and the physics sphere a dropped one falls with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerupLook {
    pub radius: f32,
    pub glow_size: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow_height: Option<f32>,
    /// Its geometry's name, while the converter looks for it (not written).
    #[serde(skip)]
    pub geometry: Option<String>,
}

/// The magic effects' textures (by file name).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MagicTextures {
    pub shield: String,
    pub daodan_shield: String,
    pub invis: String,
}

/// An autoprompt: an event's message, on levels `levels[0]..=levels[1]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Autoprompt {
    pub event: String,
    pub levels: [u16; 2],
    pub message: String,
}

/// A point of the health colour ramp: a health fraction and its colour
/// (`#aarrggbb`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthColor {
    pub health: f32,
    pub color: String,
}

/// What a hypo does: the share of full health it heals, past full the
/// share it boosts, and a super-hypo class's extra damage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HypoSettings {
    pub amount: f32,
    pub boost_amount: f32,
    pub overhypo_base_damage: f32,
    pub overhypo_max_damage: f32,
}

/// The difficulty levels and their multipliers (one per level).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DifficultySettings {
    pub levels: Vec<String>,
    pub notice_multiplier: Vec<f32>,
    pub block_multiplier: Vec<f32>,
    pub dodge_multiplier: Vec<f32>,
    pub inaccuracy_multiplier: Vec<f32>,
    pub player_hp_multiplier: Vec<f32>,
    pub enemy_hp_multiplier: Vec<f32>,
}

/// A text run's font (`size`, `style`; its shade `#aarrggbb`, when set).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextFont {
    pub size: i64,
    pub style: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shade: Option<String>,
}

/// A run of text in its font.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub font: TextFont,
}

/// A page (a text console's, the pause screen's): its text, its hint, and
/// a picture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub text: Vec<TextRun>,
    pub hint: Vec<TextRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<Picture>,
}

/// A page's picture: a texture or a part spec, by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_spec: Option<String>,
}

/// A dialog (`WMtDialogData`) as the text console and pause screen use it:
/// its size and items. (`common/text_console.json`.)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dialog {
    pub size: [i64; 2],
    pub items: Vec<DialogItem>,
}

/// A dialog's item: its id, window type, title, rectangle and font.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DialogItem {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: i64,
    pub title: String,
    /// x, y, width, height.
    pub rect: [i64; 4],
    pub font: TextFont,
}

/// `levels/NN/text_consoles.json`: the level's text consoles, by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextConsolesFile {
    pub consoles: BTreeMap<String, TextConsole>,
}

/// A text console's pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextConsole {
    pub pages: Vec<Page>,
}

/// `common/pause_screen.json`: the pause screen's dialog and the pages
/// every level shares.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseScreenFile {
    pub dialog: Dialog,
    pub help: Vec<Page>,
    pub items: Vec<ItemPage>,
    pub weapons: Vec<WeaponPage>,
    pub diary: Vec<DiaryPage>,
    /// The move keys' icons (textures), by key.
    pub key_icons: Option<BTreeMap<String, String>>,
}

/// `levels/NN/pause_pages.json`: a level's objectives and its own item
/// pages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PausePagesFile {
    pub objectives: Vec<Page>,
    pub items: Vec<ItemPage>,
}

/// An item's page: the powerup type and its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemPage {
    #[serde(rename = "type")]
    pub kind: i64,
    pub name: String,
    pub page: Page,
}

/// A weapon class's page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponPage {
    pub class: Option<String>,
    pub page: Page,
}

/// A diary page: its level and number, whether it shows a new move, the
/// move's keys, and its content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiaryPage {
    pub level: i64,
    pub page: i64,
    pub new_move: bool,
    pub keys: Vec<i64>,
    pub content: Page,
}

/// `common/hud_help.json`: the HUD help overlay's rings and labels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HudHelpFile {
    pub left: HelpRing,
    pub right: HelpRing,
    pub items: Vec<HelpItem>,
}

/// A HUD help ring: its texture and offset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HelpRing {
    pub texture: Option<String>,
    pub offset: [i16; 2],
}

/// A HUD help label: its text, offset and which ring it labels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelpItem {
    pub text: String,
    pub offset: [i16; 2],
    pub right: bool,
}

/// `common/menus.json`: the main menu's dialogs, the level names, the
/// out-of-game skin and the popup menus.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MenusFile {
    pub dialogs: BTreeMap<String, MenuDialog>,
    pub levels: Vec<MenuLevel>,
    /// Each skin part's part spec.
    pub skin: Option<BTreeMap<String, String>>,
    pub popups: BTreeMap<String, Vec<PopupItem>>,
}

/// A menu dialog (`WMtDialogData`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuDialog {
    pub id: i64,
    pub title: String,
    pub flags: i64,
    pub style: i64,
    pub size: [i64; 2],
    pub items: Vec<MenuItem>,
}

/// A menu dialog's item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: i64,
    pub title: String,
    pub flags: i64,
    pub style: i64,
    pub rect: [i64; 4],
    pub font: MenuFont,
}

/// A menu item's font: its family, size, style and shade (`#aarrggbb`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuFont {
    pub family: Option<String>,
    pub size: i64,
    pub style: i64,
    pub shade: String,
}

/// A level as the menus know it: its number, the next one, its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenuLevel {
    pub level: i64,
    pub next: i64,
    pub name: String,
}

/// A popup menu's item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PopupItem {
    pub id: i64,
    pub flags: i64,
    pub title: String,
}

/// `common/part_specs.json`: the part specs (`PSpc`), by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartSpecsFile {
    pub part_specs: BTreeMap<String, PartSpec>,
}

/// A part spec: its texture and its 3 × 3 slices' corners (top-left and
/// bottom-right, by column then row).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartSpec {
    pub texture: String,
    pub tl: Vec<[i64; 2]>,
    pub br: Vec<[i64; 2]>,
}

/// `common/messages.json` (and a level's): the message tables, by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessagesFile {
    pub tables: BTreeMap<String, MessageTable>,
}

/// A message table: by name (`messages`), or in order (`subtitles`, found
/// by the first key a line's name starts with).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageTable {
    Ordered(Vec<(String, String)>),
    ByName(BTreeMap<String, String>),
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
