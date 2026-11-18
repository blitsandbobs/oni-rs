//! Each tick, turn a character's controls into animation requests.
//!
//! The decision runs in stages and stops at the first stage that settles
//! what the character does (behaviour per `docs/MOVEMENT.md`):
//!
//! 1. **React** — landing, a long fall, stuns, recovering from a stun.
//! 2. **Act** — sliding out of a run, strikes / rolls / the air flip, jumps.
//! 3. **Commit** — an action already queued is not overridden by movement.
//! 4. **Locomote** — run / walk / crouch-move, sidestep, crouch or stand up,
//!    turn in place, stop.
//! 5. **Settle** — keep flying while airborne, fidget when idle, else rest.
//!
//! Afterwards: a press that only queued something is dropped unless the clip
//! is about to end (so inputs buffer just before a clip finishes), holding
//! jump early in a jump adds lift, the fight-stance variant is upgraded, and
//! the body turns toward where the player wants to face.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

use crate::anim::input::{key, Buttons};
use crate::anim::motion::{Motion, Urge};
use crate::anim::reaction;
use crate::anim::rules::{self, Rule};
use crate::anim::types::{flag, st, ty, var};
use crate::control::{Facing, TURN_DEADZONE};
use crate::quirks::Quirks;
use crate::skel::Character;

/// Queued requests only stick this close (in frames) to the clip's end.
const BUFFER_FRAMES: u16 = 8;
/// Airborne ticks after which the character starts flailing.
const FLAIL_AFTER: u16 = 90;

/// What the decision knows besides the character itself.
pub struct Controls<'a> {
    pub buttons: &'a Buttons,
    pub quirks: Quirks,
    /// A player character (late attack presses allowed; turns only via turn clips
    /// while standing).
    pub player: bool,
    /// Movement restricted to walking (no attacks or jumps).
    pub restricted: bool,
    /// Maximum body turn this tick (radians).
    pub turn_cap: f32,
}

/// Runs one tick of decisions for `m`.
pub fn think(m: &mut Motion, ch: &Character, facing: &mut Facing, c: &Controls) {
    if m.lock_frames > 0 || m.corpse {
        return;
    }
    if m.dying {
        // The dead ignore their controls: let the death reaction play out,
        // then settle (lying down) on whatever the body is doing.
        if !reaction::is_victim(m.kind) {
            let none = Buttons::default();
            let still = Controls { buttons: &none, ..*c };
            settle(m, ch, &still);
        }
        return;
    }
    m.expire_combo();
    if c.player {
        facing.set_modifier(0.0);
    }

    let (clip_before, queued_before) = (m.clip, m.queued);
    let _ = react(m, ch, c)
        || act(m, ch, c)
        || committed(m)
        || locomote(m, ch, facing, c)
        || settle(m, ch, c);

    // A request that merely queued is forgotten unless the clip ends soon.
    if m.clip == clip_before && m.frame + BUFFER_FRAMES + 1 < m.length() {
        m.queued = queued_before;
    }

    // Holding jump early in a jump carries the character higher.
    if m.air.frames > 0
        && c.buttons.holding(key::JUMP)
        && m.air.frames < m.tuning.num_jump_accel_frames
    {
        m.air.velocity.y += m.tuning.jump_accel_amount;
    }

    m.upgrade_varient(ch);
    steer(m, facing, c);
}

/// Tries `list` in order; the first rule whose controls match and whose
/// animation resolves wins. `fighting` looks moves up in the fight stance
/// (and enters it when the clip found is a fight-stance one).
fn play(
    m: &mut Motion,
    ch: &Character,
    c: &Controls,
    list: &[Rule],
    urge: Urge,
    fighting: bool,
) -> Option<usize> {
    let saved = m.varient;
    if fighting {
        m.varient |= var::FIGHT;
    }
    let b = c.buttons;
    let mut got = None;
    for r in list {
        if !b.holding(r.hold) || !b.just_pressed(r.press) {
            continue;
        }
        if r.after != ty::NONE && m.last_attack != r.after {
            continue;
        }
        let late = fighting && r.late_ok && c.player;
        got = m.request(ch, urge, late, r.anim);
        if got.is_some() {
            break;
        }
    }
    m.varient = saved;
    if fighting {
        if let Some(clip) = got {
            if m.clip_of(clip).varient & var::FIGHT != 0 {
                m.fight_mode();
            }
        }
    }
    got
}

/// The clip the first matching rule would resolve to, without playing it.
fn peek(m: &Motion, c: &Controls, list: &[Rule]) -> Option<usize> {
    let b = c.buttons;
    list.iter()
        .filter(|r| b.holding(r.hold) && b.just_pressed(r.press))
        .filter(|r| r.after == ty::NONE || m.last_attack == r.after)
        .find_map(|r| m.coll.lookup(r.anim, m.state, m.varient))
}

// --- 1. react ----------------------------------------------------------------

fn react(m: &mut Motion, ch: &Character, c: &Controls) -> bool {
    if m.please_land {
        let mut found = false;
        if m.please_land_hard {
            let state = m.state;
            for s in [state, st::FALLING, st::FLYING] {
                m.state = s;
                if play(m, ch, c, &rules::land(true), Urge::Force, false).is_some() {
                    found = true;
                    break;
                }
            }
            if !found {
                m.state = state;
            }
        }
        if !found {
            found = play(m, ch, c, &rules::land(false), Urge::Force, false).is_some();
        }
        m.please_land = false;
        m.please_land_hard = false;
        if found {
            return true;
        }
    }

    if m.air.frames > FLAIL_AFTER
        && m.request(ch, Urge::Force, false, ty::FALLING_FLAIL).is_some()
    {
        return true;
    }

    if stunned(m, ch) {
        return true;
    }

    matches!(m.kind, ty::STAGGER | ty::STAGGER_BEHIND | ty::STUN | ty::BLOCK)
        && play(m, ch, c, &rules::recover(), Urge::Force, false).is_some()
}

/// Holds the character in its block / stun / stagger while those last.
fn stunned(m: &mut Motion, ch: &Character) -> bool {
    let mut held = false;
    if m.block_stun > 0 {
        m.fight_mode();
        held |= m.request(ch, Urge::Appropriate, false, ty::BLOCK).is_some();
    }
    if m.hit_stun > 0 {
        // Reeling from a hit: no control until the reaction clip ends.
        m.fight_mode();
        if reaction::is_victim(m.kind) && m.frame + 1 < m.length() {
            held = true;
        }
    }
    if m.dizzy_stun > 0 {
        m.fight_mode();
        let urge = match m.kind {
            ty::STUN => Urge::Queue,
            ty::STAGGER | ty::STAGGER_BEHIND if m.stagger_stun > 0 => Urge::Queue,
            _ => Urge::Force,
        };
        if m.request(ch, urge, false, ty::STUN).is_some() {
            held = true;
        } else {
            m.dizzy_stun = 0;
        }
    }
    if m.stagger_stun > 0 {
        m.fight_mode();
        // Only an ongoing stagger is extended (Q2 in docs/QUIRKS.md).
        let kind = m.kind;
        let ok = matches!(kind, ty::STAGGER | ty::STAGGER_BEHIND)
            && m.request(ch, Urge::Queue, false, kind).is_some();
        if ok {
            held = true;
        } else {
            m.stagger_stun = 0;
        }
    }
    held
}

// --- 2. act -------------------------------------------------------------------

/// Run-family clips a crouch press turns into a slide.
fn is_running(t: u16) -> bool {
    matches!(
        t,
        ty::RUN
            | ty::RUN_START
            | ty::RUN_BACKWARDS
            | ty::RUN_BACKWARDS_START
            | ty::RUN_SIDESTEP_LEFT
            | ty::RUN_SIDESTEP_LEFT_START
            | ty::RUN_SIDESTEP_RIGHT
            | ty::RUN_SIDESTEP_RIGHT_START
    )
}

fn act(m: &mut Motion, ch: &Character, c: &Controls) -> bool {
    if !c.restricted {
        if is_running(m.kind) && play(m, ch, c, &rules::slide(), Urge::Appropriate, true).is_some()
        {
            return true;
        }
        if c.buttons.pressed != 0
            && play(m, ch, c, &rules::fighting(), Urge::Appropriate, true).is_some()
        {
            return true;
        }
        return jump(m, ch, c);
    }
    false
}

fn jump(m: &mut Motion, ch: &Character, c: &Controls) -> bool {
    if !c.buttons.just_pressed(key::JUMP) {
        return false;
    }
    let taking_off = [
        ty::JUMP_FORWARD,
        ty::JUMP_LEFT,
        c.quirks.jump_right(),
        ty::JUMP_BACKWARD,
        ty::JUMP,
    ];
    if taking_off.contains(&m.kind) {
        return false;
    }
    m.air.build = Some(m.velocity_estimate());
    let found = play(m, ch, c, &rules::jump(c.quirks), Urge::Appropriate, false).is_some();
    if !found {
        m.air.build = None;
    }
    found
}

// --- 3. commit ---------------------------------------------------------------

/// Actions that, once queued, movement input must not replace.
fn is_committed(t: u16) -> bool {
    matches!(
        t,
        ty::SLIDE
            | ty::JUMP
            | ty::JUMP_FORWARD
            | ty::JUMP_BACKWARD
            | ty::JUMP_LEFT
            | ty::JUMP_RIGHT
            | ty::KICK
            | ty::PUNCH
            | ty::KICK_FORWARD
            | ty::KICK_LEFT
            | ty::KICK_RIGHT
            | ty::KICK_BACK
            | ty::KICK_LOW
            | ty::PUNCH_FORWARD
            | ty::PUNCH_LEFT
            | ty::PUNCH_RIGHT
            | ty::PUNCH_BACK
            | ty::PUNCH_LOW
            | ty::KICK2
            | ty::KICK3
            | ty::KICK3_FORWARD
            | ty::PUNCH2
            | ty::PUNCH3
            | ty::PUNCH4
            | ty::PPKKKK
            | ty::PPKKK
            | ty::PPKK
            | ty::PPK
            | ty::PKK
            | ty::PKP
            | ty::KPK
            | ty::KPP
            | ty::KKP
            | ty::PK
            | ty::KP
            | ty::RUN_KICK
            | ty::RUN_PUNCH
            | ty::RUN_BACK_PUNCH
            | ty::RUN_BACK_KICK
            | ty::SIDESTEP_LEFT_KICK
            | ty::SIDESTEP_LEFT_PUNCH
            | ty::SIDESTEP_RIGHT_KICK
            | ty::SIDESTEP_RIGHT_PUNCH
            | ty::LAND_DEAD
            | ty::TAUNT
    )
}

fn committed(m: &Motion) -> bool {
    is_committed(m.queued)
}

// --- 4. locomote ---------------------------------------------------------------

fn locomote(m: &mut Motion, ch: &Character, facing: &mut Facing, c: &Controls) -> bool {
    advance(m, ch, facing, c)
        || sidestep(m, ch, facing, c)
        || stance(m, ch, c)
        || turn_in_place(m, ch, facing, c)
        || halt(m, ch, c)
}

/// Clips that let a held step key angle the run diagonally.
fn takes_diagonal(t: u16) -> bool {
    matches!(
        t,
        ty::WALK_BACKWARDS
            | ty::WALK_BACKWARDS_START
            | ty::RUN_BACKWARDS
            | ty::RUN_BACKWARDS_START
            | ty::RUN_SIDESTEP_LEFT_START
            | ty::RUN_SIDESTEP_RIGHT_START
            | ty::WALK
            | ty::WALK_START
            | ty::RUN
            | ty::RUN_START
            | ty::WALK_STOP
            | ty::RUN_STOP
    )
}

/// Moving forward or back (diagonally when a step key is held too).
fn advance(m: &mut Motion, ch: &Character, facing: &mut Facing, c: &Controls) -> bool {
    let b = c.buttons;
    if b.held & (key::FORWARD | key::BACKWARD) == 0 || m.air.frames > 0 {
        return false;
    }
    let crouch = b.holding(key::CROUCH);
    let list = if c.restricted {
        if crouch {
            return false;
        }
        rules::move_restricted()
    } else if crouch {
        rules::move_crouched()
    } else {
        rules::move_upright()
    };

    if let Some(clip) = play(m, ch, c, &list, Urge::Appropriate, false) {
        // Forward diagonals lean toward the step key; backward ones away.
        let lean = if b.holding(key::FORWARD) { FRAC_PI_4 } else { -FRAC_PI_4 };
        if c.player && takes_diagonal(m.clip_of(clip).anim_type) {
            if b.holding(key::STEP_RIGHT) {
                facing.set_modifier(-lean);
            } else if b.holding(key::STEP_LEFT) {
                facing.set_modifier(lean);
            }
        }
        return true;
    }
    // Knocked down: the get-up clips live in the fight stance.
    matches!(m.state, st::FALLEN_BACK | st::FALLEN_FRONT)
        && play(m, ch, c, &list, Urge::Appropriate, true).is_some()
}

/// The start clip that leads into a gait loop, if any.
fn start_of(gait: u16) -> Option<u16> {
    match gait {
        ty::RUN => Some(ty::RUN_START),
        ty::RUN_BACKWARDS => Some(ty::RUN_BACKWARDS_START),
        ty::WALK => Some(ty::WALK_START),
        ty::WALK_BACKWARDS => Some(ty::WALK_BACKWARDS_START),
        _ => None,
    }
}

/// Stepping sideways — or, when the current stance has no sidestep that fits
/// as well as its forward gait, running forward with the body turned 90°.
fn sidestep(m: &mut Motion, ch: &Character, facing: &mut Facing, c: &Controls) -> bool {
    let b = c.buttons;
    let (left, right) = (b.holding(key::STEP_LEFT), b.holding(key::STEP_RIGHT));
    if !(left || right) || m.air.frames > 0 {
        return false;
    }
    let (walk, crouch) = (b.holding(key::WALK), b.holding(key::CROUCH));
    let list = if c.restricted {
        rules::sidestep_walk(true)
    } else if walk {
        rules::sidestep_walk(false)
    } else {
        rules::sidestep_run()
    };

    let mut side = peek(m, c, &list);
    if let Some(s) = side {
        if m.clip_of(s).varient == m.varient {
            if play(m, ch, c, &list, Urge::Appropriate, false).is_some() {
                return true;
            }
            side = None;
        }
    }

    // Turned-forward alternative, by stance preference.
    let gaits: &[u16] = if c.restricted {
        &[ty::WALK]
    } else if walk && crouch {
        &[ty::CROUCH_WALK, ty::CROUCH_RUN, ty::WALK, ty::RUN]
    } else if crouch {
        &[ty::CROUCH_RUN, ty::CROUCH_WALK, ty::RUN, ty::WALK]
    } else if walk {
        &[ty::WALK, ty::RUN]
    } else {
        &[ty::RUN, ty::WALK]
    };
    let turned = gaits.iter().find_map(|&g| {
        let direct = m.coll.lookup(g, m.state, m.varient).map(|clip| (clip, g));
        direct.or_else(|| {
            let s = start_of(g)?;
            m.coll.lookup(s, m.state, m.varient).map(|clip| (clip, s))
        })
    });

    let mut moved = false;
    if let Some((clip, gait)) = turned {
        let better = side.is_none_or(|s| m.clip_of(clip).varient > m.clip_of(s).varient);
        if better {
            moved = m.request(ch, Urge::Appropriate, false, gait).is_some();
        }
    }
    if !moved && side.is_some() {
        return play(m, ch, c, &list, Urge::Appropriate, false).is_some();
    }
    if moved
        && c.player
        && matches!(
            m.kind,
            ty::CROUCH_WALK
                | ty::CROUCH_RUN
                | ty::WALK
                | ty::RUN
                | ty::RUN_START
                | ty::WALK_START
                | ty::WALK_BACKWARDS_START
                | ty::RUN_BACKWARDS_START
        )
    {
        facing.set_modifier(if left { FRAC_PI_2 } else { -FRAC_PI_2 });
    }
    moved
}

/// Crouching down or standing back up.
fn stance(m: &mut Motion, ch: &Character, c: &Controls) -> bool {
    let crouch = c.buttons.holding(key::CROUCH);
    match m.state {
        st::STANDING if crouch => play(m, ch, c, &rules::duck(), Urge::Appropriate, false).is_some(),
        st::CROUCH_START => {
            play(m, ch, c, &rules::duck(), Urge::Appropriate, false).is_some()
                || play(m, ch, c, &rules::rise(), Urge::Appropriate, false).is_some()
        }
        st::CROUCH if !crouch => play(m, ch, c, &rules::rise(), Urge::Appropriate, false).is_some(),
        _ => false,
    }
}

/// A big enough gap between where the body faces and where the player wants
/// it plays a step-turn; once started, the turn is followed through.
fn turn_in_place(m: &mut Motion, ch: &Character, facing: &Facing, c: &Controls) -> bool {
    let want = facing.desired_offset();
    if want > TURN_DEADZONE || m.turn_left {
        let found = play(m, ch, c, &rules::turn_in_place(true), Urge::Appropriate, false).is_some();
        m.turn_left = true;
        found
    } else if want < -TURN_DEADZONE || m.turn_right {
        let found = play(m, ch, c, &rules::turn_in_place(false), Urge::Appropriate, false).is_some();
        m.turn_right = true;
        found
    } else {
        false
    }
}

/// Locomotion clips that come to a stop when the move keys are released.
fn is_moving(t: u16) -> bool {
    matches!(
        t,
        ty::STANDING_TURN_LEFT
            | ty::STANDING_TURN_RIGHT
            | ty::RUN
            | ty::RUN_START
            | ty::RUN_BACKWARDS
            | ty::RUN_BACKWARDS_START
            | ty::RUN_SIDESTEP_LEFT
            | ty::RUN_SIDESTEP_LEFT_START
            | ty::RUN_SIDESTEP_RIGHT
            | ty::RUN_SIDESTEP_RIGHT_START
            | ty::WALK
            | ty::WALK_START
            | ty::WALK_BACKWARDS
            | ty::WALK_BACKWARDS_START
            | ty::WALK_SIDESTEP_LEFT
            | ty::WALK_SIDESTEP_RIGHT
            | ty::CROUCH_RUN
            | ty::CROUCH_RUN_BACKWARDS
            | ty::CROUCH_RUN_SIDESTEP_LEFT
            | ty::CROUCH_RUN_SIDESTEP_RIGHT
            | ty::CROUCH_WALK
            | ty::CROUCH_WALK_BACKWARDS
            | ty::CROUCH_WALK_SIDESTEP_LEFT
            | ty::CROUCH_WALK_SIDESTEP_RIGHT
    )
}

fn halt(m: &mut Motion, ch: &Character, c: &Controls) -> bool {
    is_moving(m.kind) && play(m, ch, c, &rules::stop(), Urge::Appropriate, false).is_some()
}

// --- 5. settle ------------------------------------------------------------------

/// Ground-locomotion clips: being airborne in one means a ledge was walked off.
fn is_ground_gait(t: u16) -> bool {
    is_moving(t) && !matches!(t, ty::STANDING_TURN_LEFT | ty::STANDING_TURN_RIGHT)
        || matches!(t, ty::RUN_STOP | ty::WALK_STOP | ty::ONE_STEP_STOP)
}

fn settle(m: &mut Motion, ch: &Character, c: &Controls) -> bool {
    let bad = m.bad_varient();
    let urge = if bad && !m.current().is_atomic(m.frame) {
        Urge::Force
    } else {
        Urge::Queue
    };

    if m.air.frames > 0 && is_ground_gait(m.kind) {
        m.fall_into_flight(ch, c.quirks);
        return false;
    }
    let jumping = matches!(
        m.kind,
        ty::FLY | ty::JUMP | ty::JUMP_FORWARD | ty::JUMP_BACKWARD | ty::JUMP_LEFT | ty::JUMP_RIGHT
    );
    if jumping && m.request(ch, Urge::Queue, false, ty::FLY).is_some() {
        return true;
    }
    if m.air.frames > 0 {
        if m.request(ch, Urge::Queue, false, ty::FLY).is_some()
            || m.request(ch, Urge::Queue, false, ty::FALLING_FLAIL).is_some()
        {
            return true;
        }
        m.fall_into_flight(ch, c.quirks);
        return false;
    }

    if m.idle_due() {
        let found = m.request(ch, urge, false, ty::IDLE).is_some();
        m.after_idle_request();
        if found {
            return true;
        }
    }

    // Cheap paths: already resting in the stance the controls ask for.
    if !bad {
        let crouch = c.buttons.holding(key::CROUCH);
        if m.kind == ty::CROUCH && m.state == st::CROUCH && crouch {
            m.queued = ty::CROUCH;
            return true;
        }
        if m.kind == ty::STAND && m.state == st::STANDING && !crouch {
            m.queued = ty::STAND;
            return true;
        }
    }

    if play(m, ch, c, &rules::settle(), urge, false).is_some()
        || play(m, ch, c, &rules::settle(), urge, true).is_some()
    {
        return true;
    }
    m.force_stand(ch);
    false
}

// --- turning --------------------------------------------------------------------

/// Whether the current clip keeps the body from turning on its own.
fn cannot_turn(m: &Motion, player: bool) -> bool {
    let by_kind = match m.kind {
        ty::STAND | ty::CROUCH => {
            // Players turn only through the step-turn clips while at rest.
            player
                || matches!(
                    m.state,
                    st::FALLEN_BACK
                        | st::FALLEN_FRONT
                        | st::SLIDING
                        | st::RUN_SLIDE
                        | st::RUN_BACK_SLIDE
                        | st::LIE_BACK
                )
        }
        ty::BLOCK | ty::PRONE => true,
        k => reaction::is_victim(k),
    };
    let c = m.current();
    by_kind || c.has_flag(flag::THROW_SOURCE) || c.has_flag(flag::THROW_TARGET)
}

/// Slews the body toward the desired facing by at most the turn cap. During
/// a step-turn clip the body only turns in the direction the turn started.
fn steer(m: &mut Motion, facing: &mut Facing, c: &Controls) {
    let cap = c.turn_cap;
    let want = facing.desired_offset();
    let (was_left, was_right) = (m.turn_left, m.turn_right);
    if want < -cap {
        m.turn_left = false;
    } else if want > cap {
        m.turn_right = false;
    } else {
        m.turn_left = false;
        m.turn_right = false;
    }

    let step_turn = matches!(
        m.kind,
        ty::CROUCH_TURN_LEFT | ty::CROUCH_TURN_RIGHT | ty::STANDING_TURN_LEFT | ty::STANDING_TURN_RIGHT
    );
    let (can_left, can_right) = if step_turn {
        (was_left, was_right)
    } else {
        let free = !cannot_turn(m, c.player);
        (free, free)
    };
    let amount = if want > 0.0 && can_left {
        want.min(cap)
    } else if want < 0.0 && can_right {
        want.max(-cap)
    } else {
        0.0
    };
    facing.rotate(amount);
}
