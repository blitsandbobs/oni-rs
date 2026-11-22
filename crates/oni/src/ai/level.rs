//! A level's AI data (`oni_schema::files`): its placed characters,
//! patrol paths and flags (from the level folder) and the combat profiles
//! (common). Behaviour that uses it:.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use glam::Vec3;

use super::perception::{Alert, Team};

/// A patrol waypoint.
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
    /// Its neutral behaviour.
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

impl LevelAi {
    /// Loads a level's AI data (`level_dir`) and the common combat profiles
    /// (`common_dir`). Missing files leave their part empty.
    pub fn load(level_dir: &Path, common_dir: &Path) -> LevelAi {
        use crate::json::read_file;
        use oni_schema::files;
        let mut out = LevelAi::default();
        let characters: files::CharactersFile = read_file(level_dir.join("characters.json"));
        for c in &characters.characters {
            let carried = |kind: &str| c.inventory.get(kind);
            const KINDS: [&str; 5] = [
                "ammoBallistic",
                "ammoEnergy",
                "hypo",
                "shieldBelt",
                "invisibility",
            ];
            let (a, p) = (&c.alert, &c.pursue);
            out.characters.push(Placed {
                name: c.name.clone(),
                class: c.class.clone(),
                flags: c.flags.clone(),
                at: Vec3::from(c.position),
                facing: c.facing,
                team: Team::from_name(&c.team),
                job: c.job.clone(),
                job_id: c.job_id,
                combat_id: c.combat_profile_id,
                melee_id: c.melee_profile_id,
                neutral_id: c.neutral_id,
                hp_offset: c.hit_point_offset.unwrap_or(0),
                weapon: c.weapon.clone().filter(|w| !w.is_empty() && w != "none"),
                ammo_percent: Some(c.ammo_percent),
                carries: KINDS.map(|k| carried(k).map_or(0, |x| x.uses)),
                drops: KINDS.map(|k| carried(k).map_or(0, |x| x.drop)),
                alarm_groups: c
                    .alarm_groups
                    .iter()
                    .filter(|&&b| b < 32)
                    .fold(0, |m, b| m | 1 << b),
                alert: [
                    alert_named(&a.initial),
                    alert_named(&a.minimum),
                    alert_named(&a.job),
                    alert_named(&a.investigate),
                ],
                pursue: Pursue {
                    strong_unseen: p.strong_unseen.clone(),
                    weak_unseen: p.weak_unseen.clone(),
                    strong_seen: p.strong_seen.clone(),
                    weak_seen: p.weak_seen.clone(),
                    lost: p.lost.clone(),
                },
                scripts: c.scripts.clone().into_iter().collect(),
            });
        }
        let flags: files::FlagsFile = read_file(level_dir.join("flags.json"));
        for f in &flags.flags {
            out.flags.insert(
                f.id as i16,
                Flag {
                    at: Vec3::from(f.position),
                    facing: f.facing,
                },
            );
        }
        let setups: files::SetupsFile = read_file(level_dir.join("setups.json"));
        for c in &setups.setups {
            let setup = Setup {
                script_id: c.script_id,
                name: c.name.clone(),
                flag: c.flag,
                class: c.class.clone(),
                team: Team::from_name(&c.team),
                weapon: c.weapon.clone(),
                ammo: c.ammo.unwrap_or(0),
                scripts: c.scripts.clone().into_iter().collect(),
            };
            out.setups.insert(setup.script_id, setup);
        }
        let patrols: files::PatrolsFile = read_file(level_dir.join("patrols.json"));
        for p in &patrols.patrols {
            let path = PatrolPath {
                id: p.id,
                name: p.name.clone(),
                return_to_nearest: p.return_to_nearest,
                waypoints: p.waypoints.iter().filter_map(waypoint).collect(),
            };
            out.patrols.insert(path.id, Rc::new(path));
        }
        let films: files::FilmsFile = read_file(level_dir.join("films.json"));
        for f in &films.films {
            let film = Film {
                start: Vec3::from(f.initial_location),
                facing: f.initial_facing,
                desired_facing: f.initial_desired_facing,
                aim: (f.aiming[0], f.aiming[1]),
                length: f.length,
                keys: f
                    .keys
                    .iter()
                    .map(|k| (k.time, k.buttons, k.turn_lr))
                    .collect(),
            };
            out.films
                .insert(f.name.clone().unwrap_or_default(), Rc::new(film));
        }
        let profiles: files::CombatProfilesFile =
            read_file(common_dir.join("combat_profiles.json"));
        for c in &profiles.profiles {
            let (b, a) = (&c.behavior, &c.alarm);
            let profile = CombatProfile {
                name: c.name.clone(),
                behavior: [
                    b.long.clone(),
                    b.medium.clone(),
                    b.short.clone(),
                    b.medium_retreat.clone(),
                    b.long_retreat.clone(),
                ],
                short_range: c.short_range,
                medium_range: c.medium_range,
                melee_when: c.melee_when.clone(),
                no_gun: c.no_gun.clone(),
                pursuit_distance: c.pursuit_distance,
                panic: [c.panic.melee, c.panic.gunfire, c.panic.sight, c.panic.hurt],
                alarm: AlarmSettings {
                    find_distance: a.find_distance,
                    ignore_distance: a.ignore_enemy_distance,
                    chase_distance: a.chase_enemy_distance,
                    damage_threshold: a.damage_threshold as i32,
                    fight_ticks: a.fight_ticks,
                    attack_if_knocked_down: a.attack_if_knocked_down,
                },
            };
            out.combat.insert(c.id, Rc::new(profile));
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

fn waypoint(w: &oni_schema::files::Waypoint) -> Option<Waypoint> {
    use oni_schema::files::Waypoint as W;
    let flag = |f: i16| Target::Flag(f);
    let point = |p: &[f32; 3]| Target::Point(Vec3::from(*p));
    // (Negative ticks read as none.)
    let ticks = |t: i64| u32::try_from(t).unwrap_or(0);
    Some(match w {
        W::MoveToFlag { flag: f } => Waypoint::MoveTo {
            at: flag(*f),
            stop: true,
            face: false,
            within: 0.0,
        },
        W::MoveToPoint { point: p } => Waypoint::MoveTo {
            at: point(p),
            stop: true,
            face: false,
            within: 0.0,
        },
        W::MoveAndFaceFlag { flag: f } => Waypoint::MoveTo {
            at: flag(*f),
            stop: true,
            face: true,
            within: 0.0,
        },
        W::MoveThroughFlag { flag: f, distance } => Waypoint::MoveTo {
            at: flag(*f),
            stop: false,
            face: false,
            within: *distance,
        },
        W::MoveThroughPoint { point: p, distance } => Waypoint::MoveTo {
            at: point(p),
            stop: false,
            face: false,
            within: *distance,
        },
        W::MoveNearFlag { flag: f, distance } => Waypoint::MoveTo {
            at: flag((*f)?),
            stop: true,
            face: false,
            within: distance.unwrap_or(0.0),
        },
        W::Stop => Waypoint::Stop,
        W::Pause { ticks: t } => Waypoint::Pause(ticks(i64::from(*t))),
        W::LookAtFlag { flag: f } => Waypoint::LookAt(flag(*f)),
        W::LookAtPoint { point: p } => Waypoint::LookAt(point(p)),
        W::StopLooking => Waypoint::StopLooking,
        W::LookInDirection { .. } => Waypoint::LookInDirection,
        W::FreeFacing => Waypoint::FreeFacing,
        W::Loop => Waypoint::Loop,
        W::LoopToWaypoint { waypoint } => Waypoint::LoopTo(*waypoint as usize),
        W::MovementMode { mode } => Waypoint::Pace(mode.clone()),
        W::GlanceAtFlag { flag: f, ticks: t } => Waypoint::Glance {
            at: flag(*f),
            ticks: *t,
        },
        W::Scan { ticks: t, angle } => Waypoint::Scan {
            ticks: ticks(i64::from(*t)),
            angle: *angle,
        },
        W::StopScanning => Waypoint::StopScanning,
        W::Guard {
            ticks: t,
            flag,
            max_angle,
        } => Waypoint::Guard {
            flag: *flag,
            ticks: t.and_then(|t| u32::try_from(t).ok()),
            angle: *max_angle,
        },
        W::TriggerScript { script } => Waypoint::Script {
            name: script.clone(),
            wait: false,
        },
        W::PlayScript { script } => Waypoint::Script {
            name: script.clone(),
            wait: true,
        },
        W::LockIntoPatrol { lock } => Waypoint::Lock(*lock),
        W::ShootAtFlag {
            flag: f,
            ticks: t,
            inaccuracy,
        } => Waypoint::ShootAt {
            at: flag(*f),
            ticks: ticks(i64::from(*t)),
            inaccuracy: *inaccuracy,
        },
        W::Unknown => return None,
    })
}
