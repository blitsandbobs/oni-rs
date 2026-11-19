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
        "spark": { "flags": [], "lifetime": 0.5 }
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
