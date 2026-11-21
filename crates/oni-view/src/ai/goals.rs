//! Goals and jobs (behaviour per `docs/GOALS.md`): what an AI character
//! does over time. It has a **job** (stand idle, or patrol along a waypoint
//! program) and a **current goal** — the job, or a transient goal that
//! perception starts (**pursuit** of a threat, **combat** with an enemy),
//! which returns to the job when done. The goals produce movement (walking
//! a planned path, via the [`Navigator`]), where to look, and — in combat —
//! the class's melee AI.

use std::f32::consts::{PI, TAU};
use std::rc::Rc;

/// Seeing and shooting through the level (`AKcGQ_Flag_LOS_*_AI`).
const SEE: u32 = crate::quads::flag::LOS_SEE_AI;
const SHOOT: u32 = crate::quads::flag::LOS_SHOOT_AI;

use glam::Vec3;

use super::level::{
    CombatProfile, LevelAi, PatrolPath, Pursue as PursueSettings, Target, Waypoint,
};
use super::melee::brain::Melee;
use super::melee::fight::Fights;
use super::melee::profile::Profile;
use super::navigate::{Navigator, State as NavState};
use super::perception::{Alert, Notice, Perception, Priority, Stimulus, Strength};
use super::{relative, yaw_toward, Direction, Orders, Pace, MAX_TURN};
use crate::anim::input::key;
use crate::quirks::Quirks;

// --- constants (`docs/GOALS.md`, *Constants*) -----------------------------------

/// A resumed patrol is back on its path within this of where it left it.
const RETURN_TO_PATH: f32 = 40.0;
/// Give up on a patrol waypoint after this many failed paths.
const PATROL_FAILS: u32 = 4;
/// Patrol guard look-around: a new direction every 240–480 ticks, offsets of
/// ±45° every 90–150.
const GUARD_DIR: (u32, u32) = (240, 240);
const GUARD_ANGLE: (u32, u32) = (90, 60);
const GUARD_ANGLE_RANGE: f32 = PI / 4.0;
/// Patrol scan offsets every 60–100 ticks.
const SCAN_ANGLE: (u32, u32) = (60, 40);
/// A moving scan follows a change of course of more than this.
const SCAN_RECALC: f32 = 3.0 * PI / 180.0;
/// Look-around: a new direction differs from the last by at least this, and
/// needs this much clear view (tries).
const LOOK_MIN_DELTA: f32 = 40.0 * PI / 180.0;
const LOOK_CLEAR: f32 = 30.0;
const LOOK_TRIES: u32 = 6;

/// Pursuit: going to a contact.
const PURSUIT_SCAN_DIST: f32 = 200.0;
const PURSUIT_STOP_DIST: f32 = 60.0;
const PURSUIT_CLOSE_DIST: f32 = 12.0;
const PURSUIT_LOS_CHECKS: u32 = 6;
const PURSUIT_SCAN: (u32, u32) = (40, 40);
const PURSUIT_SCAN_RANGE: f32 = 70.0 * PI / 180.0;
const PURSUIT_LOOK_DIR: (u32, u32) = (100, 100);
const PURSUIT_LOOK_ANGLE: (u32, u32) = (40, 40);
const PURSUIT_LOOK_RANGE: f32 = PI / 4.0;
const PURSUIT_GLANCE_FIRST: u32 = 80;
const PURSUIT_LOOK_AFTER_ARRIVING: u32 = 600;

/// Combat: decisions every this many ticks.
const COMBAT_PERIOD: u64 = 5;
/// Melee is only an option within this of the target.
const MELEE_RANGE: f32 = 80.0;
/// No technique for this long → advance for a while instead.
const MELEE_NO_TECHNIQUE: u32 = 30;
const TRY_ADVANCE: u32 = 90;
/// Recently punched by the target (for "fight back if punched").
const RECENTLY_HURT: u64 = 240;
/// Target switching (`docs/PERCEPTION.md`, *Memory*).
const SWITCH_CLOSER: f32 = 0.8;
const DONT_SWITCH_FARTHER: f32 = 3.5;
const SWITCH_IF_NOT_SHOT: u64 = 150;
const SWITCH_IF_NOT_PUNCHED: u64 = 90;
/// Never advance closer than this.
/// Within this an advance has always closed in
/// (`AI2cManeuver_Advance_AbsoluteMinDist`).
const ADVANCE_MIN: f32 = 6.0;
/// How near a route's advance stops (`AI2cManeuver_Advance_StopDist`).
const AI2_ADVANCE_STOP: f32 = 4.0;

/// Standing still, the body turns to where it looks once that is more than
/// this off its facing (the head covers the rest).
const STANDING_LOOK_RANGE: f32 = 40.0 * PI / 180.0;
const STANDING_GLANCE_RANGE: f32 = 115.0 * PI / 180.0;
const PURSUIT_AIM_RANGE: f32 = 85.0 * PI / 180.0;
const PANIC_AIM_RANGE: f32 = 25.0 * PI / 180.0;

/// Panic: dangers further than this don't scare it (the neutral-interaction
/// abort range; the default without a neutral profile).
const PANIC_ENEMY_RANGE: f32 = 100.0;
/// It kneels when the danger is this close.
const PANIC_KNEEL: f32 = 20.0;
/// The danger gone, it calms down this long after last perceiving it.
const PANIC_AFTER_CAUSE_GONE: u64 = 180;

// --- class combat settings ----------------------------------------------------

/// A class's combat timings (`classes.json` `perception.combat`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassCombat {
    pub investigate_body: u32,
    pub lost_contact: u32,
    pub taunt_percent: u32,
    pub default_profile: u16,
    /// Chance (percent) it goes for a gun when it has none.
    pub gun_pickup_percent: u32,
    /// The chance it goes for a gun with a roll or slide that picks it up
    /// (`gun_runningpickup_percentage`).
    pub gun_running_pickup_percent: u32,
    /// Dodging: at all, while its own gun fires, and whether a dodge stops
    /// it firing; how long a danger must last before it reacts (ticks), and
    /// how its dodges' lifetimes and weights are scaled.
    pub dodge: bool,
    pub dodge_while_firing: bool,
    pub dodge_stops_firing: bool,
    pub dodge_react: u32,
    pub dodge_time_scale: f32,
    pub dodge_weight_scale: f32,
}

impl Default for ClassCombat {
    fn default() -> Self {
        ClassCombat {
            investigate_body: 240,
            lost_contact: 180,
            taunt_percent: 40,
            default_profile: 0,
            gun_pickup_percent: 0,
            gun_running_pickup_percent: 0,
            dodge: false,
            dodge_while_firing: false,
            dodge_stops_firing: false,
            dodge_react: 0,
            dodge_time_scale: 1.0,
            dodge_weight_scale: 1.0,
        }
    }
}

impl ClassCombat {
    pub fn from_json(p: &serde_json::Value) -> ClassCombat {
        let c = &p["combat"];
        let u = |k: &str, d: u32| c[k].as_u64().map_or(d, |v| v as u32);
        let has = |f: &str| {
            p["flags"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(f)))
        };
        let d = ClassCombat::default();
        ClassCombat {
            investigate_body: u("investigateBodyTicks", d.investigate_body),
            lost_contact: u("lostContactTicks", d.lost_contact),
            taunt_percent: u("deadTauntPercent", d.taunt_percent),
            default_profile: u("defaultCombatProfileId", 0) as u16,
            gun_pickup_percent: u("gunPickupPercent", 0),
            gun_running_pickup_percent: u("gunRunningPickupPercent", 0),
            dodge: has("dodge"),
            dodge_while_firing: has("dodgeWhileFiring"),
            dodge_stops_firing: has("dodgeStopsFiring"),
            dodge_react: p["dodge"]["reactTicks"].as_u64().unwrap_or(0) as u32,
            dodge_time_scale: p["dodge"]["timeScale"].as_f64().unwrap_or(1.0) as f32,
            dodge_weight_scale: p["dodge"]["weightScale"].as_f64().unwrap_or(1.0) as f32,
        }
    }
}

// --- pace -----------------------------------------------------------------------

/// A movement mode (a patrol's pace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MoveMode {
    #[default]
    ByAlert,
    Creep,
    WalkNoAim,
    Walk,
    RunNoAim,
    Run,
}

impl MoveMode {
    pub fn from_name(n: &str) -> MoveMode {
        match n {
            "creep" => MoveMode::Creep,
            "walkNoAim" => MoveMode::WalkNoAim,
            "walk" => MoveMode::Walk,
            "runNoAim" => MoveMode::RunNoAim,
            "run" => MoveMode::Run,
            _ => MoveMode::ByAlert,
        }
    }

    /// A movement mode as scripts name it (`ai2_setmovementmode`).
    pub fn from_script(n: &str) -> MoveMode {
        match n.to_ascii_lowercase().as_str() {
            "creep" => MoveMode::Creep,
            "walk_noaim" => MoveMode::WalkNoAim,
            "walk" => MoveMode::Walk,
            "run_noaim" => MoveMode::RunNoAim,
            "run" => MoveMode::Run,
            _ => MoveMode::ByAlert,
        }
    }

    /// The mode itself at `alert`, hurrying or not (`AI2rMovement_Default`:
    /// by alert, the no-aim walk until medium; then walking, or running
    /// when hurrying).
    pub fn resolved(self, alert: Alert, hurry: bool) -> MoveMode {
        use MoveMode::*;
        match self {
            ByAlert if hurry => [WalkNoAim, WalkNoAim, RunNoAim, Run, Run][alert as usize],
            ByAlert => [WalkNoAim, WalkNoAim, Walk, Walk, Walk][alert as usize],
            m => m,
        }
    }

    /// Whether it aims with its gun, unforced
    /// (`AI2rMovementState_ResetAimingVarient`): not in the no-aim modes.
    pub fn aims_weapon(self) -> bool {
        !matches!(self, MoveMode::WalkNoAim | MoveMode::RunNoAim)
    }

    /// The pace at `alert`, hurrying or not.
    pub fn pace(self, alert: Alert, hurry: bool) -> Pace {
        use MoveMode::*;
        match self.resolved(alert, hurry) {
            Creep => Pace::Creep,
            WalkNoAim | Walk => Pace::Walk,
            _ => Pace::Run,
        }
    }
}

/// Which way to dodge a danger (`AI2iManeuver_MakeDodgeFromDanger`): straight
/// away from it, or — within 10, a direct hit — square to its incoming path,
/// to the side with room (either at random when both or neither have).
/// `danger` runs from the body toward it. Also says whether it was direct.
fn dodge_heading(
    danger: Vec3,
    incoming: Vec3,
    pelvis: Vec3,
    rng: &mut Rng,
    cx: &Ctx,
) -> (Vec3, bool) {
    let danger = danger.with_y_zero();
    if danger.length() >= 10.0 {
        return (-danger.normalize_or(Vec3::X), false);
    }
    let Some(side) = Vec3::new(incoming.z, 0.0, -incoming.x).try_normalize() else {
        return (Vec3::X, true);
    };
    let clear = |d: Vec3| (cx.sight)(pelvis, pelvis + d * 20.0, crate::quads::flag::CHR_COL_SKIP);
    let heading = match (clear(side), clear(-side)) {
        (true, false) => side,
        (false, true) => -side,
        _ if rng.next() & 0x8000 == 0 => -side,
        _ => side,
    };
    (heading, true)
}

// --- randomness -------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(super) struct Rng(u32);

impl Rng {
    pub(super) fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// `[0, 1)`.
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }
    /// `min ..= min + delta`.
    fn range(&mut self, (min, delta): (u32, u32)) -> u32 {
        min + if delta > 0 {
            self.next() % (delta + 1)
        } else {
            0
        }
    }
}

// --- looking around (`Oni_AI2_Guard.c`) ----------------------------------------

/// Looking around: a main direction re-picked on one timer, offset by a
/// random angle re-picked on another.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    initial: Option<f32>,
    has_dir: bool,
    dir: f32,
    stay_facing: bool,
    stay_dir: Option<f32>,
    stay_range: f32,
    dir_timer: u32,
    dir_times: (u32, u32),
    angle_timer: u32,
    angle_times: (u32, u32),
    angle_range: f32,
    offset: f32,
    /// Where it looks (yaw).
    pub look: f32,
}

impl Scan {
    fn reset(initial: Option<f32>) -> Scan {
        Scan {
            initial,
            ..Scan::default()
        }
    }

    fn new_direction(
        &mut self,
        facing: f32,
        around: Option<(f32, f32)>,
        rng: &mut Rng,
        clear: &dyn Fn(f32) -> bool,
    ) {
        let avoid = self.has_dir.then_some(self.dir);
        let mut first = !self.has_dir;
        let mut tries = 0;
        loop {
            let t = if first {
                first = false;
                self.initial.unwrap_or(facing)
            } else {
                match around {
                    None => rng.unit() * TAU,
                    Some((f, range)) => f + (rng.unit() * 2.0 - 1.0) * range,
                }
            };
            if tries < LOOK_TRIES && avoid.is_some_and(|a| relative(t - a).abs() < LOOK_MIN_DELTA) {
                tries += 1;
                continue;
            }
            if tries >= LOOK_TRIES || clear(t) {
                self.has_dir = true;
                self.dir = t.rem_euclid(TAU);
                return;
            }
            tries += 1;
        }
    }

    /// One tick: re-picks the direction / offset when their timers run out.
    fn update(&mut self, facing: f32, rng: &mut Rng, clear: &dyn Fn(f32) -> bool) -> f32 {
        let mut changed = false;
        if !self.has_dir || self.dir_timer == 0 {
            let around = self
                .stay_facing
                .then(|| (self.stay_dir.unwrap_or(facing), self.stay_range));
            self.new_direction(facing, around, rng, clear);
            changed = true;
            self.dir_timer = rng.range(self.dir_times);
            self.angle_timer = rng.range(self.angle_times);
        } else {
            self.dir_timer -= 1;
        }
        if self.angle_range > 0.0 {
            if self.angle_timer == 0 {
                self.offset = self.angle_range * (2.0 * rng.unit() - 1.0);
                self.angle_timer = rng.range(self.angle_times);
                changed = true;
            } else {
                self.angle_timer -= 1;
            }
        }
        if changed {
            self.look = if self.angle_range == 0.0 {
                self.dir
            } else {
                self.dir + self.offset
            };
        }
        self.look
    }
}

trait Flat {
    fn with_y_zero(self) -> Vec3;
}
impl Flat for Vec3 {
    fn with_y_zero(self) -> Vec3 {
        Vec3::new(self.x, 0.0, self.z)
    }
}

// --- what a goal sees of the world --------------------------------------------

/// Another character, as goals see it.
#[derive(Clone, Copy, Debug)]
pub struct Other {
    pub feet: Vec3,
    pub alive: bool,
    /// Where shots aim (its pelvis), how tall it is, and whether it is no
    /// enemy of this character (shots mustn't pass through it).
    pub pelvis: Vec3,
    pub height: f32,
    pub friend: bool,
    /// Reeling from a stun, or lying on the ground.
    pub dizzy: bool,
    pub fallen: bool,
    /// Wearing invisibility (lost at once, `AI2iCombat_TargetKnowledge…`).
    pub invisible: bool,
    /// Which way it faces (its cosmetic facing).
    pub facing: f32,
    /// No longer in the level (deleted, or handed over as a corpse).
    pub gone: bool,
}

/// An AI's own body, as its goals see it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub hp: i32,
    pub max_hp: i32,
    /// Placed unkillable.
    pub unkillable: bool,
    /// Standing or moving about, doing nothing else
    /// (`ONrCharacter_IsInactiveUpright`).
    pub inactive_upright: bool,
    /// In its powerup clip proper (`ONrCharacter_IsPoweringUp`).
    pub powering_up: bool,
    /// The powerup animation type (`ONcAnimType_Powerup`).
    pub powerup_type: u16,
    /// Its clip: the type, a count of clips begun (a new one shows as a
    /// change), whether this tick is its action frame.
    pub anim_type: u16,
    pub started: u32,
    pub action_frame: bool,
    /// Standing still (`ONrCharacter_IsStandingStill`), down, reeling.
    pub standing_still: bool,
    pub fallen: bool,
    pub victim: bool,
    /// Invisibility left.
    pub invisibility: u16,
    /// Its clip can pick things up as it goes (`ONcAnimFlag_Can_Pickup`,
    /// not `NoAIPickup`), is a pickup clip; and the escape moves that can
    /// (forward, back, left, right: the clip type and where it ends, from
    /// the feet).
    pub can_pickup: bool,
    pub picking_up: bool,
    pub escapes: [Option<(u16, Vec3)>; 4],
    /// The clip queued next, whether the current one drops a super shield
    /// (`ONcAnimFlag_DisableShield`), whether it has a thunderbolt to do.
    pub queued: u16,
    pub disables_shield: bool,
    pub has_thunderbolt: bool,
}

/// The gun in an AI's hand, as targeting sees it.
#[derive(Clone, Copy, Debug)]
pub struct GunView {
    /// Its world matrix now (barrel +X).
    pub matrix: glam::Mat4,
    pub ai: crate::weapons::AiGun,
    pub skill: crate::ai::ranged::Skill,
    pub params: crate::ai::ranged::Params,
    /// Loaded, or with something to reload it.
    pub usable: bool,
    /// Firing now (its trigger held).
    pub firing: bool,
    /// The gun stuns, or knocks down (its shots set the AI on to melee the
    /// reeling / fallen target).
    pub stuns: bool,
    pub knocks_down: bool,
}

/// What a goal can see and use this tick.
pub struct Ctx<'a> {
    pub me: usize,
    /// Its own body (what a boss watches).
    pub body: Body,
    pub now: u64,
    pub feet: Vec3,
    pub eye: Vec3,
    pub facing: f32,
    /// Lying on the ground.
    pub fallen: bool,
    pub standing: bool,
    pub pelvis: Vec3,
    /// The level's consoles, and whether this character is using one.
    pub consoles: &'a [crate::consoles::Console],
    pub using_console: bool,
    /// Crouched (or going to be).
    pub crouching: bool,
    pub perception: &'a mut Perception,
    pub level: &'a LevelAi,
    /// A clear line between two points.
    /// A clear line through the level, past what the mask's flags let
    /// through (`crate::quads::flag::LOS_*`).
    pub sight: &'a dyn Fn(Vec3, Vec3, u32) -> bool,
    /// The length of the way between two points (None: no way).
    pub path_length: &'a dyn Fn(Vec3, Vec3) -> Option<f32>,
    /// The local probe ([`crate::nav::Navigation::probe`]): from, direction,
    /// distance, stop square (None: no navigation data).
    pub probe:
        &'a dyn Fn(Vec3, glam::Vec2, f32, crate::nav::Square) -> Option<crate::nav::LocalProbe>,
    pub others: &'a [Other],
    pub fights: &'a mut Fights,
    pub melee_profile: Option<Rc<Profile>>,
    pub daze: (u32, u32),
    pub quirks: Quirks,
    /// The gun in its hand, and the guns lying about.
    pub gun: Option<GunView>,
    pub guns: &'a [FreeGun],
    /// Dangers to dodge: guns firing (their firing cones) and projectiles.
    pub spreads: &'a [Spread],
    pub projectiles: &'a [Projectile],
}

/// A gun firing, as a cone others dodge: from its muzzle along its shot's
/// line, this long, this wide at the start, widening at this angle.
#[derive(Clone, Copy, Debug)]
pub struct Spread {
    pub owner: usize,
    pub origin: Vec3,
    pub axis: Vec3,
    pub length: f32,
    pub base: f32,
    pub angle: f32,
}

/// A projectile others may dodge: where, its velocity (units/s), how wide a
/// berth it wants.
#[derive(Clone, Copy, Debug)]
pub struct Projectile {
    pub id: u64,
    pub at: Vec3,
    pub velocity: Vec3,
    pub radius: f32,
}

/// One danger it is dodging: which, how strongly, how long it lasts and has
/// lasted, and the way to go.
#[derive(Clone, Copy, Debug)]
struct Dodge {
    key: u64,
    weight: f32,
    lifetime: u32,
    left: u32,
    age: u32,
    heading: Vec3,
}

/// A gun lying free: which, where to pick it up, and whether this character
/// could use it (loaded, or it has clips for it; storable).
#[derive(Clone, Copy, Debug)]
pub struct FreeGun {
    pub index: usize,
    pub at: Vec3,
    pub usable: bool,
}

impl crate::ai::ranged::Random for Rng {
    fn unit(&mut self) -> f32 {
        Rng::unit(self)
    }
}

impl Ctx<'_> {
    fn clear_toward(&self) -> impl Fn(f32) -> bool + '_ {
        move |yaw: f32| {
            (self.sight)(
                self.eye,
                self.eye + Vec3::new(yaw.sin(), 0.0, yaw.cos()) * LOOK_CLEAR,
                SEE,
            )
        }
    }
    fn alert(&self) -> Alert {
        self.perception.alert
    }
}

// --- movement orders ------------------------------------------------------------

/// Where to go: a point, the distance that counts as there, whether to stop
/// there, and the facing to end with.
#[derive(Clone, Copy, Debug)]
struct Dest {
    at: Vec3,
    within: f32,
    stop: bool,
    face: Option<f32>,
}

/// Where to look.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    None,
    At(Vec3),
    Dir(f32),
}

// --- goals -------------------------------------------------------------------------

/// A character's job.
#[derive(Clone, Debug)]
pub enum Job {
    Idle,
    Patrol(Box<Patrol>),
}

/// What a character is doing now.
#[derive(Clone, Debug)]
pub enum Goal {
    None,
    /// Its job.
    Job,
    /// Standing idle (not its job: a patrol that ended, or too calm for its
    /// job).
    Idle,
    Pursuit(Box<Pursuit>),
    Combat(Box<Combat>),
    /// Cowering in fear (a non-combatant).
    Panic(Box<Panic>),
    /// A patrol a script ordered (`ai2_dopath`): back to the job when done.
    Path(Box<Patrol>),
    /// Running to an alarm console to raise the alarm.
    Alarm(Box<AlarmRun>),
    /// A neutral character's interaction with the player
    /// (`AI2cGoal_Neutral`): the world runs it (`world/neutral.rs`).
    Neutral,
}

/// On the way to an alarm console.
#[derive(Clone, Debug)]
pub struct AlarmRun {
    /// Where it heads: a little short of the console's marker.
    dest: Vec3,
    stand_wait: u32,
}

/// Alarm runs: aim a little short of the marker, at most this many tries
/// to get there again, this long to wait to stand still at it.
const ALARM_OFFSET: f32 = 5.0;
const ALARM_REPATHS: u32 = 3;
const ALARM_STAND_WAIT: u32 = 60;
/// A marker is reached within this.
const MARKER_REACH: f32 = 10.0;

/// A non-combatant's panic.
#[derive(Clone, Debug, Default)]
pub struct Panic {
    /// What scares it most (a contact serial).
    cause: Option<u32>,
    /// It calms down at this tick.
    pub until: u64,
    kneeling: bool,
    dazed: Option<u32>,
    done_daze: bool,
    by_hostile: bool,
    /// When it next whimpers (ticks).
    say_timer: u32,
}

impl Goal {
    pub fn name(&self) -> &'static str {
        match self {
            Goal::None => "none",
            Goal::Job => "job",
            Goal::Idle => "idle",
            Goal::Pursuit(_) => "pursuit",
            Goal::Combat(_) => "combat",
            Goal::Panic(_) => "panic",
            Goal::Path(_) => "path",
            Goal::Alarm(_) => "alarm",
            Goal::Neutral => "neutral",
        }
    }
}

/// A patrol in progress.
#[derive(Clone, Debug)]
pub struct Patrol {
    pub path: Rc<PatrolPath>,
    pub current: usize,
    begun: bool,
    pause: u32,
    scanguard: u32,
    scanning: bool,
    guarding: bool,
    started_guarding: bool,
    scan: Scan,
    done_glance: Option<usize>,
    stationary_glance: bool,
    pub locked: bool,
    /// Walking back to where it left the path.
    return_to: Option<Vec3>,
    prev_point: Vec3,
    on_path: bool,
    failed: bool,
    fails: u32,
    movement_waypoint: Option<usize>,
    shooting: u32,
    scan_course: Option<f32>,
    /// Shooting at a flag: where, the extra spread, and its aim on it.
    shoot_at: Option<Vec3>,
    shoot_spread: f32,
    shot: Option<Box<crate::ai::ranged::Targeting>>,
}

impl Patrol {
    pub fn new(path: Rc<PatrolPath>) -> Patrol {
        Patrol {
            path,
            current: 0,
            begun: false,
            pause: 0,
            scanguard: 0,
            scanning: false,
            guarding: false,
            started_guarding: false,
            scan: Scan::default(),
            done_glance: None,
            stationary_glance: false,
            locked: false,
            return_to: None,
            prev_point: Vec3::ZERO,
            on_path: false,
            failed: false,
            fails: 0,
            movement_waypoint: None,
            shooting: 0,
            shoot_at: None,
            shoot_spread: 0.0,
            shot: None,
            scan_course: None,
        }
    }
}

/// Pursuit modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PursuitMode {
    None,
    Forget,
    GoTo,
    Wait,
    Look,
    Move,
    Hunt,
    Glance,
}

impl PursuitMode {
    fn from_name(n: &str) -> PursuitMode {
        match n {
            "forget" => PursuitMode::Forget,
            "goTo" => PursuitMode::GoTo,
            "wait" => PursuitMode::Wait,
            "look" => PursuitMode::Look,
            "move" => PursuitMode::Move,
            "hunt" => PursuitMode::Hunt,
            "glance" => PursuitMode::Glance,
            _ => PursuitMode::None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            PursuitMode::None => "none",
            PursuitMode::Forget => "forget",
            PursuitMode::GoTo => "goTo",
            PursuitMode::Wait => "wait",
            PursuitMode::Look => "look",
            PursuitMode::Move => "move",
            PursuitMode::Hunt => "hunt",
            PursuitMode::Glance => "glance",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    Initial,
    PostGlance,
    PostArrival,
    ChangeTarget,
}

/// Investigating a contact.
#[derive(Clone, Debug)]
pub struct Pursuit {
    /// The contact pursued (its serial).
    pub target: u32,
    pub mode: PursuitMode,
    delay: u32,
    certain: bool,
    danger: bool,
    last_los_check: u64,
    los_ok: u32,
    los_seen: bool,
    close: bool,
    running: bool,
    scanning: bool,
    scan: Scan,
    /// When it next says its pursuit line (ticks).
    say_timer: u32,
}

/// Primary movements in combat, by precedence (ties go to the earlier).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary {
    Hold,
    Advance,
    Retreat,
    FindAlarm,
    Melee,
    GetUp,
    /// Going for a gun.
    Gun,
}

/// Whether the gun in hand has a clear line of fire at the target: within
/// its range, through no wall, and past no friend (a cylinder round each).
fn line_of_fire(target_feet: Vec3, cx: &Ctx) -> bool {
    let Some(g) = cx.gun else { return true };
    let from = g.matrix.w_axis.truncate();
    let Some(t) = cx.others.iter().find(|o| o.feet == target_feet) else {
        return true;
    };
    let to = t.pelvis;
    if g.ai.max_range > 0.0 && from.distance(to) > g.ai.max_range {
        return false;
    }
    // A lobbed shot needs an arc that reaches. (The arc itself isn't traced
    // against the level: the straight line stands in.)
    if let Some((speed, gravity)) = g.ai.aim.ballistic {
        if crate::ai::ranged::lob(to - from, speed, gravity * 98.0).is_none() {
            return false;
        }
    }
    if !(cx.sight)(from, to, SHOOT) {
        return false;
    }
    let d = to - from;
    let flat = Vec3::new(d.x, 0.0, d.z);
    cx.others
        .iter()
        .enumerate()
        .filter(|(i, o)| *i != cx.me && o.alive && o.friend && o.feet != target_feet)
        .all(|(_, o)| {
            // The segment against a vertical cylinder of radius 4.
            let rel = Vec3::new(from.x - o.feet.x, 0.0, from.z - o.feet.z);
            let a = flat.length_squared();
            if a < 1e-6 {
                return true;
            }
            let b = 2.0 * rel.dot(flat);
            let c = rel.length_squared() - 16.0;
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 {
                return true;
            }
            let s = (-b - disc.sqrt()) / (2.0 * a);
            if !(0.0..=1.0).contains(&s) {
                return true;
            }
            let y = from.y + d.y * s;
            !(o.feet.y..=o.feet.y + o.height + 8.0).contains(&y)
        })
}

/// Dodging: a body's reach, and how far a dodge goes at a time.
const BODY_RADIUS: f32 = 6.0;
const DODGE_STEP: f32 = 20.0;

/// Going for a gun: how far it looks, and how near it bends for it.
const GUN_SEARCH: f32 = 200.0;
const GUN_REACH: f32 = 12.0;

/// Range bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Band {
    None,
    Long,
    Medium,
    Short,
    MediumRetreat,
    LongRetreat,
}

/// How an advance moves: by a route through the rooms, or walking a heading
/// (the local move: straight at the target, or the clearest way near it).
#[derive(Clone, Copy, Debug, PartialEq)]
enum AdvanceWay {
    Route,
    Steer(f32),
}

/// The local move's urgency by the worst square on it
/// (`AI2cManeuver_Primary_LocalWeights`: clear, near a wall, border 1, border 2,
/// semi-passable; the rest 0).
fn local_weight(sq: crate::nav::Square) -> f32 {
    use crate::nav::Square::*;
    match sq {
        Clear => 1.0,
        NearWall => 0.85,
        Border1 => 0.7,
        Border2 => 0.6,
        SemiPassable => 0.3,
        _ => 0.0,
    }
}

/// The local path search toward `heading` (`AI2rFindLocalPath`): the
/// heading, then ±15°, ±30°, ±45°, ±60°, each probed 15 ahead stopping at
/// border 3 (3 squares of escape); the first that gets there, and its worst
/// square.
fn fan(heading: f32, from: Vec3, cx: &Ctx) -> Option<(f32, crate::nav::Square)> {
    const LOCAL_DISTANCE: f32 = 15.0;
    const STEP: f32 = 15.0 * PI / 180.0;
    std::iter::once(0.0)
        .chain((1..=4).flat_map(|k| [k as f32 * STEP, -(k as f32) * STEP]))
        .map(|off| heading + off)
        .find_map(|h| {
            let p = (cx.probe)(
                from,
                glam::Vec2::new(h.sin(), h.cos()),
                LOCAL_DISTANCE,
                crate::nav::Square::Border3,
            )?;
            p.stopped.is_none().then_some((h, p.worst))
        })
}

/// Fighting an enemy.
#[derive(Clone, Debug)]
pub struct Combat {
    pub target: usize,
    contact: u32,
    target_strength: Strength,
    pub melee: Option<Box<Melee>>,
    pub primary: Primary,
    band: Band,
    advance: (f32, f32),
    pub(super) retreat: (f32, f32),
    try_advance: u32,
    lost_contact: Option<u32>,
    target_dead: bool,
    investigate: u32,
    taunt_enable: bool,
    done_taunt: bool,
    last_hit: u64,
    last_melee_hit: u64,
    daze: Option<u32>,
    advance_goal: Option<Vec3>,
    /// When its advance last found no route (it walks without one for 90
    /// ticks before asking again).
    route_failed: Option<u64>,
    /// How the last advance check said to move (`AI2iManeuver_CheckAdvance`).
    advance_way: Option<AdvanceWay>,
    /// Shooting: its aim on the target, whether it has a line of fire, and
    /// whether this is the first sight of this enemy (startle shots).
    targeting: Option<Box<crate::ai::ranged::Targeting>>,
    pub(super) have_los: bool,
    first_sight: bool,
    /// How much too close for the gun the target is (0–1).
    too_close: f32,
    /// Going for a gun: the one it is after, its roll of the class's chance
    /// (once a fight), and the guns it found it couldn't reach.
    gun_target: Option<usize>,
    gun_roll: Option<bool>,
    unreachable: Vec<usize>,
    /// Picking it up on the run: its roll of the class's chance (once a
    /// gun), and whether it is doing so (`gun_running_pickup`).
    running_roll: Option<bool>,
    gun_running: bool,
    /// Decisions left of the melee its gun earned (the target is down).
    weapon_melee: u32,
    /// What it is dodging, and the way to go now (weighted; none below a
    /// trace).
    dodges: Vec<Dodge>,
    dodge: Option<(Vec3, f32)>,
    /// A boss's own state (`docs/AI.md`, *Bosses*).
    pub boss: super::bosses::Boss,
    /// How far it will look for a gun (a boss's; none: anywhere).
    pub gun_search: Option<(f32, f32)>,
}

/// An AI character's goals (behaviour per `docs/GOALS.md`).
#[derive(Clone, Debug)]
pub struct Agent {
    pub name: String,
    pub job: Job,
    pub goal: Goal,
    /// Below this alert it doesn't take up its job (stands idle instead).
    pub job_alert: Alert,
    /// It pursues only at this alert or above.
    pub investigate_alert: Alert,
    pub pursue: PursueSettings,
    pub profile: Rc<CombatProfile>,
    pub class: ClassCombat,
    /// Where its patrol had it last (pursuit limits are measured from here).
    pub job_location: Option<Vec3>,
    pub mode: MoveMode,
    pub nav: Option<Navigator>,
    /// Walking a heading with no route (the combat advance's local move,
    /// `primary_movement_modifier_angle`): which way, and how fast.
    steer: Option<(f32, Pace)>,
    dest: Option<Dest>,
    look: Look,
    glance: Option<(f32, u32)>,
    turning_to: Option<f32>,
    /// Buttons pressed this tick (getting up, taunting).
    press: u32,
    /// Where its eyes look (yaw): toward what it looks at, as far as the
    /// head turns from the body (vision looks this way).
    pub eye: Option<f32>,
    /// Scripts its patrol asked to run (the mission takes them).
    pub scripts: Vec<String>,
    /// The alarm console it is going for (kept while it breaks off to
    /// fight), the last one it tried, how often it tried to get there.
    pub alarm_console: Option<usize>,
    last_alarm_console: Option<usize>,
    alarm_repaths: u32,
    /// Fighting on the way to an alarm: back to it when this runs out.
    pub fight_timer: u32,
    /// A console it wants to start using (the world takes it).
    pub use_console: Option<usize>,
    /// Waiting for a patrol script to finish.
    waiting_script: bool,
    pub(super) rng: Rng,
    /// Shooting: the way its gun's barrel should point (aiming with the
    /// gun), how fast the aim may swing (×), and whether to hold the trigger.
    pub aim: Option<Vec3>,
    pub aim_speed: f32,
    pub trigger: bool,
    /// The trigger it pulls is the alternate fire's (Barabbas' grenades).
    pub alt_fire: bool,
    /// What it asks of its body this tick, for the world to do: an
    /// animation type (forced, or as appropriate), health back, the fight
    /// stance (`ONrCharacter_DoAnimation`, `_Heal`, `_FightMode`).
    pub anim_request: Option<(u16, bool)>,
    pub heal: i32,
    pub fight_mode: bool,
    /// Health it has given itself regenerating (`regenerated_amount`).
    pub regenerated: i32,
    /// Invisibility it gives itself (the super ninja's), where its teleport
    /// takes it (and facing which way: `teleportDestination`), a teleport
    /// out at once (from a fall), and a height it hangs at.
    pub give_invisibility: u16,
    pub teleport_to: Option<(Vec3, f32)>,
    pub teleport_now: Option<(Vec3, f32)>,
    pub hold_height: Option<f32>,
    /// Its super amount, set outright (Mutant Muro's shield), and its stun
    /// cleared (to turtle).
    pub set_super: Option<f32>,
    pub clear_stun: bool,
    /// A gun it wants to bend for now (the world drops its own and starts
    /// the pickup).
    pub want_pickup: Option<usize>,
    /// Its own gun let go (before a running pickup).
    pub drop_gun: bool,
    /// It aims with its gun, rather than just looking
    /// (`AI2tMovementState.aiming_weapon`): in combat as its targeting
    /// forces it (with a gun it aims, else looking), else by its movement
    /// mode. Not: its gun's poses go (`ONcCharacterFlag_DontUseGunVarient`)
    /// and its gun's aiming speed doesn't count.
    pub aiming_weapon: bool,
    /// Extra spread on its shots (degrees; a patrol's shoot-at-flag).
    pub extra_spread: f32,
    /// It left its neutral interaction (for the world to stop it,
    /// `AI2rNeutral_Exit`): Some(true) when a danger made it (the enemy
    /// line, not the call to come back).
    pub neutral_exit: Option<bool>,
}

impl Agent {
    pub fn new(
        name: String,
        job: Job,
        profile: Rc<CombatProfile>,
        class: ClassCombat,
        seed: u32,
    ) -> Agent {
        Agent {
            name,
            job,
            goal: Goal::None,
            job_alert: Alert::Low,
            investigate_alert: Alert::Medium,
            pursue: PursueSettings::default(),
            profile,
            class,
            job_location: None,
            mode: MoveMode::ByAlert,
            nav: None,
            steer: None,
            dest: None,
            look: Look::None,
            glance: None,
            turning_to: None,
            press: 0,
            eye: None,
            scripts: Vec::new(),
            alarm_console: None,
            last_alarm_console: None,
            alarm_repaths: 0,
            fight_timer: 0,
            use_console: None,
            waiting_script: false,
            aim: None,
            aim_speed: 1.0,
            trigger: false,
            alt_fire: false,
            anim_request: None,
            heal: 0,
            fight_mode: false,
            regenerated: 0,
            give_invisibility: 0,
            teleport_to: None,
            teleport_now: None,
            hold_height: None,
            set_super: None,
            clear_stun: false,
            want_pickup: None,
            drop_gun: false,
            aiming_weapon: true,
            extra_spread: 0.0,
            neutral_exit: None,
            rng: Rng(seed.wrapping_mul(2_654_435_761).max(1)),
        }
    }

    /// The melee AI, when combat has it fighting hand to hand.
    pub fn melee_mut(&mut self) -> Option<&mut Melee> {
        match &mut self.goal {
            Goal::Combat(c) if c.primary == Primary::Melee && !c.target_dead => {
                c.melee.as_deref_mut()
            }
            _ => None,
        }
    }
    pub fn melee(&self) -> Option<&Melee> {
        match &self.goal {
            Goal::Combat(c) if c.primary == Primary::Melee && !c.target_dead => c.melee.as_deref(),
            _ => None,
        }
    }

    /// Whom it fights, if anyone.
    /// The gun it is going for in a fight, if any (it doesn't kick that
    /// one about).
    pub fn gun_wanted(&self) -> Option<usize> {
        match &self.goal {
            Goal::Combat(c) => c.gun_target,
            _ => None,
        }
    }

    pub fn opponent(&self) -> Option<usize> {
        match &self.goal {
            Goal::Combat(c) if !c.target_dead => Some(c.target),
            _ => None,
        }
    }

    /// A one-line state for logs and scripts.
    pub fn describe(&self) -> String {
        match &self.goal {
            Goal::Job => match &self.job {
                Job::Idle => "job idle".into(),
                Job::Patrol(p) => format!("job patrol {} @{}", p.path.name, p.current),
            },
            Goal::Pursuit(p) => format!("pursuit {}", p.mode.name()),
            Goal::Combat(c) => format!("combat {:?} → {}", c.primary, c.target),
            Goal::Panic(p) => format!("panic{}", if p.kneeling { " kneeling" } else { "" }),
            Goal::Path(p) => format!("path {} @{}", p.path.name, p.current),
            Goal::Alarm(_) => format!("alarm {}", self.alarm_console.map_or(-1, |c| c as i64)),
            g => g.name().to_string(),
        }
    }

    /// It has taunted over a dead target this fight.
    pub(crate) fn body_taunted(&self) -> bool {
        matches!(&self.goal, Goal::Combat(c) if c.done_taunt)
    }

    pub(crate) fn casual(&self) -> bool {
        match &self.goal {
            Goal::None | Goal::Idle => true,
            Goal::Job => !matches!(&self.job, Job::Patrol(p) if p.locked),
            Goal::Path(p) => !p.locked,
            _ => false,
        }
    }

    /// Whether it is following a patrol path (its job's or a script's).
    fn patrolling(&self) -> bool {
        matches!(self.goal, Goal::Path(_))
            || (matches!(self.goal, Goal::Job) && matches!(self.job, Job::Patrol(_)))
    }

    /// Follows patrol path `path` now (a script's order); back to the job
    /// when it ends.
    pub fn do_path(&mut self, path: Rc<PatrolPath>, cx: &mut Ctx) {
        self.exit(cx);
        self.goal = Goal::Path(Box::new(Patrol::new(path)));
        eprintln!("ai: {} → {}", self.name, self.describe());
        self.patrol_enter(cx);
    }

    /// What it is doing now becomes its job (`ai2_setjobstate`).
    pub fn make_job(&mut self) {
        match std::mem::replace(&mut self.goal, Goal::Job) {
            Goal::Path(p) => self.job = Job::Patrol(p),
            Goal::Idle | Goal::None => self.job = Job::Idle,
            Goal::Job => {}
            other => self.goal = other,
        }
        self.job_location = None;
    }

    /// Stops what it is doing (made passive): no goal until it is woken.
    pub fn stop(&mut self, cx: &mut Ctx) {
        self.exit(cx);
        self.goal = Goal::None;
    }

    /// Stands idle now (`ai2_idle`), its job kept.
    pub fn idle_now(&mut self, cx: &mut Ctx) {
        self.exit(cx);
        self.enter_idle(cx);
    }

    /// A patrol script it asked to run has finished.
    pub fn script_done(&mut self) {
        self.waiting_script = false;
    }

    /// Whether it is cowering (the panic animation variants).
    pub fn panicking(&self) -> bool {
        matches!(self.goal, Goal::Panic(_))
    }

    /// Whether its alert may ease (idle or patrolling, not locked in).
    pub fn calming(&self) -> bool {
        match &self.goal {
            Goal::Idle => true,
            Goal::Job => !matches!(&self.job, Job::Patrol(p) if p.locked),
            Goal::Path(p) => !p.locked,
            _ => false,
        }
    }

    // --- movement helpers ------------------------------------------------------

    fn go_to(&mut self, at: Vec3, within: f32, stop: bool, face: Option<f32>, cx: &Ctx) {
        self.steer = None;
        self.dest = Some(Dest {
            at,
            within,
            stop,
            face,
        });
        self.nav = Some(Navigator::new(at, within, self.pace(cx)));
        self.turning_to = None;
    }

    /// A script's movement order (`AI2rPath_GoToPoint` from `ai2_lookatchar`
    /// and the like): go to `at` (within `within`), then face `face`; it
    /// looks at `look` meanwhile. Carried out even while passive (the goals
    /// don't run; the movement does).
    pub fn script_move(
        &mut self,
        at: Vec3,
        within: f32,
        face: Option<f32>,
        look: Option<Vec3>,
        alert: Alert,
    ) {
        self.dest = Some(Dest {
            at,
            within,
            stop: true,
            face,
        });
        self.nav = Some(Navigator::new(at, within, self.mode.pace(alert, false)));
        self.turning_to = None;
        if let Some(p) = look {
            self.look = Look::At(p);
        }
    }

    /// Looks at `at` (`AI2rMovement_LookAtCharacter`, as `ai2_lookatme` has
    /// it): its eyes, and its body when they can't turn that far.
    pub fn look_at(&mut self, at: Vec3) {
        self.look = Look::At(at);
    }

    /// Stops looking at anything (`AI2rMovement_StopAiming`).
    pub fn stop_looking(&mut self) {
        self.look = Look::None;
    }

    /// The advance's viability check (`AI2iManeuver_CheckAdvance`;
    /// `docs/AI.md`, *Combat advance*): its weight now — and how it moves,
    /// or None if it doesn't. `weight` is its weight so far, `second` the
    /// best other choice's.
    ///
    /// Within 50 it first probes straight at the target (stopping only at
    /// danger; an escape out of a bad area must have a clear line). Close in
    /// (urgency 0: within the advance's near distance) it stops when
    /// within 6, or the straight walk is good, or nothing blocks the line.
    /// Its weight takes the urgency (at least half); below the second
    /// choice, no. Then: a straight walk no worse than border 2 is taken;
    /// else a route through the rooms, unless it would be a simple one (the
    /// straight walk reached it, over rough ground) — then, as with no
    /// route, the first of the target's direction and ±15°…±60° clear for
    /// 15 short of border 3. The walk's worst square scales the weight
    /// (clear 1 … semi-passable 0.3); below the second choice, no.
    fn check_advance(
        &mut self,
        target: Vec3,
        dist: f32,
        weight: f32,
        second: f32,
        cx: &Ctx,
    ) -> (f32, Option<AdvanceWay>) {
        use crate::nav::Square;
        const LOCAL_TRY: f32 = 50.0;
        let (min, max) = self.combat().advance;
        let urgency = if dist > max {
            1.0
        } else if dist < min {
            0.0
        } else {
            (dist - min) / (max - min).max(1e-3)
        };
        let to = (target - cx.feet).with_y_zero();
        let heading = to.x.atan2(to.z);
        let from = cx.feet + Vec3::Y * 2.0;
        // The line from the pelvis to the target's middle
        // (`AI2rManeuver_CheckBlockage`: the level, characters skipped).
        let target_mid = target + Vec3::Y * (cx.pelvis.y - cx.feet.y);
        let blocked = || !(cx.sight)(cx.pelvis, target_mid, crate::quads::flag::CHR_COL_SKIP);
        let straight = if dist < LOCAL_TRY {
            (cx.probe)(
                from,
                glam::Vec2::new(heading.sin(), heading.cos()),
                dist,
                Square::Danger,
            )
        } else {
            None
        };
        let mut reached = straight.is_some_and(|p| p.stopped.is_none());
        if reached && straight.is_some_and(|p| p.escaped) {
            reached = !blocked();
        }
        let close_enough = if urgency > 0.0 {
            false
        } else if dist < ADVANCE_MIN || reached {
            true
        } else {
            !blocked()
        };
        if close_enough {
            return (0.0, None);
        }
        let weight = weight * urgency.max(0.5);
        if weight < second {
            return (0.0, None);
        }
        let worst = straight.map_or(Square::Impassable, |p| p.worst);
        if reached && worst < Square::Border3 {
            let weight = weight * local_weight(worst);
            return if weight < second {
                (weight, None)
            } else {
                (weight, Some(AdvanceWay::Steer(heading)))
            };
        }
        // A route — if there is one and it isn't simple.
        let simple = reached;
        if !simple && (cx.path_length)(cx.feet, target).is_some() {
            return (weight, Some(AdvanceWay::Route));
        }
        match fan(heading, from, cx) {
            Some((h, worst)) => {
                let weight = weight * local_weight(worst);
                if weight < second {
                    (weight, None)
                } else {
                    (weight, Some(AdvanceWay::Steer(h)))
                }
            }
            None => (0.0, None),
        }
    }

    /// The advance by a route (the check chose one): followed while it
    /// holds, a fresh one when the target has moved on. One the planner
    /// can't find (a door it can't open shuts the way) is known at once: it
    /// walks the clearest way toward the target instead
    /// (`AI2rFindLocalPath`) and asks again after 90 ticks.
    fn advance_move(&mut self, target: Vec3, min: f32, cx: &Ctx) {
        const RETRY: u64 = 90;
        if self.path_failed() || self.nav.as_ref().is_some_and(|n| n.plan_failed) {
            self.combat().route_failed = Some(cx.now);
            self.nav = None;
            self.dest = None;
        }
        let may_route = self
            .combat()
            .route_failed
            .is_none_or(|t| cx.now >= t + RETRY);
        if may_route {
            let stale = self
                .combat()
                .advance_goal
                .is_none_or(|g| g.distance(target) > 10.0);
            if stale || self.nav.is_none() {
                self.combat().advance_goal = Some(target);
                self.combat().route_failed = None;
                self.go_to(target, min, true, None, cx);
            }
            return;
        }
        self.combat().advance_goal = None;
        let to = (target - cx.feet).with_y_zero();
        let pace = self.mode.pace(cx.alert(), true);
        self.steer = fan(to.x.atan2(to.z), cx.feet + Vec3::Y * 2.0, cx).map(|(h, _)| (h, pace));
    }

    fn halt(&mut self) {
        self.dest = None;
        self.nav = None;
        self.steer = None;
    }

    // --- the neutral interaction's movement (`world/neutral.rs`) ---------

    /// Enters its neutral interaction (`AI2rNeutral_Enter`): running, not
    /// aiming, no glances.
    pub fn neutral_enter(&mut self, cx: &mut Ctx) {
        self.exit(cx);
        self.neutral_exit = None;
        self.goal = Goal::Neutral;
        self.mode = MoveMode::RunNoAim;
        self.glance = None;
        self.look = Look::None;
        eprintln!("ai: {} → neutral", self.name);
    }

    /// Stands where it is (`AI2rPath_Halt`), or turns to `face` there.
    pub fn neutral_halt(&mut self, feet: Vec3, face: Option<f32>) {
        self.halt();
        if let Some(f) = face {
            self.dest = Some(Dest {
                at: feet,
                within: 0.0,
                stop: true,
                face: Some(f),
            });
        }
    }

    /// Walks rather than runs from now on (to give its items).
    pub fn neutral_walk(&mut self) {
        self.mode = MoveMode::WalkNoAim;
    }

    /// Its movement order found no way.
    pub fn move_failed(&self) -> bool {
        self.path_failed()
    }

    fn pace(&self, cx: &Ctx) -> Pace {
        self.mode.pace(cx.alert(), self.hurrying())
    }

    fn hurrying(&self) -> bool {
        matches!(&self.goal, Goal::Pursuit(p) if p.mode == PursuitMode::GoTo && p.running)
    }

    /// Whether it aims with its gun now (see `aiming_weapon`): fighting, as
    /// its targeting forces (`AI2iAI_TargetingCallback_ForceAimWithWeapon`:
    /// with a usable gun it aims — not a boss that only looks — else it
    /// looks); otherwise its movement mode says.
    fn aims_weapon_now(&self, cx: &Ctx) -> bool {
        if matches!(self.goal, Goal::Combat(_)) {
            let boss_looks = self.barabbas_targeting().is_some_and(|b| !b.aim)
                || self.ninja_mode().is_some()
                || self.is_muro();
            return cx.gun.is_some_and(|g| g.usable) && !boss_looks;
        }
        self.mode
            .resolved(cx.alert(), self.hurrying())
            .aims_weapon()
    }

    fn arrived(&self, cx: &Ctx) -> bool {
        let Some(d) = self.dest else { return true };
        let there = matches!(self.nav.as_ref().map(|n| &n.state), Some(NavState::Arrived))
            || self.distance_to_dest(cx) <= d.within.max(3.0);
        there
            && d.face
                .is_none_or(|f| relative(f - cx.facing).abs() <= MAX_TURN)
    }

    fn distance_to_dest(&self, cx: &Ctx) -> f32 {
        self.dest.map_or(0.0, |d| {
            let v = d.at - cx.feet;
            Vec3::new(v.x, 0.0, v.z).length()
        })
    }

    fn path_failed(&self) -> bool {
        matches!(
            self.nav.as_ref().map(|n| &n.state),
            Some(NavState::Failed(_))
        )
    }

    fn glance_at(&mut self, at: Vec3, ticks: u32, cx: &Ctx) {
        self.glance = Some((yaw_toward(cx.feet, at), ticks));
    }

    // --- the goal machine ----------------------------------------------------

    /// Leaves the current goal.
    fn exit(&mut self, cx: &mut Ctx) {
        match std::mem::replace(&mut self.goal, Goal::None) {
            Goal::Job => {
                if let Job::Patrol(p) = &mut self.job {
                    p.pause = 0;
                    p.shooting = 0;
                    p.return_to = Some(cx.feet);
                    p.prev_point = cx.feet;
                }
            }
            Goal::Panic(_) => self.mode = MoveMode::ByAlert,
            Goal::Path(_) => self.waiting_script = false,
            Goal::Alarm(_) => self.mode = MoveMode::ByAlert,
            Goal::Neutral => {
                self.mode = MoveMode::ByAlert;
                self.neutral_exit.get_or_insert(false);
            }
            Goal::Combat(c) => {
                cx.fights.leave(cx.me);
                // (A boss behaviour ending: its movement mode back.)
                if !matches!(c.boss, super::bosses::Boss::None) {
                    self.mode = MoveMode::ByAlert;
                }
                // The alert drops to high on leaving combat.
                if cx.perception.alert > Alert::High {
                    cx.perception.alert = Alert::High;
                }
                drop(c);
            }
            _ => {}
        }
        self.halt();
        self.look = Look::None;
        self.glance = None;
        self.turning_to = None;
        cx.perception.locked = false;
    }

    /// Goes back to its job (`docs/GOALS.md`, *Returning to the job*):
    /// idle where it stands if its alert is below its job alert.
    pub fn return_to_job(&mut self, set_idle: bool, even_if_alerted: bool, cx: &mut Ctx) {
        let idle = cx.alert() < self.job_alert;
        if matches!(self.goal, Goal::Job) {
            return;
        }
        if idle && !set_idle && !matches!(self.goal, Goal::None) {
            return;
        }
        if !self.casual() && !even_if_alerted {
            return;
        }
        self.exit(cx);
        if idle {
            self.enter_idle(cx);
        } else {
            self.goal = Goal::Job;
            self.enter_job(cx);
        }
        eprintln!("ai: {} → {}", self.name, self.describe());
    }

    fn enter_idle(&mut self, _cx: &mut Ctx) {
        self.goal = Goal::Idle;
        self.halt();
        self.look = Look::None;
    }

    fn enter_job(&mut self, cx: &mut Ctx) {
        match &self.job {
            Job::Idle => self.halt(),
            Job::Patrol(_) => self.patrol_enter(cx),
        }
    }

    /// Acts on what perception noticed (the knowledge callbacks of the
    /// current goal).
    pub fn notice(&mut self, n: Notice, cx: &mut Ctx) {
        match n {
            Notice::Combat(who) => {
                if !matches!(self.goal, Goal::Combat(_))
                    && cx.others.get(who).is_some_and(|o| o.alive)
                {
                    self.enter_combat(who, cx);
                }
            }
            Notice::Pursue(id) => {
                if !matches!(self.goal, Goal::Pursuit(_) | Goal::Combat(_)) {
                    self.enter_pursuit(id, cx);
                }
            }
            Notice::Glance(at, ticks) => {
                if !matches!(self.goal, Goal::Combat(_)) {
                    self.glance_at(at, ticks, cx);
                }
            }
            Notice::Knowledge { id, degrade, event } if cx.perception.non_combatant => {
                self.noncombatant_knowledge(id, degrade, event, cx)
            }
            Notice::Knowledge { id, degrade, event } => match self.goal {
                Goal::Pursuit(_) => self.pursuit_knowledge(id, degrade, cx),
                Goal::Combat(_) => self.combat_knowledge(id, degrade, event, cx),
                _ => {}
            },
            Notice::Startle(_) => {}
        }
    }

    /// One tick of the current goal.
    pub fn update(&mut self, cx: &mut Ctx) {
        self.press = 0;
        self.aim = None;
        self.trigger = false;
        self.anim_request = None;
        self.heal = 0;
        self.fight_mode = false;
        self.give_invisibility = 0;
        self.teleport_now = None;
        self.hold_height = None;
        self.clear_stun = false;
        self.extra_spread = 0.0;
        self.aiming_weapon = self.aims_weapon_now(cx);
        if let Some((_, t)) = &mut self.glance {
            *t = t.saturating_sub(1);
            if *t == 0 {
                self.glance = None;
            }
        }
        if matches!(self.goal, Goal::None) {
            self.return_to_job(true, false, cx);
        }
        // Fighting on the way to an alarm: back to the alarm in time.
        if self.alarm_console.is_some()
            && !matches!(self.goal, Goal::Alarm(_))
            && self.fight_timer > 0
        {
            self.fight_timer -= 1;
            if self.fight_timer == 0 {
                self.alarm_setup(None, cx);
            }
        }
        // Where its patrol has it.
        if matches!(self.goal, Goal::Job) {
            if let Job::Patrol(p) = &self.job {
                if p.return_to.is_none() && p.on_path {
                    self.job_location = Some(cx.feet);
                }
            }
        }
        match self.goal {
            Goal::Job => {
                if matches!(self.job, Job::Patrol(_)) {
                    self.patrol_update(cx);
                }
            }
            Goal::Pursuit(_) => self.pursuit_update(cx),
            Goal::Combat(_) => self.combat_update(cx),
            Goal::Panic(_) => self.panic_update(cx),
            Goal::Path(_) => self.patrol_update(cx),
            Goal::Alarm(_) => self.alarm_update(cx),
            Goal::Idle | Goal::None | Goal::Neutral => {}
        }
        let hurry =
            matches!(&self.goal, Goal::Pursuit(p) if p.mode == PursuitMode::GoTo && p.running);
        let pace = self.mode.pace(cx.perception.alert, hurry);
        if let Some(n) = self.nav.as_mut() {
            n.set_pace(pace);
        }
        cx.perception.locked = match (&self.goal, &self.job) {
            (Goal::Job, Job::Patrol(p)) => p.locked,
            (Goal::Path(p), _) => p.locked,
            _ => false,
        };
    }

    /// This tick's movement for a body at `feet` facing `facing`, plus
    /// buttons to press.
    pub fn orders(&mut self, feet: Vec3, facing: f32, now: u64) -> (Orders, u32) {
        let mut o = match (self.nav.as_mut(), self.steer) {
            (Some(n), _) => n.orders(feet, facing, now),
            (None, Some((heading, pace))) => Orders {
                direction: Direction::Forward,
                pace,
                face: Some(heading),
            },
            (None, None) => Orders::default(),
        };
        // (Turning on the spot to set off belongs to the walk.)
        let walking = self
            .nav
            .as_ref()
            .is_some_and(|n| matches!(n.state, NavState::Following | NavState::Planning));
        if o.direction == Direction::Stopped && !walking {
            let arrived_facing = self.dest.and_then(|d| d.face).filter(|_| {
                matches!(self.nav.as_ref().map(|n| &n.state), Some(NavState::Arrived))
                    || self
                        .dest
                        .is_some_and(|d| (d.at - feet).with_y_zero().length() <= d.within.max(3.0))
            });
            if let Some(f) = arrived_facing {
                o.face = Some(f);
            } else {
                let (want, range) = match (self.glance, self.look) {
                    (Some((g, _)), _) => (Some(g), STANDING_GLANCE_RANGE),
                    (None, Look::At(p)) => (Some(yaw_toward(feet, p)), self.look_range()),
                    (None, Look::Dir(d)) => (Some(d), self.look_range()),
                    (None, Look::None) => (None, 0.0),
                };
                self.eye = want.map(|w| facing + relative(w - facing).clamp(-range, range));
                if let Some(w) = want {
                    let from = self.turning_to.unwrap_or(facing);
                    if relative(w - from).abs() > range {
                        self.turning_to = Some(w);
                    }
                }
                if let Some(t) = self.turning_to {
                    o.face = Some(t);
                    if relative(t - facing).abs() <= MAX_TURN {
                        self.turning_to = None;
                    }
                }
            }
        } else {
            self.turning_to = None;
            // On the move it looks where it goes (glancing aside a little).
            let range = STANDING_LOOK_RANGE;
            self.eye = self
                .glance
                .map(|(g, _)| facing + relative(g - facing).clamp(-range, range));
        }
        (o, self.press)
    }

    /// Where it wants to aim now, from `shoulder` (`AI2rMovementState`'s
    /// aiming modes → `AI2rExecutor_AimTarget` / `_AimDirection`): its gun's
    /// barrel when shooting, else a glance, else what it looks at or the way
    /// it looks; none: it stops aiming (`AI2rExecutor_AimStop`). With how
    /// fast the aim may swing (×; pursuing, half — `AI2cPursuit_AimingSpeed`).
    pub fn aim_want(&self, shoulder: Vec3) -> Option<(Vec3, f32)> {
        if let Some(dir) = self.aim {
            return Some((dir, self.aim_speed));
        }
        let speed = if matches!(self.goal, Goal::Pursuit(_)) {
            0.5
        } else {
            1.0
        };
        let yaw = |y: f32| Vec3::new(y.sin(), 0.0, y.cos());
        let dir = match (self.glance, self.look) {
            (Some((g, _)), _) => yaw(g),
            (None, Look::At(p)) => p - shoulder,
            (None, Look::Dir(d)) => yaw(d),
            (None, Look::None) => return None,
        };
        Some((dir, speed))
    }

    fn look_range(&self) -> f32 {
        match &self.goal {
            Goal::Pursuit(_) => PURSUIT_AIM_RANGE,
            Goal::Combat(_) => 10.0 * PI / 180.0,
            Goal::Panic(_) => PANIC_AIM_RANGE,
            _ => STANDING_LOOK_RANGE,
        }
    }

    // --- patrol (`docs/GOALS.md`, *Patrol*) -----------------------------------

    fn patrol(&mut self) -> &mut Patrol {
        match (&mut self.goal, &mut self.job) {
            (Goal::Path(p), _) => p,
            (_, Job::Patrol(p)) => p,
            _ => unreachable!("not patrolling"),
        }
    }

    fn patrol_enter(&mut self, cx: &mut Ctx) {
        let level = cx.level;
        let begun = self.patrol().begun;
        if begun {
            let nearest = self.patrol().path.return_to_nearest;
            let p = self.patrol();
            if p.current >= p.path.waypoints.len() {
                p.current = 0;
            }
            if nearest {
                // Resume at the movement waypoint nearest by path.
                let path = p.path.clone();
                let mut best: Option<(usize, f32)> = None;
                for (i, w) in path.waypoints.iter().enumerate() {
                    let Some(flag) = movement_flag(w) else {
                        continue;
                    };
                    let Some(f) = level.flags.get(&flag) else {
                        continue;
                    };
                    if let Some(d) = (cx.path_length)(cx.feet, f.at) {
                        if best.is_none_or(|(_, b)| d < b) {
                            best = Some((i, d));
                        }
                    }
                }
                if let Some((i, _)) = best {
                    let p = self.patrol();
                    p.current = i;
                    p.return_to = None;
                }
            }
        } else {
            let p = self.patrol();
            *p = Patrol::new(p.path.clone());
            p.begun = true;
        }
        self.patrol().on_path = false;
        let i = self.patrol().current;
        self.do_waypoint(i, true, false, cx);
    }

    /// Runs a waypoint's command (`fresh`: just reached it).
    fn do_waypoint(&mut self, index: usize, fresh: bool, retry: bool, cx: &mut Ctx) {
        let level = cx.level;
        if !retry {
            let p = self.patrol();
            p.failed = false;
            p.fails = 0;
        }
        if let Some(back) = self.patrol().return_to {
            self.go_to(back, RETURN_TO_PATH, false, None, cx);
            return;
        }
        let Some(w) = self.patrol().path.waypoints.get(index).cloned() else {
            return;
        };
        if movement_flag(&w).is_some()
            || matches!(
                w,
                Waypoint::MoveTo {
                    at: Target::Point(_),
                    ..
                }
            )
        {
            self.patrol().movement_waypoint = Some(index);
        }
        let flag = |id: i16| level.flags.get(&id).copied();
        match w {
            Waypoint::MoveTo {
                at,
                stop,
                face,
                within,
            } => {
                let facing = match (face, at) {
                    (true, Target::Flag(id)) => flag(id).map(|f| f.facing),
                    _ => None,
                };
                match level.place(at) {
                    Some(p) => self.go_to(p, within, stop, facing, cx),
                    None => eprintln!("ai: {}: patrol: no flag {:?}", self.name, at),
                }
            }
            Waypoint::Stop => self.patrol_finish(cx),
            Waypoint::Pause(n) => {
                if fresh {
                    self.patrol().pause = n;
                    self.halt();
                }
            }
            Waypoint::LookAt(t) => {
                if let Some(p) = level.place(t) {
                    self.look = Look::At(p);
                }
            }
            Waypoint::Loop => {
                // (A loop as the first waypoint would recurse for ever.)
                if index != 0 {
                    self.patrol().current = 0;
                    self.do_waypoint(0, true, false, cx);
                }
            }
            Waypoint::LoopTo(to) => {
                let n = self.patrol().path.waypoints.len();
                let cur = self.patrol().current;
                let mut next = to.min(n.saturating_sub(1));
                if next == cur {
                    next = (cur + 1) % n.max(1);
                }
                if next != cur {
                    self.patrol().current = next;
                    self.do_waypoint(next, true, false, cx);
                }
            }
            Waypoint::Pace(m) => self.mode = MoveMode::from_name(&m),
            Waypoint::StopLooking => self.look = Look::None,
            Waypoint::LookInDirection | Waypoint::FreeFacing => {}
            Waypoint::Glance { .. } | Waypoint::Scan { .. } => {} // in `fulfilled`
            Waypoint::StopScanning => {
                let p = self.patrol();
                if p.scanning {
                    p.scanning = false;
                    p.scanguard = 0;
                    self.look = Look::None;
                }
            }
            Waypoint::Guard { flag: id, .. } => {
                match flag(id) {
                    Some(f) => self.go_to(f.at, 0.0, true, Some(f.facing), cx),
                    None => eprintln!("ai: {}: patrol: no flag {id}", self.name),
                }
                let p = self.patrol();
                p.started_guarding = false;
                p.guarding = false;
                p.scanguard = 0;
            }
            Waypoint::Script { name, wait } => {
                // The mission runs it (with this character's name); a played
                // script holds the patrol until it ends.
                self.scripts.push(name);
                if wait {
                    self.waiting_script = true;
                }
            }
            Waypoint::Lock(lock) => {
                self.patrol().locked = lock;
                cx.perception.locked = lock;
                if !lock {
                    // Released: react to what it knows.
                    let notices = cx.perception.reconsider(cx.now);
                    for n in notices {
                        self.notice(n, cx);
                        if !self.patrolling() {
                            return;
                        }
                    }
                }
            }
            Waypoint::ShootAt {
                at,
                ticks,
                inaccuracy,
            } => {
                // Shoots at the spot for its time (no gun: just waits).
                let point = match at {
                    crate::ai::level::Target::Flag(id) => cx.level.flags.get(&id).map(|f| f.at),
                    crate::ai::level::Target::Point(p) => Some(p),
                };
                let p = self.patrol();
                p.shooting = ticks;
                p.shoot_at = point;
                p.shoot_spread = inaccuracy;
                p.shot = None;
            }
        }
    }

    fn patrol_finish(&mut self, cx: &mut Ctx) {
        let p = self.patrol();
        if p.current >= p.path.waypoints.len() {
            p.current = 0;
        }
        if matches!(self.goal, Goal::Path(_)) {
            // A script's patrol: back to the job.
            eprintln!("ai: {}: path over", self.name);
            self.return_to_job(true, false, cx);
            return;
        }
        // A job patrol that ends stands idle.
        self.exit(cx);
        self.enter_idle(cx);
        eprintln!("ai: {}: patrol over → idle", self.name);
    }

    /// Moves to the next waypoint. True when the patrol is over (or left).
    fn advance_waypoint(&mut self, cx: &mut Ctx) -> bool {
        let p = self.patrol();
        p.current += 1;
        p.fails = 0;
        p.done_glance = None;
        if p.current >= p.path.waypoints.len() {
            self.patrol_finish(cx);
            return true;
        }
        let i = p.current;
        self.do_waypoint(i, true, false, cx);
        !self.patrolling()
    }

    fn patrol_update(&mut self, cx: &mut Ctx) {
        if self.patrol().pause > 0 {
            self.patrol().pause -= 1;
            if self.patrol().pause > 0 {
                self.halt();
                return;
            }
            let i = self.patrol().current;
            self.do_waypoint(i, false, false, cx);
        }
        if self.patrol().shooting > 0 {
            self.patrol().shooting -= 1;
            self.shoot_at_spot(cx);
        }
        if self.patrol().scanguard > 0 {
            let p = self.patrol();
            p.scanguard -= 1;
            if p.scanguard == 0 {
                p.scanning = false;
                p.guarding = false;
                self.look = Look::None;
                // (A moving scan's end advances whatever waypoint is current
                // by then — the original's behaviour, kept.)
                if self.advance_waypoint(cx) {
                    return;
                }
            }
        }
        if self.path_failed() {
            self.patrol().failed = true;
        }
        let mut skip = false;
        if self.patrol().failed {
            let name = self.name.clone();
            let p = self.patrol();
            p.failed = false;
            p.fails += 1;
            if p.fails >= PATROL_FAILS {
                eprintln!(
                    "ai: {name}: patrol {}: giving up on waypoint {}",
                    p.path.name, p.current
                );
                skip = true;
            } else if let Some(m) = p.movement_waypoint {
                self.do_waypoint(m, true, true, cx);
            }
        }
        let start = self.patrol().current;
        while skip || self.fulfilled(cx) {
            skip = false;
            self.advance_waypoint(cx);
            if !self.patrolling() {
                return;
            }
            if self.patrol().current == start {
                eprintln!("ai: {}: patrol loops endlessly, aborting", self.name);
                self.patrol_finish(cx);
                return;
            }
        }
        if cx.fallen && !self.arrived(cx) {
            self.press |= key::FORWARD;
        }
        let clear = cx.clear_toward();
        let facing = cx.facing;
        let course = self
            .nav
            .as_ref()
            .filter(|n| n.state == NavState::Following)
            .map(|_| facing);
        let p = match (&mut self.goal, &mut self.job) {
            (Goal::Path(p), _) => p,
            (Goal::Job, Job::Patrol(p)) => p,
            _ => return,
        };
        if p.scanning {
            if let Some(c) = course {
                if p.scan_course
                    .is_none_or(|old| relative(c - old).abs() > SCAN_RECALC)
                {
                    p.scan_course = Some(c);
                    p.scan.has_dir = true;
                    p.scan.dir = c;
                    p.scan.look = c + p.scan.offset;
                }
            }
            let look = p.scan.update(facing, &mut self.rng, &clear);
            self.look = Look::Dir(look);
        } else if p.guarding {
            let look = p.scan.update(facing, &mut self.rng, &clear);
            self.look = Look::Dir(look);
        }
    }

    /// Whether the current waypoint is done (and starts glances / scans /
    /// guarding as it gets to them).
    fn fulfilled(&mut self, cx: &mut Ctx) -> bool {
        if self.patrol().pause > 0 || self.patrol().failed {
            return false;
        }
        if self.patrol().return_to.is_some() {
            if !self.arrived(cx) {
                return false;
            }
            self.patrol().return_to = None;
            let i = self.patrol().current;
            self.do_waypoint(i, true, false, cx);
        }
        let i = self.patrol().current;
        let Some(w) = self.patrol().path.waypoints.get(i).cloned() else {
            return true;
        };
        match w {
            Waypoint::MoveTo { face, .. } => {
                if self.arrived(cx) {
                    self.patrol().on_path = true;
                    return true;
                }
                if let Some(d) = self.dest {
                    if !face && d.within > 0.0 && self.distance_to_dest(cx) < d.within {
                        self.patrol().on_path = true;
                        return true;
                    }
                }
                false
            }
            Waypoint::Glance { .. } | Waypoint::Scan { .. } => {
                if self.patrol().done_glance == Some(i) {
                    let glancing = self.glance.is_some();
                    let p = self.patrol();
                    return if p.stationary_glance {
                        !glancing && !p.scanning
                    } else {
                        true
                    };
                }
                if self.glance.is_some() || self.patrol().scanning {
                    return false;
                }
                match w {
                    Waypoint::Glance { at, ticks } => match cx.level.place(at) {
                        Some(p) => self.glance_at(p, ticks, cx),
                        None => return true,
                    },
                    Waypoint::Scan { ticks, angle } => {
                        let facing = cx.facing;
                        let p = self.patrol();
                        p.scan = Scan::reset(Some(facing));
                        p.scanning = true;
                        p.guarding = false;
                        p.scanguard = ticks;
                        p.scan_course = None;
                        p.scan.dir_times = (10000, 0);
                        p.scan.angle_times = SCAN_ANGLE;
                        p.scan.angle_range = angle;
                    }
                    _ => unreachable!(),
                }
                let stopped = self.dest.is_none_or(|d| d.stop) && self.arrived(cx);
                let p = self.patrol();
                p.done_glance = Some(i);
                p.stationary_glance = stopped;
                if stopped {
                    self.halt();
                    return false;
                }
                true
            }
            Waypoint::Guard { ticks, angle, .. } => {
                if !self.arrived(cx) {
                    return false;
                }
                let facing = self.dest.and_then(|d| d.face).unwrap_or(cx.facing);
                let p = self.patrol();
                p.on_path = true;
                if !p.started_guarding {
                    p.scan = Scan::reset(Some(cx.facing));
                    p.scanning = false;
                    p.guarding = true;
                    p.started_guarding = true;
                    p.scanguard = ticks.unwrap_or(0);
                    p.scan.stay_facing = true;
                    p.scan.stay_dir = Some(facing);
                    p.scan.stay_range = angle.clamp(0.0, PI);
                    p.scan.dir_times = GUARD_DIR;
                    p.scan.angle_times = GUARD_ANGLE;
                    p.scan.angle_range = GUARD_ANGLE_RANGE;
                }
                !p.guarding
            }
            Waypoint::ShootAt { .. } => self.patrol().shooting == 0,
            Waypoint::Script { wait: true, .. } => !self.waiting_script,
            _ => true,
        }
    }

    // --- pursuit (`docs/GOALS.md`, *Pursuit*) ---------------------------------

    fn enter_pursuit(&mut self, id: u32, cx: &mut Ctx) {
        let Some(c) = cx.perception.contact(id).cloned() else {
            return;
        };
        self.exit(cx);
        let danger = cx.alert() >= Alert::Medium;
        let mut p = Pursuit {
            target: id,
            mode: PursuitMode::Glance,
            delay: 0,
            certain: c.highest >= Strength::Definite,
            danger,
            last_los_check: 0,
            los_ok: 0,
            los_seen: false,
            close: false,
            running: false,
            scanning: false,
            scan: Scan::default(),
            say_timer: 120,
        };
        if danger && self.desire_pursuit(&p, Check::Initial, cx) {
            p.mode = PursuitMode::GoTo;
        } else {
            p.mode = PursuitMode::Glance;
            p.delay = c.stimulus.glance();
        }
        self.goal = Goal::Pursuit(Box::new(p));
        let mode = self.pursuit().mode;
        eprintln!("ai: {} → pursuit ({})", self.name, mode.name());
        self.change_mode(cx);
        self.begin_mode(cx);
    }

    fn pursuit(&mut self) -> &mut Pursuit {
        match &mut self.goal {
            Goal::Pursuit(p) => p,
            _ => unreachable!("not pursuing"),
        }
    }

    fn desire_pursuit(&self, p: &Pursuit, check: Check, cx: &Ctx) -> bool {
        let Some(c) = cx.perception.contact(p.target) else {
            return false;
        };
        if check != Check::Initial && c.priority == Priority::Friendly {
            return false;
        }
        if cx.alert() < self.investigate_alert {
            return false;
        }
        let limit = self.profile.pursuit_distance;
        if limit == 0.0 {
            return true;
        }
        match check {
            Check::PostArrival | Check::ChangeTarget => match self.job_location {
                // No way back counts as close enough. (Without a job
                // location the original reads an unset flag: pursue.)
                Some(home) => (cx.path_length)(cx.feet, home).is_none_or(|d| d < limit),
                None => true,
            },
            _ => (cx.path_length)(cx.feet, c.location).is_some_and(|d| d < limit),
        }
    }

    /// Picks the mode the contact's strength calls for. True if it changed.
    fn change_mode(&mut self, cx: &mut Ctx) -> bool {
        let alerted = cx.alert() >= Alert::Medium;
        let settings = self.pursue.clone();
        let investigate = self.class.investigate_body;
        let strength = cx
            .perception
            .contact(self.pursuit().target)
            .map_or(Strength::Forgotten, |c| c.strength);
        let p = self.pursuit();
        if p.delay > 0 {
            return false;
        }
        let mut force = false;
        let new = if p.mode == PursuitMode::Forget {
            force = true;
            PursuitMode::Forget
        } else {
            match strength {
                Strength::Definite | Strength::Strong => {
                    if strength == Strength::Definite {
                        p.certain = true;
                    }
                    PursuitMode::from_name(if alerted {
                        &settings.strong_seen
                    } else {
                        &settings.strong_unseen
                    })
                }
                Strength::Weak => PursuitMode::from_name(if alerted {
                    &settings.weak_seen
                } else {
                    &settings.weak_unseen
                }),
                Strength::Forgotten => {
                    if settings.lost == "keepLooking" {
                        PursuitMode::None
                    } else {
                        force = true;
                        PursuitMode::Forget
                    }
                }
                Strength::Dead => {
                    p.delay = investigate;
                    return true;
                }
            }
        };
        if new == PursuitMode::None || (new != PursuitMode::Forget && new == p.mode) {
            return false;
        }
        if p.mode == PursuitMode::GoTo && !force {
            return false; // don't interrupt going there
        }
        p.mode = new;
        true
    }

    fn begin_mode(&mut self, cx: &mut Ctx) {
        let Some(c) = cx.perception.contact(self.pursuit().target).cloned() else {
            self.return_to_job(true, true, cx);
            return;
        };
        self.halt();
        self.look = Look::None;
        let mode = self.pursuit().mode;
        match mode {
            PursuitMode::Forget => {
                // Told to raise the alarm when it loses someone it was sure of.
                if self.pursue.lost == "alarm" && self.pursuit().certain {
                    let console = self.alarm_console.or_else(|| self.find_alarm(cx));
                    if console.is_some() && self.alarm_setup(console, cx) {
                        return;
                    }
                }
                eprintln!("ai: {}: pursuit over", self.name);
                self.return_to_job(true, true, cx);
            }
            PursuitMode::GoTo => {
                let p = self.pursuit();
                p.last_los_check = 0;
                p.los_ok = 0;
                p.los_seen = false;
                p.running = p.danger;
                p.scanning = false;
                self.go_to(c.location, PURSUIT_CLOSE_DIST, true, None, cx);
            }
            PursuitMode::Look => {
                let to = yaw_toward(cx.feet, c.location);
                let p = self.pursuit();
                if !p.scanning {
                    p.scan = Scan::reset(Some(to));
                    p.scanning = true;
                }
                p.scan.stay_facing = false;
                p.scan.dir_times = PURSUIT_LOOK_DIR;
                p.scan.angle_times = PURSUIT_LOOK_ANGLE;
                p.scan.angle_range = PURSUIT_LOOK_RANGE;
            }
            PursuitMode::Glance => {
                let to = yaw_toward(cx.feet, c.location);
                let p = self.pursuit();
                p.scanning = false;
                p.scan = Scan::reset(Some(to));
                p.scan.dir_times = (0, 0);
                p.scan.angle_times = (PURSUIT_LOOK_ANGLE.0, PURSUIT_LOOK_ANGLE.1);
                p.scan.angle_range = PURSUIT_LOOK_RANGE;
                p.scan.dir_timer = 10000;
                p.scan.has_dir = true;
                p.scan.dir = to;
                p.scan.look = to;
                p.scan.angle_timer = PURSUIT_GLANCE_FIRST;
                p.scan.offset = 0.0;
                self.look = Look::Dir(to);
            }
            // Hunting and moving were never implemented: they stand.
            PursuitMode::Hunt | PursuitMode::Move | PursuitMode::Wait | PursuitMode::None => {}
        }
    }

    fn next_mode(&mut self, cx: &mut Ctx) {
        let prev = self.pursuit().mode;
        let friendly_seen = cx
            .perception
            .contact(self.pursuit().target)
            .is_some_and(|c| c.priority == Priority::Friendly && c.strength == Strength::Definite);
        match prev {
            PursuitMode::GoTo => {
                let p = self.pursuit();
                if friendly_seen {
                    p.mode = PursuitMode::Glance;
                    p.delay = PURSUIT_GLANCE_FIRST;
                } else {
                    p.mode = PursuitMode::Look;
                    p.delay = PURSUIT_LOOK_AFTER_ARRIVING;
                }
            }
            PursuitMode::Glance => {
                let p = self.pursuit().clone();
                let go = self.desire_pursuit(&p, Check::PostGlance, cx);
                self.pursuit().mode = if go {
                    PursuitMode::GoTo
                } else {
                    PursuitMode::Forget
                };
            }
            PursuitMode::Look => {
                let p = self.pursuit().clone();
                if !self.desire_pursuit(&p, Check::PostArrival, cx) {
                    self.pursuit().mode = PursuitMode::Forget;
                }
                self.pursuit().delay = 0;
                self.change_mode(cx);
            }
            _ => {
                let p = self.pursuit();
                p.delay = 0;
                p.mode = PursuitMode::Wait;
            }
        }
        if !matches!(self.goal, Goal::Pursuit(_)) {
            return;
        }
        if self.pursuit().mode != prev {
            let mode = self.pursuit().mode;
            eprintln!(
                "ai: {}: pursuit {} → {}",
                self.name,
                prev.name(),
                mode.name()
            );
            self.begin_mode(cx);
        }
    }

    fn pursuit_update(&mut self, cx: &mut Ctx) {
        if cx.fallen {
            self.press |= key::FORWARD;
        }
        if self.pursuit().delay > 0 {
            self.pursuit().delay -= 1;
            if self.pursuit().delay == 0 {
                self.next_mode(cx);
            }
        }
        if !matches!(self.goal, Goal::Pursuit(_)) {
            return;
        }
        // Actively pursuing, now and then it says so.
        let next = 720 + self.rng.next() % 180;
        let p = self.pursuit();
        if matches!(
            p.mode,
            PursuitMode::Look | PursuitMode::Hunt | PursuitMode::Move | PursuitMode::Wait
        ) && p.say_timer > 0
        {
            p.say_timer -= 1;
            if p.say_timer == 0 {
                p.say_timer = next;
                cx.perception.vocals.push(crate::world::vocal::PURSUE);
            }
        }
        let facing = cx.facing;
        match self.pursuit().mode {
            PursuitMode::GoTo => self.goto_update(cx),
            PursuitMode::Look | PursuitMode::Glance => {
                let clear = cx.clear_toward();
                let Goal::Pursuit(p) = &mut self.goal else {
                    return;
                };
                let look = p.scan.update(facing, &mut self.rng, &clear);
                self.look = Look::Dir(look);
            }
            _ => {}
        }
    }

    fn goto_update(&mut self, cx: &mut Ctx) {
        let target = self.pursuit().target;
        cx.perception.hold(target);
        let Some(location) = cx.perception.contact(target).map(|c| c.location) else {
            return;
        };
        let dist = self.distance_to_dest(cx);
        let mut finished = false;
        let mut scan = false;
        if self.arrived(cx) || dist < PURSUIT_CLOSE_DIST {
            finished = true;
        } else if dist > PURSUIT_SCAN_DIST {
            let p = self.pursuit();
            p.running = p.danger;
        } else {
            // Can it see the place it is heading for? (The original meant to
            // check every 15 ticks but never moves its timer on: it checks
            // every tick.)
            let every = cx.quirks.investigate_check_every();
            let now = cx.now;
            let due = now >= self.pursuit().last_los_check + every;
            let los = if due {
                let seen = (cx.sight)(cx.eye, location + Vec3::Y * 5.0, SEE);
                let p = self.pursuit();
                p.last_los_check = now;
                p.los_seen = seen;
                if seen && dist < PURSUIT_STOP_DIST {
                    p.los_ok += 1;
                } else if !seen {
                    p.los_ok = 0;
                }
                seen
            } else {
                self.pursuit().los_seen
            };
            let p = self.pursuit();
            if los {
                scan = true;
                if dist < PURSUIT_STOP_DIST {
                    if p.los_ok >= PURSUIT_LOS_CHECKS {
                        finished = true;
                    }
                } else {
                    p.running = !p.close && p.danger;
                }
            }
        }
        if finished {
            let p = self.pursuit();
            p.close = true;
            p.running = false;
            p.scanning = false;
            self.next_mode(cx);
            if !matches!(self.goal, Goal::Pursuit(_)) {
                return;
            }
        }
        if self.pursuit().mode != PursuitMode::GoTo {
            return;
        }
        if scan {
            let facing = cx.facing;
            let clear = cx.clear_toward();
            let Goal::Pursuit(p) = &mut self.goal else {
                return;
            };
            if !p.scanning {
                p.scan = Scan::reset(Some(yaw_toward(cx.feet, location)));
                p.scanning = true;
                p.scan.stay_facing = true;
                p.scan.stay_dir = Some(facing);
                p.scan.stay_range = PURSUIT_SCAN_RANGE;
                p.scan.dir_timer = 0;
                p.scan.dir_times = PURSUIT_SCAN;
                p.scan.angle_range = 0.0;
                p.scan.angle_timer = 0;
            }
            let look = p.scan.update(facing, &mut self.rng, &clear);
            self.look = Look::Dir(look);
        } else {
            self.pursuit().scanning = false;
            self.look = Look::None;
        }
    }

    fn pursuit_knowledge(&mut self, id: u32, degrade: bool, cx: &mut Ctx) {
        let alerted = cx.alert() >= Alert::Medium;
        self.pursuit().danger = alerted;
        let target = self.pursuit().target;
        if degrade && id == target && self.change_mode(cx) {
            self.begin_mode(cx);
            if !matches!(self.goal, Goal::Pursuit(_)) {
                return;
            }
        }
        let best = cx.perception.contacts.first().cloned();
        let investigate = match best {
            None => true,
            Some(b) if b.id == self.pursuit().target => !degrade,
            Some(b) if b.priority >= Priority::HostileNoThreat => {
                self.pursuit().target = b.id;
                true
            }
            _ => false,
        };
        if !investigate {
            return;
        }
        let p = self.pursuit().clone();
        if !self.desire_pursuit(&p, Check::ChangeTarget, cx) {
            return;
        }
        self.pursuit().mode = PursuitMode::GoTo;
        self.begin_mode(cx);
    }

    // --- combat (`docs/GOALS.md`, *Combat*) -----------------------------------

    fn enter_combat(&mut self, who: usize, cx: &mut Ctx) {
        let Some(c) = cx.perception.about(who).cloned() else {
            return;
        };
        self.exit(cx);
        self.mode = MoveMode::ByAlert;
        let melee = cx
            .melee_profile
            .clone()
            .map(|p| Box::new(Melee::new(p, who, cx.daze, cx.me as u32 * 7919 + 1)));
        let mut combat = Combat {
            target: who,
            contact: c.id,
            target_strength: c.strength,
            melee,
            primary: Primary::Hold,
            band: Band::None,
            advance: (15.0, 20.0),
            retreat: (100.0, 1000.0),
            try_advance: 0,
            lost_contact: None,
            target_dead: false,
            investigate: 0,
            taunt_enable: false,
            done_taunt: false,
            last_hit: 0,
            last_melee_hit: 0,
            daze: None,
            advance_goal: None,
            route_failed: None,
            advance_way: None,
            targeting: None,
            have_los: true,
            first_sight: true,
            too_close: 0.0,
            gun_target: None,
            running_roll: None,
            gun_running: false,
            gun_roll: None,
            unreachable: Vec::new(),
            weapon_melee: 0,
            dodges: Vec::new(),
            dodge: None,
            boss: Default::default(),
            gun_search: None,
        };
        set_new_target(&mut combat, &c);
        self.goal = Goal::Combat(Box::new(combat));
        eprintln!(
            "ai: {} → combat with {who} (profile {})",
            self.name, self.profile.name
        );
    }

    pub(super) fn combat(&mut self) -> &mut Combat {
        match &mut self.goal {
            Goal::Combat(c) => c,
            _ => unreachable!("not fighting"),
        }
    }

    pub(super) fn combat_ref(&self) -> Option<&Combat> {
        match &self.goal {
            Goal::Combat(c) => Some(c),
            _ => None,
        }
    }

    fn target_died(&mut self, cx: &mut Ctx) {
        // The original's search for another target always finds none (a
        // bug); fixed, it takes up the best other live threat.
        if !cx.quirks.no_new_target() {
            let me = cx.me;
            let target = self.combat().target;
            let other = cx
                .perception
                .contacts
                .iter()
                .find(|c| {
                    c.who.is_some_and(|w| {
                        w != target && w != me && cx.others.get(w).is_some_and(|o| o.alive)
                    }) && c.priority >= Priority::HostileNoThreat
                        && c.strength >= Strength::Strong
                })
                .cloned();
            if let Some(o) = other {
                let who = o.who.expect("someone");
                if o.strength == Strength::Definite {
                    let c = self.combat();
                    c.target = who;
                    if let Some(m) = c.melee.as_mut() {
                        m.target = who;
                    }
                    set_new_target(c, &o);
                } else {
                    self.exit(cx);
                    self.enter_pursuit(o.id, cx);
                }
                return;
            }
        }
        let investigate = self.class.investigate_body;
        let c = self.combat();
        c.target_dead = true;
        c.investigate = investigate;
        c.taunt_enable = c.primary == Primary::Melee;
        cx.fights.leave(cx.me);
        eprintln!("ai: {}: target dead", self.name);
    }

    fn combat_knowledge(
        &mut self,
        id: u32,
        degrade: bool,
        event: Option<(Stimulus, i32)>,
        cx: &mut Ctx,
    ) {
        let Some(mut c) = cx.perception.contact(id).cloned() else {
            return;
        };
        // (What this news was, not what the contact last heard.)
        let damage = event.map_or(0, |e| e.1);
        if let Some((stimulus, _)) = event {
            c.stimulus = stimulus;
        }
        let (target, contact) = (self.combat().target, self.combat().contact);
        if degrade {
            if c.who != Some(target) || id != contact {
                return;
            }
            if c.strength == Strength::Dead {
                if !self.combat().target_dead {
                    self.target_died(cx);
                }
            } else if c.strength < Strength::Definite {
                if self.combat().target_strength == Strength::Definite {
                    // Lost sight of an invisible one: lost at once
                    // (`AI2iCombat_TargetKnowledgeChanged`).
                    if cx.others.get(target).is_some_and(|o| o.invisible) {
                        let c = self.combat();
                        c.lost_contact = Some(0);
                        c.target_strength = Strength::Strong;
                        return;
                    }
                    // Lost sight of it: remember where it really is, and
                    // after a while pursue.
                    if let Some(o) = cx.others.get(target) {
                        cx.perception.set_location(id, o.feet);
                    }
                    let delay = self.class.lost_contact;
                    let c = self.combat();
                    c.lost_contact = Some(delay);
                    c.target_strength = Strength::Strong;
                }
            } else {
                self.combat().lost_contact = None;
            }
            return;
        }
        let now = cx.now;
        // Hurt while fighting on the way to an alarm: the fight goes on.
        if matches!(c.stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) && self.fight_timer > 0 {
            self.fight_timer = self.profile.alarm.fight_ticks;
        }
        // Hurt, a boss reacts (`AI2cCombatMessage_Hurt`).
        if matches!(c.stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) {
            if self.boss_mode().is_some() {
                self.barabbas_hurt(damage.max(0) as u32, cx);
            } else if self.ninja_mode().is_some() {
                self.ninja_hurt(damage.max(0) as u32);
            } else if self.is_muro() {
                self.muro_hurt(damage.max(0) as u32);
            }
        }
        if c.who == Some(target) && matches!(c.stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) {
            let k = self.combat();
            k.last_hit = now;
            if c.stimulus == Stimulus::HitMelee {
                k.last_melee_hit = now;
            }
        }
        if id == contact {
            let k = self.combat();
            k.target_strength = c.strength;
            if c.strength == Strength::Definite {
                k.lost_contact = None;
            }
            return;
        }
        // Someone else: switch to them?
        if c.priority == Priority::Friendly {
            return;
        }
        let Some(who) = c.who.filter(|&w| cx.others.get(w).is_some_and(|o| o.alive)) else {
            return;
        };
        if who == cx.me {
            return;
        }
        let current = cx.perception.contact(contact).cloned();
        let mut change = false;
        if let Some(cur) = &current {
            match cx.perception.compare(&c, cur) {
                std::cmp::Ordering::Less => change = true, // ranks ahead
                std::cmp::Ordering::Greater => return,
                std::cmp::Ordering::Equal => {}
            }
            if c.strength == Strength::Definite {
                let d_new = cx.feet.distance_squared(c.location);
                let d_old = cx.feet.distance_squared(cur.location);
                if d_new < SWITCH_CLOSER * SWITCH_CLOSER * d_old {
                    change = true;
                } else if d_new > DONT_SWITCH_FARTHER * DONT_SWITCH_FARTHER * d_old {
                    change = false;
                }
            }
        } else {
            change = true;
        }
        let last_hit = self.combat().last_hit;
        match c.stimulus {
            Stimulus::HitWeapon if c.time > last_hit + SWITCH_IF_NOT_SHOT => change = true,
            Stimulus::HitMelee if c.time > last_hit + SWITCH_IF_NOT_PUNCHED => change = true,
            _ => {}
        }
        if change {
            eprintln!("ai: {}: switches target {target} → {who}", self.name);
            let k = self.combat();
            k.target = who;
            if let Some(m) = k.melee.as_mut() {
                m.target = who;
            }
            cx.fights.leave(cx.me);
            set_new_target(k, &c);
        }
    }

    fn combat_update(&mut self, cx: &mut Ctx) {
        // The target.
        let target = self.combat().target;
        let alive = cx.others.get(target).is_some_and(|o| o.alive);
        if !self.combat().target_dead && !alive {
            self.target_died(cx);
            if !matches!(self.goal, Goal::Combat(_)) {
                return;
            }
        }
        let target = self.combat().target;
        // (Its body gone — deleted, or a corpse now: nothing to look at.)
        if self.combat().target_dead && cx.others.get(target).is_none_or(|o| o.gone) {
            self.return_to_job(true, true, cx);
            return;
        }
        let target_feet = cx.others.get(target).map_or(cx.feet, |o| o.feet);
        // A super ninja: his clips' news; a fall takes his update over.
        if self.ninja_mode().is_some() {
            self.ninja_tick(cx);
            if self.ninja_fell(cx) {
                return;
            }
        }
        if self.is_muro() {
            self.muro_tick(cx);
            let charging = self.muro_shield_charging();
            if let Some(m) = self.combat().melee.as_mut() {
                m.shield_charging = charging;
            }
        }
        if self.combat().target_dead {
            let (half, chance) = (self.class.investigate_body / 2, self.class.taunt_percent);
            let roll = self.rng.next() % 100;
            let standing = cx.standing;
            let k = self.combat();
            let mut taunt = false;
            if k.primary != Primary::Advance && k.investigate > 0 {
                k.investigate -= 1;
                if k.taunt_enable && !k.done_taunt && standing && k.investigate < half {
                    k.done_taunt = true;
                    k.taunt_enable = false;
                    taunt = roll < chance;
                    // In any case, its check-body line.
                    cx.perception.vocals.push(crate::world::vocal::CHECK_BODY);
                }
            }
            if taunt {
                eprintln!("ai: {}: taunts", self.name);
                self.press |= key::ACTION;
            }
            if self.combat().investigate == 0 {
                self.return_to_job(true, true, cx);
                return;
            }
        } else {
            // In combat it knows where its target is.
            let contact = self.combat().contact;
            cx.perception.set_location(contact, target_feet);
            // SHIPPED BALANCE (Q11, kept in both modes): losing sight of its
            // target arms the class's lost-contact delay, but a debug `if (1)`
            // in `AI2iCombat_CheckTarget` pursues on the very next tick
            // anyway. The game's fights — and the level 1 training's combat
            // demo, which stalls without it (#62) — rely on that.
            if self.combat().lost_contact.is_some() {
                self.exit(cx);
                self.enter_pursuit(contact, cx);
                return;
            }
        }

        // Going for an alarm meanwhile: a target close enough keeps it
        // fighting; one far enough off sends it back to the alarm.
        if self.alarm_console.is_some() && !self.combat().target_dead {
            let d = (target_feet - cx.feet).length();
            let a = self.profile.alarm;
            if d < a.chase_distance {
                self.fight_timer = a.fight_ticks;
            } else if d > a.ignore_distance {
                self.alarm_setup(None, cx);
                return;
            }
        }

        // Every 5 ticks: choose a primary movement.
        if (cx.me as u64 + cx.now).is_multiple_of(COMBAT_PERIOD) || self.combat().band == Band::None
        {
            self.choose_primary(target_feet, cx);
        }

        // Carry it out.
        let primary = self.combat().primary;
        if primary != Primary::Melee {
            cx.fights.leave(cx.me);
        }
        let dist = (target_feet - cx.feet).with_y_zero().length();
        self.look = Look::At(target_feet);
        // Dodging: a step away from the danger takes over from holding,
        // advancing, retreating or going for a gun (not from melee).
        let dodge = self
            .combat()
            .dodge
            .filter(|_| primary != Primary::Melee && primary != Primary::GetUp && !cx.fallen);
        if let Some((heading, _)) = dodge {
            let to = cx.feet + heading * DODGE_STEP;
            let fresh = self.dest.as_ref().is_none_or(|d| d.at.distance(to) > 6.0);
            if fresh {
                self.go_to(to, 3.0, true, None, cx);
            }
        }
        match primary {
            _ if dodge.is_some() => {}
            Primary::Melee => {
                self.halt();
                let k = self.combat();
                if let Some(m) = k.melee.as_mut() {
                    if m.no_technique() >= MELEE_NO_TECHNIQUE {
                        m.reset_no_technique();
                        k.try_advance = TRY_ADVANCE;
                    }
                }
            }
            Primary::Advance => {
                let (min, _) = self.combat().advance;
                match self.combat().advance_way {
                    // (Toward where the target is now, along the way the
                    // check found clear.)
                    // (Not on into the target before the next check.)
                    Some(AdvanceWay::Steer(_)) if dist <= AI2_ADVANCE_STOP => self.halt(),
                    Some(AdvanceWay::Steer(heading)) => {
                        self.nav = None;
                        self.dest = None;
                        self.steer = Some((heading, self.mode.pace(cx.alert(), true)));
                    }
                    _ => self.advance_move(target_feet, min.max(AI2_ADVANCE_STOP), cx),
                }
            }
            Primary::Retreat => {
                let away = (cx.feet - target_feet).with_y_zero().normalize_or_zero();
                let to = cx.feet + away * 30.0;
                if self.nav.is_none() || self.path_failed() || self.arrived(cx) {
                    self.go_to(to, 3.0, true, None, cx);
                }
            }
            Primary::Hold => self.halt(),
            Primary::FindAlarm => self.halt(),
            Primary::GetUp => {
                self.halt();
                self.press |= key::FORWARD;
            }
            Primary::Gun => {
                let k = self.combat();
                let gun = k
                    .gun_target
                    .and_then(|g| cx.guns.iter().find(|f| f.index == g))
                    .copied();
                // Rolling or sliding for it: no new clip, just aim at it
                // (and take it when it is near).
                if self.combat().gun_running && cx.body.can_pickup {
                    self.halt();
                    if let Some(f) = gun {
                        if f.at.with_y_zero().distance(cx.feet.with_y_zero()) < 16.0 {
                            self.want_pickup = Some(f.index);
                        }
                    }
                } else {
                    match gun {
                        Some(f)
                            if f.at.with_y_zero().distance(cx.feet.with_y_zero()) <= GUN_REACH =>
                        {
                            self.halt();
                            self.want_pickup = Some(f.index);
                        }
                        Some(f) => {
                            if self.path_failed() {
                                // Out of reach: remember it, find another.
                                let k = self.combat();
                                k.unreachable.push(f.index);
                                k.gun_target = None;
                            } else if self.nav.is_none() || self.arrived(cx) {
                                self.go_to(f.at, 8.0, true, None, cx);
                            }
                        }
                        None => self.halt(),
                    }
                }
            }
        }
        // Knocked down, a boss reacts (`AI2cCombatMessage_Fallen`).
        if cx.fallen && self.boss_mode().is_some() {
            self.barabbas_fallen(cx);
        }
        // Lying down outside melee: dazed a while, then get up.
        if primary != Primary::Melee {
            let (lo, hi) = cx.daze;
            let roll = lo + self.rng.next() % hi.saturating_sub(lo).max(1);
            let fallen = cx.fallen;
            let k = self.combat();
            k.daze = match (fallen, k.daze) {
                (false, _) => None,
                (true, None) => Some(roll),
                (true, Some(n)) => Some(n.saturating_sub(1)),
            };
        }
        self.shoot(target, cx);
    }

    /// The dangers it sees (each decision; `docs/AI.md`, *Dodging*): guns
    /// firing toward it and projectiles about to reach it. Each lasts a
    /// while and fades; it reacts once one has lasted its class's reaction
    /// time. The way to go is the dodges' weighted sum.
    fn find_dodges(&mut self, cx: &mut Ctx) {
        let class = self.class;
        let firing = cx.gun.is_some_and(|g| g.firing);
        let looking =
            class.dodge && (!firing || class.dodge_while_firing || class.dodge_stops_firing);
        let pelvis = cx.pelvis;
        let mut found: Vec<(Dodge, bool)> = Vec::new();
        if looking {
            for s in cx.spreads.iter().filter(|s| s.owner != cx.me) {
                let rel = pelvis - s.origin;
                let along = rel.dot(s.axis);
                if along <= 0.0 || along > s.length {
                    continue;
                }
                let radius = s.base + along * s.angle.tan() + BODY_RADIUS;
                let away = rel - s.axis * along;
                let off = away.length();
                let mut weight = if off < 0.6 * radius {
                    0.8
                } else if off < radius {
                    0.8 - 0.5 * (off - 0.6 * radius) / (0.4 * radius)
                } else if off < 1.2 * radius {
                    0.3
                } else {
                    continue;
                };
                if !(cx.sight)(s.origin, pelvis, SHOOT) {
                    continue;
                }
                weight -= 1.0 - class.dodge_weight_scale;
                if weight <= 0.0 {
                    continue;
                }
                let (heading, direct) = dodge_heading(-away, s.axis, pelvis, &mut self.rng, cx);
                let lifetime = (20.0 * class.dodge_time_scale).round() as u32;
                found.push((
                    Dodge {
                        key: s.owner as u64,
                        weight,
                        lifetime,
                        left: lifetime,
                        age: 0,
                        heading,
                    },
                    direct,
                ));
            }
            for p in cx.projectiles {
                // Its next 50 ticks against the body.
                let path = p.velocity * (50.0 / 60.0);
                let reach = BODY_RADIUS + p.radius;
                let l2 = path.length_squared();
                let t = if l2 > 0.0 {
                    ((pelvis - p.at).dot(path) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let closest = p.at + path * t;
                if closest.distance(pelvis) > reach {
                    continue;
                }
                // The danger, from the body toward the trajectory
                // (`AI2rManeuver_NotifyNearbyProjectile`'s distance and
                // direction-to-trajectory). Shipped, both are the closest
                // point's own, from the world's origin, so the weight is almost
                // always far below zero and projectiles go undodged. The
                // Daodan DLL's `projaware` (on in the Anniversary Edition) means
                // to pass the body-relative point but, its stack offsets off,
                // passes memory that reads as (0, 0, 0): every projectile on
                // course is a direct hit at full weight, and the AI sidesteps
                // it. Q24 follows the Daodan.
                let danger = if cx.quirks.projectiles_from_origin() {
                    closest
                } else {
                    Vec3::ZERO
                };
                let mut weight = ((p.radius - danger.length()) / 10.0).min(0.8);
                if weight < -0.3 {
                    continue;
                }
                weight = weight.max(0.3) - (1.0 - class.dodge_weight_scale);
                if weight <= 0.0 || !(cx.sight)(p.at, pelvis, SHOOT) {
                    continue;
                }
                let lifetime = (45.0 * class.dodge_time_scale).round() as u32;
                let (heading, direct) = dodge_heading(danger, path, pelvis, &mut self.rng, cx);
                found.push((
                    Dodge {
                        key: p.id | (1 << 63),
                        weight,
                        lifetime,
                        left: lifetime,
                        age: 0,
                        heading,
                    },
                    direct,
                ));
            }
        }
        let k = self.combat();
        // Age what it knew; refresh or add what it found (at most eight).
        for d in &mut k.dodges {
            d.age += COMBAT_PERIOD as u32;
            d.left = d.left.saturating_sub(COMBAT_PERIOD as u32);
        }
        k.dodges.retain(|d| d.left > 0);
        for (f, direct) in found {
            if let Some(d) = k.dodges.iter_mut().find(|d| d.key == f.key) {
                // A dodge under way keeps its heading on a direct hit, or
                // while it still leads away (`AI2iManeuver_MakeDodgeFromDanger`).
                if !direct && d.heading.dot(f.heading) <= 0.0 {
                    d.heading = f.heading;
                }
                d.weight = f.weight;
                d.left = f.lifetime;
                d.lifetime = f.lifetime;
            } else if k.dodges.len() < 8 {
                k.dodges.push(f);
            }
        }
        let mut sum = Vec3::ZERO;
        let mut max = 0.0f32;
        for d in k.dodges.iter().filter(|d| d.age >= class.dodge_react) {
            let w = d.weight * d.left as f32 / d.lifetime.max(1) as f32;
            sum += d.heading * w;
            max = max.max(w);
        }
        k.dodge = (max > 0.0).then(|| (sum.normalize_or(Vec3::X), max));
    }

    /// The gun this tick (`docs/AI.md`, *Shooting*): lead the target, aim
    /// the barrel (the shot's line through the aim point, with the skill's
    /// error), and hold the trigger while the gun points close enough.
    fn shoot(&mut self, target: usize, cx: &mut Ctx) {
        self.aim = None;
        self.trigger = false;
        let behavior = {
            let band = self.combat().band;
            self.profile.behavior[band_index(band)].clone()
        };
        let Some(g) = cx.gun.filter(|g| g.usable) else {
            self.combat().targeting = None;
            return;
        };
        // A boss's own targeting: whether it aims, fires, and which trigger.
        let boss = self.barabbas_targeting();
        if boss.is_some_and(|b| !b.aim) {
            self.alt_fire = false;
            return;
        }
        let stops_firing = self.class.dodge_stops_firing;
        let k = self.combat();
        let melee_kind = matches!(
            behavior.as_str(),
            "melee" | "barabbasMelee" | "superNinjaMelee" | "mutantMuroMelee"
        );
        if k.target_dead || k.primary == Primary::Melee || melee_kind || cx.fallen {
            return;
        }
        let Some(t) = cx.others.get(target) else {
            return;
        };
        let (at, shoulder) = (t.pelvis, cx.pelvis);
        let first = std::mem::replace(&mut k.first_sight, false);
        let targeting = k.targeting.get_or_insert_with(|| {
            Box::new(crate::ai::ranged::Targeting::new(
                g.params, g.ai.aim, g.skill, first, at, shoulder,
            ))
        });
        targeting.predict(at, shoulder);
        // (Staring, it looks with the gun lowered and never fires.)
        let shooting = behavior != "stare" && boss.is_none_or(|b| b.shoot);
        let dir = targeting.aim(shoulder, g.matrix, shooting);
        // (Backing off hard — or dodging, for a class whose dodges stop its
        // firing — it doesn't fire.)
        let dodging = k.dodge.is_some() && stops_firing;
        let backing = (k.primary == Primary::Retreat && k.too_close > 0.2) || dodging;
        let fire = targeting.execute(shooting && k.have_los && !backing, g.matrix);
        let speed = targeting.aim_speed;
        if shooting || boss.is_some() {
            self.aim = Some(dir);
        }
        self.aim_speed = speed;
        self.trigger = fire;
        // (A boss hears that it pulled the trigger, `AI2cCombatMessage_Fire`.)
        if let Some(b) = boss {
            self.alt_fire = b.alternate;
            self.barabbas_fired(&behavior, fire, b.alternate);
        }
    }

    /// A patrol's shoot-at-flag: aim at the spot and fire as targeting
    /// allows (no startle, nothing to lead), for the waypoint's time.
    fn shoot_at_spot(&mut self, cx: &mut Ctx) {
        let shoulder = cx.pelvis;
        let p = self.patrol();
        let (Some(at), Some(g)) = (p.shoot_at, cx.gun.filter(|g| g.usable)) else {
            return;
        };
        if p.shooting == 0 {
            p.shot = None;
            return;
        }
        let t = p.shot.get_or_insert_with(|| {
            Box::new(crate::ai::ranged::Targeting::new(
                g.params, g.ai.aim, g.skill, false, at, shoulder,
            ))
        });
        t.predict(at, shoulder);
        let dir = t.aim(shoulder, g.matrix, true);
        let fire = t.execute(true, g.matrix);
        let (speed, spread) = (t.aim_speed, p.shoot_spread);
        self.aim = Some(dir);
        self.aim_speed = speed;
        self.trigger = fire;
        self.extra_spread = spread;
    }

    /// The way it is dodging now and how strongly (none: not dodging).
    pub fn dodging(&self) -> Option<(Vec3, f32)> {
        match &self.goal {
            Goal::Combat(k) => k.dodge,
            _ => None,
        }
    }

    /// How many of the dangers it is dodging are projectiles (not guns'
    /// firing cones).
    pub fn projectile_dodges(&self) -> usize {
        match &self.goal {
            Goal::Combat(k) => k.dodges.iter().filter(|d| d.key >> 63 == 1).count(),
            _ => 0,
        }
    }

    /// Its gun fired a shot: targeting moves on (new error, the pause).
    pub fn fired(&mut self) {
        let mut rng = std::mem::replace(&mut self.rng, Rng(1));
        if let Goal::Combat(k) = &mut self.goal {
            if let Some(t) = k.targeting.as_mut() {
                t.fired(&mut rng);
            }
        }
        let patrol = match (&mut self.goal, &mut self.job) {
            (Goal::Path(p), _) => Some(p),
            (_, Job::Patrol(p)) => Some(p),
            _ => None,
        };
        if let Some(t) = patrol.and_then(|p| p.shot.as_mut()) {
            t.fired(&mut rng);
        }
        self.rng = rng;
    }

    /// The combat layer's decision (every 5 ticks): the range band, the
    /// profile's behaviour for it, and the highest viable primary movement.
    fn choose_primary(&mut self, target_feet: Vec3, cx: &mut Ctx) {
        use super::bosses::pm::*;
        let profile = self.profile.clone();
        let alarm = self.find_alarm(cx);
        let dist = (target_feet - cx.feet).with_y_zero().length();
        let now = cx.now;
        let has_melee = self.combat().melee.is_some();
        // (Mutant Muro held in his thunderbolt keeps its band:
        // `AI2cCombatMessage_SwitchBehavior`.)
        let band = self.combat().band;
        let locked = band != Band::None && self.muro_locked(&profile.behavior[band_index(band)]);
        let k = self.combat();
        k.try_advance = k.try_advance.saturating_sub(COMBAT_PERIOD as u32);
        let mut w = [0.0f32; 7];

        if k.target_dead {
            // Go and look at the body.
            w[ADVANCE] = 1.2;
            w[HOLD] = 0.8;
            k.advance = (12.0, 30.0);
        } else {
            // The range band (a retreat band if it was closer before).
            let prev = k.band;
            let closer_before = |p: Band| {
                matches!(
                    p,
                    Band::Short | Band::MediumRetreat | Band::LongRetreat | Band::Medium
                )
            };
            k.band = if dist < profile.short_range {
                Band::Short
            } else if dist < profile.medium_range {
                if matches!(prev, Band::Short | Band::MediumRetreat | Band::LongRetreat) {
                    Band::MediumRetreat
                } else {
                    Band::Medium
                }
            } else if prev != Band::None && closer_before(prev) {
                Band::LongRetreat
            } else {
                Band::Long
            };
            if locked {
                k.band = prev;
            }
            let behavior = profile.behavior[band_index(k.band)].as_str();
            let base = match k.band {
                Band::MediumRetreat => Band::Medium,
                Band::LongRetreat => Band::Long,
                b => b,
            };
            let fight_back = match profile.melee_when.as_str() {
                "always" => true,
                "never" => false,
                "ifShortRange" => base == Band::Short,
                "ifMediumRange" => matches!(base, Band::Short | Band::Medium),
                "ifPunched" => k.last_melee_hit > 0 && now < k.last_melee_hit + RECENTLY_HURT,
                _ => base == Band::Short,
            };
            let armed = cx.gun.is_some_and(|g| g.usable);
            // (A behaviour that isn't a boss's: no boss state.)
            let boss = (behavior.starts_with("barabbas")
                || behavior.starts_with("superNinja")
                || behavior.starts_with("mutantMuro"))
            .then(|| behavior.to_string());
            if boss.is_none() && !matches!(k.boss, super::bosses::Boss::None) {
                k.boss = super::bosses::Boss::None;
                self.mode = MoveMode::ByAlert;
            }
            let k = self.combat();
            match behavior {
                // A boss's: its own mode machine (`ai/bosses.rs`).
                _ if boss.is_some() => {}
                "stare" => w[HOLD] = 1.0,
                "melee" | "barabbasMelee" | "superNinjaMelee" | "mutantMuroMelee" => w[MELEE] = 1.0,
                "runForAlarm" => {
                    w[FIND_ALARM] = 1.2;
                    w[RETREAT] = 0.8;
                }
                // Shooting, armed: hold and fire, or charge while firing.
                "firingCharge" if armed => {
                    w[ADVANCE] = 1.0;
                    w[HOLD] = 0.2;
                    k.advance = (20.0, 80.0);
                }
                _ if armed => w[HOLD] = 1.0,
                // Shooting behaviours, unarmed (no guns yet, #10): fight
                // back hand to hand if the profile says so, else the
                // no-gun option (looking for a gun finds none).
                _ => {
                    if fight_back {
                        w[MELEE] = 1.0;
                    } else {
                        // Go for a gun — if the roll says so and one is near.
                        w[GUN] = 1.0;
                        match profile.no_gun.as_str() {
                            "retreat" => {
                                w[RETREAT] = 0.8;
                                k.retreat = (100.0, 1000.0);
                            }
                            "alarm" => {
                                w[FIND_ALARM] = 1.0;
                                w[MELEE] = 0.8;
                            }
                            _ => w[MELEE] = 0.8,
                        }
                    }
                }
            }
            match &boss {
                Some(b) if b.starts_with("barabbas") => self.barabbas_primary(b, dist, &mut w, cx),
                Some(b) if b.starts_with("superNinja") => self.ninja_primary(b, dist, &mut w, cx),
                Some(b) => self.muro_primary(b, dist, &mut w, cx),
                None => {}
            }
            // (A boss may not mind being too close for its gun.)
            let ignores_too_close = self.boss_mode().is_some() && self.barabbas_ignores_too_close();
            let k = self.combat();
            // Its gun put the target down: in to finish it hand to hand.
            let t = cx.others.get(k.target);
            if let Some(g) = cx
                .gun
                .filter(|_| armed && has_melee && profile.melee_when != "never")
            {
                let earned = t.is_some_and(|t| {
                    (g.stuns && (t.dizzy || t.fallen)) || (g.knocks_down && t.fallen)
                });
                if earned && k.weapon_melee == 0 {
                    k.weapon_melee = g.ai.melee_assist / COMBAT_PERIOD as u32;
                }
            }
            let weapon_melee = k.weapon_melee > 0;
            if weapon_melee {
                k.weapon_melee -= 1;
                w[MELEE] = 1.2;
                w[HOLD] = w[HOLD].max(0.2);
                k.have_los = true;
            }
            // Too close for the gun (`docs/AI.md`, *Shooting*): back off — or,
            // if it would fight back hand to hand, melee.
            let min_safe = cx.gun.map_or(0.0, |g| g.ai.min_safe_distance);
            k.too_close = 0.0;
            if armed && !weapon_melee && min_safe > 0.0 && dist < min_safe {
                let tc = (1.5 * (1.0 - dist / min_safe)).min(1.0);
                k.too_close = tc;
                if tc >= 0.75 && !ignores_too_close {
                    w[HOLD] *= 1.0 - tc;
                    w[ADVANCE] *= 1.0 - tc;
                    if fight_back {
                        w[MELEE] = w[MELEE].max(tc);
                    } else {
                        w[RETREAT] = tc;
                        k.retreat = (min_safe, 1.5 * min_safe);
                    }
                }
            }
            // A gun with no line of fire (or out of range): close in.
            if armed && !weapon_melee {
                k.have_los = line_of_fire(target_feet, cx);
                if !k.have_los && w[MELEE] < 1.0 {
                    w[ADVANCE] = 1.2;
                    w[HOLD] = 0.4;
                    k.advance = (12.0, 30.0);
                    if dist < 18.0 {
                        w[MELEE] = w[MELEE].max(0.7);
                    }
                }
            }
            if cx.fallen && k.daze == Some(0) {
                w[GETUP] = 1.5;
            }
        }

        // The highest viable one.
        let mut checked = [false; 7];
        let gun_chance = self.class.gun_pickup_percent;
        let gun_roll = self.rng.next() % 100;
        let pick = loop {
            let (best, &bw) =
                w.iter().enumerate().fold(
                    (0, &w[0]),
                    |(bi, bw), (i, v)| if *v > *bw { (i, v) } else { (bi, bw) },
                );
            if bw <= 0.0 {
                break Primary::Hold;
            }
            if checked[best] {
                break [
                    Primary::Hold,
                    Primary::Advance,
                    Primary::Retreat,
                    Primary::FindAlarm,
                    Primary::Melee,
                    Primary::GetUp,
                    Primary::Gun,
                ][best];
            }
            checked[best] = true;
            let k = self.combat();
            match best {
                ADVANCE => {
                    let second = w
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| i != ADVANCE)
                        .map(|(_, v)| *v)
                        .fold(0.0, f32::max);
                    let (weight, way) =
                        self.check_advance(target_feet, dist, w[ADVANCE], second, cx);
                    w[ADVANCE] = weight;
                    self.combat().advance_way = way;
                }
                RETREAT => {
                    w[MELEE] = 0.0;
                    let (min, max) = k.retreat;
                    if dist >= max {
                        w[RETREAT] = 0.0;
                    } else {
                        let urgency = if dist <= min {
                            1.0
                        } else {
                            1.0 - (dist - min) / (max - min)
                        };
                        w[RETREAT] *= urgency;
                    }
                }
                FIND_ALARM => {
                    if alarm.is_none() {
                        w[FIND_ALARM] = 0.0;
                    }
                }
                GUN => {
                    // Once a fight it decides whether it goes for guns at all.
                    let goes = *k.gun_roll.get_or_insert(gun_roll < gun_chance);
                    let still = k
                        .gun_target
                        .and_then(|g| cx.guns.iter().find(|f| f.index == g))
                        .filter(|f| f.usable);
                    // (How far it looks: a boss's own radii, else these.)
                    let (near, far) = k.gun_search.take().unwrap_or((30.0, GUN_SEARCH));
                    let pick = still.or_else(|| {
                        cx.guns
                            .iter()
                            .filter(|f| f.usable && !k.unreachable.contains(&f.index))
                            .filter(|f| f.at.distance(cx.feet) <= far)
                            .min_by(|a, b| {
                                a.at.distance(cx.feet).total_cmp(&b.at.distance(cx.feet))
                            })
                    });
                    match pick.filter(|_| goes) {
                        Some(f) => {
                            k.gun_target = Some(f.index);
                            let d = f.at.distance(cx.feet);
                            w[GUN] *= 1.0 - ((d - near) / (far - near)).max(0.0);
                        }
                        None => {
                            k.gun_target = None;
                            w[GUN] = 0.0;
                        }
                    }
                }
                MELEE => {
                    let clear =
                        (cx.sight)(cx.feet + Vec3::Y * 5.0, target_feet + Vec3::Y * 5.0, SHOOT);
                    if !has_melee || dist > MELEE_RANGE || k.try_advance > 0 || !clear {
                        w[MELEE] = 0.0;
                        w[ADVANCE] = w[ADVANCE].max(1.0);
                        k.advance = (15.0, 20.0);
                        w[HOLD] = w[HOLD].max(0.2);
                    }
                }
                _ => {}
            }
        };
        // (Barabbas doesn't dodge.)
        if self.boss_mode().is_none() {
            self.find_dodges(cx);
        } else {
            let k = self.combat();
            k.dodges.clear();
            k.dodge = None;
        }
        if pick == Primary::FindAlarm {
            self.alarm_setup(alarm, cx);
            return;
        }
        let name = self.name.clone();
        let k = self.combat();
        if pick != k.primary {
            eprintln!(
                "ai: {name}: combat {:?} → {pick:?} (at {dist:.0})",
                k.primary
            );
            k.primary = pick;
            k.advance_goal = None;
        }
        // Going for a gun: maybe on the run (its chance rolled once a gun).
        if pick == Primary::Gun && self.combat().gun_target.is_some() {
            if self.combat().running_roll.is_none() {
                let roll = self.rng.next() % 100 < self.class.gun_running_pickup_percent;
                self.combat().running_roll = Some(roll);
            }
            let running = self.combat().running_roll == Some(true) && self.try_running_pickup(cx);
            self.combat().gun_running = running;
        } else {
            let k = self.combat();
            k.running_roll = None;
            k.gun_running = false;
        }
    }

    /// A gun picked up on the run (`AI2rManeuver_TryRunningGunPickup`):
    /// already in a clip that picks up, it takes the gun within 16; standing,
    /// with the gun 5–70 off on the level (within 5 up or down) and straight
    /// ahead, behind or to a side (within 20°), it starts the roll or slide
    /// that way that can pick up — once the move would carry it 10 past the
    /// gun and the way is clear. True while this is how it goes for it.
    fn try_running_pickup(&mut self, cx: &Ctx) -> bool {
        use std::f32::consts::{FRAC_PI_2, PI};
        const MIN: f32 = 5.0;
        const MAX: f32 = 70.0;
        const OVERSHOOT: f32 = 10.0;
        const VERTICAL: f32 = 5.0;
        const DELTA: f32 = 20.0 * PI / 180.0;
        const RADIUS: f32 = 16.0;
        let Some(gun) = self
            .combat()
            .gun_target
            .and_then(|g| cx.guns.iter().find(|f| f.index == g))
            .copied()
        else {
            return false;
        };
        let b = cx.body;
        let d = gun.at - cx.feet;
        let dist = d.with_y_zero().length();
        if !b.inactive_upright {
            if b.picking_up || !b.can_pickup {
                return false;
            }
            if dist < RADIUS {
                self.want_pickup = Some(gun.index);
            }
            return true;
        }
        if d.y.abs() > VERTICAL || !(MIN..=MAX).contains(&dist) {
            return false;
        }
        let angle = d.x.atan2(d.z);
        let rel = (angle - cx.facing + PI).rem_euclid(2.0 * PI) - PI;
        let way = if rel.abs() < DELTA {
            0
        } else if (rel - FRAC_PI_2).abs() < DELTA {
            2
        } else if (rel + FRAC_PI_2).abs() < DELTA {
            3
        } else if rel.abs() > PI - DELTA {
            1
        } else {
            return false;
        };
        let Some((kind, end)) = b.escapes[way] else {
            return false;
        };
        // The way must be clear.
        let flat = end.with_y_zero();
        let clear = match (flat.try_normalize(), self.nav.is_some()) {
            (Some(dir), true) => (cx.probe)(
                cx.feet,
                glam::Vec2::new(dir.x, dir.z),
                flat.length(),
                crate::nav::Square::Border3,
            )
            .is_some_and(|p| p.stopped.is_none()),
            _ => true,
        };
        if !clear {
            return false;
        }
        // Not far enough yet: keep going for it, it will be.
        let along = angle.sin() * end.x + angle.cos() * end.z;
        if along < dist + OVERSHOOT {
            return true;
        }
        eprintln!("ai: {}: running pickup", self.name);
        self.anim_request = Some((kind, true));
        // (Its own empty gun goes.)
        self.drop_gun = true;
        true
    }

    // --- alarm (`docs/GOALS.md`, *Alarm*) --------------------------------------

    /// The nearest alarm console it can use (for scripts' orders).
    pub fn nearest_alarm(&self, cx: &Ctx) -> Option<usize> {
        self.find_alarm(cx)
    }

    /// The nearest alarm console it can use (active, not yet used) within
    /// its find distance by path.
    fn find_alarm(&self, cx: &Ctx) -> Option<usize> {
        let limit = self.profile.alarm.find_distance;
        cx.consoles
            .iter()
            .enumerate()
            .filter(|(_, k)| k.is_alarm && k.active && !k.triggered)
            .filter_map(|(i, k)| {
                (cx.path_length)(cx.feet, k.stand())
                    .filter(|&d| d <= limit)
                    .map(|d| (i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Sets off for alarm console `console` (None: the one it was going
    /// for). False if there is none.
    pub fn alarm_setup(&mut self, console: Option<usize>, cx: &mut Ctx) -> bool {
        let Some(c) = console.or(self.alarm_console) else {
            return false;
        };
        self.alarm_console = Some(c);
        self.fight_timer = 0;
        if self.last_alarm_console != Some(c) {
            self.alarm_repaths = 0;
            self.last_alarm_console = Some(c);
        }
        let Some(k) = cx.consoles.get(c) else {
            return false;
        };
        let dest = k.marker - Vec3::new(k.facing.sin(), 0.0, k.facing.cos()) * ALARM_OFFSET;
        let dest = Vec3::new(dest.x, k.position.y, dest.z);
        let facing = k.facing;
        self.exit(cx);
        // Everyone's harm to it so far is forgotten.
        for c in &mut cx.perception.contacts {
            c.damage = 0;
        }
        self.mode = MoveMode::RunNoAim;
        self.goal = Goal::Alarm(Box::new(AlarmRun {
            dest,
            stand_wait: 0,
        }));
        self.go_to(dest, MARKER_REACH - ALARM_OFFSET, true, Some(facing), cx);
        eprintln!("ai: {} → alarm (console {})", self.name, k.id);
        true
    }

    /// Gives up on the alarm (or it was raised): the nearest threat is
    /// pursued, else back to the job; the console is forgotten.
    pub fn alarm_stop(&mut self, cx: &mut Ctx) {
        if matches!(self.goal, Goal::Alarm(_)) {
            self.alarm_finish(None, cx);
        }
        self.alarm_console = None;
        self.fight_timer = 0;
    }

    /// Leaves the alarm run: to attack `target` (back to the alarm after
    /// the fight timer), or the nearest threat, or back to the job.
    fn alarm_finish(&mut self, target: Option<u32>, cx: &mut Ctx) {
        let target = match target {
            Some(t) => {
                self.fight_timer = self.profile.alarm.fight_ticks;
                Some(t)
            }
            None => {
                // The nearest of the strongest threats.
                let mut best: Option<(u32, Strength, f32)> = None;
                for c in &cx.perception.contacts {
                    if c.priority < Priority::HostileThreat || c.strength <= Strength::Forgotten {
                        continue;
                    }
                    let d = c.location.distance(cx.feet);
                    match best {
                        Some((_, s, _)) if c.strength < s => break,
                        Some((_, _, bd)) if d >= bd => {}
                        _ => best = Some((c.id, c.strength, d)),
                    }
                }
                best.map(|b| b.0)
            }
        };
        match target {
            Some(id) => {
                self.exit(cx);
                self.enter_pursuit(id, cx);
            }
            None => self.return_to_job(true, true, cx),
        }
    }

    /// Its console use finished: the alarm is raised.
    pub fn console_used(&mut self, cx: &mut Ctx) {
        if matches!(self.goal, Goal::Alarm(_)) {
            eprintln!("ai: {}: alarm raised", self.name);
            self.alarm_stop(cx);
        }
    }

    fn alarm_update(&mut self, cx: &mut Ctx) {
        if cx.using_console {
            self.halt();
            return;
        }
        let Some(c) = self.alarm_console.and_then(|c| cx.consoles.get(c)).cloned() else {
            self.alarm_finish(None, cx);
            return;
        };
        // Someone to deal with first? (Knocked down, some profiles hit back
        // at the nearest enemy in sight.)
        let a = self.profile.alarm;
        if cx.fallen && a.attack_if_knocked_down {
            let near = cx
                .perception
                .contacts
                .iter()
                .filter(|t| {
                    t.priority >= Priority::HostileThreat && t.strength >= Strength::Definite
                })
                .map(|t| (t.id, t.location.distance(cx.feet)))
                .filter(|&(_, d)| d <= a.ignore_distance)
                .min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((t, _)) = near {
                self.alarm_finish(Some(t), cx);
                return;
            }
        }
        let threat = cx
            .perception
            .contacts
            .iter()
            .filter(|t| t.priority >= Priority::HostileThreat && t.strength >= Strength::Definite)
            .find(|t| {
                let d = t.location.distance(cx.feet);
                d <= a.ignore_distance && (d <= a.chase_distance || t.damage >= a.damage_threshold)
            })
            .map(|t| t.id);
        if let Some(t) = threat {
            eprintln!("ai: {}: breaks off the alarm to fight", self.name);
            self.alarm_finish(Some(t), cx);
            return;
        }
        let at_point = self.arrived(cx) || self.distance_to_dest(cx) <= MARKER_REACH - ALARM_OFFSET;
        if !at_point && !self.path_failed() {
            if cx.fallen {
                self.press |= key::FORWARD;
            }
            return;
        }
        if cx.standing && !cx.fallen {
            if c.in_reach(cx.pelvis, cx.facing, true) {
                self.use_console = self.alarm_console;
                self.halt();
                return;
            }
            if !self.arrived(cx) && !self.path_failed() {
                return; // still turning to face it
            }
        } else {
            self.halt();
            let Goal::Alarm(run) = &mut self.goal else {
                return;
            };
            if run.stand_wait == 0 {
                run.stand_wait = ALARM_STAND_WAIT;
            }
            run.stand_wait -= 1;
            if run.stand_wait > 0 {
                return;
            }
        }
        if cx.fallen {
            self.press |= key::BACKWARD; // up, away from the console
            return;
        }
        if self.alarm_repaths >= ALARM_REPATHS {
            eprintln!("ai: {}: can't reach the alarm", self.name);
            self.alarm_stop(cx);
            return;
        }
        self.alarm_repaths += 1;
        let Goal::Alarm(run) = &self.goal else { return };
        let dest = run.dest;
        self.go_to(dest, MARKER_REACH - ALARM_OFFSET, true, Some(c.facing), cx);
    }
}

impl Agent {
    // --- panic (`docs/GOALS.md`, *Alarm, panic*) ----------------------------

    /// A non-combatant's reaction to news: a danger panics it; anything
    /// else it glances at.
    fn noncombatant_knowledge(
        &mut self,
        id: u32,
        degrade: bool,
        event: Option<(Stimulus, i32)>,
        cx: &mut Ctx,
    ) {
        let Some(mut c) = cx.perception.contact(id).cloned() else {
            return;
        };
        // (What this news was, not what the contact last heard.)
        if let Some((stimulus, _)) = event {
            c.stimulus = stimulus;
        }
        if degrade {
            let Goal::Panic(p) = &self.goal else { return };
            if p.cause == Some(id)
                && (c.strength <= Strength::Forgotten || c.priority < Priority::HostileThreat)
            {
                let by_hostile = p.by_hostile;
                let other = cx
                    .perception
                    .contacts
                    .iter()
                    .find(|o| self.dangerous(o, by_hostile, cx))
                    .map(|o| o.id);
                let Goal::Panic(p) = &mut self.goal else {
                    return;
                };
                match other {
                    Some(o) => p.cause = Some(o),
                    None => {
                        p.cause = None;
                        p.until = c.time + PANIC_AFTER_CAUSE_GONE;
                    }
                }
            }
            return;
        }
        if c.priority == Priority::HostileThreat || c.stimulus.combat_sound() {
            let only_hostiles = matches!(&self.goal, Goal::Panic(p) if p.by_hostile);
            if self.dangerous(&c, only_hostiles, cx) {
                self.panic(&c, cx);
            }
        }
        if !self.panicking() {
            self.glance_at(c.location, c.stimulus.glance(), cx);
        }
    }

    /// Whether a contact is a danger to it.
    fn dangerous(&self, c: &super::perception::Contact, only_hostiles: bool, cx: &Ctx) -> bool {
        if c.strength <= Strength::Forgotten {
            return false;
        }
        if matches!(c.stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) {
            return true;
        }
        if !matches!(
            c.stimulus,
            Stimulus::SoundMelee | Stimulus::SoundGunshot | Stimulus::SightDefinite
        ) {
            return false;
        }
        if c.who.is_none_or(|w| w == cx.me) {
            return false;
        }
        let hostile =
            c.priority >= Priority::HostileThreat || (!only_hostiles && c.stimulus.combat_sound());
        hostile && cx.feet.distance(c.location) <= PANIC_ENEMY_RANGE
    }

    /// Panics (or panics longer) because of `c`.
    fn panic(&mut self, c: &super::perception::Contact, cx: &mut Ctx) {
        let [melee, gunfire, sight, hurt] = self.profile.panic;
        let time = match c.stimulus {
            Stimulus::SoundMelee => melee,
            Stimulus::SoundGunshot => gunfire,
            Stimulus::SightDefinite => sight,
            Stimulus::HitWeapon | Stimulus::HitMelee => hurt,
            _ => return,
        };
        if !self.panicking() {
            // (A neutral interaction is broken off by the danger.)
            if matches!(self.goal, Goal::Neutral) {
                self.neutral_exit = Some(true);
            }
            self.exit(cx);
            self.halt();
            self.mode = MoveMode::RunNoAim;
            self.goal = Goal::Panic(Box::new(Panic {
                say_timer: 45,
                ..Default::default()
            }));
            eprintln!("ai: {} → panic", self.name);
        }
        let extra = self.rng.range((0, time / 2));
        let now = cx.now;
        // Now and then it whimpers.
        let next = 1500 + self.rng.next() % 500;
        if let Goal::Panic(p) = &mut self.goal {
            p.say_timer = p.say_timer.saturating_sub(1);
            if p.say_timer == 0 {
                p.say_timer = next;
                cx.perception.vocals.push(crate::world::vocal::COWER);
            }
        }
        let cause = match &self.goal {
            Goal::Panic(p) => p.cause.and_then(|id| cx.perception.contact(id)).cloned(),
            _ => None,
        };
        let Goal::Panic(p) = &mut self.goal else {
            return;
        };
        if c.priority >= Priority::HostileNoThreat {
            p.by_hostile = true;
        }
        p.until = p.until.max(now + u64::from(time + extra));
        if c.who.is_some() && cause.as_ref().is_none_or(|k| c.strength > k.strength) {
            if cause.as_ref().is_some_and(|k| k.priority > c.priority) {
                return; // a more important cause
            }
            p.cause = Some(c.id);
        }
    }

    fn panic_update(&mut self, cx: &mut Ctx) {
        let until = match &self.goal {
            Goal::Panic(p) => p.until,
            _ => return,
        };
        if cx.now > until {
            eprintln!("ai: {}: calms down", self.name);
            self.return_to_job(true, true, cx);
            return;
        }
        let cause = match &self.goal {
            Goal::Panic(p) => p.cause.and_then(|id| cx.perception.contact(id)).cloned(),
            _ => None,
        };
        let enemy = cause
            .as_ref()
            .and_then(|c| c.who)
            .and_then(|w| cx.others.get(w))
            .map(|o| o.feet);
        let daze_roll = {
            let (lo, hi) = cx.daze;
            lo + self.rng.next() % hi.saturating_sub(lo).max(1)
        };
        let Goal::Panic(p) = &mut self.goal else {
            return;
        };
        let mut stop_looking = false;
        if !p.kneeling {
            let close = cause
                .as_ref()
                .is_some_and(|c| cx.feet.distance(c.location) < PANIC_KNEEL);
            if cx.crouching || close {
                p.kneeling = true;
                stop_looking = true;
            }
        } else if cx.fallen {
            if let Some(n) = p.dazed {
                p.dazed = (n > 0).then(|| n - 1);
            } else if p.done_daze {
                // Get up: away from the enemy if it is in front, crouched.
                let behind = enemy
                    .is_some_and(|e| relative(yaw_toward(cx.feet, e) - cx.facing).abs() > PI / 2.0);
                self.press |= if behind { key::FORWARD } else { key::BACKWARD } | key::CROUCH;
            } else {
                p.dazed = Some(daze_roll);
                p.done_daze = true;
                p.kneeling = true;
                stop_looking = true;
            }
        }
        // A knockdown (before it kneels) also makes it kneel when it is up.
        if cx.fallen && !p.done_daze && p.dazed.is_none() {
            p.dazed = Some(daze_roll);
            p.done_daze = true;
            p.kneeling = true;
            stop_looking = true;
        }
        let (kneeling, dazed) = (p.kneeling, p.dazed.is_some());
        if kneeling {
            self.mode = MoveMode::Creep;
        }
        self.halt();
        if stop_looking || dazed || kneeling {
            self.look = Look::None;
        } else if let Some(e) = enemy {
            self.look = Look::At(e);
        }
    }
}

/// Points a combat at a new target (its contact).
fn set_new_target(c: &mut Combat, contact: &super::perception::Contact) {
    c.contact = contact.id;
    c.target_dead = false;
    c.target_strength = contact.strength;
    c.investigate = 0;
    c.done_taunt = false;
    c.taunt_enable = false;
    c.lost_contact = None;
    c.last_hit = 0;
    c.last_melee_hit = 0;
    if matches!(contact.stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) {
        c.last_hit = contact.time;
        if contact.stimulus == Stimulus::HitMelee {
            c.last_melee_hit = contact.time;
        }
    }
    c.band = Band::None;
}

/// The flag a movement waypoint heads for (the waypoints a resumed patrol
/// may rejoin at).
fn movement_flag(w: &Waypoint) -> Option<i16> {
    match w {
        Waypoint::MoveTo {
            at: Target::Flag(f),
            ..
        }
        | Waypoint::LookAt(Target::Flag(f)) => Some(*f),
        Waypoint::Guard { flag, .. } => Some(*flag),
        _ => None,
    }
}

/// A range band's behaviour in the combat profile (long, medium, short,
/// medium retreat, long retreat).
fn band_index(b: Band) -> usize {
    match b {
        Band::Long | Band::None => 0,
        Band::Medium => 1,
        Band::Short => 2,
        Band::MediumRetreat => 3,
        Band::LongRetreat => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_follows_the_alert_and_hurry() {
        assert_eq!(MoveMode::ByAlert.pace(Alert::Lull, true), Pace::Walk);
        assert_eq!(MoveMode::ByAlert.pace(Alert::Medium, true), Pace::Run);
        assert_eq!(MoveMode::ByAlert.pace(Alert::Combat, false), Pace::Walk);
        assert_eq!(MoveMode::Run.pace(Alert::Lull, false), Pace::Run);
        assert_eq!(MoveMode::Creep.pace(Alert::High, true), Pace::Creep);
    }

    #[test]
    fn a_guard_scan_stays_within_its_range() {
        let mut rng = Rng(12345);
        let mut s = Scan::reset(Some(1.0));
        s.stay_facing = true;
        s.stay_dir = Some(1.0);
        s.stay_range = 0.5;
        s.dir_times = GUARD_DIR;
        s.angle_times = GUARD_ANGLE;
        s.angle_range = GUARD_ANGLE_RANGE;
        let clear = |_: f32| true;
        let first = s.update(0.0, &mut rng, &clear);
        assert!(
            relative(first - 1.0).abs() <= GUARD_ANGLE_RANGE + 1e-4,
            "starts looking where it faces"
        );
        for _ in 0..5000 {
            let l = s.update(0.0, &mut rng, &clear);
            assert!(
                relative(l - 1.0).abs() <= 0.5 + GUARD_ANGLE_RANGE + 1e-3,
                "{l}"
            );
        }
    }

    #[test]
    fn a_new_look_direction_avoids_the_last() {
        let mut rng = Rng(7);
        let mut s = Scan::reset(None);
        let clear = |_: f32| true;
        s.new_direction(0.0, None, &mut rng, &clear);
        assert_eq!(s.dir, 0.0, "the first look is along the facing");
        for _ in 0..50 {
            let last = s.dir;
            s.new_direction(0.0, None, &mut rng, &clear);
            assert!(relative(s.dir - last).abs() >= LOOK_MIN_DELTA - 1e-4);
        }
    }
}
