//! The melee AI's decisions (behaviour per `docs/MELEE_AI.md`): each tick it
//! keeps a block up, steps its current move, notices the target's attacks
//! (evading or blocking), and — when free to — weighs its profile's
//! techniques and picks one. Moves are carried out in [`super::moves`].

use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use glam::{Quat, Vec3};

use super::fight::Fights;
use super::geometry::{self, Attacker, Target};
use super::profile::{Kind, Move, Profile, Technique};
use super::tables::{self, Dir, MeleeState, ATTACKS, MANEUVERS, POSITIONS, THROWS};
use crate::anim::motion::Motion;
use crate::anim::types::{strike, var};

/// A fighter as the melee AI sees it this tick.
pub struct View<'a> {
    pub index: usize,
    pub feet: Vec3,
    /// Body yaw.
    pub facing: f32,
    /// Movement last tick (world).
    pub velocity: Vec3,
    pub motion: &'a Motion,
    /// The body's cylinder: radius and height span above the feet, for an
    /// attack against it (`full`) and for planning our own (`planning`).
    pub full: (f32, (f32, f32)),
    pub planning: (f32, (f32, f32)),
    pub alive: bool,
    pub airborne: bool,
    /// It could block a high / low attack from us right now.
    pub could_block: (bool, bool),
    pub knockdown_resistant: bool,
    /// The player (its body sphere is 4, an AI's 3).
    pub player: bool,
}

impl View<'_> {
    pub fn state(&self) -> MeleeState {
        MeleeState::from_anim_state(self.motion.state)
    }
    pub fn fallen(&self) -> bool {
        self.state() == MeleeState::Prone
    }
}

/// What the AI does this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    /// Controls held, and those pressed afresh this tick.
    pub held: u32,
    pub pressed: u32,
    /// The body yaw to turn toward.
    pub face: Option<f32>,
    /// Start this attack fresh (forget the combo in progress).
    pub fresh_attack: bool,
    /// Throw the target with this throw type.
    pub throw: Option<u16>,
}

/// Facts about a technique fixed for a given animation set.
#[derive(Clone, Debug, Default)]
pub(super) struct TechInfo {
    pub start: Option<MeleeState>,
    pub allow_transition: bool,
    pub dir: Option<Dir>,
    /// Damages (an attack or throw).
    pub damaging: bool,
    pub broken: bool,
    /// The first attack: its move index, animation type and clip.
    pub attack: Option<(usize, u16, usize)>,
    pub high: bool,
    pub low: bool,
    pub unblockable: bool,
    pub block_stun: bool,
    pub stagger: bool,
    pub ideal_range: f32,
    pub first_strike: u16,
    /// The first throw move.
    pub throw: Option<usize>,
    pub needs_slot: bool,
}

/// Block stun longer than this makes an attack worth landing on a block.
const BLOCK_STUN_THRESHOLD: u16 = 15;
const CURRENT_TECHNIQUE_BONUS: f32 = 2.0;
const REPEAT_PENALTY: f32 = 0.4;
const ADJUST_DECAY: f32 = 0.6;
const MAX_ADJUST: f32 = 0.8;
const ADJUST_ABORTED: f32 = 1000.0;
const MAX_RUN_IN_TICKS: f32 = 120.0;
const SKIP_POSITIONING: f32 = 3.0;
const HIT_FIRST_WEIGHT: f32 = 0.3;
const CIRCLE_EDGE: f32 = 10.0 * PI / 180.0;
const THROW_RAMP: f32 = 30.0 * PI / 180.0;
const THROW_DIST_SAFETY: f32 = 0.9;
const MIN_TOLERANCE: f32 = 5.0;

fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// A fighter's melee AI.
#[derive(Clone, Debug)]
pub struct Melee {
    pub profile: Rc<Profile>,
    pub target: usize,
    pub(super) info: Vec<TechInfo>,
    pub(super) jumps: Jumps,
    pub(super) technique: Option<usize>,
    pub(super) move_index: usize,
    pub(super) committed: bool,
    threshold: f32,
    adjust: Vec<f32>,
    pub(super) delay_until: Vec<i64>,
    pub(super) seeds: (u32, u32),
    rng: u32,
    pub(super) mv: super::moves::MoveState,
    /// A position move has brought the technique's strike in range.
    pub(super) positioned: bool,
    /// Blocking the target's attack until its strikes are over.
    pub(super) blocking: Option<BlockHold>,
    /// The target attack we've judged (its attack count), whether we noticed
    /// it, may evade it, and the ticks until it would hit us.
    seen_attack: Option<u32>,
    react: bool,
    evade: bool,
    pub(super) ticks_to_hit: Option<u16>,
    /// Ticks lying dazed after a knockdown; whether this knockdown's daze ran.
    pub daze: u32,
    dazed_this_time: bool,
    daze_range: (u32, u32),
    no_technique: u32,
    pub(super) now: i64,
    /// Test hook: use this technique next (by name), if it can be used.
    pub force: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct BlockHold {
    pub attack: u32,
    pub last_strike: u16,
    pub keys: u32,
}

impl Melee {
    pub fn new(profile: Rc<Profile>, target: usize, daze_range: (u32, u32), seed: u32) -> Melee {
        let n = profile.techniques.len();
        Melee {
            profile,
            target,
            info: Vec::new(),
            jumps: Jumps::default(),
            technique: None,
            move_index: 0,
            committed: false,
            threshold: 0.0,
            adjust: vec![0.0; n],
            delay_until: vec![0; n],
            seeds: (0, 0),
            rng: seed.max(1),
            mv: Default::default(),
            positioned: false,
            blocking: None,
            seen_attack: None,
            react: false,
            evade: false,
            ticks_to_hit: None,
            daze: 0,
            dazed_this_time: false,
            daze_range,
            no_technique: 0,
            now: 0,
            force: None,
        }
    }

    pub(super) fn random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// A roll in `[0, 100)`.
    fn percent(&mut self) -> f32 {
        (self.random() % 100) as f32
    }

    fn reroll(&mut self) {
        self.seeds = (self.random() & 0xFFFF, self.random() & 0xFFFF);
    }

    /// The technique being run.
    pub(super) fn current(&self) -> Option<&Technique> {
        self.technique.map(|t| &self.profile.techniques[t])
    }

    /// One tick of melee for `me` against `target`.
    /// `positions`: every fighter's feet, by index (for spacing around
    /// other attackers).
    pub fn think(&mut self, me: &View, target: &View, fights: &mut Fights, now: i64, positions: &[Vec3]) -> Output {
        self.now = now;
        if self.info.is_empty() {
            self.info = self.profile.techniques.iter().map(|t| tech_info(t, me.motion)).collect();
            self.jumps = Jumps::of(me.motion);
        }
        if !me.alive || me.motion.dying || crate::anim::reaction::is_victim(me.motion.kind) {
            // Hit (or dead): the plan is off.
            self.drop_technique();
            self.blocking = None;
            fights.release(me.index);
            return Output::default();
        }
        if !target.alive {
            fights.leave(me.index);
            self.drop_technique();
            return Output::default();
        }
        fights.join(me.index, target.index);

        // Knocked down: lie dazed a while (only counting while down).
        if me.fallen() {
            if !self.dazed_this_time {
                self.dazed_this_time = true;
                let (lo, hi) = self.daze_range;
                self.daze = lo + self.random() % (hi - lo).max(1);
                fights.release(me.index);
            }
            if self.daze > 0 {
                self.daze -= 1;
                return Output::default();
            }
        } else {
            self.dazed_this_time = false;
        }

        let mut out = Output {
            face: Some(bearing(me, target)),
            ..Output::default()
        };

        // Holding a block until the attack's strikes are over.
        if let Some(b) = self.blocking {
            let still = attack_id(target) == Some(b.attack) && target.motion.frame <= b.last_strike;
            if still {
                out.held = b.keys;
                return out;
            }
            self.blocking = None;
        }

        // Step the current move.
        let busy = !me.motion.current().attacks.is_empty()
            || me.motion.current().has_flag(crate::anim::types::flag::THROW_SOURCE)
            || me.airborne;
        let mut mid_move = false;
        if self.technique.is_some() {
            mid_move = super::moves::update(self, me, target, &mut out);
        }
        if busy {
            return out; // let the attack / throw / jump play out: no new decisions
        }
        if self.technique.is_some() && !mid_move && !busy {
            self.finish_technique();
        }

        // Notice the target's attack.
        let can_react = !mid_move || super::moves::can_evade(self, me);
        let mut try_react = false;
        if can_react {
            try_react = self.judge_attack(me, target);
        }
        if mid_move && !try_react {
            return out;
        }

        // Choose what to do.
        let slot = fights.slot(me.index, now);
        if self.technique.is_none() || !self.committed {
            let pick = self.choose(me, target, slot, fights, positions);
            if let Some(t) = pick {
                if self.technique != Some(t) {
                    self.start_technique(t, me, target, slot, fights);
                }
                // Striking first beats reacting — only if it lands before the
                // incoming hit.
                let first = self.info[t].first_strike;
                if try_react && self.info[t].damaging && self.ticks_to_hit.is_some_and(|h| first < h) {
                    try_react = false;
                }
            } else if !try_react {
                self.drop_technique();
            }
        }
        if try_react {
            self.react_now(me, target);
            if let Some(b) = self.blocking {
                out.held = b.keys;
                return out;
            }
        }

        if self.technique.is_none() {
            if me.fallen() {
                out.held = crate::anim::input::key::FORWARD; // get up
                return out;
            }
            fights.release(me.index);
            self.no_technique += 1;
            // Nothing to do: walk straight at the target.
            if distance(me, target) > 8.0 {
                out.held = crate::anim::input::key::FORWARD;
            }
            return out;
        }
        self.no_technique = 0;
        super::moves::start_if_needed(self, me, target, &mut out);
        out
    }

    fn start_technique(&mut self, t: usize, _me: &View, _target: &View, _slot: bool, fights: &mut Fights) {
        eprintln!("melee: fighter {} uses {:?}", _me.index, self.profile.techniques[t].name);
        self.technique = Some(t);
        self.move_index = 0;
        self.committed = false;
        self.positioned = false;
        self.mv = Default::default();
        if self.profile.techniques[t].kind == Kind::Action {
            self.adjust[t] += REPEAT_PENALTY;
        }
        if !self.info[t].needs_slot {
            fights.release(_me.index);
        }
    }

    pub(super) fn finish_technique(&mut self) {
        for a in &mut self.adjust {
            *a = if *a >= ADJUST_ABORTED { 0.0 } else { *a * ADJUST_DECAY };
        }
        self.technique = None;
        self.move_index = 0;
        self.committed = false;
        self.mv = Default::default();
    }

    /// Gives the technique up (it won't be picked again until another
    /// finishes).
    pub(super) fn abort(&mut self) {
        if let Some(t) = self.technique {
            if self.profile.techniques[t].kind == Kind::Action {
                self.adjust[t] = ADJUST_ABORTED;
            }
        }
        self.technique = None;
        self.move_index = 0;
        self.committed = false;
        self.mv = Default::default();
        self.reroll();
    }

    fn drop_technique(&mut self) {
        self.technique = None;
        self.move_index = 0;
        self.committed = false;
        self.mv = Default::default();
    }

    pub(super) fn next_move(&mut self) {
        self.move_index += 1;
        self.mv = Default::default();
        self.reroll();
    }

    /// Notices the target's attack once per attack clip; returns whether the
    /// AI should react this tick (it would be hit).
    fn judge_attack(&mut self, me: &View, target: &View) -> bool {
        let clip = target.motion.current();
        let Some(id) = attack_id(target) else {
            self.seen_attack = None;
            return false;
        };
        let last = clip.reach.as_ref().and_then(|r| r.strikes.last()).map_or(0, |s| s.tick);
        if target.motion.frame > last {
            return false;
        }
        if self.seen_attack != Some(id) {
            self.seen_attack = Some(id);
            let p = &self.profile;
            let (notice, base, extra, threshold) =
                (p.notice_percent, p.dodge_base, p.dodge_additional, p.dodge_damage_threshold);
            let damage = clip.attacks.iter().map(|a| f32::from(a.damage)).fold(0.0, f32::max);
            self.react = self.percent() < notice;
            let dodge = base + extra * (damage / threshold.max(1.0)).min(1.0);
            self.evade = self.percent() < dodge;
        }
        if !self.react {
            return false;
        }
        // Would it hit us, and when?
        let (rel, vel) = relative(target, me);
        let attacker = Attacker {
            clip,
            tick: target.motion.frame,
            air: None,
            sphere: if target.player { 4.0 } else { 3.0 },
        };
        let victim = Target {
            pos: rel,
            velocity: vel,
            radius: me.full.0,
            height: me.full.1,
            clip: Some(me.motion.current()),
            tick: me.motion.frame,
        };
        if !geometry::bounds(&attacker, &victim, false) {
            self.ticks_to_hit = None;
            return false;
        }
        match geometry::intersect(&attacker, &victim) {
            Ok(t) => {
                self.ticks_to_hit = Some(t);
                true
            }
            Err(_) => {
                self.ticks_to_hit = None;
                false
            }
        }
    }

    /// Reacting to an incoming hit: an evasion if allowed and one fits, else
    /// a block.
    fn react_now(&mut self, me: &View, target: &View) {
        let state = me.state();
        if self.evade {
            let mut choices: Vec<(usize, f32)> = Vec::new();
            for (i, t) in self.profile.of_kind(Kind::Reaction) {
                if self.delay_until[i] > self.now || self.info[i].start != Some(state) {
                    continue;
                }
                let Some(Move::Evade(e)) = t.moves.first() else { continue };
                let ev = &tables::EVADES[*e];
                if !me.motion.coll.lookup(tables::ty(ev.anim), me.motion.state, me.motion.varient | var::FIGHT).is_some() {
                    continue;
                }
                choices.push((i, t.weight.max(0.0)));
            }
            let total: f32 = choices.iter().map(|c| c.1).sum();
            if total > 0.0 {
                let mut r = total * (self.seeds.1 as f32 / 65535.0) - 0.01;
                for (i, w) in choices {
                    r -= w;
                    if r <= 0.0 {
                        self.technique = Some(i);
                        self.move_index = 0;
                        self.committed = false;
                        self.mv = Default::default();
                        return;
                    }
                }
            }
        }
        // Block: stand against high attacks, crouch against low ones.
        let clip = target.motion.current();
        let flags = clip.attacks.iter().fold(0, |f, a| f | a.flags);
        let keys = if flags & strike::HIGH != 0 {
            0
        } else if flags & strike::LOW != 0 || state.blocks_crouching() {
            crate::anim::input::key::CROUCH
        } else {
            0
        };
        let last = clip.reach.as_ref().and_then(|r| r.strikes.last()).map_or(0, |s| s.tick);
        self.blocking = attack_id(target).map(|attack| BlockHold { attack, last_strike: last, keys });
        self.drop_technique();
    }

    /// Weighs the techniques of the pool we may use and picks one.
    fn choose(&mut self, me: &View, target: &View, slot: bool, fights: &Fights, positions: &[Vec3]) -> Option<usize> {
        let kind = if slot { Kind::Action } else { Kind::Spacing };
        let state = me.state();
        let mut weights: Vec<(usize, f32)> = Vec::new();
        let mut best_attack: f32 = 0.0;
        for (i, t) in self.profile.of_kind(kind) {
            let info = &self.info[i];
            if info.broken || self.adjust[i] >= ADJUST_ABORTED {
                continue;
            }
            if self.delay_until[i] > self.now && self.technique != Some(i) {
                continue;
            }
            let mut w = t.weight * (1.0 - self.adjust[i].min(MAX_ADJUST));
            w *= direction_weight(t, info, me, target);
            if w <= 0.0 {
                continue;
            }
            if info.start != Some(state) && !info.allow_transition {
                continue;
            }
            // A technique opening with a move from another state must be able
            // to get there (nothing leads into lying down, say).
            if let Some(s) = info.start {
                if s != state && path_cost(me, state, s).is_none() {
                    continue;
                }
            }
            w *= if kind == Kind::Action {
                self.weigh_action(t, info, me, target)
            } else {
                weigh_spacing(t, info, me, target, fights, positions)
            };
            if self.technique == Some(i) {
                w *= CURRENT_TECHNIQUE_BONUS;
            }
            if w > 0.0 {
                if info.damaging {
                    best_attack = best_attack.max(w);
                }
                weights.push((i, w));
            }
        }
        if let Some(name) = self.force.take() {
            if let Some(&(i, _)) = weights.iter().find(|(i, _)| self.profile.techniques[*i].name == name) {
                return Some(i);
            }
            self.force = Some(name); // not usable yet: keep asking
        }
        // With a slot, never prefer a harmless technique over the best attack.
        if slot && !me.fallen() {
            if best_attack <= 0.0 {
                return None;
            }
            for (i, w) in &mut weights {
                if !self.info[*i].damaging {
                    *w = w.min(best_attack);
                }
            }
        }
        // Keep the current one while it scores above the bar it was picked at.
        if let Some(cur) = self.technique {
            if let Some(&(_, w)) = weights.iter().find(|(i, _)| *i == cur) {
                if w >= self.threshold {
                    return Some(cur);
                }
            }
        }
        let total: f32 = weights.iter().map(|w| w.1).sum();
        if total <= 0.0 {
            return None;
        }
        let r = total * (self.seeds.0 as f32 / 65535.0) - 0.01;
        let mut sum = 0.0;
        for (i, w) in weights {
            sum += w;
            if sum >= r {
                self.threshold = sum - r;
                return Some(i);
            }
        }
        None
    }

    /// An action's weight factor for the current situation.
    fn weigh_action(&self, t: &Technique, info: &TechInfo, me: &View, target: &View) -> f32 {
        if t.moves.is_empty() {
            return 0.0;
        }
        let w = &self.profile.weights;
        let mut f = 1.0;
        if info.attack.is_some() {
            let (high_ok, low_ok) = target.could_block;
            f *= if !high_ok && !low_ok {
                w.not_blocking
            } else if (info.high && !high_ok) || (info.low && !low_ok) {
                w.wrong_stance
            } else if info.unblockable {
                w.unblockable
            } else if info.stagger {
                w.staggers
            } else if info.block_stun {
                w.block_stun
            } else {
                w.blocked
            };
        }
        let target_dist = distance(me, target) - target.planning.0;
        let behind = wrap(target.facing - me.facing).abs() < PI / 2.0;
        // Throws: the right side, the right range.
        if let Some(ti) = info.throw {
            let Move::Throw(index) = t.moves[ti] else { return 0.0 };
            let th = &THROWS[index];
            if th.disarm != tables::Disarm::No {
                return 0.0; // nobody carries weapons yet (#10)
            }
            let rel = wrap(target.facing - me.facing).abs();
            if th.from_behind != behind {
                return 0.0;
            }
            if th.victim_running && target.state() != MeleeState::Running {
                return 0.0; // the tackle needs a running victim
            }
            f *= if behind {
                ((PI / 2.0 - rel) / THROW_RAMP).clamp(0.0, 1.0)
            } else {
                ((rel - PI / 2.0) / THROW_RAMP).clamp(0.0, 1.0)
            };
        }
        // Walk the moves up to the first attack / throw.
        let mut run_in: Option<super::profile::RunIn> = None;
        let mut skippable = false;
        let mut jump: Option<(tables::JumpKind, Dir)> = None;
        for m in &t.moves {
            match *m {
                Move::Position { index, run_in: ri } => {
                    let p = &POSITIONS[index];
                    f *= transition_cost(me, p.states);
                    if p.jump == tables::JumpKind::Vertical && target.feet.y - me.feet.y < STANDING_JUMP_HEIGHT {
                        return 0.0; // a jump straight up only for a target well above
                    }
                    if p.jump != tables::JumpKind::None {
                        jump = Some((p.jump, p.dir));
                    }
                    let last = *p.states.last().unwrap_or(&MeleeState::Standing);
                    if me.state() == last && (!p.can_delay || ri.is_none_or(|r| r.min < 1.0)) {
                        skippable = true;
                    }
                    if p.can_delay {
                        run_in = ri;
                    }
                }
                Move::Maneuver { index, a, b, .. } => {
                    let dist = distance(me, target);
                    match index {
                        tables::maneuver::ADVANCE => {
                            let m = dist - a - b;
                            if m < 0.0 {
                                return 0.0;
                            }
                            if m < b {
                                f *= m / b;
                            }
                        }
                        tables::maneuver::RETREAT => {
                            let m = a + b - dist;
                            if m < 0.0 {
                                return 0.0;
                            }
                            if m < b {
                                f *= m / b;
                            }
                        }
                        tables::maneuver::CIRCLE_LEFT | tables::maneuver::CIRCLE_RIGHT => {
                            let rel = wrap(bearing(target, me) - target.facing);
                            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                            if rel < lo || rel > hi {
                                return 0.0;
                            }
                            let edge = (rel - lo).min(hi - rel);
                            if edge < CIRCLE_EDGE {
                                f *= edge / CIRCLE_EDGE;
                            }
                        }
                        tables::maneuver::RANDOM_STOP => break,
                        tables::maneuver::BARABBAS_WAVE => return 0.0,
                        _ => {}
                    }
                }
                Move::Evade(_) => return 0.0,
                Move::Attack(_) | Move::Throw(_) => break,
            }
        }
        // How far there is to run in before striking.
        let jumps = self.jumps;
        let reach = match (info.attack, info.throw) {
            // Jumping in, the jump itself covers the ground.
            (Some(_), _) => match jump {
                Some((kind, _)) => jumps.vel(kind) * 0.5 * (jumps.min + jumps.max),
                None => info.ideal_range,
            },
            (None, Some(ti)) => match t.moves[ti] {
                Move::Throw(i) => self.throw_range(i, me) * THROW_DIST_SAFETY,
                _ => 0.0,
            },
            _ => return f,
        };
        let mut offset = 0.0;
        if let Some(ri) = run_in {
            let test = target_dist;
            if !skippable && ri.min >= 1.0 && test < ri.min {
                return 0.0;
            }
            if target_dist < ri.min + ri.tolerance && ri.tolerance > 0.0 {
                f *= ((test - ri.min) / ri.tolerance).clamp(0.0, 1.0);
            }
            let delay = target_dist - reach;
            if !(skippable && delay < -SKIP_POSITIONING) {
                let delay = delay.max(0.0);
                if delay > ri.max {
                    return 0.0;
                }
                if delay > ri.max - ri.tolerance && ri.tolerance > 0.0 {
                    f *= (ri.max - delay) / ri.tolerance;
                }
                let speed = run_speed(me).max(0.1);
                if delay / speed > MAX_RUN_IN_TICKS {
                    return 0.0;
                }
                offset = delay;
            }
        } else if info.throw.is_some() && info.attack.is_none() && target_dist > reach / THROW_DIST_SAFETY {
            return 0.0;
        } else if let Some((kind, _)) = jump {
            // Too close to jump at.
            if target_dist < 0.5 * jumps.vel(kind) * jumps.min + target.planning.0 {
                return 0.0;
            }
        }
        // The attack itself must reach, turned toward the target.
        if let Some((_, _, clip)) = info.attack {
            let dir = info.dir.and_then(Dir::offset);
            let turn = dir.map_or(0.0, |d| wrap(bearing(me, target) - (me.facing + d)));
            if me.state().can_turn_for_attack() {
                f *= turn.cos().abs();
            }
            if let Some(hit) = self.ticks_to_hit {
                if info.first_strike >= hit {
                    f *= HIT_FIRST_WEIGHT;
                }
            }
            let c = &me.motion.coll.entries[clip].clip;
            let (rel, mut vel) = relative_turned(me, target, turn);
            let along = dir.map_or(Vec3::Z, |d| Vec3::new(d.sin(), 0.0, d.cos()));
            let air = jump.map(|(kind, jdir)| {
                if matches!(kind, tables::JumpKind::Standing | tables::JumpKind::Vertical) {
                    vel = Vec3::ZERO; // these don't lead a moving target
                }
                jumps.air(me, kind, jdir)
            });
            let attacker = Attacker { clip: c, tick: 0, air, sphere: 3.0 };
            let victim = Target {
                pos: rel - along * offset,
                velocity: vel,
                radius: target.planning.0,
                height: target.planning.1,
                clip: Some(target.motion.current()),
                tick: target.motion.frame,
            };
            if !geometry::bounds(&attacker, &victim, true) {
                return 0.0;
            }
        }
        f
    }

    /// A throw move's range for our animation set.
    pub(super) fn throw_range(&self, index: usize, me: &View) -> f32 {
        let th = &THROWS[index];
        let state = if th.running { MeleeState::Running } else { MeleeState::Standing };
        me.motion
            .coll
            .all(tables::ty(th.anim), state.anim_state())
            .into_iter()
            .filter_map(|c| me.motion.coll.entries[c].clip.throw.map(|t| t.range))
            .fold(0.0, f32::max)
    }
}

/// Builds fighter `i`'s view, as seen by fighter `observer` (whether `i`
/// could block `observer`'s attacks).
pub fn view(fighters: &[crate::fighter::Fighter], i: usize, observer: usize) -> View<'_> {
    use crate::anim::types::flag;
    let f = &fighters[i];
    let feet = f.feet();
    let fallen = MeleeState::from_anim_state(f.motion.state) == MeleeState::Prone;
    // Core parts (pelvis, spine, chest, head, feet) — plus hands for an
    // incoming attack; planning our own, feet count only for height (unless
    // lying) and the radius is kept back to 0.75.
    const CORE: [usize; 6] = [0, 7, 8, 10, 3, 6];
    const HANDS: [usize; 2] = [14, 18];
    let spheres = f.body_spheres();
    let cylinder = |parts: &[usize], feet_for_height_only: bool| {
        let (mut r, mut lo, mut hi) = (0.0f32, f32::MAX, f32::MIN);
        for &(part, c, rad) in spheres.iter().filter(|(p, _, _)| parts.contains(p)) {
            let is_foot = part == 3 || part == 6;
            if !(feet_for_height_only && is_foot) {
                let d = Vec3::new(c.x - feet.x, 0.0, c.z - feet.z).length();
                r = r.max(d + rad);
            }
            lo = lo.min(c.y - rad - feet.y);
            hi = hi.max(c.y + rad - feet.y);
        }
        if lo > hi {
            (3.0, (0.0, 15.0))
        } else {
            (r, (lo, hi))
        }
    };
    let mut all: Vec<usize> = CORE.to_vec();
    all.extend(HANDS);
    let full = cylinder(&all, false);
    let (pr, ph) = cylinder(&CORE, !fallen);
    let could_block = {
        let o = &fighters[observer];
        let d = o.feet() - feet;
        let toward = d.x.atan2(d.z);
        let facing_it = wrap(toward - f.facing.aim()).abs() <= 20.0f32.to_radians();
        match (f.motion.defensive() && facing_it, f.motion.block_clip()) {
            (true, Some(c)) => {
                let c = f.motion.clip_of(c);
                (c.has_flag(flag::BLOCK_HIGH), c.has_flag(flag::BLOCK_LOW))
            }
            _ => (false, false),
        }
    };
    View {
        index: i,
        feet,
        facing: f.facing.facing,
        velocity: f.velocity,
        motion: &f.motion,
        full,
        planning: (pr * 0.75, ph),
        alive: f.alive() && !f.motion.dying,
        airborne: f.airborne(),
        could_block,
        knockdown_resistant: f.motion.tuning.knockdown_resistant,
        player: f.player,
    }
}

/// A class's jumping, for planning jump-ins: how long a jump lasts (ticks,
/// without and with holding jump the whole way) and how fast each kind of
/// jump travels.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Jumps {
    pub min: f32,
    pub max: f32,
    vel: [f32; 7],
    amount: f32,
    gravity: f32,
}

/// How much higher a target must be to jump straight up at it.
const STANDING_JUMP_HEIGHT: f32 = 7.0;

impl Jumps {
    fn of(motion: &Motion) -> Jumps {
        let t = motion.tuning;
        let g = t.jump_gravity.max(1e-3);
        let min = 2.0 * t.jump_amount / g;
        let max = min + 2.0 * t.jump_accel_amount * f32::from(t.num_jump_accel_frames) / g;
        // A running jump keeps the run's speed: the first tick of each run.
        let first = |anim: &str, state: MeleeState, axis: usize, sign: f32| {
            motion
                .coll
                .lookup(tables::ty(anim), state.anim_state(), var::FIGHT)
                .and_then(|c| motion.coll.entries[c].clip.root_motion.first().map(|m| m[axis] * sign))
                .unwrap_or(0.0)
        };
        let mut vel = [0.0; 7];
        vel[3] = first("run", MeleeState::Running, 1, 1.0);
        vel[4] = first("runBackwards", MeleeState::RunningBack, 1, -1.0);
        vel[5] = first("runSidestepLeft", MeleeState::RunningLeft, 0, 1.0);
        vel[6] = first("runSidestepRight", MeleeState::RunningRight, 0, -1.0);
        // Any jump goes sideways at least half as fast as it goes up.
        for v in &mut vel[2..] {
            *v = v.max(0.5 * t.jump_amount);
        }
        Jumps { min, max, vel, amount: t.jump_amount, gravity: g }
    }

    pub fn vel(&self, kind: tables::JumpKind) -> f32 {
        self.vel[kind as usize]
    }

    /// The flight to plan a jump attack with: our current motion plus the
    /// jump, at least the jump's speed along its direction.
    pub fn air(&self, me: &View, kind: tables::JumpKind, dir: Dir) -> geometry::Air {
        let mut v = me.motion.velocity_estimate();
        v.y = self.amount;
        if let Some(off) = dir.offset() {
            let d = Vec3::new(off.sin(), 0.0, off.cos());
            let along = v.dot(d);
            if along < self.vel(kind) {
                v += d * (self.vel(kind) - along);
            }
        }
        geometry::Air {
            velocity: v,
            gravity: self.gravity,
            expected_ticks: (0.5 * (self.min + self.max)).round() as u16,
        }
    }
}

/// Identifies the target's current attack clip (one per started attack).
pub(super) fn attack_id(target: &View) -> Option<u32> {
    let c = target.motion.current();
    (!c.attacks.is_empty() && !c.has_flag(crate::anim::types::flag::THROW_SOURCE))
        .then_some(target.motion.attacks_started)
}

/// World yaw from `a` to `b`.
pub(super) fn bearing(a: &View, b: &View) -> f32 {
    let d = b.feet - a.feet;
    d.x.atan2(d.z)
}

pub(super) fn distance(a: &View, b: &View) -> f32 {
    let d = b.feet - a.feet;
    (d.x * d.x + d.z * d.z).sqrt()
}

/// `of`'s position and velocity in `from`'s frame.
pub(super) fn relative(from: &View, of: &View) -> (Vec3, Vec3) {
    relative_turned(from, of, 0.0)
}

/// As [`relative`], with `from`'s frame turned by `turn` (to point an attack
/// at `of`).
pub(super) fn relative_turned(from: &View, of: &View, turn: f32) -> (Vec3, Vec3) {
    let q = Quat::from_rotation_y(-(from.facing + turn));
    (q * (of.feet - from.feet), q * of.velocity)
}

/// The body's run-in speed (units per tick): the forward motion of the clip
/// a running body keeps playing.
fn run_speed(me: &View) -> f32 {
    let coll = &me.motion.coll;
    let running = MeleeState::Running;
    coll.lookup(tables::ty(running.delay_anim()), running.anim_state(), me.motion.varient | var::FIGHT)
        .map(|c| {
            let rm = &coll.entries[c].clip.root_motion;
            rm.iter().map(|m| m[1]).sum::<f32>() / rm.len().max(1) as f32
        })
        .unwrap_or(1.0)
}

/// How well a technique's direction points at the target: full within 45°,
/// none past 60° (generous: 70°/110°), forward never below 0.35.
fn direction_weight(t: &Technique, info: &TechInfo, me: &View, target: &View) -> f32 {
    let Some(dir) = info.dir else { return 1.0 };
    let Some(off) = dir.offset() else { return 1.0 };
    let delta = wrap(bearing(me, target) - (me.facing + off)).abs();
    let (lo, hi) = if t.generous_direction { (70.0f32, 110.0f32) } else { (45.0, 60.0) };
    let (lo, hi) = (lo.to_radians(), hi.to_radians());
    let w = if delta <= lo {
        1.0
    } else if delta >= hi {
        0.0
    } else {
        (hi - delta) / (hi - lo)
    };
    if dir == Dir::Forward {
        w.max(0.35)
    } else {
        w
    }
}

/// A spacing technique's weight factor: harmless moves only; advance /
/// retreat toward a band; circle away from other attackers.
fn weigh_spacing(
    t: &Technique,
    info: &TechInfo,
    me: &View,
    target: &View,
    fights: &Fights,
    positions: &[Vec3],
) -> f32 {
    if info.needs_slot {
        return 0.0;
    }
    let dist = distance(me, target);
    let mut f = 1.0;
    for m in &t.moves {
        match *m {
            Move::Attack(_) | Move::Position { .. } | Move::Throw(_) => return 0.0,
            Move::Maneuver { index, a, b, .. } => match index {
                tables::maneuver::ADVANCE => f *= ((dist - a) / b.max(MIN_TOLERANCE)).max(0.0),
                tables::maneuver::RETREAT => f *= ((a - dist) / b.max(MIN_TOLERANCE)).max(0.0),
                tables::maneuver::CIRCLE_LEFT | tables::maneuver::CIRCLE_RIGHT => {
                    f *= crowd(index == tables::maneuver::CIRCLE_LEFT, a, me, target, fights, positions);
                }
                tables::maneuver::RANDOM_STOP => break,
                tables::maneuver::BARABBAS_WAVE => return 0.0,
                _ => {}
            },
            Move::Evade(_) => {}
        }
    }
    f
}

/// The cost of getting from `from` to `goal` through the transition graph
/// (the product of the steps' weights), if the body can.
pub(super) fn path_cost(me: &View, from: MeleeState, goal: MeleeState) -> Option<f32> {
    let mo = me.motion;
    let mut cur = from;
    let mut cost = 1.0;
    for _ in 0..4 {
        if cur == goal {
            return Some(cost);
        }
        let usable = |t: &&tables::Transition| {
            t.from == cur
                && t.anims
                    .iter()
                    .any(|a| mo.coll.lookup(tables::ty(a), cur.anim_state(), mo.varient | var::FIGHT).is_some())
        };
        // Straight to the goal, or else via standing.
        let step = tables::TRANSITIONS
            .iter()
            .filter(usable)
            .find(|t| t.to.contains(&goal))
            .map(|t| (t.weight, goal))
            .or_else(|| {
                tables::TRANSITIONS
                    .iter()
                    .filter(usable)
                    .find(|t| t.to.contains(&MeleeState::Standing))
                    .map(|t| (t.weight, MeleeState::Standing))
            })?;
        cost *= step.0;
        if step.1 == cur {
            return None;
        }
        cur = step.1;
    }
    (cur == goal).then_some(cost)
}

/// The preference cost of getting from the current state through `states`
/// (the product of the transitions' weights; getting up is heavily avoided).
fn transition_cost(me: &View, states: &[MeleeState]) -> f32 {
    let mo = me.motion;
    let mut cur = me.state();
    let mut cost = 1.0;
    for &goal in states {
        let mut hops = 0;
        while cur != goal && hops < 4 {
            hops += 1;
            let step = tables::TRANSITIONS.iter().find(|t| {
                t.from == cur
                    && t.to.contains(&goal)
                    && t.anims.iter().any(|a| {
                        mo.coll.lookup(tables::ty(a), cur.anim_state(), mo.varient | var::FIGHT).is_some()
                    })
            });
            match step {
                Some(t) => {
                    cost *= t.weight;
                    cur = goal;
                }
                None => return cost,
            }
        }
    }
    cost
}

/// Circling around the target: others ahead in the circling direction (at a
/// similar bearing) count against it, others behind for it.
fn crowd(left: bool, comfort: f32, me: &View, target: &View, fights: &Fights, positions: &[Vec3]) -> f32 {
    const SAME_DISTANCE: f32 = 10.0;
    let ignore_far = 20.0f32.to_radians();
    let urgent = 30.0f32.to_radians();
    let from = |p: Vec3| {
        let d = p - target.feet;
        (d.x.atan2(d.z), (d.x * d.x + d.z * d.z).sqrt())
    };
    let (mine, my_dist) = from(me.feet);
    // Circling left (sidestepping left while facing the target) lowers the
    // bearing from the target, so "ahead" is the lower side.
    let sign = if left { -1.0 } else { 1.0 };
    let mut sum = 0.0;
    for &o in fights.attackers(target.index) {
        let Some(&p) = positions.get(o) else { continue };
        if o == me.index {
            continue;
        }
        let (theirs, their_dist) = from(p);
        if their_dist > my_dist + SAME_DISTANCE && fights.holds_slot(o) {
            continue;
        }
        let rel = wrap(theirs - mine) * sign;
        if rel.abs() > comfort {
            continue;
        }
        let ignore = if (their_dist - my_dist).abs() <= SAME_DISTANCE { 0.0 } else { ignore_far };
        let pull = if rel.abs() <= urgent || comfort <= urgent {
            1.0
        } else {
            (comfort - rel.abs()) / (comfort - urgent)
        };
        if rel > ignore {
            sum -= pull;
        } else if rel < -ignore {
            sum += pull;
        }
    }
    (1.0 + sum).max(0.0)
}

/// What's fixed about a technique for `motion`'s animation set.
fn tech_info(t: &Technique, motion: &Motion) -> TechInfo {
    let coll = &motion.coll;
    let mut info = TechInfo::default();
    let rules = crate::anim::rules::fighting();
    for (mi, m) in t.moves.iter().enumerate() {
        match *m {
            Move::Attack(a) => {
                let am = &ATTACKS[a];
                if mi == 0 {
                    info.start = Some(am.from);
                }
                if info.dir.is_none() {
                    info.dir = Some(am.dir);
                }
                info.damaging = true;
                info.needs_slot = true;
                let anim = tables::ty(am.anim);
                if am.keys.is_none() && !rules.iter().any(|r| r.anim == anim) {
                    info.broken = true; // no way to ask for it (as in the original)
                }
                if info.attack.is_none() {
                    let found = coll.lookup(anim, am.from.anim_state(), var::FIGHT | motion.varient);
                    match found {
                        Some(clip) => {
                            let c = &coll.entries[clip].clip;
                            info.attack = Some((mi, anim, clip));
                            let flags = c.attacks.iter().fold(0, |f, a| f | a.flags);
                            info.high = flags & strike::HIGH != 0;
                            info.low = flags & strike::LOW != 0;
                            info.unblockable = flags & strike::UNBLOCKABLE != 0;
                            info.block_stun = c.attacks.iter().any(|a| a.block_stun > BLOCK_STUN_THRESHOLD);
                            info.stagger = c.attacks.iter().any(|a| a.stagger > 0);
                            if let Some(r) = &c.reach {
                                info.ideal_range = r.max_distance - 0.5 * r.farthest.distance;
                                info.first_strike = r.first.tick;
                            }
                        }
                        None => info.broken = true,
                    }
                }
            }
            Move::Position { index, .. } => {
                if mi == 0 {
                    info.allow_transition = true;
                }
                if info.dir.is_none() && POSITIONS[index].dir != Dir::None {
                    info.dir = Some(POSITIONS[index].dir);
                }
            }
            Move::Maneuver { index, .. } => {
                if mi == 0 {
                    info.allow_transition = true;
                    let from = MANEUVERS[index].from;
                    info.start = (from != MeleeState::None).then_some(from);
                }
            }
            Move::Evade(e) => {
                if mi == 0 {
                    info.start = Some(tables::EVADES[e].from);
                }
            }
            Move::Throw(i) => {
                if mi == 0 {
                    info.start = Some(if THROWS[i].running { MeleeState::Running } else { MeleeState::Standing });
                }
                info.damaging = true;
                info.needs_slot = true;
                if info.throw.is_none() {
                    info.throw = Some(mi);
                }
                if info.dir.is_none() {
                    info.dir = Some(Dir::Forward);
                }
            }
        }
    }
    if info.start.is_none() && !info.allow_transition {
        info.allow_transition = true;
    }
    info
}
