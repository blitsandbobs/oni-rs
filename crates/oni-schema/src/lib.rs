//! The named vocabulary of the converted assets (`docs/ASSET_FORMAT.md`).
//!
//! The original data stores enumerations as numbers (animation type 31,
//! state 7, variant bit `0x200`). The converted assets spell them out as
//! camelCase names (`"punchForward"`, `"standing"`, `["fight"]`) so they read
//! on their own; the converter writes names with these tables and the engine
//! reads them back to numbers with the same tables. Names follow the
//! original identifiers, so they line up with the original game's docs and
//! tools.
//!
//! Single values use a table indexed by value ([`name`] / [`value`]); bit
//! sets use a table indexed by bit ([`bit_names`] / [`bits`]).

mod tables;

pub use tables::{ANIM_STATES, ANIM_TYPES};

/// Variant bits a clip can require (`ONcAnimVarientMask_*`), by bit.
pub const VARIANTS: [&str; 16] = [
    "", "", "", "", "", "", "", "", // bits 0–7 unused
    "sprint", "fight", "shoulder", "rightyPistol", "leftyPistol", "rightyRifle", "leftyRifle",
    "panic",
];

/// Per-clip flags (`TRcAnimFlag_*` / `ONcAnimFlag_*`), by bit.
pub const ANIM_FLAGS: [&str; 24] = [
    "prepared",
    "invulnerable",
    "blockHigh",
    "blockLow",
    "attack",
    "dropWeapon",
    "inAir",
    "atomic",
    "noTurn",
    "aimForward",
    "aimLeft",
    "aimRight",
    "aimBackward",
    "overlay",
    "noInterpVelocity",
    "throwSource",
    "throwTarget",
    "realWorld",
    "doAim",
    "dontAim",
    "canPickup",
    "aim360",
    "disableShield",
    "noAiPickup",
];

/// Attack flags (`ONcAttackFlag_*`), by bit.
pub const ATTACK_FLAGS: [&str; 4] = ["unblockable", "high", "low", "special"];

/// Shortcut flags, by bit: `interruptsAtomic` may cut into an uninterruptible
/// clip (the original's "nuke").
pub const SHORTCUT_FLAGS: [&str; 1] = ["interruptsAtomic"];

/// The direction a clip moves the character (`TRtDirection`), by value.
pub const DIRECTIONS: [&str; 6] = ["none", "forward", "back", "left", "right", "any"];

/// Voice lines a clip can trigger (`AI2cVocalization_*`), by value.
pub const VOCALIZATIONS: [&str; 10] = [
    "taunt",
    "alert",
    "startle",
    "checkBody",
    "pursue",
    "cower",
    "superPunch",
    "superKick",
    "super3",
    "super4",
];

/// A character's body parts in skeleton order (the node names of the
/// converted character models); bit *i* of a part mask = part *i*.
pub const BODY_PARTS: [&str; 19] = [
    "pelvis",
    "left_thigh",
    "left_calf",
    "left_foot",
    "right_thigh",
    "right_calf",
    "right_foot",
    "mid",
    "chest",
    "neck",
    "head",
    "left_shoulder",
    "left_biceps",
    "left_wrist",
    "left_handfist",
    "right_shoulder",
    "right_biceps",
    "right_wrist",
    "right_handfist",
];

// --- AI melee profiles (`melee_profiles.json`) -------------------------------
//
// A melee technique is a list of moves; each move names an entry of one of
// these tables (the original's tables, in the original's order). Attack moves
// keep the original's combo notation (P = punch, K = kick, F/L/R/B/D =
// forward/left/right/back/down, H = heavy, C = crouched, CS = from crouch
// start, R = running, J = jumping); where the original lists a name twice the
// second asks for a different clip and is named apart here.

/// Attack moves, by index.
pub const MELEE_ATTACKS: [&str; 84] = [
    "P", "P P", "P P P", "P P P P", "PF", "PL", "PR", "PB", "PD", "PF PF", "PF PF PF", "PL PL",
    "PL PL PL", "PR PR", "PR PR PR", "PB PB", "PB PB PB", "PD PD", "PD PD PD", "K", "K K",
    "K K K", "K K KF", "KF", "KL", "KR", "KB", "KD", "KF KF", "KF KF KF", "KL KL", "KL KL KL",
    "KR KR", "KR KR KR", "KB KB", "KB KB KB", "KD KD", "KD KD KD", "P P K", "P K K", "P K P",
    "K P K", "K P P", "K K P", "P K", "K P", "P P K K", "P P K K K", "P P K K K K", "HP", "HPF",
    "HK", "HKF", "CS-P", "CS-K", "C-P", "C-P (low)", "C-PF", "C-K", "C-K (low)", "C-KF",
    "GETUP-KF", "GETUP-KB", "R-P", "R-K", "RB-P", "RB-K", "RL-P", "RL-K", "RR-P", "RR-K",
    "R-SLIDE", "J-P", "J-K", "JF-P", "JF-PB", "JF-K", "JF-KB", "JB-P", "JB-K", "JL-P", "JL-K",
    "JR-P", "JR-K",
];

/// Position moves (get into the state an attack starts from), by index.
pub const MELEE_POSITIONS: [&str; 20] = [
    "runForward", "runLeft", "runRight", "runBack", "jumpUp", "jumpForward", "jumpLeft",
    "jumpRight", "jumpBack", "startToCrouch", "crouch", "stand", "closeForward", "closeLeft",
    "closeRight", "closeBack", "runJumpForward", "runJumpLeft", "runJumpRight", "runJumpBack",
];

/// Maneuvers (movement and pauses within a technique), by index.
pub const MELEE_MANEUVERS: [&str; 14] = [
    "advance", "retreat", "circleLeft", "circleRight", "pause", "crouch", "jump", "taunt",
    "randomStop", "getUpForward", "getUpBack", "getUpRollLeft", "getUpRollRight",
    "barabbasWave",
];

/// Evasions, by index (an `…Alt` asks for the direction-specific jump clip).
pub const MELEE_EVADES: [&str; 24] = [
    "jumpForward", "jumpForwardAlt", "jumpBack", "jumpBackAlt", "jumpLeft", "jumpLeftAlt",
    "jumpRight", "jumpRightAlt", "runJumpForward", "runJumpForwardAlt", "runJumpBack",
    "runJumpBackAlt", "runJumpLeft", "runJumpLeftAlt", "runJumpRight", "runJumpRightAlt",
    "rollForward", "rollBack", "rollLeft", "rollRight", "slideForward", "slideBack", "slideLeft",
    "slideRight",
];

/// Throws, by index.
pub const MELEE_THROWS: [&str; 25] = [
    "punchFront", "kickFront", "punchBehind", "kickBehind", "runPunchFront", "runKickFront",
    "runPunchBehind", "runKickBehind", "punchFrontDisarm", "kickFrontDisarm",
    "punchBehindDisarm", "kickBehindDisarm", "runPunchFrontDisarm", "runKickFrontDisarm",
    "runPunchBehindDisarm", "runKickBehindDisarm", "punchFrontRifleDisarm",
    "kickFrontRifleDisarm", "punchBehindRifleDisarm", "kickBehindRifleDisarm",
    "runPunchFrontRifleDisarm", "runKickFrontRifleDisarm", "runPunchBehindRifleDisarm",
    "runKickBehindRifleDisarm", "tackle",
];

/// Technique flags, by bit.
pub const TECHNIQUE_FLAGS: [&str; 3] = ["interruptible", "generousDirection", "fearless"];

/// The name of `value` in a by-value table.
pub fn name(table: &[&'static str], value: u16) -> Option<&'static str> {
    table.get(usize::from(value)).copied().filter(|n| !n.is_empty())
}

/// The value named `name` in a by-value table.
pub fn value(table: &[&str], name: &str) -> Option<u16> {
    table
        .iter()
        .position(|n| !n.is_empty() && *n == name)
        .and_then(|i| u16::try_from(i).ok())
}

/// The names of the set bits of `bits` in a by-bit table, lowest bit first.
/// Bits the table doesn't name are dropped.
pub fn bit_names(table: &[&'static str], bits: u32) -> Vec<&'static str> {
    table
        .iter()
        .enumerate()
        .filter(|&(i, n)| i < 32 && bits & (1 << i) != 0 && !n.is_empty())
        .map(|(_, n)| *n)
        .collect()
}

/// The bit set named by `names` in a by-bit table (unknown names ignored).
pub fn bits<'a>(table: &[&str], names: impl IntoIterator<Item = &'a str>) -> u32 {
    names
        .into_iter()
        .filter_map(|n| table.iter().position(|t| !t.is_empty() && *t == n))
        .filter(|&i| i < 32)
        .fold(0, |acc, i| acc | (1 << i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values_have_their_names() {
        assert_eq!(name(&ANIM_TYPES, 31), Some("punchForward"));
        assert_eq!(name(&ANIM_TYPES, 106), Some("throwForwardPunch"));
        assert_eq!(name(&ANIM_TYPES, 96), Some("thrown1"));
        assert_eq!(name(&ANIM_STATES, 7), Some("standing"));
        assert_eq!(name(&ANIM_STATES, 50), Some("fallenFront"));
        assert_eq!(value(&ANIM_STATES, "falling"), Some(38));
        assert_eq!(bit_names(&VARIANTS, 0x0200), vec!["fight"]);
        assert_eq!(bit_names(&ANIM_FLAGS, 1 << 15), vec!["throwSource"]);
        assert_eq!(name(&DIRECTIONS, 2), Some("back"));
    }

    #[test]
    fn names_round_trip() {
        for (i, n) in ANIM_TYPES.iter().enumerate() {
            assert_eq!(value(&ANIM_TYPES, n), Some(i as u16), "{n}");
        }
        for (i, n) in ANIM_STATES.iter().enumerate() {
            assert_eq!(value(&ANIM_STATES, n), Some(i as u16), "{n}");
        }
        let mask = 0x0200 | 0x0100 | 0x8000;
        assert_eq!(bits(&VARIANTS, bit_names(&VARIANTS, mask)), mask);
        assert_eq!(bits(&BODY_PARTS, bit_names(&BODY_PARTS, 0x6_0000)), 0x6_0000);
    }

    #[test]
    fn names_are_unique() {
        for t in [
            &ANIM_TYPES[..],
            &ANIM_STATES[..],
            &ANIM_FLAGS[..],
            &BODY_PARTS[..],
            &MELEE_ATTACKS[..],
            &MELEE_POSITIONS[..],
            &MELEE_MANEUVERS[..],
            &MELEE_EVADES[..],
            &MELEE_THROWS[..],
        ] {
            let mut seen = std::collections::HashSet::new();
            assert!(t.iter().all(|n| seen.insert(*n)));
        }
    }
}
