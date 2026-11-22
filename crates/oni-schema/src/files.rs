//! The converted files' formats, shared by the converter (which writes
//! them) and the engine (which reads them): a key renamed on one side is
//! renamed on both, and a file that doesn't read as its type is reported
//! rather than read as zeros. What `oni-convert` writes is standard glTF
//! 2.0, JSON, PNG and WAV (bar the few files listed below), so it reads
//! without knowing Oni's binary formats; Oni-specific data in glTF rides in
//! `extras` as structured objects. The types here are the authority on names and shapes, their
//! doc comments on what the fields mean. `scripts/compare_trees.py` checks
//! that two converted trees hold the same data (numbers at f32 precision,
//! keys in any order, glTF binary chunks byte for byte).
//!
//! # Conventions
//!
//! - **Keys** are camelCase. A key is omitted when its value is absent or
//!   its default (empty, zero, false, or as a field says); readers apply
//!   the default.
//! - **Units**: lengths in Oni world units (the level geometry's); angles
//!   in radians; time in ticks (1/60 s); per-frame tracks have one entry
//!   per tick. Exceptions say so (sound distances in feet, placed items'
//!   yaw in degrees, particle lifetimes in seconds).
//! - **Frames**: glTF's right-handed, Y-up space, as the meshes use.
//!   "Local" or "character-local" is relative to the character: +Z forward,
//!   +X to its left, +Y up. Yaw grows counter-clockwise seen from above
//!   (toward +X).
//! - **Matrices** "4×3" are 12 floats column by column: the X, Y and Z
//!   axes, then the translation. `axes` are a rotation's three unit
//!   columns; quaternions are `[x, y, z, w]`. Colours are `"#aarrggbb"`
//!   unless stated.
//! - **Enumerations** are camelCase names, not numbers: a single value is
//!   a string (`"punchForward"`, `"standing"`), a set of bits an array of
//!   names (`["fight"]`, `["throwSource", "atomic"]`). The names follow the
//!   original identifiers; the vocabularies are the tables in this crate
//!   (`ANIM_TYPES`, `VARIANTS`, `TEAMS`, `DOOR_FLAGS`, …), which converter
//!   and engine share.
//! - **Ranges** are objects (`{start, end}` or `{start, length}`), ticks
//!   inclusive unless stated.
//! - **File names** are asset names with anything but letters, digits, `_`
//!   and `-` as `_`. Names that differ only by case (`left`, `Left`) would
//!   be one file on a case-insensitive disk: all but one (the lowercase
//!   one, else the first) take a `~N` suffix (`Left~1.png`), and every
//!   reference to them (glTF materials, texture names in JSON) uses the
//!   suffixed name.
//! - **Paths**: named (shared) assets live in `common/`, a level's unnamed
//!   ones in `levels/NN/`; a named reference resolves in `common/`. Model
//!   paths in JSON are relative to `common/` (`models/<name>.glb`) unless a
//!   field says otherwise; textures are named by file stem (as the PNGs are
//!   named). Big textures (`TXMB`: splash screens, menus) are assembled from
//!   their 256-pixel tiles into one PNG in each level's own `textures/`.
//!
//! # Files
//!
//! - `oni.json`: the manifest — converter version, formats, per-level
//!   counts and warnings, source file hashes (untyped; `write_manifest`).
//! - `movies/outro.bik`: the ending movie, copied unchanged (Bink 1, decoded
//!   by `oni-bink`) from the game data folder or a `movies/` folder beside
//!   it; absent when neither has it.
//! - `common/textures/<name>.png`: the named textures (`TXMP`).
//! - `common/audio/<name>.wav`: the sounds (`SNDD`), as PCM WAV.
//! - `common/models/<name>.glb`: the named geometries (`M3GM`), one mesh
//!   each with its texture.
//! - `common/characters/<BODY>/model.glb`, `low.glb`: a character body,
//!   node per part (`convert_characters` in `oni-convert`).
//! - `common/characters/<BODY>/character.json`: a readable rig summary
//!   (untyped; `write_character_json`).
//! - `common/characters/classes.json` → [`CharacterClassesFile`].
//! - `common/characters/melee_profiles.json` → [`MeleeProfilesFile`].
//! - `common/animations/<CLIP>.glb`: one animation clip, its metadata in
//!   [`ClipExtras`] (`convert_animations` in `oni-convert`).
//! - `common/animations/index.json`: the clips by name prefix (untyped).
//! - `common/animations/collections/<TRAC>.json` →
//!   [`AnimationCollectionFile`].
//! - `common/anim_sounds.json` → [`AnimSoundsFile`].
//! - `common/combat_profiles.json` → [`CombatProfilesFile`].
//! - `common/door_classes.json` → [`DoorClassesFile`].
//! - `common/fonts.json` → [`FontsFile`]; `common/fonts/<family>_<size>_<style>.png`:
//!   each font's glyph atlas (white, coverage as alpha).
//! - `common/game_settings.json` → [`GameSettingsFile`].
//! - `common/hud_help.json` → [`HudHelpFile`].
//! - `common/impacts.json` → [`ImpactsFile`].
//! - `common/laser_classes.json` → [`LaserClassesFile`].
//! - `common/menus.json` → [`MenusFile`].
//! - `common/messages.json` → [`MessagesFile`].
//! - `common/object_animations.json`, `levels/NN/object_animations.json` →
//!   [`ObjectAnimationsFile`].
//! - `common/part_specs.json` → [`PartSpecsFile`].
//! - `common/particle_classes.json` → [`ParticleClassesFile`].
//! - `common/pause_screen.json` → [`PauseScreenFile`].
//! - `common/sounds.json` → [`SoundsFile`].
//! - `common/strings.json`, `common/text.json`, `levels/NN/text.json`: the
//!   raw `TStr` and `IGSt` string tables (untyped; `convert_strings`,
//!   `convert_ui_text`).
//! - `common/text_console.json` → [`Dialog`].
//! - `common/texture_animations.json` → [`TextureAnimationsFile`].
//! - `common/texture_flags.json` → [`TextureFlagsFile`].
//! - `common/turrets.json` → [`TurretClassesFile`].
//! - `common/weapons.json` → [`WeaponsFile`].
//! - `levels/NN/textures/`: the level's own textures (and `models/`, when
//!   it has unnamed geometry).
//! - `levels/NN/environment.glb`: the level's geometry, quads grouped by
//!   texture and object (`convert_environment` in `oni-convert`).
//! - `levels/NN/quads.bin`: every quad as collision and decals see it
//!   (`convert_quads` in `oni-convert`).
//! - `levels/NN/navigation.json` → [`NavigationFile`]; `navigation.bin`:
//!   its rooms' grids (`convert_navigation` in `oni-convert`).
//! - `levels/NN/scripts.lua`: the level's BSL scripts translated to Lua
//!   (`convert_scripts` in `oni-convert`).
//! - `levels/NN/level.json`: the level's objects as decoded, for reading
//!   (untyped; `level_json::write_level_json`).
//! - `levels/NN/characters.json` → [`CharactersFile`].
//! - `levels/NN/consoles.json` → [`ConsolesFile`].
//! - `levels/NN/corpses.json` → [`CorpsesFile`].
//! - `levels/NN/doors.json` → [`DoorsFile`].
//! - `levels/NN/env_particles.json` → [`EnvParticlesFile`].
//! - `levels/NN/films.json` → [`FilmsFile`].
//! - `levels/NN/flags.json` → [`FlagsFile`].
//! - `levels/NN/items.json` → [`ItemsFile`].
//! - `levels/NN/lasers.json` → [`LasersFile`].
//! - `levels/NN/neutrals.json` → [`NeutralsFile`].
//! - `levels/NN/objects.json` → [`ObjectsFile`].
//! - `levels/NN/particles.json` → [`ParticlesFile`].
//! - `levels/NN/patrols.json` → [`PatrolsFile`].
//! - `levels/NN/pause_pages.json` → [`PausePagesFile`].
//! - `levels/NN/setups.json` → [`SetupsFile`].
//! - `levels/NN/sky.json` → [`SkyFile`].
//! - `levels/NN/sounds.json` → [`SoundObjectsFile`].
//! - `levels/NN/text_consoles.json` → [`TextConsolesFile`].
//! - `levels/NN/triggers.json` → [`TriggersFile`].
//! - `levels/NN/turrets.json` → [`TurretsFile`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A point or direction.
pub type V3 = [f32; 3];

/// An event a console, door or laser trigger fires (`OBJtEvent`;
/// `EVENT_TYPES`). Event lists run in order; an `id` of `0xFFFF` means all
/// of that kind.
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
    /// Per class used: per animation frame, the beam's origin and unit
    /// direction in the laser's own space (`[ox, oy, oz, dx, dy, dz]`).
    pub classes: BTreeMap<String, Vec<[f32; 6]>>,
    /// Per class used: its emitter's eye.
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
    /// Ticks on and off.
    pub time_on: u16,
    pub time_off: u16,
    /// What tripping it does.
    pub events: Vec<Event>,
    /// `LASER_FLAGS` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// A laser class's emitter eye: its model (the emitter class's geometry, a
/// path under `common/`) and, per animation frame, the emitter's 4×3 matrix
/// in the laser's own space.
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
    /// What scripts and events name it by.
    pub id: u16,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// Where its user stands and faces (from the class's action point).
    pub marker: Marker,
    /// What using it does.
    pub events: Vec<Event>,
    /// Its screen's textures when inactive, active and used.
    pub screens: Screens,
    /// Its screen's model (a path under `common/`), drawn at the console
    /// with the texture `screens` gives per state.
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

/// A console's screen textures (upper-cased names; none: `NONE`).
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
    /// What scripts and events name it by.
    pub id: u16,
    /// Its object id (its level quads' tag): its closed leaf is the
    /// environment's `door_0x<objectId>` node.
    pub object_id: u32,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// How near someone must come to open it (its activation radius).
    pub radius: f32,
    /// The key it needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<u16>,
    /// `DOOR_FLAGS` names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// What opening it does (as consoles').
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
    /// Its moving leaves: the object setups made for them (one, or two for
    /// double doors).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leaves: Vec<DoorLeaf>,
}

/// A door leaf: its object setup's origin (4×3, `debugOrigMatrix`).
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
    /// Its leaves' animation (as in `object_animations.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<ObjectAnimation>,
    /// What sound passes it while it isn't open (`all`, `shouting`,
    /// `gunfire`, `none`) and how much it muffles what does (1: all).
    pub sound_allow: String,
    pub sound_muffle: f32,
    /// The noise the AI hears it make (none shipped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiNoise>,
}

/// A noise the AI hears: its kind (`ignore`, `interesting`, `danger`,
/// `melee`, `gunshot`) and how far.
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
    /// `OBJECT_ANIMATION_FLAGS` names (`loop`, `pingpong`, `random`,
    /// `autostart`, `localized`). A localized clip is in Max space (a
    /// quarter-turn about X) and placed by its object's origin.
    pub flags: Vec<String>,
    /// As stored; the original steps one frame a tick regardless.
    pub ticks_per_frame: u16,
    pub num_frames: u16,
    pub door_open_frames: u16,
    /// 4×3, applied after each key's rotation.
    pub scale: [f32; 12],
    pub keyframes: Vec<ObjectAnimationKey>,
}

/// An object animation's key: its frame, translation and rotation
/// (a quaternion; Oni's quaternion-to-matrix is the transpose of the usual).
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
    /// Its head's and barrel's models (paths under `common/`); the base is
    /// baked into the level's geometry as `turret_0x<object id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barrel: Option<String>,
    /// In base space.
    pub head_pivot: V3,
    /// In head space.
    pub barrel_pivot: V3,
    /// Ticks it stays on (0: for ever).
    pub timeout: u32,
    pub pitch: Limits,
    pub yaw: Limits,
    pub active_sound: String,
    /// As a gun's.
    pub ai: AiGunParams,
    /// As a character class's.
    pub targeting: Targeting,
    pub skill: ShootSkill,
    /// Its particles; the barrel points along +Z.
    pub attachments: Vec<TurretAttachment>,
}

/// A turning part's limits (radians) and speed (radians a tick).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub min: f32,
    pub max: f32,
    pub speed: f32,
}

/// A turret's particle attachment: its class, which shooter it follows (−1:
/// every shooter), its chamber time (shooters only) and its matrix (4×3,
/// barrel space).
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
    /// Ticks, as are the three below.
    pub predict_position_delay: u32,
    pub predict_delay: u32,
    pub predict_velocity: u32,
    pub predict_trend: u32,
}

/// A shooting skill (`AI2tShootingSkill`; characters' and turrets').
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShootSkill {
    /// 0–1.
    pub recoil_compensation: f32,
    pub best_aiming_sine: f32,
    /// Of the target's distance.
    pub shot_error_percent: f32,
    pub shot_decay: f32,
    pub gun_inaccuracy: f32,
    /// Ticks between shots, at least and at most.
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
    /// Its script id (not unique).
    pub id: u16,
    pub class: String,
    pub position: V3,
    pub axes: [V3; 3],
    /// The teams it fires at (bit n: team n).
    pub target_teams: u32,
    /// `initialActive` (never honoured by the game).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// `levels/NN/corpses.json`: the level's corpse array (`CRSA`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CorpsesFile {
    /// Its size (`max_corpses`, 20 in every level): the static corpses and
    /// room for the dead of play.
    pub max: u32,
    pub corpses: Vec<Corpse>,
}

/// A pre-placed corpse: its class (a character class), each of its 19 body
/// parts' world matrix (4×3) and its bounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corpse {
    /// Empty in the shipped levels.
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
    /// What waypoints and scripts refer to.
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
/// `size` along its axes (unit columns of its rotation), the script
/// functions it runs on entry, inside and exit, who sets it off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TriggerVolume {
    pub id: i32,
    pub name: String,
    pub position: V3,
    pub axes: [V3; 3],
    pub size: V3,
    /// `TEAMS` names: whose members trip it.
    pub teams: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
    /// A volume id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<i32>,
    /// `TRIGGER_FLAGS` names (`entryOnce`, `entryDisabled`, `disabled`,
    /// `playerOnly`, …).
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
    /// What a patrolling character's `jobId` names (not the name).
    pub id: u16,
    pub name: String,
    pub waypoints: Vec<Waypoint>,
    /// Joined (and rejoined) at its nearest waypoint.
    #[serde(default, skip_serializing_if = "is_false")]
    pub return_to_nearest: bool,
}

/// A patrol waypoint (`AI2tWaypoint`; `PATROL_WAYPOINTS`). Ticks keep the
/// data's own widths; angles are radians; `flag` is a flag's id.
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
        /// A `MOVEMENT_MODES` name (`byAlertLevel`, `creep`, `walk`, `run`,
        /// …).
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
        /// Absent: forever.
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

/// `levels/NN/characters.json`: every character the level places (the
/// player's start has the `player` flag).
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
    /// A `TEAMS` name.
    pub team: String,
    /// A `JOBS` name; a `patrol` follows the patrol whose id is `jobId`.
    pub job: String,
    pub alert: AlertLevels,
    pub pursue: PursueModes,
    pub ammo_percent: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<String>,
    /// Its event scripts by event (`spawn`, `die`, `combat`, `alarm`,
    /// `hurt`, `defeated`, `outOfAmmo`, `noPath`), only those set.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    /// Added to its class's hit points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_point_offset: Option<i32>,
    /// Its patrol path or guard's id (0 is a real id; every patroller has
    /// one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<u16>,
    /// Always present (0 is a real id).
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

/// A character setup (`AISA`, `AItCharacterSetup`): what `chr_create
/// <scriptId>` makes, at its default flag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSetup {
    pub script_id: u16,
    /// The flag it starts at.
    pub flag: i16,
    /// A `TEAMS` name (the shipped villains' setups say team 1, `tctf`).
    pub team: String,
    /// Absent: the original names it `ai_<n>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo: Option<u16>,
    /// As placed characters'.
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

/// A combat profile (`OBJtOSD_Combat`): how a placed character with its
/// `combatProfileId` fights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatProfile {
    pub id: u16,
    pub name: String,
    pub behavior: CombatBehaviors,
    pub medium_range: f32,
    /// A `MELEE_WHEN` name (`never`, `ifPunched`, `always`, …).
    pub melee_when: String,
    /// A `NO_GUN` name (`melee`, `retreat`, `alarm`).
    pub no_gun: String,
    pub short_range: f32,
    pub pursuit_distance: f32,
    /// For non-combatants.
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
/// its class (in `common/particle_classes.json`), its matrix (4×3, in its
/// owner's space), its decal scale, where it came from (`env`, `object`),
/// the tag scripts' `particle` command names it by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvParticle {
    pub class: String,
    pub matrix: [f32; 12],
    pub decal_scale: [f32; 2],
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Held back at the start: made only when a script says.
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

/// `levels/NN/items.json`: the level's placed weapons and powerups
/// (written only for levels that place any).
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

/// A placed powerup: its kind (`ammoBallistic`, `ammoEnergy`, `hypo`,
/// `shieldBelt`, `invisibility`, `lsi`) and where (yaw in degrees).
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

/// `levels/NN/sky.json`: the level's sky (`ONSK`; a level without one has
/// no file).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkyFile {
    pub name: String,
    /// The box's textures: top, left, right, front, back (`null`: none).
    pub textures: Vec<Option<String>>,
    pub planets: Vec<Planet>,
    pub star_count: u32,
}

/// A sky's planet (the sun, moons).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Planet {
    pub texture: String,
    /// World units.
    pub size: [f32; 2],
    pub azimuth: f32,
    pub bearing: f32,
}

/// `levels/NN/objects.json`: the level's placed objects (`OBOA`), door
/// leaves aside.
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
    /// A `PHYSICS_LEVELS` name (`none`, `static`, `linear`, `animate`,
    /// `newton`).
    pub physics_level: String,
    pub position: V3,
    /// A quaternion (Oni reads its matrix transposed).
    pub orientation: [f32; 4],
    pub scale: f32,
    /// 4×3.
    pub origin: [f32; 12],
    /// Its models (paths under `common/`).
    pub geometry: Vec<String>,
    pub radius: ObjectRadius,
    /// Its own `ENVP` array, matrices in its space (left out of
    /// `env_particles.json`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<EnvParticle>,
    /// Its object animation's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<String>,
}

/// A placed object's collision sphere radius: as the original computes it
/// (`shipped`), and the largest coordinate (`fixed`; `docs/deviations.md`).
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

/// A neutral behaviour (`OBJtOSD_Neutral`): the conversation a neutral
/// character starts with the player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Neutral {
    pub id: u16,
    pub name: String,
    /// `noResume`, `noResumeAfterItems`, `uninterruptable`.
    pub flags: Vec<String>,
    pub trigger_range: f32,
    pub conversation_range: f32,
    pub follow_range: f32,
    pub abort_enemy_range: f32,
    pub trigger_sound: String,
    pub abort_sound: String,
    /// (An object file older than version 25 has none: `what_the`.)
    pub enemy_sound: String,
    pub end_script: String,
    pub give: NeutralGive,
    /// Up to ten.
    pub lines: Vec<NeutralLine>,
}

/// What a neutral gives the player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeutralGive {
    /// A weapon class, or `none`.
    pub weapon: String,
    pub ballistic_ammo: u8,
    pub energy_ammo: u8,
    pub hypos: u8,
    /// `shield`, `invisibility`, `lsi`.
    pub flags: Vec<String>,
}

/// A neutral conversation's line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeutralLine {
    /// `playerLine`, `giveItem`, `animOnce`, `otherAnimOnce`.
    pub flags: Vec<String>,
    /// What the speaker and the other character play.
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
    /// Feet (a world unit is a third of one).
    pub max_volume_distance: f32,
    pub min_volume_distance: f32,
    /// Copies playing at once.
    pub threshold: u32,
    pub min_occlusion: f32,
    /// `interruptOnStop`, `playOnce`, `pan`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Its parts, by group name (absent: none).
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
    /// 0 low … 3 highest.
    pub priority: u32,
    /// Feet, as ambients'.
    pub max_volume_distance: f32,
    pub min_volume_distance: f32,
    /// Once this many copies play, the impulse `alt` plays instead.
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

/// A sound group's permutation: its weight, volume and pitch ranges
/// (`[min, max]` factors), and its sound (`audio/<sound>.wav`; 85 named
/// sounds don't ship).
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

/// A sound object: its ambient (a name in `common/sounds.json`), where, how
/// loud, and its shape — a sphere (heard while the listener is within
/// `min`, placed at the object) or a box (heard, not placed, while the
/// listener is inside). The data's pitch is never used and not exported.
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
    /// Each texture's material (by texture file stem).
    pub textures: BTreeMap<String, String>,
    /// In the original's order.
    pub entries: Vec<ImpactEntry>,
}

/// An impact effect: what it is for (impact, material, component,
/// modifier), the sound it makes and the particles it sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImpactEntry {
    pub impact: String,
    pub material: String,
    /// `impact`, `damage`, `projectile` (three independent slots).
    pub component: String,
    /// `any`, `heavy`, `medium`, `light`.
    pub modifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<ImpactSound>,
    /// Each made in turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<ImpactParticle>,
}

/// An impact effect's sound, and the noise the AI hears (when it does).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImpactSound {
    /// Possibly empty.
    pub impulse: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiNoise>,
}

/// An impact effect's particle: its class, orientation and where it goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImpactParticle {
    pub class: String,
    /// To the surface struck: `unchanged`, `reversed`, `reflected`, `normal`.
    pub orientation: String,
    pub location: ImpactLocation,
}

/// Where an impact effect's particle goes (`P3tEffectLocationType`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ImpactLocation {
    Default,
    CollisionOffset {
        /// Along the surface's normal.
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
    /// Per variant.
    pub sounds: BTreeMap<String, Vec<AnimSound>>,
}

/// An animation sound: for an animation-sound type (`Kick`, `Punch`,
/// `Jump`, …, or `Animation` with a `clip` and `frame`) and modifier, its
/// impulse.
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
    /// The powerups' looks, by kind (as placed powerups').
    pub powerups: BTreeMap<String, PowerupLook>,
    pub magic_textures: MagicTextures,
    /// The HUD's prompts for what the player could do.
    pub autoprompts: Vec<Autoprompt>,
    /// A character's health colour ramp (melee flashes, the HUD): health
    /// fractions ascending; blended between, the end colours beyond.
    pub health_color: Vec<HealthColor>,
    /// The game's event sounds → impulses (`doorSuccess`, `useHypo`,
    /// `autosave`, …; empty: silent).
    pub event_sounds: BTreeMap<String, String>,
    /// The condition sounds → ambients (`healthLow`, `shield`,
    /// `timerCritical`, …).
    pub condition_sounds: BTreeMap<String, String>,
    pub hypo: HypoSettings,
    /// The most health a hypo can push a character to, × its maximum.
    pub boosted_health: f32,
    pub difficulty: DifficultySettings,
}

/// A powerup's look: its model (a path under `common/`, from level 0 or the
/// first level holding it), the glow sprite (a texture name) and its size
/// and height, and the physics sphere a dropped one falls with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerupLook {
    pub radius: f32,
    pub glow_size: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How far above its place the glow lies: the model's lowest point +
    /// 0.1 (absent without a model).
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
    /// `weapon`, a powerup's name, `usedoor`, `useconsole`, `usealarm`,
    /// `usehypo`, `nci`.
    pub event: String,
    pub levels: [u16; 2],
    /// A name in the messages table.
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
    /// How much harder a super-hypo class hits above full health, from just
    /// past full to double.
    pub overhypo_base_damage: f32,
    pub overhypo_max_damage: f32,
}

/// The difficulty levels and their multipliers (one per level).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DifficultySettings {
    /// `["easy", "normal", "hard"]`.
    pub levels: Vec<String>,
    /// On the AI's melee: attack notice, block and dodge chances.
    pub notice_multiplier: Vec<f32>,
    pub block_multiplier: Vec<f32>,
    pub dodge_multiplier: Vec<f32>,
    /// On the AI's aim.
    pub inaccuracy_multiplier: Vec<f32>,
    pub player_hp_multiplier: Vec<f32>,
    pub enemy_hp_multiplier: Vec<f32>,
}

/// A text run's font (`size`, `style` 0 plain, 1 bold, 2 italic; its shade
/// `#aarrggbb`, when set), resolved over the page's over the dialog's.
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
/// its size and items. (`common/text_console.json`: `dialog_text_console`.)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dialog {
    pub size: [i64; 2],
    pub items: Vec<DialogItem>,
}

/// A dialog's item: its id, window type, title, rectangle and font. In the
/// text console's: 110 the text area, 102 Next, 2 Close, 0 the outline.
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

/// `levels/NN/text_consoles.json`: the level's data consoles (`TxtC`), by
/// name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextConsolesFile {
    pub consoles: BTreeMap<String, TextConsole>,
}

/// A text console's pages (`IGPA` of `IGPG`; their strings `IGSA` of `IGSt`).
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

/// `common/hud_help.json`: the HUD help overlay's rings and labels (level
/// 0's `IGHH` `hud_help_info`).
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
    /// Its middle from the ring's centre.
    pub offset: [i16; 2],
}

/// A HUD help label: its text, offset and which ring it labels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelpItem {
    pub text: String,
    /// From its side's overlay's top-left corner.
    pub offset: [i16; 2],
    pub right: bool,
}

/// `common/menus.json`: the main menu's dialogs, the level names, the
/// out-of-game skin and the popup menus.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MenusFile {
    /// Level 0's out-of-game dialogs, by name without the `dialog_` prefix
    /// (`mainmenu`, `loadgame`, `options`, `NewGame`, `QuitYesNo`,
    /// `ChangeRestart`, `Progress`, `ll_progress`).
    pub dialogs: BTreeMap<String, MenuDialog>,
    /// The level descriptors (`ONLD`).
    pub levels: Vec<MenuLevel>,
    /// The out-of-game UI (`psui_oniUI`): each part (`button_off`,
    /// `checkbox_on`, `sl_thumb`, …) → its name in `part_specs.json`.
    pub skin: Option<BTreeMap<String, String>>,
    /// The popup menus' items by menu name (`pm_difficulty`).
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
    /// `WMcWindowType_*`: 3 box, 4 button, 5 checkbox, 8 list box, 11
    /// picture, 12 popup menu, 13 progress bar, 17 slider, 20 text.
    #[serde(rename = "type")]
    pub kind: i64,
    /// A picture's: its texture.
    pub title: String,
    pub flags: i64,
    /// The control's style bits in the high 16 (a button's `0x10` default;
    /// a text's `0x01`/`0x02`/`0x04` left/centre/right, `0x10` vertically
    /// centred).
    pub style: i64,
    /// x, y, width, height.
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

/// A level as the menus know it: its number, the next one, its name (what
/// the load-game list shows; some name levels that don't ship).
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

/// A part spec (nine-slice frame, `WM_PartSpecification.h`): its texture
/// and its 3 × 3 slices' texel corners (top-left and bottom-right, column
/// by column: the left column top, middle, bottom, then the middle and
/// right). The corners keep their size; edges and middle stretch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartSpec {
    pub texture: String,
    pub tl: Vec<[i64; 2]>,
    pub br: Vec<[i64; 2]>,
}

/// `common/messages.json` (and a level's, if it has its own `SUBT`): the
/// message tables, by name (`messages`,
/// `subtitles`), as stored (with the original's `[x.…]` style spans).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessagesFile {
    pub tables: BTreeMap<String, MessageTable>,
}

/// A message table: by name (`messages`: what scripts' `message` shows), or
/// in order (`subtitles`: a line takes the first whose key its name starts
/// with).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageTable {
    Ordered(Vec<(String, String)>),
    ByName(BTreeMap<String, String>),
}

/// `common/fonts.json`: the font families (`TSFF`), by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FontsFile {
    pub families: BTreeMap<String, FontFamily>,
}

/// A font family (`Tahoma`, …): its fonts, and the characters a line may
/// break after.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FontFamily {
    pub fonts: Vec<FontFile>,
    pub breaking: String,
}

/// A font (`TSFT`): its size, style and metrics, its atlas (a path under
/// `common/`) and its glyphs by character code: `[x, y, width, kerning]`.
/// A glyph's cell in the atlas is `width` wide and ascending + descending
/// high at (`x`, `y`), its bitmap already placed in it by its origin; a
/// string is drawn cell by cell from the top of its line, the pen moving on
/// by each glyph's `kerning`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFile {
    pub size: u16,
    /// `plain`, `bold`, `italic`.
    pub style: String,
    /// Pixels, as the rest; a line is ascending + descending + leading high.
    pub ascending_height: u32,
    pub descending_height: u32,
    pub leading_height: i16,
    pub leading_width: i16,
    pub atlas: String,
    pub glyphs: BTreeMap<String, [u32; 4]>,
}

/// `common/texture_animations.json`: the animated textures, by file stem.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TextureAnimationsFile {
    pub animations: BTreeMap<String, TextureAnimation>,
}

/// An animated texture: its frames (paths from the converted tree's root,
/// the texture itself first; the `TXAN`'s others are often unnamed,
/// `TXMP_<index>`), how long each shows (ticks) and how it runs
/// (`pingPong`, `randomFrame`, `randomStart`, `dontAnimate`: the texture's
/// own `M3cTextureFlags_Anim_*`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureAnimation {
    pub time_per_frame: u16,
    pub frames: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// `levels/NN/navigation.json`: the level's pathfinding rooms (`BNV`), in
/// world coordinates (as `environment.glb`), their grids in a binary file
/// beside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationFile {
    /// A grid square's size (4).
    pub square_size: f32,
    /// The grid cells' kinds, by value (`clear`, `nearWall`, …, `danger`,
    /// `impassable`).
    pub square_types: Vec<String>,
    /// The grids' file (beside this one): `navigation.bin`, the rooms'
    /// grids back to back, one byte per square.
    pub grids: String,
    pub rooms: Vec<NavRoom>,
}

/// A pathfinding room: its flags (`ROOM_FLAGS`), bounds, side planes,
/// links to its neighbours, its stair, and its grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavRoom {
    pub flags: Vec<String>,
    pub min: V3,
    pub max: V3,
    /// A convex volume: a point is inside when `a·x + b·y + c·z + d ≤ 0`
    /// for every plane.
    pub planes: Vec<[f32; 4]>,
    /// The rooms it opens onto (links come in matching pairs).
    pub links: Vec<NavLink>,
    /// A stair room contains only points where `0 ≤ plane·p ≤ height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stair: Option<NavStair>,
    /// Absent (a "simple" room): all clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<NavGrid>,
}

/// A link to a neighbouring room (its index): its portal's flags
/// (`PORTAL_FLAGS`: `ghost`, `door`, `stairsUp`, `stairsDown`) and quad,
/// whose two lowest corners are the portal's edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavLink {
    pub to: u32,
    pub portal: Vec<String>,
    pub quad: [V3; 4],
}

/// A room's stair: its plane and height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavStair {
    pub plane: [f32; 4],
    pub height: f32,
}

/// A room's grid: where it starts, its size in squares, and where its
/// cells sit in the grids' file — `width × height` bytes at `offset`,
/// row-major (`x + z·width`), each indexing `squareTypes`. Cell `(i, j)`
/// covers world x `[origin.x + 4i, +4)`, z `[origin.z + 4j, +4)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavGrid {
    pub origin: [f32; 2],
    pub width: u32,
    pub height: u32,
    pub offset: usize,
}

/// `common/weapons.json`: the weapon classes (`ONWC`), by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WeaponsFile {
    pub classes: BTreeMap<String, WeaponClass>,
}

/// A weapon class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponClass {
    pub name: String,
    /// Its model (a path under `common/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `twoHanded`, `autofire`, `energy`, `hasAlternate`, `hasLaser`, …,
    /// the AI hints `aiStun` / `aiKnockdown` / `aiSingleShot`, the cursor
    /// ones.
    pub flags: Vec<String>,
    pub max_ammo: u16,
    pub shooter_count: u16,
    /// Ticks, as the next two.
    pub reload_time: u16,
    pub reload_delay_time: u16,
    pub stop_chamber_threshold: u16,
    pub recoil_anim_type: AnimRef,
    pub reload_anim_type: AnimRef,
    pub aiming_speed: f32,
    pub recoil: WeaponRecoil,
    /// The laser marker, in gun space.
    pub sight: WeaponSight,
    pub max_sight_distance: f32,
    /// `#aarrggbb`.
    pub laser_color: String,
    /// In gun space, as the offsets below.
    pub second_hand: SecondHand,
    pub pickup_offset: V3,
    pub display_y_offset: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_sound: Option<String>,
    /// How the AI shoots it, per shooter.
    pub ai: AiGunParams,
    /// Its second shooter's (with more than one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_alternate: Option<AiGunParams>,
    /// The particles riding the gun.
    pub attachments: Vec<WeaponAttachment>,
    /// Its physics sphere's radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sphere_radius: Option<f32>,
    /// The reticle, the reticle on a target, the sight's tunnel (absent
    /// without a texture).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<WeaponSprite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_targeted: Option<WeaponSprite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<WeaponTunnel>,
    /// The HUD's textures: its icon, the empty and the full ammo ring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_empty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_fill: Option<String>,
    /// Its instance name (its new-weapon message's key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

/// A weapon's recoil (radians).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponRecoil {
    /// Already in the internal per-shot units.
    pub base: f32,
    pub max: f32,
    pub factor: f32,
    pub return_speed: f32,
    pub firing_return_speed: f32,
}

/// Where a weapon's sight is and points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponSight {
    pub position: V3,
    pub vector: V3,
}

/// Where a two-handed weapon's second hand goes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SecondHand {
    pub position: V3,
    pub vector: V3,
    pub up: V3,
}

/// A weapon's particle attachment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponAttachment {
    pub class: String,
    /// 4×3, gun space.
    pub matrix: [f32; 12],
    /// Non-zero: a shooter, using this much ammo a shot.
    pub ammo_used: u16,
    /// Ticks, as the other times.
    pub chamber_time: u16,
    pub cheat_chamber_time: u16,
    pub min_fire_time: u16,
    /// A shooter's index; for a follower the shooter it follows (−1: every
    /// shooter).
    pub shooter_index: i32,
    pub start_delay: u16,
}

/// A weapon sprite (`WPtSprite`): its texture, shade (`#aarrggbb`) and
/// scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponSprite {
    pub texture: String,
    pub shade: String,
    pub scale: f32,
}

/// A weapon sight's tunnel: its sprite, how many and how far apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponTunnel {
    pub texture: String,
    pub shade: String,
    pub scale: f32,
    pub count: u32,
    pub spacing: f32,
}

/// `common/characters/classes.json`: the character classes (`ONCC`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterClassesFile {
    pub count: usize,
    pub classes: Vec<CharacterClass>,
}

/// A character class: its body, textures and animations, how it moves,
/// fights, sounds and sees.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterClass {
    pub name: Option<String>,
    /// Its variant's `ONCV` instance.
    pub variant: Option<String>,
    /// Its particles (`ONCP`): on a body part, its animations, or its hit
    /// flash.
    pub particles: Vec<ClassParticle>,
    /// That variant's name, what classes are matched by.
    pub variant_name: Option<String>,
    /// The variant it becomes on hard, if any.
    pub variant_upgrade: Option<String>,
    pub body: Option<String>,
    /// Its body's `model.glb` (a path under `common/characters/`).
    pub model: Option<String>,
    /// Its texture set's (`TRMA`) name.
    pub textures: Option<String>,
    /// Its own skin: each body part's texture, in part order (a path from
    /// this file's folder). Classes share bodies with different skins, so
    /// the one baked into `model.glb` is only a default.
    pub part_textures: Vec<Option<String>>,
    /// The name of its aiming screens' set.
    pub aiming_screens: Option<String>,
    pub aiming: Vec<AimingScreen>,
    pub animation_collection: Option<String>,
    pub animation_count: usize,
    /// Its clips, the collection resolved.
    pub animations: Vec<String>,
    pub stats: Option<ClassStats>,
    pub air: Option<ClassAir>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim_impacts: Option<AnimImpacts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pain: Option<PainConstants>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocalizations: Option<Vec<Vocalization>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_impacts: Option<Vec<AttackImpact>>,
    /// Per body part (19, part order), the material struck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_materials: Option<Vec<Option<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_impacts: Option<PartImpacts>,
    /// Its default melee profile, by name.
    pub melee_profile: Option<String>,
    /// Its melee profile's id, while the converter resolves it (not written).
    #[serde(skip)]
    pub melee_profile_id: Option<i64>,
    /// Ticks lying stunned after a knockdown, at least and at most (the
    /// game uses these, not its melee profile's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dazed: Option<Dazed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perception: Option<ClassPerception>,
    /// A hypo heals a point each time this many ticks run out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypo_time: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ClassShadow>,
}

/// A class's particle: its name, class, and where (a body part's number,
/// `animation` or `hitFlash`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassParticle {
    pub name: String,
    pub class: String,
    pub part: AnimRef,
}

/// An aiming screen: its clip, whose frames are a grid of upper-body poses
/// (frame = row × width + column, the straight pose at the middle), and its
/// yaw and pitch steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AimingScreen {
    pub clip: String,
    pub yaw: AimSteps,
    pub pitch: AimSteps,
}

/// An aiming screen's steps one way: the angle between frames (radians) and
/// how many frames, each side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AimSteps {
    pub negative_delta: f32,
    pub positive_delta: f32,
    pub negative_frames: i16,
    pub positive_frames: i16,
}

/// A class's stats (times in ticks).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassStats {
    pub max_hit_points: u32,
    pub scale: [f32; 2],
    /// By damage type (only those not zero).
    pub damage_resistance: BTreeMap<String, f32>,
    pub boss_shield: f32,
    pub shot_time: u16,
    pub fight_mode_duration: u32,
    pub idle_delay: u32,
    pub idle_restart_delay: u32,
    pub death_delete_delay: u16,
    pub feet_bits: u32,
    pub left_handed: bool,
    pub super_hypo: bool,
    pub super_shield: bool,
    pub knockdown_resistant: bool,
}

/// A class's air constants (ticks, units per tick, units per tick²).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassAir {
    pub fall_gravity: f32,
    pub jump_gravity: f32,
    pub jump_amount: f32,
    pub max_velocity: f32,
    pub jump_accel_amount: f32,
    pub frames_fall_gravity: u16,
    pub num_jump_accel_frames: u16,
    pub damage_start: f32,
    pub damage_end: f32,
}

/// The impact types a class's movements make, and its steps' weight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimImpacts {
    pub modifier: String,
    /// Walk, run, crouch, slide, land, land hard, land dead, knockdown,
    /// thrown, standing turn, run start, one step, run stop, walk stop,
    /// sprint.
    pub impacts: Vec<String>,
}

/// How a class cries out (`painConstants`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PainConstants {
    pub base_percentage: u16,
    pub max_percentage: u16,
    pub percentage_threshold: u16,
    pub timer: u16,
    pub min_timer: u16,
    pub max_light: u16,
    pub max_medium: u16,
    pub death_chance: u16,
    pub volume_threshold: u16,
    pub medium_threshold: u16,
    pub heavy_threshold: u16,
    pub min_volume: f32,
    pub light: String,
    pub medium: String,
    pub heavy: String,
    pub death: String,
}

/// What a class's AI says: a chance (%) and an ambient, per kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vocalization {
    pub kind: String,
    pub chance: u8,
    pub sound: String,
}

/// A special attack's (by name) impact type and weight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttackImpact {
    pub attack: String,
    pub impact: String,
    pub modifier: String,
}

/// Per body part, the impact a blow with it makes on a hit, a block, a kill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartImpacts {
    pub hit: Vec<Option<String>>,
    pub blocked: Vec<Option<String>>,
    pub killed: Vec<Option<String>>,
}

/// Ticks dazed, at least and at most.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dazed {
    pub min: i64,
    pub max: i64,
}

/// A class's AI perception and combat settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassPerception {
    pub vision: Vision,
    pub hearing_acuity: f32,
    pub contact_decay: ContactDecay,
    pub alert_decay: AlertDecay,
    /// `AI_FLAGS` names.
    pub flags: Vec<String>,
    pub turning_nimbleness: f32,
    pub combat: ClassCombatSettings,
    pub dodge: DodgeSettings,
    pub targeting: Targeting,
    /// 13 slots, a gun picking one by its `shootSkillIndex`.
    pub shooting_skill: Vec<ShootSkill>,
}

/// How far and wide a class sees (angles in radians).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vision {
    pub central_distance: f32,
    pub peripheral_distance: f32,
    pub vertical_angle: f32,
    pub central_full_angle: f32,
    pub central_max_angle: f32,
    pub peripheral_full_angle: f32,
    pub peripheral_max_angle: f32,
}

/// Ticks a contact takes to weaken a step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactDecay {
    pub definite: i64,
    pub strong: i64,
    pub weak: i64,
}

/// Ticks an alert level takes to fall a step (0: never).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertDecay {
    pub high: i64,
    pub medium: i64,
    pub low: i64,
}

/// A class's combat timings, and the combat profile an unplaced character
/// fights by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassCombatSettings {
    pub make_sure_ticks: i64,
    pub investigate_body_ticks: i64,
    pub lost_contact_ticks: i64,
    pub dead_taunt_percent: i64,
    pub gun_pickup_percent: i64,
    pub gun_running_pickup_percent: i64,
    pub default_combat_profile_id: i64,
}

/// How a class dodges projectiles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DodgeSettings {
    pub react_ticks: i64,
    pub time_scale: f32,
    pub weight_scale: f32,
}

/// A class's shadow (every shipped class: `shadow1`, heights 8 / 12, sizes
/// 6 / 4.5 / 3, alphas 48 / 48).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassShadow {
    pub texture: Option<String>,
    /// Where it is gone, and where it starts fading (above the floor).
    pub height_max: f32,
    pub height_fade: f32,
    /// Half-sizes on the ground, at the fade, at the top.
    pub size_max: f32,
    pub size_fade: f32,
    pub size_min: f32,
    /// Opacities (0–255).
    pub alpha_max: i64,
    pub alpha_fade: i64,
}

/// `common/characters/melee_profiles.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeleeProfilesFile {
    pub profiles: Vec<MeleeProfile>,
}

/// A melee profile (`ONCC`'s `CMPr`): its techniques and how it weighs
/// them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeleeProfile {
    pub id: u32,
    /// What classes' `meleeProfile` names.
    pub name: String,
    /// The class whose animations and jump physics the AI plans with.
    pub class: String,
    pub attack_notice_percent: u32,
    pub dodge: MeleeDodge,
    pub block: MeleeBlock,
    pub weights: MeleeWeights,
    pub dazed: MeleeDazed,
    /// Attacks, taken with an attack slot.
    pub actions: Vec<Technique>,
    /// Evasions when attacked.
    pub reactions: Vec<Technique>,
    /// Moving around while waiting for a slot.
    pub spacing: Vec<Technique>,
}

/// A melee profile's dodge chances.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeleeDodge {
    pub base_percent: u32,
    pub additional_percent: u32,
    pub damage_threshold: u32,
}

/// A melee profile's block chances.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeleeBlock {
    pub skill_percent: u32,
    pub group_percent: u32,
}

/// A melee profile's technique weights.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeleeWeights {
    pub target_not_blocking: f32,
    pub target_wrong_stance: f32,
    pub blocked_but_unblockable: f32,
    pub blocked_but_staggers: f32,
    pub blocked_but_block_stun: f32,
    pub blocked: f32,
    pub throw_into_danger: f32,
}

/// Ticks dazed, at least and at most (a melee profile's; the original
/// ignores it for the class's `dazed`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeleeDazed {
    pub min: u16,
    pub max: u16,
}

/// A melee technique: its moves and how it is chosen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Technique {
    pub name: String,
    pub weight: u32,
    /// Unused by the game.
    pub importance: u32,
    /// Ticks.
    pub repeat_delay: u32,
    pub moves: Vec<MeleeMove>,
    /// `TECHNIQUE_FLAGS`: `interruptible`, `generousDirection`, `fearless`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// A melee move: its type (`attack`, `position`, `maneuver`, `evade`,
/// `throw`, `unknown`), which move, and the parameters it has (times in
/// ticks, angles in radians).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeleeMove {
    #[serde(rename = "type")]
    pub kind: String,
    /// A name from the type's `MELEE_*` table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "move")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<u32>,
    /// Run / close / run-jump positions: run-in range and tolerance (units;
    /// spacing maneuvers use `tolerance` with `targetRange`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_run_in: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_run_in: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f32>,
    /// The random-stop maneuver's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chance_percent: Option<f32>,
    /// Maneuvers' range (with `minRange`, `threshold`); also Barabas' wave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_range: Option<f32>,
    /// Maneuvers'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_range: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_range: Option<f32>,
    /// Circling's angles (spacing: `closenessAngle`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closeness_angle: Option<f32>,
}

/// `common/animations/collections/<name>.json`: an animation collection
/// (`TRAC`) — a character class's clips (files beside the collections
/// folder) and their weights, and the collection it extends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnimationCollectionFile {
    pub name: String,
    /// The collection consulted when this one has no exact match (null at
    /// the end of the chain).
    pub recursive: Option<String>,
    /// In the data's order.
    pub entries: Vec<CollectionEntry>,
}

/// A collection's clip and its relative selection weight among clips that
/// answer the same request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectionEntry {
    pub anim: String,
    pub weight: u16,
}

/// A name from a table, or the number when the table has none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Named {
    Name(String),
    Number(u32),
}

impl Named {
    /// Its name (none for a bare number).
    pub fn name(&self) -> Option<&str> {
        match self {
            Named::Name(n) => Some(n),
            Named::Number(_) => None,
        }
    }
}

/// `common/particle_classes.json`: the particle classes (`BINA` `PAR3`), by
/// name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParticleClassesFile {
    #[serde(default)]
    pub count: usize,
    pub classes: BTreeMap<String, ParticleClass>,
}

/// A particle value (`P3tValue`): a number (constant), a string, a
/// variable, a range (`[lo, hi]`) or bell curve (`[mean, sd]`), a shade
/// (`#aarrggbb`) or shade range or bell, an enum, a cycle (`[length, max]`);
/// `null` where it is used: unset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParticleValue {
    Number(f32),
    Text(String),
    Var {
        var: String,
    },
    Range {
        range: [f32; 2],
    },
    Bell {
        bell: [f32; 2],
    },
    Shade {
        shade: String,
    },
    ShadeRange {
        #[serde(rename = "shadeRange")]
        shade_range: [String; 2],
    },
    ShadeBell {
        #[serde(rename = "shadeBell")]
        shade_bell: [String; 2],
    },
    Enum {
        #[serde(rename = "enum")]
        value: u32,
    },
    Cycle {
        cycle: [f32; 2],
    },
    Unknown {
        unknown: u32,
    },
}

/// A particle class: its flags, appearance, variables, event action lists,
/// emitters and attractor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticleClass {
    /// `decorative`, `geometry`, `collideEnv`, `collideChar`, and the
    /// optional per-particle fields (`hasVelocity`, `hasOwner`,
    /// `hasLensFrames`, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_type: Option<u32>,
    /// `lockToLinkedParticle`, `dieOnCutscene`, `invisible`, `decal`,
    /// `edgeFade`, `lensFlare`, ….
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags2: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_mask: Option<u32>,
    /// Seconds (absent: unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_radius: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dodge_radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alert_radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flyby_sound: Option<String>,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attractor: Option<Attractor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<ParticleVariable>,
    /// Each event's actions: `[first, end)` into `actions`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub events: BTreeMap<String, [u16; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ParticleAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emitters: Vec<ParticleEmitter>,
}

/// How a particle class looks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_scale: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<ParticleValue>,
    /// Contrails: fading out edge-on (when `flags2` has `edgeFade`), and the
    /// longest link drawn; absent when 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_fade_min: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_fade_max: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_contrail: Option<ParticleValue>,
    /// Lens flares (`flags2` has `lensFlare`): how far behind something its
    /// point still counts as seen, the frames it fades in and out over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lensflare_dist: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens_in_frames: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens_out_frames: Option<u16>,
    /// Decals: how many at once, frames to fade out, how far one wraps
    /// (degrees).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_decals: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decal_fade_frames: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decal_wrap: Option<ParticleValue>,
    /// Its sprite's texture or its geometry's model, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

/// A particle class's attractor: what it is drawn to and how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attractor {
    pub iterator: Named,
    pub selector: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_distance: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_angle: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theta_min: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theta_max: Option<ParticleValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theta_weight: Option<ParticleValue>,
}

/// A particle class's variable: its name, data type, initial value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleVariable {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub init: Option<ParticleValue>,
}

/// An action: its type, the variables it writes and its parameters (values;
/// unset slots null, trailing ones left out).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleAction {
    #[serde(rename = "type")]
    pub kind: Named,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vars: Vec<Option<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Option<ParticleValue>>,
}

/// An emitter: the class it emits and how (rate, position, direction,
/// speed, each a kind and its values), linked and oriented how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticleEmitter {
    pub class: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    pub limit: u16,
    /// 0–65535.
    pub chance: u16,
    pub burst: f32,
    pub link: Named,
    pub rate: EmitterSpec,
    pub position: EmitterSpec,
    pub direction: EmitterSpec,
    pub speed: EmitterSpec,
    pub orient_dir: Named,
    pub orient_up: Named,
}

/// An emitter's rate, position, direction or speed: its kind and values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmitterSpec {
    #[serde(rename = "type")]
    pub kind: Named,
    pub values: Vec<Option<ParticleValue>>,
}

/// An animation clip's metadata (`TRAM`), in its glTF file's
/// `animations[0].extras` (the file itself: `convert_animations` in
/// `oni-convert`): enumerations by name, defaults left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipExtras {
    /// What the clip does (`ANIM_TYPES`: `run`, `punchForward`, …). Always
    /// present.
    pub anim_type: String,
    /// The body state it starts from and ends in (`ANIM_STATES`). Always
    /// present; `none`: reachable only through `shortcuts`.
    pub from_state: String,
    pub to_state: String,
    /// `VARIANTS` a character must have for the clip to be eligible
    /// (`fight`, `sprint`, weapon holds, `panic`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variant: Vec<String>,
    /// Variants on exit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variant_end: Vec<String>,
    /// `ANIM_FLAGS` names (`invulnerable`, `blockHigh`, `atomic`, `inAir`,
    /// `throwSource`, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Per tick, the character-local ground displacement `[x, z]` (absent:
    /// stands still).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root_motion: Vec<[f32; 2]>,
    /// Per tick, the pelvis height above the feet (absent: constant 6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub root_height: Vec<f32>,
    /// The net yaw the clip turns the body through, committed to the facing
    /// when it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_rotation: Option<f32>,
    /// Which way it moves and strikes (`DIRECTIONS`: `forward`, `back`,
    /// `left`, `right`, `any`; absent: `none`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_direction: Option<String>,
    /// The animation type of the aiming pose used with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aiming_type: Option<String>,
    /// Body-part bit masks: the parts it moves; played as an overlay, the
    /// parts it replaces (the rest of `usedParts` it layers on top).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_parts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_parts: Option<u32>,
    /// Extra states it may be entered from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shortcuts: Vec<ClipShortcut>,
    /// Clips that follow this one with no pause and no blend.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct: Vec<String>,
    /// The ticks it can't be broken off (absent: never; no end: to the end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atomic: Option<AtomicRange>,
    /// Ticks immune to hits (absent: none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invulnerable: Option<Invulnerable>,
    /// Unavailable before this level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_level: Option<u16>,
    /// Blend length out of it when it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_blend: Option<u16>,
    /// Cap on the blend into it (absent: none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_blend_in: Option<u16>,
    /// After it, only resting clips may start (soft: blocking too) for this
    /// long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_pause: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_pause: Option<u16>,
    /// When its action fires (a pickup, a weapon swap, a throw's disarm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_frame: Option<i16>,
    /// The voice line it triggers (`VOCALIZATIONS`: `taunt`, `alert`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocalization: Option<String>,
    /// Its strikes (at most two).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attacks: Vec<ClipAttack>,
    /// Present on clips that throw someone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throw: Option<ClipThrow>,
    /// For attack clips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<ClipReach>,
    /// Per tick `[x, forward, floor, height]`: the root's offset from the
    /// clip's start, the body's lowest point above the floor, its height
    /// (toe to head).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_track: Vec<[f32; 4]>,
    /// Damage the character playing it takes (how a thrown victim is hurt).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub self_damage: Vec<SelfDamage>,
    /// The ticks a foot comes down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footsteps: Vec<u16>,
    /// Each footstep's kind (1 left, 2 right, 3 single); written whenever
    /// `footsteps` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footstep_kinds: Option<Vec<u16>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub particles: Vec<ClipParticle>,
    /// The special attack it is (a class's `attackImpacts` key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_frame: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<ClipSound>,
}

/// A shortcut into a clip: from a state, over a blend (ticks);
/// `interruptsAtomic` lets it cut into an uninterruptible clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipShortcut {
    pub from_state: String,
    pub blend_length: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub interrupts_atomic: bool,
}

/// A clip's atomic ticks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AtomicRange {
    pub start: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u16>,
}

/// A clip's invulnerable ticks (`length` from `start`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invulnerable {
    pub start: u8,
    pub length: u8,
}

/// A clip's attack: when it hurts, how much, with which parts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipAttack {
    /// When it can land (inclusive).
    pub damage_window: FrameRange,
    /// Hit points taken on a clean hit.
    pub damage: u16,
    /// The total shove given the victim.
    pub knockback: f32,
    /// The attacker's body parts that strike (the character model's node
    /// names).
    pub striking_parts: Vec<String>,
    /// Which of two simultaneous strikes wins.
    pub precedence: u16,
    /// The animation type the victim plays (`hitHead`, `knockdownBody`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
    /// `ATTACK_FLAGS`: `unblockable`, `high`, `low`, `special` (breaks
    /// through blocks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Ticks the victim is held after a hit / a block / a stagger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_stun: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_stun: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagger: Option<u16>,
}

/// A span of frames.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameRange {
    pub start: u16,
    pub end: u16,
}

/// A throw clip's victim: where it goes and faces, the range, its clip type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipThrow {
    /// In the thrower's local frame.
    pub target_offset: V3,
    /// Relative to the thrower's (π: facing the thrower).
    pub target_facing: f32,
    /// How far away the victim may be for the throw to take.
    pub range: f32,
    /// One of the `thrownN` types.
    pub target_anim_type: String,
}

/// An attack clip's reach (`TRAM`'s extents): how far and high it strikes,
/// as the original's importer computes it from the active attack's
/// striking parts at every damage tick. Character-local: `x` left,
/// `forward` ahead, heights above the floor, angles 0 ahead growing toward
/// the left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipReach {
    /// The farthest any strike gets from where the clip starts, and the
    /// strikes' height span.
    pub max_distance: f32,
    pub min_height: f32,
    pub max_height: f32,
    /// 36 spokes, 10° apart: spoke *i* the farthest reach within ±30° of
    /// direction *i*·10° (0: none).
    pub spokes: Vec<f32>,
    /// The first strike, and the one reaching farthest from the start.
    pub first: StrikeBound,
    pub farthest: StrikeBound,
    /// Which way the strikes reach most (used when `moveDirection` is
    /// absent).
    pub direction: String,
    /// Each damage tick, from that tick's root.
    pub strikes: Vec<StrikeReach>,
}

/// One end of an attack clip's reach: its root offset `[x, forward]` and
/// pelvis height then, its reach along the attack direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrikeBound {
    pub tick: u16,
    pub attack: u8,
    pub strike: u8,
    pub offset: [f32; 2],
    pub height: f32,
    pub distance: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub angle: f32,
}

/// A strike's reach at a tick: along the attack direction, the bearing of
/// its farthest point, its height span.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrikeReach {
    pub tick: u16,
    pub angle: f32,
    pub distance: f32,
    pub min_height: f32,
    pub max_height: f32,
}

/// Damage a clip does its own character, at a tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelfDamage {
    pub tick: u16,
    pub damage: u16,
}

/// A clip's particle: from tick `start` to `stop`, on a body part (its
/// number), the class particle's `name`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipParticle {
    pub start: u16,
    pub stop: u16,
    pub part: u16,
    pub name: String,
}

/// A clip's impulse sound at a frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipSound {
    pub impulse: String,
    pub frame: u16,
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
