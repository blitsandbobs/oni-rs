//! Animation sounds (//!): the swoosh of a punch or kick, a
//! jump, a roll, a reload — by the clip's animation type and the
//! character's variant (`common/anim_sounds.json`).

use std::collections::HashMap;
use std::path::Path;

/// Animation-sound weights ("mod types").
const ANY: u8 = 0;
const CROUCH: u8 = 1;
const JUMP: u8 = 2;
const HEAVY: u8 = 3;
const MEDIUM: u8 = 4;
const LIGHT: u8 = 5;

fn weight(name: &str) -> u8 {
    match name {
        "crouch" => CROUCH,
        "jump" => JUMP,
        "heavy damage" => HEAVY,
        "medium damage" => MEDIUM,
        "light damage" => LIGHT,
        _ => ANY,
    }
}

/// The animation types that sound, their sound type and weight (the
/// original's table; every other type is silent).
fn sound_type(kind: &str) -> Option<(&'static str, u8)> {
    const TABLE: &[(&str, &str, u8)] = &[
        ("walk", "Walk", ANY),
        ("run", "Run", ANY),
        ("slide", "Slide", ANY),
        ("jump", "Jump", ANY),
        ("stand", "Stand", ANY),
        ("runBackwards", "Run", ANY),
        ("runSidestepLeft", "Run", ANY),
        ("runSidestepRight", "Run", ANY),
        ("kick", "Kick", ANY),
        ("walkSidestepLeft", "Walk", ANY),
        ("walkSidestepRight", "Walk", ANY),
        ("walkBackwards", "Walk", ANY),
        ("stance", "Stand", ANY),
        ("jumpForward", "Jump", ANY),
        ("jumpBackward", "Jump", ANY),
        ("jumpLeft", "Jump", ANY),
        ("jumpRight", "Jump", ANY),
        ("punch", "Punch", ANY),
        ("block", "Block", ANY),
        ("land", "Land", ANY),
        ("fly", "Fly", ANY),
        ("kickForward", "Kick", ANY),
        ("kickLeft", "Kick", ANY),
        ("kickRight", "Kick", ANY),
        ("kickBack", "Kick", ANY),
        ("kickLow", "Kick", ANY),
        ("punchForward", "Punch", ANY),
        ("punchLeft", "Punch", ANY),
        ("punchRight", "Punch", ANY),
        ("punchBack", "Punch", ANY),
        ("punchLow", "Punch", ANY),
        ("kick2", "Kick", ANY),
        ("kick3", "Kick", ANY),
        ("punch2", "Punch", ANY),
        ("punch3", "Punch", ANY),
        ("landForward", "Land", ANY),
        ("landRight", "Land", ANY),
        ("landLeft", "Land", ANY),
        ("landBack", "Land", ANY),
        ("ppk", "Kick", ANY),
        ("pkk", "Kick", ANY),
        ("pkp", "Punch", ANY),
        ("kpk", "Kick", ANY),
        ("kpp", "Punch", ANY),
        ("kkp", "Punch", ANY),
        ("pk", "Kick", ANY),
        ("kp", "Punch", ANY),
        ("punchHeavy", "Punch", ANY),
        ("kickHeavy", "Kick", ANY),
        ("punchForwardHeavy", "Punch", ANY),
        ("kickForwardHeavy", "Kick", ANY),
        ("hitOverlay", "Getting Hit", ANY),
        ("runKick", "Kick", ANY),
        ("runPunch", "Punch", ANY),
        ("runBackPunch", "Punch", ANY),
        ("runBackKick", "Kick", ANY),
        ("sidestepLeftKick", "Kick", ANY),
        ("sidestepLeftPunch", "Punch", ANY),
        ("sidestepRightKick", "Kick", ANY),
        ("sidestepRightPunch", "Punch", ANY),
        ("prone", "Fall", ANY),
        ("hitHead", "Getting Hit", ANY),
        ("hitBody", "Getting Hit", ANY),
        ("hitFoot", "Getting Hit", ANY),
        ("hitCrouch", "Getting Hit", CROUCH),
        ("hitFallen", "Getting Hit", ANY),
        ("hitHeadBehind", "Getting Hit", ANY),
        ("hitBodyBehind", "Getting Hit", ANY),
        ("hitFootBehind", "Getting Hit", ANY),
        ("knockdownHeadBehind", "Knockdown", ANY),
        ("knockdownBodyBehind", "Knockdown", ANY),
        ("knockdownFootBehind", "Knockdown", ANY),
        ("hitCrouchBehind", "Getting Hit", ANY),
        ("knockdownCrouchBehind", "Knockdown", ANY),
        ("throwForwardPunch", "Punch", ANY),
        ("throwForwardKick", "Kick", ANY),
        ("throwBehindPunch", "Punch", ANY),
        ("throwBehindKick", "Kick", ANY),
        ("runThrowForwardPunch", "Punch", ANY),
        ("runThrowBehindPunch", "Punch", ANY),
        ("runThrowForwardKick", "Kick", ANY),
        ("runThrowBehindKick", "Kick", ANY),
        ("startleLeftUnused", "Startle", ANY),
        ("startleRightUnused", "Startle", ANY),
        ("kick3Forward", "Kick", ANY),
        ("hitFootOuch", "Getting Hit", ANY),
        ("hitJewels", "Getting Hit", ANY),
        ("ppkk", "Kick", ANY),
        ("ppkkk", "Kick", ANY),
        ("ppkkkk", "Kick", ANY),
        ("landHard", "Land", ANY),
        ("landForwardHard", "Land", ANY),
        ("landRightHard", "Land", ANY),
        ("landLeftHard", "Land", ANY),
        ("landBackHard", "Land", ANY),
        ("landDead", "Land", ANY),
        ("crouchForward", "Roll", CROUCH),
        ("crouchBack", "Roll", CROUCH),
        ("crouchLeft", "Roll", CROUCH),
        ("crouchRight", "Roll", CROUCH),
        ("getupKickBack", "Kick", ANY),
        ("holster", "Holster", ANY),
        ("drawPistol", "Draw Weapon", ANY),
        ("drawRifle", "Draw Weapon", ANY),
        ("punch4", "Punch", ANY),
        ("reloadPistol", "Reload Pistol", ANY),
        ("reloadRifle", "Reload Rifle", ANY),
        ("reloadStream", "Reload Stream", ANY),
        ("reloadSuperball", "Reload Superball", ANY),
        ("reloadVandegraf", "Reload Vandegraf", ANY),
        ("reloadScramCannon", "Reload Scram Cannon", ANY),
        ("reloadMercuryBow", "Reload Mercury Bow", ANY),
        ("reloadScreamer", "Reload Screamer", ANY),
        ("pfPf", "Punch", ANY),
        ("pfPfPf", "Punch", ANY),
        ("plPl", "Punch", ANY),
        ("plPlPl", "Punch", ANY),
        ("prPr", "Punch", ANY),
        ("prPrPr", "Punch", ANY),
        ("pbPb", "Punch", ANY),
        ("pbPbPb", "Punch", ANY),
        ("pdPd", "Punch", ANY),
        ("pdPdPd", "Punch", ANY),
        ("kfKf", "Kick", ANY),
        ("kfKfKf", "Kick", ANY),
        ("klKl", "Kick", ANY),
        ("klKlKl", "Kick", ANY),
        ("krKr", "Kick", ANY),
        ("krKrKr", "Kick", ANY),
        ("kbKb", "Kick", ANY),
        ("kbKbKb", "Kick", ANY),
        ("kdKd", "Kick", ANY),
        ("kdKdKd", "Kick", ANY),
        ("startleLeft", "Startle", ANY),
        ("startleRight", "Startle", ANY),
        ("startleBack", "Startle", ANY),
        ("startleForward", "Startle", ANY),
        ("consoleWalk", "Walk", ANY),
        ("runStop", "Run", ANY),
        ("walkStop", "Walk", ANY),
        ("runStart", "Run", ANY),
        ("walkStart", "Walk", ANY),
        ("runBackwardsStart", "Run", ANY),
        ("walkBackwardsStart", "Walk", ANY),
        ("stun", "Run", ANY),
        ("staggerBehind", "Walk", ANY),
        ("oneStepStop", "Run", ANY),
        ("runSidestepLeftStart", "Run", ANY),
        ("runSidestepRightStart", "Run", ANY),
        ("powerup", "Powerup", ANY),
        ("fallingFlail", "Falling Flail", ANY),
        ("hitOverlayAi", "Getting Hit", ANY),
    ];
    TABLE
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|&(_, t, w)| (t, w))
}

struct Row {
    kind: String,
    weight: u8,
    impulse: String,
}

pub struct AnimSounds {
    /// Variant name → parent; the variants' instance names → names.
    parents: HashMap<String, Option<String>>,
    instances: HashMap<String, String>,
    rows: HashMap<String, Vec<Row>>,
    /// Clips with a sound of their own (whoever plays them): impulse and
    /// frame.
    clips: HashMap<String, (String, u16)>,
}

impl AnimSounds {
    pub fn load(common: &Path) -> Option<AnimSounds> {
        let text = std::fs::read_to_string(common.join("anim_sounds.json")).ok()?;
        let j: oni_schema::files::AnimSoundsFile = serde_json::from_str(&text)
            .map_err(|e| eprintln!("anim_sounds.json: {e}"))
            .ok()?;
        let mut a = AnimSounds {
            parents: j.variants.into_iter().collect(),
            instances: j.instances.into_iter().collect(),
            rows: HashMap::new(),
            clips: HashMap::new(),
        };
        for (variant, rows) in j.sounds {
            for r in rows {
                if let Some(clip) = r.clip {
                    // (Bound to the clip itself, for everyone.)
                    a.clips
                        .insert(clip, (r.impulse, r.frame.unwrap_or(0) as u16));
                } else {
                    a.rows.entry(variant.clone()).or_default().push(Row {
                        kind: r.kind,
                        weight: weight(&r.modifier),
                        impulse: r.impulse,
                    });
                }
            }
        }
        Some(a)
    }

    /// The best of a variant's rows of `kind` for `weight`: one of that
    /// weight, else the last of any weight.
    fn pick<'a>(rows: impl Iterator<Item = &'a Row>, weight: u8) -> Option<&'a str> {
        let mut found = None;
        for r in rows {
            if r.weight == ANY || r.weight == weight {
                found = Some(r.impulse.as_str());
                if r.weight == weight {
                    break;
                }
            }
        }
        found
    }

    /// The sound clip `clip` (of animation type `kind`, its blows up to
    /// `damage`) plays on `frame` for a character of variant `variant`
    /// (its instance name), if any.
    pub fn sound(
        &self,
        variant: Option<&str>,
        clip: &str,
        kind: &str,
        damage: u16,
        frame: u16,
    ) -> Option<&str> {
        if let Some((impulse, at)) = self.clips.get(clip) {
            return (*at == frame).then_some(impulse.as_str());
        }
        if frame != 0 {
            return None;
        }
        let (sound_kind, mut w) = sound_type(kind)?;
        if w == ANY {
            w = match damage {
                d if d >= 15 => HEAVY,
                d if d >= 10 => MEDIUM,
                0 => ANY,
                _ => LIGHT,
            };
        }
        let mut at = variant.and_then(|v| self.instances.get(v)).cloned();
        for _ in 0..32 {
            let v = at?;
            if let Some(rows) = self.rows.get(&v) {
                let found = Self::pick(rows.iter().filter(|r| r.kind == sound_kind), w)
                    .or_else(|| Self::pick(rows.iter().filter(|r| r.kind == "Any"), w));
                if found.is_some() {
                    return found;
                }
            }
            at = self.parents.get(&v).cloned().flatten();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sounds() -> AnimSounds {
        let j = serde_json::json!({
            "variants": { "Any": null, "Konoko": "Any", "Cop_Female": "Konoko" },
            "instances": { "konoko": "Konoko", "copfem": "Cop_Female" },
            "sounds": {
                "Any": [
                    { "type": "Kick", "modifier": "heavy damage", "impulse": "heavy_leg" },
                    { "type": "Kick", "modifier": "medium damage", "impulse": "med_leg" },
                    { "type": "Roll", "modifier": "any", "impulse": "kon_roll" }
                ],
                "Konoko": [
                    { "type": "Punch", "modifier": "any", "impulse": "kon_pnch_lt" },
                    { "type": "Animation", "modifier": "any", "impulse": "kon_yawn", "clip": "KONOKOidle_spec2", "frame": 0 }
                ]
            }
        });
        let dir = std::env::temp_dir().join(format!("oni-anim-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("anim_sounds.json"), j.to_string()).unwrap();
        AnimSounds::load(&dir).unwrap()
    }

    #[test]
    fn by_variant_type_and_weight() {
        let s = sounds();
        // Konoko's own punch; a cop inherits it.
        assert_eq!(
            s.sound(Some("konoko"), "x", "punchForward", 8, 0),
            Some("kon_pnch_lt")
        );
        assert_eq!(
            s.sound(Some("copfem"), "x", "punch", 8, 0),
            Some("kon_pnch_lt")
        );
        // Kicks come from Any, by the clip's damage.
        assert_eq!(
            s.sound(Some("konoko"), "x", "kickHeavy", 20, 0),
            Some("heavy_leg")
        );
        assert_eq!(s.sound(Some("konoko"), "x", "kick", 12, 0), Some("med_leg"));
        // A roll (crouch weight) from Any's any-weight row.
        assert_eq!(
            s.sound(Some("konoko"), "x", "crouchForward", 0, 0),
            Some("kon_roll")
        );
        // Only on frame 0; silent types stay silent.
        assert_eq!(s.sound(Some("konoko"), "x", "punch", 8, 3), None);
        assert_eq!(s.sound(Some("konoko"), "x", "stand", 0, 0), None);
        // A clip with its own sound, for anyone.
        assert_eq!(
            s.sound(None, "KONOKOidle_spec2", "idle", 0, 0),
            Some("kon_yawn")
        );
    }
}
