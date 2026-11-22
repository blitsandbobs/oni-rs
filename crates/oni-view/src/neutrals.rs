//! A level's neutral behaviours (`levels/NN/neutrals.json`; the original's
//! `OBJtOSD_Neutral`): the conversations neutral characters start with the
//! player (`docs/NEUTRAL.md`).

use oni_schema::files;
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

fn anim(a: &Option<files::AnimRef>) -> u16 {
    match a {
        Some(files::AnimRef::Name(n)) => oni_schema::value(&oni_schema::ANIM_TYPES, n).unwrap_or(0),
        Some(files::AnimRef::Number(t)) => *t,
        None => 0,
    }
}

fn behavior(v: &files::Neutral) -> Behavior {
    let has = |flags: &[String], n: &str| flags.iter().any(|f| f == n);
    let (fl, g) = (&v.flags, &v.give);
    Behavior {
        id: v.id,
        name: v.name.clone(),
        no_resume: has(fl, "noResume"),
        no_resume_after_items: has(fl, "noResumeAfterItems"),
        uninterruptable: has(fl, "uninterruptable"),
        trigger_range: v.trigger_range,
        conversation_range: v.conversation_range,
        follow_range: v.follow_range,
        abort_enemy_range: v.abort_enemy_range,
        trigger_sound: v.trigger_sound.clone(),
        abort_sound: v.abort_sound.clone(),
        enemy_sound: v.enemy_sound.clone(),
        end_script: v.end_script.clone(),
        give_weapon: g.weapon.clone(),
        give_ballistic: u16::from(g.ballistic_ammo),
        give_energy: u16::from(g.energy_ammo),
        give_hypos: u16::from(g.hypos),
        give_shield: has(&g.flags, "shield"),
        give_invisibility: has(&g.flags, "invisibility"),
        give_lsi: has(&g.flags, "lsi"),
        lines: v
            .lines
            .iter()
            .map(|l| Line {
                player: has(&l.flags, "playerLine"),
                give: has(&l.flags, "giveItem"),
                anim: anim(&l.anim),
                anim_once: has(&l.flags, "animOnce"),
                other_anim: anim(&l.other_anim),
                other_anim_once: has(&l.flags, "otherAnimOnce"),
                sound: l.sound.clone(),
            })
            .collect(),
    }
}

/// The level's behaviours, by id (none without the file).
pub fn load(level_dir: &Path) -> HashMap<u16, Rc<Behavior>> {
    let doc: files::NeutralsFile = crate::json::read_file(level_dir.join("neutrals.json"));
    doc.neutrals
        .iter()
        .map(behavior)
        .map(|b| (b.id, Rc::new(b)))
        .collect()
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
        let b = behavior(&serde_json::from_value(v).unwrap());
        assert!(b.no_resume_after_items && !b.no_resume);
        assert_eq!(
            (b.give_energy, b.give_shield, b.give_weapon.as_str()),
            (2, true, "w6_vdg")
        );
        assert!(
            b.lines[0].player
                && b.lines[0].anim_once
                && b.lines[0].anim != 0
                && b.lines[0].other_anim == 0
        );
    }
}
