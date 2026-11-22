//! Particle classes, loaded from `common/particle_classes.json`
//! (`oni_schema::files::ParticleClassesFile`) into typed definitions with
//! variable references, emitter classes and effect classes resolved.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use oni_schema::files;

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
    /// birth.
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
/// How a class's sprites are drawn (`M3cGeomState_SpriteMode_*`, the
/// data's `spriteType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpriteType {
    /// Facing the camera (0; and any number not below).
    #[default]
    Normal,
    /// Facing the camera, turned toward its direction (1).
    Rotated,
    /// Along its direction, turned to face the camera (2).
    Billboard,
    /// Along its direction, upright along its up (3).
    Arrow,
    /// Across its direction (4).
    Flat,
    /// A contrail as wide as its orientation's X (5).
    ContrailOriented,
    /// A contrail facing the camera (6).
    Contrail,
    /// Along its direction and its right (7).
    Discus,
}

impl SpriteType {
    pub fn from_data(n: u64) -> SpriteType {
        match n {
            1 => SpriteType::Rotated,
            2 => SpriteType::Billboard,
            3 => SpriteType::Arrow,
            4 => SpriteType::Flat,
            5 => SpriteType::ContrailOriented,
            6 => SpriteType::Contrail,
            7 => SpriteType::Discus,
            _ => SpriteType::Normal,
        }
    }

    /// Drawn as a contrail (from the particle to its link), not a sprite.
    pub fn is_contrail(self) -> bool {
        matches!(self, SpriteType::ContrailOriented | SpriteType::Contrail)
    }
}

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
    /// (`World::sprites`) and its turn in its own plane (degrees).
    pub y_scale: Option<Value>,
    pub sprite_type: SpriteType,
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
        Classes::from_file(&crate::json::read_file(path))
    }

    pub fn from_file(doc: &files::ParticleClassesFile) -> Classes {
        let by_name: HashMap<String, ClassId> = doc
            .classes
            .keys()
            .enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect();
        let list = doc
            .classes
            .iter()
            .map(|(name, c)| class(name, c, &by_name))
            .collect();
        Classes { list, by_name }
    }
}

fn shade_of(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0)
}

/// A value (`oni_schema::files::ParticleValue`), with a variable resolved against
/// `vars`.
fn value(j: Option<&files::ParticleValue>, vars: &[VarDef]) -> Value {
    use files::ParticleValue as P;
    match j {
        None | Some(P::Unknown { .. }) => Value::Null,
        Some(P::Number(n)) => Value::Float(*n),
        Some(P::Text(s)) => Value::Str(s.as_str().into()),
        Some(P::Var { var }) => vars
            .iter()
            .position(|v| &v.name == var)
            .map_or(Value::Null, Value::Var),
        Some(P::Range { range: [a, b] }) => Value::Range(*a, *b),
        Some(P::Bell { bell: [a, b] }) => Value::Bell(*a, *b),
        Some(P::Shade { shade }) => Value::Shade(shade_of(shade)),
        Some(P::ShadeRange {
            shade_range: [a, b],
        }) => Value::ShadeRange(shade_of(a), shade_of(b)),
        Some(P::ShadeBell { shade_bell: [a, b] }) => Value::ShadeBell(shade_of(a), shade_of(b)),
        Some(P::Enum { value }) => Value::Enum(*value),
        Some(P::Cycle { cycle: [a, b] }) => Value::Cycle(*a, *b),
    }
}

fn values<const N: usize>(j: &[Option<files::ParticleValue>], vars: &[VarDef]) -> [Value; N] {
    std::array::from_fn(|k| value(j.get(k).and_then(Option::as_ref), vars))
}

/// A number value's number (else 0).
fn number(j: Option<&files::ParticleValue>) -> f32 {
    match j {
        Some(files::ParticleValue::Number(n)) => *n,
        _ => 0.0,
    }
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

fn class(name: &str, c: &files::ParticleClass, by_name: &HashMap<String, ClassId>) -> Class {
    let has = |list: &[String], n: &str| list.iter().any(|x| x == n);
    let mut flags = 0;
    for n in c.flags.iter().chain(&c.flags2) {
        if let Some((_, b)) = CLASS_FLAGS.iter().find(|(k, _)| k == n) {
            flags |= b;
        }
    }
    // Decals are always decorative (`P3.c`'s class load).
    if flags & cflag::DECAL != 0 {
        flags |= cflag::DECORATIVE;
    }
    // Variables first: values refer to them by name.
    let mut vars: Vec<VarDef> = c
        .variables
        .iter()
        .map(|v| VarDef {
            name: v.name.clone(),
            kind: match v.kind.as_str() {
                "float" => VarKind::Float,
                "string" => VarKind::Str,
                "shade" => VarKind::Shade,
                "enum" => VarKind::Enum,
                _ => VarKind::Sound,
            },
            init: Value::Null,
        })
        .collect();
    let inits: Vec<Value> = c
        .variables
        .iter()
        .map(|v| value(v.init.as_ref(), &vars))
        .collect();
    for (v, i) in vars.iter_mut().zip(inits) {
        v.init = i;
    }
    let actions = c
        .actions
        .iter()
        .map(|x| {
            let mut slot = [None; 8];
            for (k, v) in x.vars.iter().take(8).enumerate() {
                slot[k] = v
                    .as_ref()
                    .and_then(|n| vars.iter().position(|d| &d.name == n));
            }
            Action {
                kind: x.kind.name().map_or(Kind::Other, Kind::from_name),
                vars: slot,
                params: x.params.iter().map(|v| value(v.as_ref(), &vars)).collect(),
            }
        })
        .collect();
    let events = std::array::from_fn(|e| {
        let r = c.events.get(EVENT_NAMES[e]).copied().unwrap_or([0, 0]);
        usize::from(r[0])..usize::from(r[1])
    });
    let emitters = c
        .emitters
        .iter()
        .map(|e| {
            let fl = e
                .flags
                .iter()
                .filter_map(|n| EMITTER_FLAGS.iter().position(|k| k == n))
                .fold(0, |acc, b| acc | 1 << b);
            let ty = |s: &files::EmitterSpec| s.kind.name().unwrap_or("").to_string();
            Emitter {
                class: by_name.get(&e.class).copied(),
                flags: fl,
                limit: e.limit,
                chance: e.chance,
                burst: e.burst,
                link: match e.link.name().unwrap_or("none") {
                    "emitter" => Link::Emitter,
                    "ourLink" => Link::Inherited,
                    s if s.starts_with("lastEmission") => Link::LastEmission(
                        s.trim_start_matches("lastEmission").parse().unwrap_or(0),
                    ),
                    _ => Link::None,
                },
                rate: (
                    match ty(&e.rate).as_str() {
                        "random" => Rate::Random,
                        "instant" => Rate::Instant,
                        "distance" => Rate::Distance,
                        "attractor" => Rate::Attractor,
                        _ => Rate::Continuous,
                    },
                    values(&e.rate.values, &vars),
                ),
                position: (
                    match ty(&e.position).as_str() {
                        "line" => Position::Line,
                        "circle" => Position::Circle,
                        "sphere" => Position::Sphere,
                        "offset" => Position::Offset,
                        "cylinder" => Position::Cylinder,
                        "bodySurface" => Position::BodySurface,
                        "bodyBones" => Position::BodyBones,
                        _ => Position::Point,
                    },
                    values(&e.position.values, &vars),
                ),
                direction: (
                    match ty(&e.direction).as_str() {
                        "random" => Direction::Random,
                        "cone" => Direction::Cone,
                        "ring" => Direction::Ring,
                        "offset" => Direction::Offset,
                        "inaccurate" => Direction::Inaccurate,
                        "attractor" => Direction::Attractor,
                        _ => Direction::Straight,
                    },
                    values(&e.direction.values, &vars),
                ),
                speed: (ty(&e.speed) == "stratified", values(&e.speed.values, &vars)),
                orient_dir: orient(e.orient_dir.name().unwrap_or("parentPosY")),
                orient_up: orient(e.orient_up.name().unwrap_or("parentPosZ")),
            }
        })
        .collect();
    let attractor = c.attractor.as_ref().map(|a| AttractorDef {
        iterator: match a.iterator.name().unwrap_or("none") {
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
        conical: a.selector.name() == Some("conical"),
        max_distance: value(a.max_distance.as_ref(), &vars),
        max_angle: value(a.max_angle.as_ref(), &vars),
        theta_min: value(a.theta_min.as_ref(), &vars),
        theta_max: value(a.theta_max.as_ref(), &vars),
        theta_weight: value(a.theta_weight.as_ref(), &vars),
    });
    let collision_radius = match value(c.collision_radius.as_ref(), &vars) {
        Value::Float(r) if r > 0.0 => r,
        _ => 0.0,
    };
    let a = &c.appearance;
    Class {
        name: name.to_string(),
        flags,
        action_mask: c.action_mask.unwrap_or(0),
        lifetime: value(c.lifetime.as_ref(), &vars),
        collision_radius,
        dodge_radius: c.dodge_radius.unwrap_or(0.0),
        alert_radius: c.alert_radius.unwrap_or(0.0),
        flyby: c
            .flyby_sound
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(Rc::from),
        scale: value(a.scale.as_ref(), &vars),
        y_scale: has(&c.flags, "separateYScale").then(|| value(a.y_scale.as_ref(), &vars)),
        sprite_type: SpriteType::from_data(u64::from(c.sprite_type.unwrap_or(0))),
        rotation: value(a.rotation.as_ref(), &vars),
        alpha: value(a.alpha.as_ref(), &vars),
        tint: value(a.tint.as_ref(), &vars),
        instance: a.instance.clone().unwrap_or_default(),
        max_decals: a.max_decals.unwrap_or(100),
        decal_fade_frames: a.decal_fade_frames.unwrap_or(60),
        decal_wrap: value(a.decal_wrap.as_ref(), &vars),
        lens: has(&c.flags2, "lensFlare").then(|| Lens {
            dist: value(a.lensflare_dist.as_ref(), &vars),
            fade: has(&c.flags, "hasLensFrames").then(|| {
                (
                    a.lens_in_frames.unwrap_or(0).max(1),
                    a.lens_out_frames.unwrap_or(0).max(1),
                )
            }),
        }),
        edge_fade: has(&c.flags2, "edgeFade").then(|| {
            (
                number(a.edge_fade_min.as_ref()),
                number(a.edge_fade_max.as_ref()),
            )
        }),
        max_contrail: number(a.max_contrail.as_ref()),
        contrail_emitter: has(&c.flags2, "contrailEmitter"),
        attractor,
        vars,
        actions,
        events,
        emitters,
    }
}
