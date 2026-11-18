//! A level's AI data (`docs/ASSET_FORMAT.md`): its placed characters,
//! patrol paths and flags (from the level folder) and the combat profiles
//! (common). Behaviour that uses it: `docs/GOALS.md`.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use glam::Vec3;
use serde_json::Value;

use super::perception::{Alert, Team};

/// A patrol waypoint (`docs/GOALS.md`, *Patrol*).
#[derive(Clone, Debug, PartialEq)]
pub enum Waypoint {
    MoveTo { at: Target, stop: bool, face: bool, within: f32 },
    Stop,
    Pause(u32),
    LookAt(Target),
    StopLooking,
    LookInDirection,
    FreeFacing,
    /// Back to waypoint 0.
    Loop,
    /// To waypoint N (the next one if N is itself).
    LoopTo(usize),
    Pace(String),
    Glance { at: Target, ticks: u32 },
    Scan { ticks: u32, angle: f32 },
    StopScanning,
    Guard { flag: i16, ticks: Option<u32>, angle: f32 },
    Script { name: String, wait: bool },
    Lock(bool),
    ShootAt { at: Target, ticks: u32 },
}

/// Where a waypoint points: a flag (by id) or a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Flag(i16),
    Point(Vec3),
}

#[derive(Clone, Debug)]
pub struct PatrolPath {
    pub id: u16,
    pub name: String,
    pub return_to_nearest: bool,
    pub waypoints: Vec<Waypoint>,
}

#[derive(Clone, Copy, Debug)]
pub struct Flag {
    pub at: Vec3,
    pub facing: f32,
}

/// How a placed character fights (`combat_profiles.json`).
#[derive(Clone, Debug)]
pub struct CombatProfile {
    pub name: String,
    /// Per range band: long, medium, short, medium retreat, long retreat.
    pub behavior: [String; 5],
    pub short_range: f32,
    pub medium_range: f32,
    pub melee_when: String,
    pub no_gun: String,
    pub pursuit_distance: f32,
    /// How long a non-combatant panics (ticks) at a fight noise, gunfire,
    /// the sight of an enemy, being hurt.
    pub panic: [u32; 4],
}

impl Default for CombatProfile {
    fn default() -> Self {
        CombatProfile {
            name: "default".into(),
            behavior: std::array::from_fn(|_| "holdAndFire".to_string()),
            short_range: 40.0,
            medium_range: 200.0,
            melee_when: "ifShortRange".into(),
            no_gun: "melee".into(),
            pursuit_distance: 200.0,
            panic: [600, 900, 600, 1200],
        }
    }
}

/// What a placed character does about a contact it pursues.
#[derive(Clone, Debug)]
pub struct Pursue {
    pub strong_unseen: String,
    pub weak_unseen: String,
    pub strong_seen: String,
    pub weak_seen: String,
    pub lost: String,
}

impl Default for Pursue {
    fn default() -> Self {
        Pursue {
            strong_unseen: "move".into(),
            weak_unseen: "forget".into(),
            strong_seen: "hunt".into(),
            weak_seen: "look".into(),
            lost: "return".into(),
        }
    }
}

/// A character the level places.
#[derive(Clone, Debug)]
pub struct Placed {
    pub name: String,
    pub class: String,
    pub flags: Vec<String>,
    pub at: Vec3,
    pub facing: f32,
    pub team: Team,
    pub job: String,
    pub job_id: Option<u16>,
    pub combat_id: u16,
    pub melee_id: Option<u16>,
    pub alert: [Alert; 4],
    pub pursue: Pursue,
}

impl Placed {
    pub fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// Everything AI about a level.
#[derive(Clone, Debug, Default)]
pub struct LevelAi {
    pub characters: Vec<Placed>,
    pub patrols: HashMap<u16, Rc<PatrolPath>>,
    pub flags: HashMap<i16, Flag>,
    pub combat: HashMap<u16, Rc<CombatProfile>>,
}

fn read(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn v3(v: &Value) -> Vec3 {
    let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    Vec3::new(f(0), f(1), f(2))
}

fn alert(v: &Value) -> Alert {
    match v.as_str() {
        Some("low") => Alert::Low,
        Some("medium") => Alert::Medium,
        Some("high") => Alert::High,
        Some("combat") => Alert::Combat,
        _ => Alert::Lull,
    }
}

impl LevelAi {
    /// Loads a level's AI data (`level_dir`) and the common combat profiles
    /// (`common_dir`). Missing files leave their part empty.
    pub fn load(level_dir: &Path, common_dir: &Path) -> LevelAi {
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        let mut out = LevelAi::default();
        for c in read(&level_dir.join("characters.json"))["characters"].as_array().into_iter().flatten() {
            let a = &c["alert"];
            let p = &c["pursue"];
            out.characters.push(Placed {
                name: s(&c["name"]),
                class: s(&c["class"]),
                flags: c["flags"].as_array().into_iter().flatten().map(s).collect(),
                at: v3(&c["position"]),
                facing: c["facing"].as_f64().unwrap_or(0.0) as f32,
                team: Team::from_name(c["team"].as_str().unwrap_or("neutral")),
                job: s(&c["job"]),
                job_id: c["jobId"].as_u64().map(|v| v as u16),
                combat_id: c["combatProfileId"].as_u64().unwrap_or(0) as u16,
                melee_id: c["meleeProfileId"].as_u64().map(|v| v as u16),
                alert: [alert(&a["initial"]), alert(&a["minimum"]), alert(&a["job"]), alert(&a["investigate"])],
                pursue: Pursue {
                    strong_unseen: s(&p["strongUnseen"]),
                    weak_unseen: s(&p["weakUnseen"]),
                    strong_seen: s(&p["strongSeen"]),
                    weak_seen: s(&p["weakSeen"]),
                    lost: s(&p["lost"]),
                },
            });
        }
        for f in read(&level_dir.join("flags.json"))["flags"].as_array().into_iter().flatten() {
            out.flags.insert(
                f["id"].as_i64().unwrap_or(0) as i16,
                Flag { at: v3(&f["position"]), facing: f["facing"].as_f64().unwrap_or(0.0) as f32 },
            );
        }
        for p in read(&level_dir.join("patrols.json"))["patrols"].as_array().into_iter().flatten() {
            let path = PatrolPath {
                id: p["id"].as_u64().unwrap_or(0) as u16,
                name: s(&p["name"]),
                return_to_nearest: p["returnToNearest"].as_bool().unwrap_or(false),
                waypoints: p["waypoints"].as_array().into_iter().flatten().filter_map(waypoint).collect(),
            };
            out.patrols.insert(path.id, Rc::new(path));
        }
        for c in read(&common_dir.join("combat_profiles.json"))["profiles"].as_array().into_iter().flatten() {
            let b = &c["behavior"];
            let f = |v: &Value, d: f32| v.as_f64().map_or(d, |x| x as f32);
            let profile = CombatProfile {
                name: s(&c["name"]),
                behavior: [s(&b["long"]), s(&b["medium"]), s(&b["short"]), s(&b["mediumRetreat"]), s(&b["longRetreat"])],
                short_range: f(&c["shortRange"], 40.0),
                medium_range: f(&c["mediumRange"], 200.0),
                melee_when: s(&c["meleeWhen"]),
                no_gun: s(&c["noGun"]),
                pursuit_distance: f(&c["pursuitDistance"], 200.0),
                panic: {
                    let u = |k: &str, d: u32| c["panic"][k].as_u64().map_or(d, |v| v as u32);
                    [u("melee", 600), u("gunfire", 900), u("sight", 600), u("hurt", 1200)]
                },
            };
            out.combat.insert(c["id"].as_u64().unwrap_or(0) as u16, Rc::new(profile));
        }
        out
    }

    /// Where a waypoint target is (a flag's position, or the point).
    pub fn place(&self, t: Target) -> Option<Vec3> {
        match t {
            Target::Flag(id) => self.flags.get(&id).map(|f| f.at),
            Target::Point(p) => Some(p),
        }
    }
}

fn waypoint(w: &Value) -> Option<Waypoint> {
    let flag = || w["flag"].as_i64().map(|v| Target::Flag(v as i16));
    let point = || w["point"].is_array().then(|| Target::Point(v3(&w["point"])));
    let f = |k: &str| w[k].as_f64().unwrap_or(0.0) as f32;
    let ticks = || w["ticks"].as_u64().unwrap_or(0) as u32;
    Some(match w["type"].as_str()? {
        "moveToFlag" => Waypoint::MoveTo { at: flag()?, stop: true, face: false, within: 0.0 },
        "moveToPoint" => Waypoint::MoveTo { at: point()?, stop: true, face: false, within: 0.0 },
        "moveAndFaceFlag" => Waypoint::MoveTo { at: flag()?, stop: true, face: true, within: 0.0 },
        "moveThroughFlag" => Waypoint::MoveTo { at: flag()?, stop: false, face: false, within: f("distance") },
        "moveThroughPoint" => Waypoint::MoveTo { at: point()?, stop: false, face: false, within: f("distance") },
        "moveNearFlag" => Waypoint::MoveTo { at: flag()?, stop: true, face: false, within: f("distance") },
        "stop" => Waypoint::Stop,
        "pause" => Waypoint::Pause(ticks()),
        "lookAtFlag" => Waypoint::LookAt(flag()?),
        "lookAtPoint" => Waypoint::LookAt(point()?),
        "stopLooking" => Waypoint::StopLooking,
        "lookInDirection" => Waypoint::LookInDirection,
        "freeFacing" => Waypoint::FreeFacing,
        "loop" => Waypoint::Loop,
        "loopToWaypoint" => Waypoint::LoopTo(w["waypoint"].as_u64().unwrap_or(0) as usize),
        "movementMode" => Waypoint::Pace(w["mode"].as_str().unwrap_or("byAlertLevel").to_string()),
        "glanceAtFlag" => Waypoint::Glance { at: flag()?, ticks: ticks() },
        "scan" => Waypoint::Scan { ticks: ticks(), angle: f("angle") },
        "stopScanning" => Waypoint::StopScanning,
        "guard" => Waypoint::Guard {
            flag: w["flag"].as_i64()? as i16,
            ticks: w["ticks"].as_u64().map(|t| t as u32),
            angle: f("maxAngle"),
        },
        "triggerScript" => Waypoint::Script { name: w["script"].as_str()?.to_string(), wait: false },
        "playScript" => Waypoint::Script { name: w["script"].as_str()?.to_string(), wait: true },
        "lockIntoPatrol" => Waypoint::Lock(w["lock"].as_bool().unwrap_or(false)),
        "shootAtFlag" => Waypoint::ShootAt { at: flag()?, ticks: ticks() },
        _ => return None,
    })
}
