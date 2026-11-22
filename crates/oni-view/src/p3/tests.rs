//! The particle system against a fake world: hand-written classes (not the
//! game's data).

use std::rc::Rc;

use glam::Vec3;
use serde_json::json;

use super::*;

/// A flat floor at y = 0 and one character: a sphere of radius 5 at
/// `target`.
#[derive(Default)]
struct Fake {
    target: Vec3,
    hits: Vec<(usize, f32, u32, Option<u64>)>,
    blasts: Vec<(Vec3, f32, f32)>,
    /// Glass broken near: centre, radius.
    glass: Vec<(Vec3, f32)>,
    /// A placed object: its sphere.
    object: Option<(Vec3, f32)>,
}

impl Host for Fake {
    fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit> {
        let to = from + ray;
        (from.y > 0.0 && to.y <= 0.0).then(|| EnvHit {
            fraction: from.y / (from.y - to.y),
            normal: Vec3::Y,
            surface: 1,
            quad: None,
        })
    }
    fn ray_chars(
        &mut self,
        from: Vec3,
        ray: Vec3,
        radius: f32,
        _owner: Option<usize>,
    ) -> Option<CharHit> {
        let r = 5.0 + radius;
        let l = ray.length();
        if l < 1e-9 {
            return (from.distance(self.target) <= r).then_some(CharHit {
                fraction: 0.0,
                who: 7,
                normal: Vec3::Y,
                part: 0,
            });
        }
        let d = ray / l;
        let oc = from - self.target;
        let b = oc.dot(d);
        let c = oc.length_squared() - r * r;
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let s = -b - disc.sqrt();
        (0.0..=l).contains(&s).then(|| CharHit {
            fraction: s / l,
            who: 7,
            normal: (from + d * s - self.target).normalize(),
            part: 0,
        })
    }
    fn ray_objects(&mut self, from: Vec3, ray: Vec3, radius: f32) -> Option<(f32, Vec3, u32)> {
        let (c, r) = self.object?;
        ray_sphere_entry(from, ray, c, r + radius).map(|(t, n)| (t, n, 0))
    }
    fn hit_char(
        &mut self,
        who: usize,
        _p: Vec3,
        _d: Vec3,
        kind: u32,
        damage: f32,
        _s: f32,
        _k: f32,
        _o: u32,
        stamp: Option<u64>,
        _si: bool,
    ) -> bool {
        self.hits.push((who, damage, kind, stamp));
        false
    }
    fn blast(
        &mut self,
        centre: Vec3,
        _k: u32,
        damage: f32,
        _s: f32,
        _kb: f32,
        radius: f32,
        _f: u32,
        _o: u32,
        _st: u64,
        _si: bool,
    ) {
        self.blasts.push((centre, damage, radius));
    }
    fn targets(&self) -> Vec<Target> {
        vec![Target {
            who: 7,
            position: self.target,
            velocity: Vec3::ZERO,
        }]
    }
    fn damage_surfaces_near(
        &mut self,
        centre: Vec3,
        radius: f32,
        _damage: f32,
        _blast: Option<super::glass::Blast>,
    ) {
        self.glass.push((centre, radius));
    }
    fn super_amount(&self, who: usize) -> f32 {
        if who == 3 {
            0.5
        } else {
            0.0
        }
    }
    fn hostile(&self, _me: usize, _other: usize) -> bool {
        true
    }
    fn clear(&mut self, _from: Vec3, _to: Vec3) -> bool {
        true
    }
    fn build_decal(
        &mut self,
        quad: Option<u32>,
        p: &crate::decal::Params,
    ) -> Option<crate::decal::Mesh> {
        let corners = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
        Some(crate::decal::Mesh {
            quad,
            positions: corners
                .iter()
                .map(|(x, y)| p.position + p.right * (x * p.xscale) + p.up * (y * p.yscale))
                .collect(),
            uvs: vec![[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            shades: vec![0xFFFF_FFFF; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        })
    }
}

fn classes() -> Rc<Classes> {
    let doc = json!({ "classes": {
        "gun": {
            "flags": ["hasOrientation", "hasOwner"],
            "actions": [ { "type": "emitParticles", "params": [ {"enum": 0}, 1.0 ] } ],
            "events": { "pulse": [0, 1] },
            "emitters": [ {
                "class": "bullet", "limit": 0, "chance": 65535, "burst": 1.0, "link": "none",
                "rate": { "type": "continuous", "values": [] },
                "position": { "type": "point", "values": [] },
                "direction": { "type": "straight", "values": [] },
                "speed": { "type": "uniform", "values": [600.0] },
                "orientDir": "velocity", "orientUp": "parentPosZ"
            } ]
        },
        "bullet": {
            "flags": ["collideEnv", "collideChar", "hasVelocity", "hasOrientation", "hasOwner"],
            "lifetime": 2.0,
            "actions": [
                { "type": "moveLine" },
                { "type": "damageHitChar", "params": [ {"range": [8.0, 10.0]}, 0.0, 1.0, {"enum": 0}, {"enum": 1} ] },
                { "type": "die" },
                { "type": "die" }
            ],
            "events": { "update": [0, 1], "collideChar": [1, 3], "collideEnv": [3, 4] }
        },
        "ball": {
            "flags": ["collideEnv", "hasVelocity"],
            "lifetime": 10.0,
            "actions": [
                { "type": "moveLine" },
                { "type": "moveGravity", "params": [1.0] },
                { "type": "damageBlast", "params": [30.0, 0.0, 5.0, 10.0, {"enum": 1}, {"enum": 5}, {"enum": 0}] },
                { "type": "die" }
            ],
            "events": { "update": [0, 2], "collideEnv": [2, 4] }
        },
        "shard": { "flags": ["hasVelocity", "hasOrientation"], "lifetime": 10.0 },
        "once": {
            "flags": [],
            "emitters": [ {
                "class": "spark", "flags": ["initiallyActive", "increaseEmitCount", "deactivateWhenEmitCountThreshold"],
                "limit": 1, "chance": 65535, "burst": 3.0, "link": "emitter",
                "rate": { "type": "instant", "values": [] },
                "position": { "type": "point", "values": [] },
                "direction": { "type": "straight", "values": [] },
                "speed": { "type": "uniform", "values": [0.0] },
                "orientDir": "parentPosY", "orientUp": "parentPosZ"
            } ]
        },
        "spark": { "flags": [], "lifetime": 0.5 },
        "rats": {
            "flags": [],
            "actions": [ { "type": "emitParticles", "params": [ {"enum": 0}, {"range": [4.0, 7.0]} ] } ],
            "events": { "pulse": [0, 1] },
            "emitters": [ {
                "class": "spark", "limit": 0, "chance": 65535, "burst": 1.0, "link": "none",
                "rate": { "type": "continuous", "values": [] },
                "position": { "type": "point", "values": [] },
                "direction": { "type": "straight", "values": [] },
                "speed": { "type": "uniform", "values": [0.0] },
                "orientDir": "parentPosY", "orientUp": "parentPosZ"
            } ]
        },
        "hole": {
            "flags2": ["decal"],
            "appearance": { "scale": 0.5, "alpha": 0.8, "instance": "_HOLE", "maxDecals": 3, "decalFadeFrames": 60 }
        },
        "burn": {
            "flags2": ["decal"],
            "lifetime": 0.5,
            "appearance": { "scale": 1.0, "alpha": 1.0, "instance": "_BURN", "maxDecals": 100, "decalFadeFrames": 30 }
        },
        "flash": { "flags": ["hasOrientation"] },
        "head": { "flags": [], "lifetime": 5.0 },
        "tail": {
            "flags": ["hasContrailData"], "spriteType": 6, "lifetime": 5.0,
            "actions": [ { "type": "die", "params": [] } ],
            "events": { "brokenLink": [0, 1] }
        }
    } });
    Rc::new(Classes::from_file(&serde_json::from_value(doc).unwrap()))
}

fn spawn(sys: &mut System, name: &str, at: Vec3, velocity: Vec3, host: &mut Fake) -> Handle {
    let id = sys.classes.get(name).unwrap();
    let mut p = sys.create(id, sys.tick);
    p.position = at;
    p.velocity = velocity;
    p.owner = owner::CHARACTER_WEAPON | 3;
    let t = f64::from(sys.tick) / HZ;
    sys.created(p, t, host).unwrap()
}

fn count(sys: &System, name: &str) -> usize {
    let id = sys.classes.get(name).unwrap();
    sys.iter().filter(|(_, p)| p.class == id).count()
}

#[test]
fn a_pulse_fires_a_bullet_that_hits_once_and_dies() {
    let mut sys = System::new(classes());
    let mut host = Fake {
        target: Vec3::new(0.0, 10.0, 100.0),
        ..Fake::default()
    };
    // The gun faces +Z (its forward axis, Y, turned onto Z).
    let gun = spawn(
        &mut sys,
        "gun",
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::ZERO,
        &mut host,
    );
    sys.get_mut(gun).unwrap().orientation = Mat3::from_cols(Vec3::X, Vec3::Z, -Vec3::Y);
    sys.send(gun, event::PULSE, 0.0, &mut host);
    assert_eq!(count(&sys, "bullet"), 1);
    // 95 units at 600/s: about 9.5 ticks.
    for tick in 1..=12 {
        sys.update(tick, &mut host);
    }
    assert_eq!(host.hits.len(), 1, "hit exactly once");
    let (who, damage, kind, stamp) = host.hits[0];
    assert_eq!((who, kind, stamp), (7, 0, None));
    assert!((8.0..=10.0).contains(&damage));
    assert_eq!(count(&sys, "bullet"), 0, "died on the hit");
}

#[test]
fn gravity_and_a_blast_where_it_lands() {
    let mut sys = System::new(classes());
    let mut host = Fake {
        target: Vec3::new(500.0, 0.0, 0.0),
        ..Fake::default()
    };
    let ball = spawn(
        &mut sys,
        "ball",
        Vec3::new(0.0, 50.0, 0.0),
        Vec3::new(10.0, 0.0, 0.0),
        &mut host,
    );
    for tick in 1..=30 {
        sys.update(tick, &mut host);
    }
    let p = sys.get(ball).unwrap();
    assert!(
        (p.velocity.y + 49.0).abs() < 0.5,
        "half a second of gravity: {}",
        p.velocity.y
    );
    for tick in 31..=120 {
        sys.update(tick, &mut host);
    }
    // Falls 50 units in ~1 s; blasts half a unit off the floor.
    assert_eq!(host.blasts.len(), 1);
    let (centre, damage, radius) = host.blasts[0];
    assert!((centre.y - 0.5).abs() < 1e-3);
    assert!((centre.x - 10.1).abs() < 0.5, "landed at x {}", centre.x);
    assert_eq!((damage, radius), (30.0, 10.0));
    assert!(!sys.alive(ball));
}

#[test]
fn an_instant_emitter_fires_one_burst() {
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let parent = spawn(&mut sys, "once", Vec3::ZERO, Vec3::ZERO, &mut host);
    for tick in 1..=5 {
        sys.update(tick, &mut host);
    }
    assert_eq!(count(&sys, "spark"), 3);
    assert!(
        sys.iter().filter(|(_, p)| p.link == Some(parent)).count() == 3,
        "linked to their emitter"
    );
    // Sparks live half a second.
    for tick in 6..=40 {
        sys.update(tick, &mut host);
    }
    assert_eq!(count(&sys, "spark"), 0);
    assert!(sys.alive(parent), "no lifetime: lives on");
}

/// A decal's effect at a wall hit (on a floor facing up).
fn hole(sys: &mut System, name: &str, host: &mut Fake) -> Option<Handle> {
    use super::effect::{Cause, Data, Location, Orientation, Spec};
    let class = sys.classes.get(name).unwrap();
    let mut data = Data::new(sys.tick, Vec3::ZERO);
    data.cause = Cause::HitWall(Some(0));
    data.normal = Vec3::Y;
    data.decal_scale = [1.0, 1.0];
    let spec = Spec {
        class,
        orientation: Orientation::Unchanged,
        location: Location::Decal {
            is_static: false,
            random_rotation: true,
        },
    };
    sys.create_effect(&spec, &data, host)
}

#[test]
fn a_random_emit_count_emits_its_low_end() {
    // (`P3iAction_EmitParticles` reads the count as a constant: a range's
    // low end, never a draw.)
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let rats = spawn(&mut sys, "rats", Vec3::ZERO, Vec3::ZERO, &mut host);
    sys.send(rats, event::PULSE, 0.0, &mut host);
    assert_eq!(count(&sys, "spark"), 4);
}

#[test]
fn decals_past_their_class_count_fade_oldest_first() {
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let holes: Vec<Handle> = (0..4)
        .map(|_| hole(&mut sys, "hole", &mut host).unwrap())
        .collect();
    assert_eq!(sys.decals.dynamic.len(), 4);
    assert!(
        sys.decals.dynamic.iter().all(|d| d.alpha == 204),
        "0.8 of full"
    );
    sys.update(1, &mut host);
    // One too many: the oldest starts fading, from its own opacity.
    let fading: Vec<bool> = holes
        .iter()
        .map(|&h| sys.get(h).unwrap().flags & pflag::FADE_OUT != 0)
        .collect();
    assert_eq!(fading, vec![true, false, false, false]);
    for tick in 2..=31 {
        sys.update(tick, &mut host);
    }
    let a = sys
        .decals
        .dynamic
        .iter()
        .find(|d| d.particle == holes[0])
        .unwrap()
        .alpha;
    assert!((95..=110).contains(&a), "halfway down from 204: {a}");
    for tick in 32..=62 {
        sys.update(tick, &mut host);
    }
    assert!(!sys.alive(holes[0]), "gone after its 60 frames");
    assert_eq!(sys.decals.dynamic.len(), 3, "its decal with it");
    assert!(holes[1..].iter().all(|&h| sys.alive(h)));
}

#[test]
fn a_decal_with_a_lifetime_fades_when_it_runs_out() {
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let h = hole(&mut sys, "burn", &mut host).unwrap();
    for tick in 1..=28 {
        sys.update(tick, &mut host);
    }
    assert_eq!(sys.get(h).unwrap().flags & pflag::FADE_OUT, 0);
    for tick in 29..=30 {
        sys.update(tick, &mut host);
    }
    // (Under a frame left counts as none.)
    assert_ne!(
        sys.get(h).unwrap().flags & pflag::FADE_OUT,
        0,
        "half a second: fading"
    );
    for tick in 32..=62 {
        sys.update(tick, &mut host);
    }
    assert!(!sys.alive(h) && sys.decals.dynamic.is_empty());
}

#[test]
fn a_decal_needs_a_wall_and_a_decal_place() {
    use super::effect::{Cause, Data, Location, Orientation, Spec};
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let class = sys.classes.get("hole").unwrap();
    let data = Data::new(0, Vec3::ZERO);
    let spec = Spec {
        class,
        orientation: Orientation::Unchanged,
        location: Location::Decal {
            is_static: false,
            random_rotation: false,
        },
    };
    assert!(
        sys.create_effect(&spec, &data, &mut host).is_none(),
        "no wall hit"
    );
    let mut data = Data::new(0, Vec3::ZERO);
    data.cause = Cause::HitWall(None);
    data.normal = Vec3::Y;
    let spec = Spec {
        location: Location::Default,
        ..spec
    };
    assert!(
        sys.create_effect(&spec, &data, &mut host).is_none(),
        "not placed as a decal"
    );
    assert!(sys.decals.dynamic.is_empty());
}

#[test]
fn effect_orientations_fall_back_without_a_cause() {
    use super::effect::{Cause, Data, Location, Orientation, Spec};
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let class = sys.classes.get("flash").unwrap();
    let make = |sys: &mut System, host: &mut Fake, o: Orientation, cause: Cause| {
        let mut data = Data::new(0, Vec3::ZERO);
        data.cause = cause;
        data.normal = Vec3::X;
        data.parent_velocity = Some(Vec3::new(1.0, -1.0, 0.0));
        let h = sys
            .create_effect(
                &Spec {
                    class,
                    orientation: o,
                    location: Location::Default,
                },
                &data,
                host,
            )
            .unwrap();
        sys.get(h).unwrap().orientation.y_axis
    };
    // Along the normal, given a cause; without one, along the parent's flight.
    assert!(
        make(
            &mut sys,
            &mut host,
            Orientation::Normal,
            Cause::HitWall(None)
        )
        .distance(Vec3::X)
            < 1e-5
    );
    let flight = Vec3::new(1.0, -1.0, 0.0).normalize();
    assert!(make(&mut sys, &mut host, Orientation::Normal, Cause::None).distance(flight) < 1e-5);
    // Reflected off the wall (x = 0 plane, normal +x): the x turns back.
    let r = make(
        &mut sys,
        &mut host,
        Orientation::Reflected,
        Cause::HitWall(None),
    );
    assert!(
        r.distance(Vec3::new(-1.0, -1.0, 0.0).normalize()) < 1e-5,
        "{r}"
    );
    // Reversed: back along the flight.
    assert!(make(&mut sys, &mut host, Orientation::Reversed, Cause::None).distance(-flight) < 1e-5);
}

#[test]
fn the_dynamic_decals_memory_throws_out_the_oldest() {
    let mut d = super::effect::Decals::default();
    let mut owners = Vec::new();
    let mut purged_all = Vec::new();
    // 132-byte decals (a flat quad): 775 fit; the 776th wraps round and
    // throws out the first.
    for k in 0..776u32 {
        let (_, purged) = d.allocate_for_test(132, k);
        purged_all.extend(purged);
        owners.push(k);
    }
    assert_eq!(
        purged_all,
        vec![0],
        "the oldest goes when the ring comes round (not the one it touches)"
    );
    // Small decals run out of block records first: 785 of them.
    let mut d = super::effect::Decals::default();
    let mut first = None;
    for k in 0..800u32 {
        let (_, purged) = d.allocate_for_test(100, k);
        if first.is_none() && !purged.is_empty() {
            first = Some((k, purged));
        }
    }
    assert_eq!(
        first,
        Some((785, vec![0])),
        "the 786th takes the first's record"
    );
}

/// Classes for the motion and texture actions.
fn motion_classes() -> Rc<Classes> {
    let doc = json!({ "classes": {
        // Up (world Y) to a top speed of 10, the sideways motion dying away.
        "drift": {
            "flags": ["hasVelocity"],
            "actions": [
                { "type": "moveDrift", "params": [2.0, 10.0, 1.0, 0.0, 1.0, 0.0, {"enum": 1}] },
                { "type": "moveLine" }
            ],
            "events": { "update": [0, 2] }
        },
        "spiral": {
            "flags": ["hasOrientation", "hasOffsetPos"],
            "variables": [ { "name": "theta", "type": "float", "init": 90.0 } ],
            "actions": [ { "type": "moveSpiral", "vars": ["theta"], "params": [10.0, 0.0] } ],
            "events": { "update": [0, 1] }
        },
        "tangent": {
            "flags": ["hasVelocity", "hasOrientation"],
            "actions": [ { "type": "spiralTangent", "params": [90.0, 10.0, 180.0] } ],
            "events": { "create": [0, 1] }
        },
        // Up at 20; dies past y = 8.
        "riser": {
            "flags": ["hasVelocity"],
            "actions": [ { "type": "moveLine" }, { "type": "killBeyondPoint", "params": [{"enum": 2}, 8.0] } ],
            "events": { "update": [0, 2] }
        },
        "charge": {
            "flags": [],
            "actions": [ { "type": "glassCharge", "params": [0.0, 15.0] } ],
            "events": { "create": [0, 1] }
        },
        "flash": {
            "flags": ["hasTextureTime"],
            "actions": [ { "type": "setTextureTick", "params": [5.0] } ],
            "events": { "update": [0, 1] }
        },
        "super": {
            "flags": ["hasOwner"],
            "variables": [ { "name": "size", "type": "float", "init": 0.0 } ],
            "actions": [ { "type": "superParticle", "vars": ["size"], "params": [1.0, 2.0, 0.0, 10.0] } ],
            "events": { "create": [0, 1] }
        }
    } });
    Rc::new(Classes::from_file(&serde_json::from_value(doc).unwrap()))
}

#[test]
fn motion_and_texture_actions_follow_the_original() {
    let mut sys = System::new(motion_classes());
    let mut host = Fake::default();
    let drift = spawn(
        &mut sys,
        "drift",
        Vec3::new(0.0, 50.0, 0.0),
        Vec3::new(5.0, 0.0, 0.0),
        &mut host,
    );
    let spiral = spawn(&mut sys, "spiral", Vec3::ZERO, Vec3::ZERO, &mut host);
    let tangent = spawn(&mut sys, "tangent", Vec3::ZERO, Vec3::ZERO, &mut host);
    let riser = spawn(
        &mut sys,
        "riser",
        Vec3::ZERO,
        Vec3::new(0.0, 20.0, 0.0),
        &mut host,
    );
    spawn(
        &mut sys,
        "charge",
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::ZERO,
        &mut host,
    );
    let flash = spawn(&mut sys, "flash", Vec3::ZERO, Vec3::ZERO, &mut host);
    let sup = spawn(&mut sys, "super", Vec3::ZERO, Vec3::ZERO, &mut host);

    // Sent along the spiral's tangent once: rate × radius = π × 10, across
    // the angle (90°: along −X).
    let v = sys.get(tangent).unwrap().velocity;
    assert!(
        (v.x + std::f32::consts::PI * 10.0).abs() < 1e-3 && v.z.abs() < 1e-3,
        "{v}"
    );
    // The glass charge bursts where it is, at its radius.
    assert_eq!(host.glass, [(Vec3::new(1.0, 2.0, 3.0), 15.0)]);
    // Super: base + delta × the owner's super amount (0.5).
    assert_eq!(sys.get(sup).unwrap().vars[0].f(), Some(2.0));

    for tick in 1..=20 {
        sys.update(tick, &mut host);
    }
    assert!(
        sys.alive(riser),
        "short of its point (y {})",
        sys.get(riser).map_or(0.0, |p| p.position.y)
    );
    for tick in 21..=30 {
        sys.update(tick, &mut host);
    }
    // Past y = 8 (at 20 a second: 0.4 s), gone.
    assert!(!sys.alive(riser), "died beyond its point");
    // The spiral's offset: radius 10 at 90° — on +Z, as shipped.
    let o = sys.get(spiral).unwrap().offset;
    assert!((o - Vec3::new(0.0, 0.0, 10.0)).length() < 1e-4, "{o}");
    // The texture tick: its animation at tick 5 when the step began (the
    // update's time, as in the original), one tick on as drawn.
    let f = sys.get(flash).unwrap();
    assert_eq!(sys.tick.wrapping_sub(f.texture_time), 6);

    for tick in 31..=180 {
        sys.update(tick, &mut host);
    }
    // Three seconds of drift: up towards 10 (10 × (1 − e⁻⁶)), the sideways
    // 5 dying to 5 e⁻³.
    let v = sys.get(drift).unwrap().velocity;
    assert!((v.y - 10.0 * (1.0 - (-6.0f32).exp())).abs() < 0.01, "{v}");
    assert!((v.x - 5.0 * (-3.0f32).exp()).abs() < 0.01, "{v}");
}

#[test]
fn broken_glass_bursts_into_shards_over_the_pane_blown_away_from_a_blast() {
    use super::glass::{Blast, Pane};
    // A 40 × 40 pane upright in z = 0, facing +z.
    let pane = Pane {
        corners: [
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(40.0, 10.0, 0.0),
            Vec3::new(40.0, 50.0, 0.0),
            Vec3::new(0.0, 50.0, 0.0),
        ],
        normal: Vec3::Z,
        d: 0.0,
    };
    let shards = |blast: Option<Blast>| {
        let mut sys = System::new(classes());
        let class = sys.classes.get("shard").unwrap();
        sys.break_glass(class, &pane, 10.0, blast, 2, &mut Fake::default());
        sys.iter()
            .map(|(_, p)| (p.position, p.velocity))
            .collect::<Vec<_>>()
    };
    // Shot: 3 × 3 cells of 15, 5 shards each, over the pane, barely moving.
    let shot = shards(None);
    assert_eq!(shot.len(), 45);
    assert!(shot.iter().all(|(at, v)| (0.0..=40.0).contains(&at.x)
        && (10.0..=50.0).contains(&at.y)
        && at.z == 0.0
        && v.length() <= 10.0 + 1e-4));
    // A blast behind the pane (−z) blows the shards out to +z, one in front
    // back to −z.
    let mean_z = |s: &[(Vec3, Vec3)]| s.iter().map(|(_, v)| v.z).sum::<f32>() / s.len() as f32;
    let behind = shards(Some(Blast {
        centre: Vec3::new(20.0, 30.0, -10.0),
        dir: None,
        radius: 20.0,
    }));
    assert!(mean_z(&behind) > 20.0, "{}", mean_z(&behind));
    let front = shards(Some(Blast {
        centre: Vec3::new(20.0, 30.0, 10.0),
        dir: None,
        radius: 20.0,
    }));
    assert!(mean_z(&front) < -20.0, "{}", mean_z(&front));
}

#[test]
fn temporary_particles_go_together() {
    use super::effect::{Data, Location, Orientation, Spec};
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let class = sys.classes.get("shard").unwrap();
    let spec = Spec {
        class,
        orientation: Orientation::Unchanged,
        location: Location::Default,
    };
    sys.temporary = true;
    sys.create_effect(&spec, &Data::new(0, Vec3::ZERO), &mut host)
        .unwrap();
    sys.temporary = false;
    let kept = sys
        .create_effect(&spec, &Data::new(0, Vec3::ZERO), &mut host)
        .unwrap();
    sys.kill_temporary(&mut host);
    assert_eq!(sys.iter().count(), 1);
    assert!(sys.alive(kept), "made outside the mode, it stays");
}

#[test]
fn a_contrail_whose_link_dies_hears_of_it() {
    // (`P3rDisplayClass_Contrail`: a link gone breaks, sending BrokenLink —
    // here the tail's dies.)
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let head = spawn(&mut sys, "head", Vec3::ZERO, Vec3::ZERO, &mut host);
    let tail = spawn(&mut sys, "tail", Vec3::X, Vec3::ZERO, &mut host);
    sys.get_mut(tail).unwrap().link = Some(head);
    sys.update(1, &mut host);
    assert_eq!(count(&sys, "tail"), 1, "linked, it lives");
    sys.kill(head, &mut host);
    sys.update(2, &mut host);
    assert_eq!(count(&sys, "tail"), 0, "its link gone, it dies");
}

#[test]
fn a_shot_stops_at_a_placed_object_and_harms_no_one() {
    // (`P3iCheckCollisions`: the other physics in the way — a placed
    // object's sphere — is a hit like a character's, `CollideChar`; the
    // bullet dies there, its damage reaching no one.)
    let mut sys = System::new(classes());
    let mut host = Fake {
        target: Vec3::new(0.0, 10.0, 300.0),
        object: Some((Vec3::new(0.0, 10.0, 60.0), 8.0)),
        ..Fake::default()
    };
    let gun = spawn(
        &mut sys,
        "gun",
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::ZERO,
        &mut host,
    );
    sys.get_mut(gun).unwrap().orientation = Mat3::from_cols(Vec3::X, Vec3::Z, -Vec3::Y);
    sys.send(gun, event::PULSE, 0.0, &mut host);
    let mut last_z = 0.0f32;
    for tick in 1..=12 {
        sys.update(tick, &mut host);
        for (_, p) in sys.iter() {
            last_z = last_z.max(p.position.z);
        }
    }
    assert_eq!(count(&sys, "bullet"), 0, "it died on the object");
    assert!(host.hits.is_empty(), "harming no one");
    assert!(last_z <= 53.0, "not past the object's front: {last_z}");
}
