//! Oni's particle system ("Particle3"): the classes the converter exports
//! (`common/particle_classes.json`) run here — emitters, the action lists
//! each event runs, collision with the level and with characters, damage.
//! Weapon shots are particles (`docs/WEAPONS.md`); the behaviour is
//! described in `docs/PARTICLES.md`.
//!
//! The system knows nothing of the world: what it needs (rays against the
//! level and characters, damage, attractor targets) it asks a [`Host`].

mod actions;
pub mod class;
pub mod effect;
mod emit;
pub mod glass;

use std::cell::Cell;
use std::rc::Rc;

use glam::{Mat3, Mat4, Vec3};

use class::{cflag, Class, Value, VarKind};
pub use class::{event, ClassId, Classes};

/// Ticks per second.
pub const HZ: f64 = 60.0;
/// The "emit at once" time sentinel.
const NEVER: f64 = 1e9;
/// An emitter's count while it is switched off.
const DISABLED: u16 = 0xFFFF;

/// Damage owner words (`WPtDamageOwnerType`): the type in the top byte,
/// a character index (or turret id) below.
pub mod owner {
    pub const CHARACTER_WEAPON: u32 = 1 << 24;
    pub const CHARACTER_MELEE: u32 = 2 << 24;
    pub const TURRET: u32 = 3 << 24;
    pub const ENVIRONMENTAL: u32 = 4 << 24;
    pub const FALLING: u32 = 5 << 24;
    pub const ACT_OF_GOD: u32 = 6 << 24;
    pub const TYPE_MASK: u32 = 0xFF00_0000;

    /// The character an owner word names, if it names one.
    pub fn character(o: u32) -> Option<usize> {
        matches!(o & TYPE_MASK, CHARACTER_WEAPON | CHARACTER_MELEE)
            .then_some((o & !TYPE_MASK) as usize)
    }
}

/// Per-particle flags.
pub mod pflag {
    pub const HIDDEN: u32 = 1 << 0;
    pub const FADE_OUT: u32 = 1 << 1;
    pub const STOPPED: u32 = 1 << 2;
    pub const CHOP: u32 = 1 << 3;
    pub const ORIENT_TO_VELOCITY: u32 = 1 << 5;
    pub const VELOCITY_CHANGED: u32 = 1 << 6;
    pub const EXPLODE_ON_CONTACT: u32 = 1 << 7;
    pub const TEMPORARY: u32 = 1 << 8;
    pub const HAS_ATTRACTOR: u32 = 1 << 9;
    pub const BREAK_LINKS_TO_ME: u32 = 1 << 10;
    pub const CUTSCENE_KILLED: u32 = 1 << 11;
    /// It plays an ambient sound.
    pub const PLAYING_SOUND: u32 = 1 << 12;
    /// It owns a decal (dynamic, or static: a level's).
    pub const DECAL: u32 = 1 << 13;
    pub const DECAL_STATIC: u32 = 1 << 14;
}

/// A reference to a particle that goes stale when the particle dies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle {
    slot: u32,
    generation: u32,
}

impl Handle {
    /// A number unique to this particle (the duplicate-hit stamp).
    pub fn id(self) -> u64 {
        (u64::from(self.slot) << 32) | u64::from(self.generation)
    }
}

/// A variable's value at run time.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    F(f32),
    U(u32),
    S(Rc<str>),
}

impl Val {
    pub fn f(&self) -> Option<f32> {
        match self {
            Val::F(x) => Some(*x),
            _ => None,
        }
    }
}

/// An emitter's running state.
#[derive(Clone, Debug)]
pub struct EmitterState {
    /// Particles emitted, or [`DISABLED`].
    pub count: u16,
    pub last_time: f64,
    pub last: Option<Handle>,
}

/// One live particle.
#[derive(Clone)]
pub struct Particle {
    pub class: ClassId,
    pub flags: u32,
    pub action_mask: u32,
    /// Position — in its dynamic matrix's space if it has one.
    pub position: Vec3,
    pub tick: u32,
    pub time: f64,
    /// Seconds left; 0 = unlimited.
    pub lifetime: f32,
    pub fadeout_time: f32,
    pub link: Option<Handle>,
    pub emitters: Vec<EmitterState>,
    pub velocity: Vec3,
    /// Columns: right (X), forward (Y), up (Z).
    pub orientation: Mat3,
    pub offset: Vec3,
    /// The matrix it rides (a weapon's), shared.
    pub matrix: Option<Rc<Cell<Mat4>>>,
    pub owner: u32,
    /// The attractor it is after (a character index), and when it may look
    /// again (tick).
    pub attractor: Option<usize>,
    pub attractor_check: u32,
    pub vars: Vec<Val>,
    /// When it was made, among all particles.
    pub serial: u64,
    /// Its texture animation's random start (16 bits; with a texture index)
    /// and the tick its animation's time runs from (with a texture time:
    /// its birth, unless an action moves it).
    pub texture_index: u32,
    pub texture_time: u32,
}

/// What a particle collided with in this step.
#[derive(Clone, Copy, Debug)]
pub struct Collision {
    /// The level (true) or a character.
    pub env: bool,
    pub point: Vec3,
    pub normal: Vec3,
    /// The (shortened) ray that hit.
    pub dir: Vec3,
    /// The level surface, or the character index.
    pub id: u32,
    pub t: f64,
    /// A level hit's quad (its material), when known.
    pub quad: Option<u32>,
    /// A character hit's body part.
    pub part: Option<u32>,
}

/// Where a ray from `from` along `ray` enters the sphere `centre`, `r`
/// (`PHrCollision_Point_SphereTree`): the fraction along it (0–1) and the
/// normal there. Heading away from it, or starting inside, is no hit.
pub fn ray_sphere_entry(from: Vec3, ray: Vec3, centre: Vec3, r: f32) -> Option<(f32, Vec3)> {
    match crate::geom::ray_sphere(from, ray, centre, r)? {
        crate::geom::Entry::Inside => None,
        crate::geom::Entry::At(t) => {
            (t <= 1.0).then(|| (t, (from + ray * t - centre).normalize_or(Vec3::Y)))
        }
    }
}

/// A collision's id with this bit: a placed object (its index), not a
/// character — a physics hit that harms no one.
pub const OBJECT_HIT: u32 = 1 << 31;

/// A level hit: how far along the ray (0–1), the surface normal and an id.
#[derive(Clone, Copy, Debug)]
pub struct EnvHit {
    pub fraction: f32,
    pub normal: Vec3,
    pub surface: u32,
    /// The level quad hit (its material), when known.
    pub quad: Option<u32>,
}

/// What a particle wants heard (`docs/AUDIO.md`, *Particles*): the world
/// plays these after the particles' turn.
#[derive(Clone, Debug)]
pub enum SoundEvent {
    /// An impulse sound at a place.
    Impulse { name: Rc<str>, at: Vec3 },
    /// A particle starts an ambient sound (that follows it) …
    AmbientStart {
        particle: Handle,
        name: Rc<str>,
        at: Vec3,
    },
    /// … or stops it (it plays out).
    AmbientStop { particle: Handle },
    /// An impact effect (`docs/AUDIO.md`, *Impact effects*): by name and
    /// weight, where, how fast, what it hit, whose.
    /// (With the particle's orientation and flags, for the effect's
    /// particles.)
    Impact {
        impact: Rc<str>,
        modifier: u32,
        at: Vec3,
        velocity: Option<Vec3>,
        orientation: Option<Mat3>,
        hit: Option<Collision>,
        owner: u32,
        flags: u32,
    },
    /// A projectile with a flyby sound is out.
    Flyby { particle: Handle },
}

/// A character hit along a ray.
#[derive(Clone, Copy, Debug)]
pub struct CharHit {
    pub fraction: f32,
    pub who: usize,
    pub normal: Vec3,
    /// The body part struck.
    pub part: usize,
}

/// A character an attractor can pick.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub who: usize,
    /// Its upper body (the particles' aim point).
    pub position: Vec3,
    pub velocity: Vec3,
}

/// What the particle system needs from the world.
pub trait Host {
    /// The first level surface along `from → from + ray`.
    fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit>;
    /// The first character hit along the ray — by a point, or by a sphere
    /// of `radius` — except a hit leaving `owner`'s own body.
    fn ray_chars(
        &mut self,
        from: Vec3,
        ray: Vec3,
        radius: f32,
        owner: Option<usize>,
    ) -> Option<CharHit>;
    /// The first placed object met along the ray (its collision sphere;
    /// grown by `radius` for a sphere): how far along (0–1), the normal
    /// there, which object.
    fn ray_objects(&mut self, _from: Vec3, _ray: Vec3, _radius: f32) -> Option<(f32, Vec3, u32)> {
        None
    }
    /// A direct hit (`WPrHitChar`); true when `stamp` is the victim's last
    /// particle (a duplicate, rejected).
    #[allow(clippy::too_many_arguments)]
    fn hit_char(
        &mut self,
        who: usize,
        point: Vec3,
        dir: Vec3,
        kind: u32,
        damage: f32,
        stun: f32,
        knockback: f32,
        owner: u32,
        stamp: Option<u64>,
        self_immune: bool,
    ) -> bool;
    /// A blast (`WPrBlast`).
    #[allow(clippy::too_many_arguments)]
    fn blast(
        &mut self,
        centre: Vec3,
        kind: u32,
        damage: f32,
        stun: f32,
        knockback: f32,
        radius: f32,
        falloff: u32,
        owner: u32,
        stamp: u64,
        self_immune: bool,
    );
    /// Damages a level surface (breaks glass, `damage` hard); true if it
    /// broke.
    fn damage_surface(&mut self, _surface: u32, _damage: f32) -> bool {
        false
    }
    fn breakable(&self, _surface: u32) -> bool {
        false
    }
    /// Breaks the glass within `radius` of `centre`, `damage` hard, blown
    /// out by `blast` (none: shattered as by a shot).
    fn damage_surfaces_near(
        &mut self,
        _centre: Vec3,
        _radius: f32,
        _damage: f32,
        _blast: Option<glass::Blast>,
    ) {
    }
    /// Character `who`'s super amount (`ONtCharacter.super_amount`).
    fn super_amount(&self, _who: usize) -> f32 {
        0.0
    }
    /// The living characters (attractor candidates).
    fn targets(&self) -> Vec<Target>;
    /// Whether `other` is hostile to character `me`.
    fn hostile(&self, me: usize, other: usize) -> bool;
    /// A clear line through the level.
    fn clear(&mut self, from: Vec3, to: Vec3) -> bool;
    /// Extra degrees of aim error for shots from `owner` (an AI's skill).
    fn inaccuracy(&self, _owner: u32) -> f32 {
        0.0
    }
    /// A point on character `who`'s body (`ONrParticle3_FindBodySurfacePosition`
    /// / `…BodyBonePosition`): the part's following matrix, the point in its
    /// space, and (on the surface) the way out there. `r`: the draws.
    fn body_point(
        &mut self,
        _who: usize,
        _surface: bool,
        _radius: f32,
        _r: [f32; 3],
    ) -> Option<(Rc<Cell<Mat4>>, Vec3, Option<glam::Mat3>)> {
        None
    }
    /// Character `who`'s body part `part`, as a matrix that follows it (for
    /// effects riding what they struck).
    fn part_matrix(&mut self, _who: usize, _part: usize) -> Option<Rc<Cell<Mat4>>> {
        None
    }
    /// The level quad struck at `point` facing `normal` (for decals).
    fn struck_quad(&mut self, _point: Vec3, _normal: Vec3) -> Option<u32> {
        None
    }
    /// Lays a decal on the level (`crate::decal::build`).
    fn build_decal(
        &mut self,
        _quad: Option<u32>,
        _params: &crate::decal::Params,
    ) -> Option<crate::decal::Mesh> {
        None
    }
}

struct Slot {
    generation: u32,
    particle: Option<Particle>,
}

/// All live particles.
pub struct System {
    pub classes: Rc<Classes>,
    slots: Vec<Slot>,
    free: Vec<u32>,
    rng: u32,
    /// Game time in ticks (the last update's).
    pub tick: u32,
    /// Sounds the particles want heard, for the world to play.
    pub sounds: Vec<SoundEvent>,
    /// The colour new particles of global-tint classes take (melee flashes
    /// by the victim's health), while set.
    pub global_tint: Option<u32>,
    /// The decals the particles own.
    pub decals: effect::Decals,
    /// Particles made so far (each one's creation order).
    serial: u64,
    /// New particles are temporary (`particle_temp_start`): they, and what
    /// they emit, go at `particle_temp_kill`.
    pub temporary: bool,
    /// The variables an action reads, as they were when it began (reused,
    /// not allocated afresh for every action of every particle).
    scratch_vars: Vec<Val>,
}

/// The outcome of running an action list.
#[derive(Default, Clone, Copy)]
struct Outcome {
    dead: bool,
    stop: bool,
}

/// One inverse-normal table (deciles), for bell-curve values.
const NORMAL: [f32; 11] = [
    0.0, 0.1257, 0.2533, 0.3853, 0.5244, 0.6745, 0.8416, 1.0364, 1.2816, 1.6449, 3.0902,
];

impl System {
    pub fn new(classes: Rc<Classes>) -> System {
        System {
            classes,
            slots: Vec::new(),
            free: Vec::new(),
            rng: 0x2545_F491,
            tick: 0,
            sounds: Vec::new(),
            global_tint: None,
            decals: effect::Decals::default(),
            serial: 0,
            temporary: false,
            scratch_vars: Vec::new(),
        }
    }

    /// Live particles (for drawing and tests).
    pub fn iter(&self) -> impl std::iter::Iterator<Item = (Handle, &Particle)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.particle.as_ref().map(|p| {
                (
                    Handle {
                        slot: i as u32,
                        generation: s.generation,
                    },
                    p,
                )
            })
        })
    }

    pub fn get(&self, h: Handle) -> Option<&Particle> {
        let s = self.slots.get(h.slot as usize)?;
        (s.generation == h.generation).then_some(s.particle.as_ref()?)
    }

    pub fn get_mut(&mut self, h: Handle) -> Option<&mut Particle> {
        let s = self.slots.get_mut(h.slot as usize)?;
        if s.generation != h.generation {
            return None;
        }
        s.particle.as_mut()
    }

    pub fn alive(&self, h: Handle) -> bool {
        self.get(h).is_some()
    }

    /// How many particles live, and the classes with the most (for
    /// finding a runaway emitter): (count, [(class index, count)], most first).
    pub fn census(&self) -> (usize, Vec<(ClassId, usize)>) {
        let mut by: std::collections::HashMap<ClassId, usize> = Default::default();
        let mut n = 0;
        for p in self.slots.iter().filter_map(|s| s.particle.as_ref()) {
            n += 1;
            *by.entry(p.class).or_default() += 1;
        }
        let mut top: Vec<_> = by.into_iter().collect();
        top.sort_by_key(|a| std::cmp::Reverse(a.1));
        top.truncate(3);
        (n, top)
    }

    pub fn class(&self, id: ClassId) -> &Class {
        &self.classes.list[id]
    }

    // --- random numbers ---

    fn rand16(&mut self) -> u16 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as u16
    }

    /// Uniform in [0, 1].
    fn random(&mut self) -> f32 {
        f32::from(self.rand16()) / 65535.0
    }

    /// Uniform in [−1, 1].
    fn signed_random(&mut self) -> f32 {
        f32::from(self.rand16()) * 2.0 / 65535.0 - 1.0
    }

    /// A standard-normal sample, from the decile table (about ±3).
    fn normal(&mut self) -> f32 {
        let d = 0.999 * self.signed_random();
        let x = d.abs() * 10.0;
        let i = (x as usize).min(9);
        let f = x - i as f32;
        let v = NORMAL[i] + (NORMAL[i + 1] - NORMAL[i]) * f;
        v.copysign(d)
    }

    // --- values ---

    /// Evaluates a value for a particle (random ones roll anew each time).
    fn eval(&mut self, v: &Value, vars: &[Val]) -> Option<Val> {
        let lerp_shade = |a: u32, b: u32, t: f32| -> u32 {
            (0..4).fold(0, |acc, k| {
                let (ca, cb) = ((a >> (k * 8)) & 0xFF, (b >> (k * 8)) & 0xFF);
                let c = (ca as f32 + (cb as f32 - ca as f32) * t)
                    .round()
                    .clamp(0.0, 255.0) as u32;
                acc | c << (k * 8)
            })
        };
        Some(match v {
            Value::Null => return None,
            Value::Var(i) => vars.get(*i)?.clone(),
            Value::Float(x) => Val::F(*x),
            Value::Range(a, b) => {
                let r = self.random();
                Val::F(a + r * (b - a))
            }
            Value::Bell(m, s) => {
                let n = self.normal();
                Val::F(m + n * s)
            }
            Value::Str(s) => Val::S(s.clone()),
            Value::Shade(c) => Val::U(*c),
            Value::ShadeRange(a, b) => {
                let r = self.random();
                Val::U(lerp_shade(*a, *b, r))
            }
            Value::ShadeBell(m, s) => {
                let c = (0..4).fold(0u32, |acc, k| {
                    let (cm, cs) = (
                        ((m >> (k * 8)) & 0xFF) as f32,
                        ((s >> (k * 8)) & 0xFF) as f32,
                    );
                    let n = self.normal();
                    acc | ((cm + n * cs).round().clamp(0.0, 255.0) as u32) << (k * 8)
                });
                Val::U(c)
            }
            Value::Enum(e) => Val::U(*e),
            Value::Cycle(len, max) => {
                let t = f64::from(self.tick) / HZ;
                Val::F(((t / f64::from(*len)).fract() as f32) * max)
            }
        })
    }

    fn eval_f(&mut self, v: &Value, vars: &[Val]) -> Option<f32> {
        self.eval(v, vars)?.f()
    }

    // --- creation and death ---

    /// Creates a particle of `class` at `tick` (the caller places it, then
    /// sends it [`event::CREATE`] via [`System::created`]).
    pub fn create(&mut self, class: ClassId, tick: u32) -> Particle {
        let c = &self.classes.list[class];
        let mut flags = 0;
        if c.has(cflag::INITIALLY_HIDDEN) {
            flags |= pflag::HIDDEN;
        }
        if self.temporary {
            flags |= pflag::TEMPORARY;
        }
        let emitters = c
            .emitters
            .iter()
            .map(|e| EmitterState {
                count: if e.flags & class::eflag::INITIALLY_ACTIVE != 0 {
                    0
                } else {
                    DISABLED
                },
                last_time: NEVER,
                last: None,
            })
            .collect();
        let classes = self.classes.clone();
        let c = &classes.list[class];
        let lifetime = self.eval_f(&c.lifetime, &[]).unwrap_or(0.0).max(0.0);
        let inits: Vec<Value> = c.vars.iter().map(|v| v.init.clone()).collect();
        let kinds: Vec<VarKind> = c.vars.iter().map(|v| v.kind).collect();
        let mut vars: Vec<Val> = inits
            .iter()
            .zip(kinds)
            .map(|(v, k)| {
                self.eval(v, &[]).unwrap_or(if k == VarKind::Float {
                    Val::F(0.0)
                } else {
                    Val::U(0)
                })
            })
            .collect();
        // The global tint, into a tint variable.
        if let (Some(tint), Value::Var(k), true) =
            (self.global_tint, &c.tint, c.has(cflag::USE_GLOBAL_TINT))
        {
            if let Some(v) = vars.get_mut(*k) {
                *v = Val::U(tint);
            }
        }
        self.serial += 1;
        // (A random start for its texture, as the original rolls one.)
        let texture_index = if c.has(cflag::TEXTURE_INDEX) {
            u32::from(self.rand16())
        } else {
            0
        };
        Particle {
            class,
            flags,
            action_mask: c.action_mask,
            position: Vec3::ZERO,
            tick,
            time: f64::from(tick) / HZ,
            lifetime,
            fadeout_time: 0.0,
            link: None,
            emitters,
            velocity: Vec3::ZERO,
            orientation: Mat3::IDENTITY,
            offset: Vec3::ZERO,
            matrix: None,
            owner: 0,
            attractor: None,
            attractor_check: 0,
            vars,
            serial: self.serial,
            texture_index,
            texture_time: tick,
        }
    }

    /// Adds a created particle and sends it Create at time `t`. Returns its
    /// handle, or `None` if it died on creation.
    pub fn created(&mut self, p: Particle, t: f64, host: &mut dyn Host) -> Option<Handle> {
        let flyby = self.classes.list[p.class].flyby.is_some();
        let h = self.insert(p);
        if flyby {
            self.sounds.push(SoundEvent::Flyby { particle: h });
        }
        let mut p = self.take(h)?;
        self.find_attractor(&mut p, 0, host);
        let out = self.run(&mut p, h, event::CREATE, t, 0.0, None, false, host);
        if out.dead {
            self.die(p, h, t, host);
            return None;
        }
        self.put(h, p);
        Some(h)
    }

    fn insert(&mut self, p: Particle) -> Handle {
        if let Some(slot) = self.free.pop() {
            let s = &mut self.slots[slot as usize];
            s.particle = Some(p);
            Handle {
                slot,
                generation: s.generation,
            }
        } else {
            self.slots.push(Slot {
                generation: 1,
                particle: Some(p),
            });
            Handle {
                slot: self.slots.len() as u32 - 1,
                generation: 1,
            }
        }
    }

    /// Takes a particle out of its slot to work on it (the slot stays
    /// reserved).
    fn take(&mut self, h: Handle) -> Option<Particle> {
        let s = self.slots.get_mut(h.slot as usize)?;
        if s.generation != h.generation {
            return None;
        }
        s.particle.take()
    }

    fn put(&mut self, h: Handle, p: Particle) {
        if let Some(s) = self.slots.get_mut(h.slot as usize) {
            if s.generation == h.generation {
                s.particle = Some(p);
            }
        }
    }

    /// A particle's death: its Die list, then its slot is freed (every
    /// handle to it goes stale).
    fn die(&mut self, mut p: Particle, h: Handle, t: f64, host: &mut dyn Host) {
        self.run(&mut p, h, event::DIE, t, 0.0, None, false, host);
        self.drop_decal(&p, h);
        // (Its ambient sound stops with it.)
        if p.flags & pflag::PLAYING_SOUND != 0 {
            self.sounds.push(SoundEvent::AmbientStop { particle: h });
        }
        self.release(h);
    }

    fn release(&mut self, h: Handle) {
        if let Some(s) = self.slots.get_mut(h.slot as usize) {
            if s.generation == h.generation {
                s.particle = None;
                s.generation = s.generation.wrapping_add(1);
                self.free.push(h.slot);
            }
        }
    }

    /// Kills a particle from outside (a weapon dropped, a level ending).
    /// `P3rSetTint`: a particle of a class taking the global tint takes
    /// `tint` (its tint variable).
    pub fn set_tint(&mut self, h: Handle, tint: u32) {
        let classes = self.classes.clone();
        let Some(p) = self.get_mut(h) else { return };
        let c = &classes.list[p.class];
        if let (Value::Var(k), true) = (&c.tint, c.has(cflag::USE_GLOBAL_TINT)) {
            if let Some(v) = p.vars.get_mut(*k) {
                *v = Val::U(tint);
            }
        }
    }

    /// `P3rKillTemporaryParticles`: every temporary particle dies.
    pub fn kill_temporary(&mut self, host: &mut dyn Host) {
        let doomed: Vec<Handle> = self
            .iter()
            .filter(|(_, p)| p.flags & pflag::TEMPORARY != 0)
            .map(|(h, _)| h)
            .collect();
        for h in doomed {
            self.kill(h, host);
        }
    }

    pub fn kill(&mut self, h: Handle, host: &mut dyn Host) {
        if let Some(p) = self.take(h) {
            let t = p.time;
            self.die(p, h, t, host);
        }
    }

    /// Sends `event` to a particle at time `t`.
    pub fn send(&mut self, h: Handle, ev: usize, t: f64, host: &mut dyn Host) {
        let Some(mut p) = self.take(h) else { return };
        let out = self.run(&mut p, h, ev, t, 0.0, None, false, host);
        if out.dead {
            self.die(p, h, t, host);
        } else {
            self.put(h, p);
        }
    }

    // --- positions ---

    /// Where a particle really is, in the world: its matrix applied to its
    /// position plus offset.
    pub fn world_position(&self, p: &Particle) -> Vec3 {
        let local = p.position + p.offset;
        match &p.matrix {
            Some(m) => m.get().transform_point3(local),
            None => local,
        }
    }

    /// Its orientation in the world.
    pub fn world_orientation(&self, p: &Particle) -> Mat3 {
        match &p.matrix {
            Some(m) => Mat3::from_mat4(m.get()) * p.orientation,
            None => p.orientation,
        }
    }

    /// Follows a lock-to-linked chain: the position of the particle at its
    /// end, if the link holds.
    fn linked_position(&self, p: &Particle) -> Option<Vec3> {
        let mut h = p.link?;
        for _ in 0..8 {
            let q = self.get(h)?;
            if q.flags & pflag::BREAK_LINKS_TO_ME != 0 {
                return None;
            }
            match q
                .link
                .filter(|_| self.class(q.class).has(cflag::LOCK_TO_LINKED))
            {
                Some(next) => h = next,
                None => return Some(self.world_position(q)),
            }
        }
        None
    }

    // --- the tick ---

    /// Advances every particle to `tick` (essential classes, then
    /// decorative ones).
    pub fn update(&mut self, tick: u32, host: &mut dyn Host) {
        self.tick = tick;
        let final_t = f64::from(tick) / HZ;
        for decorative in [false, true] {
            let mut i = 0;
            // New particles appended during the pass are updated in it too,
            // from their own creation time.
            while i < self.slots.len() {
                let generation = self.slots[i].generation;
                let h = Handle {
                    slot: i as u32,
                    generation,
                };
                i += 1;
                // (Decals have their own update.)
                let is = match &self.slots[i - 1].particle {
                    Some(p) => {
                        let c = &self.classes.list[p.class];
                        c.has(cflag::DECORATIVE) == decorative && !c.has(cflag::DECAL)
                    }
                    None => false,
                };
                if is {
                    self.update_particle(h, final_t, tick, host);
                }
            }
        }
        self.update_decals(tick, host);
    }

    fn update_particle(&mut self, h: Handle, final_t: f64, tick: u32, host: &mut dyn Host) {
        let Some(mut p) = self.take(h) else { return };
        if p.time >= final_t - 0.001 {
            self.put(h, p);
            return;
        }
        let classes = self.classes.clone();
        let c = &classes.list[p.class];
        // Lock to the linked particle: a broken link sends BrokenLink.
        if c.has(cflag::LOCK_TO_LINKED) && p.link.is_some() {
            match self.linked_position(&p) {
                Some(at) => {
                    p.position = at;
                    p.offset = Vec3::ZERO;
                    p.matrix = None;
                }
                None => {
                    p.link = None;
                    let t = p.time;
                    if self
                        .run(&mut p, h, event::BROKEN_LINK, t, 0.0, None, false, host)
                        .dead
                    {
                        self.die(p, h, t, host);
                        return;
                    }
                }
            }
        }
        // A contrail's link gone (`P3rDisplayClass_Contrail`, as it draws:
        // here, each tick): broken, and BrokenLink sent.
        if matches!(c.sprite_type, 5 | 6) && !c.contrail_emitter {
            if let Some(l) = p.link {
                let gone = self
                    .get(l)
                    .is_none_or(|q| q.flags & pflag::BREAK_LINKS_TO_ME != 0);
                if gone {
                    p.link = None;
                    let t = p.time;
                    if self
                        .run(&mut p, h, event::BROKEN_LINK, t, 0.0, None, false, host)
                        .dead
                    {
                        self.die(p, h, t, host);
                        return;
                    }
                }
            }
        }
        let mut last_hit: Option<(bool, u32, f64)> = None;
        let mut last_emitter = None;
        while p.time < final_t - 0.001 {
            let cur = p.time;
            let mut run_t = final_t;
            // Which emitter fires next, if any before the step's end.
            let mut emit = None;
            for (e, def) in c.emitters.iter().enumerate() {
                if let Some(t) = self.check_emitter(&p, e, def, cur, run_t) {
                    if t < run_t {
                        run_t = t;
                        emit = Some(e);
                    }
                }
            }
            // Collision along the step; a hit ends the step there and
            // cancels its emission.
            let mut hit = None;
            if (c.has(cflag::COLLIDE_ENV) || c.has(cflag::COLLIDE_CHAR))
                && p.flags & (pflag::STOPPED | pflag::CHOP) == 0
            {
                hit = self.check_collision(&p, c, cur, run_t, &mut last_hit, host);
                if let Some(k) = &hit {
                    run_t = k.t;
                    emit = None;
                }
            }
            let dt = (run_t - cur) as f32;
            // The update list.
            let out = self.run(&mut p, h, event::UPDATE, cur, dt, hit, hit.is_some(), host);
            if out.dead {
                self.die(p, h, run_t, host);
                return;
            }
            // Lifetime.
            if p.lifetime != 0.0 {
                p.lifetime -= dt;
                if p.lifetime < 1.0 / HZ as f32 {
                    p.lifetime = 0.0;
                    if p.flags & (pflag::FADE_OUT | pflag::CHOP) != 0
                        || c.events[event::LIFETIME].is_empty()
                    {
                        self.die(p, h, run_t, host);
                        return;
                    }
                    if self
                        .run(&mut p, h, event::LIFETIME, run_t, 0.0, None, false, host)
                        .dead
                    {
                        self.die(p, h, run_t, host);
                        return;
                    }
                }
            }
            // Orient to velocity.
            if c.has(cflag::ORIENTATION)
                && c.has(cflag::VELOCITY)
                && p.flags & pflag::ORIENT_TO_VELOCITY != 0
                && p.flags & pflag::VELOCITY_CHANGED != 0
                && p.velocity.length_squared() > 1e-12
            {
                p.orientation = emit::basis(p.velocity, p.orientation.z_axis);
            }
            // Emission.
            if let Some(e) = emit {
                p.time = run_t;
                self.emit(&mut p, h, e, run_t, 1, host);
                let def = &c.emitters[e];
                let st = &mut p.emitters[e];
                if st.count != DISABLED && def.flags & class::eflag::INCREASE_COUNT != 0 {
                    st.count += 1;
                    if st.count >= def.limit && def.flags & class::eflag::DEACTIVATE_AT_LIMIT != 0 {
                        st.count = DISABLED;
                    } else if st.count == DISABLED {
                        st.count = 0;
                    }
                }
                st.last_time = run_t;
            }
            // The collision's own event.
            if let Some(k) = hit {
                let ev = if p.flags & pflag::EXPLODE_ON_CONTACT != 0 {
                    event::EXPLODE
                } else if k.env {
                    event::COLLIDE_ENV
                } else {
                    event::COLLIDE_CHAR
                };
                if self
                    .run(&mut p, h, ev, run_t, 0.0, Some(k), true, host)
                    .dead
                {
                    self.die(p, h, run_t, host);
                    return;
                }
            }
            // (A bounce may have set the time back to the hit.)
            let mut next = run_t.max(p.time.min(run_t));
            if next - cur < 1e-4 && emit.is_some() && last_emitter == emit {
                next += 1.0 / HZ;
            }
            last_emitter = emit;
            p.time = next;
        }
        p.time = final_t;
        p.tick = tick;
        self.put(h, p);
    }

    /// When emitter `e` next fires, if it is on.
    fn check_emitter(
        &mut self,
        p: &Particle,
        e: usize,
        def: &class::Emitter,
        cur: f64,
        run_t: f64,
    ) -> Option<f64> {
        use class::Rate;
        let st = &p.emitters[e];
        if st.count == DISABLED {
            return None;
        }
        let soon = cur + 0.01 / HZ;
        let (rate, vals) = &def.rate;
        let mut last = st.last_time;
        // Not fired yet: at once — but a random or attractor emitter counts
        // from time 0 instead (`P3iCheckEmitter`), so its first is already
        // due. (Counting from now, a random interval longer than a tick
        // never came round: the steam and coolant of level 2's brain.)
        if last >= NEVER {
            match rate {
                Rate::Random | Rate::Attractor => last = 0.0,
                _ => return Some(soon),
            }
        }
        match rate {
            Rate::Continuous => {
                let interval = f64::from(self.eval_f(&vals[0], &p.vars).unwrap_or(0.0));
                Some((last + interval).max(soon))
            }
            Rate::Random => {
                let lo = f64::from(self.eval_f(&vals[0], &p.vars).unwrap_or(0.0));
                let hi = f64::from(self.eval_f(&vals[1], &p.vars).unwrap_or(0.0));
                if lo >= hi {
                    return Some((last + lo).max(soon));
                }
                let (a, b) = (last + lo, last + hi);
                if a >= run_t {
                    None
                } else if b <= cur {
                    Some(soon)
                } else {
                    let from = a.max(cur);
                    Some(from + f64::from(self.random()) * (b - from))
                }
            }
            Rate::Instant => Some(soon),
            Rate::Distance => {
                let d = self.eval_f(&vals[0], &p.vars).unwrap_or(0.0);
                match st.last.and_then(|l| self.get(l)) {
                    None if st.last.is_none() => Some(soon),
                    None => None,
                    Some(q) => (self.world_position(q).distance(self.world_position(p)) > d)
                        .then_some(soon),
                }
            }
            Rate::Attractor => None,
        }
    }

    /// The first collision along a particle's step from `cur` to `run_t`.
    fn check_collision(
        &mut self,
        p: &Particle,
        c: &Class,
        cur: f64,
        run_t: f64,
        last_hit: &mut Option<(bool, u32, f64)>,
        host: &mut dyn Host,
    ) -> Option<Collision> {
        let dt = run_t - cur;
        let ray = if c.has(cflag::VELOCITY) {
            p.velocity * dt as f32
        } else {
            Vec3::ZERO
        };
        let from = p.position;
        let mut best: Option<Collision> = None;
        if c.has(cflag::COLLIDE_ENV) && ray.length_squared() > 0.0 {
            if let Some(k) = host.ray_env(from, ray) {
                best = Some(Collision {
                    env: true,
                    point: from + ray * k.fraction,
                    normal: if k.normal.dot(ray) > 0.0 {
                        -k.normal
                    } else {
                        k.normal
                    },
                    dir: ray * k.fraction,
                    id: k.surface,
                    t: cur + dt * f64::from(k.fraction),
                    quad: k.quad,
                    part: None,
                });
            }
        }
        if c.has(cflag::COLLIDE_CHAR) && (ray.length_squared() > 0.0 || c.collision_radius > 0.0) {
            let limit = best.map_or(1.0, |b| ((b.t - cur) / dt.max(1e-9)) as f32);
            let r = ray * limit;
            if let Some(k) = host.ray_chars(from, r, c.collision_radius, owner::character(p.owner))
            {
                best = Some(Collision {
                    env: false,
                    point: from + r * k.fraction,
                    normal: k.normal,
                    dir: r * k.fraction,
                    id: k.who as u32,
                    t: cur + dt * f64::from(limit * k.fraction),
                    quad: None,
                    part: Some(k.part as u32),
                });
            }
            // The other physics in the way (`P3gPhysicsList`): the placed
            // objects, met by their spheres — a hit like a character's
            // (`CollideChar`), harming no one.
            let limit = best.map_or(1.0, |b| ((b.t - cur) / dt.max(1e-9)) as f32);
            let r = ray * limit;
            if let Some((f, normal, k)) = host.ray_objects(from, r, c.collision_radius) {
                best = Some(Collision {
                    env: false,
                    point: from + r * f,
                    normal,
                    dir: r * f,
                    id: OBJECT_HIT | k,
                    t: cur + dt * f64::from(limit * f),
                    quad: None,
                    part: None,
                });
            }
        }
        let mut k = best?;
        // One hit on the same thing per tick.
        if let Some((env, id, t)) = *last_hit {
            if env == k.env && id == k.id && k.t < t + 1.0 / HZ {
                k.t = t + 1.0 / HZ;
                if k.t > run_t {
                    return None;
                }
            }
        }
        *last_hit = Some((k.env, k.id, k.t));
        k.normal = if k.normal.length_squared() > 1e-12 {
            k.normal.normalize()
        } else {
            let y = p.orientation.y_axis;
            if y.length_squared() > 1e-12 {
                y.normalize()
            } else {
                Vec3::Y
            }
        };
        Some(k)
    }

    // --- attractors ---

    /// The candidates `p`'s class may be attracted to, with their scores
    /// (lower is better).
    fn attractor_candidates(&mut self, p: &Particle, host: &mut dyn Host) -> Vec<(usize, f32)> {
        use class::Iterator as It;
        let classes = self.classes.clone();
        let Some(a) = classes.list[p.class].attractor.as_ref() else {
            return Vec::new();
        };
        let me = owner::character(p.owner);
        let max_d = self.eval_f(&a.max_distance, &p.vars).unwrap_or(f32::MAX);
        let at = self.world_position(p);
        let forward = {
            let o = self.world_orientation(p).y_axis;
            if classes.list[p.class].has(cflag::ORIENTATION) && o.length_squared() > 1e-12 {
                o.normalize()
            } else {
                p.velocity.normalize_or_zero()
            }
        };
        let (max_angle, th_min, th_max, weight) = (
            self.eval_f(&a.max_angle, &p.vars).unwrap_or(180.0),
            self.eval_f(&a.theta_min, &p.vars).unwrap_or(0.0),
            self.eval_f(&a.theta_max, &p.vars).unwrap_or(0.0),
            self.eval_f(&a.theta_weight, &p.vars).unwrap_or(1.0),
        );
        let check_env = classes.list[p.class].has(cflag::ATTRACTOR_CHECK_ENV);
        let mut out = Vec::new();
        for t in host.targets() {
            let ok = match a.iterator {
                It::Character => Some(t.who) != me,
                It::HostileCharacter => me.is_none_or(|m| m != t.who && host.hostile(m, t.who)),
                It::AllCharacters => true,
                _ => false,
            };
            if !ok {
                continue;
            }
            let d = t.position.distance(at);
            if d > max_d {
                continue;
            }
            let mut score = d;
            if a.conical {
                let theta = forward
                    .angle_between((t.position - at).normalize_or_zero())
                    .to_degrees();
                if theta > max_angle {
                    continue;
                }
                if theta > th_max {
                    score *= weight;
                } else if theta > th_min {
                    score *= 1.0 + (weight - 1.0) * (theta - th_min) / (th_max - th_min).max(1e-6);
                }
            }
            if p.attractor == Some(t.who) {
                score *= 0.9;
            } else if check_env && !host.clear(at, t.position) {
                continue;
            }
            out.push((t.who, score));
        }
        out
    }

    /// Picks the attractor (`FindAttractor`); `delay` ticks until the next
    /// look. True if it newly found one.
    fn find_attractor(&mut self, p: &mut Particle, delay: u32, host: &mut dyn Host) -> bool {
        use class::Iterator as It;
        let classes = self.classes.clone();
        let Some(a) = classes.list[p.class].attractor.as_ref() else {
            return false;
        };
        if delay == 0 && p.attractor.is_some() {
            return false;
        }
        if matches!(a.iterator, It::EmittedTowards | It::Parent) {
            return false;
        }
        if self.tick < p.attractor_check {
            return false;
        }
        let had = p.attractor.is_some();
        let best = self
            .attractor_candidates(p, host)
            .into_iter()
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|x| x.0);
        p.attractor = best;
        if best.is_some() {
            p.flags |= pflag::HAS_ATTRACTOR;
            p.attractor_check = self.tick + delay;
        } else {
            p.flags &= !pflag::HAS_ATTRACTOR;
            p.attractor_check = self.tick + delay + (self.random() * 10.0) as u32;
        }
        !had && best.is_some()
    }

    /// The attractor's position and velocity, if it still exists.
    fn attractor_of(&self, p: &Particle, host: &dyn Host) -> Option<Target> {
        let who = p.attractor?;
        host.targets().into_iter().find(|t| t.who == who)
    }
}

#[cfg(test)]
mod tests;
