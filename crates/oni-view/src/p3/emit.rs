//! Emitting particles (`docs/PARTICLES.md`, *Emitters*): where a burst's
//! particles start, which way they fly and face, what they inherit.

use std::f32::consts::TAU;

use glam::{Mat3, Vec3};

use super::class::{self, cflag, eflag, Direction, Link, Orient, Position};
use super::{Handle, Host, Particle, System, HZ};

/// An orthonormal frame with `dir` as forward (Y) and `up` as the up hint
/// (Z); falls back to world up, then world Z, when they are parallel.
pub(super) fn basis(dir: Vec3, up: Vec3) -> Mat3 {
    let Some(d) = dir.try_normalize() else {
        return Mat3::IDENTITY;
    };
    let right = d
        .cross(up)
        .try_normalize()
        .or_else(|| d.cross(Vec3::Y).try_normalize())
        .or_else(|| d.cross(Vec3::Z).try_normalize())
        .unwrap_or(Vec3::X);
    let up = right.cross(d).normalize();
    Mat3::from_cols(right, d, up)
}

/// An emitted particle's orientation (`P3rConstructOrientation`): its
/// forward (Y) from `dir_o`, its up (Z) as near `up_o` as square to it.
/// `parent`: the emitter's orientation; `offset`: where the particle starts
/// from the emitter; `vel`: the way it is sent.
fn construct_orientation(
    dir_o: Orient,
    up_o: Orient,
    parent: Option<Mat3>,
    offset: Vec3,
    vel: Vec3,
) -> Mat3 {
    let p = parent.unwrap_or(Mat3::IDENTITY);
    let axis = |world: bool, axis: usize, negative: bool| {
        let v = if world { Mat3::IDENTITY } else { p }.col(axis);
        if negative {
            -v
        } else {
            v
        }
    };
    // Up along the very axis it faces (the same space): the next axis's +
    // instead (X → Y, Y → Z, Z → X; the original's table).
    let up_o = match (dir_o, up_o) {
        (
            Orient::Axis {
                world: w1,
                axis: a1,
                ..
            },
            Orient::Axis {
                world: w2,
                axis: a2,
                ..
            },
        ) if w1 == w2 && a1 == a2 => Orient::Axis {
            world: w1,
            axis: (a1 + 1) % 3,
            negative: false,
        },
        _ => up_o,
    };
    // Its forward: a missing velocity or offset falls back to the parent's
    // Y or Z (unturned).
    let dir = match dir_o {
        Orient::Axis {
            world,
            axis: a,
            negative,
        } => axis(world, a, negative),
        Orient::Velocity => vel.try_normalize().unwrap_or(p.y_axis),
        Orient::ReverseVelocity => vel.try_normalize().map_or(p.y_axis, |v| -v),
        Orient::TowardsEmitter => offset.try_normalize().map_or(p.z_axis, |v| -v),
        Orient::AwayFromEmitter => offset.try_normalize().unwrap_or(p.z_axis),
    };
    // Its up hint — none for a missing one (then world Y).
    let up = match up_o {
        Orient::Axis {
            world,
            axis: a,
            negative,
        } => axis(world, a, negative),
        Orient::Velocity => vel,
        Orient::ReverseVelocity => -vel,
        Orient::TowardsEmitter => -offset,
        Orient::AwayFromEmitter => offset,
    };
    let right = match dir.cross(up).try_normalize() {
        Some(r) => r,
        // Aligned: world Y as the up; facing straight up or down, +Z as the
        // right.
        None => match Vec3::new(-dir.z, 0.0, dir.x).try_normalize() {
            Some(r) => r,
            None if dir.y.abs() < 1e-12 => return Mat3::IDENTITY,
            None => Vec3::Z,
        },
    };
    let up = right.cross(dir).normalize_or_zero();
    Mat3::from_cols(right, dir, up)
}

impl System {
    /// A random unit vector.
    pub(super) fn random_unit(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(
                self.signed_random(),
                self.signed_random(),
                self.signed_random(),
            );
            let l = v.length_squared();
            if l > 1e-4 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }

    /// A direction in a cone of `angle` degrees around +Y; `bias` > 0
    /// pulls it toward the centre. Returns it and the random fraction used.
    fn cone(&mut self, angle: f32, bias: f32, theta: Option<f32>) -> (Vec3, f32) {
        let mut c = self.random();
        if bias > 0.0 {
            c = c.powf(bias + 1.0);
        } else if bias < 0.0 {
            c = c.powf(1.0 / (1.0 - bias));
        }
        let phi = (c * angle).to_radians();
        let theta = theta.unwrap_or_else(|| TAU * self.random());
        (
            Vec3::new(theta.cos() * phi.sin(), phi.cos(), theta.sin() * phi.sin()),
            c,
        )
    }

    /// Emits `bursts` bursts from emitter `e` of `parent` at time `t`.
    pub(super) fn emit(
        &mut self,
        parent: &mut Particle,
        ph: Handle,
        e: usize,
        t: f64,
        bursts: u32,
        host: &mut dyn Host,
    ) {
        for _ in 0..bursts {
            self.emit_burst(parent, ph, e, t, host);
        }
    }

    fn emit_burst(
        &mut self,
        parent: &mut Particle,
        ph: Handle,
        e: usize,
        t: f64,
        host: &mut dyn Host,
    ) {
        let classes = self.classes.clone();
        let pc = &classes.list[parent.class];
        let def = &pc.emitters[e];
        let Some(child_class) = def.class else { return };
        let cc = &classes.list[child_class];
        if parent.flags & super::pflag::CUTSCENE_KILLED != 0
            && cc.has(cflag::DIE_ON_CUTSCENE | cflag::EXPIRE_ON_CUTSCENE)
        {
            return;
        }
        if def.chance < u16::MAX && self.rand16() >= def.chance {
            return;
        }
        let mut count = def.burst.floor() as usize;
        if self.random() < def.burst.fract() {
            count += 1;
        }
        // Emitting toward attractors: the parent's candidates.
        let wants_targets = def.direction.0 == Direction::Attractor
            || cc
                .attractor
                .as_ref()
                .is_some_and(|a| a.iterator == class::Iterator::EmittedTowards);
        let mut targets = Vec::new();
        if wants_targets {
            targets = self
                .attractor_candidates(parent, host)
                .into_iter()
                .map(|x| x.0)
                .take(32)
                .collect();
            if def.flags & eflag::ONE_PER_ATTRACTOR != 0 {
                let n = if def.flags & eflag::AT_LEAST_ONE != 0 {
                    targets.len().max(1)
                } else {
                    targets.len()
                };
                count *= n;
            }
        }
        let per = count.checked_div(targets.len().max(1)).unwrap_or(1).max(1);

        let parent_has_orientation = pc.has(cflag::ORIENTATION);
        let parent_orient = parent_has_orientation.then_some(parent.orientation);
        // A parent riding a matrix: a child that rides matrices shares it
        // (staying in its space); any other child is baked into the world.
        let applied = match (&parent.matrix, cc.has(cflag::DYNAMIC_MATRIX)) {
            (Some(m), false) => Some(m.get()),
            _ => None,
        };
        let shared = if cc.has(cflag::DYNAMIC_MATRIX) {
            parent.matrix.clone()
        } else {
            None
        };
        let m3 = applied.map(glam::Mat3::from_mat4);
        let tick = (t * HZ).round() as u32;
        let vars = parent.vars.clone();

        for i in 0..count {
            let mut char_value: Option<f32> = None;
            let (pos_kind, pv) = &def.position;
            let pf = |s: &mut System, k: usize| s.eval_f(&pv[k], &vars).unwrap_or(0.0);
            // On the owner's body (`P3cEmitterPosition_BodySurface` /
            // `BodyBones`): the part's matrix, the point in its space, its
            // orientation there; none (the emitter's own point) when the
            // owner is no character.
            let mut body = None;
            if matches!(pos_kind, Position::BodySurface | Position::BodyBones) {
                let who = pc
                    .has(cflag::OWNER)
                    .then(|| super::owner::character(parent.owner))
                    .flatten();
                if let (Some(who), Some(radius)) = (who, self.eval_f(&pv[0], &vars)) {
                    let r = [self.random(), self.random(), self.random()];
                    body = host.body_point(who, *pos_kind == Position::BodySurface, radius, r);
                }
            }
            let offset = match pos_kind {
                Position::Point | Position::BodySurface | Position::BodyBones => Vec3::ZERO,
                Position::Line => {
                    let s = self.signed_random();
                    char_value = Some(s.abs());
                    Vec3::new(pf(self, 0) * s, 0.0, 0.0)
                }
                Position::Circle | Position::Cylinder => {
                    let (h, (a, b)) = if *pos_kind == Position::Cylinder {
                        (pf(self, 0), (pf(self, 1), pf(self, 2)))
                    } else {
                        (0.0, (pf(self, 0), pf(self, 1)))
                    };
                    let c = self.random();
                    char_value = Some(c);
                    let r = a + (b - a) * c;
                    let th = TAU * self.random();
                    Vec3::new(r * th.cos(), h * self.random(), r * th.sin())
                }
                Position::Sphere => {
                    let (a, b) = (pf(self, 0), pf(self, 1));
                    let c = self.random();
                    char_value = Some(c);
                    self.random_unit() * (a + (b - a) * c)
                }
                Position::Offset => Vec3::new(pf(self, 0), pf(self, 1), pf(self, 2)),
            };
            let (dir_kind, dv) = &def.direction;
            let df = |s: &mut System, k: usize| s.eval_f(&dv[k], &vars).unwrap_or(0.0);
            let target = if targets.is_empty() {
                None
            } else if def.flags & eflag::ONE_PER_ATTRACTOR != 0 {
                targets.get(i / per).copied()
            } else {
                let k = if def.flags & eflag::ROTATE_ATTRACTORS != 0 {
                    i + parent.emitters[e].count as usize
                } else {
                    i
                };
                targets.get(k % targets.len()).copied()
            };
            // (World space for attractor directions.)
            let mut world_dir = false;
            let dir = match dir_kind {
                Direction::Straight => Vec3::Y,
                Direction::Random => self.random_unit(),
                Direction::Cone => {
                    let (a, b) = (df(self, 0), df(self, 1));
                    let (d, c) = self.cone(a, b, None);
                    char_value = Some(c);
                    d
                }
                // On the cone's edge — at its angle exactly, not within it —
                // a burst spread evenly round from the offset.
                Direction::Ring => {
                    let (phi, b) = (df(self, 0).to_radians(), df(self, 1));
                    let th = b.to_radians() + i as f32 * TAU / count.max(1) as f32;
                    Vec3::new(th.cos() * phi.sin(), phi.cos(), th.sin() * phi.sin())
                }
                Direction::Offset => Vec3::new(df(self, 0), df(self, 1), df(self, 2)),
                Direction::Inaccurate => {
                    let (base, inacc, bias) = (df(self, 0), df(self, 1), df(self, 2));
                    let angle = base + inacc * host.inaccuracy(parent.owner);
                    let (d, c) = self.cone(angle, bias, None);
                    char_value = Some(c);
                    d
                }
                Direction::Attractor => {
                    let from = self.world_position(parent);
                    match target.and_then(|w| host.targets().into_iter().find(|t| t.who == w)) {
                        Some(tg) => {
                            world_dir = true;
                            (tg.position - from).normalize_or(Vec3::Y)
                        }
                        None => Vec3::Y,
                    }
                }
            };
            let (stratified, sv) = &def.speed;
            let v1 = self.eval_f(&sv[0], &vars).unwrap_or(0.0);
            let speed = if *stratified {
                let v2 = self.eval_f(&sv[1], &vars).unwrap_or(0.0);
                v1 + char_value.unwrap_or(0.0) * (v2 - v1)
            } else {
                v1
            };

            let mut child = self.create(child_class, tick);
            // The parent's colour, into the child's tint variable.
            if def.flags & eflag::INHERIT_TINT != 0 {
                let classes = self.classes.clone();
                if let class::Value::Var(k) = classes.list[child_class].tint {
                    if let Some(t) = self.eval(&classes.list[parent.class].tint, &parent.vars) {
                        if let Some(v) = child.vars.get_mut(k) {
                            *v = t;
                        }
                    }
                }
            }
            if def.flags & eflag::ORIENT_TO_VELOCITY != 0 {
                child.flags |= super::pflag::ORIENT_TO_VELOCITY;
            }
            child.flags |= parent.flags & (super::pflag::TEMPORARY | super::pflag::CUTSCENE_KILLED);
            child.link = match def.link {
                Link::None => None,
                Link::Emitter => Some(ph),
                Link::Inherited => parent.link,
                Link::LastEmission(k) => parent.emitters.get(k).and_then(|s| s.last),
            };
            child.owner = if pc.has(cflag::OWNER) {
                parent.owner
            } else {
                0
            };
            child.matrix = shared.clone();

            // Position: the parent's (offset included), the emit offset
            // turned by its orientation; both through a baked matrix. On a
            // body, the point itself: a child that rides matrices rides the
            // part's; any other is put in the world through it.
            let rot = parent_orient.unwrap_or(Mat3::IDENTITY);
            let mut m3 = m3;
            let mut off = rot * offset;
            if let Some((cell, at, _)) = &body {
                if cc.has(cflag::DYNAMIC_MATRIX) {
                    child.matrix = Some(cell.clone());
                    m3 = None;
                    child.position = *at;
                } else {
                    let m = cell.get();
                    m3 = Some(glam::Mat3::from_mat4(m));
                    child.matrix = None;
                    child.position = m.transform_point3(*at);
                }
                child.offset = Vec3::ZERO;
                off = Vec3::ZERO;
            } else {
                let mut base = parent.position + parent.offset;
                if let Some(m) = applied {
                    base = m.transform_point3(base);
                    off = m3.unwrap() * off;
                }
                if cc.has(cflag::OFFSET_POS) {
                    child.position = base;
                    child.offset = off;
                } else {
                    child.position = base + off;
                }
            }
            // Velocity.
            let mut d = dir;
            if !world_dir {
                d = rot * d;
                if let Some(m) = m3 {
                    d = m * d;
                }
            }
            if cc.has(cflag::VELOCITY) {
                child.velocity = d * speed;
                if def.flags & eflag::ADD_PARENT_VELOCITY != 0 {
                    child.velocity += parent.velocity;
                }
            }
            // Orientation: on the body's surface, the way out there (through
            // the part's matrix unless riding it); else built from the kinds.
            if cc.has(cflag::ORIENTATION) {
                child.orientation = match body.as_ref().and_then(|b| b.2) {
                    Some(o) => m3.map_or(o, |m| m * o),
                    None => {
                        let po = parent_orient.map(|o| m3.map_or(o, |m| m * o));
                        construct_orientation(def.orient_dir, def.orient_up, po, off, d)
                    }
                };
            }
            // An attractor handed down by the emitter.
            if let Some(a) = cc.attractor.as_ref() {
                match a.iterator {
                    class::Iterator::EmittedTowards => child.attractor = target,
                    class::Iterator::Parent => child.attractor = parent.attractor,
                    _ => {}
                }
                if child.attractor.is_some() {
                    child.flags |= super::pflag::HAS_ATTRACTOR;
                    child.attractor_check = tick + (self.random() * 10.0) as u32;
                }
            }
            let h = self.created(child, t, host);
            parent.emitters[e].last = h;
            if h.is_none() {
                return;
            }
        }
    }
}
