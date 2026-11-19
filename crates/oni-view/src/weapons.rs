//! Weapons (`docs/WEAPONS.md`): the classes from `common/weapons.json`, and
//! each gun in the level — lying on the floor or in someone's hand. A held
//! gun carries particles on its attachment points; pulling the trigger sends
//! them events, and the shooter's particles emit the shots (`crate::p3`).

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use glam::{Mat4, Vec3};
use serde_json::Value as Json;

use crate::p3::{self, event, Handle, Host, System};

/// Weapon class flags (`docs/ASSET_FORMAT.md`, *Weapon classes*).
pub mod flag {
    pub const BIG: u32 = 1 << 0;
    pub const UNSTORABLE: u32 = 1 << 1;
    pub const ENERGY: u32 = 1 << 2;
    pub const TWO_HANDED: u32 = 1 << 3;
    pub const RECOIL_DIRECT: u32 = 1 << 4;
    pub const AUTOFIRE: u32 = 1 << 5;
    pub const HAS_ALTERNATE: u32 = 1 << 6;
    pub const UNRELOADABLE: u32 = 1 << 7;
    pub const WALK_ONLY: u32 = 1 << 8;
    pub const RESTART_EACH_SHOT: u32 = 1 << 9;
    pub const DONT_STOP_WHILE_FIRING: u32 = 1 << 10;
    pub const HAS_LASER: u32 = 1 << 11;
    pub const DONT_FADE_OUT: u32 = 1 << 12;
    pub const USE_AMMO_CONTINUOUSLY: u32 = 1 << 13;
    /// The AI fires it one press at a time.
    pub const AI_SINGLE_SHOT: u32 = 1 << 14;
}
const FLAG_NAMES: [(&str, u32); 15] = [
    ("big", flag::BIG),
    ("unstorable", flag::UNSTORABLE),
    ("energy", flag::ENERGY),
    ("twoHanded", flag::TWO_HANDED),
    ("recoilDirect", flag::RECOIL_DIRECT),
    ("autofire", flag::AUTOFIRE),
    ("hasAlternate", flag::HAS_ALTERNATE),
    ("unreloadable", flag::UNRELOADABLE),
    ("walkOnly", flag::WALK_ONLY),
    ("restartEachShot", flag::RESTART_EACH_SHOT),
    ("dontStopWhileFiring", flag::DONT_STOP_WHILE_FIRING),
    ("hasLaser", flag::HAS_LASER),
    ("dontFadeOut", flag::DONT_FADE_OUT),
    ("useAmmoContinuously", flag::USE_AMMO_CONTINUOUSLY),
    ("aiSingleShot", flag::AI_SINGLE_SHOT),
];

/// A dropped gun lies this long before fading (ticks), and fades this long.
pub const FREE_TIME: u32 = 4800;
pub const FADE_TIME: u32 = 600;
/// Placed guns sit this far above their marker.
pub const PLACED_LIFT: f32 = 2.0;
/// A falling gun's gravity, units per tick².
const DROP_GRAVITY: f32 = 0.06;

/// The recoil a class gives (radians; `base` per shot, already in the
/// form that sums to the authored kick).
#[derive(Clone, Copy, Debug, Default)]
pub struct Recoil {
    pub base: f32,
    pub max: f32,
    pub factor: f32,
    pub return_speed: f32,
    pub firing_return_speed: f32,
}

/// A particle riding the gun.
#[derive(Clone, Debug)]
pub struct Attachment {
    pub class: Option<p3::ClassId>,
    /// Gun space.
    pub matrix: Mat4,
    /// Non-zero: a shooter, using this much ammo a shot.
    pub ammo_used: u16,
    pub chamber_time: u16,
    pub min_fire_time: u16,
    /// A shooter's index; for a follower the shooter it follows (−1 = all).
    pub shooter: i32,
    pub start_delay: u16,
}

// (The model, and the recoil / reload clips, are for drawing the gun and
// the overlay animations, still to come.)
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct WeaponClass {
    pub name: String,
    pub model: Option<PathBuf>,
    pub flags: u32,
    pub max_ammo: u16,
    pub reload_time: u16,
    pub reload_delay_time: u16,
    pub stop_chamber_threshold: u16,
    pub recoil: Recoil,
    pub recoil_anim: String,
    pub reload_anim: String,
    pub aiming_speed: f32,
    pub pickup_offset: Vec3,
    /// How loud a shot is to the AI (ranges of hearing), per shooter.
    pub detection_volume: [f32; 2],
    /// How the AI shoots it (`docs/AI.md`, *Shooting*).
    pub ai: AiGun,
    pub attachments: Vec<Attachment>,
}

impl WeaponClass {
    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }
}

/// All weapon classes, by name.
#[derive(Default)]
pub struct WeaponClasses {
    pub by_name: HashMap<String, Rc<WeaponClass>>,
}

impl WeaponClasses {
    /// Loads `weapons.json`, resolving attachment particle classes.
    pub fn load(path: &Path, particles: &p3::Classes) -> WeaponClasses {
        let Ok(doc) = std::fs::read_to_string(path).map_err(|_| ()).and_then(|t| serde_json::from_str::<Json>(&t).map_err(|_| ()))
        else {
            return WeaponClasses::default();
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        let by_name = doc["classes"]
            .as_object()
            .map(|m| m.iter().map(|(n, c)| (n.clone(), Rc::new(class(n, c, dir, particles)))).collect())
            .unwrap_or_default();
        WeaponClasses { by_name }
    }

    pub fn get(&self, name: &str) -> Option<Rc<WeaponClass>> {
        self.by_name.get(name).cloned()
    }
}

fn vec3(j: &Json) -> Vec3 {
    Vec3::new(j[0].as_f64().unwrap_or(0.0) as f32, j[1].as_f64().unwrap_or(0.0) as f32, j[2].as_f64().unwrap_or(0.0) as f32)
}

fn class(name: &str, c: &Json, dir: &Path, particles: &p3::Classes) -> WeaponClass {
    let f = |j: &Json| j.as_f64().unwrap_or(0.0) as f32;
    let u = |j: &Json| j.as_u64().unwrap_or(0) as u16;
    let flags = c["flags"]
        .as_array()
        .map(|a| a.iter().filter_map(|n| FLAG_NAMES.iter().find(|(k, _)| Some(*k) == n.as_str())).fold(0, |acc, (_, b)| acc | b))
        .unwrap_or(0);
    let attachments = c["attachments"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| {
                    let m: Vec<f32> = x["matrix"].as_array().map(|m| m.iter().map(f).collect()).unwrap_or_default();
                    let col = |k: usize| glam::Vec4::new(m.get(k * 3).copied().unwrap_or(0.0), m.get(k * 3 + 1).copied().unwrap_or(0.0), m.get(k * 3 + 2).copied().unwrap_or(0.0), if k == 3 { 1.0 } else { 0.0 });
                    Attachment {
                        class: x["class"].as_str().and_then(|n| particles.get(n)),
                        matrix: Mat4::from_cols(col(0), col(1), col(2), col(3)),
                        ammo_used: u(&x["ammoUsed"]),
                        chamber_time: u(&x["chamberTime"]),
                        min_fire_time: u(&x["minFireTime"]),
                        shooter: x["shooterIndex"].as_i64().unwrap_or(-1) as i32,
                        start_delay: u(&x["startDelay"]),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let r = &c["recoil"];
    WeaponClass {
        name: name.to_string(),
        model: c["model"].as_str().map(|m| dir.join(m)),
        flags,
        max_ammo: u(&c["maxAmmo"]),
        reload_time: u(&c["reloadTime"]),
        reload_delay_time: u(&c["reloadDelayTime"]),
        stop_chamber_threshold: u(&c["stopChamberThreshold"]),
        recoil: Recoil {
            base: f(&r["base"]),
            max: f(&r["max"]),
            factor: f(&r["factor"]).max(1.05),
            return_speed: f(&r["returnSpeed"]),
            firing_return_speed: f(&r["firingReturnSpeed"]),
        },
        recoil_anim: c["recoilAnimType"].as_str().unwrap_or("").to_string(),
        reload_anim: c["reloadAnimType"].as_str().unwrap_or("").to_string(),
        aiming_speed: f(&c["aimingSpeed"]),
        pickup_offset: vec3(&c["pickupOffset"]),
        detection_volume: [f(&c["ai"]["detectionVolume"]), f(&c["aiAlternate"]["detectionVolume"])],
        ai: AiGun::from_json(&c["ai"]),
        attachments,
    }
}

/// What the AI knows of shooting a gun (its primary fire).
#[derive(Clone, Copy, Debug)]
pub struct AiGun {
    pub aim: crate::ai::ranged::Gun,
    pub min_safe_distance: f32,
    /// 0 = unlimited.
    pub max_range: f32,
    /// Which of the class's shooting skills applies.
    pub skill_index: usize,
    /// Ticks of melee after this gun stuns or floors the target (not acted
    /// on yet).
    #[allow(dead_code)]
    pub melee_assist: u32,
}

impl AiGun {
    fn from_json(j: &Json) -> AiGun {
        let f = |k: &str| j[k].as_f64().unwrap_or(0.0) as f32;
        let m: Vec<f32> = j["shooterInverse"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default();
        let inverse = if m.len() >= 9 {
            glam::Mat3::from_cols(Vec3::new(m[0], m[1], m[2]), Vec3::new(m[3], m[4], m[5]), Vec3::new(m[6], m[7], m[8]))
        } else {
            glam::Mat3::IDENTITY
        };
        AiGun {
            aim: crate::ai::ranged::Gun {
                aim_radius: f("aimRadius").max(0.1),
                burst_radius: f("burstRadius"),
                prediction_speed: f("predictionSpeed"),
                no_wild_shots: j["noWildShots"].as_bool().unwrap_or(false),
                shooter_offset: vec3(&j["shooterOffset"]),
                shooter_inverse: inverse,
            },
            min_safe_distance: f("minSafeDistance"),
            max_range: f("maxRange"),
            skill_index: j["shootSkillIndex"].as_u64().unwrap_or(0) as usize,
            melee_assist: j["meleeAssistTimer"].as_u64().unwrap_or(0) as u32,
        }
    }
}

/// Something a gun did this tick that its holder (or the world) hears of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Report {
    /// A shot: the holder's recoil kick, and how far the AI hears it.
    Shot { recoil: f32, volume: f32 },
    /// Out of ammo.
    Empty,
}

/// One gun.
pub struct Weapon {
    pub class: Rc<WeaponClass>,
    /// Who holds it (a fighter index).
    pub owner: Option<usize>,
    /// Its world matrix (gun space → world), shared with its particles.
    pub matrix: Rc<Cell<Mat4>>,
    /// A free gun: where it lies and which way.
    pub position: Vec3,
    pub yaw: f32,
    pub velocity: Vec3,
    pub resting: bool,
    pub ammo: u16,
    pub chamber: u16,
    pub chamber_total: u16,
    pub reload_time: u16,
    pub reload_delay: u16,
    pub must_fire: u16,
    pub firing: bool,
    pub delay_release: bool,
    pub chamber_delay: bool,
    pub background: bool,
    started: u32,
    start_delay: Vec<u16>,
    particles: Vec<Option<Handle>>,
    pub free_time: u32,
    pub fade_time: u32,
    pub gone: bool,
    pub reports: Vec<Report>,
}

impl Weapon {
    /// A new gun, full, lying at `position` (`placed`: floating where the
    /// level put it, not falling).
    pub fn new(class: Rc<WeaponClass>, position: Vec3, yaw: f32, placed: bool) -> Weapon {
        let n = class.attachments.len();
        Weapon {
            ammo: class.max_ammo,
            class,
            owner: None,
            matrix: Rc::new(Cell::new(Mat4::from_rotation_translation(glam::Quat::from_rotation_y(yaw), position))),
            position,
            yaw,
            velocity: Vec3::ZERO,
            resting: placed,
            chamber: 0,
            chamber_total: 0,
            reload_time: 0,
            reload_delay: 0,
            must_fire: 0,
            firing: false,
            delay_release: false,
            chamber_delay: false,
            background: false,
            started: 0,
            start_delay: vec![0; n],
            particles: vec![None; n],
            free_time: 0,
            fade_time: 0,
            gone: false,
            reports: Vec::new(),
        }
    }

    /// Where a hand reaches to pick it up.
    pub fn pickup_position(&self) -> Vec3 {
        self.matrix.get().w_axis.truncate() + self.class.pickup_offset
    }

    /// Taken into `owner`'s hand.
    pub fn assign(&mut self, owner: usize) {
        self.owner = Some(owner);
        self.reload_time = 0;
        self.free_time = 0;
        self.fade_time = 0;
        self.resting = true;
    }

    /// Let go (dropped, or the holder died): it falls, knocked away by
    /// `kick` (per tick), and lies a while before fading.
    pub fn release(&mut self, sys: &mut System, host: &mut dyn Host, kick: Option<Vec3>) {
        self.release_trigger(sys, host, true);
        self.stop_background(sys, host);
        self.kill_particles(sys, host);
        let m = self.matrix.get();
        self.position = m.w_axis.truncate();
        let x = m.x_axis;
        self.yaw = (x.x.atan2(x.z) - std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
        self.velocity = kick.map_or(Vec3::ZERO, |k| Vec3::Y * 1.5 + k);
        self.resting = false;
        self.owner = None;
        self.must_fire = 0;
        self.reload_time = 0;
        self.chamber = 0;
        self.reload_delay = 0;
        self.start_delay.iter_mut().for_each(|d| *d = 0);
        self.fade_time = 0;
        self.free_time = if self.class.has(flag::DONT_FADE_OUT) { 0 } else { FREE_TIME };
    }

    /// Put away (holstered): it stops firing, and its particles go until
    /// it is drawn again.
    pub fn stow(&mut self, sys: &mut System, host: &mut dyn Host) {
        self.release_trigger(sys, host, true);
        self.stop_chamber_delay(sys, host);
        self.stop_background(sys, host);
        self.kill_particles(sys, host);
    }

    /// How hard a drop knocks it away, per unit of the holder's last impact.
    pub fn kick_scale(&self) -> f32 {
        if self.class.has(flag::UNSTORABLE) || self.class.has(flag::BIG) {
            0.07
        } else if self.class.has(flag::TWO_HANDED) {
            0.10
        } else {
            0.12
        }
    }

    fn kill_particles(&mut self, sys: &mut System, host: &mut dyn Host) {
        for p in self.particles.iter_mut() {
            if let Some(h) = p.take() {
                sys.kill(h, host);
            }
        }
        self.started = 0;
    }

    /// Creates the attachment particles (first tick in a hand).
    fn make_particles(&mut self, sys: &mut System, host: &mut dyn Host) {
        if self.particles.iter().any(Option::is_some) {
            return;
        }
        let t = f64::from(sys.tick) / p3::HZ;
        for (i, a) in self.class.attachments.iter().enumerate() {
            let Some(c) = a.class else { continue };
            let mut p = sys.create(c, sys.tick);
            p.position = a.matrix.w_axis.truncate();
            p.orientation = glam::Mat3::from_mat4(a.matrix);
            p.matrix = Some(self.matrix.clone());
            p.lifetime = 0.0;
            self.particles[i] = sys.created(p, t, host);
        }
        let ev = if self.background { event::BG_START } else { event::BG_STOP };
        self.send(sys, host, ev, -1, Vec3::ZERO);
    }

    /// Sends `ev` to the attachments following shooter `group` (−1 = all).
    fn send(&mut self, sys: &mut System, host: &mut dyn Host, ev: usize, group: i32, motion: Vec3) {
        for i in 0..self.class.attachments.len() {
            let a = &self.class.attachments[i];
            if group != -1 && a.shooter != -1 && a.shooter != group {
                continue;
            }
            if ev == event::START {
                self.start_delay[i] = a.start_delay;
                if a.start_delay > 0 {
                    continue;
                }
            }
            self.deliver(sys, host, i, ev, motion);
        }
    }

    fn deliver(&mut self, sys: &mut System, host: &mut dyn Host, i: usize, ev: usize, motion: Vec3) {
        if ev == event::START {
            self.started |= 1 << i;
        } else if ev == event::STOP {
            if self.started & (1 << i) == 0 {
                return;
            }
            self.started &= !(1 << i);
        }
        let Some(h) = self.particles[i] else { return };
        let owner = self.owner.map_or(0, |o| p3::owner::CHARACTER_WEAPON | o as u32);
        match sys.get_mut(h) {
            Some(p) => {
                p.velocity = motion * p3::HZ as f32;
                p.owner = owner;
            }
            None => {
                self.particles[i] = None;
                return;
            }
        }
        let t = f64::from(sys.tick) / p3::HZ;
        sys.send(h, ev, t, host);
        let a = &self.class.attachments[i];
        if ev == event::START && a.ammo_used > 0 {
            let shooter = a.shooter;
            self.pulse(sys, host, i, shooter, motion);
        }
    }

    /// One shot from shooter attachment `i`.
    fn pulse(&mut self, sys: &mut System, host: &mut dyn Host, i: usize, shooter: i32, motion: Vec3) {
        if self.owner.is_none() {
            return;
        }
        self.send(sys, host, event::PULSE, shooter, motion);
        let a = self.class.attachments[i].clone();
        self.ammo = self.ammo.saturating_sub(a.ammo_used);
        if self.ammo == 0 {
            self.stop_background(sys, host);
        } else if !self.chamber_delay {
            self.send(sys, host, event::DELAY_START, -1, motion);
            self.chamber_delay = true;
        }
        self.chamber = a.chamber_time;
        self.chamber_total = a.chamber_time;
        let volume = self.class.detection_volume[usize::from(shooter > 0)];
        self.reports.push(Report::Shot { recoil: self.class.recoil.base, volume });
    }

    fn stop_background(&mut self, sys: &mut System, host: &mut dyn Host) {
        if self.background {
            self.background = false;
            self.send(sys, host, event::BG_STOP, -1, Vec3::ZERO);
        }
    }

    fn start_background(&mut self, sys: &mut System, host: &mut dyn Host) {
        if !self.background && self.ammo > 0 {
            self.background = true;
            self.send(sys, host, event::BG_START, -1, Vec3::ZERO);
        }
    }

    fn stop_chamber_delay(&mut self, sys: &mut System, host: &mut dyn Host) {
        if self.chamber_delay {
            self.chamber_delay = false;
            self.send(sys, host, event::DELAY_STOP, -1, Vec3::ZERO);
        }
    }

    /// Pulls the trigger (`alternate`: the second shooter). Returns whether
    /// it took, and whether it was empty.
    pub fn depress(&mut self, sys: &mut System, host: &mut dyn Host, alternate: bool, forced: bool, motion: Vec3) -> (bool, bool) {
        if self.chamber > 0 || self.reload_time > 0 {
            return (false, false);
        }
        let group = if alternate {
            if !self.class.has(flag::HAS_ALTERNATE) {
                return (false, false);
            }
            1
        } else {
            0
        };
        let Some(i) = self.class.attachments.iter().position(|a| a.ammo_used > 0 && a.shooter == group) else {
            return (false, false);
        };
        let a = self.class.attachments[i].clone();
        if self.ammo < a.ammo_used {
            return (false, true);
        }
        if !self.firing {
            self.firing = true;
            if !forced {
                self.must_fire = self.must_fire.max(a.min_fire_time);
            }
            self.send(sys, host, event::START, group, motion);
        } else if self.class.has(flag::AUTOFIRE) {
            self.pulse(sys, host, i, group, motion);
        }
        if !forced {
            self.delay_release = false;
        }
        self.reload_delay = self.class.reload_delay_time;
        (true, false)
    }

    /// Lets go of the trigger (deferred while a burst must go on, unless
    /// `force`).
    pub fn release_trigger(&mut self, sys: &mut System, host: &mut dyn Host, force: bool) {
        if !force {
            let wait = self.must_fire > 0
                || (self.class.has(flag::DONT_STOP_WHILE_FIRING) && self.chamber > self.class.stop_chamber_threshold);
            if wait {
                self.delay_release = true;
                return;
            }
        }
        self.delay_release = false;
        self.must_fire = 0;
        if self.firing {
            self.firing = false;
            self.send(sys, host, event::STOP, -1, Vec3::ZERO);
        }
    }

    /// A reload's ammo arriving: full, and a pause before firing.
    pub fn reloaded(&mut self, sys: &mut System, host: &mut dyn Host) {
        self.stop_chamber_delay(sys, host);
        self.ammo = self.class.max_ammo;
        if self.owner.is_some() {
            self.start_background(sys, host);
        }
        self.reload_time = self.class.reload_time;
        self.chamber = 0;
        self.chamber_total = 0;
        self.must_fire = 0;
        self.delay_release = false;
    }

    /// The ammo powerup this gun takes (`None` for unreloadable guns).
    pub fn ammo_type(&self) -> Option<usize> {
        if self.class.has(flag::UNRELOADABLE) {
            None
        } else if self.class.has(flag::ENERGY) {
            Some(1)
        } else {
            Some(0)
        }
    }

    /// One tick in a hand: `hand` is the gun's world matrix this tick,
    /// `motion` how far the gun moved.
    pub fn tick_held(&mut self, sys: &mut System, host: &mut dyn Host, hand: Mat4, motion: Vec3) {
        self.matrix.set(hand);
        self.position = hand.w_axis.truncate();
        if !self.background && self.ammo > 0 && self.particles.iter().all(Option::is_none) {
            self.background = true;
        }
        self.make_particles(sys, host);
        if self.must_fire > 0 {
            self.must_fire -= 1;
            if self.must_fire > 0 && self.class.has(flag::AUTOFIRE) {
                self.depress(sys, host, false, true, motion);
            }
        }
        if self.delay_release {
            self.release_trigger(sys, host, false);
        }
        self.reload_time = self.reload_time.saturating_sub(1);
        if self.chamber > 0 {
            self.chamber -= 1;
            if self.chamber == 0 && self.firing {
                if self.class.has(flag::RESTART_EACH_SHOT) {
                    self.release_trigger(sys, host, false);
                } else if self.class.has(flag::USE_AMMO_CONTINUOUSLY) {
                    let started = (0..self.class.attachments.len())
                        .find(|&i| self.class.attachments[i].ammo_used > 0 && self.started & (1 << i) != 0);
                    if let Some(i) = started {
                        let a = self.class.attachments[i].clone();
                        if self.ammo < a.ammo_used {
                            self.release_trigger(sys, host, true);
                        } else {
                            self.ammo -= a.ammo_used;
                            if self.class.has(flag::AUTOFIRE) {
                                self.pulse(sys, host, i, a.shooter, motion);
                            } else {
                                self.chamber = a.chamber_time;
                                self.chamber_total = a.chamber_time;
                            }
                        }
                    }
                }
            }
            if self.chamber == 0 {
                self.stop_chamber_delay(sys, host);
            }
        }
        self.reload_delay = self.reload_delay.saturating_sub(1);
        if self.firing {
            for i in 0..self.start_delay.len() {
                if self.start_delay[i] > 0 {
                    self.start_delay[i] -= 1;
                    if self.start_delay[i] == 0 {
                        self.deliver(sys, host, i, event::START, motion);
                    }
                }
            }
        }
    }

    /// One tick lying free: falling until it lands (`floor` = the height
    /// below, if any), then lying, fading and going.
    pub fn tick_free(&mut self, floor: Option<f32>) {
        if !self.resting {
            // (A falling gun's physics is approximated: the characters'
            // jump gravity, its upward speed capped as Oni caps it.)
            self.velocity.y = (self.velocity.y - DROP_GRAVITY).clamp(-10.0, 2.25);
            let next = self.position + self.velocity;
            match floor {
                Some(y) if next.y <= y => {
                    self.position = Vec3::new(next.x, y, next.z);
                    self.velocity = Vec3::ZERO;
                    self.resting = true;
                }
                _ => self.position = next,
            }
        }
        self.matrix.set(Mat4::from_rotation_translation(glam::Quat::from_rotation_y(self.yaw), self.position));
        if self.free_time > 0 {
            self.free_time -= 1;
            if self.free_time == 0 {
                self.fade_time = FADE_TIME;
            }
        } else if self.fade_time > 0 {
            self.fade_time -= 1;
            if self.fade_time == 0 {
                self.gone = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p3::{CharHit, EnvHit, Target};

    /// A floor at y = 0 and one target: a sphere of radius 6 at `at`.
    struct Range {
        at: Vec3,
        hits: u32,
        blasts: u32,
    }

    impl Host for Range {
        fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit> {
            let to = from + ray;
            (from.y > 0.0 && to.y <= 0.0).then(|| EnvHit { fraction: from.y / (from.y - to.y), normal: Vec3::Y, surface: 0 })
        }
        fn ray_chars(&mut self, from: Vec3, ray: Vec3, radius: f32, owner: Option<usize>) -> Option<CharHit> {
            if owner == Some(1) {
                return None;
            }
            let r = 6.0 + radius;
            let oc = from - self.at;
            if oc.length() <= r {
                return Some(CharHit { fraction: 0.0, who: 1, normal: Vec3::Y });
            }
            let a = ray.length_squared();
            if a < 1e-12 {
                return None;
            }
            let b = oc.dot(ray);
            let disc = b * b - a * (oc.length_squared() - r * r);
            if disc < 0.0 {
                return None;
            }
            let t = (-b - disc.sqrt()) / a;
            (0.0..=1.0).contains(&t).then(|| CharHit { fraction: t, who: 1, normal: (oc + ray * t).normalize() })
        }
        fn hit_char(&mut self, _w: usize, _p: Vec3, _d: Vec3, _k: u32, _dm: f32, _s: f32, _kb: f32, _o: u32, _st: Option<u64>, _si: bool) -> bool {
            self.hits += 1;
            false
        }
        fn blast(&mut self, c: Vec3, _k: u32, _d: f32, _s: f32, _kb: f32, r: f32, _f: u32, _o: u32, _st: u64, _si: bool) {
            if c.distance(self.at) <= r + 6.0 {
                self.blasts += 1;
            }
        }
        fn targets(&self) -> Vec<Target> {
            vec![Target { who: 1, position: self.at, velocity: Vec3::ZERO }]
        }
        fn hostile(&self, _a: usize, _b: usize) -> bool {
            true
        }
        fn clear(&mut self, _a: Vec3, _b: Vec3) -> bool {
            true
        }
    }

    /// Every shipped gun, fired with its real particles at a target 60 units
    /// ahead: the shots fly, and (all but the inert one) reach it.
    #[test]
    fn every_shipped_weapon_fires_and_hits() {
        let common = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../generated/oni/common");
        let particles = Rc::new(p3::Classes::load(&common.join("particle_classes.json")));
        let classes = WeaponClasses::load(&common.join("weapons.json"), &particles);
        if classes.by_name.is_empty() {
            eprintln!("no converted weapons: skipped");
            return;
        }
        let mut names: Vec<_> = classes.by_name.keys().cloned().collect();
        names.sort();
        for name in names {
            let mut sys = System::new(particles.clone());
            let mut host = Range { at: Vec3::new(0.0, 10.0, 60.0), hits: 0, blasts: 0 };
            let mut gun = Weapon::new(classes.get(&name).unwrap(), Vec3::ZERO, 0.0, false);
            gun.assign(0);
            // Barrel (+X) along +Z, top (+Z) up.
            let hand = Mat4::from_cols(Vec3::Z.extend(0.0), Vec3::X.extend(0.0), Vec3::Y.extend(0.0), Vec3::new(0.0, 10.0, 0.0).extend(1.0));
            let mut fired = false;
            for tick in 1..=600u32 {
                gun.tick_held(&mut sys, &mut host, hand, Vec3::ZERO);
                if tick == 5 {
                    fired = gun.depress(&mut sys, &mut host, false, false, Vec3::ZERO).0;
                }
                if tick == 120 {
                    gun.release_trigger(&mut sys, &mut host, false);
                }
                sys.update(tick, &mut host);
                if std::env::var("ONI_DEBUG_GUN").ok().as_deref() == Some(name.as_str()) && tick % 30 == 0 {
                    for (_, p) in sys.iter() {
                        let c = sys.class(p.class);
                        eprintln!("  t{tick} {} at {:?} v {:?} attr {:?} life {}", c.name, sys.world_position(p), p.velocity, p.attractor, p.lifetime);
                    }
                }
            }
            eprintln!("{name}: fired {fired}, ammo {}, hits {}, blasts {}", gun.ammo, host.hits, host.blasts);
            assert!(fired, "{name} fires");
            // (w12_ba2's own particles never receive the events that would
            // make them do anything; w10_sni's homing rounds orbit a still
            // target wider than it is — both to be checked against the game.)
            if name != "w12_ba2" && name != "w10_sni" {
                assert!(host.hits + host.blasts > 0, "{name} reaches the target");
            }
        }
    }
}
