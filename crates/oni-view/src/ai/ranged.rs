//! Ranged targeting (`docs/AI.md`, *Shooting*): how an armed AI leads its
//! target, where it aims (with its skill's error), and when its gun is
//! pointed well enough to pull the trigger. Pure: the world hands in the
//! gun's pose each tick and acts on what comes out.

use std::collections::VecDeque;

use glam::{Mat3, Mat4, Vec3};

/// A class's shooting skill for one weapon slot.
#[derive(Clone, Copy, Debug, Default)]
pub struct Skill {
    pub recoil_compensation: f32,
    /// The sine of the angle it settles for at close range.
    pub best_aiming_sine: f32,
    /// Shot error, percent of the target's distance.
    pub error_percent: f32,
    /// How much of the last error carries over (0–1).
    pub decay: f32,
    /// Widens the gun's own spread (the particle emitter's inaccuracy).
    pub gun_inaccuracy: f32,
    /// Ticks between aimed shots.
    pub delay: (u16, u16),
}

impl Skill {
    pub fn from_json(j: &serde_json::Value) -> Skill {
        let f = |k: &str| j[k].as_f64().unwrap_or(0.0) as f32;
        Skill {
            recoil_compensation: f("recoilCompensation"),
            best_aiming_sine: f("bestAimingSine"),
            error_percent: f("shotErrorPercent"),
            decay: f("shotDecay").clamp(0.0, 1.0),
            gun_inaccuracy: f("gunInaccuracy"),
            delay: (j["delayMin"].as_u64().unwrap_or(0) as u16, j["delayMax"].as_u64().unwrap_or(0) as u16),
        }
    }
}

/// A class's targeting settings.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Startle shots miss by up to this ratio of the distance, capped.
    pub startle_ratio: f32,
    pub startle_distance: f32,
    pub predict_amount: f32,
    /// Ticks: lead from where the target was this long ago; the velocity
    /// window (from `delay` to `velocity` ago) and the trend window (to
    /// `trend` ago).
    pub position_delay: usize,
    pub delay: usize,
    pub velocity: usize,
    pub trend: usize,
}

impl Default for Params {
    fn default() -> Params {
        Params { startle_ratio: 0.5, startle_distance: 25.0, predict_amount: 1.0, position_delay: 0, delay: 5, velocity: 15, trend: 60 }
    }
}

impl Params {
    pub fn from_json(j: &serde_json::Value) -> Params {
        let d = Params::default();
        let f = |k: &str, v: f32| j[k].as_f64().map_or(v, |x| x as f32);
        let u = |k: &str, v: usize| j[k].as_u64().map_or(v, |x| x as usize);
        Params {
            startle_ratio: f("startleMissRatio", d.startle_ratio),
            startle_distance: f("startleMissDistance", d.startle_distance),
            predict_amount: f("predictAmount", d.predict_amount),
            position_delay: u("predictPositionDelay", d.position_delay),
            delay: u("predictDelay", d.delay),
            velocity: u("predictVelocity", d.velocity),
            trend: u("predictTrend", d.trend).max(1),
        }
    }
}

/// What targeting needs of the gun.
#[derive(Clone, Copy, Debug)]
pub struct Gun {
    pub aim_radius: f32,
    pub burst_radius: f32,
    /// Shot speed for leading (units/s).
    pub prediction_speed: f32,
    pub no_wild_shots: bool,
    /// Gun space: where the shot leaves (off its own line) and the turn from
    /// the shot's line onto the barrel (+X).
    pub shooter_offset: Vec3,
    pub shooter_inverse: Mat3,
}

/// Targeting one character (`AI2tTargetingState`).
#[derive(Clone, Debug)]
pub struct Targeting {
    pub params: Params,
    pub gun: Gun,
    pub skill: Skill,
    /// Where the target was, newest first.
    samples: VecDeque<Vec3>,
    pub target_pt: Vec3,
    pub target_distance: f32,
    error: Vec3,
    error_distance: f32,
    miss_enable: bool,
    miss_tolerance: f32,
    returning: bool,
    thwarted: u32,
    pub delay: u32,
    point_blank: u32,
    last_disallowed: bool,
    /// 1 when the target is nose to nose (within 10), 0 beyond 25.
    near: f32,
    desired: Vec3,
    aiming_distance: f32,
    /// How fast the aim may swing (×): 0.5 settling, 1.0 far off.
    pub aim_speed: f32,
}

/// A small random generator (the caller's state).
pub trait Random {
    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32;
}

impl Targeting {
    /// Starts targeting from `shoulder` at a target at `at`. `initial`: the
    /// first sight of this enemy — the opening shots go wild (startle) unless
    /// the gun forbids it.
    pub fn new(params: Params, gun: Gun, skill: Skill, initial: bool, at: Vec3, shoulder: Vec3) -> Targeting {
        let d = at.distance(shoulder);
        let miss_enable = initial && !gun.no_wild_shots;
        Targeting {
            params,
            gun,
            skill,
            samples: VecDeque::new(),
            target_pt: at,
            target_distance: d,
            error: Vec3::ZERO,
            error_distance: 0.0,
            miss_enable,
            miss_tolerance: if miss_enable { (d * params.startle_ratio).min(params.startle_distance) } else { 0.0 },
            returning: true,
            thwarted: 0,
            delay: 0,
            point_blank: 0,
            last_disallowed: true,
            near: 0.0,
            desired: Vec3::ZERO,
            aiming_distance: d,
            aim_speed: 0.5,
        }
    }

    /// This tick's sight of the target (its location + height): the point
    /// to aim at, led by its recent velocity when it runs straight.
    pub fn predict(&mut self, at: Vec3, shoulder: Vec3) {
        let p = self.params;
        self.samples.push_front(at);
        self.samples.truncate(p.trend + 1);
        let n = self.samples.len() - 1;
        let ago = |k: usize| self.samples[k.min(n)];
        self.target_pt = ago(p.position_delay);
        self.target_distance = self.target_pt.distance(shoulder);
        if n < p.velocity || n < p.trend || p.velocity <= p.delay || p.trend <= p.delay {
            return;
        }
        let velocity = (ago(p.delay) - ago(p.velocity)) * 60.0 / (p.velocity - p.delay) as f32;
        let trend = (ago(p.delay) - ago(p.trend)) * 60.0 / (p.trend - p.delay) as f32;
        let speed = velocity.length();
        if speed < 1.0 || self.gun.prediction_speed <= 0.0 {
            return;
        }
        let cos = velocity.dot(trend) / (speed * trend.length()).max(1e-6);
        let accuracy = cos.min(trend.length() / speed);
        let weight = ((accuracy - 0.5) / 0.4).clamp(0.0, 1.0);
        if weight <= 0.0 {
            return;
        }
        let lead = p.predict_amount * weight * self.target_distance / self.gun.prediction_speed + p.position_delay as f32 / 60.0;
        self.target_pt += velocity * lead;
    }

    /// The direction the gun's barrel should point (world) so its shot's
    /// line runs from the shooter to the aim point — with the skill's error
    /// while shooting. `gun` is the gun's world matrix now.
    pub fn aim(&mut self, shoulder: Vec3, gun: Mat4, shooting: bool) -> Vec3 {
        let aim_pt = if shooting { self.target_pt + self.error } else { self.target_pt };
        let v = aim_pt - shoulder;
        let len = v.length();
        self.near = ((len - 25.0) / (10.0 - 25.0)).clamp(0.0, 1.0);
        // (Nose to nose, the reference point pulls back along the barrel so
        // a muzzle inside the target doesn't swing the aim.)
        let weapon_pt = gun.transform_point3(self.gun.shooter_offset - Vec3::X * 8.0 * self.near);
        self.desired = (shoulder - weapon_pt) + v;
        self.aiming_distance = self.desired.length();
        let r = Mat3::from_mat4(gun);
        let local = r.transpose() * self.desired.normalize_or(Vec3::X);
        (r * (self.gun.shooter_inverse * local)).normalize_or(Vec3::X)
    }

    /// Whether to fire this tick (`Execute`): `allowed` — it means to shoot
    /// and has a line of fire. Judged from where the gun points now.
    pub fn execute(&mut self, allowed: bool, gun: Mat4) -> bool {
        if !allowed {
            self.last_disallowed = true;
            return false;
        }
        if self.last_disallowed {
            self.last_disallowed = false;
            self.returning = true;
            self.point_blank = 0;
        }
        self.fire(gun)
    }

    fn fire(&mut self, gun: Mat4) -> bool {
        if self.delay > 0 {
            self.delay -= 1;
            if self.delay > 0 {
                return false;
            }
        }
        // Point blank a while: fire, whatever the aim.
        if self.aiming_distance < 10.0 {
            self.point_blank += 1;
            if self.point_blank >= 10 {
                self.returning = false;
                return true;
            }
        } else {
            self.point_blank = 0;
        }
        // Where the shot would go, in the shooter's frame: along (axial) and
        // how far off at the target (perpendicular).
        let r = Mat3::from_mat4(gun);
        let s = self.gun.shooter_inverse * (r.transpose() * self.desired);
        let (axial, off) = (s.x, (s.y * s.y + s.z * s.z).sqrt());
        // (As shipped, near and far are read the wrong way round: the strict
        // cone and the class's angle apply nose to nose, and every target
        // beyond 25 units gets four times the tolerance.)
        let ratio = 0.5 + self.near * (2.0 - 0.5);
        if off > 0.0 && axial / off < ratio {
            self.aim_speed = 1.0;
            return false;
        }
        let mut tol = self.gun.aim_radius;
        let mid_burst = self.gun.burst_radius > self.gun.aim_radius && !self.returning;
        if mid_burst {
            tol = self.gun.burst_radius;
        }
        tol += 1.5 * self.error_distance;
        if self.miss_enable {
            tol = tol.max(self.miss_tolerance);
        }
        if self.thwarted > 30 {
            tol += 0.1 * (self.thwarted - 30) as f32;
        }
        if self.near < 1.0 {
            tol *= 1.0 + 3.0 * (1.0 - self.near);
        } else {
            tol = tol.max(self.skill.best_aiming_sine * axial);
        }
        if off > tol {
            if off >= 5.0 * tol {
                self.aim_speed = 1.0;
            }
            if !self.miss_enable {
                self.returning = true;
            }
            if mid_burst {
                self.start_delay(0.5);
            }
            self.thwarted += 1;
            return false;
        }
        if self.miss_enable {
            self.miss_tolerance = self.miss_tolerance.min(off);
            if self.miss_tolerance < self.gun.aim_radius {
                self.miss_enable = false;
            }
        }
        self.returning = false;
        self.thwarted = 0;
        self.aim_speed = 0.5;
        true
    }

    fn start_delay(&mut self, roll: f32) {
        let (lo, hi) = self.skill.delay;
        self.delay = u32::from(lo) + (roll * f32::from(hi.saturating_sub(lo))) as u32;
    }

    /// A shot really left the gun: a new error for the next, and — for a gun
    /// that doesn't fire bursts — the pause before it.
    pub fn fired(&mut self, rng: &mut dyn Random) {
        let inacc = self.skill.error_percent * self.target_distance / 100.0;
        self.error *= self.skill.decay;
        self.error_distance = inacc * (1.0 - self.skill.decay);
        self.error += Vec3::new(rng.unit() * 2.0 - 1.0, rng.unit() * 2.0 - 1.0, rng.unit() * 2.0 - 1.0) * inacc;
        if self.gun.burst_radius == 0.0 {
            let roll = rng.unit();
            self.start_delay(roll);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(f32);
    impl Random for Fixed {
        fn unit(&mut self) -> f32 {
            self.0
        }
    }

    fn gun() -> Gun {
        Gun {
            aim_radius: 2.5,
            burst_radius: 0.0,
            prediction_speed: 900.0,
            no_wild_shots: false,
            shooter_offset: Vec3::ZERO,
            shooter_inverse: Mat3::IDENTITY,
        }
    }

    /// A gun at the origin pointing its barrel along `dir`.
    fn pointing(dir: Vec3) -> Mat4 {
        let x = dir.normalize();
        let z = (Vec3::Y - x * x.y).normalize();
        Mat4::from_mat3(Mat3::from_cols(x, z.cross(x), z))
    }

    #[test]
    fn a_straight_runner_is_led_a_juker_is_not() {
        let mut t = Targeting::new(Params::default(), gun(), Skill::default(), false, Vec3::new(0.0, 0.0, 300.0), Vec3::ZERO);
        // Running +X at 60 units/s for a second.
        for k in 0..=60 {
            t.predict(Vec3::new(k as f32, 0.0, 300.0), Vec3::ZERO);
        }
        // 300 units at 900 units/s: a third of a second ahead (20 units).
        assert!((t.target_pt.x - 80.0).abs() < 1.0, "led to {}", t.target_pt.x);
        // Back and forth every 10 ticks: no lead.
        let mut j = Targeting::new(Params::default(), gun(), Skill::default(), false, Vec3::new(0.0, 0.0, 300.0), Vec3::ZERO);
        for k in 0..=60 {
            let x = if (k / 10) % 2 == 0 { (k % 10) as f32 } else { 10.0 - (k % 10) as f32 };
            j.predict(Vec3::new(x, 0.0, 300.0), Vec3::ZERO);
        }
        assert!((j.target_pt.x - j.samples[0].x).abs() < 1e-3);
    }

    #[test]
    fn fires_when_pointed_and_waits_its_delay() {
        let target = Vec3::new(0.0, 0.0, 100.0);
        let skill = Skill { delay: (30, 30), ..Skill::default() };
        let mut t = Targeting::new(Params::default(), gun(), skill, false, target, Vec3::ZERO);
        let on = pointing(Vec3::Z);
        let dir = t.aim(Vec3::ZERO, on, true);
        assert!(dir.angle_between(Vec3::Z) < 1e-3);
        assert!(t.execute(true, on), "pointed straight at it");
        t.fired(&mut Fixed(0.5));
        assert_eq!(t.delay, 30);
        let fired_again = (0..29).any(|_| t.execute(true, on));
        assert!(!fired_again, "waits out the delay");
        assert!(t.execute(true, on));
        // Pointed well off: no shot.
        let off = pointing(Vec3::new(0.5, 0.0, 1.0));
        t.aim(Vec3::ZERO, off, true);
        assert!(!t.execute(true, off));
    }

    #[test]
    fn startle_shots_go_wide_then_tighten() {
        let target = Vec3::new(0.0, 0.0, 100.0);
        let mut t = Targeting::new(Params::default(), gun(), Skill::default(), true, target, Vec3::ZERO);
        // 10 units off at 100: outside the plain tolerance (2.5 × 4), inside
        // the startle one (25).
        let off = pointing(Vec3::new(0.12, 0.0, 1.0));
        t.aim(Vec3::ZERO, off, true);
        assert!(t.execute(true, off), "a wild opening shot");
        let on = pointing(Vec3::Z);
        t.aim(Vec3::ZERO, on, true);
        assert!(t.execute(true, on));
        // Startle over: the wide aim no longer fires.
        t.aim(Vec3::ZERO, off, true);
        assert!(!t.execute(true, off));
    }
}
