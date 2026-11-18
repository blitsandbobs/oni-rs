//! Melee profiles, loaded from the converter's
//! `common/characters/melee_profiles.json` (`docs/ASSET_FORMAT.md`).

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use oni_schema as sc;
use serde_json::Value;

/// One step of a technique. Indices are into the tables of
/// [`super::tables`]; times are ticks, angles radians, distances units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    Attack(usize),
    /// A delaying position (run / close / run-jump) carries its run-in band.
    Position { index: usize, run_in: Option<RunIn> },
    Maneuver { index: usize, duration: u32, a: f32, b: f32 },
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
    /// Ignores danger (ledges, hazards) — danger isn't modelled yet.
    #[allow(dead_code)]
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
    /// Bonus for throwing into danger — danger isn't modelled yet.
    #[allow(dead_code)]
    pub throw_into_danger: f32,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    /// The class whose animations the original plans with; we plan with the
    /// fighter's own (the same class for every shipped profile but two).
    #[allow(dead_code)]
    pub class: String,
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
        self.techniques.iter().enumerate().filter(move |(_, t)| t.kind == kind)
    }
}

/// All profiles by name.
pub fn load(path: &Path) -> std::io::Result<HashMap<String, Rc<Profile>>> {
    let text = std::fs::read_to_string(path)?;
    let doc: Value =
        serde_json::from_str(&text).map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
    Ok(doc["profiles"]
        .as_array()
        .map(|a| a.iter().map(profile).map(|p| (p.name.clone(), Rc::new(p))).collect())
        .unwrap_or_default())
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn profile(p: &Value) -> Profile {
    let w = &p["weights"];
    let mut techniques = Vec::new();
    for (key, kind) in [("actions", Kind::Action), ("reactions", Kind::Reaction), ("spacing", Kind::Spacing)] {
        for t in p[key].as_array().into_iter().flatten() {
            techniques.push(technique(t, kind));
        }
    }
    Profile {
        name: p["name"].as_str().unwrap_or("").to_string(),
        class: p["class"].as_str().unwrap_or("").to_string(),
        notice_percent: f(&p["attackNoticePercent"]),
        dodge_base: f(&p["dodge"]["basePercent"]),
        dodge_additional: f(&p["dodge"]["additionalPercent"]),
        dodge_damage_threshold: f(&p["dodge"]["damageThreshold"]),
        block_skill: f(&p["block"]["skillPercent"]),
        block_group: f(&p["block"]["groupPercent"]),
        weights: GuardWeights {
            not_blocking: f(&w["targetNotBlocking"]),
            wrong_stance: f(&w["targetWrongStance"]),
            unblockable: f(&w["blockedButUnblockable"]),
            staggers: f(&w["blockedButStaggers"]),
            block_stun: f(&w["blockedButBlockStun"]),
            blocked: f(&w["blocked"]),
            throw_into_danger: f(&w["throwIntoDanger"]),
        },
        techniques,
    }
}

fn technique(t: &Value, kind: Kind) -> Technique {
    let flags: Vec<&str> = t["flags"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect();
    Technique {
        name: t["name"].as_str().unwrap_or("").to_string(),
        kind,
        interruptible: flags.contains(&"interruptible"),
        generous_direction: flags.contains(&"generousDirection"),
        fearless: flags.contains(&"fearless"),
        weight: f(&t["weight"]),
        repeat_delay: t["repeatDelay"].as_u64().unwrap_or(0) as u32,
        moves: t["moves"].as_array().into_iter().flatten().filter_map(parse_move).collect(),
    }
}

fn parse_move(m: &Value) -> Option<Move> {
    let name = m["move"].as_str()?;
    let index = |table: &[&str]| sc::value(table, name).map(usize::from);
    Some(match m["type"].as_str()? {
        "attack" => Move::Attack(index(&sc::MELEE_ATTACKS)?),
        "position" => Move::Position {
            index: index(&sc::MELEE_POSITIONS)?,
            run_in: m.get("maxRunIn").map(|_| RunIn {
                min: f(&m["minRunIn"]),
                max: f(&m["maxRunIn"]),
                tolerance: f(&m["tolerance"]),
            }),
        },
        "maneuver" => {
            // The two extra parameters, whichever names this maneuver uses.
            let pick = |keys: &[&str]| keys.iter().find_map(|k| m.get(*k)).map(f).unwrap_or(0.0);
            Move::Maneuver {
                index: index(&sc::MELEE_MANEUVERS)?,
                duration: m["duration"].as_u64().unwrap_or(0) as u32,
                a: pick(&["minRange", "maxRange", "targetRange", "minAngle", "closenessAngle", "chancePercent"]),
                b: pick(&["threshold", "tolerance", "maxAngle"]),
            }
        }
        "evade" => Move::Evade(index(&sc::MELEE_EVADES)?),
        "throw" => Move::Throw(index(&sc::MELEE_THROWS)?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let combo = k.techniques.iter().find(|t| t.name == "Punch Combo").expect("Punch Combo");
        assert_eq!(combo.kind, Kind::Action);
        assert_eq!(combo.moves.len(), 5);
        assert!(matches!(combo.moves[0], Move::Position { index: 12, run_in: Some(_) }));
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
        let parsed: usize = all.values().flat_map(|p| &p.techniques).map(|t| t.moves.len()).sum();
        assert_eq!(parsed, in_json);
    }
}
