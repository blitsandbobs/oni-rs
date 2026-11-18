//! Which animation a character tries for a given input, by situation.
//!
//! A rule names the controls that must have just been pressed and those that
//! must be held (plus, for combo follow-ups, which attack must have come
//! before), and the animation type to try. Rules are tried in order and the
//! first one whose animation exists for the character's current body state
//! wins — so one input resolves to a standing, running, crouched or airborne
//! version of a move purely by which clips the data provides. The lists are
//! built from compact descriptions below; their order is the game's
//! precedence (see `docs/MOVEMENT.md`).

use crate::anim::input::key as k;
use crate::anim::types::ty::*;
use crate::quirks::Quirks;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Controls that must have gone down this tick.
    pub press: u32,
    /// Controls that must be held.
    pub hold: u32,
    /// Required previous attack (`NONE` = no requirement).
    pub after: u16,
    /// Animation type to try.
    pub anim: u16,
    /// An attack the player may land a few frames late (see `Motion::request`).
    pub late_ok: bool,
}

const fn rule(anim: u16) -> Rule {
    Rule {
        press: 0,
        hold: 0,
        after: NONE,
        anim,
        late_ok: false,
    }
}

impl Rule {
    const fn press(mut self, m: u32) -> Rule {
        self.press = m;
        self
    }
    const fn hold(mut self, m: u32) -> Rule {
        self.hold = m;
        self
    }
    const fn after(mut self, t: u16) -> Rule {
        self.after = t;
        self
    }
    const fn late(mut self) -> Rule {
        self.late_ok = true;
        self
    }
}

/// Fighting moves (strikes, rolls, combos, the mid-air flip), in precedence
/// order: special follow-ups, then directional strikes, rolls, moving strikes,
/// combo continuations, and finally the plain starters.
pub fn fighting() -> Vec<Rule> {
    let mut r = Vec::new();

    // Special follow-ups that need a held direction.
    r.push(rule(KICK3_FORWARD).press(k::KICK).hold(k::FORWARD).after(KICK2).late());
    r.push(rule(GETUP_KICK_BACK).press(k::KICK).hold(k::BACKWARD).late());

    // A strike with a direction (or crouch) held picks the directional version.
    for (dir, anim) in [
        (k::FORWARD, PUNCH_FORWARD),
        (k::STEP_LEFT, PUNCH_LEFT),
        (k::STEP_RIGHT, PUNCH_RIGHT),
        (k::BACKWARD, PUNCH_BACK),
        (k::CROUCH, PUNCH_LOW),
    ] {
        r.push(rule(anim).press(k::PUNCH).hold(dir).late());
    }
    for (dir, anim) in [
        (k::FORWARD, KICK_FORWARD),
        (k::STEP_LEFT, KICK_LEFT),
        (k::BACKWARD, KICK_BACK),
        (k::STEP_RIGHT, KICK_RIGHT),
        (k::CROUCH, KICK_LOW),
    ] {
        r.push(rule(anim).press(k::KICK).hold(dir).late());
    }

    // Rolls: crouch tapped with a direction held, or a direction tapped while
    // crouched.
    let rolls = [
        (k::FORWARD, CROUCH_FORWARD),
        (k::BACKWARD, CROUCH_BACK),
        (k::STEP_LEFT, CROUCH_LEFT),
        (k::STEP_RIGHT, CROUCH_RIGHT),
    ];
    for (dir, anim) in rolls {
        r.push(rule(anim).press(k::CROUCH).hold(dir).late());
    }
    for (dir, anim) in rolls {
        r.push(rule(anim).press(dir).hold(k::CROUCH).late());
    }

    // Strikes out of a run / backpedal / sidestep.
    for (dir, punch, kick) in [
        (k::FORWARD, RUN_PUNCH, RUN_KICK),
        (k::BACKWARD, RUN_BACK_PUNCH, RUN_BACK_KICK),
        (k::STEP_LEFT, SIDESTEP_LEFT_PUNCH, SIDESTEP_LEFT_KICK),
        (k::STEP_RIGHT, SIDESTEP_RIGHT_PUNCH, SIDESTEP_RIGHT_KICK),
    ] {
        r.push(rule(punch).press(k::PUNCH).hold(dir));
        r.push(rule(kick).press(k::KICK).hold(dir));
    }

    // Combo continuations: (previous attack, button) → next. Each pair is
    // unique, so their relative order does not matter.
    for (prev, button, next) in COMBO_CHAINS {
        r.push(rule(*next).press(*button).after(*prev));
    }

    // Unarmed starters (the rifle punch only resolves when holding a rifle).
    r.push(rule(PUNCH_RIFLE).press(k::PUNCH));
    r.push(rule(PUNCH).press(k::PUNCH));
    r.push(rule(KICK).press(k::KICK));
    r.push(rule(FLIP).press(k::CROUCH)); // only airborne states have a flip clip
    r
}

/// The combo graph: from a previous attack, a punch or kick press leads to
/// the next attack in the chain.
const COMBO_CHAINS: &[(u16, u32, u16)] = &[
    (PUNCH, k::PUNCH, PUNCH2),
    (PUNCH, k::KICK, PK),
    (PUNCH2, k::PUNCH, PUNCH3),
    (PUNCH2, k::KICK, PPK),
    (PUNCH3, k::PUNCH, PUNCH4),
    (KICK, k::KICK, KICK2),
    (KICK, k::PUNCH, KP),
    (KICK2, k::KICK, KICK3),
    (KICK2, k::PUNCH, KKP),
    (PK, k::PUNCH, PKP),
    (PK, k::KICK, PKK),
    (KP, k::PUNCH, KPP),
    (KP, k::KICK, KPK),
    (PPK, k::KICK, PPKK),
    (PPKK, k::KICK, PPKKK),
    (PPKKK, k::KICK, PPKKKK),
];

/// The gait clips for one direction: (start clip, loop clip).
struct Gait {
    run: (u16, u16),
    walk: (u16, u16),
    crouch_run: u16,
    crouch_walk: u16,
}

const AHEAD: Gait = Gait {
    run: (RUN_START, RUN),
    walk: (WALK_START, WALK),
    crouch_run: CROUCH_RUN,
    crouch_walk: CROUCH_WALK,
};
const BACK: Gait = Gait {
    run: (RUN_BACKWARDS_START, RUN_BACKWARDS),
    walk: (WALK_BACKWARDS_START, WALK_BACKWARDS),
    crouch_run: CROUCH_RUN_BACKWARDS,
    crouch_walk: CROUCH_WALK_BACKWARDS,
};

/// Moving forward/back while standing: walking when the walk modifier is held,
/// else running, falling back to walking if the run is unavailable.
pub fn move_upright() -> Vec<Rule> {
    let mut r = Vec::new();
    for (dir, g) in [(k::FORWARD, &AHEAD), (k::BACKWARD, &BACK)] {
        r.push(rule(g.walk.0).hold(dir | k::WALK));
        r.push(rule(g.walk.1).hold(dir | k::WALK));
    }
    for (dir, g) in [(k::FORWARD, &AHEAD), (k::BACKWARD, &BACK)] {
        for a in [g.run.0, g.run.1, g.walk.0, g.walk.1] {
            r.push(rule(a).hold(dir));
        }
    }
    r
}

/// Moving forward/back with movement restricted: walking only.
pub fn move_restricted() -> Vec<Rule> {
    [(k::FORWARD, &AHEAD), (k::BACKWARD, &BACK)]
        .iter()
        .flat_map(|(dir, g)| [rule(g.walk.0).hold(*dir), rule(g.walk.1).hold(*dir)])
        .collect()
}

/// Moving forward/back while crouched.
pub fn move_crouched() -> Vec<Rule> {
    let mut r = Vec::new();
    for (dir, g) in [(k::FORWARD, &AHEAD), (k::BACKWARD, &BACK)] {
        r.push(rule(g.crouch_walk).hold(k::CROUCH | k::WALK | dir));
        r.push(rule(g.crouch_run).hold(k::CROUCH | dir));
        r.push(rule(g.crouch_walk).hold(k::CROUCH | dir));
    }
    r
}

/// The sidestep clips for one side.
struct Side {
    key: u32,
    start: u16,
    run: u16,
    walk: u16,
    crouch_run: u16,
    crouch_walk: u16,
}

const SIDES: [Side; 2] = [
    Side {
        key: k::STEP_LEFT,
        start: RUN_SIDESTEP_LEFT_START,
        run: RUN_SIDESTEP_LEFT,
        walk: WALK_SIDESTEP_LEFT,
        crouch_run: CROUCH_RUN_SIDESTEP_LEFT,
        crouch_walk: CROUCH_WALK_SIDESTEP_LEFT,
    },
    Side {
        key: k::STEP_RIGHT,
        start: RUN_SIDESTEP_RIGHT_START,
        run: RUN_SIDESTEP_RIGHT,
        walk: WALK_SIDESTEP_RIGHT,
        crouch_run: CROUCH_RUN_SIDESTEP_RIGHT,
        crouch_walk: CROUCH_WALK_SIDESTEP_RIGHT,
    },
];

/// Sidestepping at a run (crouched sidesteps first).
pub fn sidestep_run() -> Vec<Rule> {
    let mut r: Vec<Rule> = SIDES
        .iter()
        .map(|s| rule(s.crouch_run).hold(k::CROUCH | s.key))
        .collect();
    for s in &SIDES {
        for a in [s.start, s.run, s.walk] {
            r.push(rule(a).hold(s.key));
        }
    }
    r
}

/// Sidestepping at a walk. `restricted` replaces the crouched sidestep with a
/// plain walking one.
pub fn sidestep_walk(restricted: bool) -> Vec<Rule> {
    let mut r: Vec<Rule> = SIDES
        .iter()
        .map(|s| {
            let a = if restricted { s.walk } else { s.crouch_walk };
            rule(a).hold(k::CROUCH | k::WALK | s.key)
        })
        .collect();
    for s in &SIDES {
        r.push(rule(s.walk).hold(s.key | k::WALK));
        r.push(rule(s.walk).hold(s.key));
    }
    r
}

/// Coming to a halt from locomotion.
pub fn stop() -> Vec<Rule> {
    vec![
        rule(ONE_STEP_STOP),
        rule(RUN_STOP),
        rule(WALK_STOP),
        rule(CROUCH).hold(k::CROUCH),
        rule(STAND),
    ]
}

/// Recovering from a stagger, stun or block.
pub fn recover() -> Vec<Rule> {
    vec![rule(CROUCH).hold(k::CROUCH), rule(STAND), rule(CROUCH)]
}

/// Settling when nothing else applies.
pub fn settle() -> Vec<Rule> {
    vec![
        rule(ONE_STEP_STOP),
        rule(RUN_STOP),
        rule(WALK_STOP),
        rule(CROUCH).hold(k::CROUCH),
        rule(PRONE).hold(k::CROUCH),
        rule(STAND),
        rule(CROUCH),
        rule(PRONE),
    ]
}

/// Ducking while crouch is held.
pub fn duck() -> Vec<Rule> {
    vec![rule(CROUCH).hold(k::CROUCH)]
}

/// Standing up.
pub fn rise() -> Vec<Rule> {
    vec![rule(STAND)]
}

/// Turning in place (crouched first if crouch is held).
pub fn turn_in_place(left: bool) -> Vec<Rule> {
    let (crouch, stand) = if left {
        (CROUCH_TURN_LEFT, STANDING_TURN_LEFT)
    } else {
        (CROUCH_TURN_RIGHT, STANDING_TURN_RIGHT)
    };
    vec![rule(crouch).hold(k::CROUCH), rule(stand), rule(crouch)]
}

/// Crouch pressed mid-run.
pub fn slide() -> Vec<Rule> {
    vec![rule(SLIDE).press(k::CROUCH)]
}

/// The direction-dependent jump takeoff.
pub fn jump(q: Quirks) -> Vec<Rule> {
    vec![
        rule(JUMP_FORWARD).hold(k::FORWARD),
        rule(JUMP_LEFT).hold(k::STEP_LEFT),
        rule(q.jump_right()).hold(k::STEP_RIGHT),
        rule(JUMP_BACKWARD).hold(k::BACKWARD),
        rule(JUMP),
    ]
}

/// Touching down, by held direction; `hard` after a damaging fall.
pub fn land(hard: bool) -> Vec<Rule> {
    let set = if hard {
        [LAND_FORWARD_HARD, LAND_LEFT_HARD, LAND_RIGHT_HARD, LAND_BACK_HARD, LAND_HARD]
    } else {
        [LAND_FORWARD, LAND_LEFT, LAND_RIGHT, LAND_BACK, LAND]
    };
    vec![
        rule(set[0]).hold(k::FORWARD),
        rule(set[1]).hold(k::STEP_LEFT),
        rule(set[2]).hold(k::STEP_RIGHT),
        rule(set[3]).hold(k::BACKWARD),
        rule(set[4]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_follow_the_directional_strikes() {
        let f = fighting();
        let first_combo = f.iter().position(|r| r.after != NONE && r.hold == 0).unwrap();
        let last_directional = f.iter().rposition(|r| r.anim == KICK_LOW).unwrap();
        assert!(last_directional < first_combo);
        assert_eq!(f.last().unwrap().anim, FLIP, "the flip is the final fallback");
    }

    #[test]
    fn faithful_jump_right_asks_for_the_landing_type() {
        assert!(jump(Quirks::FAITHFUL).iter().any(|r| r.anim == LAND_RIGHT));
        assert!(jump(Quirks::FIXED).iter().any(|r| r.anim == JUMP_RIGHT));
    }
}
