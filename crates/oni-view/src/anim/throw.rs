//! Throws (behaviour per `docs/COMBAT.md` §6).
//!
//! A player's forward punch or kick, and running punch or kick, becomes a
//! throw at the moment the strike would start, if someone can be thrown:
//!
//! * **Who.** Among the throwable opponents within reach and within a cone in
//!   front of where the thrower aims, the one closest to that aim line (not
//!   the nearest). If that one is momentarily invulnerable there's no throw.
//! * **Which way.** Facing each other (more than a right angle apart) means a
//!   front throw; facing the same way, a throw from behind.
//! * **Which clips.** Every thrower clip of that throw type starting from the
//!   thrower's state, whose variant the thrower has (fight stance allowed), is
//!   paired with the victim clip its throw data names, looked up in the
//!   *thrower's* animation set from the victim's state. A pair counts only if
//!   the victim is within that clip's range. The most specific pair wins — the
//!   victim clip's variant first, then the thrower's — and a pair needs some
//!   variant at all.
//!
//! Without a throw the strike plays as normal.

use glam::Vec3;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use crate::anim::collection::AnimCollection;
use crate::anim::types::{ty, var};

/// How far away (3D, feet to feet) a throw victim may be at most.
pub const SEARCH_RANGE: f32 = 24.0;
/// How far off the thrower's aim a throw victim may be.
pub const SEARCH_ARC: f32 = FRAC_PI_4;
/// Blend length into the victim's clip (ticks).
pub const VICTIM_BLEND: u16 = 6;

/// Someone who could be thrown this tick, as the thrower sees them.
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    /// Fighter index.
    pub who: usize,
    /// Their feet minus the thrower's (world space).
    pub offset: Vec3,
    /// Their body facing (radians).
    pub facing: f32,
    /// Their animation state and variant bits.
    pub state: u16,
    pub varient: u16,
    /// Inside their current clip's invulnerable window.
    pub invulnerable: bool,
}

/// What a would-be thrower can see: its own facings and the candidates.
/// (Characters that can never be thrown — the dead, the knockdown-resistant,
/// anyone in a wholly invulnerable clip — are left out by the caller.)
#[derive(Clone, Debug, Default)]
pub struct Sense {
    pub aim: f32,
    pub facing: f32,
    pub candidates: Vec<Candidate>,
    /// An AI's ordered throw: any strike it starts becomes this throw type.
    pub only: Option<u16>,
    /// …and with the victim's side in one of these variants (a disarm: its
    /// gun's; 0 any).
    pub victim_needs: u16,
}

/// A throw that has just begun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Started {
    /// The victim's fighter index.
    pub target: usize,
    /// The victim's clip (an index into the *thrower's* collection).
    pub target_clip: usize,
    /// How far the thrower turns to face the victim squarely (radians).
    pub turn: f32,
}

fn wrap(x: f32) -> f32 {
    (x + PI).rem_euclid(TAU) - PI
}

/// The front / behind throw types a strike can become, if any.
pub fn throw_types(strike: u16) -> Option<(u16, u16)> {
    match strike {
        ty::PUNCH_FORWARD => Some((ty::THROW_FORWARD_PUNCH, ty::THROW_BEHIND_PUNCH)),
        ty::KICK_FORWARD => Some((ty::THROW_FORWARD_KICK, ty::THROW_BEHIND_KICK)),
        ty::RUN_PUNCH => Some((ty::RUN_THROW_FORWARD_PUNCH, ty::RUN_THROW_BEHIND_PUNCH)),
        ty::RUN_KICK => Some((ty::RUN_THROW_FORWARD_KICK, ty::RUN_THROW_BEHIND_KICK)),
        _ => None,
    }
}

/// Picks the throw victim: the candidate in reach closest to the aim line.
/// Returns it with its bearing off the aim.
fn pick(sense: &Sense) -> Option<(&Candidate, f32)> {
    sense
        .candidates
        .iter()
        .filter(|c| c.offset.length() <= SEARCH_RANGE)
        .map(|c| (c, wrap(c.offset.x.atan2(c.offset.z) - sense.aim)))
        .filter(|(_, a)| a.abs() <= SEARCH_ARC)
        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
}

/// Tries to turn `strike` (a clip the thrower in `state` with `varient` is
/// about to start) into a throw. Returns the thrower's throw clip and what
/// it starts.
pub fn attempt(
    coll: &AnimCollection,
    strike: u16,
    state: u16,
    varient: u16,
    sense: &Sense,
) -> Option<(usize, Started)> {
    let (front, behind) = match sense.only {
        Some(t) => (t, t),
        None => throw_types(strike)?,
    };
    let (victim, bearing) = pick(sense)?;
    if victim.invulnerable {
        return None;
    }
    let kind = if wrap(victim.facing - sense.facing).abs() > FRAC_PI_2 {
        front
    } else {
        behind
    };
    let distance = victim.offset.length();
    let mine = varient | var::FIGHT;
    let theirs = victim.varient | var::FIGHT;

    let mut best: Option<(u32, usize, usize)> = None; // (score, mine, theirs)
    for clip in coll.all(kind, state) {
        let c = &coll.entries[clip].clip;
        if c.varient & !mine != 0 {
            continue;
        }
        let Some(t) = c.throw else { continue };
        if distance > t.range {
            continue;
        }
        let Some(target_clip) = coll.lookup(t.target_anim_type, victim.state, theirs) else {
            continue;
        };
        if sense.victim_needs != 0 && coll.entries[target_clip].clip.varient & sense.victim_needs == 0 {
            continue;
        }
        let score = (u32::from(coll.entries[target_clip].clip.varient) << 16) | u32::from(c.varient);
        if score > best.map_or(0, |b| b.0) {
            best = Some((score, clip, target_clip));
        }
    }
    let (_, clip, target_clip) = best?;
    Some((
        clip,
        Started {
            target: victim.who,
            target_clip,
            turn: bearing,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::collection::tests::konoko;
    use crate::anim::types::st;

    fn facing_me(at: f32, facing: f32) -> Sense {
        Sense {
            aim: 0.0,
            facing: 0.0,
            candidates: vec![Candidate {
                who: 1,
                offset: Vec3::new(0.0, 0.0, at),
                facing,
                state: st::STANDING,
                varient: 0,
                invulnerable: false,
            }],
            only: None,
            victim_needs: 0,
        }
    }

    #[test]
    fn forward_punch_becomes_a_front_throw() {
        let Some(c) = konoko() else { return };
        let (clip, s) = attempt(&c, ty::PUNCH_FORWARD, st::STANDING, 0, &facing_me(8.0, PI))
            .expect("a throw");
        assert_eq!(c.entries[clip].clip.anim_type, ty::THROW_FORWARD_PUNCH);
        assert_eq!(s.target, 1);
        assert!(ty::is_thrown(c.entries[s.target_clip].clip.anim_type));
    }

    #[test]
    fn same_facing_is_a_throw_from_behind() {
        let Some(c) = konoko() else { return };
        let (clip, _) = attempt(&c, ty::KICK_FORWARD, st::STANDING, 0, &facing_me(8.0, 0.0))
            .expect("a throw");
        assert_eq!(c.entries[clip].clip.anim_type, ty::THROW_BEHIND_KICK);
    }

    #[test]
    fn out_of_reach_or_off_aim_is_no_throw() {
        let Some(c) = konoko() else { return };
        // Beyond every punch throw's range (the search reaches further).
        assert!(attempt(&c, ty::PUNCH_FORWARD, st::STANDING, 0, &facing_me(20.0, PI)).is_none());
        let mut side = facing_me(8.0, PI);
        side.candidates[0].offset = Vec3::new(8.0, 0.0, 0.0); // 90° off the aim
        assert!(attempt(&c, ty::PUNCH_FORWARD, st::STANDING, 0, &side).is_none());
        // Other strikes never throw.
        assert!(attempt(&c, ty::PUNCH, st::STANDING, 0, &facing_me(8.0, PI)).is_none());
    }

    #[test]
    fn closest_to_aim_wins_not_nearest() {
        let Some(c) = konoko() else { return };
        let mut s = facing_me(9.0, PI);
        s.candidates.push(Candidate {
            who: 2,
            offset: Vec3::new(4.0, 0.0, 5.0), // nearer, but ~39° off the aim
            ..s.candidates[0]
        });
        let (_, started) = attempt(&c, ty::PUNCH_FORWARD, st::STANDING, 0, &s).expect("a throw");
        assert_eq!(started.target, 1);
    }
}
