//! Melee profiles, loaded from the converter's
//! `common/characters/melee_profiles.json`
//! (`oni_schema::files::MeleeProfilesFile`).

use oni_schema::files;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use oni_schema as sc;

/// One step of a technique. Indices are into the tables of
/// [`super::tables`]; times are ticks, angles radians, distances units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    Attack(usize),
    /// A delaying position (run / close / run-jump) carries its run-in band.
    Position {
        index: usize,
        run_in: Option<RunIn>,
    },
    Maneuver {
        index: usize,
        duration: u32,
        a: f32,
        b: f32,
    },
    Evade(usize),
    Throw(usize),
}

/// How far a delaying position move may run in before its attack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunIn {
    pub min: f32,
    pub max: f32,
    pub tolerance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Action,
    Reaction,
    Spacing,
}

#[derive(Clone, Debug)]
pub struct Technique {
    pub name: String,
    pub kind: Kind,
    pub interruptible: bool,
    pub generous_direction: bool,
    /// Ignores danger (ledges).
    pub fearless: bool,
    pub weight: f32,
    /// Ticks before it may be used again.
    pub repeat_delay: u32,
    pub moves: Vec<Move>,
}

/// Weight multipliers by the target's guard.
#[derive(Clone, Copy, Debug, Default)]
pub struct GuardWeights {
    pub not_blocking: f32,
    pub wrong_stance: f32,
    pub unblockable: f32,
    pub staggers: f32,
    pub block_stun: f32,
    pub blocked: f32,
    /// Bonus for throwing the target over a drop.
    pub throw_into_danger: f32,
}

/// What a profile plans its techniques with (`AI2rMelee_PrepareForUse`):
/// its class's animations and jumping.
pub struct Planning {
    pub coll: Rc<crate::anim::collection::AnimCollection>,
    pub tuning: crate::anim::motion::ClassTuning,
}

impl std::fmt::Debug for Planning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Planning")
    }
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub id: u16,
    pub name: String,
    /// The class whose animations and jumps its techniques are planned with
    /// (`char_classname`), and those once loaded (`Classes::plan`; none: the
    /// fighter's own).
    pub class: String,
    pub plan: std::cell::OnceCell<Rc<Planning>>,
    pub notice_percent: f32,
    pub dodge_base: f32,
    pub dodge_additional: f32,
    pub dodge_damage_threshold: f32,
    pub block_skill: f32,
    pub block_group: f32,
    pub weights: GuardWeights,
    /// Actions, then reactions, then spacing — the original's order (indices
    /// into this list key the per-technique timers).
    pub techniques: Vec<Technique>,
}

impl Profile {
    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = (usize, &Technique)> {
        self.techniques
            .iter()
            .enumerate()
            .filter(move |(_, t)| t.kind == kind)
    }
}

/// All profiles by name.
pub fn load(path: &Path) -> std::io::Result<HashMap<String, Rc<Profile>>> {
    let text = std::fs::read_to_string(path)?;
    let doc: files::MeleeProfilesFile = serde_json::from_str(&text)
        .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
    Ok(doc
        .profiles
        .iter()
        .map(profile)
        .map(|p| (p.name.clone(), Rc::new(p)))
        .collect())
}

fn profile(p: &files::MeleeProfile) -> Profile {
    let w = &p.weights;
    let mut techniques = Vec::new();
    for (list, kind) in [
        (&p.actions, Kind::Action),
        (&p.reactions, Kind::Reaction),
        (&p.spacing, Kind::Spacing),
    ] {
        for t in list {
            techniques.push(technique(t, kind));
        }
    }
    Profile {
        id: p.id as u16,
        name: p.name.clone(),
        class: p.class.clone(),
        plan: Default::default(),
        notice_percent: p.attack_notice_percent as f32,
        dodge_base: p.dodge.base_percent as f32,
        dodge_additional: p.dodge.additional_percent as f32,
        dodge_damage_threshold: p.dodge.damage_threshold as f32,
        block_skill: p.block.skill_percent as f32,
        block_group: p.block.group_percent as f32,
        weights: GuardWeights {
            not_blocking: w.target_not_blocking,
            wrong_stance: w.target_wrong_stance,
            unblockable: w.blocked_but_unblockable,
            staggers: w.blocked_but_staggers,
            block_stun: w.blocked_but_block_stun,
            blocked: w.blocked,
            throw_into_danger: w.throw_into_danger,
        },
        techniques,
    }
}

fn technique(t: &files::Technique, kind: Kind) -> Technique {
    let has = |f: &str| t.flags.iter().any(|x| x == f);
    Technique {
        name: t.name.clone(),
        kind,
        interruptible: has("interruptible"),
        generous_direction: has("generousDirection"),
        fearless: has("fearless"),
        weight: t.weight as f32,
        repeat_delay: t.repeat_delay,
        moves: t.moves.iter().filter_map(parse_move).collect(),
    }
}

fn parse_move(m: &files::MeleeMove) -> Option<Move> {
    let name = m.name.as_deref()?;
    let index = |table: &[&str]| sc::value(table, name).map(usize::from);
    Some(match m.kind.as_str() {
        "attack" => Move::Attack(index(&sc::MELEE_ATTACKS)?),
        "position" => Move::Position {
            index: index(&sc::MELEE_POSITIONS)?,
            run_in: m.max_run_in.map(|max| RunIn {
                min: m.min_run_in.unwrap_or(0.0),
                max,
                tolerance: m.tolerance.unwrap_or(0.0),
            }),
        },
        // The two extra parameters, whichever this maneuver has.
        "maneuver" => Move::Maneuver {
            index: index(&sc::MELEE_MANEUVERS)?,
            duration: m.duration.unwrap_or(0),
            a: m.min_range
                .or(m.max_range)
                .or(m.target_range)
                .or(m.min_angle)
                .or(m.closeness_angle)
                .or(m.chance_percent)
                .unwrap_or(0.0),
            b: m.threshold.or(m.tolerance).or(m.max_angle).unwrap_or(0.0),
        },
        "evade" => Move::Evade(index(&sc::MELEE_EVADES)?),
        "throw" => Move::Throw(index(&sc::MELEE_THROWS)?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;

    #[test]
    fn profiles_load() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/common/characters/melee_profiles.json");
        let Ok(all) = load(&path) else {
            eprintln!("SKIP: generated profiles not found");
            return;
        };
        assert_eq!(all.len(), 45);
        let k = &all["KONOKO"];
        let combo = k
            .techniques
            .iter()
            .find(|t| t.name == "Punch Combo")
            .expect("Punch Combo");
        assert_eq!(combo.kind, Kind::Action);
        assert_eq!(combo.moves.len(), 5);
        assert!(matches!(
            combo.moves[0],
            Move::Position {
                index: 12,
                run_in: Some(_)
            }
        ));
        assert_eq!(combo.moves[1], Move::Attack(0));
        // Every move of every profile parsed (two shipped techniques are empty).
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let in_json: usize = doc["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| ["actions", "reactions", "spacing"].map(|k| p[k].clone()))
            .flat_map(|ts| ts.as_array().cloned().unwrap_or_default())
            .map(|t| t["moves"].as_array().map_or(0, |m| m.len()))
            .sum();
        let parsed: usize = all
            .values()
            .flat_map(|p| &p.techniques)
            .map(|t| t.moves.len())
            .sum();
        assert_eq!(parsed, in_json);
    }
}
