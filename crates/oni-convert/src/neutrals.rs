//! Neutral behaviours (`OT_Neutral`, `OBJiNeutral_Read`; `docs/ASSET_FORMAT.md`,
//! *Neutral behaviours*): the conversations a neutral character starts with
//! the player — its ranges, its trigger / abort / enemy lines, the script
//! run at its end, what it gives, and up to ten lines, each said by it or by
//! the player, with an animation for each side.
//! Written to `levels/NN/neutrals.json`.

use std::path::Path;

use oni_dat::Dat;
use oni_schema::files;
#[cfg(test)]
use serde_json::json;

use crate::{cstr, le_f32, le_u16, le_u32};

/// Every name field is 32 bytes (`BFcMaxFileNameLength`,
/// `SLcScript_MaxNameLength`, `WPcMaxWeaponName`).
const NAME: usize = 32;
/// Object-file versions: the enemy line (`OBJcVersion_25`), each line's
/// other-character animation and its padding (`OBJcVersion_24`).
const V_ENEMY_SOUND: u32 = 25;
const V_OTHER_ANIM: u32 = 24;
const MAX_LINES: usize = 10;

/// A behaviour's flags (`AI2cNeutralBehaviorFlag_*`).
const BEHAVIOR_FLAGS: [&str; 3] = ["noResume", "noResumeAfterItems", "uninterruptable"];
/// What it gives besides ammo and hypos (`AI2cNeutralGive_*`).
const GIVE_FLAGS: [&str; 3] = ["shield", "invisibility", "lsi"];
/// A line's flags (`AI2cNeutralLineFlag_*`).
const LINE_FLAGS: [&str; 4] = ["playerLine", "giveItem", "animOnce", "otherAnimOnce"];

fn flag_names(bits: u32, names: &[&'static str]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .filter(|(k, _)| bits & (1 << k) != 0)
        .map(|(_, n)| n.to_string())
        .collect()
}

fn anim(t: u16) -> Option<files::AnimRef> {
    match t {
        0 => None,
        t => Some(
            oni_schema::name(&oni_schema::ANIM_TYPES, t).map_or(files::AnimRef::Number(t), |n| {
                files::AnimRef::Name(n.to_string())
            }),
        ),
    }
}

/// One behaviour from its object data (`version`: the object file's).
fn behavior(d: &[u8], version: u32) -> Option<files::Neutral> {
    let mut at = 0usize;
    let string = |at: &mut usize| -> Option<String> {
        d.get(*at..*at + NAME)?;
        let s = cstr(d, *at, NAME);
        *at += NAME;
        Some(s)
    };
    let name = string(&mut at)?;
    let id = le_u16(d, at);
    let num_lines = (le_u16(d, at + 2) as usize).min(MAX_LINES);
    let flags = le_u32(d, at + 4);
    let ranges = [
        le_f32(d, at + 8),
        le_f32(d, at + 12),
        le_f32(d, at + 16),
        le_f32(d, at + 20),
    ];
    at += 24;
    let trigger = string(&mut at)?;
    let abort = string(&mut at)?;
    let enemy = if version >= V_ENEMY_SOUND {
        string(&mut at)?
    } else {
        "what_the".to_string()
    };
    let end_script = string(&mut at)?;
    let give_weapon = string(&mut at)?;
    let give = d.get(at..at + 4)?;
    at += 4;
    let mut lines = Vec::new();
    for _ in 0..num_lines {
        let line_flags = le_u16(d, at);
        at += if version >= V_OTHER_ANIM { 4 } else { 2 };
        let anim_type = le_u16(d, at);
        at += 2;
        let other = if version >= V_OTHER_ANIM {
            at += 2;
            le_u16(d, at - 2)
        } else {
            0
        };
        let sound = string(&mut at)?;
        lines.push(files::NeutralLine {
            flags: flag_names(u32::from(line_flags), &LINE_FLAGS),
            anim: anim(anim_type),
            other_anim: anim(other),
            sound,
        });
    }
    Some(files::Neutral {
        id,
        name,
        flags: flag_names(flags, &BEHAVIOR_FLAGS),
        trigger_range: ranges[0],
        conversation_range: ranges[1],
        follow_range: ranges[2],
        abort_enemy_range: ranges[3],
        trigger_sound: trigger,
        abort_sound: abort,
        enemy_sound: enemy,
        end_script,
        give: files::NeutralGive {
            weapon: give_weapon,
            ballistic_ammo: give[0],
            energy_ammo: give[1],
            hypos: give[2],
            flags: flag_names(u32::from(give[3]), &GIVE_FLAGS),
        },
        lines,
    })
}

/// The level's neutral behaviours.
pub fn neutrals(dat: &Dat<'_>, raw: &[u8]) -> Vec<files::Neutral> {
    let (version, objects) = crate::objects::read(dat, raw, "Neutral");
    objects
        .iter()
        .filter_map(|o| behavior(o.data, version))
        .collect()
}

pub fn write(dat: &Dat<'_>, raw: &[u8], root: &Path) -> std::io::Result<()> {
    let n = neutrals(dat, raw);
    if n.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(
        root.join("neutrals.json"),
        serde_json::to_string_pretty(&files::NeutralsFile { neutrals: n })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.resize(NAME, 0);
        v
    }

    #[test]
    fn a_behaviour_reads_as_the_original_lays_it_out() {
        let mut d = name("hello");
        d.extend_from_slice(&7u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&5u32.to_le_bytes());
        for r in [100.0f32, 30.0, 200.0, 50.0] {
            d.extend_from_slice(&r.to_le_bytes());
        }
        for s in ["hi", "bye", "eek", "end_fn", "w1_tap"] {
            d.extend(name(s));
        }
        d.extend_from_slice(&[10, 0, 2, 4]);
        // One line: the player says it (flag 1), anim 0, other 0, its sound.
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&[0, 0]);
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend(name("line1"));
        let b = serde_json::to_value(behavior(&d, 25).unwrap()).unwrap();
        assert_eq!(b["id"], 7);
        assert_eq!(b["flags"], json!(["noResume", "uninterruptable"]));
        assert_eq!(b["conversationRange"], 30.0);
        assert_eq!(b["enemySound"], "eek");
        assert_eq!(b["endScript"], "end_fn");
        assert_eq!(b["give"]["weapon"], "w1_tap");
        assert_eq!(b["give"]["hypos"], 2);
        assert_eq!(b["give"]["flags"], json!(["lsi"]));
        assert_eq!(b["lines"][0]["flags"], json!(["playerLine"]));
        assert_eq!(b["lines"][0]["sound"], "line1");
        assert!(b["lines"][0]["anim"].is_null());
    }
}
