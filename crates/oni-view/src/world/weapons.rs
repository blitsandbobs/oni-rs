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

/// The hand bone a gun hangs from (`right_handfist`).
const HAND_BONE: usize = 18;
/// Picking up: how near (XZ) and how far below / above the feet a gun may
/// be to start reaching for it.
const PICKUP_RADIUS: f32 = 8.0;
const PICKUP_BELOW: f32 = 3.0;
const PICKUP_ABOVE: f32 = 18.0;

/// Damage kinds (`P3tEnumDamageType`).
mod damage {
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
const POWERUP_DATA: [&str; 6] = ["ammoBallistic", "ammoEnergy", "hypo", "shieldBelt", "invisibility", "lsi"];
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
}

/// Guns, their classes and the particles.
pub struct Armory {
    pub classes: Rc<WeaponClasses>,
    pub particles: p3::System,
    pub weapons: Vec<Weapon>,
    pub powerups: Vec<Powerup>,
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
            last_fire: Vec::new(),
        }
    }
}

/// What the particles see of the world while they move.
pub struct ShotHost<'a> {
    pub fighters: &'a mut [Fighter],
    /// Each fighter's posed bones, worked out when first needed.
    bones: Vec<Option<Vec<Mat4>>>,
    sight: Option<&'a crate::physics::Sight>,
    teams: Vec<crate::ai::perception::Team>,
    now: u64,
    /// Noises the hits make (gunshot impacts), for the world to spread.
    pub sounds: Vec<(Vec3, f32, Option<usize>)>,
    /// Glass: where the panes start among the switchable parts, each pane
    /// (broken, middle, reach), and the ones shots broke.
    glass_base: usize,
    panes: Vec<(bool, Vec3, f32)>,
    pub broke: Vec<usize>,
    /// Each fighter's extra spread (degrees) on its shots, and turrets'.
    inaccuracy: Vec<f32>,
    turret_spread: f32,
}

impl<'a> ShotHost<'a> {
    /// Each fighter's extra spread, and the turrets'.
    pub(super) fn set_spread(&mut self, fighters: Vec<f32>, turrets: f32) {
        self.inaccuracy = fighters;
        self.turret_spread = turrets;
    }

    pub(super) fn new(fighters: &'a mut [Fighter], sight: Option<&'a crate::physics::Sight>, now: u64) -> ShotHost<'a> {
        let teams = fighters.iter().map(|f| f.team()).collect();
        let n = fighters.len();
        ShotHost { fighters, bones: vec![None; n], sight, teams, now, sounds: Vec::new(), glass_base: 0, panes: Vec::new(), broke: Vec::new(), inaccuracy: Vec::new(), turret_spread: 0.0 }
    }

    fn bones_of(&mut self, i: usize) -> &[Mat4] {
        if self.bones[i].is_none() {
            self.bones[i] = Some(self.fighters[i].bones());
        }
        self.bones[i].as_deref().unwrap_or_default()
    }

    /// `InflictDamage`: a hit or a blast reaching character `who`. True when
    /// the particle already hit it (a duplicate).
    #[allow(clippy::too_many_arguments)]
    fn inflict(&mut self, who: usize, dir: Vec3, kind: u32, damage: i32, stun: i32, knockback: f32, stamp: Option<u64>, owner: u32) -> bool {
        let attacker = p3::owner::character(owner)
            .and_then(|a| Some((a, self.fighters.get(a)?.feet(), self.teams.get(a).copied())));
        let Some(f) = self.fighters.get_mut(who) else { return false };
        if !f.alive() {
            return false;
        }
        if let Some(s) = stamp {
            if f.last_particle == Some(s) {
                return true;
            }
            f.last_particle = Some(s);
        }
        if f.weapon_immune {
            return false;
        }
        if f.inventory.shield > 0 {
            // A shield belt soaks it all (the excess is lost).
            f.inventory.shield = f.inventory.shield.saturating_sub(damage.max(0) as u16);
            return false;
        }
        let physical = if kind == damage::PICK_UP { 0 } else { damage };
        f.last_impact = dir * (damage as f32).clamp(5.0, 15.0);
        f.take_damage(physical, dir * knockback);
        // Facing the shot, or turned away from it: the reaction's side.
        let from = (-dir).x.atan2(-dir.z);
        let behind = {
            let d = (from - f.facing.facing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            d.abs() > std::f32::consts::FRAC_PI_2
        };
        let side = |t: u16| if behind { reaction::from_behind(t) } else { t };
        let stun = stun.max(0) as u16;
        let current = f.motion.current().anim_type;
        if !f.unstoppable && f.alive() {
            match kind {
                damage::MINOR_STUN if stun > 0 => {
                    f.pending_reaction = ty::STUN;
                    f.motion.dizzy_stun = f.motion.dizzy_stun.max(stun);
                }
                damage::MAJOR_STUN if stun > 0 => {
                    if !matches!(current, ty::STAGGER | ty::STAGGER_BEHIND | ty::STUN) {
                        f.pending_reaction = if behind { ty::STAGGER_BEHIND } else { ty::STAGGER };
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
                            f.pending_reaction = if behind { ty::STAGGER_BEHIND } else { ty::STAGGER };
                        }
                        f.motion.stagger_stun = 22;
                    }
                }
                damage::MAJOR_KNOCKDOWN => f.pending_reaction = side(ty::KNOCKDOWN_HEAD),
                damage::BLOWN_UP => f.pending_reaction = if behind { ty::BLOWNUP_BEHIND } else { ty::BLOWNUP },
                _ => {}
            }
        }
        if let (Some((a, feet, team)), Some(p)) = (attacker, f.perception.as_mut()) {
            use crate::ai::perception::{Stimulus, Strength};
            p.perceive(Some(a), team, Stimulus::HitWeapon, Strength::Definite, feet, damage, self.now);
        }
        let f = &self.fighters[who];
        eprintln!("shot: fighter {who} takes {physical} (kind {kind}, hp {}) reaction {}", f.hp, f.pending_reaction);
        false
    }
}

/// Where a ray `from → from + ray` enters the box `lo..hi`: the fraction
/// along it and the face's outward axis (box space).
fn ray_box(from: Vec3, ray: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, Vec3)> {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let mut normal = Vec3::ZERO;
    for k in 0..3 {
        let (o, d) = (from[k], ray[k]);
        if d.abs() < 1e-9 {
            if o < lo[k] || o > hi[k] {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((lo[k] - o) / d, (hi[k] - o) / d);
        let mut n = Vec3::ZERO;
        n[k] = -d.signum();
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a > t0 {
            t0 = a;
            normal = n;
        }
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, normal))
}

/// Where a ray first touches a sphere (fraction along it; 0 if it starts
/// inside).
fn ray_sphere(from: Vec3, ray: Vec3, centre: Vec3, r: f32) -> Option<f32> {
    let oc = from - centre;
    let c = oc.length_squared() - r * r;
    if c <= 0.0 {
        return Some(0.0);
    }
    let a = ray.length_squared();
    if a < 1e-12 {
        return None;
    }
    let b = oc.dot(ray);
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    (0.0..=1.0).contains(&t).then_some(t)
}

impl ShotHost<'_> {
    /// Breaks pane `k` now (later shots this tick fly through it).
    fn break_pane(&mut self, k: usize) {
        self.panes[k].0 = true;
        self.broke.push(k);
        for f in self.fighters.iter_mut() {
            f.physics.set_door(self.glass_base + k, false);
        }
        self.sounds.push((self.panes[k].1, 300.0, None));
    }
}

impl Host for ShotHost<'_> {
    fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit> {
        let f = self.fighters.first()?;
        let base = self.glass_base;
        let n = self.panes.len();
        f.physics.cast(from, ray).map(|(fraction, normal, part)| {
            // A glass pane: surface = its index + 1.
            let surface = part.filter(|&k| k >= base && k - base < n).map_or(0, |k| (k - base + 1) as u32);
            EnvHit { fraction, normal, surface }
        })
    }

    fn breakable(&self, surface: u32) -> bool {
        surface > 0 && self.panes.get(surface as usize - 1).is_some_and(|p| !p.0)
    }

    fn damage_surface(&mut self, surface: u32, _damage: f32) -> bool {
        if !self.breakable(surface) {
            return false;
        }
        let k = surface as usize - 1;
        self.break_pane(k);
        true
    }

    fn damage_surfaces_near(&mut self, centre: Vec3, radius: f32) {
        for k in 0..self.panes.len() {
            let (broken, at, reach) = self.panes[k];
            if !broken && at.distance(centre) <= radius + reach {
                self.break_pane(k);
            }
        }
    }

    fn ray_chars(&mut self, from: Vec3, ray: Vec3, radius: f32, owner: Option<usize>) -> Option<CharHit> {
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
                let Some(m) = bones.get(part.bone) else { continue };
                let hit = if radius > 0.0 {
                    let Some((c, r)) = part.bound_sphere else { continue };
                    let wc = m.transform_point3(c);
                    ray_sphere(from, ray, wc, r + radius).map(|t| (t, (from + ray * t - wc).normalize_or(Vec3::Y)))
                } else {
                    let inv = m.inverse();
                    let (lo, hi) = part.bound_box;
                    ray_box(inv.transform_point3(from), inv.transform_vector3(ray), lo, hi)
                        .map(|(t, n)| (t, Mat3::from_mat4(*m).mul_vec3(n).normalize_or(Vec3::Y)))
                };
                let Some((t, normal)) = hit else { continue };
                // A shot leaving its own shooter's body passes.
                if owner == Some(i) && normal.dot(ray) > 0.0 {
                    continue;
                }
                if best.is_none_or(|b| t < b.fraction) {
                    best = Some(CharHit { fraction: t, who: i, normal });
                }
            }
        }
        best
    }

    fn hit_char(&mut self, who: usize, point: Vec3, dir: Vec3, kind: u32, damage: f32, stun: f32, knockback: f32, owner: u32, stamp: Option<u64>, self_immune: bool) -> bool {
        let owner = if owner == 0 { p3::owner::ENVIRONMENTAL } else { owner };
        if self_immune && p3::owner::character(owner) == Some(who) {
            return false;
        }
        let dir = dir.try_normalize().unwrap_or_else(|| {
            let f = &self.fighters[who];
            (f.feet() + Vec3::Y * f.motion.height() - point).try_normalize().unwrap_or(Vec3::Z)
        });
        self.inflict(who, dir, kind, damage.round() as i32, stun.round() as i32, knockback, stamp, owner)
    }

    fn blast(&mut self, centre: Vec3, kind: u32, damage: f32, stun: f32, knockback: f32, radius: f32, falloff: u32, owner: u32, stamp: u64, self_immune: bool) {
        let owner = if owner == 0 { p3::owner::ENVIRONMENTAL } else { owner };
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
                if !s.clear(centre, pelvis) {
                    continue;
                }
            }
            let p = match falloff {
                1 => 1.0 - d,
                2 => 1.0 / ((1.0 + d) * (1.0 + d)),
                _ => 1.0,
            };
            let dir = v.try_normalize().unwrap_or(Vec3::Y);
            self.inflict(who, dir, kind, (p * damage).round() as i32, (p * stun).round() as i32, p * knockback, Some(stamp), owner);
        }
        self.sounds.push((centre, 400.0, p3::owner::character(owner)));
    }

    fn targets(&self) -> Vec<Target> {
        self.fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive())
            .map(|(who, f)| Target { who, position: f.feet() + Vec3::Y * (f.motion.height() + 4.0), velocity: f.velocity * p3::HZ as f32 })
            .collect()
    }

    fn hostile(&self, me: usize, other: usize) -> bool {
        match (self.teams.get(me), self.teams.get(other)) {
            (Some(a), Some(b)) => a.regards(*b) == crate::ai::perception::Relation::Hostile,
            _ => false,
        }
    }

    fn clear(&mut self, from: Vec3, to: Vec3) -> bool {
        self.sight.is_none_or(|s| s.clear(from, to))
    }

    fn inaccuracy(&self, owner: u32) -> f32 {
        if owner & p3::owner::TYPE_MASK == p3::owner::TURRET {
            return self.turret_spread;
        }
        p3::owner::character(owner).and_then(|c| self.inaccuracy.get(c)).copied().unwrap_or(0.0)
    }
}

/// A gun's world matrix in fighter `f`'s hand this tick: at the hand, and —
/// for an aiming character — pointing straight down its aim (`yaw` the aim
/// direction, `pitch` up positive). The gun's barrel is its +X, its top +Z.
fn hand_matrix(f: &Fighter, bones: &[Mat4], aim: Option<(f32, f32)>) -> Mat4 {
    let hand = bones.get(HAND_BONE).copied().unwrap_or(f.placement);
    let Some((yaw, pitch)) = aim else { return hand };
    let d = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
    let up = (Vec3::Y - d * d.y).try_normalize().unwrap_or(Vec3::Z);
    let rot = Mat3::from_cols(d, up.cross(d), up);
    Mat4::from_mat3_translation(rot, hand.w_axis.truncate())
}

impl World {
    /// Loads the weapon and particle classes (`common/`) and the level's
    /// placed guns (`items.json`).
    pub fn attach_weapons(&mut self, common: &Path) {
        let particles = Rc::new(p3::Classes::load(&common.join("particle_classes.json")));
        let classes = Rc::new(WeaponClasses::load(&common.join("weapons.json"), &particles));
        self.armory.particles = p3::System::new(particles);
        self.armory.classes = classes;
        self.armory.weapons.clear();
        self.armory.powerups.clear();
        self.turrets = super::turrets::load(&self.level_dir, common, &self.armory.particles.classes.clone());
        // (Keeping the mixer an output device may already play.)
        let (mixer, realtime) = (self.audio.mixer.clone(), self.audio.realtime);
        self.audio = crate::audio::Audio::load(common, &self.level_dir);
        self.audio.mixer = mixer;
        self.audio.realtime = realtime;
        let Ok(text) = std::fs::read_to_string(self.level_dir.join("items.json")) else { return };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        for w in doc["weapons"].as_array().into_iter().flatten() {
            let Some(class) = w["class"].as_str().and_then(|n| self.armory.classes.get(n)) else { continue };
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
            let Some(kind) = POWERUP_DATA.iter().position(|n| Some(*n) == p["type"].as_str()) else { continue };
            let v = &p["position"];
            let at = Vec3::new(v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32, v[2].as_f64().unwrap_or(0.0) as f32);
            self.armory.powerups.push(Powerup { kind, amount: POWERUP_DEFAULT[kind], position: at });
        }
    }

    pub fn powerups(&self) -> &[Powerup] {
        &self.armory.powerups
    }

    /// A powerup of `kind` (a script name) at `at`; `amount` defaults to
    /// the kind's.
    pub fn spawn_powerup(&mut self, kind: &str, at: Vec3, amount: Option<u16>) -> bool {
        let Some(kind) = POWERUPS.iter().position(|n| *n == kind) else { return false };
        self.armory.powerups.push(Powerup { kind, amount: amount.unwrap_or(POWERUP_DEFAULT[kind]), position: at });
        true
    }

    /// How much of a powerup fighter `who` could take.
    fn could_receive(&self, who: usize, kind: usize, amount: u16) -> u16 {
        let f = &self.fighters[who];
        let inv = &f.inventory;
        let add = |have: u16| if have >= CARRY_MAX { 0 } else { amount.min(CARRY_MAX - have) };
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
    pub fn give_powerup(&mut self, who: usize, kind: &str, amount: Option<u16>, ignore_max: bool) -> u16 {
        let Some(kind) = POWERUPS.iter().position(|n| *n == kind) else { return 0 };
        let amount = amount.unwrap_or(POWERUP_DEFAULT[kind]);
        let give = if ignore_max { amount } else { self.could_receive(who, kind, amount) };
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

    /// The nearest powerup fighter `who` could take, within reach.
    fn powerup_in_reach(&self, who: usize) -> Option<usize> {
        let feet = self.fighters.get(who)?.feet();
        self.armory
            .powerups
            .iter()
            .enumerate()
            .filter(|(_, p)| self.could_receive(who, p.kind, p.amount) > 0)
            .filter_map(|(i, p)| {
                let d = ((p.position - feet) * Vec3::new(1.0, 0.0, 1.0)).length();
                let dy = p.position.y - feet.y;
                (d < PICKUP_RADIUS && (-PICKUP_BELOW..=PICKUP_ABOVE).contains(&dy)).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    /// Takes every gun from fighter `who` and empties its pockets
    /// (`chr_inv_reset`).
    pub fn reset_inventory(&mut self, who: usize) {
        let Some(f) = self.fighters.get_mut(who) else { return };
        f.inventory = crate::fighter::Inventory::default();
        let guns: Vec<usize> = [f.hand.take(), f.holster.take(), f.magic.take()].into_iter().flatten().collect();
        for w in guns {
            self.delete_weapon(w);
        }
        self.set_weapon_varient(who);
    }

    /// Removes gun `w` from the world (its particles go).
    fn delete_weapon(&mut self, w: usize) {
        let Armory { particles, weapons, .. } = &mut self.armory;
        let now = self.now as u64;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
        weapons[w].stow(particles, &mut host);
        weapons[w].owner = None;
        weapons[w].gone = true;
    }

    /// A cutscene or console putting the gun in hand away (`on`), into the
    /// hidden slot — or onto the floor if that is taken; or giving it back
    /// (only into an empty hand). (`chr_forceholster`.)
    pub fn force_holster(&mut self, who: usize, on: bool) {
        let Some(f) = self.fighters.get(who) else { return };
        if on {
            if f.hand.is_some() && !self.holster(who, true) {
                // (No clip: straight away.)
                self.put_away(who, true);
            }
        } else if f.hand.is_none() && f.magic.is_some() && !self.draw(who) {
            self.bring_to_hand(who);
        }
    }

    /// Holds a gun that is empty, with nothing to reload it and no reload
    /// under way (`chr_has_empty_weapon`).
    pub fn has_empty_weapon(&self, who: usize) -> bool {
        let Some(f) = self.fighters.get(who) else { return false };
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
                let shooter = g.class.attachments.iter().find(|a| a.ammo_used > 0 && a.shooter == 0)?;
                Some(crate::ai::goals::Spread {
                    owner,
                    origin: m.transform_point3(shooter.matrix.w_axis.truncate()),
                    axis: m.transform_vector3(shooter.matrix.y_axis.truncate()).normalize_or(Vec3::Z),
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
                    usable: !g.class.has(flag::UNSTORABLE) && (g.ammo > 0 || clips || f.infinite_ammo),
                }
            })
            .collect()
    }

    /// The AIs' aim and triggers this tick (`docs/AI.md`, *Shooting*): each
    /// aim swings toward where its goals want the barrel — at most a sixth of
    /// a turn a second × its speed × the gun's aiming speed, never more than
    /// 130° off the body (then straight ahead) — and the trigger is held
    /// while its targeting says so. Returns each fighter's trigger bits.
    pub(super) fn ai_guns(&mut self) -> Vec<u32> {
        use std::f32::consts::{PI, TAU};
        let wrap = |a: f32| (a + PI).rem_euclid(TAU) - PI;
        let mut out = vec![0u32; self.fighters.len()];
        for (i, f) in self.fighters.iter_mut().enumerate() {
            let Some(crate::ai::Brain::Agent(a)) = f.brain.as_ref() else {
                f.ai_aim = None;
                continue;
            };
            let (want, speed, trigger) = (a.aim, a.aim_speed, a.trigger);
            let Some(w) = f.hand else {
                f.ai_aim = None;
                continue;
            };
            let Some(dir) = want.filter(|_| f.alive()) else {
                f.ai_aim = None;
                continue;
            };
            let mut yaw = dir.x.atan2(dir.z);
            let mut pitch = dir.y.clamp(-1.0, 1.0).asin();
            if wrap(yaw - f.facing.facing).abs() > 130f32.to_radians() {
                yaw = f.facing.facing;
                pitch = 0.0;
            }
            let (cy, cp) = f.ai_aim.unwrap_or((f.facing.facing, 0.0));
            let (dy, dp) = (wrap(yaw - cy), pitch - cp);
            let step = TAU / 60.0 * speed * self.armory.weapons[w].class.aiming_speed.max(0.01);
            let dist = (dy * dy + dp * dp).sqrt();
            f.ai_aim = Some(if dist <= step { (yaw, pitch) } else { (cy + dy * step / dist, cp + dp * step / dist) });
            // (A gun that fires one shot a press waits for its chamber.)
            let gun = &self.armory.weapons[w];
            let single = gun.class.has(flag::AI_SINGLE_SHOT) && (gun.chamber > 0 || gun.reload_time > 0);
            if trigger && !single {
                out[i] = key::FIRE1;
            }
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
                let skill = f.shooting.get(self.armory.weapons[w].class.ai.skill_index).map_or(0.0, |s| s.gun_inaccuracy) + extra;
                let hostile = player_team.is_some_and(|t| f.team().regards(t) == crate::ai::perception::Relation::Hostile);
                skill * if hostile { self.difficulty.inaccuracy } else { 1.0 }
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
        let holstered: std::collections::HashSet<usize> =
            self.fighters.iter().flat_map(|f| [f.holster, f.magic]).flatten().collect();
        self.armory
            .weapons
            .iter()
            .enumerate()
            .filter(|(i, g)| !g.gone && !holstered.contains(i) && g.owner.is_none_or(|o| self.fighters.get(o).is_some_and(|f| f.hand == Some(*i))))
            .filter_map(|(_, g)| Some((g.class.model.clone()?, g.matrix.get())))
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
                let m = Mat4::from_mat3_translation(sys.world_orientation(p) * scale, sys.world_position(p));
                Some((models.join(format!("{}.glb", c.instance)), m))
            })
            .collect()
    }

    /// The particles to draw, as sprites: each class's texture at its
    /// scale — facing the camera, or stretched along its flight for the
    /// directional sprite kinds. (Models, contrails' trails, tint and fading
    /// aren't drawn yet.)
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
            .filter_map(|(_, p)| {
                let c = sys.class(p.class);
                if c.has(cflag::INVISIBLE) || c.has(cflag::GEOMETRY) || c.instance.is_empty() {
                    return None;
                }
                let x = read(&c.scale, &p.vars)?;
                let y = c.y_scale.as_ref().and_then(|v| read(v, &p.vars)).unwrap_or(x);
                let axis = matches!(c.sprite_type, 2 | 3 | 5 | 6 | 7).then(|| {
                    if c.has(cflag::ORIENTATION) {
                        sys.world_orientation(p).y_axis
                    } else {
                        p.velocity
                    }
                });
                Some(crate::render::Sprite { at: sys.world_position(p), size: [x, y], axis, texture: textures.join(format!("{}.png", c.instance)) })
            })
            .collect()
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
        let Some(at) = self.fighters.get(who).map(|f| f.feet()) else { return false };
        let Some(w) = self.spawn_weapon(class, at, 0.0, true) else { return false };
        if let Some(old) = self.fighters[who].hand.take() {
            self.delete_weapon(old);
        }
        self.take_weapon(who, w);
        true
    }

    /// Fighter `who` takes gun `w` into its hand, dropping what it held.
    pub fn take_weapon(&mut self, who: usize, w: usize) {
        if let Some(old) = self.fighters[who].hand.take() {
            self.drop_weapon_index(old, None);
        }
        self.armory.weapons[w].assign(who);
        self.fighters[who].hand = Some(w);
        self.set_weapon_varient(who);
    }

    /// Fighter `who` lets go of the gun in its hand (`knock`: thrown clear,
    /// as a knockdown does).
    pub fn drop_weapon(&mut self, who: usize, knock: bool) {
        let Some(w) = self.fighters.get_mut(who).and_then(|f| f.hand.take()) else { return };
        let kick = knock.then(|| self.fighters[who].last_impact * self.armory.weapons[w].kick_scale());
        self.drop_weapon_index(w, kick);
        self.set_weapon_varient(who);
    }

    fn drop_weapon_index(&mut self, w: usize, kick: Option<Vec3>) {
        let Armory { particles, weapons, .. } = &mut self.armory;
        let now = self.now as u64;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
        weapons[w].release(particles, &mut host, kick);
    }

    /// The pistol / rifle animation variant for what fighter `who` holds.
    fn set_weapon_varient(&mut self, who: usize) {
        use crate::anim::types::var;
        let rifle = self.fighters[who].hand.map(|w| self.armory.weapons[w].class.has(flag::TWO_HANDED));
        let f = &mut self.fighters[who];
        f.motion.varient &= !(var::RIGHTY_PISTOL | var::RIGHTY_RIFLE | var::LEFTY_PISTOL | var::LEFTY_RIFLE);
        match rifle {
            Some(true) => f.motion.varient |= var::RIGHTY_RIFLE,
            Some(false) => f.motion.varient |= var::RIGHTY_PISTOL,
            None => {}
        }
    }

    /// Whether fighter `who` holds a gun it can fire (loaded, or with
    /// something to reload it with).
    pub fn armed(&self, who: usize) -> bool {
        self.fighters.get(who).and_then(|f| f.hand).is_some_and(|w| !self.fighters[who].weapon_empty || self.armory.weapons[w].ammo > 0)
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
        let Some(w) = self.fighters.get(who).and_then(|f| f.hand) else { return false };
        let gun = &self.armory.weapons[w];
        let f = &self.fighters[who];
        if gun.ammo >= gun.class.max_ammo || gun.reload_delay > 0 || gun.chamber > 0 || f.overlays[crate::fighter::INVENTORY].is_some() {
            return false;
        }
        if !self.has_reload(who, w) || !f.motion.current().attacks.is_empty() || reaction::is_victim(f.motion.current().anim_type) {
            return false;
        }
        let energy = gun.class.has(flag::ENERGY);
        let reload_type = oni_type(&gun.class.reload_anim);
        if !self.start_overlay(who, crate::fighter::INVENTORY, reload_type, crate::anim::types::st::STANDING, false) {
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
    fn start_overlay(&mut self, who: usize, slot: usize, kind: u16, from: u16, magic: bool) -> bool {
        let f = &mut self.fighters[who];
        let Some(clip) = f.motion.coll.lookup(kind, from, f.motion.varient) else { return false };
        f.overlays[slot] = Some(crate::fighter::Overlay { clip, frame: 0, magic });
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
        if !f.motion.current().attacks.is_empty() || reaction::is_victim(f.motion.current().anim_type) {
            return false;
        }
        self.start_overlay(who, crate::fighter::INVENTORY, clip::HOLSTER, crate::anim::types::st::STANDING, magic)
    }

    /// Draw the holstered gun (or the hidden slot's): the clip plays, the gun
    /// comes to hand at its action frame.
    fn draw(&mut self, who: usize) -> bool {
        let f = &self.fighters[who];
        let Some(w) = f.magic.or(f.holster) else { return false };
        if !f.motion.current().attacks.is_empty() || reaction::is_victim(f.motion.current().anim_type) {
            return false;
        }
        let kind = if self.armory.weapons[w].class.has(flag::TWO_HANDED) { clip::DRAW_RIFLE } else { clip::DRAW_PISTOL };
        self.start_overlay(who, crate::fighter::INVENTORY, kind, crate::anim::types::st::STANDING, false)
    }

    /// The gun from the holster (or the hidden slot) into the empty hand.
    fn bring_to_hand(&mut self, who: usize) {
        let f = &mut self.fighters[who];
        if f.hand.is_some() {
            return;
        }
        f.hand = if f.magic.is_some() { f.magic.take() } else { f.holster.take() };
        if let Some(w) = f.hand {
            self.armory.weapons[w].assign(who);
        }
        self.set_weapon_varient(who);
    }

    /// The gun in hand into the holster — swapping with what was there —
    /// or (`magic`) into the hidden slot, or onto the floor if that is taken.
    fn put_away(&mut self, who: usize, magic: bool) {
        let Some(w) = self.fighters[who].hand else { return };
        if magic && self.fighters[who].magic.is_some() {
            self.drop_weapon(who, false);
            return;
        }
        {
            let Armory { particles, weapons, .. } = &mut self.armory;
            let now = self.now as u64;
            let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
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
    /// one.
    fn overlays_tick(&mut self, who: usize) {
        let f = &mut self.fighters[who];
        if f.overlays[crate::fighter::INVENTORY].is_some() && reaction::is_knockdown(f.motion.current().anim_type) {
            f.overlays[crate::fighter::INVENTORY] = None;
        }
        for slot in [crate::fighter::RECOIL, crate::fighter::INVENTORY] {
            let f = &self.fighters[who];
            let Some(o) = f.overlays[slot] else { continue };
            let c = f.motion.clip_of(o.clip);
            let (kind, frames, action) = (c.anim_type, c.frames(), c.action_frame);
            if action == Some(o.frame) && slot == crate::fighter::INVENTORY {
                if clip::is_reload(kind) {
                    if let Some(w) = f.hand {
                        let Armory { particles, weapons, .. } = &mut self.armory;
                        let now = self.now as u64;
                        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
                        weapons[w].reloaded(particles, &mut host);
                    }
                } else if kind == clip::HOLSTER {
                    self.put_away(who, o.magic);
                    // A holstered gun coming out: its draw clip, from its
                    // action frame.
                    let f = &self.fighters[who];
                    if !o.magic {
                        if let Some(w) = f.hand {
                            let kind = if self.armory.weapons[w].class.has(flag::TWO_HANDED) { clip::DRAW_RIFLE } else { clip::DRAW_PISTOL };
                            if self.start_overlay(who, crate::fighter::INVENTORY, kind, crate::anim::types::st::STANDING, false) {
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
        let Some((item, started, waited)) = f.pickup else { return };
        let c = f.motion.current();
        if !clip::is_pickup(c.anim_type) {
            f.pickup = if started || waited > 10 { None } else { Some((item, false, waited + 1)) };
            return;
        }
        f.pickup = Some((item, true, waited));
        let at = c.action_frame.unwrap_or(0).min(c.frames().saturating_sub(1));
        if f.motion.frame != at {
            return;
        }
        f.pickup = None;
        let feet = f.feet();
        let near = |p: Vec3| {
            let d = ((p - feet) * Vec3::new(1.0, 0.0, 1.0)).length();
            d < PICKUP_FINISH_RADIUS && (-PICKUP_FINISH_BELOW..=PICKUP_FINISH_ABOVE).contains(&(p.y - feet.y))
        };
        match item {
            Item::Weapon(w) => {
                let g = &self.armory.weapons[w];
                if g.owner.is_none() && !g.gone && near(g.pickup_position()) {
                    self.take(who, item);
                }
            }
            Item::Powerup(k) => {
                if self.armory.powerups.get(k).is_some_and(|p| near(p.position)) {
                    self.take(who, item);
                }
            }
        }
    }

    /// Takes an item now.
    fn take(&mut self, who: usize, item: Item) {
        match item {
            Item::Weapon(w) => {
                eprintln!("weapon: fighter {who} picks up {}", self.armory.weapons[w].class.name);
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
                (d.length() < PICKUP_RADIUS && (-PICKUP_BELOW..=PICKUP_ABOVE).contains(&dy)).then_some((i, d.length()))
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
        let item = self.powerup_in_reach(who).map(Item::Powerup).or_else(|| self.weapon_in_reach(who).map(Item::Weapon));
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
            if f.motion.current().has_flag(crate::anim::types::flag::CAN_PICKUP) {
                self.take(who, item);
                return true;
            }
            let (at, rifle) = match item {
                Item::Weapon(w) => (self.armory.weapons[w].pickup_position(), Some(self.armory.weapons[w].class.has(flag::TWO_HANDED))),
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
            if f.motion.request(&ch, crate::anim::motion::Urge::Appropriate, false, kind).is_some() {
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
            let empty = self.armory.weapons[w].ammo == 0 && !self.has_reload(who, w) && !self.fighters[who].reloading();
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
            self.reload(who);
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
        let busy = self.fighters[who].weapon_empty || self.fighters[who].overlays[crate::fighter::INVENTORY].is_some();
        let inaccuracy = self.inaccuracies();
        let Armory { particles, weapons, .. } = &mut self.armory;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
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
            gun_release(&mut self.armory, &mut self.fighters, self.sight.as_ref(), now, w);
            if (single || !self.fighters[who].player) && !self.reload(who) {
                self.armory.weapons[w].reports.push(Report::Empty);
            }
        }
    }

    /// The player's auto-aim (`docs/WEAPONS.md`, *Auto-aim*): the turn
    /// (yaw, pitch) that would put gun `w`'s shot on the one hostile within
    /// reach of the aim — none when two qualify. It steers the gun only;
    /// the player's own aim stays where it is.
    fn auto_aim(&self, who: usize, w: usize) -> (f32, f32) {
        use std::f32::consts::PI;
        let f = &self.fighters[who];
        let gun = &self.armory.weapons[w];
        let Some(shooter) = gun.class.attachments.iter().find(|a| a.ammo_used > 0 && a.shooter == 0) else {
            return (0.0, 0.0);
        };
        let m = gun.matrix.get();
        let muzzle = m.transform_point3(shooter.matrix.w_axis.truncate());
        // The shot's line in gun space, as angles off the barrel (+X; +Y
        // left, +Z up).
        let s = shooter.matrix.y_axis.truncate().normalize_or(Vec3::X);
        let (off_yaw, off_pitch) = (s.y.atan2(s.x), s.z.clamp(-1.0, 1.0).asin());
        let (aim_yaw, aim_pitch) = (f.facing.aim(), f.aiming_ud);
        let me = f.team();
        let pelvis = f.feet() + Vec3::Y * f.motion.height();
        let wrap = |a: f32| (a + PI).rem_euclid(2.0 * PI) - PI;
        let mut found: Option<(f32, f32)> = None;
        for (i, t) in self.fighters.iter().enumerate() {
            if i == who || !t.alive() || t.team().regards(me) != crate::ai::perception::Relation::Hostile {
                continue;
            }
            let chest = t.feet() + Vec3::Y * (t.motion.height() + 4.0);
            let to = chest - muzzle;
            let dist = to.length();
            if !(1.0..=500.0).contains(&dist) {
                continue;
            }
            let mid = t.feet() + Vec3::Y * t.motion.height();
            if self.sight.as_ref().is_some_and(|s| !s.clear(mid, pelvis)) {
                continue;
            }
            let d = to / dist;
            let want_yaw = d.x.atan2(d.z) - off_yaw;
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
            let weight = if worst <= 0.15 { 1.0 } else { (0.35 - worst) / 0.2 };
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
            let Some(w) = self.fighters[who].hand else { continue };
            let f = &self.fighters[who];
            if !f.alive() {
                continue;
            }
            let bones = f.bones();
            let aim = if f.player {
                let (lr, ud) = self.auto_aim(who, w);
                Some((f.facing.aim() + lr, (f.aiming_ud + f.recoil + ud).clamp(-1.5, 1.5)))
            } else {
                f.ai_aim.map(|(y, p)| (y, (p + f.recoil).clamp(-1.5, 1.5)))
            };
            let f = &self.fighters[who];
            let m = hand_matrix(f, &bones, aim);
            let before = self.armory.weapons[w].position;
            let Armory { particles, weapons, .. } = &mut self.armory;
            let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
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
            let Some(who) = self.armory.weapons[w].owner else { continue };
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
                        self.start_overlay(who, crate::fighter::RECOIL, kind, crate::anim::types::st::ANYTHING, false);
                        if volume > 0.0 {
                            let at = self.fighters[who].feet() + Vec3::Y * self.fighters[who].motion.height();
                            self.sound(crate::ai::perception::Stimulus::SoundGunshot, at, volume, Some(who), None);
                        }
                    }
                    Report::Empty => eprintln!("weapon: fighter {who} *click*"),
                }
            }
        }
        // Free guns.
        for w in 0..self.armory.weapons.len() {
            let gun = &self.armory.weapons[w];
            if gun.owner.is_some() || gun.gone {
                continue;
            }
            let floor = if gun.resting {
                None
            } else {
                self.fighters.first().and_then(|f| f.physics.cast(gun.position + Vec3::Y * 2.0, Vec3::Y * -40.0)).map(|(t, _, _)| gun.position.y + 2.0 - 40.0 * t)
            };
            self.armory.weapons[w].tick_free(floor);
        }
        // The shots.
        let tick = self.now as u32;
        let glass_base = self.glass_base();
        let panes: Vec<(bool, Vec3, f32)> = self.glass.iter().map(|p| (p.broken, p.centre, p.radius)).collect();
        let inaccuracy = self.inaccuracies();
        let spread = self.turret_spread();
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
        host.glass_base = glass_base;
        host.set_spread(inaccuracy, spread);
        host.panes = panes;
        self.armory.particles.update(tick, &mut host);
        let sounds = std::mem::take(&mut host.sounds);
        let broke = std::mem::take(&mut host.broke);
        drop(host);
        for k in broke {
            self.set_glass(k, true);
        }
        for (at, volume, maker) in sounds {
            self.sound(crate::ai::perception::Stimulus::SoundGunshot, at, volume, maker, None);
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
                f.recoil -= if gun.chamber == 0 { r.return_speed } else { r.firing_return_speed };
            }
            if f.player && gun.class.has(flag::RECOIL_DIRECT) {
                // The player's kick stays in the aim: pull it back down.
                f.aiming_ud += f.recoil;
                f.recoil = 0.0;
            } else {
                f.recoil = f.recoil.clamp(0.0, r.max);
            }
            f.aiming_ud = f.aiming_ud.clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        }
        // Knocked down: the gun flies.
        for who in 0..self.fighters.len() {
            let f = &self.fighters[who];
            if f.hand.is_some() && (f.motion.current().has_flag(crate::anim::types::flag::DROP_WEAPON) || (!f.alive() && f.motion.dying)) {
                self.drop_weapon(who, true);
            }
        }
        // (Gone guns stay in the list: fighters hold guns by index.)
    }
}

/// An animation type by its name (`oni_schema::ANIM_TYPES`; 0 if unknown).
fn oni_type(name: &str) -> u16 {
    oni_schema::ANIM_TYPES.iter().position(|n| *n == name).unwrap_or(0) as u16
}

fn gun_release(armory: &mut Armory, fighters: &mut [Fighter], sight: Option<&crate::physics::Sight>, now: u64, w: usize) {
    let Armory { particles, weapons, .. } = armory;
    let mut host = ShotHost::new(fighters, sight, now);
    weapons[w].release_trigger(particles, &mut host, false);
}
