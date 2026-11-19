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

/// Guns, their classes and the particles.
pub struct Armory {
    pub classes: Rc<WeaponClasses>,
    pub particles: p3::System,
    pub weapons: Vec<Weapon>,
    /// Each fighter's fire buttons last tick (for presses).
    last_fire: Vec<u32>,
}

impl Default for Armory {
    fn default() -> Armory {
        Armory {
            classes: Rc::new(WeaponClasses::default()),
            particles: p3::System::new(Rc::new(p3::Classes::default())),
            weapons: Vec::new(),
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
}

impl<'a> ShotHost<'a> {
    fn new(fighters: &'a mut [Fighter], sight: Option<&'a crate::physics::Sight>, now: u64) -> ShotHost<'a> {
        let teams = fighters.iter().map(|f| f.team()).collect();
        let n = fighters.len();
        ShotHost { fighters, bones: vec![None; n], sight, teams, now, sounds: Vec::new() }
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

impl Host for ShotHost<'_> {
    fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit> {
        let f = self.fighters.first()?;
        f.physics.cast(from, ray).map(|(fraction, normal)| EnvHit { fraction, normal, surface: 0 })
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
    pub fn give_weapon(&mut self, who: usize, class: &str) -> bool {
        let Some(at) = self.fighters.get(who).map(|f| f.feet()) else { return false };
        let Some(w) = self.spawn_weapon(class, at, 0.0, true) else { return false };
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

    /// Starts a reload (`WPrTryReload`): a clip is used at once, the gun
    /// fills at the end. (The reload animation overlay isn't played yet: the
    /// gun fills after the clip's length.)
    pub fn reload(&mut self, who: usize) -> bool {
        let Some(w) = self.fighters.get(who).and_then(|f| f.hand) else { return false };
        let gun = &self.armory.weapons[w];
        if gun.ammo >= gun.class.max_ammo || gun.reload_delay > 0 || gun.chamber > 0 || self.fighters[who].reloading > 0 {
            return false;
        }
        if !self.has_reload(who, w) {
            return false;
        }
        let energy = gun.class.has(flag::ENERGY);
        let f = &mut self.fighters[who];
        if !f.infinite_ammo {
            if energy {
                f.inventory.cells -= 1;
            } else {
                f.inventory.ammo -= 1;
            }
        }
        f.reloading = RELOAD_TICKS;
        eprintln!("weapon: fighter {who} reloads");
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
                (d.length() < PICKUP_RADIUS && (-PICKUP_BELOW..=PICKUP_ABOVE).contains(&dy)).then_some((i, d.length()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    /// The swap key: pick up a gun in reach; else holster the one in hand,
    /// or draw the holstered one. (The pickup, holster and draw clips aren't
    /// played yet: it happens at once.)
    pub fn swap(&mut self, who: usize) {
        if let Some(w) = self.weapon_in_reach(who) {
            eprintln!("weapon: fighter {who} picks up {}", self.armory.weapons[w].class.name);
            self.take_weapon(who, w);
            return;
        }
        let f = &self.fighters[who];
        match (f.hand, f.holster) {
            (Some(w), _) if self.armory.weapons[w].class.has(flag::UNSTORABLE) => {}
            (hand, holster) if hand.is_some() || holster.is_some() => {
                if let Some(w) = hand {
                    let Armory { particles, weapons, .. } = &mut self.armory;
                    let now = self.now as u64;
                    let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
                    weapons[w].stow(particles, &mut host);
                }
                let f = &mut self.fighters[who];
                f.hand = holster;
                f.holster = hand;
                self.set_weapon_varient(who);
                eprintln!("weapon: fighter {who} {}", if hand.is_some() { "holsters" } else { "draws" });
            }
            _ => {}
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
        if pressed & key::FIRE1 != 0 {
            let empty = self.armory.weapons[w].ammo == 0 && !self.has_reload(who, w) && self.fighters[who].reloading == 0;
            self.fighters[who].weapon_empty = empty;
        }
        let motion = Vec3::ZERO;
        let Armory { particles, weapons, .. } = &mut self.armory;
        let now = self.now as u64;
        let busy = self.fighters[who].weapon_empty || self.fighters[who].reloading > 0;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
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
            if single && !self.reload(who) {
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
            let aim = f.player.then(|| {
                let (lr, ud) = self.auto_aim(who, w);
                (f.facing.aim() + lr, (f.aiming_ud + f.recoil + ud).clamp(-1.5, 1.5))
            });
            let f = &self.fighters[who];
            let m = hand_matrix(f, &bones, aim);
            let before = self.armory.weapons[w].position;
            let Armory { particles, weapons, .. } = &mut self.armory;
            let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
            let motion = m.w_axis.truncate() - before;
            weapons[w].tick_held(particles, &mut host, m, motion);
        }
        // Reloads finishing.
        for who in 0..self.fighters.len() {
            let f = &mut self.fighters[who];
            if f.reloading > 0 {
                f.reloading -= 1;
                if f.reloading == 0 {
                    if let Some(w) = f.hand {
                        let Armory { particles, weapons, .. } = &mut self.armory;
                        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
                        weapons[w].reloaded(particles, &mut host);
                    }
                }
            }
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
                        self.fighters[who].recoil_speed += recoil;
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
                self.fighters.first().and_then(|f| f.physics.cast(gun.position + Vec3::Y * 2.0, Vec3::Y * -40.0)).map(|(t, _)| gun.position.y + 2.0 - 40.0 * t)
            };
            self.armory.weapons[w].tick_free(floor);
        }
        // The shots.
        let tick = self.now as u32;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
        self.armory.particles.update(tick, &mut host);
        let sounds = std::mem::take(&mut host.sounds);
        drop(host);
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
        self.armory.weapons.retain(|g| !g.gone || g.owner.is_some());
    }
}

/// How long a reload takes until clips play as overlays (ticks; the
/// pistol clip's action frame).
pub const RELOAD_TICKS: u16 = 50;

fn gun_release(armory: &mut Armory, fighters: &mut [Fighter], sight: Option<&crate::physics::Sight>, now: u64, w: usize) {
    let Armory { particles, weapons, .. } = armory;
    let mut host = ShotHost::new(fighters, sight, now);
    weapons[w].release_trigger(particles, &mut host, false);
}
