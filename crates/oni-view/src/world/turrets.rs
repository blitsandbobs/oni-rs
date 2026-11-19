//! Turrets (`docs/TURRETS.md`): placed guns on a swivelling head that,
//! switched on (a laser's event, a script), turn toward the nearest
//! character of their target teams and fire while their targeting says the
//! barrel is on — for their class's time, then back to rest and off.

use std::cell::Cell;
use std::f32::consts::{PI, TAU};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use glam::{Mat3, Mat4, Quat, Vec3};
use serde_json::Value;

use super::weapons::ShotHost;
use super::World;
use crate::p3::{self, event, Handle};

/// Every this many ticks it looks for its target again.
const CHECK_TICKS: u16 = 15;
/// Line of sight is tested from this far out from the base.
const LOS_START: f32 = 15.0;

/// One attachment: a particle riding the barrel.
struct Attachment {
    class: Option<p3::ClassId>,
    /// −1: follows every shooter.
    shooter: i32,
    chamber: u16,
    /// Barrel space.
    matrix: Mat4,
}

/// A turret class (`common/turrets.json`).
pub struct TurretClass {
    pub head: Option<PathBuf>,
    pub barrel: Option<PathBuf>,
    head_pivot: Vec3,
    barrel_pivot: Vec3,
    timeout: u32,
    /// Its hum while active (an ambient sound).
    active_sound: Option<String>,
    /// (min, max, speed a tick), radians.
    pitch: (f32, f32, f32),
    yaw: (f32, f32, f32),
    attachments: Vec<Attachment>,
    gun: crate::ai::ranged::Gun,
    params: crate::ai::ranged::Params,
    pub skill: crate::ai::ranged::Skill,
}

/// One placed turret.
pub struct Turret {
    pub class: Rc<TurretClass>,
    /// Its script id (not unique).
    pub id: u16,
    position: Vec3,
    rotation: Mat3,
    teams: u32,
    pub active: bool,
    /// Head yaw and barrel pitch, and where they head; both in [0, 2π).
    pub yaw: f32,
    pub pitch: f32,
    desired: (f32, f32),
    fire: bool,
    chamber: u16,
    check: u16,
    active_time: u32,
    pub firing: bool,
    pub target: Option<usize>,
    los: bool,
    background: bool,
    particles: Vec<Option<Handle>>,
    /// Barrel space → world (its particles ride it).
    matrix: Rc<Cell<Mat4>>,
    targeting: Option<Box<crate::ai::ranged::Targeting>>,
    /// Shots fired (for tests).
    pub shots: u32,
    /// Its hum, while playing (`None` also when out of earshot at the
    /// start — but `humming` is set either way, as in the original).
    sound: Option<usize>,
    humming: bool,
}

fn wrap(a: f32) -> f32 {
    a.rem_euclid(TAU)
}

/// The shortest signed turn from `a` to `b`.
fn delta(a: f32, b: f32) -> f32 {
    (b - a + PI).rem_euclid(TAU) - PI
}

impl Turret {
    /// The object's matrix (rotation, base position).
    fn object(&self) -> Mat4 {
        Mat4::from_mat3_translation(self.rotation, self.position)
    }

    /// The head's and the barrel's world matrices.
    pub fn head_matrix(&self) -> Mat4 {
        self.object() * Mat4::from_rotation_translation(Quat::from_rotation_y(self.yaw), self.class.head_pivot)
    }

    fn rebuild(&self) {
        // (A positive pitch raises the barrel's +Z toward +Y.)
        let barrel = Mat4::from_rotation_translation(Quat::from_rotation_x(-self.pitch), self.class.barrel_pivot);
        self.matrix.set(self.head_matrix() * barrel);
    }

    pub fn barrel_matrix(&self) -> Mat4 {
        self.matrix.get()
    }

    /// Where its aim is reckoned from: the base plus the head and barrel
    /// pivots — not turned by the mount (as shipped: an upside-down
    /// ceiling or wall turret aims some 8 units high. SHIPPED BALANCE).
    fn aim_origin(&self) -> Vec3 {
        self.position + self.class.head_pivot + self.class.barrel_pivot
    }

    /// The yaw and pitch that point the barrel at `p` (object space, from
    /// the unturned pivots, as `aim_origin`).
    fn look_at(&self, p: Vec3) -> (f32, f32) {
        let local = self.rotation.transpose() * (p - self.position) - (self.class.head_pivot + self.class.barrel_pivot);
        let flat = (local.x * local.x + local.z * local.z).sqrt();
        (wrap(local.x.atan2(local.z)), wrap(local.y.atan2(flat)))
    }

    /// Clips an angle to `(min, max)` (taken in (−π, π]).
    fn clip(a: f32, (min, max, _): (f32, f32, f32)) -> f32 {
        let s = if a >= PI { a - TAU } else { a };
        wrap(s.clamp(min, max))
    }

    /// Turns the head and barrel toward where they head, each at its speed.
    fn slew(&mut self) {
        let c = &self.class;
        self.desired = (Self::clip(self.desired.0, c.yaw), Self::clip(self.desired.1, c.pitch));
        let step = |cur: f32, want: f32, speed: f32| wrap(cur + delta(cur, want).clamp(-speed, speed));
        self.yaw = step(self.yaw, self.desired.0, c.yaw.2);
        self.pitch = step(self.pitch, self.desired.1, c.pitch.2);
    }
}

fn v3(j: &Value) -> Vec3 {
    Vec3::new(j[0].as_f64().unwrap_or(0.0) as f32, j[1].as_f64().unwrap_or(0.0) as f32, j[2].as_f64().unwrap_or(0.0) as f32)
}

fn class(c: &Value, common: &Path, particles: &p3::Classes) -> TurretClass {
    let f = |j: &Value| j.as_f64().unwrap_or(0.0) as f32;
    let limits = |j: &Value| (f(&j["min"]), f(&j["max"]), f(&j["speed"]));
    let mut gun = crate::weapons::AiGun::from_json(&c["ai"]).aim;
    gun.barrel_z = true;
    TurretClass {
        head: c["head"].as_str().map(|m| common.join(m)),
        barrel: c["barrel"].as_str().map(|m| common.join(m)),
        head_pivot: v3(&c["headPivot"]),
        barrel_pivot: v3(&c["barrelPivot"]),
        timeout: c["timeout"].as_u64().unwrap_or(0) as u32,
        active_sound: c["activeSound"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
        pitch: limits(&c["pitch"]),
        yaw: limits(&c["yaw"]),
        attachments: c["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                let m: Vec<f32> = a["matrix"].as_array().map(|m| m.iter().map(f).collect()).unwrap_or_default();
                let col = |k: usize| {
                    glam::Vec4::new(
                        m.get(k * 3).copied().unwrap_or(0.0),
                        m.get(k * 3 + 1).copied().unwrap_or(0.0),
                        m.get(k * 3 + 2).copied().unwrap_or(0.0),
                        if k == 3 { 1.0 } else { 0.0 },
                    )
                };
                Attachment {
                    class: a["class"].as_str().and_then(|n| particles.get(n)),
                    shooter: a["shooter"].as_i64().unwrap_or(-1) as i32,
                    chamber: a["chamber"].as_u64().unwrap_or(0) as u16,
                    matrix: Mat4::from_cols(col(0), col(1), col(2), col(3)),
                }
            })
            .collect(),
        gun,
        params: crate::ai::ranged::Params::from_json(&c["targeting"]),
        skill: crate::ai::ranged::Skill::from_json(&c["skill"]),
    }
}

/// Loads the turret classes and a level's placed turrets.
pub fn load(level_dir: &Path, common: &Path, particles: &p3::Classes) -> Vec<Turret> {
    let read = |p: &Path| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let (Some(classes), Some(placed)) = (read(&common.join("turrets.json")), read(&level_dir.join("turrets.json"))) else {
        return Vec::new();
    };
    let mut by_name: std::collections::HashMap<String, Rc<TurretClass>> = Default::default();
    placed["turrets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let name = t["class"].as_str()?;
            let class = match by_name.get(name) {
                Some(c) => c.clone(),
                None => {
                    let c = Rc::new(class(&classes["classes"][name], common, particles));
                    by_name.insert(name.to_string(), c.clone());
                    c
                }
            };
            let a = &t["axes"];
            let tu = Turret {
                class,
                id: t["id"].as_u64().unwrap_or(0) as u16,
                position: v3(&t["position"]),
                rotation: Mat3::from_cols(v3(&a[0]), v3(&a[1]), v3(&a[2])),
                teams: t["targetTeams"].as_u64().unwrap_or(1) as u32,
                active: false,
                yaw: 0.0,
                pitch: 0.0,
                desired: (0.0, 0.0),
                fire: false,
                chamber: 60,
                check: 0,
                active_time: 0,
                firing: false,
                target: None,
                los: false,
                background: false,
                particles: Vec::new(),
                matrix: Rc::new(Cell::new(Mat4::IDENTITY)),
                targeting: None,
                shots: 0,
                sound: None,
                humming: false,
            };
            tu.rebuild();
            Some(tu)
        })
        .collect()
}

impl World {
    /// Sends `ev` to turret `k`'s attachments following shooter `group`
    /// (−1: all) — stopped, owned by the turret.
    fn turret_send(&mut self, k: usize, ev: usize, group: i32) {
        let t = &self.turrets[k];
        let owner = p3::owner::TURRET | u32::from(t.id);
        let handles: Vec<Handle> = t
            .class
            .attachments
            .iter()
            .zip(&t.particles)
            .filter(|(a, _)| group == -1 || a.shooter == -1 || a.shooter == group)
            .filter_map(|(_, h)| *h)
            .collect();
        let now = self.now as u64;
        let time = f64::from(self.armory.particles.tick) / p3::HZ;
        let inaccuracy = self.inaccuracies();
        let spread = self.turret_spread();
        let super::weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
        host.set_spread(inaccuracy, spread);
        for h in handles {
            if let Some(p) = particles.get_mut(h) {
                p.velocity = Vec3::ZERO;
                p.owner = owner;
            }
            particles.send(h, ev, time, &mut host);
        }
    }

    /// Creates turret `k`'s attachment particles (riding its barrel), with
    /// their background effects off.
    fn turret_particles(&mut self, k: usize) {
        let t = &self.turrets[k];
        let (class, matrix) = (t.class.clone(), t.matrix.clone());
        let now = self.now as u64;
        let tick = self.armory.particles.tick;
        let time = f64::from(tick) / p3::HZ;
        let super::weapons::Armory { particles, .. } = &mut self.armory;
        let mut host = ShotHost::new(&mut self.fighters, self.sight.as_ref(), now);
        let handles = class
            .attachments
            .iter()
            .map(|a| {
                let c = a.class?;
                let mut p = particles.create(c, tick);
                p.position = a.matrix.w_axis.truncate();
                p.orientation = Mat3::from_mat4(a.matrix);
                p.matrix = Some(matrix.clone());
                p.lifetime = 0.0;
                particles.created(p, time, &mut host)
            })
            .collect();
        drop(host);
        self.turrets[k].particles = handles;
        self.turret_send(k, event::BG_STOP, -1);
    }

    /// Switches turrets by script id (`turret_activate` / `_deactivate` /
    /// `_reset`; −1: every turret — except to deactivate, which then
    /// switches none, as shipped).
    pub fn turret_set(&mut self, id: i64, what: &str) {
        for k in 0..self.turrets.len() {
            let t = &self.turrets[k];
            let all = id == -1 || id == 0xFFFF;
            let picked = if all { what != "deactivate" } else { i64::from(t.id) == id };
            if !picked {
                continue;
            }
            match what {
                "activate" if !t.active => {
                    let t = &mut self.turrets[k];
                    t.active = true;
                    t.active_time = 0;
                    t.targeting = None;
                    self.turret_hum(k, true);
                    self.turret_send(k, event::BG_START, -1);
                }
                "deactivate" | "reset" if t.active => {
                    let t = &mut self.turrets[k];
                    t.desired = (0.0, 0.0);
                    t.target = None;
                    t.targeting = None;
                    t.fire = false;
                    t.active = false;
                    let was_firing = std::mem::replace(&mut t.firing, false);
                    if was_firing {
                        self.turret_send(k, event::STOP, -1);
                    }
                    // (The background flag stays set: the idle glow stays
                    // dark — as shipped.)
                    self.turret_send(k, event::BG_STOP, -1);
                    self.turret_hum(k, false);
                }
                _ => {}
            }
        }
    }

    /// Every turret's tick (`docs/TURRETS.md`), before the guns'.
    pub(super) fn turrets_tick(&mut self) {
        for k in 0..self.turrets.len() {
            if self.turrets[k].particles.is_empty() && !self.turrets[k].class.attachments.is_empty() {
                self.turret_particles(k);
            }
            // (Always taken as in view: its idle effect comes on once.)
            if !self.turrets[k].background && !self.turrets[k].active {
                self.turrets[k].background = true;
                self.turret_send(k, event::BG_START, -1);
            }
            let t = &mut self.turrets[k];
            if t.active {
                let c = t.class.clone();
                if c.timeout != 0 && t.active_time > c.timeout {
                    // Its time is up: once back at rest, it switches off.
                    t.fire = false;
                    if t.desired == (t.yaw, t.pitch) {
                        self.turret_off(k);
                    }
                } else {
                    t.active_time += 1;
                    self.turret_combat(k);
                }
            } else {
                t.fire = false;
            }
            let t = &mut self.turrets[k];
            t.slew();
            t.rebuild();
            // Firing.
            if t.chamber > 0 {
                t.chamber -= 1;
            } else if t.fire && !self.all_passive {
                t.fire = false;
                let Some(a) = t.class.attachments.iter().find(|a| a.shooter == 0) else { continue };
                let chamber = a.chamber;
                let start = !t.firing;
                t.firing = true;
                t.chamber = chamber;
                t.shots += 1;
                if start {
                    self.turret_send(k, event::START, 0);
                }
                self.turret_send(k, event::PULSE, 0);
            } else if t.firing {
                t.firing = false;
                self.turret_send(k, event::STOP, -1);
            }
        }
    }

    /// Starts or stops turret `k`'s hum.
    fn turret_hum(&mut self, k: usize, on: bool) {
        let t = &mut self.turrets[k];
        if on && !t.humming {
            t.humming = true;
            if let Some(name) = t.class.active_sound.clone() {
                let at = t.position;
                let sound = self.audio.ambient_start(&name, at, self.sight.as_ref());
                self.turrets[k].sound = sound;
            }
        } else if !on && t.humming {
            t.humming = false;
            if let Some(h) = t.sound.take() {
                self.audio.ambient_stop(h);
            }
        }
    }

    /// Switches turret `k` off (its time up).
    fn turret_off(&mut self, k: usize) {
        let t = &mut self.turrets[k];
        if !t.active {
            return;
        }
        t.target = None;
        t.targeting = None;
        t.active = false;
        let was_firing = std::mem::replace(&mut t.firing, false);
        if was_firing {
            self.turret_send(k, event::STOP, -1);
        }
        self.turret_send(k, event::BG_STOP, -1);
        self.turret_hum(k, false);
    }

    /// An active turret's target, aim and trigger this tick.
    fn turret_combat(&mut self, k: usize) {
        let t = &self.turrets[k];
        let mut target = t.target;
        let mut los = t.los;
        let mut check = t.check;
        if check > 0 {
            check -= 1;
        } else {
            check = CHECK_TICKS;
            // The nearest living character on one of its teams — anywhere.
            let teams = t.teams;
            let from = t.position;
            target = self
                .fighters
                .iter()
                .enumerate()
                .filter(|(_, f)| f.alive() && teams & (1 << f.team() as u32) != 0)
                .min_by(|a, b| a.1.feet().distance_squared(from).total_cmp(&b.1.feet().distance_squared(from)))
                .map(|(i, _)| i);
            los = match target {
                Some(i) => {
                    let to = self.fighters[i].feet() + Vec3::Y * self.fighters[i].motion.height();
                    let d = to - from;
                    d.length() < LOS_START || self.sight.as_ref().is_none_or(|s| s.clear(from + d.normalize() * LOS_START, to))
                }
                None => false,
            };
        }
        let t = &mut self.turrets[k];
        t.check = check;
        t.target = target;
        t.los = los;
        let Some(i) = target else {
            t.desired = (0.0, 0.0);
            return;
        };
        let at = self.fighters[i].feet() + Vec3::Y * self.fighters[i].motion.height();
        let t = &mut self.turrets[k];
        let from = t.aim_origin();
        let m = t.matrix.get();
        let c = t.class.clone();
        let targeting = t
            .targeting
            .get_or_insert_with(|| Box::new(crate::ai::ranged::Targeting::new(c.params, c.gun, c.skill, false, at, from)));
        targeting.predict(at, from);
        let dir = targeting.aim(from, m, los);
        let length = targeting.target_distance.max(1.0);
        let fire = targeting.execute(los, m);
        let want = from + dir * length;
        t.desired = t.look_at(want);
        t.fire = fire;
    }

    /// The turrets' shot spread (degrees): the difficulty's multiplier on
    /// their classes' (all 1).
    pub(super) fn turret_spread(&self) -> f32 {
        self.turrets.first().map_or(1.0, |t| t.class.skill.gun_inaccuracy) * self.difficulty.inaccuracy
    }

    /// What to draw of the turrets: each head and barrel model and matrix.
    pub fn turret_draws(&self) -> Vec<(PathBuf, Mat4)> {
        self.turrets
            .iter()
            .flat_map(|t| {
                [t.class.head.clone().map(|p| (p, t.head_matrix())), t.class.barrel.clone().map(|p| (p, t.barrel_matrix()))]
            })
            .flatten()
            .collect()
    }
}
