//! Carrying out a technique's moves (
//! *Running moves*): attacks wait for their moment and are asked for with
//! the keys the movement rules need; position moves step through the
//! transition graph and run in; maneuvers move or hold keys for a while;
//! evasions and throws are asked for when they fit.

use std::f32::consts::PI;

use super::brain::{bearing, distance, relative_turned, Melee, Output, View};
use super::geometry::{self, Attacker, Miss, Target};
use super::profile::Move;
use super::tables::{
    self, Dir, MeleeState, ATTACKS, EVADES, MANEUVERS, POSITIONS, THROWS, TRANSITIONS,
};
use crate::anim::input::key as k;
use crate::anim::types::{flag, var};

/// An attack waits for this many ticks left of an uninterruptible clip when
/// it follows directly, or this many via a shortcut.
const QUEUE_DIRECT: u16 = 4;
const QUEUE_SHORTCUT: u16 = 8;
const GIVE_UP_ANGLE: f32 = 60.0 * PI / 180.0;
const START_ANGLE: f32 = 15.0 * PI / 180.0;
const STANDING_TURN: f32 = 120.0 * PI / 180.0;
const THROW_FACING: f32 = 45.0 * PI / 180.0;
const NOT_FACING_TICKS: u32 = 60;
const END_OF_CLIP: u16 = 3;
const MAX_RUN_IN_TICKS: u32 = 120;
/// Slack on a run-in's reach before a strike that won't connect is given
/// up (`AI2cMelee_AdditionalDelayTolerance`).
const ADDITIONAL_DELAY_TOLERANCE: f32 = 5.0;
/// A request not realised within this long has failed.
const REQUEST_TIMEOUT: u32 = 30;

/// Per-move progress (reset for each move).
#[derive(Clone, Debug, Default)]
pub struct MoveState {
    pub started: bool,
    pub ticks: u32,
    /// An attack / evade / throw has been asked for, and has begun.
    pub launched: bool,
    pub realised: bool,
    pub requested: u16,
    pub realised_at: i64,
    /// Position: which state of its list we're heading for; running in.
    pub state_index: usize,
    pub delaying: bool,
    pub delay_ticks: u32,
    pub last_keys: u32,
    pub not_facing: u32,
    /// Ticks to wait after letting go of jump (we'd have overshot).
    pub miss_by_height: u32,
    /// Maneuver: when it ends.
    pub until: Option<i64>,
}

/// The melee state the body is in for planning: the current clip's start
/// state, or its end state near its end.
fn body_state(me: &View) -> MeleeState {
    let m = me.motion;
    let left = m.length().saturating_sub(m.frame);
    let s = if left <= END_OF_CLIP {
        m.state
    } else {
        m.current().from_state
    };
    MeleeState::from_anim_state(s)
}

/// Whether the AI is in an attack, throw or jump that must play out.
fn busy(me: &View) -> bool {
    let c = me.motion.current();
    !c.attacks.is_empty() || c.has_flag(flag::THROW_SOURCE) || me.airborne
}

/// How far the attack direction is off the target.
fn facing_delta(me: &View, target: &View, dir: Dir) -> f32 {
    let off = dir.offset().unwrap_or(0.0);
    let d = bearing(me, target) - (me.facing + off);
    (crate::angle::wrap_pi(d)).abs()
}

/// May the current move be cut short by an evasion?
pub fn can_evade(m: &Melee, me: &View) -> bool {
    match m.current().and_then(|t| t.moves.get(m.move_index)) {
        Some(Move::Attack(_)) | Some(Move::Throw(_)) => !m.mv.launched,
        Some(Move::Position { .. }) => !me.airborne,
        Some(Move::Maneuver { .. }) => true,
        Some(Move::Evade(_)) => false,
        None => true,
    }
}

/// Runs a freshly chosen technique's first move this tick.
pub fn start_if_needed(m: &mut Melee, me: &View, target: &View, out: &mut Output) {
    if !m.mv.started {
        update(m, me, target, out);
    }
}

/// Steps the current move; returns whether it is still under way.
pub fn update(m: &mut Melee, me: &View, target: &View, out: &mut Output) -> bool {
    loop {
        let Some(t) = m.technique else { return false };
        let Some(&mv) = m.profile.techniques[t].moves.get(m.move_index) else {
            return false;
        };
        let first = !m.mv.started;
        m.mv.started = true;
        if !first {
            m.mv.ticks += 1;
        }
        let step = match mv {
            Move::Attack(a) => attack(m, me, target, out, a),
            Move::Position { index, run_in } => position(m, me, target, out, index, run_in),
            Move::Maneuver {
                index,
                duration,
                a,
                b,
            } => maneuver(m, me, target, out, index, duration, a, b),
            Move::Evade(e) => evade(m, me, target, out, e),
            Move::Throw(i) => throw(m, me, target, out, i),
        };
        match step {
            Step::Going => return true,
            Step::Done => {
                m.next_move();
                // Run the next move straight away unless it waits for the
                // body to finish what it is doing.
                if busy(me) {
                    return true;
                }
            }
            Step::Abort => {
                m.abort();
                return false;
            }
        }
    }
}

enum Step {
    Going,
    Done,
    Abort,
}

fn set_delay(m: &mut Melee) {
    if let Some(t) = m.technique {
        m.delay_until[t] = m.now + i64::from(m.profile.techniques[t].repeat_delay);
    }
}

fn commit(m: &mut Melee) {
    if m.current().is_some_and(|t| !t.interruptible) {
        m.committed = true;
    }
}

/// The keys the movement rules need for animation type `anim` (and the
/// attack that must come first), or the table's own.
fn attack_keys(anim: u16, fixed: Option<(u32, u32)>) -> Option<(u32, u32, u16)> {
    if let Some((hold, press)) = fixed {
        return Some((hold, press, crate::anim::types::ty::NONE));
    }
    crate::anim::rules::fighting()
        .into_iter()
        .find(|r| r.anim == anim)
        .map(|r| (r.hold, r.press, r.after))
}

/// Waits for the moment an attack can follow the current clip.
fn in_queue_window(me: &View, anim: u16) -> bool {
    let mo = me.motion;
    if !mo.current().is_atomic(mo.frame) {
        return true;
    }
    let left = mo.length().saturating_sub(mo.frame);
    let direct = mo
        .coll
        .lookup(anim, mo.state, mo.varient | var::FIGHT)
        .is_some();
    left <= if direct { QUEUE_DIRECT } else { QUEUE_SHORTCUT }
}

fn attack(m: &mut Melee, me: &View, target: &View, out: &mut Output, index: usize) -> Step {
    let am = &ATTACKS[index];
    let anim = tables::ty(am.anim);
    let Some((hold, press, after)) = attack_keys(anim, am.keys) else {
        return Step::Abort;
    };
    if m.mv.launched {
        if !m.mv.realised {
            if me.motion.kind == anim {
                m.mv.realised = true;
                m.mv.realised_at = m.now;
                set_delay(m);
                commit(m);
            } else if m.mv.ticks > REQUEST_TIMEOUT {
                return Step::Abort;
            } else {
                out.held = hold;
                out.pressed = press;
                return Step::Going;
            }
        }
        // Chain straight into a following attack (it waits for its moment);
        // otherwise let this one finish.
        let next_is_attack = m
            .current()
            .and_then(|t| t.moves.get(m.move_index + 1))
            .is_some_and(|n| matches!(n, Move::Attack(_)));
        return if next_is_attack || !busy(me) {
            Step::Done
        } else {
            Step::Going
        };
    }

    if !in_queue_window(me, anim) {
        out.held = m.mv.last_keys;
        return Step::Going;
    }
    // Lined up?
    let delta = facing_delta(me, target, am.dir);
    if !m.committed {
        if delta > GIVE_UP_ANGLE {
            return Step::Abort;
        }
        if delta > START_ANGLE && body_state(me).can_turn_for_attack() {
            return Step::Going; // keep turning toward it
        }
    }
    // Would it land? (After positioning, the positioning already judged it.)
    if !m.positioned {
        let state = body_state(me);
        let clip = me
            .motion
            .coll
            .lookup(anim, state.anim_state(), me.motion.varient | var::FIGHT);
        let Some(clip) = clip else { return Step::Abort };
        let (rel, vel) = relative_turned(me, target, 0.0);
        let attacker = Attacker {
            clip: &me.motion.coll.entries[clip].clip,
            tick: 0,
            air: None,
            sphere: 3.0,
            quirks: m.quirks,
        };
        let victim = Target {
            pos: rel,
            velocity: vel,
            radius: target.planning.0,
            height: target.planning.1,
            clip: Some(target.motion.current()),
            tick: target.motion.frame,
        };
        match geometry::intersect(&attacker, &victim) {
            Ok(_) => {}
            Err(Miss::Distance)
                if m.mv.last_keys & (k::FORWARD | k::BACKWARD | k::STEP_LEFT | k::STEP_RIGHT)
                    != 0 =>
            {
                out.held = m.mv.last_keys;
                return Step::Going;
            }
            Err(Miss::Angle) => {} // launch anyway
            Err(_) if m.move_index > 0 => {}
            Err(_) => return Step::Abort,
        }
    }
    // A combo follow-up needs its opener.
    if after != crate::anim::types::ty::NONE && me.motion.last_attack != after {
        return Step::Abort;
    }
    out.held = hold;
    out.pressed = press;
    out.fresh_attack = after == crate::anim::types::ty::NONE;
    m.mv.launched = true;
    m.mv.requested = anim;
    m.mv.ticks = 0;
    Step::Going
}

/// The transition to take from `cur` toward `goal`, if the body can.
fn transition(me: &View, cur: MeleeState, goal: MeleeState) -> Option<u32> {
    let mo = me.motion;
    TRANSITIONS
        .iter()
        .filter(|t| t.from == cur && t.to.contains(&goal))
        .find(|t| {
            t.anims.iter().any(|a| {
                mo.coll
                    .lookup(tables::ty(a), mo.state, mo.varient | var::FIGHT)
                    .is_some()
            })
        })
        .map(|t| t.keys)
}

fn position(
    m: &mut Melee,
    me: &View,
    target: &View,
    out: &mut Output,
    index: usize,
    run_in: Option<super::profile::RunIn>,
) -> Step {
    let p = &POSITIONS[index];
    let cur = body_state(me);
    let Some(&goal) = p.states.get(m.mv.state_index) else {
        m.positioned = true;
        return Step::Done;
    };
    // In the air on a jump-in: keep flying (holding jump for lift) until the
    // strike would land soon enough, then strike.
    // (Any jump counts: the original resolves a directionless jump from the
    // keys held.)
    let jumping = cur.is_jumping() || me.airborne || me.motion.kind == crate::anim::types::ty::JUMP;
    if p.jump != tables::JumpKind::None && goal.is_jumping() && jumping {
        if me.airborne && air_strike_ready(m, me, target, p) {
            m.mv.state_index = p.states.len();
            m.positioned = true;
            return Step::Done;
        }
        if !me.airborne && cur == goal {
            m.mv.delay_ticks += 1;
            if m.mv.delay_ticks > MAX_RUN_IN_TICKS {
                return Step::Abort;
            }
        }
        out.held = m.mv.last_keys;
        return Step::Going;
    }
    if cur == goal {
        // Running in: until the strike would connect from here.
        if m.mv.state_index == 0 && p.can_delay && run_in.is_some() {
            m.mv.delaying = true;
            m.mv.delay_ticks += 1;
            if m.mv.delay_ticks > MAX_RUN_IN_TICKS {
                return Step::Abort;
            }
            if !in_reach_after_run_in(m, me, target) {
                // Facing the target but not in reach: maybe this strike won't
                // connect from any run-in (`AI2iMelee_Position_Update`).
                if let (Some(r), Some(delay)) = (run_in, delay_distance(m, me, target)) {
                    let touching = me.touching_character
                        && distance(me, target) <= me.planning.0 + target.planning.0 + 1.0;
                    if facing_delta(me, target, p.dir) < START_ANGLE
                        && (delay < 0.0
                            || delay > r.max + r.tolerance + ADDITIONAL_DELAY_TOLERANCE
                            || touching)
                    {
                        return Step::Abort;
                    }
                }
                // Running in along a clear way only.
                let want = bearing(me, target) + p.dir.offset().unwrap_or(0.0);
                let fearless = m.technique.is_some_and(|t| {
                    m.quirks.fearless(
                        m.profile.techniques[t].fearless,
                        m.info[t].start.map_or(0, |s| s as u8),
                    )
                });
                if !fearless
                    && super::brain::steer(me, want) != Some(0.0)
                    && super::brain::floor_probe(
                        me,
                        glam::Vec3::new(want.sin(), 0.0, want.cos()),
                        distance(me, target),
                        super::brain::fall_danger(me),
                    )
                    .ledge
                    .is_some()
                {
                    return Step::Abort;
                }
                let keys = cur.maintain_keys() | p.dir.keys();
                m.mv.last_keys = keys;
                out.held = if facing_delta(me, target, p.dir) > STANDING_TURN {
                    0
                } else {
                    keys
                };
                return Step::Going;
            }
        } else if p.can_wait_for_facing
            && m.mv.state_index + 1 == p.states.len()
            && facing_delta(me, target, p.dir) > START_ANGLE
        {
            m.mv.not_facing += 1;
            if m.mv.not_facing > NOT_FACING_TICKS {
                return Step::Abort;
            }
            return Step::Going;
        }
        m.mv.state_index += 1;
        m.mv.delaying = false;
        if m.mv.state_index >= p.states.len() {
            m.positioned = true;
            return Step::Done;
        }
        return Step::Going;
    }
    // Step toward the next state.
    let keys = transition(me, cur, goal)
        .or_else(|| transition(me, cur, MeleeState::Standing))
        .or_else(|| (cur == MeleeState::None).then_some(goal.maintain_keys()));
    let Some(keys) = keys else { return Step::Abort };
    m.mv.last_keys = keys;
    // Far off facing: turn on the spot first (`AI2cMeleeStandingTurnAngle`) —
    // except lying down, where the body can't turn until it is up (port
    // choice: the original waits there for good).
    let lying = cur == MeleeState::Prone;
    out.held = if facing_delta(me, target, p.dir) > STANDING_TURN && !lying {
        0
    } else {
        keys
    };
    Step::Going
}

/// Airborne on a jump-in: strike now? Yes when right next to the target, or
/// when the strike would land by two thirds of its strike window. A strike
/// passing over the target's head lets go of jump (and waits a moment).
fn air_strike_ready(m: &mut Melee, me: &View, target: &View, p: &tables::PositionMove) -> bool {
    const MISS_BY_HEIGHT_TICKS: u32 = 4;
    let Some(t) = m.technique else { return true };
    let info = &m.info[t];
    let Some((_, _, clip)) = info.attack else {
        return true;
    };
    let (rel, vel) = relative_turned(me, target, 0.0);
    let along = match p.dir {
        Dir::Forward => rel.z,
        Dir::Left => rel.x,
        Dir::Right => -rel.x,
        Dir::Back => -rel.z,
        Dir::None => (rel.x * rel.x + rel.z * rel.z).sqrt(),
    };
    if along < target.planning.0 + info.ideal_range {
        return true;
    }
    let c = &m.planned_coll(me).entries[clip].clip;
    let air = geometry::Air {
        velocity: me.motion.air.velocity,
        gravity: me.motion.tuning.jump_gravity,
        expected_ticks: 0,
    };
    let attacker = Attacker {
        clip: c,
        tick: 0,
        air: Some(air),
        sphere: 3.0,
        quirks: m.quirks,
    };
    let victim = Target {
        pos: rel,
        velocity: vel,
        radius: target.planning.0,
        height: target.planning.1,
        clip: Some(target.motion.current()),
        tick: target.motion.frame,
    };
    match geometry::intersect(&attacker, &victim) {
        Ok(ticks) => {
            if m.mv.miss_by_height > 0 {
                m.mv.miss_by_height -= 1;
                return false;
            }
            let strikes = c.reach.as_ref().map(|r| &r.strikes);
            let first = strikes.and_then(|s| s.first()).map_or(0, |s| s.tick);
            let last = strikes.and_then(|s| s.last()).map_or(0, |s| s.tick);
            ticks <= last - (last - first) / 3
        }
        Err(Miss::AboveHead) if m.mv.last_keys & k::JUMP != 0 => {
            m.mv.last_keys &= !k::JUMP;
            m.mv.miss_by_height = MISS_BY_HEIGHT_TICKS;
            false
        }
        Err(_) => false,
    }
}

/// Whether, running in, the technique's strike (or throw) would connect if
/// the AI stopped and struck now.
fn in_reach_after_run_in(m: &Melee, me: &View, target: &View) -> bool {
    let Some(t) = m.technique else { return true };
    let info = &m.info[t];
    let dist = distance(me, target) - target.planning.0;
    // Running in to jump: take off once the jump would carry us there.
    let jump = m.profile.techniques[t]
        .moves
        .iter()
        .find_map(|mv| match mv {
            Move::Position { index, .. } if POSITIONS[*index].jump != tables::JumpKind::None => {
                Some(POSITIONS[*index].jump)
            }
            _ => None,
        });
    if let Some(kind) = jump {
        let j = m.jumps;
        return dist <= j.vel(kind) * 0.5 * (j.min + j.max);
    }
    if let Some((_, _, clip)) = info.attack {
        let dir = info.dir.and_then(Dir::offset).unwrap_or(0.0);
        let turn = {
            let d = bearing(me, target) - (me.facing + dir);
            crate::angle::wrap_pi(d)
        };
        let (rel, vel) = relative_turned(me, target, turn);
        let attacker = Attacker {
            clip: &m.planned_coll(me).entries[clip].clip,
            tick: 0,
            air: None,
            sphere: 3.0,
            quirks: m.quirks,
        };
        let victim = Target {
            pos: rel,
            velocity: vel,
            radius: target.planning.0,
            height: target.planning.1,
            clip: Some(target.motion.current()),
            tick: target.motion.frame,
        };
        // (The strike's reach alone: from here on running in would only
        // close the gap, `TRrCheckAnimationBounds`.)
        return geometry::bounds(&attacker, &victim, true);
    }
    if let Some(ti) = info.throw {
        if let Some(Move::Throw(i)) = m.profile.techniques[t].moves.get(ti) {
            return dist <= m.throw_range(*i, me) * 0.9;
        }
    }
    true
}

/// How much farther a run-in has to go before the technique's strike
/// would be at its ideal range — or, ending in a jump, at the middle of its
/// jump's reach (`AI2iMelee_Position_Update`'s `delay_distance`). None for
/// a technique with no strike to run in to.
fn delay_distance(m: &Melee, me: &View, target: &View) -> Option<f32> {
    let t = m.technique?;
    let info = &m.info[t];
    info.attack?;
    let dist = distance(me, target) - target.planning.0;
    let jump = m.profile.techniques[t]
        .moves
        .iter()
        .find_map(|mv| match mv {
            Move::Position { index, .. } if POSITIONS[*index].jump != tables::JumpKind::None => {
                Some(POSITIONS[*index].jump)
            }
            _ => None,
        });
    Some(match jump {
        Some(kind) => dist - m.jumps.vel(kind) * 0.5 * (m.jumps.min + m.jumps.max),
        None => dist - info.ideal_range,
    })
}

#[allow(clippy::too_many_arguments)]
fn maneuver(
    m: &mut Melee,
    me: &View,
    target: &View,
    out: &mut Output,
    index: usize,
    duration: u32,
    a: f32,
    b: f32,
) -> Step {
    let man = &MANEUVERS[index];
    if index == tables::maneuver::RANDOM_STOP {
        return if (m.random() % 100) as f32 >= a {
            Step::Done
        } else {
            Step::Abort
        };
    }
    if m.mv.until.is_none() {
        if busy(me) {
            return Step::Going;
        }
        m.mv.until = Some(m.now + i64::from(duration));
    }
    if m.mv.until.is_some_and(|u| m.now >= u) {
        set_delay(m);
        return Step::Done;
    }
    match man.moving {
        Some(dir) => {
            let dist = distance(me, target);
            if index == tables::maneuver::ADVANCE && dist < (a - b).max(8.0) {
                set_delay(m);
                return Step::Done;
            }
            if index == tables::maneuver::RETREAT && dist > (a + b).max(75.0) {
                set_delay(m);
                return Step::Done;
            }
            // Face the target and move in the maneuver's direction, steering
            // round walls and ledges (turning the body by the detour).
            let want = bearing(me, target) + dir.offset().unwrap_or(0.0);
            let fearless = m.technique.is_some_and(|t| {
                m.quirks.fearless(
                    m.profile.techniques[t].fearless,
                    m.info[t].start.map_or(0, |s| s as u8),
                )
            });
            match super::brain::steer(me, want) {
                Some(dev) => out.face = out.face.map(|f| f + dev),
                None if !fearless => return Step::Abort,
                None => {}
            }
            out.held = dir.keys();
        }
        None => out.held = man.keys,
    }
    Step::Going
}

fn evade(m: &mut Melee, me: &View, target: &View, out: &mut Output, index: usize) -> Step {
    let ev = &EVADES[index];
    let anim = tables::ty(ev.anim);
    if m.mv.realised {
        let length = i64::from(me.motion.length());
        return if m.now >= m.mv.realised_at + length {
            Step::Done
        } else {
            Step::Going
        };
    }
    // The attack is over: no need.
    if super::brain::attack_id(target).is_none() {
        return Step::Done;
    }
    if me.motion.kind == anim {
        m.mv.realised = true;
        m.mv.realised_at = m.now;
        set_delay(m);
        return Step::Going;
    }
    if m.mv.ticks > REQUEST_TIMEOUT {
        return Step::Abort;
    }
    if !in_queue_window(me, anim) {
        return Step::Going;
    }
    out.held = ev.keys.0;
    out.pressed = ev.keys.1;
    m.mv.launched = true;
    Step::Going
}

fn throw(m: &mut Melee, me: &View, target: &View, out: &mut Output, index: usize) -> Step {
    let th = &THROWS[index];
    let anim = tables::ty(th.anim);
    // (A disarm wants the victim's side for that gun: `AI2iMelee_FindThrow`'s
    // desired variant.)
    out.throw_victim = {
        use crate::anim::types::var;
        match th.disarm {
            tables::Disarm::Pistol => var::RIGHTY_PISTOL | var::LEFTY_PISTOL,
            tables::Disarm::Rifle => var::RIGHTY_RIFLE | var::LEFTY_RIFLE,
            tables::Disarm::No => 0,
        }
    };
    if m.mv.launched {
        let c = me.motion.current();
        if c.has_flag(flag::THROW_SOURCE) || !c.attacks.is_empty() {
            set_delay(m);
            commit(m);
            return Step::Done;
        }
        if m.mv.ticks > REQUEST_TIMEOUT {
            return Step::Abort;
        }
        out.held = if th.running { k::FORWARD } else { 0 };
        out.throw = Some(anim);
        return Step::Going;
    }
    if facing_delta(me, target, Dir::Forward) > THROW_FACING
        || distance(me, target) > m.throw_range(index, me)
    {
        return Step::Abort;
    }
    if target.knockdown_resistant || target.motion.invulnerable() {
        return Step::Abort;
    }
    // A disarm: the target must still have that gun out.
    let (pistol, rifle) = super::brain::held_gun(target);
    match th.disarm {
        tables::Disarm::Pistol if !pistol => return Step::Abort,
        tables::Disarm::Rifle if !rifle => return Step::Abort,
        _ => {}
    }
    let rel = {
        let d = target.facing - me.facing;
        (crate::angle::wrap_pi(d)).abs()
    };
    if (th.from_behind && rel >= THROW_FACING) || (!th.from_behind && rel <= PI - THROW_FACING) {
        return Step::Abort;
    }
    if busy(me) || me.motion.current().is_atomic(me.motion.frame) {
        return Step::Going;
    }
    // Asked for as a punch the engine turns into this throw.
    out.held = if th.running { k::FORWARD } else { 0 };
    out.pressed = k::PUNCH;
    out.fresh_attack = true;
    out.throw = Some(anim);
    m.mv.launched = true;
    m.mv.ticks = 0;
    Step::Going
}
