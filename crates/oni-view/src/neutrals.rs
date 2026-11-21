//! A level's neutral behaviours (`levels/NN/neutrals.json`; the original's
//! `OBJtOSD_Neutral`): the conversations neutral characters start with the
//! player (`docs/NEUTRAL.md`).

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

/// One line: who says it, its sound, the animations on each side.
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// The player says it (else the neutral character).
    pub player: bool,
    /// The neutral character walks up and gives its items on this line.
    pub give: bool,
    /// The speaker's animation (an animation type; 0: none), played once
    /// (blocking until it ends) or held for the line.
    pub anim: u16,
    pub anim_once: bool,
    /// The other one's.
    pub other_anim: u16,
    pub other_anim_once: bool,
    pub sound: String,
}

/// A behaviour.
#[derive(Clone, Debug, Default)]
pub struct Behavior {
    pub id: u16,
    pub name: String,
    pub no_resume: bool,
    pub no_resume_after_items: bool,
    pub uninterruptable: bool,
    pub trigger_range: f32,
    pub conversation_range: f32,
    pub follow_range: f32,
    pub abort_enemy_range: f32,
    pub trigger_sound: String,
    pub abort_sound: String,
    pub enemy_sound: String,
    pub end_script: String,
    pub give_weapon: String,
    pub give_ballistic: u16,
    pub give_energy: u16,
    pub give_hypos: u16,
    pub give_shield: bool,
    pub give_invisibility: bool,
    pub give_lsi: bool,
    pub lines: Vec<Line>,
}

fn anim(v: &serde_json::Value) -> u16 {
    match v {
        serde_json::Value::String(n) => oni_schema::value(&oni_schema::ANIM_TYPES, n).unwrap_or(0),
        v => v.as_u64().unwrap_or(0) as u16,
    }
}

fn behavior(v: &serde_json::Value) -> Behavior {
    let flags = |v: &serde_json::Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_string)).collect() };
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    let fl = flags(&v["flags"]);
    let give = &v["give"];
    let gf = flags(&give["flags"]);
    let n = |k: &str| give[k].as_u64().unwrap_or(0) as u16;
    Behavior {
        id: v["id"].as_u64().unwrap_or(0) as u16,
        name: s("name"),
        no_resume: fl.iter().any(|f| f == "noResume"),
        no_resume_after_items: fl.iter().any(|f| f == "noResumeAfterItems"),
        uninterruptable: fl.iter().any(|f| f == "uninterruptable"),
        trigger_range: f("triggerRange"),
        conversation_range: f("conversationRange"),
        follow_range: f("followRange"),
        abort_enemy_range: f("abortEnemyRange"),
        trigger_sound: s("triggerSound"),
        abort_sound: s("abortSound"),
        enemy_sound: s("enemySound"),
        end_script: s("endScript"),
        give_weapon: give["weapon"].as_str().unwrap_or("").to_string(),
        give_ballistic: n("ballisticAmmo"),
        give_energy: n("energyAmmo"),
        give_hypos: n("hypos"),
        give_shield: gf.iter().any(|f| f == "shield"),
        give_invisibility: gf.iter().any(|f| f == "invisibility"),
        give_lsi: gf.iter().any(|f| f == "lsi"),
        lines: v["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| {
                let lf = flags(&l["flags"]);
                let has = |n: &str| lf.iter().any(|f| f == n);
                Line {
                    player: has("playerLine"),
                    give: has("giveItem"),
                    anim: anim(&l["anim"]),
                    anim_once: has("animOnce"),
                    other_anim: anim(&l["otherAnim"]),
                    other_anim_once: has("otherAnimOnce"),
                    sound: l["sound"].as_str().unwrap_or("").to_string(),
                }
            })
            .collect(),
    }
}

/// The level's behaviours, by id (none without the file).
pub fn load(level_dir: &Path) -> HashMap<u16, Rc<Behavior>> {
    let Ok(text) = std::fs::read_to_string(level_dir.join("neutrals.json")) else { return HashMap::new() };
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    doc["neutrals"].as_array().into_iter().flatten().map(behavior).map(|b| (b.id, Rc::new(b))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_behaviour_reads_from_its_json() {
        let v = serde_json::json!({
            "id": 4, "name": "Tarmac Special", "flags": ["noResumeAfterItems"],
            "triggerRange": 100.0, "conversationRange": 30.0, "followRange": 200.0, "abortEnemyRange": 50.0,
            "triggerSound": "a", "abortSound": "b", "enemySound": "c", "endScript": "",
            "give": {"weapon": "w6_vdg", "ballisticAmmo": 0, "energyAmmo": 2, "hypos": 0, "flags": ["shield"]},
            "lines": [{"flags": ["playerLine", "animOnce"], "anim": "actTalk", "otherAnim": null, "sound": "x"}]
        });
        let b = behavior(&v);
        assert!(b.no_resume_after_items && !b.no_resume);
        assert_eq!((b.give_energy, b.give_shield, b.give_weapon.as_str()), (2, true, "w6_vdg"));
        assert!(b.lines[0].player && b.lines[0].anim_once && b.lines[0].anim != 0 && b.lines[0].other_anim == 0);
    }
}
