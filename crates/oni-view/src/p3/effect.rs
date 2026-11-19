//! Effects (`docs/PARTICLES.md`, *Effects* and *Decals*; `P3rCreateEffect`):
//! a particle made at a place for a cause — an impact effect's sparks and
//! flashes, a collision effect, a level's decal — placed and turned by its
//! specification, and, for a decal class, the decal it lays on the level;
//! and the decals' own lives (`P3iUpdateDecalClass`): they only age, and
//! fade out when their time is up or their class has too many.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use glam::{Mat3, Mat4, Vec3};

use super::class::{cflag, ClassId};
use super::{emit, pflag, Handle, Host, Particle, System, Val, HZ};
use crate::decal::{self, dflag, Mesh};

/// How an effect's particle is turned (`P3tEnumCollisionOrientation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// As its parent.
    Unchanged,
    /// Its parent's, turned back.
    Reversed,
    /// Its parent's flight, bounced off the surface.
    Reflected,
    /// Along the surface's normal.
    Normal,
}

impl Orientation {
    pub fn from_name(s: &str) -> Orientation {
        match s {
            "reversed" => Orientation::Reversed,
            "reflected" => Orientation::Reflected,
            "normal" => Orientation::Normal,
            _ => Orientation::Unchanged,
        }
    }

    fn from_enum(e: u32) -> Orientation {
        [Orientation::Unchanged, Orientation::Reversed, Orientation::Reflected, Orientation::Normal].get(e as usize).copied().unwrap_or(Orientation::Unchanged)
    }
}

/// Where an effect's particle goes (`P3tEffectLocationType`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Location {
    Default,
    /// Off the surface by this, along its normal.
    CollisionOffset(f32),
    /// (Never implemented in the original: its place is left unset.)
    FindWall,
    /// A decal laid on the surface struck: kept for good (a level's), and
    /// turned at random.
    Decal { is_static: bool, random_rotation: bool },
    /// Riding what it struck.
    AttachedToObject,
}

/// What to make (`P3tEffectSpecification`).
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub class: ClassId,
    pub orientation: Orientation,
    pub location: Location,
}

/// Why it is made (`P3tEffectCauseType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    None,
    /// A particle struck the level (on this quad, when known).
    HitWall(Option<u32>),
    /// A particle struck a character or object.
    HitPhysics,
    /// A blow landed.
    MeleeHit,
    /// A cutscene cleared the particles.
    CutsceneKilled,
}

/// Where and why (`P3tEffectData`).
#[derive(Clone, Debug)]
pub struct Data {
    pub tick: u32,
    pub owner: u32,
    pub position: Vec3,
    pub velocity: Vec3,
    /// The surface's normal (for causes with a surface).
    pub normal: Vec3,
    /// A decal's up (a level's decal: its object's Z).
    pub upvector: Vec3,
    /// Multiplies the decal class's size.
    pub decal_scale: [f32; 2],
    pub parent_velocity: Option<Vec3>,
    pub parent_orientation: Option<Mat3>,
    /// The parent's flags (temporary, cut by a cutscene).
    pub parent_flags: u32,
    pub cause: Cause,
    pub attach: Option<Rc<Cell<Mat4>>>,
}

impl Data {
    pub fn new(tick: u32, position: Vec3) -> Data {
        Data {
            tick,
            owner: 0,
            position,
            velocity: Vec3::ZERO,
            normal: Vec3::ZERO,
            upvector: Vec3::ZERO,
            decal_scale: [0.0; 2],
            parent_velocity: None,
            parent_orientation: None,
            parent_flags: 0,
            cause: Cause::None,
            attach: None,
        }
    }
}

/// A live decal: its particle, mesh, texture, opacity (0–255).
#[derive(Clone, Debug)]
pub struct Decal {
    pub particle: Handle,
    pub mesh: Rc<Mesh>,
    pub texture: Rc<str>,
    pub alpha: u16,
    /// (Set, but the original's drawing never shows it: the corners' light
    /// replaces it.)
    #[allow(dead_code)]
    pub tint: u32,
}

/// The dynamic decals' memory (`P3cDynamicDecalBufferSize`): addresses
/// up to one short of it, and a ring of this many block records (one per
/// smallest decal, and ten more).
pub const DYNAMIC_BYTES: u32 = 100 * 1024;
const MAX_ADDRESS: u32 = DYNAMIC_BYTES - 1;
const BLOCKS: usize = DYNAMIC_BYTES as usize / 132 + 10;

/// One block of the dynamic decals' ring: where, whose (none once its
/// decal is gone — its space stays taken until the ring comes round).
#[derive(Clone, Copy, Debug)]
struct Block {
    address: u32,
    owner: Option<Handle>,
}

/// The decals: static ones (a level's) and dynamic ones (bullet holes,
/// burns), each oldest first; the dynamic ones live in a ring of memory
/// that, when full, throws out the oldest (their particles die at once).
#[derive(Default)]
pub struct Decals {
    pub fixed: Vec<Decal>,
    pub dynamic: Vec<Decal>,
    /// The ring's blocks, oldest first (a freed one keeps its place), and
    /// the newest's place and size (none after wrapping).
    ring: VecDeque<Block>,
    newest: Option<(u32, u32)>,
}

impl Decals {
    pub fn iter(&self) -> impl Iterator<Item = &Decal> {
        self.fixed.iter().chain(self.dynamic.iter())
    }

    fn find(&mut self, h: Handle) -> Option<&mut Decal> {
        self.fixed.iter_mut().chain(self.dynamic.iter_mut()).find(|d| d.particle == h)
    }

    fn remove(&mut self, h: Handle) {
        self.fixed.retain(|d| d.particle != h);
        self.dynamic.retain(|d| d.particle != h);
        for b in self.ring.iter_mut().filter(|b| b.owner == Some(h)) {
            b.owner = None;
        }
    }

    /// Room for `size` bytes in the ring (`lrar_allocate`, the game's
    /// `lrar_cache2`): right after the newest block, or from the start when
    /// it would run off the end; the oldest blocks it would overlap — or
    /// every oldest one while all the records are taken — are thrown out.
    /// Returns where, and the owners thrown out.
    fn allocate(&mut self, size: u32) -> (u32, Vec<Handle>) {
        let size = (size + 3) & !3;
        let mut purged = Vec::new();
        loop {
            let address = self.newest.map_or(0, |(a, s)| a + s);
            while let Some(b) = self.ring.front() {
                if !(self.ring.len() >= BLOCKS || (address <= b.address && address + size > b.address)) {
                    break;
                }
                purged.extend(self.ring.pop_front().and_then(|b| b.owner));
            }
            if address + size > MAX_ADDRESS {
                // Off the end: wrap.
                self.newest = None;
                continue;
            }
            self.ring.push_back(Block { address, owner: None });
            self.newest = Some((address, size));
            return (address, purged);
        }
    }

}

#[cfg(test)]
impl Decals {
    /// [`Decals::allocate`] with owners standing in for particles (by
    /// number), for tests.
    pub(super) fn allocate_for_test(&mut self, size: u32, owner: u32) -> (u32, Vec<u32>) {
        let (at, purged) = self.allocate(size);
        if let Some(b) = self.ring.back_mut() {
            b.owner = Some(Handle { slot: owner, generation: 1 });
        }
        (at, purged.into_iter().map(|h| h.slot).collect())
    }
}

impl System {
    /// `P3rCreateEffect`: makes `spec`'s particle for `data`. Returns it, or
    /// `None` if none was made (or it died at once).
    pub fn create_effect(&mut self, spec: &Spec, data: &Data, host: &mut dyn Host) -> Option<Handle> {
        let classes = self.classes.clone();
        let c = &classes.list[spec.class];
        let by_cutscene = data.cause == Cause::CutsceneKilled || data.parent_flags & pflag::CUTSCENE_KILLED != 0;
        if by_cutscene && (c.has(cflag::EXPIRE_ON_CUTSCENE) || c.has(cflag::DIE_ON_CUTSCENE)) {
            return None;
        }
        let mut p = self.create(spec.class, data.tick);
        p.flags |= data.parent_flags & pflag::TEMPORARY;
        if by_cutscene {
            p.flags |= pflag::CUTSCENE_KILLED;
        }
        let mut position = match spec.location {
            Location::CollisionOffset(offset) => data.position + data.normal * offset,
            _ => data.position,
        };
        if c.has(cflag::ORIENTATION) {
            p.orientation = orientation(spec.orientation, data);
        }
        p.velocity = data.velocity;
        p.owner = data.owner;
        if let Some(m) = &data.attach {
            // Made local to what it rides.
            position = m.get().inverse().transform_point3(position);
            p.matrix = Some(m.clone());
        }
        p.position = position;
        let decal = if c.has(cflag::DECAL) { Some(self.lay_decal(&mut p, spec, data, host)?) } else { None };
        let h = self.created(p, f64::from(data.tick) / HZ, host)?;
        if let Some((mesh, is_static, alpha, tint)) = decal {
            let d = Decal { particle: h, mesh: Rc::new(mesh), texture: c.instance.as_str().into(), alpha, tint };
            if is_static {
                self.decals.fixed.push(d);
            } else {
                self.decals.dynamic.push(d);
                if let Some(b) = self.decals.ring.back_mut() {
                    b.owner = Some(h);
                }
            }
        }
        Some(h)
    }

    /// The decal block of `P3rCreateEffect`: builds `p`'s decal. `None` if
    /// it can't be made (the particle is not made either).
    fn lay_decal(&mut self, p: &mut Particle, spec: &Spec, data: &Data, host: &mut dyn Host) -> Option<(Mesh, bool, u16, u32)> {
        let (Location::Decal { is_static, random_rotation }, Cause::HitWall(quad)) = (spec.location, data.cause) else {
            eprintln!("particles: tried to create decal that wasn't location = decal, cause = wall ({})", self.classes.list[spec.class].name);
            return None;
        };
        let classes = self.classes.clone();
        let c = &classes.list[spec.class];
        let xscale = self.eval_f(&c.scale, &p.vars).unwrap_or(1.0);
        let yscale = match &c.y_scale {
            Some(y) => self.eval_f(y, &p.vars).unwrap_or(1.0),
            None => xscale,
        };
        let wrap = self.eval_f(&c.decal_wrap, &p.vars).unwrap_or(60.0).to_radians();
        let direction = data.parent_velocity.or(data.parent_orientation.map(|o| o.y_axis));
        let rotation = random_rotation.then(|| self.random() * std::f32::consts::TAU);
        let (up, right) = decal::frame(data.normal, data.upvector, rotation)?;
        let alpha = self.eval_f(&c.alpha, &p.vars).unwrap_or(0.0).clamp(0.0, 1.0);
        let tint = match self.eval(&c.tint, &p.vars) {
            Some(Val::U(t)) => t,
            _ => 0xFFFF_FFFF,
        };
        let mut flags = 0;
        if is_static {
            flags |= dflag::STATIC;
        }
        if c.has(cflag::DECAL_CULL_BACKFACING) {
            flags |= dflag::CULL_BACK_FACING;
        }
        if c.has(cflag::DECAL_FULL_BRIGHT) {
            flags |= dflag::FULL_BRIGHT;
        }
        let params = decal::Params {
            flags,
            wrap,
            direction,
            position: data.position,
            fwd: data.normal,
            up,
            right,
            xscale: xscale * data.decal_scale[0],
            yscale: yscale * data.decal_scale[1],
        };
        let mesh = host.build_decal(quad, &params)?;
        if !is_static {
            let (_, purged) = self.decals.allocate(mesh.bytes() as u32);
            // (Thrown out of memory: their particles die there and then.)
            for h in purged {
                self.decals.remove(h);
                self.kill(h, host);
            }
        }
        p.flags |= if is_static { pflag::DECAL_STATIC } else { pflag::DECAL };
        Some((mesh, is_static, (alpha * 255.0).round_ties_even() as u16, tint))
    }

    /// A dying particle's decal goes with it.
    pub(super) fn drop_decal(&mut self, p: &Particle, h: Handle) {
        if p.flags & (pflag::DECAL | pflag::DECAL_STATIC) != 0 {
            self.decals.remove(h);
        }
    }

    /// `P3iUpdateDecalClass` for every decal class: each decal ages; past
    /// its lifetime, or beyond its class's count (oldest first), it fades,
    /// then dies.
    pub(super) fn update_decals(&mut self, tick: u32, host: &mut dyn Host) {
        let final_t = f64::from(tick) / HZ;
        let classes = self.classes.clone();
        let mut by_class: std::collections::BTreeMap<ClassId, Vec<(u64, Handle)>> = std::collections::BTreeMap::new();
        for (h, p) in self.iter() {
            if classes.list[p.class].has(cflag::DECAL) {
                by_class.entry(p.class).or_default().push((p.serial, h));
            }
        }
        for (class, mut list) in by_class {
            list.sort_by_key(|(s, _)| *s);
            let c = &classes.list[class];
            let max = c.max_decals as usize;
            let overflow = if max == 0 || list.len() < max { 0 } else { list.len() - max };
            for (itr, &(_, h)) in list.iter().enumerate() {
                let Some(mut p) = self.take(h) else { continue };
                let delta = (final_t - p.time) as f32;
                let mut dead = false;
                if p.lifetime != 0.0 {
                    p.lifetime -= delta;
                    if p.lifetime < 1.0 / HZ as f32 {
                        p.lifetime = 0.0;
                        dead = self.decal_fade(&mut p, class);
                    }
                }
                if !dead && p.flags & pflag::FADE_OUT == 0 && itr < overflow {
                    dead = self.decal_fade(&mut p, class);
                }
                if dead {
                    let t = p.time;
                    self.die(p, h, t, host);
                    continue;
                }
                if p.flags & pflag::FADE_OUT != 0 {
                    let alpha = (255.0 * p.lifetime / p.fadeout_time).round_ties_even() as u16;
                    if let Some(d) = self.decals.find(h) {
                        d.alpha = alpha;
                    }
                }
                p.tick = tick;
                p.time = final_t;
                self.put(h, p);
            }
        }
    }

    /// `P3iDecalFade`: starts a decal fading from its (re-rolled) opacity
    /// over its class's fade frames; true if it should die now instead.
    fn decal_fade(&mut self, p: &mut Particle, class: ClassId) -> bool {
        let classes = self.classes.clone();
        let c = &classes.list[class];
        if p.flags & pflag::FADE_OUT != 0 || c.decal_fade_frames == 0 {
            return true;
        }
        let Some(alpha) = self.eval_f(&c.alpha, &p.vars) else { return false };
        if alpha <= 0.0 {
            return true;
        }
        p.lifetime = f32::from(c.decal_fade_frames) / HZ as f32;
        p.fadeout_time = p.lifetime / alpha;
        p.flags |= pflag::FADE_OUT;
        false
    }

    /// The collision-effect action's particle (`P3.c`'s CollisionEffect):
    /// `class` at the collision (or where the parent is), off the wall by
    /// `offset`, turned by `orient` (an enum parameter; else along the
    /// normal); a decal class always lays a dynamic, randomly turned decal.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn collision_effect(
        &mut self,
        parent: &Particle,
        class: ClassId,
        offset: f32,
        orient: Option<u32>,
        hit: Option<super::Collision>,
        quad: Option<u32>,
        t: f64,
        host: &mut dyn Host,
    ) {
        let parent_class = &self.classes.list[parent.class];
        let has_v = parent_class.has(cflag::VELOCITY);
        let has_o = parent_class.has(cflag::ORIENTATION);
        let mut data = Data::new((t * HZ).round() as u32, self.world_position(parent));
        data.owner = parent.owner;
        data.parent_flags = parent.flags;
        data.parent_velocity = has_v.then_some(parent.velocity);
        data.parent_orientation = has_o.then(|| self.world_orientation(parent));
        let mut orientation = orient.map_or(Orientation::Normal, Orientation::from_enum);
        let mut location = if offset != 0.0 { Location::CollisionOffset(offset) } else { Location::Default };
        match hit {
            Some(k) => {
                data.position = k.point;
                data.normal = k.normal;
                data.cause = if k.env { Cause::HitWall(quad) } else { Cause::HitPhysics };
            }
            None => {
                data.cause = if parent.flags & pflag::CUTSCENE_KILLED != 0 { Cause::CutsceneKilled } else { Cause::None };
                if matches!(orientation, Orientation::Reflected | Orientation::Normal) {
                    orientation = Orientation::Unchanged;
                }
            }
        }
        if self.classes.list[class].has(cflag::DECAL) {
            location = Location::Decal { is_static: false, random_rotation: true };
            orientation = Orientation::Normal;
            data.decal_scale = [1.0, 1.0];
        }
        self.create_effect(&Spec { class, orientation, location }, &data, host);
    }
}

/// An effect particle's orientation (`P3rCreateEffect`): reflected and
/// normal need a cause (and reflected a parent velocity), else they fall
/// back to the next.
fn orientation(o: Orientation, data: &Data) -> Mat3 {
    let cause = !matches!(data.cause, Cause::None | Cause::CutsceneKilled);
    let up = data.parent_orientation.map_or(Vec3::Y, |m| m.z_axis);
    let unchanged = || match (data.parent_orientation, data.parent_velocity) {
        (Some(m), _) => m,
        (None, Some(v)) => emit::basis(v, Vec3::Y),
        _ => Mat3::IDENTITY,
    };
    match o {
        Orientation::Reflected | Orientation::Normal if cause => {
            if o == Orientation::Reflected {
                let n = data.normal;
                if let Some(v) = data.parent_velocity.filter(|_| n.dot(n) > 0.001) {
                    return emit::basis(v - n * (2.0 * v.dot(n) / n.dot(n)), up);
                }
            }
            emit::basis(data.normal, up)
        }
        Orientation::Reversed => match (data.parent_orientation, data.parent_velocity) {
            (Some(m), _) => Mat3::from_cols(m.x_axis, -m.y_axis, -m.z_axis),
            (None, Some(v)) => emit::basis(-v, Vec3::Y),
            _ => Mat3::IDENTITY,
        },
        _ => unchanged(),
    }
}
