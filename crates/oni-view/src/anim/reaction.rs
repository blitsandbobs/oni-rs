//! How a hit's animation type is adjusted for the victim's situation
//! (behaviour per `docs/COMBAT.md` §4 and `docs/MOVEMENT.md`).
//!
//! An attack names the reaction it causes (`damageAnimation`: a head, body or
//! foot hit, or a knockdown). Before it plays, the reaction is adapted to the
//! victim: a hit lands as a knockdown on someone airborne, as a crouched hit on
//! someone crouching, as a "hit while down" on someone lying on the ground; a
//! knockdown-resistant character shrugs light hits off and is merely
//! staggered by knockdowns; and a hit from behind uses the behind variant.

use crate::anim::types::{st, ty};
use crate::quirks::Quirks;

/// Reaction clips: while one plays, the character is the victim of a hit and
/// ordinary requests can't cut it short.
pub fn is_victim(t: u16) -> bool {
    matches!(
        t,
        ty::HIT_HEAD
            | ty::HIT_BODY
            | ty::HIT_FOOT
            | ty::HIT_CROUCH
            | ty::HIT_FALLEN
            | ty::HIT_FOOT_OUCH
            | ty::HIT_JEWELS
            | ty::HIT_HEAD_BEHIND
            | ty::HIT_BODY_BEHIND
            | ty::HIT_FOOT_BEHIND
            | ty::HIT_CROUCH_BEHIND
            | ty::STUN
            | ty::STAGGER
            | ty::STAGGER_BEHIND
    ) || is_knockdown(t)
        || ty::is_thrown(t)
}

/// Reactions that put the character on the ground.
pub fn is_knockdown(t: u16) -> bool {
    matches!(
        t,
        ty::LAND_DEAD
            | ty::BLOWNUP
            | ty::BLOWNUP_BEHIND
            | ty::KNOCKDOWN_HEAD
            | ty::KNOCKDOWN_BODY
            | ty::KNOCKDOWN_FOOT
            | ty::KNOCKDOWN_HEAD_BEHIND
            | ty::KNOCKDOWN_BODY_BEHIND
            | ty::KNOCKDOWN_FOOT_BEHIND
            | ty::KNOCKDOWN_CROUCH
            | ty::KNOCKDOWN_CROUCH_BEHIND
    )
}

/// Reactions played facing away from the attacker.
pub fn is_behind(t: u16) -> bool {
    matches!(
        t,
        ty::HIT_HEAD_BEHIND
            | ty::HIT_BODY_BEHIND
            | ty::KNOCKDOWN_HEAD_BEHIND
            | ty::KNOCKDOWN_BODY_BEHIND
            | ty::KNOCKDOWN_FOOT_BEHIND
            | ty::HIT_CROUCH_BEHIND
            | ty::KNOCKDOWN_CROUCH_BEHIND
            | ty::STAGGER_BEHIND
            | ty::BLOWNUP_BEHIND
    )
}

/// The knockdown a reaction becomes when it must floor the victim (airborne,
/// or hit hard while already staggered). Hits map to the matching
/// knockdown; anything else falls to a frontal head knockdown — including
/// knockdowns that are already knockdowns, which the shipped game also sends
/// to the head variant (Q4 in `docs/QUIRKS.md`; fixed: they stay as they are).
pub fn to_knockdown(t: u16, q: Quirks) -> u16 {
    match t {
        ty::HIT_HEAD => ty::KNOCKDOWN_HEAD,
        ty::HIT_BODY => ty::KNOCKDOWN_BODY,
        ty::HIT_FOOT => ty::KNOCKDOWN_FOOT,
        ty::HIT_CROUCH => ty::KNOCKDOWN_CROUCH,
        ty::HIT_HEAD_BEHIND => ty::KNOCKDOWN_HEAD_BEHIND,
        ty::HIT_BODY_BEHIND => ty::KNOCKDOWN_BODY_BEHIND,
        ty::HIT_FOOT_BEHIND => ty::KNOCKDOWN_FOOT_BEHIND,
        ty::HIT_CROUCH_BEHIND => ty::KNOCKDOWN_CROUCH_BEHIND,
        _ if !q.faithful && is_knockdown(t) => t,
        _ => ty::KNOCKDOWN_HEAD,
    }
}

/// A crouching victim's version of a frontal hit or knockdown.
fn crouched(t: u16) -> u16 {
    match t {
        ty::HIT_HEAD | ty::HIT_BODY | ty::HIT_FOOT => ty::HIT_CROUCH,
        ty::KNOCKDOWN_HEAD | ty::KNOCKDOWN_BODY | ty::KNOCKDOWN_FOOT => ty::KNOCKDOWN_CROUCH,
        other => other,
    }
}

/// A knockdown-resistant character's version: light hits become a flinch
/// (an overlay, not a full-body clip), staggers soften to a body hit, and
/// knockdowns soften to a stagger.
fn resisted(t: u16) -> u16 {
    match t {
        ty::HIT_HEAD
        | ty::HIT_BODY
        | ty::HIT_FOOT
        | ty::HIT_JEWELS
        | ty::HIT_FOOT_OUCH
        | ty::HIT_CROUCH
        | ty::STUN => ty::HIT_OVERLAY,
        ty::STAGGER => ty::HIT_BODY,
        ty::KNOCKDOWN_HEAD
        | ty::KNOCKDOWN_BODY
        | ty::KNOCKDOWN_FOOT
        | ty::KNOCKDOWN_CROUCH
        | ty::BLOWNUP => ty::STAGGER,
        other => other,
    }
}

/// The victim's situation when a reaction is chosen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Victim {
    pub airborne: bool,
    /// The state the victim's current clip ends in.
    pub to_state: u16,
    /// Apply knockdown resistance (only when the hit lands, not when the
    /// chosen reaction is finally played).
    pub resistant: bool,
}

/// Adapts reaction `t` to the victim: airborne → knockdown (even through
/// resistance); crouched → crouched hit; lying down → hit while down; then
/// resistance.
pub fn adapt(t: u16, v: Victim, q: Quirks) -> u16 {
    if v.airborne && is_victim(t) {
        return to_knockdown(t, q);
    }
    let mut t = t;
    if v.to_state == st::CROUCH {
        t = crouched(t);
    }
    if matches!(v.to_state, st::FALLEN_FRONT | st::FALLEN_BACK) {
        t = ty::HIT_FALLEN;
    }
    if v.resistant {
        t = resisted(t);
    }
    t
}

/// The behind variant of a reaction (for a hit landing from more than 90°
/// off the victim's facing). Foot and groin hits share the body variant;
/// reactions without a behind variant are unchanged.
pub fn from_behind(t: u16) -> u16 {
    match t {
        ty::HIT_HEAD => ty::HIT_HEAD_BEHIND,
        ty::HIT_BODY | ty::HIT_FOOT | ty::HIT_FOOT_OUCH | ty::HIT_JEWELS => ty::HIT_BODY_BEHIND,
        ty::KNOCKDOWN_HEAD => ty::KNOCKDOWN_HEAD_BEHIND,
        ty::KNOCKDOWN_BODY => ty::KNOCKDOWN_BODY_BEHIND,
        ty::KNOCKDOWN_FOOT => ty::KNOCKDOWN_FOOT_BEHIND,
        ty::HIT_CROUCH => ty::HIT_CROUCH_BEHIND,
        ty::KNOCKDOWN_CROUCH => ty::KNOCKDOWN_CROUCH_BEHIND,
        ty::BLOWNUP => ty::BLOWNUP_BEHIND,
        ty::STAGGER => ty::STAGGER_BEHIND,
        other => other,
    }
}

/// The generic version of a reaction, tried when the specific clip is missing.
pub fn generic(t: u16) -> u16 {
    match t {
        ty::HIT_HEAD | ty::HIT_FOOT | ty::HIT_CROUCH | ty::HIT_FOOT_OUCH | ty::HIT_JEWELS => {
            ty::HIT_BODY
        }
        ty::HIT_HEAD_BEHIND | ty::HIT_FOOT_BEHIND | ty::HIT_CROUCH_BEHIND => ty::HIT_BODY_BEHIND,
        ty::KNOCKDOWN_HEAD | ty::KNOCKDOWN_FOOT | ty::KNOCKDOWN_CROUCH => ty::KNOCKDOWN_BODY,
        ty::KNOCKDOWN_HEAD_BEHIND | ty::KNOCKDOWN_FOOT_BEHIND | ty::KNOCKDOWN_CROUCH_BEHIND => {
            ty::KNOCKDOWN_BODY_BEHIND
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDING: Victim = Victim {
        airborne: false,
        to_state: st::STANDING,
        resistant: false,
    };

    #[test]
    fn a_standing_victim_takes_the_hit_as_named() {
        assert_eq!(adapt(ty::HIT_HEAD, STANDING, Quirks::FIXED), ty::HIT_HEAD);
        assert_eq!(adapt(ty::KNOCKDOWN_HEAD, STANDING, Quirks::FIXED), ty::KNOCKDOWN_HEAD);
    }

    #[test]
    fn situation_adapts_the_reaction() {
        let air = Victim { airborne: true, ..STANDING };
        assert_eq!(adapt(ty::HIT_BODY, air, Quirks::FIXED), ty::KNOCKDOWN_BODY);
        let crouch = Victim { to_state: st::CROUCH, ..STANDING };
        assert_eq!(adapt(ty::HIT_FOOT, crouch, Quirks::FIXED), ty::HIT_CROUCH);
        assert_eq!(adapt(ty::KNOCKDOWN_HEAD, crouch, Quirks::FIXED), ty::KNOCKDOWN_CROUCH);
        let down = Victim { to_state: st::FALLEN_BACK, ..STANDING };
        assert_eq!(adapt(ty::KNOCKDOWN_HEAD, down, Quirks::FIXED), ty::HIT_FALLEN);
        let tough = Victim { resistant: true, ..STANDING };
        assert_eq!(adapt(ty::HIT_HEAD, tough, Quirks::FIXED), ty::HIT_OVERLAY);
        assert_eq!(adapt(ty::KNOCKDOWN_BODY, tough, Quirks::FIXED), ty::STAGGER);
        // Airborne beats resistance.
        let tough_air = Victim { airborne: true, resistant: true, ..STANDING };
        assert_eq!(adapt(ty::HIT_HEAD, tough_air, Quirks::FIXED), ty::KNOCKDOWN_HEAD);
    }

    #[test]
    fn behind_variants() {
        assert_eq!(from_behind(ty::HIT_FOOT), ty::HIT_BODY_BEHIND);
        assert_eq!(from_behind(ty::STAGGER), ty::STAGGER_BEHIND);
        assert_eq!(from_behind(ty::HIT_FALLEN), ty::HIT_FALLEN);
        assert!(is_behind(ty::KNOCKDOWN_HEAD_BEHIND) && !is_behind(ty::HIT_HEAD));
    }

    #[test]
    fn knockdown_upgrade_quirk() {
        assert_eq!(to_knockdown(ty::KNOCKDOWN_FOOT, Quirks::FIXED), ty::KNOCKDOWN_FOOT);
        assert_eq!(to_knockdown(ty::KNOCKDOWN_FOOT, Quirks::FAITHFUL), ty::KNOCKDOWN_HEAD);
        assert_eq!(to_knockdown(ty::STAGGER, Quirks::FIXED), ty::KNOCKDOWN_HEAD);
    }
}
