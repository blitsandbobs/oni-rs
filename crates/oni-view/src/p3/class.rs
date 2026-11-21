//! Particle classes, loaded from `common/particle_classes.json`
//! (`docs/ASSET_FORMAT.md`, *Particle classes*) into typed definitions with
//! variable references, emitter classes and effect classes resolved.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use serde_json::Value as Json;

/// A class index into [`Classes::list`].
pub type ClassId = usize;

/// A stored value (`P3tValue`). Variable references are resolved to the
/// variable's index; an unknown or empty one is `Null`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Var(usize),
    Float(f32),
    Range(f32, f32),
    Bell(f32, f32),
    Str(Rc<str>),
    Shade(u32),
    ShadeRange(u32, u32),
    ShadeBell(u32, u32),
    Enum(u32),
    Cycle(f32, f32),
}

/// A variable's data type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Float,
    Str,
    Shade,
    Enum,
    Sound,
}

pub struct VarDef {
    pub name: String,
    pub kind: VarKind,
    pub init: Value,
}

/// Action types (`P3tActionType`), the ones the runtime knows by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    AnimateLinear,
    AnimateAccel,
    AnimateRandom,
    AnimatePingpong,
    AnimateLoop,
    AnimateToValue,
    ColorInterpolate,
    FadeOut,
    EnableAtTime,
    DisableAtTime,
    Die,
    SetLifetime,
    EmitActivate,
    EmitDeactivate,
    EmitParticles,
    ChangeClass,
    KillLastEmitted,
    ExplodeLastEmitted,
    SoundStart,
    SoundEnd,
    ImpulseSound,
    DamageHitChar,
    DamageBlast,
    Explode,
    DamageEnvironment,
    GlassCharge,
    Stop,
    RotateX,
    RotateY,
    RotateZ,
    FindAttractor,
    AttractGravity,
    AttractHoming,
    AttractSpring,
    MoveLine,
    MoveGravity,
    MoveSpiral,
    MoveResistance,
    MoveDrift,
    SetVelocity,
    SpiralTangent,
    KillBeyondPoint,
    CollisionEffect,
    StickToWall,
    Bounce,
    Chop,
    ImpactEffect,
    Show,
    Hide,
    SetVariable,
    RecalculateAll,
    EnableAbove,
    EnableBelow,
    EnableNow,
    DisableNow,
    SuperballTrigger,
    StopIfBreakable,
    AvoidWalls,
    RandomSwirl,
    StopIfSlow,
    StopLink,
    BreakLink,
    SetTextureTick,
    RandomTextureFrame,
    SuperParticle,
    /// Known to the data but without an effect here (render-only, or no
    /// procedure in the original).
    Other,
}

impl Kind {
    fn from_name(s: &str) -> Kind {
        use Kind::*;
        match s {
            "animateLinear" => AnimateLinear,
            "animateAccel" => AnimateAccel,
            "animateRandom" => AnimateRandom,
            "animatePingpong" => AnimatePingpong,
            "animateLoop" => AnimateLoop,
            "animateToValue" => AnimateToValue,
            "colorInterpolate" => ColorInterpolate,
            "fadeOut" => FadeOut,
            "enableAtTime" => EnableAtTime,
            "disableAtTime" => DisableAtTime,
            "die" => Die,
            "setLifetime" => SetLifetime,
            "emitActivate" => EmitActivate,
            "emitDeactivate" => EmitDeactivate,
            "emitParticles" => EmitParticles,
            "changeClass" => ChangeClass,
            "killLastEmitted" => KillLastEmitted,
            "explodeLastEmitted" => ExplodeLastEmitted,
            "soundStart" => SoundStart,
            "soundEnd" => SoundEnd,
            "impulseSound" => ImpulseSound,
            "damageHitChar" => DamageHitChar,
            "damageBlast" => DamageBlast,
            "explode" => Explode,
            "damageEnvironment" => DamageEnvironment,
            "glassCharge" => GlassCharge,
            "stop" => Stop,
            "rotateX" => RotateX,
            "rotateY" => RotateY,
            "rotateZ" => RotateZ,
            "findAttractor" => FindAttractor,
            "attractGravity" => AttractGravity,
            "attractHoming" => AttractHoming,
            "attractSpring" => AttractSpring,
            "moveLine" => MoveLine,
            "moveGravity" => MoveGravity,
            "moveSpiral" => MoveSpiral,
            "moveResistance" => MoveResistance,
            "moveDrift" => MoveDrift,
            "setVelocity" => SetVelocity,
            "spiralTangent" => SpiralTangent,
            "killBeyondPoint" => KillBeyondPoint,
            "collisionEffect" => CollisionEffect,
            "stickToWall" => StickToWall,
            "bounce" => Bounce,
            "chop" => Chop,
            "impactEffect" => ImpactEffect,
            "show" => Show,
            "hide" => Hide,
            "setVariable" => SetVariable,
            "recalculateAll" => RecalculateAll,
            "enableAbove" => EnableAbove,
            "enableBelow" => EnableBelow,
            "enableNow" => EnableNow,
            "disableNow" => DisableNow,
            "superballTrigger" => SuperballTrigger,
            "stopIfBreakable" => StopIfBreakable,
            "avoidWalls" => AvoidWalls,
            "randomSwirl" => RandomSwirl,
            "stopIfSlow" => StopIfSlow,
            "stopLink" => StopLink,
            "breakLink" => BreakLink,
            "setTextureTick" => SetTextureTick,
            "randomTextureFrame" => RandomTextureFrame,
            "superParticle" => SuperParticle,
            _ => Other,
        }
    }

    /// Skipped while the particle is stopped (the motion family).
    pub fn not_if_stopped(self) -> bool {
        use Kind::*;
        matches!(
            self,
            MoveLine
                | MoveGravity
                | MoveSpiral
                | MoveResistance
                | MoveDrift
                | SetVelocity
                | SpiralTangent
                | KillBeyondPoint
        )
    }

    /// Skipped while the particle is chopped (damage and effects).
    pub fn not_if_chopped(self) -> bool {
        use Kind::*;
        matches!(
            self,
            SetLifetime
                | EmitParticles
                | ChangeClass
                | DamageHitChar
                | DamageBlast
                | Explode
                | DamageEnvironment
                | GlassCharge
                | Stop
                | CollisionEffect
                | StickToWall
                | Bounce
                | Chop
                | ImpactEffect
        )
    }

    /// Skipped in a step that ends in a collision (velocity changes that
    /// would act on the pre-collision path).
    pub fn not_if_collision(self) -> bool {
        use Kind::*;
        matches!(
            self,
            MoveGravity | MoveSpiral | MoveResistance | MoveDrift | SetVelocity | SpiralTangent
        )
    }
}

pub struct Action {
    pub kind: Kind,
    /// The variables it writes, resolved (`None` = unset reference).
    pub vars: [Option<usize>; 8],
    pub params: Vec<Value>,
}

impl Action {
    pub fn param(&self, k: usize) -> &Value {
        self.params.get(k).unwrap_or(&Value::Null)
    }
}

/// Events (`P3tEventType`).
pub mod event {
    pub const UPDATE: usize = 0;
    pub const PULSE: usize = 1;
    pub const START: usize = 2;
    pub const STOP: usize = 3;
    pub const BG_START: usize = 4;
    pub const BG_STOP: usize = 5;
    pub const COLLIDE_ENV: usize = 6;
    pub const COLLIDE_CHAR: usize = 7;
    pub const LIFETIME: usize = 8;
    pub const EXPLODE: usize = 9;
    pub const BROKEN_LINK: usize = 10;
    pub const CREATE: usize = 11;
    pub const DIE: usize = 12;
    pub const NEW_ATTRACTOR: usize = 13;
    pub const DELAY_START: usize = 14;
    pub const DELAY_STOP: usize = 15;
}
const EVENT_NAMES: [&str; 16] = [
    "update",
    "pulse",
    "start",
    "stop",
    "bgStart",
    "bgStop",
    "collideEnv",
    "collideChar",
    "lifetime",
    "explode",
    "brokenLink",
    "create",
    "die",
    "newAttractor",
    "delayStart",
    "delayStop",
];

/// Emitter flag bits.
pub mod eflag {
    pub const INITIALLY_ACTIVE: u32 = 1 << 0;
    pub const INCREASE_COUNT: u32 = 1 << 1;
    pub const DEACTIVATE_AT_LIMIT: u32 = 1 << 2;
    pub const ADD_PARENT_VELOCITY: u32 = 1 << 4;
    pub const ORIENT_TO_VELOCITY: u32 = 1 << 6;
    pub const ONE_PER_ATTRACTOR: u32 = 1 << 8;
    pub const AT_LEAST_ONE: u32 = 1 << 9;
    pub const ROTATE_ATTRACTORS: u32 = 1 << 10;
    /// The child takes the parent's colour.
    pub const INHERIT_TINT: u32 = 1 << 7;
}
const EMITTER_FLAGS: [&str; 11] = [
    "initiallyActive",
    "increaseEmitCount",
    "deactivateWhenEmitCountThreshold",
    "activateWhenEmitCountThreshold",
    "addParentVelocity",
    "sameDynamicMatrix",
    "orientToVelocity",
    "inheritTint",
    "onePerAttractor",
    "atLeastOne",
    "rotateAttractors",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    None,
    Emitter,
    LastEmission(usize),
    /// The parent's own link.
    Inherited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    Continuous,
    Random,
    Instant,
    Distance,
    Attractor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Point,
    Line,
    Circle,
    Sphere,
    Offset,
    Cylinder,
    BodySurface,
    BodyBones,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Straight,
    Random,
    Cone,
    Ring,
    Offset,
    Inaccurate,
    Attractor,
}

/// Orientation choices for an emitted particle's forward / up axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orient {
    /// ±X, ±Y, ±Z of the parent (`world` = false) or the world.
    Axis {
        world: bool,
        axis: usize,
        negative: bool,
    },
    Velocity,
    ReverseVelocity,
    TowardsEmitter,
    AwayFromEmitter,
}

pub struct Emitter {
    pub class: Option<ClassId>,
    pub flags: u32,
    pub limit: u16,
    pub chance: u16,
    pub burst: f32,
    pub link: Link,
    pub rate: (Rate, [Value; 2]),
    pub position: (Position, [Value; 3]),
    pub direction: (Direction, [Value; 3]),
    /// Uniform (false) or stratified (true), and its values.
    pub speed: (bool, [Value; 3]),
    pub orient_dir: Orient,
    pub orient_up: Orient,
}

/// Class flag bits used by the runtime.
pub mod cflag {
    pub const DECORATIVE: u32 = 1 << 0;
    pub const GEOMETRY: u32 = 1 << 1;
    pub const COLLIDE_ENV: u32 = 1 << 2;
    pub const COLLIDE_CHAR: u32 = 1 << 3;
    pub const VELOCITY: u32 = 1 << 4;
    pub const ORIENTATION: u32 = 1 << 5;
    pub const OFFSET_POS: u32 = 1 << 6;
    pub const DYNAMIC_MATRIX: u32 = 1 << 7;
    pub const PREV_POSITION: u32 = 1 << 8;
    pub const OWNER: u32 = 1 << 9;
    pub const ATTRACTOR: u32 = 1 << 10;
    /// Its texture's animation starts at a random point / runs from its
    /// birth (`docs/PARTICLES.md`, *Animated textures*).
    pub const TEXTURE_INDEX: u32 = 1 << 11;
    pub const TEXTURE_TIME: u32 = 1 << 12;
    // flags2
    pub const ATTRACTOR_CHECK_ENV: u32 = 1 << 16;
    pub const EXPIRE_ON_CUTSCENE: u32 = 1 << 17;
    pub const DIE_ON_CUTSCENE: u32 = 1 << 18;
    pub const INITIALLY_HIDDEN: u32 = 1 << 19;
    pub const INVISIBLE: u32 = 1 << 20;
    pub const LOCK_TO_LINKED: u32 = 1 << 21;
    /// Painted onto a surface (a decal).
    pub const DECAL: u32 = 1 << 23;
    /// Its colour comes from the global tint while one is set.
    pub const USE_GLOBAL_TINT: u32 = 1 << 24;
    /// A decal unlit by the level / not laid on faces turned away.
    pub const DECAL_FULL_BRIGHT: u32 = 1 << 25;
    pub const DECAL_CULL_BACKFACING: u32 = 1 << 26;
}
const CLASS_FLAGS: [(&str, u32); 23] = [
    ("decorative", cflag::DECORATIVE),
    ("geometry", cflag::GEOMETRY),
    ("collideEnv", cflag::COLLIDE_ENV),
    ("collideChar", cflag::COLLIDE_CHAR),
    ("hasVelocity", cflag::VELOCITY),
    ("hasOrientation", cflag::ORIENTATION),
    ("hasOffsetPos", cflag::OFFSET_POS),
    ("hasDynamicMatrix", cflag::DYNAMIC_MATRIX),
    ("hasPrevPosition", cflag::PREV_POSITION),
    ("hasOwner", cflag::OWNER),
    ("hasAttractor", cflag::ATTRACTOR),
    ("hasTextureIndex", cflag::TEXTURE_INDEX),
    ("hasTextureTime", cflag::TEXTURE_TIME),
    ("attractorCheckEnvironment", cflag::ATTRACTOR_CHECK_ENV),
    ("expireOnCutscene", cflag::EXPIRE_ON_CUTSCENE),
    ("dieOnCutscene", cflag::DIE_ON_CUTSCENE),
    ("initiallyHidden", cflag::INITIALLY_HIDDEN),
    ("invisible", cflag::INVISIBLE),
    ("lockToLinkedParticle", cflag::LOCK_TO_LINKED),
    ("decal", cflag::DECAL),
    ("useGlobalTint", cflag::USE_GLOBAL_TINT),
    ("decalFullBright", cflag::DECAL_FULL_BRIGHT),
    ("decalCullBackfacing", cflag::DECAL_CULL_BACKFACING),
];

/// How a lens flare is seen (`P3tAppearance`): how far behind something
/// its point still counts as seen, and — with lens frames — the frames it
/// fades in and out over (else it just shows or not).
#[derive(Clone, Debug)]
pub struct Lens {
    pub dist: Value,
    pub fade: Option<(u16, u16)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Iterator {
    None,
    Link,
    ParticleClass,
    ParticleTag,
    Character,
    HostileCharacter,
    EmittedTowards,
    Parent,
    AllCharacters,
}

pub struct AttractorDef {
    pub iterator: Iterator,
    /// Conical (true) or distance (false).
    pub conical: bool,
    pub max_distance: Value,
    pub max_angle: Value,
    pub theta_min: Value,
    pub theta_max: Value,
    pub theta_weight: Value,
}

// (The name, the AI's dodge and alert radii and the look are for drawing
// and the AI's dodging, still to come.)
#[allow(dead_code)]
pub struct Class {
    pub name: String,
    pub flags: u32,
    pub action_mask: u32,
    pub lifetime: Value,
    /// A sphere radius against characters (0 = a point).
    pub collision_radius: f32,
    pub dodge_radius: f32,
    pub alert_radius: f32,
    /// The impulse a projectile makes passing the listener.
    pub flyby: Option<Rc<str>>,
    pub scale: Value,
    /// A separate height (when the class has one), how its sprite is drawn
    /// (`M3cGeomState_SpriteMode_*`: 0 normal, 1 rotated, 2 billboard, 3
    /// arrow, 4 flat, 5 / 6 contrails, 7 discus; `World::sprites`) and its
    /// turn in its own plane (degrees).
    pub y_scale: Option<Value>,
    pub sprite_type: u32,
    pub rotation: Value,
    pub alpha: Value,
    /// Its colour (a shade: ARGB).
    pub tint: Value,
    /// Texture (sprite) or model (geometry) name.
    pub instance: String,
    /// Decals: how many may live at once, frames to fade out, how far they
    /// wrap round corners (degrees).
    pub max_decals: u16,
    pub decal_fade_frames: u16,
    pub decal_wrap: Value,
    /// A lens flare (`P3cParticleClassFlag2_Appearance_LensFlare`): drawn
    /// over the scene where its point is seen.
    pub lens: Option<Lens>,
    /// A contrail's edge fade (`P3cParticleClassFlag2_Appearance_EdgeFade`):
    /// edge-on below the first it is gone, up to the second it fades in.
    pub edge_fade: Option<(f32, f32)>,
    /// The longest contrail link drawn (`max_contrail`; 0: any).
    pub max_contrail: f32,
    /// Drawn as the contrail to its own latest emission
    /// (`P3cParticleClassFlag2_ContrailEmitter`).
    pub contrail_emitter: bool,
    pub attractor: Option<AttractorDef>,
    pub vars: Vec<VarDef>,
    pub actions: Vec<Action>,
    pub events: [Range<usize>; 16],
    pub emitters: Vec<Emitter>,
}

impl Class {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

#[derive(Default)]
pub struct Classes {
    pub list: Vec<Class>,
    pub by_name: HashMap<String, ClassId>,
}

impl Classes {
    pub fn get(&self, name: &str) -> Option<ClassId> {
        self.by_name.get(name).copied()
    }

    /// Loads `particle_classes.json`; empty when missing.
    pub fn load(path: &Path) -> Classes {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Classes::default();
        };
        let Ok(doc) = serde_json::from_str::<Json>(&text) else {
            return Classes::default();
        };
        Classes::from_json(&doc)
    }

    pub fn from_json(doc: &Json) -> Classes {
        let Some(map) = doc["classes"].as_object() else {
            return Classes::default();
        };
        let by_name: HashMap<String, ClassId> = map
            .keys()
            .enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect();
        let list = map
            .iter()
            .map(|(name, c)| class(name, c, &by_name))
            .collect();
        Classes { list, by_name }
    }
}

fn f32_of(j: &Json) -> f32 {
    j.as_f64().unwrap_or(0.0) as f32
}

fn shade_of(j: &Json) -> u32 {
    j.as_str()
        .and_then(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
        .unwrap_or(0)
}

/// A JSON value (see `docs/ASSET_FORMAT.md`), with `{"var"}` resolved
/// against `vars`.
fn value(j: &Json, vars: &[VarDef]) -> Value {
    let pair = |k: &str| (f32_of(&j[k][0]), f32_of(&j[k][1]));
    match j {
        Json::Null => Value::Null,
        Json::Number(n) => Value::Float(n.as_f64().unwrap_or(0.0) as f32),
        Json::String(s) => Value::Str(s.as_str().into()),
        Json::Object(o) => {
            if let Some(n) = o.get("var").and_then(Json::as_str) {
                vars.iter()
                    .position(|v| v.name == n)
                    .map_or(Value::Null, Value::Var)
            } else if o.contains_key("range") {
                let (a, b) = pair("range");
                Value::Range(a, b)
            } else if o.contains_key("bell") {
                let (a, b) = pair("bell");
                Value::Bell(a, b)
            } else if let Some(s) = o.get("shade") {
                Value::Shade(shade_of(s))
            } else if let Some(s) = o.get("shadeRange") {
                Value::ShadeRange(shade_of(&s[0]), shade_of(&s[1]))
            } else if let Some(s) = o.get("shadeBell") {
                Value::ShadeBell(shade_of(&s[0]), shade_of(&s[1]))
            } else if let Some(e) = o.get("enum") {
                Value::Enum(e.as_u64().unwrap_or(0) as u32)
            } else if o.contains_key("cycle") {
                let (a, b) = pair("cycle");
                Value::Cycle(a, b)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

fn values<const N: usize>(j: &Json, vars: &[VarDef]) -> [Value; N] {
    std::array::from_fn(|k| value(&j[k], vars))
}

fn orient(s: &str) -> Orient {
    match s {
        "velocity" => Orient::Velocity,
        "reverseVelocity" => Orient::ReverseVelocity,
        "towardsEmitter" => Orient::TowardsEmitter,
        "awayFromEmitter" => Orient::AwayFromEmitter,
        _ => {
            let world = s.starts_with("world");
            let rest = s.trim_start_matches("world").trim_start_matches("parent");
            let negative = rest.starts_with("Neg");
            let axis = match rest.chars().last() {
                Some('X') => 0,
                Some('Z') => 2,
                _ => 1,
            };
            Orient::Axis {
                world,
                axis,
                negative,
            }
        }
    }
}

fn class(name: &str, c: &Json, by_name: &HashMap<String, ClassId>) -> Class {
    let names = |k: &str| -> Vec<String> {
        c[k].as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut flags = 0;
    for n in names("flags").iter().chain(names("flags2").iter()) {
        if let Some((_, b)) = CLASS_FLAGS.iter().find(|(k, _)| k == n) {
            flags |= b;
        }
    }
    // Decals are always decorative (`P3.c`'s class load).
    if flags & cflag::DECAL != 0 {
        flags |= cflag::DECORATIVE;
    }
    // Variables first: values refer to them by name.
    let mut vars: Vec<VarDef> = c["variables"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| VarDef {
                    name: v["name"].as_str().unwrap_or("").to_string(),
                    kind: match v["type"].as_str() {
                        Some("float") => VarKind::Float,
                        Some("string") => VarKind::Str,
                        Some("shade") => VarKind::Shade,
                        Some("enum") => VarKind::Enum,
                        _ => VarKind::Sound,
                    },
                    init: Value::Null,
                })
                .collect()
        })
        .unwrap_or_default();
    let inits: Vec<Value> = c["variables"]
        .as_array()
        .map(|a| a.iter().map(|v| value(&v["init"], &vars)).collect())
        .unwrap_or_default();
    for (v, i) in vars.iter_mut().zip(inits) {
        v.init = i;
    }
    let actions = c["actions"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| {
                    let mut slot = [None; 8];
                    if let Some(vs) = x["vars"].as_array() {
                        for (k, v) in vs.iter().take(8).enumerate() {
                            slot[k] = v
                                .as_str()
                                .and_then(|n| vars.iter().position(|d| d.name == n));
                        }
                    }
                    Action {
                        kind: x["type"].as_str().map_or(Kind::Other, Kind::from_name),
                        vars: slot,
                        params: x["params"]
                            .as_array()
                            .map(|p| p.iter().map(|v| value(v, &vars)).collect())
                            .unwrap_or_default(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let events = std::array::from_fn(|e| {
        let r = &c["events"][EVENT_NAMES[e]];
        (r[0].as_u64().unwrap_or(0) as usize)..(r[1].as_u64().unwrap_or(0) as usize)
    });
    let emitters = c["emitters"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    let fl = e["flags"].as_array().map_or(0, |f| {
                        f.iter()
                            .filter_map(|n| {
                                EMITTER_FLAGS.iter().position(|k| Some(*k) == n.as_str())
                            })
                            .fold(0, |acc, b| acc | 1 << b)
                    });
                    let ty = |k: &str| e[k]["type"].as_str().unwrap_or("").to_string();
                    Emitter {
                        class: e["class"].as_str().and_then(|n| by_name.get(n).copied()),
                        flags: fl,
                        limit: e["limit"].as_u64().unwrap_or(0) as u16,
                        chance: e["chance"].as_u64().unwrap_or(65535) as u16,
                        burst: f32_of(&e["burst"]),
                        link: match e["link"].as_str().unwrap_or("none") {
                            "emitter" => Link::Emitter,
                            "ourLink" => Link::Inherited,
                            s if s.starts_with("lastEmission") => Link::LastEmission(
                                s.trim_start_matches("lastEmission").parse().unwrap_or(0),
                            ),
                            _ => Link::None,
                        },
                        rate: (
                            match ty("rate").as_str() {
                                "random" => Rate::Random,
                                "instant" => Rate::Instant,
                                "distance" => Rate::Distance,
                                "attractor" => Rate::Attractor,
                                _ => Rate::Continuous,
                            },
                            values(&e["rate"]["values"], &vars),
                        ),
                        position: (
                            match ty("position").as_str() {
                                "line" => Position::Line,
                                "circle" => Position::Circle,
                                "sphere" => Position::Sphere,
                                "offset" => Position::Offset,
                                "cylinder" => Position::Cylinder,
                                "bodySurface" => Position::BodySurface,
                                "bodyBones" => Position::BodyBones,
                                _ => Position::Point,
                            },
                            values(&e["position"]["values"], &vars),
                        ),
                        direction: (
                            match ty("direction").as_str() {
                                "random" => Direction::Random,
                                "cone" => Direction::Cone,
                                "ring" => Direction::Ring,
                                "offset" => Direction::Offset,
                                "inaccurate" => Direction::Inaccurate,
                                "attractor" => Direction::Attractor,
                                _ => Direction::Straight,
                            },
                            values(&e["direction"]["values"], &vars),
                        ),
                        speed: (
                            ty("speed") == "stratified",
                            values(&e["speed"]["values"], &vars),
                        ),
                        orient_dir: orient(e["orientDir"].as_str().unwrap_or("parentPosY")),
                        orient_up: orient(e["orientUp"].as_str().unwrap_or("parentPosZ")),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let attractor = c.get("attractor").map(|a| AttractorDef {
        iterator: match a["iterator"].as_str().unwrap_or("none") {
            "link" => Iterator::Link,
            "particleClass" => Iterator::ParticleClass,
            "particleTag" => Iterator::ParticleTag,
            "character" => Iterator::Character,
            "hostileCharacter" => Iterator::HostileCharacter,
            "emittedTowards" => Iterator::EmittedTowards,
            "parent" => Iterator::Parent,
            "allCharacters" => Iterator::AllCharacters,
            _ => Iterator::None,
        },
        conical: a["selector"].as_str() == Some("conical"),
        max_distance: value(&a["maxDistance"], &vars),
        max_angle: value(&a["maxAngle"], &vars),
        theta_min: value(&a["thetaMin"], &vars),
        theta_max: value(&a["thetaMax"], &vars),
        theta_weight: value(&a["thetaWeight"], &vars),
    });
    let collision_radius = match value(&c["collisionRadius"], &vars) {
        Value::Float(r) if r > 0.0 => r,
        _ => 0.0,
    };
    Class {
        name: name.to_string(),
        flags,
        action_mask: c["actionMask"].as_u64().unwrap_or(0) as u32,
        lifetime: value(&c["lifetime"], &vars),
        collision_radius,
        dodge_radius: f32_of(&c["dodgeRadius"]),
        alert_radius: f32_of(&c["alertRadius"]),
        flyby: c["flybySound"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(Rc::from),
        scale: value(&c["appearance"]["scale"], &vars),
        y_scale: names("flags")
            .iter()
            .any(|n| n == "separateYScale")
            .then(|| value(&c["appearance"]["yScale"], &vars)),
        sprite_type: c["spriteType"].as_u64().unwrap_or(0) as u32,
        rotation: value(&c["appearance"]["rotation"], &vars),
        alpha: value(&c["appearance"]["alpha"], &vars),
        tint: value(&c["appearance"]["tint"], &vars),
        instance: c["appearance"]["instance"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        max_decals: c["appearance"]["maxDecals"].as_u64().unwrap_or(100) as u16,
        decal_fade_frames: c["appearance"]["decalFadeFrames"].as_u64().unwrap_or(60) as u16,
        decal_wrap: value(&c["appearance"]["decalWrap"], &vars),
        lens: names("flags2")
            .iter()
            .any(|n| n == "lensFlare")
            .then(|| Lens {
                dist: value(&c["appearance"]["lensflareDist"], &vars),
                fade: names("flags")
                    .iter()
                    .any(|n| n == "hasLensFrames")
                    .then(|| {
                        let f = |k: &str| c["appearance"][k].as_u64().unwrap_or(0) as u16;
                        (f("lensInFrames").max(1), f("lensOutFrames").max(1))
                    }),
            }),
        edge_fade: names("flags2").iter().any(|n| n == "edgeFade").then(|| {
            let f = |k: &str| c["appearance"][k].as_f64().unwrap_or(0.0) as f32;
            (f("edgeFadeMin"), f("edgeFadeMax"))
        }),
        max_contrail: c["appearance"]["maxContrail"].as_f64().unwrap_or(0.0) as f32,
        contrail_emitter: names("flags2").iter().any(|n| n == "contrailEmitter"),
        attractor,
        vars,
        actions,
        events,
        emitters,
    }
}
