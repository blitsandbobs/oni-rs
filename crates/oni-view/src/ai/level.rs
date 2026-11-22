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
    MoveTo {
        at: Target,
        stop: bool,
        face: bool,
        within: f32,
    },
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
    Glance {
        at: Target,
        ticks: u32,
    },
    Scan {
        ticks: u32,
        angle: f32,
    },
    StopScanning,
    Guard {
        flag: i16,
        ticks: Option<u32>,
        angle: f32,
    },
    Script {
        name: String,
        wait: bool,
    },
    Lock(bool),
    /// Shoot at a spot for a while, with this much extra spread (degrees).
    ShootAt {
        at: Target,
        ticks: u32,
        inaccuracy: f32,
    },
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
    pub alarm: AlarmSettings,
}

/// When an AI goes for an alarm, what draws it into a fight on the way.
#[derive(Clone, Copy, Debug)]
pub struct AlarmSettings {
    /// Alarm consoles further than this (by path) aren't looked for.
    pub find_distance: f32,
    /// Enemies further than this are ignored…
    pub ignore_distance: f32,
    /// …nearer than this are attacked (or if they've done this much harm).
    pub chase_distance: f32,
    pub damage_threshold: i32,
    /// After breaking off to fight, back to the alarm after this (0: never).
    pub fight_ticks: u32,
    pub attack_if_knocked_down: bool,
}

impl Default for AlarmSettings {
    fn default() -> Self {
        AlarmSettings {
            find_distance: 300.0,
            ignore_distance: 80.0,
            chase_distance: 0.0,
            damage_threshold: 30,
            fight_ticks: 360,
            attack_if_knocked_down: false,
        }
    }
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
            alarm: AlarmSettings::default(),
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
    /// Its neutral behaviour (`docs/NEUTRAL.md`).
    pub neutral_id: Option<u16>,
    /// Added to its class's hit points.
    pub hp_offset: i32,
    /// The alarm groups it answers (bits).
    pub alarm_groups: u32,
    pub alert: [Alert; 4],
    pub pursue: Pursue,
    /// Its event scripts (`spawn`, `die`, `combat`, `hurt`, `alarm`, …).
    pub scripts: HashMap<String, String>,
    /// The gun it starts with (its class name), how full (percent), and
    /// what it carries besides: clips, cells, hypos, shield, invisibility.
    pub weapon: Option<String>,
    pub ammo_percent: Option<u32>,
    pub carries: [u16; 5],
    /// What it drops dying ("death candy", `drop_*`): clips, cells and hypos
    /// by count; a shield or invisibility if non-zero.
    pub drops: [u16; 5],
}

/// A character setup (`AItCharacterSetup`, `setups.json`): what
/// `chr_create` makes — for the cutscenes — at its flag, known to the scripts
/// by its script id.
#[derive(Clone, Debug)]
pub struct Setup {
    pub script_id: u16,
    /// Its name (none: `ai_<n>`, as the original names it).
    pub name: Option<String>,
    pub flag: i16,
    /// Its class (none: the first class, as in the original — no shipped
    /// setup lacks one).
    pub class: Option<String>,
    pub team: Team,
    pub weapon: Option<String>,
    pub ammo: u16,
    pub scripts: HashMap<String, String>,
}

impl Placed {
    pub fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// A recorded performance (`films.json`): where it starts, and the
/// controls over time (keys from their tick on).
#[derive(Clone, Debug)]
pub struct Film {
    pub start: Vec3,
    pub facing: f32,
    /// The facing it wants (`initialDesiredFacing`), and its aim (look-mode
    /// turn, pitch: `initialAimingLR` / `UD`).
    pub desired_facing: f32,
    pub aim: (f32, f32),
    pub length: u32,
    /// Tick, buttons (the original's button bits), turn left/right.
    pub keys: Vec<(u32, u64, f32)>,
}

/// Everything AI about a level.
#[derive(Clone, Debug, Default)]
pub struct LevelAi {
    pub characters: Vec<Placed>,
    /// The character setups `chr_create` makes, by script id (`setups.json`).
    pub setups: HashMap<u16, Setup>,
    pub patrols: HashMap<u16, Rc<PatrolPath>>,
    pub flags: HashMap<i16, Flag>,
    pub combat: HashMap<u16, Rc<CombatProfile>>,
    /// The level's films, by name.
    pub films: HashMap<String, Rc<Film>>,
}

fn read(path: &Path) -> Value {
    crate::json::load(path)
}

use crate::json::vec3 as v3;

/// An alert level by name (`lull`, `low`, `medium`, `high`, `combat`).
pub fn alert_named(n: &str) -> Alert {
    match n.to_ascii_lowercase().as_str() {
        "low" => Alert::Low,
        "medium" => Alert::Medium,
        "high" => Alert::High,
        "combat" => Alert::Combat,
        _ => Alert::Lull,
    }
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
        for c in read(&level_dir.join("characters.json"))["characters"]
            .as_array()
            .into_iter()
            .flatten()
        {
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
                neutral_id: c["neutralId"].as_u64().map(|v| v as u16),
                hp_offset: c["hitPointOffset"].as_i64().unwrap_or(0) as i32,
                weapon: c["weapon"]
                    .as_str()
                    .filter(|w| !w.is_empty() && *w != "none")
                    .map(String::from),
                ammo_percent: c["ammoPercent"].as_u64().map(|v| v as u32),
                carries: [
                    "ammoBallistic",
                    "ammoEnergy",
                    "hypo",
                    "shieldBelt",
                    "invisibility",
                ]
                .map(|k| c["inventory"][k]["use"].as_u64().unwrap_or(0) as u16),
                drops: [
                    "ammoBallistic",
                    "ammoEnergy",
                    "hypo",
                    "shieldBelt",
                    "invisibility",
                ]
                .map(|k| c["inventory"][k]["drop"].as_u64().unwrap_or(0) as u16),
                alarm_groups: c["alarmGroups"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|b| b.as_u64().filter(|&b| b < 32))
                    .fold(0, |m, b| m | 1 << b),
                alert: [
                    alert(&a["initial"]),
                    alert(&a["minimum"]),
                    alert(&a["job"]),
                    alert(&a["investigate"]),
                ],
                pursue: Pursue {
                    strong_unseen: s(&p["strongUnseen"]),
                    weak_unseen: s(&p["weakUnseen"]),
                    strong_seen: s(&p["strongSeen"]),
                    weak_seen: s(&p["weakSeen"]),
                    lost: s(&p["lost"]),
                },
                scripts: c["scripts"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
        for f in read(&level_dir.join("flags.json"))["flags"]
            .as_array()
            .into_iter()
            .flatten()
        {
            out.flags.insert(
                f["id"].as_i64().unwrap_or(0) as i16,
                Flag {
                    at: v3(&f["position"]),
                    facing: f["facing"].as_f64().unwrap_or(0.0) as f32,
                },
            );
        }
        for c in read(&level_dir.join("setups.json"))["setups"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let text = |v: &Value| v.as_str().map(str::to_string);
            let setup = Setup {
                script_id: c["scriptId"].as_u64().unwrap_or(0) as u16,
                name: text(&c["name"]),
                flag: c["flag"].as_i64().unwrap_or(0) as i16,
                class: text(&c["class"]),
                team: Team::from_name(c["team"].as_str().unwrap_or("neutral")),
                weapon: text(&c["weapon"]),
                ammo: c["ammo"].as_u64().unwrap_or(0) as u16,
                scripts: c["scripts"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect(),
            };
            out.setups.insert(setup.script_id, setup);
        }
        for p in read(&level_dir.join("patrols.json"))["patrols"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let path = PatrolPath {
                id: p["id"].as_u64().unwrap_or(0) as u16,
                name: s(&p["name"]),
                return_to_nearest: p["returnToNearest"].as_bool().unwrap_or(false),
                waypoints: p["waypoints"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(waypoint)
                    .collect(),
            };
            out.patrols.insert(path.id, Rc::new(path));
        }
        for f in read(&level_dir.join("films.json"))["films"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let keys = f["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|k| {
                    (
                        k["time"].as_u64().unwrap_or(0) as u32,
                        k["buttons"].as_u64().unwrap_or(0),
                        k["turnLR"].as_f64().unwrap_or(0.0) as f32,
                    )
                })
                .collect();
            let film = Film {
                start: v3(&f["initialLocation"]),
                facing: f["initialFacing"].as_f64().unwrap_or(0.0) as f32,
                desired_facing: f["initialDesiredFacing"]
                    .as_f64()
                    .or_else(|| f["initialFacing"].as_f64())
                    .unwrap_or(0.0) as f32,
                aim: (
                    f["aiming"][0].as_f64().unwrap_or(0.0) as f32,
                    f["aiming"][1].as_f64().unwrap_or(0.0) as f32,
                ),
                length: f["length"].as_u64().unwrap_or(0) as u32,
                keys,
            };
            out.films.insert(s(&f["name"]), Rc::new(film));
        }
        for c in read(&common_dir.join("combat_profiles.json"))["profiles"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let b = &c["behavior"];
            let f = |v: &Value, d: f32| v.as_f64().map_or(d, |x| x as f32);
            let profile = CombatProfile {
                name: s(&c["name"]),
                behavior: [
                    s(&b["long"]),
                    s(&b["medium"]),
                    s(&b["short"]),
                    s(&b["mediumRetreat"]),
                    s(&b["longRetreat"]),
                ],
                short_range: f(&c["shortRange"], 40.0),
                medium_range: f(&c["mediumRange"], 200.0),
                melee_when: s(&c["meleeWhen"]),
                no_gun: s(&c["noGun"]),
                pursuit_distance: f(&c["pursuitDistance"], 200.0),
                panic: {
                    let u = |k: &str, d: u32| c["panic"][k].as_u64().map_or(d, |v| v as u32);
                    [
                        u("melee", 600),
                        u("gunfire", 900),
                        u("sight", 600),
                        u("hurt", 1200),
                    ]
                },
                alarm: {
                    let a = &c["alarm"];
                    let d = AlarmSettings::default();
                    AlarmSettings {
                        find_distance: f(&a["findDistance"], d.find_distance),
                        ignore_distance: f(&a["ignoreEnemyDistance"], d.ignore_distance),
                        chase_distance: f(&a["chaseEnemyDistance"], d.chase_distance),
                        damage_threshold: a["damageThreshold"].as_i64().unwrap_or(30) as i32,
                        fight_ticks: a["fightTicks"].as_u64().unwrap_or(360) as u32,
                        attack_if_knocked_down: a["attackIfKnockedDown"].as_bool().unwrap_or(false),
                    }
                },
            };
            out.combat
                .insert(c["id"].as_u64().unwrap_or(0) as u16, Rc::new(profile));
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
    let point = || {
        w["point"]
            .is_array()
            .then(|| Target::Point(v3(&w["point"])))
    };
    let f = |k: &str| w[k].as_f64().unwrap_or(0.0) as f32;
    let ticks = || w["ticks"].as_u64().unwrap_or(0) as u32;
    Some(match w["type"].as_str()? {
        "moveToFlag" => Waypoint::MoveTo {
            at: flag()?,
            stop: true,
            face: false,
            within: 0.0,
        },
        "moveToPoint" => Waypoint::MoveTo {
            at: point()?,
            stop: true,
            face: false,
            within: 0.0,
        },
        "moveAndFaceFlag" => Waypoint::MoveTo {
            at: flag()?,
            stop: true,
            face: true,
            within: 0.0,
        },
        "moveThroughFlag" => Waypoint::MoveTo {
            at: flag()?,
            stop: false,
            face: false,
            within: f("distance"),
        },
        "moveThroughPoint" => Waypoint::MoveTo {
            at: point()?,
            stop: false,
            face: false,
            within: f("distance"),
        },
        "moveNearFlag" => Waypoint::MoveTo {
            at: flag()?,
            stop: true,
            face: false,
            within: f("distance"),
        },
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
        "glanceAtFlag" => Waypoint::Glance {
            at: flag()?,
            ticks: ticks(),
        },
        "scan" => Waypoint::Scan {
            ticks: ticks(),
            angle: f("angle"),
        },
        "stopScanning" => Waypoint::StopScanning,
        "guard" => Waypoint::Guard {
            flag: w["flag"].as_i64()? as i16,
            ticks: w["ticks"].as_u64().map(|t| t as u32),
            angle: f("maxAngle"),
        },
        "triggerScript" => Waypoint::Script {
            name: w["script"].as_str()?.to_string(),
            wait: false,
        },
        "playScript" => Waypoint::Script {
            name: w["script"].as_str()?.to_string(),
            wait: true,
        },
        "lockIntoPatrol" => Waypoint::Lock(w["lock"].as_bool().unwrap_or(false)),
        "shootAtFlag" => Waypoint::ShootAt {
            at: flag()?,
            ticks: ticks(),
            inaccuracy: w["inaccuracy"].as_f64().unwrap_or(0.0) as f32,
        },
        _ => return None,
    })
}
