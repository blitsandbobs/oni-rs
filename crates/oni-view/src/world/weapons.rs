//! The world's guns and particles (`docs/WEAPONS.md`): placed and dropped
//! guns, the guns in characters' hands — aimed, fired, reloaded — and the
//! particle system their shots fly through, which reaches back into the
//! world through [`ShotHost`] for collision and damage.

use std::path::Path;
use std::rc::Rc;

use glam::{Mat3, Mat4, Vec3};

use super::World;
use crate::anim::input::key;
use crate::anim::reaction;
use crate::anim::types::ty;
use crate::fighter::Fighter;
use crate::p3::{self, CharHit, EnvHit, Host, Target};
use crate::weapons::{flag, Report, Weapon, WeaponClasses};

/// Picking up: how near (XZ) and how far below / above the feet a gun may
/// be to start reaching for it.
const PICKUP_RADIUS: f32 = 8.0;
const PICKUP_BELOW: f32 = 3.0;
const PICKUP_ABOVE: f32 = 18.0;

/// Damage kinds (`P3tEnumDamageType`).
mod damage {
    pub const NORMAL: u32 = 0;
    pub const MINOR_STUN: u32 = 1;
    pub const MAJOR_STUN: u32 = 2;
    pub const MINOR_KNOCKDOWN: u32 = 3;
    pub const MAJOR_KNOCKDOWN: u32 = 4;
    pub const BLOWN_UP: u32 = 5;
    pub const PICK_UP: u32 = 6;
}

/// Something a character can pick up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Weapon(usize),
    Powerup(usize),
}

/// Animation types of the gun clips.
mod clip {
    pub const PICKUP_OBJECT: u16 = 157;
    pub const PICKUP_PISTOL: u16 = 158;
    pub const PICKUP_RIFLE: u16 = 159;
    pub const HOLSTER: u16 = 160;
    pub const DRAW_PISTOL: u16 = 161;
    pub const DRAW_RIFLE: u16 = 162;
    pub const PICKUP_OBJECT_MID: u16 = 227;
    pub const PICKUP_PISTOL_MID: u16 = 228;
    pub const PICKUP_RIFLE_MID: u16 = 229;

    pub fn is_pickup(t: u16) -> bool {
        (157..=159).contains(&t) || (227..=229).contains(&t)
    }
    pub fn is_reload(t: u16) -> bool {
        (164..=171).contains(&t)
    }
}

/// Finishing a pickup at the clip's action frame: the wider reach.
const PICKUP_FINISH_RADIUS: f32 = 16.0;
const PICKUP_FINISH_BELOW: f32 = 6.0;
const PICKUP_FINISH_ABOVE: f32 = 24.0;
/// An item more than this above the feet takes the mid-height pickup clip.
const PICKUP_MID: f32 = 9.0;

/// Powerup kinds (`WPtPowerupType`), by their script names.
pub const POWERUPS: [&str; 6] = ["ammo", "cell", "hypo", "shield", "invis", "lsi"];
/// The data's names for them (`items.json`).
const POWERUP_DATA: [&str; 6] = [
    "ammoBallistic",
    "ammoEnergy",
    "hypo",
    "shieldBelt",
    "invisibility",
    "lsi",
];
/// How much one gives by default, and the most one can carry (ammo, cells
/// and hypos add up; shields and invisibility top up to the amount).
const POWERUP_DEFAULT: [u16; 6] = [1, 1, 1, 100, 1800, 1];
const CARRY_MAX: u16 = 6;

/// A powerup lying in the level.
#[derive(Clone, Copy, Debug)]
pub struct Powerup {
    pub kind: usize,
    pub amount: u16,
    pub position: Vec3,
    /// Which way it lies.
    pub rotation: f32,
    /// Its physics while it falls or slides (a dropped one,
    /// `WPrPowerup_AddPhysics`); gone once it has rested 120 ticks.
    pub body: Option<crate::objphys::Body>,
    rest: u32,
}

impl Powerup {
    fn new(kind: usize, amount: u16, position: Vec3) -> Powerup {
        Powerup {
            kind,
            amount,
            position,
            rotation: 0.0,
            body: None,
            rest: 0,
        }
    }
}

/// A powerup's looks (`game_settings.json` `powerups`): its model and its
/// physics sphere's radius.
#[derive(Clone, Debug, Default)]
pub struct PowerupLook {
    pub model: Option<std::path::PathBuf>,
    pub radius: f32,
    /// The glow lying under it (`WPrPowerup_Draw`): its texture, size, and
    /// height above the powerup's place.
    pub glow: Option<std::path::PathBuf>,
    pub glow_size: [f32; 2],
    pub glow_height: f32,
}

/// A loose thing (to leave itself out of what it meets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Loose {
    Weapon(usize),
    Powerup(usize),
}

/// A loose powerup's friction, the speed under which it counts as still
/// (`5 / 60` a tick), and how long it rests before its physics goes
/// (`WPcPowerupPhysicsTimer`).
const POWERUP_STATIC_FRICTION: f32 = 0.0025;
const POWERUP_DYNAMIC_FRICTION: f32 = 0.75;
const POWERUP_STILL: f32 = 5.0 / 60.0;
const POWERUP_REST: u32 = 120;
/// A glow's size without settings (`WPcPowerupDefaultGlowSize`).
const POWERUP_GLOW_SIZE: f64 = 5.0;

/// Guns, their classes and the particles.
pub struct Armory {
    pub classes: Rc<WeaponClasses>,
    pub particles: p3::System,
    pub weapons: Vec<Weapon>,
    pub powerups: Vec<Powerup>,
    /// Each powerup type's looks (by [`POWERUPS`] order).
    pub powerup_looks: Vec<PowerupLook>,
    /// Each fighter's fire buttons last tick (for presses).
    last_fire: Vec<u32>,
}

impl Default for Armory {
    fn default() -> Armory {
        Armory {
            classes: Rc::new(WeaponClasses::default()),
            particles: p3::System::new(Rc::new(p3::Classes::default())),
            weapons: Vec::new(),
            powerups: Vec::new(),
            powerup_looks: Vec::new(),
            last_fire: Vec::new(),
        }
    }
}

/// What the particles see of the world while they move.
pub struct ShotHost<'a> {
    pub fighters: &'a mut [Fighter],
    /// Each fighter's posed bones, worked out when first needed.
    bones: Vec<Option<Vec<Mat4>>>,
    sight: Option<&'a crate::quads::Quads>,
    teams: Vec<crate::ai::perception::Team>,
    now: u64,
    /// Noises the hits make (gunshot impacts), for the world to spread.
    pub sounds: Vec<(Vec3, f32, Option<usize>)>,
    /// Glass: each pane's quad and its
    /// (broken, middle, reach), and the ones shots broke.
    pane_quads: Vec<u32>,
    panes: Vec<(bool, Vec3, f32)>,
    /// The doors' leaves (what shots hit besides the level).
    pub doors: &'a [crate::chrphys::DoorBox],
    /// Panes broken: which, how hard, what blew them out.
    pub broke: Vec<(usize, f32, Option<p3::glass::Blast>)>,
    /// Each fighter's extra spread (degrees) on its shots, and turrets'.
    inaccuracy: Vec<f32>,
    turret_spread: f32,
    /// The level's quads (for decals).
    pub quads: Option<&'a crate::quads::Quads>,
    /// Rolls for a boss shield's fractions (seeded by the tick).
    rng: u32,
    /// The placed objects particles may meet: each one's collision sphere.
    pub objects: Vec<(Vec3, f32)>,
}

impl<'a> ShotHost<'a> {
    /// Each fighter's extra spread, and the turrets'.
    pub(super) fn set_spread(&mut self, fighters: Vec<f32>, turrets: f32) {
        self.inaccuracy = fighters;
        self.turret_spread = turrets;
    }

    pub(super) fn new(
        fighters: &'a mut [Fighter],
        sight: Option<&'a crate::quads::Quads>,
        now: u64,
    ) -> ShotHost<'a> {
        let teams = fighters.iter().map(|f| f.team()).collect();
        let n = fighters.len();
        ShotHost {
            fighters,
            bones: vec![None; n],
            sight,
            teams,
            now,
            sounds: Vec::new(),
            pane_quads: Vec::new(),
            panes: Vec::new(),
            doors: &[],
            broke: Vec::new(),
            inaccuracy: Vec::new(),
            turret_spread: 0.0,
            quads: None,
            rng: (now as u32).wrapping_mul(0x9E37_79B9) | 1,
            objects: Vec::new(),
        }
    }

    fn bones_of(&mut self, i: usize) -> &[Mat4] {
        if self.bones[i].is_none() {
            self.bones[i] = Some(self.fighters[i].bones());
        }
        self.bones[i].as_deref().unwrap_or_default()
    }

    /// `InflictDamage`: a hit or a blast reaching character `who` — at `at`,
    /// `radius` across (a shot's 4, a blast's own). True when the particle
    /// already hit it (a duplicate).
    #[allow(clippy::too_many_arguments)]
    fn inflict(
        &mut self,
        who: usize,
        at: Vec3,
        radius: f32,
        dir: Vec3,
        kind: u32,
        damage: i32,
        stun: i32,
        knockback: f32,
        stamp: Option<u64>,
        owner: u32,
    ) -> bool {
        let attacker = p3::owner::character(owner)
            .and_then(|a| Some((a, self.fighters.get(a)?.feet(), self.teams.get(a).copied())));
        // A shield lights up where it is struck (`ONrCharacter_FindParts`:
        // the parts whose bounds come within reach).
        let shield_mask = match self.fighters.get(who) {
            Some(f)
                if f.alive()
                    && (f.inventory.shield > 0 || f.boss_shield || f.has_super_shield()) =>
            {
                let mut spheres = [None; crate::world::magic::PARTS];
                for p in &f.character.parts {
                    if let (Some(s), Some(slot)) = (p.bound_sphere, spheres.get_mut(p.bone)) {
                        *slot = Some(s);
                    }
                }
                let bones = self.bones_of(who).to_vec();
                crate::world::magic::parts_near(&bones, &spheres, at, radius)
            }
            _ => 0,
        };
        let Some(f) = self.fighters.get_mut(who) else {
            return false;
        };
        if !f.alive() {
            return false;
        }
        if let Some(s) = stamp {
            if f.last_particle == Some(s) {
                return true;
            }
            f.last_particle = Some(s);
        }
        // Any shield — a belt's, a boss's, a super shield — flares where
        // struck.
        let super_shield = f.has_super_shield();
        if f.inventory.shield > 0 || f.boss_shield || super_shield {
            f.effects.shield_hit(shield_mask, f.boss_shield);
        }
        // A super shield (or a script's immunity) takes no harm at all.
        if super_shield || f.weapon_immune {
            return false;
        }
        // Its class shrugs off a share of the stun and knockback (not the
        // damage) of this kind of hit (`damageResistance`), unless a belt
        // takes it.
        let (stun, knockback) = match f.motion.tuning.damage_resistance.get(kind as usize) {
            Some(&r) if r > 0.0 && f.inventory.shield == 0 => {
                let keep = (1.0 - r).max(0.0);
                ((stun.max(0) as f32 * keep).round() as i32, knockback * keep)
            }
            _ => (stun, knockback),
        };
        // A boss shield takes its class's share off the hit, its knockback
        // and its stun (a fraction left over counting by chance).
        let (damage, stun, knockback) = if f.boss_shield && f.inventory.shield == 0 {
            let keep = 1.0 - f.motion.tuning.boss_shield;
            let mut chance = |x: f32| {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 17;
                self.rng ^= self.rng << 5;
                let whole = x.max(0.0).floor();
                whole as i32 + i32::from((self.rng & 0xFFFF) as f32 / 65536.0 < x - whole)
            };
            (
                chance(damage as f32 * keep),
                chance(stun as f32 * keep),
                knockback * keep,
            )
        } else {
            (damage, stun, knockback)
        };
        let Some(f) = self.fighters.get_mut(who) else {
            return false;
        };
        if f.inventory.shield > 0 {
            // A shield belt soaks it all (the excess is lost): no hurt, no
            // reaction — only where it came from, on the hit ring.
            f.inventory.shield = f.inventory.shield.saturating_sub(damage.max(0) as u16);
            f.hits[crate::world::hit_quadrant(f.facing.aim(), dir)] = 255;
            return false;
        }
        let physical = if kind == damage::PICK_UP { 0 } else { damage };
        // Picked up (`WPrHitChar`): enough of it (a share of 100 by damage)
        // and it is lifted toward the shooter, the hit's stun more ticks of
        // it (at most 60).
        if kind == damage::PICK_UP && f.alive() && !f.is_unstoppable() {
            let lifted = f.lift.timer > 0 || {
                f.lift.percent += damage as f32;
                f.lift.percent >= 100.0
            };
            if let (true, Some((a, _, _))) = (lifted, attacker) {
                f.lift.timer = (f.lift.timer + stun.max(0) as u16).min(60);
                f.lift.owner = Some(a);
            }
        }
        f.last_impact = dir * (damage as f32).clamp(5.0, 15.0);
        f.take_damage(physical, dir * knockback);
        // Facing the shot, or turned away from it: the reaction's side.
        let from = (-dir).x.atan2(-dir.z);
        let behind = {
            let d = crate::angle::wrap_pi(from - f.facing.facing);
            d.abs() > std::f32::consts::FRAC_PI_2
        };
        let side = |t: u16| if behind { reaction::from_behind(t) } else { t };
        let stun = stun.max(0) as u16;
        let current = f.motion.current().anim_type;
        if !f.is_unstoppable() && f.alive() {
            match kind {
                damage::NORMAL => f.minor_hit_reaction(),
                damage::MINOR_STUN if stun > 0 => {
                    f.pending_reaction = ty::STUN;
                    f.motion.dizzy_stun = f.motion.dizzy_stun.max(stun);
                }
                damage::MAJOR_STUN if stun > 0 => {
                    if !matches!(current, ty::STAGGER | ty::STAGGER_BEHIND | ty::STUN) {
                        f.pending_reaction = if behind {
                            ty::STAGGER_BEHIND
                        } else {
                            ty::STAGGER
                        };
                    }
                    f.motion.stagger_stun = 30;
                    f.motion.dizzy_stun = f.motion.dizzy_stun.max(stun);
                }
                damage::MINOR_KNOCKDOWN => {
                    let k = f.knockback.length();
                    if k > 4.1 {
                        if !reaction::is_knockdown(current) {
                            f.pending_reaction = side(ty::KNOCKDOWN_HEAD);
                        }
                    } else if k > 2.2 {
                        if !reaction::is_victim(current) {
                            f.pending_reaction = if behind {
                                ty::STAGGER_BEHIND
                            } else {
                                ty::STAGGER
                            };
                        }
                        f.motion.stagger_stun = 22;
                    }
                }
                damage::MAJOR_KNOCKDOWN => f.pending_reaction = side(ty::KNOCKDOWN_HEAD),
                damage::BLOWN_UP => {
                    f.pending_reaction = if behind {
                        ty::BLOWNUP_BEHIND
                    } else {
                        ty::BLOWNUP
                    }
                }
                _ => {}
            }
        }
        if let (Some((a, feet, team)), Some(p)) = (attacker, f.perception.as_mut()) {
            use crate::ai::perception::{Stimulus, Strength};
            p.perceive(
                Some(a),
                team,
                Stimulus::HitWeapon,
                Strength::Definite,
                feet,
                damage,
                self.now,
            );
        }
        let f = &self.fighters[who];
        eprintln!(
            "shot: fighter {who} takes {physical} (kind {kind}, hp {}) reaction {}",
            f.hp, f.pending_reaction
        );
        false
    }
}

/// Where a ray `from → from + ray` enters the box `lo..hi`: the fraction
/// along it and the face's outward axis (box space).
fn ray_box(from: Vec3, ray: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, Vec3)> {
    let (t, normal) = crate::geom::segment_box(from, ray, Vec3::ZERO, crate::geom::AXES, lo, hi)?;
    Some((t, normal.unwrap_or(Vec3::ZERO)))
}

/// Where a ray first touches a sphere (fraction along it; 0 if it starts
/// inside).
fn ray_sphere(from: Vec3, ray: Vec3, centre: Vec3, r: f32) -> Option<f32> {
    match crate::geom::ray_sphere(from, ray, centre, r)? {
        crate::geom::Entry::Inside => Some(0.0),
        crate::geom::Entry::At(t) => (t <= 1.0).then_some(t),
    }
}

impl ShotHost<'_> {
    /// Breaks pane `k` now (later shots this tick fly through it), `damage`
    /// hard, blown out by `blast`.
    fn break_pane(&mut self, k: usize, damage: f32, blast: Option<p3::glass::Blast>) {
        self.panes[k].0 = true;
        self.broke.push((k, damage, blast));
        self.sounds.push((self.panes[k].1, 300.0, None));
    }
}

impl Host for ShotHost<'_> {
    fn super_amount(&self, who: usize) -> f32 {
        self.fighters.get(who).map_or(0.0, |f| f.super_amount)
    }

    fn ray_objects(&mut self, from: Vec3, ray: Vec3, radius: f32) -> Option<(f32, Vec3, u32)> {
        // (A point against each sphere; a sphere against it grown by its
        // radius — `PHrCollision_Point_SphereTree` / `…SphereTree_SphereTree`.)
        self.objects
            .iter()
            .enumerate()
            .filter_map(|(k, &(c, r))| {
                p3::ray_sphere_entry(from, ray, c, r + radius).map(|(t, n)| (t, n, k as u32))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    fn part_matrix(&mut self, who: usize, part: usize) -> Option<Rc<std::cell::Cell<Mat4>>> {
        self.fighters.get_mut(who)?.part_matrix(part)
    }

    fn body_point(
        &mut self,
        who: usize,
        surface: bool,
        radius: f32,
        r: [f32; 3],
    ) -> Option<(Rc<std::cell::Cell<Mat4>>, Vec3, Option<glam::Mat3>)> {
        let f = self.fighters.get_mut(who).filter(|f| !f.gone)?;
        let (part, point, out) = f.body_point(surface, radius, r)?;
        Some((f.part_matrix(part)?, point, out))
    }

    fn struck_quad(&mut self, point: Vec3, normal: Vec3) -> Option<u32> {
        super::particle_sounds::struck_quad(self.quads?, point, normal)
    }

    fn build_decal(
        &mut self,
        quad: Option<u32>,
        params: &crate::decal::Params,
    ) -> Option<crate::decal::Mesh> {
        crate::decal::build(self.quads?, quad, params)
    }

    fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit> {
        let env = crate::chrphys::Env::new(self.quads?, self.doors);
        // (A pane broken earlier this tick is passed through.)
        let mut start = 0.0f32;
        for _ in 0..4 {
            let at = from + ray * start;
            let h = crate::chrphys::level_ray(&env, at, ray * (1.0 - start))?;
            let fraction = start + h.fraction * (1.0 - start);
            let pane = h
                .quad
                .and_then(|q| self.pane_quads.iter().position(|&p| p == q));
            if pane.is_some_and(|k| self.panes[k].0) {
                start = (fraction + 1e-4).min(1.0);
                continue;
            }
            // A glass pane: surface = its index + 1.
            let surface = pane.map_or(0, |k| k as u32 + 1);
            return Some(EnvHit {
                fraction,
                normal: h.normal,
                surface,
                quad: h.quad,
            });
        }
        None
    }

    fn breakable(&self, surface: u32) -> bool {
        surface > 0 && self.panes.get(surface as usize - 1).is_some_and(|p| !p.0)
    }

    fn damage_surface(&mut self, surface: u32, damage: f32) -> bool {
        if !self.breakable(surface) {
            return false;
        }
        let k = surface as usize - 1;
        self.break_pane(k, damage, None);
        true
    }

    fn damage_surfaces_near(
        &mut self,
        centre: Vec3,
        radius: f32,
        damage: f32,
        blast: Option<p3::glass::Blast>,
    ) {
        for k in 0..self.panes.len() {
            let (broken, at, reach) = self.panes[k];
            if !broken && at.distance(centre) <= radius + reach {
                self.break_pane(k, damage, blast);
            }
        }
    }

    fn ray_chars(
        &mut self,
        from: Vec3,
        ray: Vec3,
        radius: f32,
        owner: Option<usize>,
    ) -> Option<CharHit> {
        let mut best: Option<CharHit> = None;
        for i in 0..self.fighters.len() {
            let f = &self.fighters[i];
            if !f.alive() || f.motion.corpse {
                continue;
            }
            // The whole body first.
            // (A generous sphere round the pelvis: the parts decide.)
            let pelvis = f.motion.height();
            let centre = f.feet() + Vec3::Y * pelvis;
            if ray_sphere(from, ray, centre, pelvis * 1.8 + radius).is_none() {
                continue;
            }
            let character = f.character.clone();
            let bones = self.bones_of(i).to_vec();
            for part in &character.parts {
                let Some(m) = bones.get(part.bone) else {
                    continue;
                };
                let hit = if radius > 0.0 {
                    let Some((c, r)) = part.bound_sphere else {
                        continue;
                    };
                    let wc = m.transform_point3(c);
                    ray_sphere(from, ray, wc, r + radius)
                        .map(|t| (t, (from + ray * t - wc).normalize_or(Vec3::Y)))
                } else {
                    let inv = m.inverse();
                    let (lo, hi) = part.bound_box;
                    ray_box(
                        inv.transform_point3(from),
                        inv.transform_vector3(ray),
                        lo,
                        hi,
                    )
                    .map(|(t, n)| (t, Mat3::from_mat4(*m).mul_vec3(n).normalize_or(Vec3::Y)))
                };
                let Some((t, normal)) = hit else { continue };
                // A shot leaving its own shooter's body passes.
                if owner == Some(i) && normal.dot(ray) > 0.0 {
                    continue;
                }
                if best.is_none_or(|b| t < b.fraction) {
                    best = Some(CharHit {
                        fraction: t,
                        who: i,
                        normal,
                        part: part.bone,
                    });
                }
            }
        }
        best
    }

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
    ) -> bool {
        let owner = if owner == 0 {
            p3::owner::ENVIRONMENTAL
        } else {
            owner
        };
        if self_immune && p3::owner::character(owner) == Some(who) {
            return false;
        }
        let dir = dir.try_normalize().unwrap_or_else(|| {
            let f = &self.fighters[who];
            (f.feet() + Vec3::Y * f.motion.height() - point)
                .try_normalize()
                .unwrap_or(Vec3::Z)
        });
        self.inflict(
            who,
            point,
            4.0,
            dir,
            kind,
            damage.round() as i32,
            stun.round() as i32,
            knockback,
            stamp,
            owner,
        )
    }

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
    ) {
        let owner = if owner == 0 {
            p3::owner::ENVIRONMENTAL
        } else {
            owner
        };
        for who in 0..self.fighters.len() {
            let f = &self.fighters[who];
            if !f.alive() || (self_immune && p3::owner::character(owner) == Some(who)) {
                continue;
            }
            let pelvis = f.feet() + Vec3::Y * f.motion.height();
            let v = pelvis - centre;
            let d = v.length() / radius.max(1e-6);
            if d > 1.0 {
                continue;
            }
            if let Some(s) = self.sight {
                if !s.shoots(centre, pelvis) {
                    continue;
                }
            }
            let p = match falloff {
                1 => 1.0 - d,
                2 => 1.0 / ((1.0 + d) * (1.0 + d)),
                _ => 1.0,
            };
            let dir = v.try_normalize().unwrap_or(Vec3::Y);
            self.inflict(
                who,
                centre,
                radius,
                dir,
                kind,
                (p * damage).round() as i32,
                (p * stun).round() as i32,
                p * knockback,
                Some(stamp),
                owner,
            );
        }
        self.sounds
            .push((centre, 400.0, p3::owner::character(owner)));
    }

    fn targets(&self) -> Vec<Target> {
        self.fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive())
            .map(|(who, f)| Target {
                who,
                position: f.feet() + Vec3::Y * (f.motion.height() + 4.0),
                velocity: f.velocity * p3::HZ as f32,
            })
            .collect()
    }

    fn hostile(&self, me: usize, other: usize) -> bool {
        match (self.teams.get(me), self.teams.get(other)) {
            (Some(a), Some(b)) => a.regards(*b) == crate::ai::perception::Relation::Hostile,
            _ => false,
        }
    }

    fn clear(&mut self, from: Vec3, to: Vec3) -> bool {
        self.sight
            .is_none_or(|s| s.clear(from, to, crate::quads::flag::OBJ_COL_SKIP))
    }

    fn inaccuracy(&self, owner: u32) -> f32 {
        if owner & p3::owner::TYPE_MASK == p3::owner::TURRET {
            return self.turret_spread;
        }
        p3::owner::character(owner)
            .and_then(|c| self.inaccuracy.get(c))
            .copied()
            .unwrap_or(0.0)
    }
}

/// The gun in fighter `f`'s hand (the original's weapon matrix): the hand's
/// own matrix (a left hand's turned half round its X), so the gun follows
/// the hand as the clip holds it. The gun's barrel is its +X, its top +Z.
fn hand_matrix(f: &Fighter, bones: &[Mat4]) -> Mat4 {
    let hand = bones.get(f.gun_hand()).copied().unwrap_or(f.placement);
    if f.motion.tuning.left_handed {
        hand * Mat4::from_rotation_x(std::f32::consts::PI)
    } else {
        hand
    }
}

/// The gun aimed exactly (`ONrCharacter_DoExactAiming`'s goal): at the
/// hand, its barrel straight down the aim (`yaw`, `pitch` up positive), its
/// top toward the world's up.
fn aim_matrix(at: Vec3, yaw: f32, pitch: f32) -> Mat4 {
    let d = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    let up = (Vec3::Y - d * d.y).try_normalize().unwrap_or(Vec3::Z);
    Mat4::from_mat3_translation(Mat3::from_cols(d, up.cross(d), up), at)
}

/// How many ticks the gun takes to swing from the hand to the aim.
const EXACT_AIM_TICKS: u64 = 8;

/// The blend of two gun matrices (`MUrMatrix_Lerp`): place and turn apart.
fn matrix_lerp(a: Mat4, b: Mat4, t: f32) -> Mat4 {
    let (_, ra, ta) = a.to_scale_rotation_translation();
    let (_, rb, tb) = b.to_scale_rotation_translation();
    Mat4::from_rotation_translation(ra.lerp(rb, t), ta.lerp(tb, t))
}

/// What a gun's sight meets (`AMtRayResultType`): nothing, the level or an
/// object, or a character (a living one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SightTarget {
    None,
    Environment,
    /// A placed object (`AMcRayResult_Object`).
    Object,
    Character,
}

/// A gun's sight this tick (`ONiDrawWeaponSight`): from the sight on the
/// gun to where it stops, what it meets, the laser's colour (a class with a
/// laser) and the tunnel's sprites along it.
#[derive(Clone, Debug)]
pub struct GunSight {
    pub from: Vec3,
    pub to: Vec3,
    pub target: SightTarget,
    pub laser: Option<[f32; 3]>,
    pub tunnel: Vec<Vec3>,
}

/// How far the sight looks for the level and objects (`AMcPickLimit`).
const PICK_LIMIT: f32 = 1000.0;
/// A sight that meets something stops this short of it.
const SIGHT_SHORT: f32 = 2.0;
/// The laser's half-width (`FXgLaser_Width`).
const LASER_WIDTH: f32 = 0.2;

/// Where a ray first meets fighter `f`'s body (its parts' boxes): the
/// fraction along it.
/// A body made ready for many rays at once (`ray_body` over and over):
/// each part's inverse matrix and box, and a sphere holding them all.
pub(super) struct RayBody {
    parts: Vec<(Mat4, Vec3, Vec3)>,
    centre: Vec3,
    radius: f32,
}

impl RayBody {
    pub(super) fn new(f: &Fighter, bones: &[Mat4]) -> RayBody {
        let mut parts = Vec::new();
        let mut corners = Vec::new();
        for part in &f.character.parts {
            let Some(m) = bones.get(part.bone) else {
                continue;
            };
            let (lo, hi) = part.bound_box;
            for k in 0..8 {
                let c = Vec3::new(
                    if k & 1 == 0 { lo.x } else { hi.x },
                    if k & 2 == 0 { lo.y } else { hi.y },
                    if k & 4 == 0 { lo.z } else { hi.z },
                );
                corners.push(m.transform_point3(c));
            }
            parts.push((m.inverse(), lo, hi));
        }
        let (mut a, mut b) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for c in &corners {
            a = a.min(*c);
            b = b.max(*c);
        }
        let centre = (a + b) * 0.5;
        let radius = corners
            .iter()
            .map(|c| c.distance(centre))
            .fold(0.0, f32::max)
            + 0.01;
        RayBody {
            parts,
            centre,
            radius,
        }
    }

    /// [`ray_body`]'s answer for this body.
    pub(super) fn ray(&self, from: Vec3, ray: Vec3) -> Option<f32> {
        if self.parts.is_empty() || ray_sphere(from, ray, self.centre, self.radius).is_none() {
            return None;
        }
        self.parts
            .iter()
            .filter_map(|(inv, lo, hi)| {
                ray_box(
                    inv.transform_point3(from),
                    inv.transform_vector3(ray),
                    *lo,
                    *hi,
                )
                .map(|(t, _)| t)
            })
            .min_by(f32::total_cmp)
    }
}

pub(super) fn ray_body(f: &Fighter, bones: &[Mat4], from: Vec3, ray: Vec3) -> Option<f32> {
    f.character
        .parts
        .iter()
        .filter_map(|part| {
            let m = bones.get(part.bone)?;
            let inv = m.inverse();
            let (lo, hi) = part.bound_box;
            ray_box(
                inv.transform_point3(from),
                inv.transform_vector3(ray),
                lo,
                hi,
            )
            .map(|(t, _)| t)
        })
        .min_by(f32::total_cmp)
}

impl World {
    /// A loose thing met a surface hard enough (`WPiGenerateFallImpact`):
    /// the impact on its material, at the point met.
    fn fall_impact_on(&mut self, c: crate::objphys::Contact, impact: &str) {
        use p3::effect::{Cause, Data};
        let Some(impacts) = self.impacts.clone() else {
            return;
        };
        let Some(impact) = impacts.impact(impact) else {
            return;
        };
        let material = self
            .quads
            .quads
            .get(c.quad as usize)
            .and_then(|q| self.quads.textures.get(q.texture as usize))
            .and_then(|t| impacts.texture_material(t))
            .or_else(|| impacts.material("Base"));
        let Some(material) = material else { return };
        let mut data = Data::new(self.armory.particles.tick, c.point);
        data.normal = c.normal;
        data.cause = Cause::HitWall(Some(c.quad));
        self.impact_effect(impact, material, crate::impacts::ANY, data, 1.0, None, None);
    }

    /// Whether fighter `who`'s gun pokes through a wall
    /// (`ONrCharacter_IsGunThroughWall`): the level, a door or a placed
    /// object between the middle of its pelvis and any of the gun's shooting
    /// attachments, or the gun's pivot. Such a gun neither fires nor shows
    /// its sight.
    pub fn gun_through_wall(&self, who: usize) -> bool {
        let Some(f) = self.fighters.get(who) else {
            return false;
        };
        let Some(w) = f.hand else { return false };
        let gun = &self.armory.weapons[w];
        let m = gun.matrix.get();
        let bones = f.bones();
        let pelvis = f
            .character
            .parts
            .iter()
            .find(|p| p.bone == 0)
            .and_then(|p| Some(bones.first()?.transform_point3(p.bound_sphere?.0)))
            .unwrap_or_else(|| f.feet() + Vec3::Y * f.motion.height());
        let env = crate::chrphys::Env::new(&self.quads, &self.door_boxes);
        // (A long gun held over a ledge counts as through it, the original's
        // known limit.)
        let blocked = |to: Vec3| {
            let v = to - pelvis;
            crate::chrphys::level_ray(&env, pelvis, v).is_some()
                || (v.length() > 1e-3
                    && self
                        .ray_to_object(pelvis, v.normalize(), v.length())
                        .is_some())
        };
        gun.class
            .attachments
            .iter()
            .filter(|a| a.ammo_used > 0)
            .any(|a| blocked(m.transform_point3(a.matrix.w_axis.truncate())))
            || blocked(m.w_axis.truncate())
    }

    /// Fighter `who`'s gun sight (`ONiDrawWeaponSight`): the player's, with
    /// a gun that has ammo and isn't through a wall, and a sight. From the
    /// sight along its direction to the first character (but itself), level
    /// surface, door or placed object — at most the class's sight distance,
    /// stopping 2 short of what it meets.
    pub fn gun_sight(&self, who: usize) -> Option<GunSight> {
        let f = self.fighters.get(who).filter(|f| f.player && !f.gone)?;
        let gun = &self.armory.weapons[f.hand?];
        let class = &gun.class;
        if gun.ammo == 0 || class.sight.1 == Vec3::ZERO || self.gun_through_wall(who) {
            return None;
        }
        let m = gun.matrix.get();
        let from = m.transform_point3(class.sight.0);
        let dir = m.transform_vector3(class.sight.1).try_normalize()?;
        let max = class.max_sight_distance;
        // The nearest of the characters (any distance) and the level and
        // objects (out to the pick limit).
        let mut hit: Option<(f32, SightTarget)> = None;
        for (j, o) in self.fighters.iter().enumerate() {
            if j == who || o.gone {
                continue;
            }
            if let Some(t) = ray_body(o, &o.bones(), from, dir * max) {
                let d = t * max;
                if hit.is_none_or(|(h, _)| d < h) {
                    // (A dead one is no target.)
                    hit = Some((
                        d,
                        if o.alive() {
                            SightTarget::Character
                        } else {
                            SightTarget::None
                        },
                    ));
                }
            }
        }
        let env = crate::chrphys::Env::new(&self.quads, &self.door_boxes);
        if let Some(h) = crate::chrphys::level_ray(&env, from, dir * PICK_LIMIT) {
            let d = h.fraction * PICK_LIMIT;
            if hit.is_none_or(|(h, _)| d < h) {
                hit = Some((d, SightTarget::Environment));
            }
        }
        if let Some(d) = self.ray_to_object(from, dir, PICK_LIMIT) {
            if hit.is_none_or(|(h, _)| d < h) {
                hit = Some((d, SightTarget::Object));
            }
        }
        let (to, target) = match hit {
            Some((d, _)) if d > max => (from + dir * max, SightTarget::None),
            Some((d, t)) if d > 1.0 / 1024.0 => (
                from + dir * d * (1.0 - (SIGHT_SHORT / d).clamp(0.0, 1.0)),
                t,
            ),
            Some((d, t)) => (from + dir * d, t),
            None => (from + dir * max, SightTarget::None),
        };
        // The tunnel: from a quarter-spacing in, one every spacing, while a
        // whole spacing still fits.
        let mut tunnel = Vec::new();
        if let Some(tn) = &class.tunnel {
            let dist = from.distance(to);
            let mut at = tn.spacing * 0.25;
            for _ in 0..tn.count {
                if at + tn.spacing > dist {
                    break;
                }
                tunnel.push(from + dir * at);
                at += tn.spacing;
            }
        }
        Some(GunSight {
            from,
            to,
            target,
            laser: class.has(flag::HAS_LASER).then_some(class.laser_color),
            tunnel,
        })
    }

    /// Loads the weapon and particle classes (`common/`) and the level's
    /// placed guns (`items.json`).
    pub fn attach_weapons(&mut self, common: &Path) {
        let particles = Rc::new(p3::Classes::load(&common.join("particle_classes.json")));
        let classes = Rc::new(WeaponClasses::load(
            &common.join("weapons.json"),
            &particles,
        ));
        self.armory.particles = p3::System::new(particles);
        self.armory.classes = classes;
        self.armory.weapons.clear();
        self.armory.powerups.clear();
        self.turrets = super::turrets::load(
            &self.level_dir,
            common,
            &self.armory.particles.classes.clone(),
        );
        // (Keeping the mixer an output device may already play.)
        let (mixer, realtime) = (self.audio.mixer.clone(), self.audio.realtime);
        self.audio = crate::audio::Audio::load(common, &self.level_dir);
        self.audio.mixer = mixer;
        self.audio.realtime = realtime;
        self.impacts = crate::impacts::Impacts::load(common).map(Rc::new);
        self.anim_sounds = crate::anim_sounds::AnimSounds::load(common).map(Rc::new);
        self.game_sounds = super::objects_sound::GameSounds::load(common);
        let gs: serde_json::Value = crate::json::load(common.join("game_settings.json"));
        self.hypo = super::hypo::HypoSettings::load(&gs);
        let settings: serde_json::Value = crate::json::load(common.join("game_settings.json"));
        // The magic effects' textures (shield, super shield, invisibility).
        self.magic_textures = ["shield", "daodanShield", "invis"].map(|k| {
            settings["magicTextures"][k]
                .as_str()
                .map(|t| common.join("textures").join(format!("{t}.png")))
        });
        self.armory.powerup_looks = POWERUP_DATA
            .iter()
            .map(|k| {
                let p = &settings["powerups"][*k];
                let size = |k: usize| p["glowSize"][k].as_f64().unwrap_or(POWERUP_GLOW_SIZE) as f32;
                PowerupLook {
                    model: p["model"].as_str().map(|m| common.join(m)),
                    radius: p["radius"].as_f64().unwrap_or(2.0) as f32,
                    glow: p["glow"]
                        .as_str()
                        .map(|g| common.join("textures").join(format!("{g}.png"))),
                    glow_size: [size(0), size(1)],
                    glow_height: p["glowHeight"].as_f64().unwrap_or(0.0) as f32,
                }
            })
            .collect();
        self.env_particles =
            super::env_particles::load(&self.level_dir, &self.armory.particles.classes.clone());
        self.quads =
            crate::quads::Quads::load(&self.level_dir.join("quads.bin")).unwrap_or_default();
        // (The doors' flat shades come from their quads.)
        self.door_shades = self
            .doors
            .iter()
            .map(|d| (d.object_id, self.door_shade(d.object_id)))
            .collect();
        // The object animations scripts name: the common ones, then the level's.
        self.object_anims.clear();
        for path in [
            common.join("object_animations.json"),
            self.level_dir.join("object_animations.json"),
        ] {
            let doc: serde_json::Value = crate::json::load(&path);
            for a in doc["animations"].as_array().into_iter().flatten() {
                if let (Some(name), Some(anim)) =
                    (a["name"].as_str(), crate::doors::object_animation(a))
                {
                    self.object_anims.insert(name.to_string(), anim);
                }
            }
        }
        self.mark_door_quads();
        // (Glass is breakable by its material, `AKiComputeBreakableFlags`.)
        for k in 0..self.glass.len() {
            let (quad, broken) = (self.glass[k].quad, self.glass[k].broken);
            if let Some(q) = self.quads.quads.get_mut(quad as usize) {
                q.flags |= crate::quads::flag::BREAKABLE;
                if broken {
                    q.flags |= crate::quads::flag::BROKEN_GLASS;
                }
            }
        }
        // (Anyone placed before the level's quads were known.)
        for k in 0..self.fighters.len() {
            let quads = &self.quads;
            self.fighters[k].snap_to_floor(quads);
        }
        let flags: serde_json::Value = crate::json::load(common.join("texture_flags.json"));
        self.additive_textures = flags["additive"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect();
        self.texture_anims = crate::texanim::load(&common.join(".."));
        // The health colour ramp.
        let gs: serde_json::Value = crate::json::load(common.join("game_settings.json"));
        self.health_color = gs["healthColor"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some((
                    p["health"].as_f64()? as f32,
                    u32::from_str_radix(p["color"].as_str()?.trim_start_matches('#'), 16).ok()?,
                ))
            })
            .collect();
        self.env_particles_started = false;
        let Ok(text) = std::fs::read_to_string(self.level_dir.join("items.json")) else {
            return;
        };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        for w in doc["weapons"].as_array().into_iter().flatten() {
            let Some(class) = w["class"].as_str().and_then(|n| self.armory.classes.get(n)) else {
                continue;
            };
            let p = &w["position"];
            let at = Vec3::new(
                p[0].as_f64().unwrap_or(0.0) as f32,
                p[1].as_f64().unwrap_or(0.0) as f32 + crate::weapons::PLACED_LIFT,
                p[2].as_f64().unwrap_or(0.0) as f32,
            );
            let yaw = (w["yaw"].as_f64().unwrap_or(0.0) as f32).to_radians();
            self.armory.weapons.push(Weapon::new(class, at, yaw, true));
        }
        for p in doc["powerups"].as_array().into_iter().flatten() {
            let Some(kind) = POWERUP_DATA
                .iter()
                .position(|n| Some(*n) == p["type"].as_str())
            else {
                continue;
            };
            let v = &p["position"];
            let at = Vec3::new(
                v[0].as_f64().unwrap_or(0.0) as f32,
                v[1].as_f64().unwrap_or(0.0) as f32,
                v[2].as_f64().unwrap_or(0.0) as f32,
            );
            self.armory
                .powerups
                .push(Powerup::new(kind, POWERUP_DEFAULT[kind], at));
        }
    }

    /// Fighter `who` drops what it carries, dying (`ONiCharacter_DropInventoryItems`;
    /// not the player): its "death candy" — each clip, cell and hypo it was
    /// given to drop, a full shield or invisibility if so — then, unless it
    /// keeps them, its worn shield, invisibility and its LSI.
    fn drop_inventory(&mut self, who: usize) {
        let f = &mut self.fighters[who];
        if f.player || f.inventory.dropped {
            return;
        }
        f.inventory.dropped = true;
        let inv = f.inventory;
        let mut drops: Vec<(usize, u16)> = Vec::new();
        for (kind, &n) in inv.drops.iter().enumerate().take(3) {
            drops.extend(std::iter::repeat_n((kind, 1), usize::from(n)));
        }
        if inv.drops[3] > 0 {
            drops.push((3, POWERUP_DEFAULT[3]));
        }
        if inv.drops[4] > 0 {
            drops.push((4, POWERUP_DEFAULT[4]));
        }
        if !inv.no_auto_drop {
            if inv.shield > 0 {
                drops.push((3, inv.shield));
            }
            if inv.invisibility > 0 {
                drops.push((4, inv.invisibility));
            }
            if inv.lsi {
                drops.push((5, 1));
            }
        }
        for (kind, amount) in drops {
            self.drop_powerup(who, kind, amount);
        }
    }

    /// A powerup falls from fighter `who` (`WPrPowerup_Drop`): from its
    /// middle, nudged up to 1 aside, turned at random, with a little random
    /// speed sideways.
    /// A powerup by name falls from fighter `who` (`drop_powerup`).
    pub(super) fn drop_powerup_named(&mut self, who: usize, kind: &str, amount: u16) {
        if let Some(k) = POWERUPS.iter().position(|n| *n == kind) {
            self.drop_powerup(who, k, amount);
        }
    }

    fn drop_powerup(&mut self, who: usize, kind: usize, amount: u16) {
        let f = &self.fighters[who];
        let pelvis = f.sight_points()[0];
        let mut at = Vec3::new(pelvis.x, f.feet().y + f.motion.height(), pelvis.z);
        let deviate = |w: &mut World, v: &mut Vec3, by: f32| {
            v.x += (w.random() * 2.0 - 1.0) * by;
            v.z += (w.random() * 2.0 - 1.0) * by;
        };
        deviate(self, &mut at, 1.0);
        let radius = self
            .armory
            .powerup_looks
            .get(kind)
            .map_or(2.0, |l| l.radius);
        let mut body = crate::objphys::Body::new(
            at,
            radius,
            POWERUP_STATIC_FRICTION,
            POWERUP_DYNAMIC_FRICTION,
        );
        deviate(self, &mut body.velocity, 1.5 / 60.0);
        let mut p = Powerup::new(kind, amount, at);
        p.rotation = self.random() * std::f32::consts::TAU;
        p.body = Some(body);
        eprintln!("weapon: fighter {who} drops {}", POWERUPS[kind]);
        self.armory.powerups.push(p);
    }

    /// The other bodies a loose thing (`me`) may meet
    /// (`PHrPhysics_Callback_FindPhyCollisions`): the guns and powerups
    /// lying free with physics, the placed objects shown (their spheres) and
    /// — for a gun, `characters` — the characters' stacks, but not the dead
    /// once settled (their physics off).
    /// The characters as obstacles to loose things (each its body's tree).
    pub(super) fn character_obstacles(&self) -> Vec<crate::objphys::Obstacle> {
        self.fighters
            .iter()
            .filter(|f| !f.gone && !f.settled)
            .map(|f| {
                let t = f.tree();
                crate::objphys::Obstacle {
                    spheres: t.leaves.iter().map(|s| (s.center, s.radius)).collect(),
                    whole: (t.root.center, t.root.radius),
                    character: true,
                }
            })
            .collect()
    }

    /// What loose thing `me` may meet: the other loose guns and powerups,
    /// the objects, and (given) the characters — built once by the caller,
    /// as they don't move while the loose things do.
    pub(super) fn loose_obstacles(
        &self,
        me: Option<Loose>,
        characters: Option<&[crate::objphys::Obstacle]>,
    ) -> Vec<crate::objphys::Obstacle> {
        use crate::objphys::Obstacle;
        let one = |c: Vec3, r: f32, character: bool| Obstacle {
            spheres: vec![(c, r)],
            whole: (c, r),
            character,
        };
        let mut out = Vec::new();
        for (k, g) in self.armory.weapons.iter().enumerate() {
            if me == Some(Loose::Weapon(k)) || g.owner.is_some() || g.gone {
                continue;
            }
            if let Some(b) = &g.body {
                out.push(one(b.position, b.radius, false));
            }
        }
        for (k, p) in self.armory.powerups.iter().enumerate() {
            if me == Some(Loose::Powerup(k)) {
                continue;
            }
            if let Some(b) = &p.body {
                out.push(one(b.position, b.radius, false));
            }
        }
        for k in 0..self.objects.len() {
            if self.objects[k].flags & crate::world::objects::oflag::INVISIBLE != 0 {
                continue;
            }
            out.push(one(
                self.object_matrix(k).w_axis.truncate(),
                self.object_setups[self.objects[k].setup].radius,
                false,
            ));
        }
        if let Some(c) = characters {
            out.extend_from_slice(c);
        }
        out
    }

    /// One tick of the falling and sliding powerups (`WPiPowerup_Update`):
    /// their physics (`crate::objphys`; characters don't stop them), the
    /// "Powerup" impact once a pass on meeting a surface hard enough, and —
    /// on the ground or barely moving for 120 ticks — their physics goes.
    fn powerups_tick(&mut self) {
        let mut impacts = Vec::new();
        // (What each moving one may meet: not characters.)
        let others: Vec<Vec<crate::objphys::Obstacle>> = (0..self.armory.powerups.len())
            .map(|k| {
                if self.armory.powerups[k].body.is_some() {
                    self.loose_obstacles(Some(Loose::Powerup(k)), None)
                } else {
                    Vec::new()
                }
            })
            .collect();
        for (p, others) in self.armory.powerups.iter_mut().zip(&others) {
            let Some(mut b) = p.body else { continue };
            let mut on_ground = false;
            b.tick(&self.quads, others, |body, met| {
                let level = || met.iter().filter(|c| c.body.is_none());
                if let Some(c) = level().find(|c| body.velocity.dot(c.normal).abs() > 0.35) {
                    impacts.push(*c);
                }
                on_ground = level().any(|c| c.normal.y > crate::chrphys::MIN_FLOOR_NORMAL_Y);
            });
            b.velocity.y = b.velocity.y.min(2.25);
            p.position = b.position;
            if on_ground || b.velocity.length_squared() < POWERUP_STILL * POWERUP_STILL {
                p.rest += 1;
            } else {
                p.rest = 0;
            }
            p.body = (p.rest < POWERUP_REST).then_some(b);
        }
        for c in impacts {
            self.fall_impact_on(c, "Powerup");
        }
    }

    /// What to draw for the powerups lying about: each one's model, turned.
    pub fn powerup_draws(&self) -> Vec<(std::path::PathBuf, Mat4)> {
        self.armory
            .powerups
            .iter()
            .filter_map(|p| {
                let model = self.armory.powerup_looks.get(p.kind)?.model.clone()?;
                Some((
                    model,
                    Mat4::from_rotation_translation(
                        glam::Quat::from_rotation_y(p.rotation),
                        p.position,
                    ),
                ))
            })
            .collect()
    }

    /// The glows lying under the powerups: flat sprites turned with them,
    /// at a third of full brightness, drawn without writing depth
    /// (`WPrPowerup_Draw`).
    fn powerup_glows(&self) -> impl Iterator<Item = crate::render::Sprite> + '_ {
        self.armory.powerups.iter().filter_map(|p| {
            let look = self.armory.powerup_looks.get(p.kind)?;
            let texture = look.glow.clone()?;
            let turn = Mat3::from_rotation_y(p.rotation);
            let bright = 1.0 / 3.0;
            Some(crate::render::Sprite {
                at: p.position + Vec3::Y * look.glow_height,
                size: look.glow_size,
                mode: crate::render::SpriteMode::Fixed([turn.x_axis, turn.z_axis]),
                rotation: 0.0,
                color: [bright; 3],
                texture,
                contrail: false,
                no_depth: false,
            })
        })
    }

    pub fn powerups(&self) -> &[Powerup] {
        &self.armory.powerups
    }

    /// A powerup of `kind` (a script name) at `at`; `amount` defaults to
    /// the kind's.
    pub fn spawn_powerup(&mut self, kind: &str, at: Vec3, amount: Option<u16>) -> bool {
        let Some(kind) = POWERUPS.iter().position(|n| *n == kind) else {
            return false;
        };
        self.armory.powerups.push(Powerup::new(
            kind,
            amount.unwrap_or(POWERUP_DEFAULT[kind]),
            at,
        ));
        true
    }

    /// How much of a powerup fighter `who` could take.
    fn could_receive(&self, who: usize, kind: usize, amount: u16) -> u16 {
        let f = &self.fighters[who];
        let inv = &f.inventory;
        let add = |have: u16| {
            if have >= CARRY_MAX {
                0
            } else {
                amount.min(CARRY_MAX - have)
            }
        };
        match kind {
            0 => add(inv.ammo),
            1 => add(inv.cells),
            2 => add(inv.hypos),
            3 => amount.saturating_sub(inv.shield),
            4 => amount.saturating_sub(inv.invisibility),
            _ => u16::from(f.player),
        }
    }

    /// Gives fighter `who` a powerup (`give_powerup`); with `ignore_max`
    /// the carrying limits don't apply. Returns how much it took.
    pub fn give_powerup(
        &mut self,
        who: usize,
        kind: &str,
        amount: Option<u16>,
        ignore_max: bool,
    ) -> u16 {
        let Some(kind) = POWERUPS.iter().position(|n| *n == kind) else {
            return 0;
        };
        let amount = amount.unwrap_or(POWERUP_DEFAULT[kind]);
        let give = if ignore_max {
            amount
        } else {
            self.could_receive(who, kind, amount)
        };
        // (The player hears what it got: ammo, cells, hypos, the LSI.)
        if give > 0 && self.fighters[who].player {
            if let Some(event) = [
                "receiveAmmo",
                "receiveCell",
                "receiveHypo",
                "",
                "",
                "receiveLsi",
            ]
            .get(kind)
            .filter(|e| !e.is_empty())
            {
                self.event_sound(event, None);
            }
        }
        if give > 0 && who == 0 && self.fighters[who].player {
            self.hud.item_met(kind as u32);
            self.keep_unlocks();
        }
        let inv = &mut self.fighters[who].inventory;
        match kind {
            0 => inv.ammo += give,
            1 => inv.cells += give,
            2 => inv.hypos += give,
            3 => inv.shield += give,
            4 => inv.invisibility += give,
            _ => inv.lsi |= give > 0,
        }
        give
    }

    /// What the player has met, kept between runs (in play).
    fn keep_unlocks(&self) {
        if self.persist_continues {
            crate::game::store_unlocks(self.hud.items_seen, &self.hud.weapon_classes_seen);
        }
    }

    /// The nearest powerup fighter `who` could take, within reach.
    pub(super) fn powerup_in_reach(&self, who: usize) -> Option<usize> {
        let feet = self.fighters.get(who)?.feet();
        self.armory
            .powerups
            .iter()
            .enumerate()
            .filter(|(_, p)| self.could_receive(who, p.kind, p.amount) > 0)
            .filter_map(|(i, p)| {
                let d = ((p.position - feet) * Vec3::new(1.0, 0.0, 1.0)).length();
                let dy = p.position.y - feet.y;
                (d < PICKUP_RADIUS && (-PICKUP_BELOW..=PICKUP_ABOVE).contains(&dy))
                    .then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    /// Takes every gun from fighter `who` and empties its pockets
    /// (`chr_inv_reset`).
    pub fn reset_inventory(&mut self, who: usize) {
        let Some(f) = self.fighters.get_mut(who) else {
            return;
        };
        f.inventory = crate::fighter::Inventory::default();
        let guns: Vec<usize> = [f.hand.take(), f.holster.take(), f.magic.take()]
            .into_iter()
            .flatten()
            .collect();
        for w in guns {
            self.delete_weapon(w);
        }
        self.set_weapon_varient(who);
    }

    /// Removes gun `w` from the world (its particles go).
    fn delete_weapon(&mut self, w: usize) {
        let Armory {
            particles, weapons, ..
        } = &mut self.armory;
        let now = self.now as u64;
        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        weapons[w].stow(particles, &mut host);
        weapons[w].owner = None;
        weapons[w].gone = true;
    }

    /// A cutscene or console putting the gun in hand away (`on`), into the
    /// hidden slot — or onto the floor if that is taken; or giving it back
    /// (only into an empty hand). (`chr_forceholster`.)
    pub fn force_holster(&mut self, who: usize, on: bool) {
        let Some(f) = self.fighters.get(who) else {
            return;
        };
        if on {
            if f.hand.is_some() && !self.holster(who, true) {
                // (No clip: straight away.)
                self.put_away(who, true);
            }
        } else if f.hand.is_none() && f.magic.is_some() && !self.draw(who) {
            self.bring_to_hand(who);
        }
    }

    /// A cutscene putting the gun in hand away (`ONrCharacter_CinematicHolster`:
    /// a forced holster, into the hidden slot — but never an unstorable gun,
    /// which `ONrCharacter_Holster` refuses even forced).
    pub fn cinematic_holster(&mut self, who: usize) {
        let Some(w) = self.fighters.get(who).and_then(|f| f.hand) else {
            return;
        };
        if !self.armory.weapons[w].class.has(flag::UNSTORABLE) {
            self.holster(who, true);
        }
    }

    /// Holds a gun that is empty, with nothing to reload it and no reload
    /// under way (`chr_has_empty_weapon`).
    pub fn has_empty_weapon(&self, who: usize) -> bool {
        let Some(f) = self.fighters.get(who) else {
            return false;
        };
        let Some(w) = f.hand else { return false };
        self.armory.weapons[w].ammo == 0 && !self.has_reload(who, w) && !f.reloading()
    }

    /// The gun in fighter `who`'s hand as its AI's targeting sees it.
    pub(super) fn gun_view(&self, who: usize) -> Option<crate::ai::goals::GunView> {
        let f = self.fighters.get(who)?;
        let w = f.hand?;
        let gun = &self.armory.weapons[w];
        let ai = gun.class.ai;
        Some(crate::ai::goals::GunView {
            matrix: gun.matrix.get(),
            ai,
            skill: f.shooting.get(ai.skill_index).copied().unwrap_or_default(),
            params: f.targeting,
            usable: gun.ammo > 0 || self.has_reload(who, w) || f.reloading(),
            firing: gun.firing,
            stuns: gun.class.has(flag::AI_STUN),
            knocks_down: gun.class.has(flag::AI_KNOCKDOWN),
        })
    }

    /// The guns firing now, as cones others dodge (those with a dodge
    /// pattern): from the shooter along its shot's line.
    pub(super) fn spreads(&self) -> Vec<crate::ai::goals::Spread> {
        self.armory
            .weapons
            .iter()
            .filter(|g| g.firing && g.class.dodge_pattern[0] > 0.0)
            .filter_map(|g| {
                let owner = g.owner?;
                let m = g.matrix.get();
                let shooter = g
                    .class
                    .attachments
                    .iter()
                    .find(|a| a.ammo_used > 0 && a.shooter == 0)?;
                Some(crate::ai::goals::Spread {
                    owner,
                    origin: m.transform_point3(shooter.matrix.w_axis.truncate()),
                    axis: m
                        .transform_vector3(shooter.matrix.y_axis.truncate())
                        .normalize_or(Vec3::Z),
                    length: g.class.dodge_pattern[0],
                    base: g.class.dodge_pattern[1],
                    angle: g.class.dodge_pattern[2],
                })
            })
            .collect()
    }

    /// The projectiles others may dodge (classes with a dodge radius).
    pub(super) fn projectiles(&self) -> Vec<crate::ai::goals::Projectile> {
        let sys = &self.armory.particles;
        sys.iter()
            .filter(|(_, p)| sys.class(p.class).dodge_radius > 0.0)
            .map(|(h, p)| crate::ai::goals::Projectile {
                id: h.id(),
                at: sys.world_position(p),
                velocity: p.velocity,
                radius: sys.class(p.class).dodge_radius,
            })
            .collect()
    }

    /// The guns lying free, as fighter `who` sees them (`FreeGun`): usable
    /// if storable and loaded or it carries clips for them.
    pub(super) fn free_guns(&self, who: usize) -> Vec<crate::ai::goals::FreeGun> {
        let f = &self.fighters[who];
        self.armory
            .weapons
            .iter()
            .enumerate()
            .filter(|(_, g)| g.owner.is_none() && !g.gone)
            .map(|(index, g)| {
                let clips = match g.ammo_type() {
                    Some(0) => f.inventory.ammo > 0,
                    Some(1) => f.inventory.cells > 0,
                    _ => false,
                };
                crate::ai::goals::FreeGun {
                    index,
                    at: g.pickup_position(),
                    usable: !g.class.has(flag::UNSTORABLE)
                        && (g.ammo > 0 || clips || f.infinite_ammo),
                }
            })
            .collect()
    }

    /// The AIs' aim and triggers this tick (`AI2rExecutor_UpdateAim`;
    /// `docs/AI.md`, *Aiming and looking*): each aim — kept to the body as it
    /// turns — swings toward what the AI wants to aim or look at
    /// (`Agent::aim_want`, from the shoulder 15 up) by at most a sixth of a
    /// turn a second × its speed (× the gun's aiming speed, gun in hand),
    /// never more than 130° off the body (then straight ahead), and back to
    /// straight ahead once it wants nothing. Its upper body follows through
    /// the aiming screens (`aiming_lr` / `_ud`), its gun too. The trigger is
    /// held while its targeting says so. Returns each fighter's trigger bits.
    pub(super) fn ai_guns(&mut self) -> Vec<u32> {
        use std::f32::consts::{PI, TAU};
        const SHOULDER: f32 = 15.0;
        const MAX_OFF: f32 = 130.0 * PI / 180.0;
        use crate::angle::wrap_pi as wrap;
        let mut out = vec![0u32; self.fighters.len()];
        let mut varients = Vec::new();
        for (i, f) in self.fighters.iter_mut().enumerate() {
            let Some(crate::ai::Brain::Agent(a)) = f.brain.as_ref() else {
                f.ai_aim = None;
                continue;
            };
            // Aiming with its gun or just looking: its gun's poses with it.
            if f.no_gun_varient == a.aiming_weapon {
                f.no_gun_varient = !a.aiming_weapon;
                varients.push(i);
            }
            let aiming_weapon = a.aiming_weapon;
            // (A film carries its own aim.)
            if f.film.is_some() || !f.alive() {
                f.ai_aim = None;
                continue;
            }
            let facing = f.facing.facing;
            let want = a.aim_want(f.feet() + glam::Vec3::Y * SHOULDER);
            let trigger = a.trigger && a.aim.is_some();
            let alternate = a.alt_fire;
            let (mut yaw, mut pitch, speed) = match want {
                Some((dir, speed)) if dir.length_squared() > 1e-6 => {
                    let d = dir.normalize();
                    (crate::angle::yaw_of(d), d.y.clamp(-1.0, 1.0).asin(), speed)
                }
                // (Stopping: back to the middle at the usual speed.)
                _ => (facing, 0.0, 1.0),
            };
            if wrap(yaw - facing).abs() > MAX_OFF {
                yaw = facing;
                pitch = 0.0;
            }
            // Where it aims now: as last tick, turned with the body since.
            let (cy, cp) = match f.ai_aim {
                Some((y, p)) => (y + (facing - f.ai_aim_facing), p),
                None => (facing, 0.0),
            };
            let (dy, dp) = (wrap(yaw - cy), pitch - cp);
            // (The gun's aiming speed only aiming with it.)
            let gun = f
                .hand
                .filter(|_| aiming_weapon)
                .map_or(1.0, |w| self.armory.weapons[w].class.aiming_speed.max(0.01));
            let step = TAU / 60.0 * speed * gun;
            let dist = (dy * dy + dp * dp).sqrt();
            let now = if dist <= step {
                (yaw, pitch)
            } else {
                (cy + dy * step / dist, cp + dp * step / dist)
            };
            // (Back in the middle with nothing wanted, it stops aiming.)
            f.ai_aim = if want.is_none() && dist <= step {
                None
            } else {
                Some((wrap(now.0), now.1))
            };
            f.ai_aim_facing = facing;
            (f.aiming_lr, f.aiming_ud) =
                f.ai_aim.map_or((0.0, 0.0), |(y, p)| (wrap(y - facing), p));
            let Some(w) = f.hand else { continue };
            // (A gun that fires one shot a press waits for its chamber.)
            let gun = &self.armory.weapons[w];
            let single =
                gun.class.has(flag::AI_SINGLE_SHOT) && (gun.chamber > 0 || gun.reload_time > 0);
            if trigger && !single {
                out[i] = if alternate { key::FIRE2 } else { key::FIRE1 };
            }
        }
        for i in varients {
            self.set_weapon_varient(i);
        }
        out
    }

    /// Each fighter's extra shot spread (degrees): an AI's skill with its
    /// gun, widened by the difficulty if it is the player's enemy; players
    /// shoot true.
    pub(super) fn inaccuracies(&self) -> Vec<f32> {
        let player_team = self.fighters.first().map(|p| p.team());
        self.fighters
            .iter()
            .map(|f| {
                if f.player {
                    return 0.0;
                }
                let Some(w) = f.hand else { return 0.0 };
                let extra = match f.brain.as_ref() {
                    Some(crate::ai::Brain::Agent(a)) => a.extra_spread,
                    _ => 0.0,
                };
                let skill = f
                    .shooting
                    .get(self.armory.weapons[w].class.ai.skill_index)
                    .map_or(0.0, |s| s.gun_inaccuracy)
                    + extra;
                let hostile = player_team.is_some_and(|t| {
                    f.team().regards(t) == crate::ai::perception::Relation::Hostile
                });
                skill
                    * if hostile {
                        self.difficulty.inaccuracy
                    } else {
                        1.0
                    }
            })
            .collect()
    }

    /// How fast the gun in fighter `who`'s hand aims (scales the mouse).
    pub fn aiming_speed(&self, who: usize) -> Option<f32> {
        let w = self.fighters.get(who)?.hand?;
        Some(self.armory.weapons[w].class.aiming_speed).filter(|s| *s > 0.0)
    }

    /// What to draw of the guns: each one's model and world matrix (guns
    /// in hands, and guns lying about; holstered ones are not shown).
    pub fn gun_draws(&self) -> Vec<(std::path::PathBuf, Mat4)> {
        let holstered: std::collections::HashSet<usize> = self
            .fighters
            .iter()
            .flat_map(|f| [f.holster, f.magic])
            .flatten()
            .collect();
        self.armory
            .weapons
            .iter()
            .enumerate()
            .filter(|(i, g)| {
                !g.gone
                    && !holstered.contains(i)
                    && g.owner.is_none_or(|o| {
                        self.fighters
                            .get(o)
                            .is_some_and(|f| f.hand == Some(*i) && f.effects.body_alpha() >= 1.0)
                    })
            })
            .filter_map(|(_, g)| Some((g.class.model.clone()?, g.matrix.get())))
            .collect()
    }

    /// The guns in the hands of bodies fading (invisibility): drawn with
    /// the body, at its alpha (the original's weapon is part of the body
    /// drawn); none once it is gone.
    pub fn faded_gun_draws(&self) -> Vec<(std::path::PathBuf, Mat4, f32)> {
        self.fighters
            .iter()
            .filter_map(|f| {
                let alpha = f.effects.body_alpha();
                let g = self.armory.weapons.get(f.hand?)?;
                (alpha < 1.0 && alpha > 0.0 && !g.gone)
                    .then(|| Some((g.class.model.clone()?, g.matrix.get(), alpha)))
                    .flatten()
            })
            .collect()
    }

    /// The particles that are models (superballs, grenades, darts, shells,
    /// casings): each one's model and world matrix — its position, its
    /// orientation, its class's scale.
    pub fn particle_models(&self) -> Vec<(std::path::PathBuf, Mat4)> {
        use p3::class::{cflag, Value};
        let sys = &self.armory.particles;
        let models = self.level_dir.join("../../common/models");
        sys.iter()
            .filter(|(_, p)| p.flags & p3::pflag::HIDDEN == 0)
            .filter_map(|(_, p)| {
                let c = sys.class(p.class);
                if !c.has(cflag::GEOMETRY) || c.has(cflag::INVISIBLE) || c.instance.is_empty() {
                    return None;
                }
                let scale = match c.scale {
                    Value::Float(x) if x > 0.0 => x,
                    _ => 1.0,
                };
                let m = Mat4::from_mat3_translation(
                    sys.world_orientation(p) * scale,
                    sys.world_position(p),
                );
                Some((models.join(format!("{}.glb", c.instance)), m))
            })
            .collect()
    }

    /// The particles to draw, as sprites: each class's texture at its
    /// scale — facing the camera, or stretched along its flight for the
    /// directional sprite kinds. (Models, contrails' trails, tint and fading
    /// aren't drawn yet.)
    /// The decals to draw from `eye` (`docs/PARTICLES.md`, *Decals*): the
    /// level's, then the rest, oldest first — each only while the quad it
    /// was laid on is drawn (not one-sided and seen from behind), at the
    /// quad's see-through opacity while it is see-through, full otherwise
    /// (`P3iDisplayDecals`). Drawn opacity: the decal's times that, doubled,
    /// at most full (the original's draw doubles it).
    pub fn decal_draws(&self, eye: Vec3) -> Vec<crate::render::DecalDraw> {
        use crate::quads::flag;
        let textures = self.level_dir.join("../../common/textures");
        let quads = &self.quads;
        // The quad's opacity this frame; none while it isn't drawn.
        let quad_alpha = |quad: Option<u32>| -> Option<u32> {
            let Some(q) = quad.and_then(|q| quads.quads.get(q as usize)) else {
                return Some(255);
            };
            if q.flags & flag::AK_2SIDED == 0 {
                let (n, _) = quads.plane(q.plane);
                if (eye - quads.corner(q, 0)).dot(n) < 0.0 {
                    return None;
                }
            }
            Some(if q.flags & flag::JELLO != 0 {
                super::camera::JELLO_AMT
            } else {
                255
            })
        };
        let draw = |mesh: Rc<crate::decal::Mesh>, alpha: u16, texture: &str| {
            let shown = quad_alpha(mesh.quad)?;
            Some(crate::render::DecalDraw {
                positions: mesh,
                alpha: ((u32::from(alpha) * shown * 2) >> 8).min(255) as f32 / 255.0,
                texture: textures.join(format!("{texture}.png")),
                additive: self.additive_textures.contains(texture),
            })
        };
        let decals = &self.armory.particles.decals;
        // The level's, then the characters' shadows (laid again each frame,
        // so after them), then the rest.
        let mut out: Vec<_> = decals
            .fixed
            .iter()
            .filter_map(|d| draw(d.mesh.clone(), d.alpha, &d.texture))
            .collect();
        for k in 0..self.fighters.len() {
            if let Some((mesh, alpha, texture)) = self.shadow(k) {
                out.extend(draw(Rc::new(mesh), alpha, &texture));
            }
        }
        out.extend(
            decals
                .dynamic
                .iter()
                .filter_map(|d| draw(d.mesh.clone(), d.alpha, &d.texture)),
        );
        out
    }

    /// Fighter `k`'s shadow this frame: its decal, opacity and texture.
    pub fn shadow(&self, k: usize) -> Option<(crate::decal::Mesh, u16, String)> {
        let f = self.fighters.get(k)?;
        let s = f.shadow.as_ref()?;
        if f.gone || f.no_shadow || f.settled {
            return None;
        }
        let pelvis = f.bones().first()?.w_axis.truncate();
        let feet = f.feet();
        // Standing: on the floor underfoot.
        let floor = (!f.airborne())
            .then(|| {
                self.quads.ray(
                    feet + Vec3::Y,
                    Vec3::Y * -2.0,
                    crate::quads::flag::OBJ_COL_SKIP,
                )
            })
            .flatten()
            .map(|h| h.quad);
        // (None for a body faded out; fading with it as it turns invisible.)
        let seen = f.effects.body_alpha();
        if seen <= 0.0 {
            return None;
        }
        let (at, quad, size, alpha) = s.place(&self.quads, pelvis, feet.y, floor)?;
        let alpha = if seen < 1.0 {
            (f32::from(alpha) * seen).round() as u16
        } else {
            alpha
        };
        let mesh = crate::decal::Shadow::build(&self.quads, quad, at, size)?;
        Some((mesh, alpha, s.texture.clone()))
    }

    /// The animated textures, by file stem.
    pub fn texture_anims(&self) -> &std::collections::HashMap<String, crate::texanim::TexAnim> {
        &self.texture_anims
    }

    /// The game time (ticks) things animate by: the particles' clock.
    pub fn game_tick(&self) -> u32 {
        self.armory.particles.tick
    }

    /// The texture particle `p` shows now: its class's, or — animated — the
    /// frame showing, its time from the particle's birth (with a texture
    /// time) or the game's, started at its random index (with a texture
    /// index; `M3iResolveTexture`).
    pub fn particle_texture(&self, p: &p3::Particle) -> std::path::PathBuf {
        use p3::class::cflag;
        let sys = &self.armory.particles;
        let c = sys.class(p.class);
        match self.texture_anims.get(&c.instance) {
            Some(a) => {
                let time = if c.has(cflag::TEXTURE_TIME) {
                    sys.tick.wrapping_sub(p.texture_time)
                } else {
                    sys.tick
                };
                let instance = if c.has(cflag::TEXTURE_INDEX) {
                    p.texture_index
                } else {
                    0
                };
                a.file(time, instance).to_path_buf()
            }
            None => self
                .level_dir
                .join("../../common/textures")
                .join(format!("{}.png", c.instance)),
        }
    }

    pub fn sprites(&self) -> Vec<crate::render::Sprite> {
        use p3::class::{cflag, Value};
        let sys = &self.armory.particles;
        let textures = self.level_dir.join("../../common/textures");
        let read = |v: &Value, vars: &[p3::Val]| -> Option<f32> {
            match v {
                Value::Float(x) => Some(*x),
                Value::Range(a, b) => Some((a + b) / 2.0),
                Value::Bell(m, _) => Some(*m),
                Value::Var(i) => vars.get(*i)?.f(),
                _ => None,
            }
        };
        sys.iter()
            .filter(|(_, p)| p.flags & p3::pflag::HIDDEN == 0)
            .filter_map(|(h, p)| {
                let c = sys.class(p.class);
                // (Decals are drawn as decals; contrails as their links.)
                if c.has(cflag::INVISIBLE)
                    || c.has(cflag::GEOMETRY)
                    || c.has(cflag::DECAL)
                    || c.instance.is_empty()
                    || matches!(c.sprite_type, 5 | 6)
                {
                    return None;
                }
                let x = read(&c.scale, &p.vars)?;
                let y = c
                    .y_scale
                    .as_ref()
                    .and_then(|v| read(v, &p.vars))
                    .unwrap_or(x);
                // How it lies (`MSrGeomContext_Method_Sprite_Draw`): its
                // direction its orientation's Y, else its velocity; the
                // fixed kinds from its orientation's axes.
                let o = c.has(cflag::ORIENTATION).then(|| sys.world_orientation(p));
                let dir = o.map_or(p.velocity, |o| o.y_axis);
                let mode = match (c.sprite_type, o) {
                    (1, _) => crate::render::SpriteMode::Rotated(dir),
                    (2 | 5 | 6, _) => crate::render::SpriteMode::Billboard(dir),
                    // Arrow: along its direction, upright along its up.
                    (3, Some(o)) => crate::render::SpriteMode::Fixed([o.y_axis, o.z_axis]),
                    // Flat: across its direction.
                    (4, Some(o)) => crate::render::SpriteMode::Fixed([o.x_axis, o.z_axis]),
                    // Discus: along its direction and its right.
                    (7, Some(o)) => crate::render::SpriteMode::Fixed([o.y_axis, o.x_axis]),
                    _ => crate::render::SpriteMode::Facing,
                };
                let rotation = read(&c.rotation, &p.vars).unwrap_or(0.0);
                // Its tint (ARGB) and alpha: additive, so fainter is darker.
                let shade = match &c.tint {
                    Value::Shade(s) | Value::ShadeRange(s, _) | Value::ShadeBell(s, _) => *s,
                    Value::Var(i) => match p.vars.get(*i) {
                        Some(p3::Val::U(u)) => *u,
                        _ => 0xFFFF_FFFF,
                    },
                    _ => 0xFFFF_FFFF,
                };
                // (A lens flare: as far as it is seen; over everything.)
                let lens = c.lens.as_ref().map(|_| self.lens_alpha(h).unwrap_or(0.0));
                if lens == Some(0.0) {
                    return None;
                }
                // (A fading flare fades with the fog, as a thing at its depth
                // would: `P3rModifyAlphaByFogFromDepth`.)
                let fog = match c.lens.as_ref() {
                    Some(l) if l.fade.is_some() => self.flare_fog(sys.world_position(p)),
                    _ => 1.0,
                };
                if fog <= 0.0 {
                    return None;
                }
                let alpha = read(&c.alpha, &p.vars).unwrap_or(1.0).clamp(0.0, 1.0)
                    * lens.unwrap_or(1.0)
                    * fog;
                let channel = |shift: u32| ((shade >> shift) & 0xFF) as f32 / 255.0 * alpha;
                Some(crate::render::Sprite {
                    at: sys.world_position(p),
                    size: [x, y],
                    mode,
                    rotation,
                    color: [channel(16), channel(8), channel(0)],
                    texture: self.particle_texture(p),
                    contrail: false,
                    no_depth: lens.is_some(),
                })
            })
            .chain(
                (0..self.fighters.len())
                    .filter_map(|who| self.gun_sight(who).map(|s| (who, s)))
                    .flat_map(|(who, s)| self.sight_sprites(who, &s, &textures)),
            )
            .chain(self.powerup_glows())
            .chain(self.contrails())
            .collect()
    }

    /// The contrails (`P3rDisplayClass_Contrail`, `M3rContrail_Draw`): each
    /// particle of a contrail class joined to the one it links to by a
    /// band — across its X (`Contrail`, sprite type 5) or square to the
    /// link and the line of sight (`ContrailRotated`, 6) at its scale;
    /// v 0 at it, 1 at the link; its colour at its end, the link's at the
    /// other, both at its alpha. A contrail emitter draws the part of the
    /// band to its newest emission that has grown since it was emitted.
    /// How much of a lens flare at `at` the fog leaves (`gl_calculate_fog_factor`
    /// at its screen depth, linear from the fog's start to its end). As
    /// shipped (Q28) a flare past the end is left whole.
    fn flare_fog(&self, at: Vec3) -> f32 {
        let f = &self.fog.fog;
        let (start, end) = (f.start, f.end);
        if start == end || start >= 1.0 || end <= 0.0 {
            return 1.0;
        }
        let clip = self.camera.render_camera(4.0 / 3.0).view_proj() * at.extend(1.0);
        if clip.w <= 1e-6 {
            return 1.0;
        }
        // (Screen depth as the fog has it: −1 near, 1 far.)
        let z = clip.z / clip.w * 2.0 - 1.0;
        if z <= start {
            1.0
        } else if z >= end {
            if self.quirks.flares_unfogged_past_fog_end() {
                1.0
            } else {
                0.0
            }
        } else {
            (end - z) / (end - start)
        }
    }

    /// How many contrail links are drawn now.
    pub fn contrail_count(&self) -> usize {
        self.contrails().len()
    }

    fn contrails(&self) -> Vec<crate::render::Sprite> {
        use p3::class::{cflag, Link, Value};
        let sys = &self.armory.particles;
        let cam = self.camera.view.location;
        let read = |v: &Value, vars: &[p3::Val]| -> Option<f32> {
            match v {
                Value::Float(x) => Some(*x),
                Value::Range(a, b) => Some((a + b) / 2.0),
                Value::Bell(m, _) => Some(*m),
                Value::Var(i) => vars.get(*i)?.f(),
                _ => None,
            }
        };
        let tint = |p: &p3::Particle| -> u32 {
            match &sys.class(p.class).tint {
                Value::Shade(s) | Value::ShadeRange(s, _) | Value::ShadeBell(s, _) => *s,
                Value::Var(i) => match p.vars.get(*i) {
                    Some(p3::Val::U(u)) => *u,
                    _ => 0xFFFF_FFFF,
                },
                _ => 0xFFFF_FFFF,
            }
        };
        let live = |h: p3::Handle| {
            sys.get(h)
                .filter(|q| q.flags & p3::pflag::BREAK_LINKS_TO_ME == 0)
        };
        // A particle's half-width at its end of a link toward `to`.
        let width = |p: &p3::Particle, at: Vec3, to: Vec3| -> Option<Vec3> {
            let c = sys.class(p.class);
            let scale = read(&c.scale, &p.vars)?;
            Some(if c.sprite_type == 5 {
                if !c.has(cflag::ORIENTATION) {
                    return None;
                }
                sys.world_orientation(p).x_axis * scale
            } else {
                let w = (to - at).cross(at - cam);
                let l = w.length();
                if l > 0.01 {
                    w * (scale / l)
                } else {
                    w
                }
            })
        };
        // Its width as the far end of a link: its own (toward its own link),
        // or — first in its chain, or its link gone — the near end's.
        let far_width = |q: &p3::Particle, at: Vec3, near: Vec3| -> Vec3 {
            q.link
                .and_then(live)
                .and_then(|r| width(q, at, sys.world_position(r)))
                .unwrap_or(near)
        };
        let mut out = Vec::new();
        for (_, p) in sys.iter() {
            let c = sys.class(p.class);
            if !matches!(c.sprite_type, 5 | 6)
                || p.flags & p3::pflag::HIDDEN != 0
                || c.has(cflag::INVISIBLE)
                || c.instance.is_empty()
            {
                continue;
            }
            // What it joins to, and how much of the band shows.
            let (link, start_v) = if c.contrail_emitter {
                let Some(k) = c
                    .emitters
                    .iter()
                    .enumerate()
                    .position(|(k, e)| e.link == Link::LastEmission(k))
                else {
                    continue;
                };
                let Some(st) = p.emitters.get(k) else {
                    continue;
                };
                let Some(l) = st.last.and_then(live) else {
                    continue;
                };
                let interval = read(&c.emitters[k].rate.1[0], &p.vars).unwrap_or(1.0);
                let since = (p.time - st.last_time) as f32;
                let v = if since < 0.0 {
                    1.0
                } else if since > interval {
                    0.0
                } else {
                    1.0 - since / interval
                };
                (l, v)
            } else {
                let Some(l) = p.link.and_then(live) else {
                    continue;
                };
                (l, 0.0)
            };
            let (at, to) = (sys.world_position(p), sys.world_position(link));
            let along = to - at;
            if c.max_contrail * c.max_contrail > 0.001
                && along.length_squared() > c.max_contrail * c.max_contrail
            {
                continue;
            }
            let Some(w0) = width(p, at, to) else { continue };
            let w1 = far_width(link, to, w0);
            // Its alpha: fading out, its life left; else its appearance's.
            let mut alpha = if p.flags & p3::pflag::FADE_OUT != 0 && p.fadeout_time > 0.0 {
                (p.lifetime / p.fadeout_time).clamp(0.0, 1.0)
            } else {
                read(&c.alpha, &p.vars).unwrap_or(1.0).clamp(0.0, 1.0)
            };
            if let Some((lo, hi)) = c.edge_fade {
                let edge = 1.0
                    - along
                        .normalize_or_zero()
                        .dot((at - cam).normalize_or_zero())
                        .abs();
                if edge < lo {
                    continue;
                } else if edge < hi {
                    alpha *= (edge - lo) / (hi - lo);
                }
            }
            let rgb = |shade: u32| {
                let ch = |shift: u32| ((shade >> shift) & 0xFF) as f32 / 255.0 * alpha;
                [ch(16), ch(8), ch(0)]
            };
            let (c0, c1) = (rgb(tint(p)), rgb(tint(link)));
            out.push(crate::render::Sprite {
                at,
                size: [0.0, 0.0],
                mode: crate::render::SpriteMode::Quad([
                    (at + w0, [0.0, start_v], c0),
                    (at - w0, [1.0, start_v], c0),
                    (to - w1, [1.0, 1.0], c1),
                    (to + w1, [0.0, 1.0], c1),
                ]),
                rotation: 0.0,
                color: c0,
                texture: self.particle_texture(p),
                contrail: true,
                no_depth: false,
            });
        }
        out
    }

    /// The sprites drawing a gun's sight (`ONiDrawLaserSight`,
    /// `ONiDrawTunnel`): the laser, a camera-facing contrail 0.2 to each
    /// side in the laser's colour (`laser_contrail`, additive); the tunnel's
    /// sprites at 5 × their scale, in their shade.
    fn sight_sprites(
        &self,
        who: usize,
        s: &GunSight,
        textures: &std::path::Path,
    ) -> Vec<crate::render::Sprite> {
        let mut out = Vec::new();
        let along = s.to - s.from;
        if let Some(color) = s.laser.filter(|_| along.length() > 1e-3) {
            out.push(crate::render::Sprite {
                at: (s.from + s.to) * 0.5,
                size: [along.length() * 0.5, LASER_WIDTH],
                mode: crate::render::SpriteMode::Billboard(along),
                rotation: 0.0,
                color,
                texture: textures.join("laser_contrail.png"),
                contrail: true,
                no_depth: false,
            });
        }
        let class = self.fighters[who]
            .hand
            .map(|w| &self.armory.weapons[w].class);
        if let Some(t) = class.and_then(|c| c.tunnel.as_ref()) {
            for &at in &s.tunnel {
                let size = 5.0 * t.scale;
                out.push(crate::render::Sprite {
                    at,
                    size: [size, size],
                    mode: crate::render::SpriteMode::Facing,
                    rotation: 0.0,
                    color: t.color,
                    texture: textures.join(format!("{}.png", t.texture)),
                    contrail: false,
                    no_depth: false,
                });
            }
        }
        // The player's reticle (`ONrGameState_DisplayAiming`, `WPrDrawSprite`):
        // at the sight's end — the targeted one on a character — sized by
        // the class: a fixed 5, 0.7 × distance^0.3 (half scale), or 0.03 ×
        // distance (no scale), times its own scale.
        if let Some(c) = class.filter(|_| who == 0) {
            let sprite = if s.target == SightTarget::Character {
                c.cursor_targeted.as_ref()
            } else {
                c.cursor.as_ref()
            };
            if let Some(sp) = sprite {
                let d = self.camera.view.location.distance(s.to);
                let size = if c.has(crate::weapons::flag::NO_SCALE) {
                    0.03 * d
                } else if c.has(crate::weapons::flag::HALF_SCALE) {
                    0.7 * d.powf(0.3)
                } else {
                    5.0
                } * sp.scale;
                out.push(crate::render::Sprite {
                    at: s.to,
                    size: [size * 0.5, size * 0.5],
                    mode: crate::render::SpriteMode::Facing,
                    rotation: 0.0,
                    color: sp.color,
                    texture: textures.join(format!("{}.png", sp.texture)),
                    contrail: false,
                    no_depth: false,
                });
            }
        }
        out
    }

    pub fn weapons(&self) -> &[Weapon] {
        &self.armory.weapons
    }

    pub fn particles(&self) -> &p3::System {
        &self.armory.particles
    }

    /// A new gun of `class` lying at `at` (falling there unless `placed`).
    pub fn spawn_weapon(&mut self, class: &str, at: Vec3, yaw: f32, placed: bool) -> Option<usize> {
        let c = self.armory.classes.get(class)?;
        self.armory.weapons.push(Weapon::new(c, at, yaw, placed));
        Some(self.armory.weapons.len() - 1)
    }

    /// Puts a new gun of `class` in fighter `who`'s hand (the one it held
    /// goes to the floor).
    /// (The gun it held is deleted, as `chr_giveweapon` does.)
    pub fn give_weapon(&mut self, who: usize, class: &str) -> bool {
        let Some(at) = self.fighters.get(who).map(|f| f.feet()) else {
            return false;
        };
        let Some(w) = self.spawn_weapon(class, at, 0.0, true) else {
            return false;
        };
        if let Some(old) = self.fighters[who].hand.take() {
            self.delete_weapon(old);
        }
        self.take_weapon(who, w);
        true
    }

    /// Fighter `who` takes gun `w` into its hand, dropping what it held.
    pub fn take_weapon(&mut self, who: usize, w: usize) {
        // (The player's first of a class: its message, `ONrInGameUI_NewWeapon`.)
        if who == 0 && self.fighters[who].player {
            let instance = self.armory.weapons[w].class.instance.clone();
            self.hud.new_weapon(&instance);
            let class = self.armory.weapons[w].class.name.clone();
            self.hud.weapon_met(&class);
            self.keep_unlocks();
        }
        if let Some(old) = self.fighters[who].hand.take() {
            self.drop_weapon_index(old, None);
        }
        self.armory.weapons[w].assign(who);
        self.fighters[who].hand = Some(w);
        self.set_weapon_varient(who);
        // In the hand from this frame on (the original rebuilds the
        // weapon's matrix before drawing); the swing to the aim starts over.
        let f = &mut self.fighters[who];
        f.exact_aim_since = self.now as u64;
        f.exact_aim_done = false;
        let m = hand_matrix(f, &f.bones());
        self.armory.weapons[w].matrix.set(m);
    }

    /// Fighter `who` lets go of the gun in its hand (`knock`: thrown clear,
    /// as a knockdown does).
    pub fn drop_weapon(&mut self, who: usize, knock: bool) {
        let Some(w) = self.fighters.get_mut(who).and_then(|f| f.hand.take()) else {
            return;
        };
        let kick =
            knock.then(|| self.fighters[who].last_impact * self.armory.weapons[w].kick_scale());
        self.drop_weapon_index(w, kick);
        self.set_weapon_varient(who);
    }

    /// Gun `w`, held by no one, falls to the floor (`WPrMagicDrop`).
    pub(super) fn drop_weapon_free(&mut self, w: usize) {
        self.drop_weapon_index(w, None);
    }

    fn drop_weapon_index(&mut self, w: usize, kick: Option<Vec3>) {
        let Armory {
            particles, weapons, ..
        } = &mut self.armory;
        let now = self.now as u64;
        let reset = self.quirks.drop_resets_cooldown();
        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        weapons[w].release(particles, &mut host, kick, reset);
    }

    /// The pistol / rifle animation variant for what fighter `who` holds.
    pub(super) fn set_weapon_varient(&mut self, who: usize) {
        use crate::anim::types::var;
        let rifle = self.fighters[who]
            .hand
            .map(|w| self.armory.weapons[w].class.has(flag::TWO_HANDED));
        let f = &mut self.fighters[who];
        f.motion.varient &=
            !(var::RIGHTY_PISTOL | var::RIGHTY_RIFLE | var::LEFTY_PISTOL | var::LEFTY_RIFLE);
        // (Its gun's poses off: as unarmed, `ONrCharacter_SetWeaponVarient`.)
        match rifle.filter(|_| !f.no_gun_varient) {
            Some(true) => f.motion.varient |= var::RIGHTY_RIFLE,
            Some(false) => f.motion.varient |= var::RIGHTY_PISTOL,
            None => {}
        }
    }

    /// Whether fighter `who` holds a gun it can fire (loaded, or with
    /// something to reload it with).
    pub fn armed(&self, who: usize) -> bool {
        self.fighters
            .get(who)
            .and_then(|f| f.hand)
            .is_some_and(|w| !self.fighters[who].weapon_empty || self.armory.weapons[w].ammo > 0)
    }

    /// Fighter `who` has spare ammo for gun `w`.
    fn has_reload(&self, who: usize, w: usize) -> bool {
        let f = &self.fighters[who];
        if f.infinite_ammo {
            return true;
        }
        match self.armory.weapons[w].ammo_type() {
            Some(0) => f.inventory.ammo > 0,
            Some(1) => f.inventory.cells > 0,
            _ => false,
        }
    }

    /// Starts a reload (`WPrTryReload`): refused when full, busy or just
    /// fired, or with nothing to reload with; a clip is used at once, and
    /// the gun fills at the reload clip's action frame.
    pub fn reload(&mut self, who: usize) -> bool {
        let Some(w) = self.fighters.get(who).and_then(|f| f.hand) else {
            return false;
        };
        let gun = &self.armory.weapons[w];
        let f = &self.fighters[who];
        if gun.ammo >= gun.class.max_ammo
            || gun.reload_delay > 0
            || gun.chamber > 0
            || f.overlays[crate::fighter::INVENTORY].is_some()
        {
            return false;
        }
        if !self.has_reload(who, w)
            || !f.motion.current().attacks.is_empty()
            || reaction::is_victim(f.motion.current().anim_type)
        {
            return false;
        }
        let energy = gun.class.has(flag::ENERGY);
        let reload_type = oni_type(&gun.class.reload_anim);
        if !self.start_overlay(
            who,
            crate::fighter::INVENTORY,
            reload_type,
            crate::anim::types::st::STANDING,
            false,
        ) {
            eprintln!("weapon: fighter {who} has no reload clip");
            return false;
        }
        let f = &mut self.fighters[who];
        if !f.infinite_ammo {
            if energy {
                f.inventory.cells -= 1;
            } else {
                f.inventory.ammo -= 1;
            }
        }
        eprintln!("weapon: fighter {who} reloads");
        true
    }

    /// Starts clip type `kind` (looked up from state `from`) in overlay
    /// `slot`; false if the character has no such clip.
    fn start_overlay(
        &mut self,
        who: usize,
        slot: usize,
        kind: u16,
        from: u16,
        magic: bool,
    ) -> bool {
        let f = &mut self.fighters[who];
        let Some(clip) = f.motion.coll.lookup(kind, from, f.motion.varient) else {
            return false;
        };
        f.overlays[slot] = Some(crate::fighter::Overlay {
            clip,
            frame: 0,
            magic,
        });
        true
    }

    /// Holster (`magic`: forced, into the hidden slot) — the clip plays,
    /// the gun goes at its action frame. Refused for unstorable guns and
    /// during attacks and hit reactions.
    fn holster(&mut self, who: usize, magic: bool) -> bool {
        let f = &self.fighters[who];
        let Some(w) = f.hand else { return false };
        if self.armory.weapons[w].class.has(flag::UNSTORABLE) && !magic {
            return false;
        }
        if !f.motion.current().attacks.is_empty()
            || reaction::is_victim(f.motion.current().anim_type)
        {
            return false;
        }
        self.start_overlay(
            who,
            crate::fighter::INVENTORY,
            clip::HOLSTER,
            crate::anim::types::st::STANDING,
            magic,
        )
    }

    /// Draw the holstered gun (or the hidden slot's): the clip plays, the gun
    /// comes to hand at its action frame.
    fn draw(&mut self, who: usize) -> bool {
        let f = &self.fighters[who];
        let Some(w) = f.magic.or(f.holster) else {
            return false;
        };
        if !f.motion.current().attacks.is_empty()
            || reaction::is_victim(f.motion.current().anim_type)
        {
            return false;
        }
        let kind = if self.armory.weapons[w].class.has(flag::TWO_HANDED) {
            clip::DRAW_RIFLE
        } else {
            clip::DRAW_PISTOL
        };
        self.start_overlay(
            who,
            crate::fighter::INVENTORY,
            kind,
            crate::anim::types::st::STANDING,
            false,
        )
    }

    /// The gun from the holster (or the hidden slot) into the empty hand.
    fn bring_to_hand(&mut self, who: usize) {
        let f = &mut self.fighters[who];
        if f.hand.is_some() {
            return;
        }
        f.hand = if f.magic.is_some() {
            f.magic.take()
        } else {
            f.holster.take()
        };
        if let Some(w) = f.hand {
            self.armory.weapons[w].assign(who);
        }
        self.set_weapon_varient(who);
    }

    /// The gun in hand into the holster — swapping with what was there —
    /// or (`magic`) into the hidden slot, or onto the floor if that is taken.
    pub(super) fn put_away(&mut self, who: usize, magic: bool) {
        let Some(w) = self.fighters[who].hand else {
            return;
        };
        if magic && self.fighters[who].magic.is_some() {
            self.drop_weapon(who, false);
            return;
        }
        {
            let Armory {
                particles, weapons, ..
            } = &mut self.armory;
            let now = self.now as u64;
            let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
            weapons[w].stow(particles, &mut host);
        }
        let f = &mut self.fighters[who];
        f.hand = None;
        if magic {
            f.magic = Some(w);
        } else {
            f.hand = f.holster.take();
            f.holster = Some(w);
            if let Some(n) = f.hand {
                self.armory.weapons[n].assign(who);
            }
        }
        self.set_weapon_varient(who);
    }

    /// Overlays advance a frame; each acts at its clip's action frame first
    /// (`docs/WEAPONS.md`, *Overlays*). A knockdown cancels the inventory
    /// one — a reload not yet at its action frame giving back the clip it
    /// took (shipped, the clip is lost: Q26).
    fn overlays_tick(&mut self, who: usize) {
        let refund = !self.quirks.interrupted_reload_loses_clip();
        let f = &mut self.fighters[who];
        if let Some(o) = f.overlays[crate::fighter::INVENTORY] {
            if reaction::is_knockdown(f.motion.current().anim_type) {
                f.overlays[crate::fighter::INVENTORY] = None;
                let c = f.motion.clip_of(o.clip);
                let unfilled =
                    clip::is_reload(c.anim_type) && c.action_frame.is_some_and(|a| o.frame <= a);
                if refund && unfilled && !f.infinite_ammo {
                    match f.hand.and_then(|w| self.armory.weapons[w].ammo_type()) {
                        Some(0) => f.inventory.ammo += 1,
                        Some(1) => f.inventory.cells += 1,
                        _ => {}
                    }
                }
            }
        }
        for slot in [
            crate::fighter::RECOIL,
            crate::fighter::INVENTORY,
            crate::fighter::HIT_REACTION,
        ] {
            let f = &self.fighters[who];
            let Some(o) = f.overlays[slot] else { continue };
            // (An overlay's own animation sound, on its own frame.)
            self.anim_sound(who, o.clip, o.frame, false);
            let f = &self.fighters[who];
            let c = f.motion.clip_of(o.clip);
            let (kind, frames, action) = (c.anim_type, c.frames(), c.action_frame);
            if action == Some(o.frame) && slot == crate::fighter::INVENTORY {
                if clip::is_reload(kind) {
                    if let Some(w) = f.hand {
                        let Armory {
                            particles, weapons, ..
                        } = &mut self.armory;
                        let now = self.now as u64;
                        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
                        weapons[w].reloaded(particles, &mut host);
                    }
                } else if kind == clip::HOLSTER {
                    self.put_away(who, o.magic);
                    // A holstered gun coming out: its draw clip, from its
                    // action frame.
                    let f = &self.fighters[who];
                    if !o.magic {
                        if let Some(w) = f.hand {
                            let kind = if self.armory.weapons[w].class.has(flag::TWO_HANDED) {
                                clip::DRAW_RIFLE
                            } else {
                                clip::DRAW_PISTOL
                            };
                            if self.start_overlay(
                                who,
                                crate::fighter::INVENTORY,
                                kind,
                                crate::anim::types::st::STANDING,
                                false,
                            ) {
                                let f = &mut self.fighters[who];
                                if let Some(n) = f.overlays[crate::fighter::INVENTORY].as_mut() {
                                    n.frame = f.motion.clip_of(n.clip).action_frame.unwrap_or(0);
                                }
                            }
                            continue;
                        }
                    }
                } else if kind == clip::DRAW_PISTOL || kind == clip::DRAW_RIFLE {
                    self.bring_to_hand(who);
                }
            }
            let f = &mut self.fighters[who];
            if let Some(o) = f.overlays[slot].as_mut() {
                o.frame += 1;
                if o.frame >= frames {
                    f.overlays[slot] = None;
                }
            }
        }
    }

    /// A pickup under way: at the pickup clip's action frame the item is
    /// taken, if still within the wider reach (`docs/WEAPONS.md`, *Pickup*).
    fn pickup_tick(&mut self, who: usize) {
        let f = &mut self.fighters[who];
        let Some((item, started, waited)) = f.pickup else {
            return;
        };
        let c = f.motion.current();
        if !clip::is_pickup(c.anim_type) {
            f.pickup = if started || waited > 10 {
                None
            } else {
                Some((item, false, waited + 1))
            };
            return;
        }
        f.pickup = Some((item, true, waited));
        let at = c
            .action_frame
            .unwrap_or(0)
            .min(c.frames().saturating_sub(1));
        if f.motion.frame != at {
            return;
        }
        f.pickup = None;
        let feet = f.feet();
        let near = |p: Vec3| {
            let d = ((p - feet) * Vec3::new(1.0, 0.0, 1.0)).length();
            d < PICKUP_FINISH_RADIUS
                && (-PICKUP_FINISH_BELOW..=PICKUP_FINISH_ABOVE).contains(&(p.y - feet.y))
        };
        match item {
            Item::Weapon(w) => {
                let g = &self.armory.weapons[w];
                if g.owner.is_none() && !g.gone && near(g.pickup_position()) {
                    self.take(who, item);
                }
            }
            Item::Powerup(k) => {
                if self
                    .armory
                    .powerups
                    .get(k)
                    .is_some_and(|p| near(p.position))
                {
                    self.take(who, item);
                }
            }
        }
    }

    /// Takes an item now.
    fn take(&mut self, who: usize, item: Item) {
        match item {
            Item::Weapon(w) => {
                eprintln!(
                    "weapon: fighter {who} picks up {}",
                    self.armory.weapons[w].class.name
                );
                self.take_weapon(who, w);
            }
            Item::Powerup(k) => {
                let p = self.armory.powerups[k];
                if self.give_powerup(who, POWERUPS[p.kind], Some(p.amount), false) > 0 {
                    // (Taken whole, even if only part of it fitted.)
                    self.armory.powerups.remove(k);
                    eprintln!("weapon: fighter {who} picks up {}", POWERUPS[p.kind]);
                }
            }
        }
    }

    /// `ai2_barabbas_retrievegun`: Barabbas's gun (`WPrFindBarabbasWeapon`:
    /// the first gun of an unreloadable class) back in his hand at once —
    /// unless he holds a gun, someone else holds it, or it is up on the
    /// building's top floor (above 700).
    pub fn barabbas_retrieve_gun(&mut self, who: usize) -> bool {
        if self.fighters.get(who).is_none_or(|f| f.hand.is_some()) {
            return false;
        }
        let Some(w) = self
            .armory
            .weapons
            .iter()
            .position(|g| !g.gone && g.class.flags & flag::UNRELOADABLE != 0)
        else {
            eprintln!("### ai2_barabbas_retrievegun: can't find barabbas's weapon");
            return false;
        };
        let g = &self.armory.weapons[w];
        if g.owner.is_some() || g.pickup_position().y > 700.0 {
            return false;
        }
        self.take(who, Item::Weapon(w));
        true
    }

    /// The gun fighter `who` could reach for: the nearest free one within
    /// reach of its feet, if its holster is empty (`docs/WEAPONS.md`,
    /// *Pickup*).
    pub fn weapon_in_reach(&self, who: usize) -> Option<usize> {
        let f = self.fighters.get(who)?;
        if f.holster.is_some() {
            return None;
        }
        let feet = f.feet();
        self.armory
            .weapons
            .iter()
            .enumerate()
            .filter(|(_, g)| g.owner.is_none() && !g.gone)
            .filter_map(|(i, g)| {
                let p = g.pickup_position();
                let d = (p - feet) * Vec3::new(1.0, 0.0, 1.0);
                let dy = p.y - feet.y;
                (d.length() < PICKUP_RADIUS && (-PICKUP_BELOW..=PICKUP_ABOVE).contains(&dy))
                    .then_some((i, d.length()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    /// The swap key (not while an inventory clip plays): pick up what is
    /// in reach — powerups first — bending for it with the pickup clip (at
    /// once in a roll or slide); else holster the gun in hand, or draw the
    /// holstered one.
    pub fn swap(&mut self, who: usize) {
        if self.fighters[who].overlays[crate::fighter::INVENTORY].is_some() {
            return;
        }
        let item = self
            .powerup_in_reach(who)
            .map(Item::Powerup)
            .or_else(|| self.weapon_in_reach(who).map(Item::Weapon));
        if let Some(item) = item {
            self.start_pickup(who, item);
            return;
        }
        let f = &self.fighters[who];
        if f.hand.is_some() {
            self.holster(who, false);
        } else if f.holster.is_some() {
            self.draw(who);
        }
    }

    /// Bends for `item` with the pickup clip (object, pistol or rifle; the
    /// mid-height one for items more than 9 above the feet) — or, in a roll
    /// or slide, takes it at once. True if it started.
    pub fn start_pickup(&mut self, who: usize, item: Item) -> bool {
        {
            let f = &self.fighters[who];
            if f.motion
                .current()
                .has_flag(crate::anim::types::flag::CAN_PICKUP)
            {
                self.take(who, item);
                return true;
            }
            let (at, rifle) = match item {
                Item::Weapon(w) => (
                    self.armory.weapons[w].pickup_position(),
                    Some(self.armory.weapons[w].class.has(flag::TWO_HANDED)),
                ),
                Item::Powerup(k) => (self.armory.powerups[k].position, None),
            };
            let mid = at.y > f.feet().y + PICKUP_MID;
            let kind = match (rifle, mid) {
                (None, false) => clip::PICKUP_OBJECT,
                (None, true) => clip::PICKUP_OBJECT_MID,
                (Some(false), false) => clip::PICKUP_PISTOL,
                (Some(false), true) => clip::PICKUP_PISTOL_MID,
                (Some(true), false) => clip::PICKUP_RIFLE,
                (Some(true), true) => clip::PICKUP_RIFLE_MID,
            };
            let f = &mut self.fighters[who];
            let ch = f.character.clone();
            if f.motion
                .request(&ch, crate::anim::motion::Urge::Appropriate, false, kind)
                .is_some()
            {
                f.pickup = Some((item, false, 0));
                return true;
            }
            false
        }
    }

    /// On a fire 1 press, whether fighter `who`'s gun is empty with nothing
    /// to reload it — decided before its buttons are read, since an empty gun
    /// turns fire 1 back into a punch.
    pub fn check_empty(&mut self, who: usize, held: u32) {
        let last = self.armory.last_fire.get(who).copied().unwrap_or(0);
        let Some(w) = self.fighters.get(who).and_then(|f| f.hand) else {
            if let Some(f) = self.fighters.get_mut(who) {
                f.weapon_empty = false;
            }
            return;
        };
        if held & !last & key::FIRE1 != 0 {
            let empty = self.armory.weapons[w].ammo == 0
                && !self.has_reload(who, w)
                && !self.fighters[who].reloading();
            self.fighters[who].weapon_empty = empty;
        }
    }

    /// Fighter `who`'s fire controls this tick → its gun (`docs/WEAPONS.md`,
    /// *Firing*). Fire 1 shoots (and wins over fire 2, the alternate).
    fn fire_controls(&mut self, who: usize, held: u32) {
        if self.armory.last_fire.len() < self.fighters.len() {
            self.armory.last_fire.resize(self.fighters.len(), 0);
        }
        let last = std::mem::replace(&mut self.armory.last_fire[who], held);
        let pressed = held & !last;
        let released = last & !held;
        if pressed & key::SWAP != 0 {
            self.swap(who);
        }
        if pressed & key::RELOAD != 0 {
            // (A pointless reload — full, or nothing to load — fails aloud
            // for the player.)
            let pointless = self.fighters[who].hand.is_some_and(|w| {
                let gun = &self.armory.weapons[w];
                gun.ammo >= gun.class.max_ammo || !self.has_reload(who, w)
            });
            self.reload(who);
            if pointless && self.fighters[who].player {
                self.event_sound("inventoryFail", None);
            }
        }
        if pressed & key::DROP != 0 {
            self.drop_weapon(who, false);
        }
        let Some(w) = self.fighters[who].hand else {
            self.fighters[who].weapon_empty = false;
            return;
        };
        let motion = Vec3::ZERO;
        let now = self.now as u64;
        // (A gun through a wall stops firing.)
        let busy = self.fighters[who].weapon_empty
            || self.fighters[who].overlays[crate::fighter::INVENTORY].is_some()
            || self.gun_through_wall(who);
        let inaccuracy = self.inaccuracies();
        let Armory {
            particles, weapons, ..
        } = &mut self.armory;
        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        host.inaccuracy = inaccuracy;
        let gun = &mut weapons[w];
        if busy {
            gun.release_trigger(particles, &mut host, false);
            return;
        }
        let mut empty = false;
        let mut single = false;
        if held & key::FIRE1 != 0 {
            single = pressed & key::FIRE1 != 0;
            let (_, e) = gun.depress(particles, &mut host, false, false, motion);
            empty = e;
        } else if released & key::FIRE1 != 0 {
            gun.release_trigger(particles, &mut host, false);
        } else if pressed & key::FIRE2 != 0 && gun.class.has(flag::HAS_ALTERNATE) {
            single = true;
            empty = gun.depress(particles, &mut host, true, false, motion).1;
        } else if released & key::FIRE2 != 0 {
            gun.release_trigger(particles, &mut host, false);
        }
        drop(host);
        if empty {
            // Out of ammo: a single press reloads; a held trigger just clicks.
            gun_release(
                &mut self.armory,
                &mut self.fighters,
                Some(&self.quads),
                now,
                w,
            );
            let click = if single || !self.fighters[who].player {
                let reloaded = self.reload(who);
                if !reloaded {
                    self.armory.weapons[w].reports.push(Report::Empty);
                }
                !reloaded
            } else {
                true
            };
            if click {
                self.empty_click(w);
            }
        }
    }

    /// Gun `w` clicks empty (unless chambering or reloading) at the gun; an
    /// automatic one then waits its shooter's chamber time to click again.
    fn empty_click(&mut self, w: usize) {
        let gun = &mut self.armory.weapons[w];
        if gun.chamber > 0 || gun.reload_time > 0 || gun.class.empty_sound.is_empty() {
            return;
        }
        let at = gun.matrix.get().w_axis.truncate();
        if gun.class.has(flag::AUTOFIRE) {
            if let Some(a) = gun.class.attachments.iter().find(|a| a.ammo_used > 0) {
                (gun.chamber, gun.chamber_total) = (a.chamber_time, a.chamber_time);
            }
        }
        let sound = gun.class.empty_sound.clone();
        self.audio
            .impulse_play(&sound, Some(at), None, Some(&self.quads));
    }

    /// The player's auto-aim (`docs/WEAPONS.md`, *Auto-aim*): the turn
    /// (yaw, pitch) that would put gun `w`'s shot on the one hostile within
    /// reach of the aim — none when two qualify. It steers the gun only;
    /// the player's own aim stays where it is.
    fn auto_aim(&self, who: usize, w: usize) -> (f32, f32) {
        let f = &self.fighters[who];
        let gun = &self.armory.weapons[w];
        let Some(shooter) = gun
            .class
            .attachments
            .iter()
            .find(|a| a.ammo_used > 0 && a.shooter == 0)
        else {
            return (0.0, 0.0);
        };
        let m = gun.matrix.get();
        let muzzle = m.transform_point3(shooter.matrix.w_axis.truncate());
        // The shot's line in gun space, as angles off the barrel (+X; +Y
        // left, +Z up).
        let s = shooter.matrix.y_axis.truncate().normalize_or(Vec3::X);
        let (off_yaw, off_pitch) = (s.y.atan2(s.x), s.z.clamp(-1.0, 1.0).asin());
        let (aim_yaw, aim_pitch) = (f.facing.aim() + f.aiming_lr, f.aiming_ud);
        let me = f.team();
        let pelvis = f.feet() + Vec3::Y * f.motion.height();
        use crate::angle::wrap_pi as wrap;
        let mut found: Option<(f32, f32)> = None;
        for (i, t) in self.fighters.iter().enumerate() {
            if i == who
                || !t.alive()
                || t.team().regards(me) != crate::ai::perception::Relation::Hostile
            {
                continue;
            }
            let chest = t.feet() + Vec3::Y * (t.motion.height() + 4.0);
            let to = chest - muzzle;
            let dist = to.length();
            if !(1.0..=500.0).contains(&dist) {
                continue;
            }
            let mid = t.feet() + Vec3::Y * t.motion.height();
            if !self
                .quads
                .clear(mid, pelvis, crate::quads::flag::LOS_SEE_PLAYER)
            {
                continue;
            }
            let d = to / dist;
            let want_yaw = crate::angle::yaw_of(d) - off_yaw;
            let want_pitch = d.y.asin() - off_pitch;
            let (dy, dp) = (wrap(want_yaw - aim_yaw), want_pitch - aim_pitch);
            let worst = dy.abs().max(dp.abs());
            if worst >= 0.35 {
                continue;
            }
            if found.is_some() {
                // Two candidates: no help at all.
                return (0.0, 0.0);
            }
            let weight = if worst <= 0.15 {
                1.0
            } else {
                (0.35 - worst) / 0.2
            };
            let fade = ((195.0 - dist) / 60.0).clamp(0.0, 1.0);
            found = Some((dy * weight * fade, dp * weight * fade));
        }
        found.unwrap_or((0.0, 0.0))
    }

    /// The weapons' tick: guns follow their holders' hands and fire, free
    /// guns fall and fade, shots fly, recoil settles.
    pub(super) fn weapons_tick(&mut self, controls: &[u32]) {
        let now = self.now as u64;
        // Guns in hands: where the hand puts them.
        for who in 0..self.fighters.len() {
            let Some(w) = self.fighters[who].hand else {
                continue;
            };
            let f = &self.fighters[who];
            if !f.alive() {
                continue;
            }
            let bones = f.bones();
            // Aiming (the player always; an AI with a target) in a clip with
            // a gun's aiming screen: the gun swings to the aim, clipped to
            // the screen's reach; else it rides the hand.
            let aim = if f.player {
                let (lr, ud) = self.auto_aim(who, w);
                self.fighters[who].auto_aim = (lr, ud);
                let f = &self.fighters[who];
                Some((
                    f.facing.aim() + f.aiming_lr + lr,
                    f.aiming_ud + f.recoil + ud,
                ))
            } else {
                f.ai_aim.map(|(y, p)| (y, p + f.recoil))
            };
            let f = &self.fighters[who];
            let hand = hand_matrix(f, &bones);
            let exact = aim.and_then(|a| f.gun_screen().map(|s| (a, s)));
            let m = match exact {
                Some(((yaw, pitch), s)) => {
                    use crate::angle::wrap_pi as wrap;
                    let rel = wrap(yaw - f.facing.facing).clamp(
                        -s.yaw.0[0] * f32::from(s.yaw.1[0]),
                        s.yaw.0[1] * f32::from(s.yaw.1[1]),
                    );
                    let pitch = pitch.clamp(
                        -s.pitch.0[0] * f32::from(s.pitch.1[0]),
                        s.pitch.0[1] * f32::from(s.pitch.1[1]),
                    );
                    let goal = aim_matrix(hand.w_axis.truncate(), f.facing.facing + rel, pitch);
                    let f = &mut self.fighters[who];
                    let since = now.saturating_sub(f.exact_aim_since);
                    if !f.exact_aim_done && since > EXACT_AIM_TICKS {
                        f.exact_aim_done = true;
                    }
                    if f.exact_aim_done {
                        goal
                    } else {
                        matrix_lerp(hand, goal, since as f32 / EXACT_AIM_TICKS as f32)
                    }
                }
                None => {
                    let f = &mut self.fighters[who];
                    f.exact_aim_since = now;
                    f.exact_aim_done = false;
                    hand
                }
            };
            let before = self.armory.weapons[w].position;
            self.armory.weapons[w].infinite_ammo = self.fighters[who].infinite_ammo;
            let Armory {
                particles, weapons, ..
            } = &mut self.armory;
            let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
            let motion = m.w_axis.truncate() - before;
            weapons[w].tick_held(particles, &mut host, m, motion);
        }
        // Overlays and pickups.
        for who in 0..self.fighters.len() {
            self.overlays_tick(who);
            self.pickup_tick(who);
        }
        // Triggers.
        for (who, &held) in controls.iter().enumerate().take(self.fighters.len()) {
            if self.fighters[who].alive() {
                self.fire_controls(who, held);
            }
        }
        // What the guns did: kicks and the noise of the shots.
        for w in 0..self.armory.weapons.len() {
            let reports = std::mem::take(&mut self.armory.weapons[w].reports);
            let Some(who) = self.armory.weapons[w].owner else {
                continue;
            };
            for r in reports {
                match r {
                    Report::Shot { recoil, volume } => {
                        let f = &mut self.fighters[who];
                        let compensation = f
                            .shooting
                            .get(self.armory.weapons[w].class.ai.skill_index)
                            .filter(|_| !f.player)
                            .map_or(0.0, |s| s.recoil_compensation);
                        f.recoil_speed += recoil * (1.0 - compensation);
                        if let Some(crate::ai::Brain::Agent(a)) = f.brain.as_mut() {
                            a.fired();
                        }
                        let kind = oni_type(&self.armory.weapons[w].class.recoil_anim);
                        self.start_overlay(
                            who,
                            crate::fighter::RECOIL,
                            kind,
                            crate::anim::types::st::ANYTHING,
                            false,
                        );
                        if volume > 0.0 {
                            let at = self.fighters[who].feet()
                                + Vec3::Y * self.fighters[who].motion.height();
                            self.sound(
                                crate::ai::perception::Stimulus::SoundGunshot,
                                at,
                                volume,
                                Some(who),
                                None,
                            );
                        }
                    }
                    Report::Empty => eprintln!("weapon: fighter {who} *click*"),
                }
            }
        }
        // Free guns.
        let mut characters: Option<Vec<crate::objphys::Obstacle>> = None;
        for w in 0..self.armory.weapons.len() {
            let gun = &self.armory.weapons[w];
            if gun.owner.is_some() || gun.gone {
                continue;
            }
            // Meeting a surface hard enough: its "Weapon" impact. (Other
            // bodies stop it too: characters, objects, loose things.)
            let others = if self.armory.weapons[w].body.is_some() && !self.armory.weapons[w].resting
            {
                if characters.is_none() {
                    characters = Some(self.character_obstacles());
                }
                self.loose_obstacles(Some(Loose::Weapon(w)), characters.as_deref())
            } else {
                Vec::new()
            };
            for c in self.armory.weapons[w].tick_free(&self.quads, &others) {
                self.fall_impact_on(c, "Weapon");
            }
        }
        self.powerups_tick();
        // The shots.
        let tick = self.now as u32;
        let pane_quads: Vec<u32> = self.glass.iter().map(|p| p.quad).collect();
        let panes: Vec<(bool, Vec3, f32)> = self
            .glass
            .iter()
            .map(|p| (p.broken, p.centre, p.radius))
            .collect();
        let inaccuracy = self.inaccuracies();
        let spread = self.turret_spread();
        let objects = self.object_spheres();
        let mut host = ShotHost::new(&mut self.fighters, Some(&self.quads), now);
        host.pane_quads = pane_quads;
        host.doors = &self.door_boxes;
        host.objects = objects;
        host.set_spread(inaccuracy, spread);
        host.panes = panes;
        host.quads = Some(&self.quads);
        for f in host.fighters.iter_mut() {
            f.update_part_cells();
        }
        self.armory.particles.update(tick, &mut host);
        let sounds = std::mem::take(&mut host.sounds);
        let broke = std::mem::take(&mut host.broke);
        drop(host);
        for (k, damage, blast) in broke {
            self.shatter(k, damage, blast);
        }
        for (at, volume, maker) in sounds {
            self.sound(
                crate::ai::perception::Stimulus::SoundGunshot,
                at,
                volume,
                maker,
                None,
            );
        }
        // Recoil settling (`docs/WEAPONS.md`, *Recoil*).
        for f in &mut self.fighters {
            let Some(w) = f.hand else {
                f.recoil = 0.0;
                f.recoil_speed = 0.0;
                continue;
            };
            let gun = &self.armory.weapons[w];
            let r = gun.class.recoil;
            if f.recoil_speed != 0.0 {
                f.recoil += f.recoil_speed;
                f.recoil_speed /= r.factor;
                if f.recoil_speed < 0.001 {
                    f.recoil_speed = 0.0;
                }
            } else if f.recoil != 0.0 {
                f.recoil -= if gun.chamber == 0 {
                    r.return_speed
                } else {
                    r.firing_return_speed
                };
            }
            if f.player && gun.class.has(flag::RECOIL_DIRECT) {
                // The player's kick stays in the aim: pull it back down.
                f.aiming_ud += f.recoil;
                f.recoil = 0.0;
            } else {
                f.recoil = f.recoil.clamp(0.0, r.max);
            }
            f.aiming_ud = f
                .aiming_ud
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        }
        // A throw's action frame takes the victim's gun
        // (`ONrCharacter_Update`'s throw swap): into the thrower's hand if
        // it has none in hand or stowed, else knocked from the victim's.
        for who in 0..self.fighters.len() {
            let f = &self.fighters[who];
            let Some(victim) = f.throwing else { continue };
            if f.motion.current().action_frame != Some(f.motion.frame)
                || f.motion.current().frames() == 0
            {
                continue;
            }
            let Some(gun) = self.fighters.get(victim).and_then(|v| v.hand) else {
                continue;
            };
            if f.hand.is_none() && f.holster.is_none() {
                self.fighters[victim].hand = None;
                self.set_weapon_varient(victim);
                self.take_weapon(who, gun);
                eprintln!("throw: fighter {who} takes {victim}'s gun");
            } else {
                self.drop_weapon(victim, true);
            }
        }
        // Knocked down: the gun flies.
        for who in 0..self.fighters.len() {
            let f = &self.fighters[who];
            let died = !f.alive() && f.motion.dying;
            if f.hand.is_some()
                && (f
                    .motion
                    .current()
                    .has_flag(crate::anim::types::flag::DROP_WEAPON)
                    || died)
            {
                self.drop_weapon(who, true);
            }
            if died {
                self.drop_inventory(who);
            }
        }
        // (Gone guns stay in the list: fighters hold guns by index.)
    }
}

/// An animation type by its name (`oni_schema::ANIM_TYPES`; 0 if unknown).
fn oni_type(name: &str) -> u16 {
    oni_schema::ANIM_TYPES
        .iter()
        .position(|n| *n == name)
        .unwrap_or(0) as u16
}

fn gun_release(
    armory: &mut Armory,
    fighters: &mut [Fighter],
    sight: Option<&crate::quads::Quads>,
    now: u64,
    w: usize,
) {
    let Armory {
        particles, weapons, ..
    } = armory;
    let mut host = ShotHost::new(fighters, sight, now);
    weapons[w].release_trigger(particles, &mut host, false);
}
