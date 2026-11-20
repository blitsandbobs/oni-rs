//! What each action does (`docs/PARTICLES.md`, *Actions*). An action list
//! runs in order; an action may kill the particle (ending everything) or
//! stop the list.

use glam::{Mat3, Quat, Vec3};

use super::class::{cflag, event, Kind, Value};
use super::{owner, pflag, Collision, Handle, Host, Outcome, Particle, System, Val, DISABLED, HZ, NEVER};

/// Gravity, units per second² (1 unit = 10 cm).
const GRAVITY: f32 = 98.0;

/// An enum parameter read raw: an enum's value, a float's bits as a whole
/// number, nothing for anything else.
fn raw_enum(v: &Value) -> Option<u32> {
    match v {
        Value::Enum(e) => Some(*e),
        Value::Float(f) => Some(*f as u32),
        _ => None,
    }
}

impl System {
    /// Runs the action list of event `ev` on `p` at time `t` (`dt` = the
    /// step's length, for the update list).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run(
        &mut self,
        p: &mut Particle,
        h: Handle,
        ev: usize,
        t: f64,
        dt: f32,
        hit: Option<Collision>,
        expect_collision: bool,
        host: &mut dyn Host,
    ) -> Outcome {
        let classes = self.classes.clone();
        let c = &classes.list[p.class];
        let range = c.events[ev].clone();
        let mut out = Outcome::default();
        for i in range {
            let Some(a) = c.actions.get(i) else { break };
            if ev == event::UPDATE && i < 32 && p.action_mask & (1 << i) != 0 {
                continue;
            }
            if (a.kind.not_if_stopped() && p.flags & pflag::STOPPED != 0)
                || (a.kind.not_if_chopped() && p.flags & pflag::CHOP != 0)
                || (a.kind.not_if_collision() && expect_collision)
            {
                continue;
            }
            let o = self.action(p, h, i, t, dt, hit, host);
            if o.dead {
                return o;
            }
            if o.stop {
                out.stop = true;
                break;
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn action(&mut self, p: &mut Particle, h: Handle, i: usize, t: f64, dt: f32, hit: Option<Collision>, host: &mut dyn Host) -> Outcome {
        let classes = self.classes.clone();
        let c = &classes.list[p.class];
        let a = &c.actions[i];
        let none = Outcome::default();
        let dead = Outcome { dead: true, stop: false };
        let stop = Outcome { dead: false, stop: true };
        let vars = p.vars.clone();
        macro_rules! f {
            ($k:expr) => {
                match self.eval_f(a.param($k), &vars) {
                    Some(x) => x,
                    None => return none,
                }
            };
        }
        macro_rules! var {
            ($k:expr) => {
                match a.vars[$k] {
                    Some(v) if v < p.vars.len() => v,
                    _ => return none,
                }
            };
        }
        let getf = |p: &Particle, v: usize| p.vars[v].f().unwrap_or(0.0);
        let has_v = c.has(cflag::VELOCITY);
        let has_o = c.has(cflag::ORIENTATION);
        match a.kind {
            Kind::AnimateLinear => {
                let v = var!(0);
                let rate = f!(0);
                p.vars[v] = Val::F(getf(p, v) + rate * dt);
            }
            Kind::AnimateAccel => {
                let (v, vel) = (var!(0), var!(1));
                let acc = f!(0);
                let speed = getf(p, vel) + acc * dt;
                p.vars[vel] = Val::F(speed);
                p.vars[v] = Val::F(getf(p, v) + speed * dt);
            }
            Kind::AnimateRandom => {
                let v = var!(0);
                let (lo, hi, rate) = (f!(0), f!(1), f!(2));
                let x = getf(p, v);
                let bias = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
                let r = self.random();
                let d = if bias < 0.5 { rate * (1.0 - 4.0 * bias * r) } else { rate * (-1.0 + 4.0 * (1.0 - bias) * r) };
                p.vars[v] = Val::F(x + dt * d);
            }
            Kind::AnimatePingpong => {
                let (v, dir) = (var!(0), var!(1));
                let (lo, hi, rate) = (f!(0), f!(1), f!(2));
                let up = matches!(p.vars[dir], Val::U(0)) || matches!(p.vars[dir], Val::F(x) if x == 0.0);
                let mut x = getf(p, v);
                if up {
                    x += dt * rate;
                    if x > hi {
                        x = hi;
                        p.vars[dir] = Val::U(1);
                    }
                } else {
                    x -= dt * rate;
                    if x < lo {
                        x = lo;
                        p.vars[dir] = Val::U(0);
                    }
                }
                p.vars[v] = Val::F(x);
            }
            Kind::AnimateLoop => {
                let v = var!(0);
                let (lo, hi, rate) = (f!(0), f!(1), f!(2));
                let mut x = getf(p, v) + dt * rate;
                if rate > 0.0 && x > hi {
                    x = lo;
                } else if rate < 0.0 && x < lo {
                    x = hi;
                }
                p.vars[v] = Val::F(x);
            }
            Kind::AnimateToValue => {
                let v = var!(0);
                let (rate, end) = (f!(0), f!(1));
                let mut x = getf(p, v) + dt * rate;
                if (rate > 0.0 && x > end) || (rate < 0.0 && x < end) {
                    x = end;
                }
                p.vars[v] = Val::F(x);
            }
            Kind::FadeOut => {
                if p.flags & pflag::FADE_OUT != 0 {
                    return none;
                }
                let time = f!(0);
                let Some(alpha) = self.eval_f(&c.alpha, &vars) else { return none };
                if alpha <= 0.0 {
                    return dead;
                }
                p.lifetime = time;
                p.fadeout_time = time / alpha;
                p.flags |= pflag::FADE_OUT;
            }
            Kind::EnableAtTime | Kind::DisableAtTime => {
                let Some(idx) = raw_enum(a.param(0)) else { return none };
                let target = f!(1);
                let enable = if a.kind == Kind::EnableAtTime { p.lifetime < target } else { p.lifetime > target };
                if idx < 31 {
                    if enable {
                        p.action_mask &= !(1 << idx);
                    } else {
                        p.action_mask |= 1 << idx;
                    }
                }
            }
            Kind::Die => return dead,
            Kind::SetLifetime => p.lifetime = f!(0).max(0.0),
            Kind::EmitActivate => {
                let Some(e) = raw_enum(a.param(0)).map(|e| e as usize).filter(|e| *e < p.emitters.len()) else { return none };
                let st = &mut p.emitters[e];
                st.count = 0;
                st.last_time = NEVER;
                st.last = None;
            }
            Kind::EmitDeactivate => {
                if let Some(st) = raw_enum(a.param(0)).and_then(|e| p.emitters.get_mut(e as usize)) {
                    st.count = DISABLED;
                }
            }
            Kind::EmitParticles => {
                let Some(e) = raw_enum(a.param(0)).map(|e| e as usize).filter(|e| *e < p.emitters.len()) else { return none };
                // (The fraction of the count never counts: always floored.)
                let n = match a.param(1) {
                    Value::Float(x) => x.max(0.0).floor() as u32,
                    _ => 0,
                };
                self.emit(p, h, e, t, n, host);
                let def = &c.emitters[e];
                let st = &mut p.emitters[e];
                if st.count != DISABLED && def.flags & super::class::eflag::INCREASE_COUNT != 0 {
                    let total = u32::from(st.count) + n;
                    st.count = if total >= u32::from(def.limit) && def.flags & super::class::eflag::DEACTIVATE_AT_LIMIT != 0 {
                        DISABLED
                    } else {
                        (total % u32::from(DISABLED)) as u16
                    };
                }
                st.last_time = t;
            }
            Kind::KillLastEmitted | Kind::ExplodeLastEmitted => {
                let Some(last) = raw_enum(a.param(0)).and_then(|e| p.emitters.get(e as usize)).and_then(|s| s.last) else {
                    return none;
                };
                if a.kind == Kind::KillLastEmitted {
                    self.kill(last, host);
                } else {
                    self.send(last, event::EXPLODE, t, host);
                }
            }
            Kind::DamageHitChar => {
                let Some(k) = hit.filter(|k| !k.env) else { return none };
                if p.flags & pflag::CUTSCENE_KILLED != 0 {
                    return none;
                }
                let (damage, stun, knockback) = (f!(0), f!(1), f!(2));
                let kind = match a.param(3) {
                    Value::Enum(e) if *e < 7 => *e,
                    _ => 0,
                };
                let self_immune = raw_enum(a.param(4)).is_some_and(|x| x != 0);
                let many = match a.param(5) {
                    Value::Enum(e) => *e != 0,
                    _ => true,
                };
                let who = self.owner_word(p, c);
                let stamp = (!many).then(|| h.id());
                if host.hit_char(k.id as usize, k.point, k.dir, kind, damage, stun, knockback, who, stamp, self_immune) {
                    return stop;
                }
            }
            Kind::DamageBlast => {
                let (damage, stun, knockback, radius) = (f!(0), f!(1), f!(2), f!(3));
                let falloff = raw_enum(a.param(4)).unwrap_or(0);
                let kind = match a.param(5) {
                    Value::Enum(e) if *e < 7 => *e,
                    _ => 0,
                };
                let self_immune = raw_enum(a.param(6)).is_some_and(|x| x != 0);
                let centre = match hit {
                    Some(k) => k.point + k.normal * 0.5,
                    None => self.world_position(p),
                };
                // (Breaking glass too, unless told not to.)
                if !matches!(a.param(7), Value::Enum(0)) {
                    host.damage_surfaces_near(centre, radius);
                }
                if p.flags & pflag::CUTSCENE_KILLED == 0 {
                    let who = self.owner_word(p, c);
                    host.blast(centre, kind, damage, stun, knockback, radius, falloff, who, h.id(), self_immune);
                }
            }
            Kind::Explode => {
                let o = self.run(p, h, event::EXPLODE, t, 0.0, hit, true, host);
                if o.dead {
                    return dead;
                }
            }
            Kind::DamageEnvironment => {
                if let Some(k) = hit.filter(|k| k.env) {
                    let d = f!(0);
                    host.damage_surface(k.id, d);
                }
            }
            Kind::Stop => {
                if self.run(p, h, event::STOP, t, 0.0, hit, false, host).dead {
                    return dead;
                }
            }
            Kind::RotateX | Kind::RotateY | Kind::RotateZ => {
                let world = !has_o || raw_enum(a.param(0)) == Some(1);
                let rate = f!(1);
                let with_velocity = raw_enum(a.param(2)).is_some_and(|x| x != 0);
                let axis = match a.kind {
                    Kind::RotateX => 0,
                    Kind::RotateY => 1,
                    _ => 2,
                };
                let angle = (rate * dt).to_radians();
                let axis_v = if world { Mat3::IDENTITY.col(axis) } else { p.orientation.col(axis) };
                let r = Mat3::from_quat(Quat::from_axis_angle(axis_v.normalize_or(Vec3::Y), angle));
                if has_o {
                    p.orientation = r * p.orientation;
                }
                if with_velocity && has_v {
                    p.velocity = r * p.velocity;
                    p.flags |= pflag::VELOCITY_CHANGED;
                }
            }
            Kind::FindAttractor => {
                let delay = (f!(0) * HZ as f32).round().max(0.0) as u32;
                if self.find_attractor(p, delay, host) && self.run(p, h, event::NEW_ATTRACTOR, t, 0.0, None, false, host).dead {
                    return dead;
                }
            }
            Kind::AttractHoming => {
                if p.flags & pflag::HAS_ATTRACTOR == 0 || !has_v {
                    return none;
                }
                let (turn, predict) = (f!(0), raw_enum(a.param(1)).is_some_and(|x| x != 0));
                let horizontal = raw_enum(a.param(2)).is_some_and(|x| x != 0);
                let Some(tg) = self.attractor_of(p, host) else { return none };
                let at = self.world_position(p);
                let mut speed = p.velocity.length();
                if speed < 1e-6 {
                    let seed = if has_o { p.orientation.y_axis } else { tg.position - at };
                    p.velocity = seed.normalize_or(Vec3::Y) * 1e-3;
                    speed = 1e-3;
                }
                let mut aim = tg.position;
                if predict {
                    let dist = (tg.position - at).length();
                    aim += tg.velocity * (dist / speed).min(1.0);
                }
                let want = aim - at;
                let old_y = p.velocity.y;
                let (from, to) = (p.velocity.normalize(), want.normalize_or(p.velocity.normalize()));
                let angle = from.angle_between(to);
                let max = (turn * dt).to_radians();
                let q = Quat::from_rotation_arc(from, to);
                let q = if angle > max && angle > 1e-6 { Quat::IDENTITY.slerp(q, max / angle) } else { q };
                // (The velocity only: the orientation follows it only if the
                // particle orients to its velocity.)
                p.velocity = q * p.velocity;
                if horizontal {
                    p.velocity.y = old_y;
                }
                p.flags |= pflag::VELOCITY_CHANGED;
            }
            Kind::AttractSpring => {
                if p.flags & pflag::HAS_ATTRACTOR == 0 || !has_v {
                    return none;
                }
                let (ratio, max, desired) = (f!(0), f!(1), f!(2));
                let Some(tg) = self.attractor_of(p, host) else { return none };
                let d = tg.position - self.world_position(p);
                let dist = d.length();
                let dir = if dist > 1e-6 { d / dist } else { self.random_unit_dir() };
                let acc = ((dist - desired) * ratio * dt).min(max);
                p.velocity += dir * acc;
                p.flags |= pflag::VELOCITY_CHANGED;
            }
            Kind::AttractGravity => {
                if p.flags & pflag::HAS_ATTRACTOR == 0 || !has_v {
                    return none;
                }
                let (g, max_g) = (f!(0), f!(1));
                let Some(tg) = self.attractor_of(p, host) else { return none };
                let d = tg.position - self.world_position(p);
                let l2 = d.length_squared().max(1e-6);
                p.velocity += d * (g * 100.0 / l2).min(max_g) * dt;
                p.flags |= pflag::VELOCITY_CHANGED;
            }
            Kind::MoveLine => {
                if has_v {
                    p.position += p.velocity * dt;
                }
            }
            // The offset position spiralled about the particle's direction
            // (`P3iAction_MoveSpiral`): its angle `theta` (a variable, in
            // degrees) turns at the rate; the offset lies at the radius (0:
            // the current one) on the orientation's X and Z axes, or, without
            // one, on axes across the velocity.
            Kind::MoveSpiral => {
                let v = var!(0);
                if !c.has(cflag::OFFSET_POS) || !(has_v || has_o) {
                    return none;
                }
                let mut radius = f!(0);
                if radius.abs() < 0.01 {
                    radius = p.offset.length();
                }
                let rate = f!(1);
                let mut theta = getf(p, v) + dt * rate;
                while theta < 0.0 {
                    theta += 360.0;
                }
                while theta > 360.0 {
                    theta -= 360.0;
                }
                p.vars[v] = Val::F(theta);
                // (As shipped: the sine from the cosine, negative below 180°.)
                let cos = theta.to_radians().cos();
                let mut sin = (1.0 - cos * cos).max(0.0).sqrt();
                if theta < 180.0 {
                    sin = -sin;
                }
                let (across, up) = if has_o {
                    (p.orientation.x_axis, p.orientation.z_axis)
                } else {
                    let across = Vec3::new(-p.velocity.z, 0.0, p.velocity.x).normalize_or_zero();
                    (across, across.cross(p.velocity).normalize_or_zero())
                };
                p.offset = radius * (cos * across - sin * up);
            }
            Kind::MoveGravity => {
                if has_v {
                    let g = f!(0);
                    p.velocity.y -= GRAVITY * g * dt;
                    p.flags |= pflag::VELOCITY_CHANGED;
                }
            }
            Kind::MoveResistance => {
                if has_v {
                    let (k, vmin) = (f!(0), f!(1));
                    let m = p.velocity.length();
                    if m > vmin {
                        let to = vmin + (m - vmin) * (-k * dt).exp();
                        p.velocity *= to / m;
                        p.flags |= pflag::VELOCITY_CHANGED;
                    }
                }
            }
            Kind::SetVelocity => {
                if !has_v {
                    return none;
                }
                let speed = f!(0);
                let Some(space) = raw_enum(a.param(1)) else { return none };
                let Some(kill_sideways) = raw_enum(a.param(2)) else { return none };
                if has_o && space == 0 {
                    let dir = p.orientation.y_axis;
                    let axial = dir.dot(p.velocity);
                    let tangential = p.velocity - dir * axial;
                    p.velocity = dir * speed;
                    if kill_sideways == 0 {
                        p.velocity += tangential;
                    }
                } else {
                    p.velocity = p.velocity.normalize_or_zero() * speed;
                }
                p.flags |= pflag::VELOCITY_CHANGED;
            }
            Kind::CollisionEffect => {
                let Value::Str(name) = a.param(0) else { return none };
                let Some(class) = classes.get(name) else { return none };
                let offset = self.eval_f(a.param(1), &vars).unwrap_or(0.0);
                let orient = match a.param(2) {
                    Value::Enum(e) => Some(*e),
                    _ => None,
                };
                let attach = matches!(a.param(3), Value::Enum(e) if *e != 0);
                let quad = hit.filter(|k| k.env).and_then(|k| host.struck_quad(k.point, k.normal));
                self.collision_effect(p, class, offset, orient, attach, hit, quad, t, host);
            }
            Kind::Bounce => {
                let Some(k) = hit else { return none };
                let direct = f!(0);
                p.position = k.point + k.normal * 0.01;
                let vn = p.velocity.dot(k.normal);
                if vn > 0.0 {
                    return none;
                }
                // (The glancing elasticity is read but never used.)
                p.velocity = (p.velocity - 2.0 * vn * k.normal) * direct;
                if k.env && p.velocity.length() < 1.0 {
                    p.velocity = Vec3::ZERO;
                    p.flags |= pflag::STOPPED;
                    p.flags &= !pflag::ORIENT_TO_VELOCITY;
                }
                p.time = k.t;
                p.flags |= pflag::VELOCITY_CHANGED;
            }
            Kind::StickToWall => {
                let Some(k) = hit else { return none };
                p.position = k.point;
                p.velocity = Vec3::ZERO;
                p.flags |= pflag::STOPPED;
            }
            Kind::Chop => {
                if p.flags & (pflag::FADE_OUT | pflag::CHOP) != 0 {
                    return none;
                }
                let speed = p.velocity.length();
                if speed < 0.01 {
                    return none;
                }
                let scale = self.eval_f(&c.scale, &vars).unwrap_or(1.0);
                p.lifetime = 2.0 * scale / speed;
                p.fadeout_time = p.lifetime;
                p.flags |= pflag::CHOP;
            }
            Kind::Show => p.flags &= !pflag::HIDDEN,
            Kind::Hide => p.flags |= pflag::HIDDEN,
            Kind::SetVariable => {
                let v = var!(0);
                if let Some(x) = self.eval(a.param(0), &vars) {
                    p.vars[v] = x;
                }
            }
            Kind::RecalculateAll => {
                for (k, def) in c.vars.iter().enumerate() {
                    if let Some(x) = self.eval(&def.init, &[]) {
                        p.vars[k] = x;
                    }
                }
            }
            Kind::EnableAbove | Kind::EnableBelow => {
                let Some(idx) = raw_enum(a.param(0)) else { return none };
                let (value, threshold) = (f!(1), f!(2));
                let enable = if a.kind == Kind::EnableAbove { value > threshold } else { value < threshold };
                if idx < 31 {
                    if enable {
                        p.action_mask &= !(1 << idx);
                    } else {
                        p.action_mask |= 1 << idx;
                    }
                }
            }
            Kind::EnableNow | Kind::DisableNow => {
                if let Some(idx) = raw_enum(a.param(0)).filter(|i| *i < 31) {
                    if a.kind == Kind::EnableNow {
                        p.action_mask &= !(1 << idx);
                    } else {
                        p.action_mask |= 1 << idx;
                    }
                }
            }
            Kind::SuperballTrigger => {
                let Some(e) = raw_enum(a.param(0)).map(|e| e as usize).filter(|e| *e < p.emitters.len()) else { return none };
                let fuse = f!(1);
                let st = &p.emitters[e];
                let Some(ball) = st.last.filter(|b| self.alive(*b)) else { return none };
                if t >= st.last_time + f64::from(fuse) {
                    self.send(ball, event::EXPLODE, t, host);
                } else if let Some(b) = self.get_mut(ball) {
                    b.flags |= pflag::EXPLODE_ON_CONTACT;
                }
            }
            Kind::StopIfBreakable => {
                if hit.is_some_and(|k| k.env && host.breakable(k.id)) {
                    return stop;
                }
            }
            Kind::StopIfSlow => {
                let s = f!(0);
                if has_v && p.velocity.length() < s {
                    return stop;
                }
            }
            Kind::AvoidWalls => {
                let (ax, ay, az, rs, tc) = (var!(0), var!(1), var!(2), var!(3), var!(4));
                let (sense, turn, decay) = (f!(0), f!(1), f!(2));
                if !has_v {
                    return none;
                }
                let mut axis = Vec3::new(getf(p, ax), getf(p, ay), getf(p, az));
                let mut rot = getf(p, rs);
                let mut until = getf(p, tc) - dt;
                let mut found = false;
                if until <= 0.0 {
                    until = 0.2;
                    let ray = p.velocity.normalize_or_zero() * sense;
                    if ray.length_squared() > 0.0 {
                        if let Some(k) = host.ray_env(p.position, ray) {
                            axis = k.normal.cross(ray).try_normalize().unwrap_or_else(|| Vec3::Y.cross(ray).normalize_or(Vec3::Y));
                            rot = turn;
                            found = true;
                        }
                    }
                }
                if !found {
                    rot -= decay * dt;
                    if rot < 1e-3 {
                        rot = 0.0;
                    }
                }
                if rot > 0.0 && axis.length_squared() > 1e-12 {
                    let r = Mat3::from_quat(Quat::from_axis_angle(axis.normalize(), (rot * dt).to_radians()));
                    p.velocity = r * p.velocity;
                    if has_o {
                        p.orientation = r * p.orientation;
                    }
                    p.flags |= pflag::VELOCITY_CHANGED;
                }
                p.vars[ax] = Val::F(axis.x);
                p.vars[ay] = Val::F(axis.y);
                p.vars[az] = Val::F(axis.z);
                p.vars[rs] = Val::F(rot);
                p.vars[tc] = Val::F(until);
            }
            Kind::RandomSwirl => {
                if !has_o {
                    return none;
                }
                let (base, delta, speed) = (f!(0), f!(1), f!(2));
                let v = var!(0);
                let angle = (getf(p, v) + (base + delta).to_radians() * dt).rem_euclid(std::f32::consts::TAU);
                p.vars[v] = Val::F(angle);
                let axis = angle.cos() * p.orientation.x_axis + angle.sin() * p.orientation.z_axis;
                if let Some(axis) = axis.try_normalize() {
                    let r = Mat3::from_quat(Quat::from_axis_angle(axis, (speed * dt).to_radians()));
                    if has_v {
                        p.velocity = r * p.velocity;
                    }
                    p.orientation = r * p.orientation;
                }
                p.flags |= pflag::VELOCITY_CHANGED;
            }
            // Sounds (`docs/AUDIO.md`, *Particles*): heard where the
            // particle really is.
            Kind::ImpulseSound => {
                let Value::Str(name) = a.param(0) else { return none };
                let at = self.world_position(p);
                self.sounds.push(super::SoundEvent::Impulse { name: name.clone(), at });
            }
            Kind::SoundStart => {
                let Value::Str(name) = a.param(0) else { return none };
                if p.flags & pflag::PLAYING_SOUND != 0 {
                    self.sounds.push(super::SoundEvent::AmbientStop { particle: h });
                }
                let at = self.world_position(p);
                self.sounds.push(super::SoundEvent::AmbientStart { particle: h, name: name.clone(), at });
                p.flags |= pflag::PLAYING_SOUND;
            }
            Kind::SoundEnd => {
                if p.flags & pflag::PLAYING_SOUND != 0 {
                    self.sounds.push(super::SoundEvent::AmbientStop { particle: h });
                }
                p.flags &= !pflag::PLAYING_SOUND;
            }
            Kind::ImpactEffect => {
                let Value::Str(impact) = a.param(0) else { return none };
                let modifier = match a.param(1) {
                    Value::Enum(e) => *e,
                    _ => 0,
                };
                // At the hit (a little off the surface), else where it is.
                let at = match hit {
                    Some(k) => k.point + k.normal * 0.2,
                    None => self.world_position(p),
                };
                let velocity = has_v.then_some(p.velocity);
                let orientation = has_o.then(|| self.world_orientation(p));
                let owner = p.owner;
                let flags = p.flags;
                self.sounds.push(super::SoundEvent::Impact { impact: impact.clone(), modifier, at, velocity, orientation, hit, owner, flags });
            }
            // A colour between two by a blend (`P3iAction_ColorInterpolate`,
            // `IMrShade_Interpolate`): the lock lights' red and green.
            Kind::ColorInterpolate => {
                let v = var!(0);
                let shade = |k: usize| match a.param(k) {
                    Value::Shade(s) | Value::ShadeRange(s, _) | Value::ShadeBell(s, _) => Some(*s),
                    Value::Var(i) => match vars.get(*i) {
                        Some(Val::U(u)) => Some(*u),
                        _ => None,
                    },
                    _ => None,
                };
                let (Some(c0), Some(c1)) = (shade(0), shade(1)) else { return none };
                let t = f!(2);
                p.vars[v] = Val::U(crate::decal::shade_lerp(c0, c1, t));
            }
            Kind::StopLink => p.link = None,
            Kind::BreakLink => p.flags |= pflag::BREAK_LINKS_TO_ME,
            // Sound, impact effects (decals, sparks by material), colour,
            // drift, the spiral tangent and the rest are not modelled yet
            // (#23, #24).
            _ => {}
        }
        none
    }

    /// A particle's damage owner (`0` if its class keeps none; falling and
    /// acts of god count as no owner).
    fn owner_word(&self, p: &Particle, c: &super::class::Class) -> u32 {
        if !c.has(cflag::OWNER) {
            return 0;
        }
        match p.owner & owner::TYPE_MASK {
            owner::FALLING | owner::ACT_OF_GOD => 0,
            _ => p.owner,
        }
    }

    fn random_unit_dir(&mut self) -> Vec3 {
        let v = Vec3::new(self.signed_random(), self.signed_random(), self.signed_random());
        v.normalize_or(Vec3::Y)
    }
}
