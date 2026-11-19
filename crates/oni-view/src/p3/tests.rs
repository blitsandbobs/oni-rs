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
}

impl Host for Fake {
    fn ray_env(&mut self, from: Vec3, ray: Vec3) -> Option<EnvHit> {
        let to = from + ray;
        (from.y > 0.0 && to.y <= 0.0).then(|| EnvHit { fraction: from.y / (from.y - to.y), normal: Vec3::Y, surface: 1, triangle: None })
    }
    fn ray_chars(&mut self, from: Vec3, ray: Vec3, radius: f32, _owner: Option<usize>) -> Option<CharHit> {
        let r = 5.0 + radius;
        let l = ray.length();
        if l < 1e-9 {
            return (from.distance(self.target) <= r).then_some(CharHit { fraction: 0.0, who: 7, normal: Vec3::Y });
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
        (0.0..=l).contains(&s).then(|| CharHit { fraction: s / l, who: 7, normal: (from + d * s - self.target).normalize() })
    }
    fn hit_char(&mut self, who: usize, _p: Vec3, _d: Vec3, kind: u32, damage: f32, _s: f32, _k: f32, _o: u32, stamp: Option<u64>, _si: bool) -> bool {
        self.hits.push((who, damage, kind, stamp));
        false
    }
    fn blast(&mut self, centre: Vec3, _k: u32, damage: f32, _s: f32, _kb: f32, radius: f32, _f: u32, _o: u32, _st: u64, _si: bool) {
        self.blasts.push((centre, damage, radius));
    }
    fn targets(&self) -> Vec<Target> {
        vec![Target { who: 7, position: self.target, velocity: Vec3::ZERO }]
    }
    fn hostile(&self, _me: usize, _other: usize) -> bool {
        true
    }
    fn clear(&mut self, _from: Vec3, _to: Vec3) -> bool {
        true
    }
    fn build_decal(&mut self, quad: Option<u32>, p: &crate::decal::Params) -> Option<crate::decal::Mesh> {
        let corners = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
        Some(crate::decal::Mesh {
            quad,
            positions: corners.iter().map(|(x, y)| p.position + p.right * (x * p.xscale) + p.up * (y * p.yscale)).collect(),
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
        "hole": {
            "flags2": ["decal"],
            "appearance": { "scale": 0.5, "alpha": 0.8, "instance": "_HOLE", "maxDecals": 3, "decalFadeFrames": 60 }
        },
        "burn": {
            "flags2": ["decal"],
            "lifetime": 0.5,
            "appearance": { "scale": 1.0, "alpha": 1.0, "instance": "_BURN", "maxDecals": 100, "decalFadeFrames": 30 }
        },
        "flash": { "flags": ["hasOrientation"] }
    } });
    Rc::new(Classes::from_json(&doc))
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
    let mut host = Fake { target: Vec3::new(0.0, 10.0, 100.0), ..Fake::default() };
    // The gun faces +Z (its forward axis, Y, turned onto Z).
    let gun = spawn(&mut sys, "gun", Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, &mut host);
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
    let mut host = Fake { target: Vec3::new(500.0, 0.0, 0.0), ..Fake::default() };
    let ball = spawn(&mut sys, "ball", Vec3::new(0.0, 50.0, 0.0), Vec3::new(10.0, 0.0, 0.0), &mut host);
    for tick in 1..=30 {
        sys.update(tick, &mut host);
    }
    let p = sys.get(ball).unwrap();
    assert!((p.velocity.y + 49.0).abs() < 0.5, "half a second of gravity: {}", p.velocity.y);
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
    assert!(sys.iter().filter(|(_, p)| p.link == Some(parent)).count() == 3, "linked to their emitter");
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
    let spec = Spec { class, orientation: Orientation::Unchanged, location: Location::Decal { is_static: false, random_rotation: true } };
    sys.create_effect(&spec, &data, host)
}

#[test]
fn decals_past_their_class_count_fade_oldest_first() {
    let mut sys = System::new(classes());
    let mut host = Fake::default();
    let holes: Vec<Handle> = (0..4).map(|_| hole(&mut sys, "hole", &mut host).unwrap()).collect();
    assert_eq!(sys.decals.dynamic.len(), 4);
    assert!(sys.decals.dynamic.iter().all(|d| d.alpha == 204), "0.8 of full");
    sys.update(1, &mut host);
    // One too many: the oldest starts fading, from its own opacity.
    let fading: Vec<bool> = holes.iter().map(|&h| sys.get(h).unwrap().flags & pflag::FADE_OUT != 0).collect();
    assert_eq!(fading, vec![true, false, false, false]);
    for tick in 2..=31 {
        sys.update(tick, &mut host);
    }
    let a = sys.decals.dynamic.iter().find(|d| d.particle == holes[0]).unwrap().alpha;
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
    assert_ne!(sys.get(h).unwrap().flags & pflag::FADE_OUT, 0, "half a second: fading");
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
    let spec = Spec { class, orientation: Orientation::Unchanged, location: Location::Decal { is_static: false, random_rotation: false } };
    assert!(sys.create_effect(&spec, &data, &mut host).is_none(), "no wall hit");
    let mut data = Data::new(0, Vec3::ZERO);
    data.cause = Cause::HitWall(None);
    data.normal = Vec3::Y;
    let spec = Spec { location: Location::Default, ..spec };
    assert!(sys.create_effect(&spec, &data, &mut host).is_none(), "not placed as a decal");
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
        let h = sys.create_effect(&Spec { class, orientation: o, location: Location::Default }, &data, host).unwrap();
        sys.get(h).unwrap().orientation.y_axis
    };
    // Along the normal, given a cause; without one, along the parent's flight.
    assert!(make(&mut sys, &mut host, Orientation::Normal, Cause::HitWall(None)).distance(Vec3::X) < 1e-5);
    let flight = Vec3::new(1.0, -1.0, 0.0).normalize();
    assert!(make(&mut sys, &mut host, Orientation::Normal, Cause::None).distance(flight) < 1e-5);
    // Reflected off the wall (x = 0 plane, normal +x): the x turns back.
    let r = make(&mut sys, &mut host, Orientation::Reflected, Cause::HitWall(None));
    assert!(r.distance(Vec3::new(-1.0, -1.0, 0.0).normalize()) < 1e-5, "{r}");
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
    assert_eq!(purged_all, vec![0], "the oldest goes when the ring comes round (not the one it touches)");
    // Small decals run out of block records first: 785 of them.
    let mut d = super::effect::Decals::default();
    let mut first = None;
    for k in 0..800u32 {
        let (_, purged) = d.allocate_for_test(100, k);
        if first.is_none() && !purged.is_empty() {
            first = Some((k, purged));
        }
    }
    assert_eq!(first, Some((785, vec![0])), "the 786th takes the first's record");
}
