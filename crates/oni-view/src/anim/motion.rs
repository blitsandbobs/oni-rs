//! A character's animation state and the rules for changing it.
//!
//! The model (behaviour per `docs/MOVEMENT.md`):
//!
//! * The character plays one clip at a time. Each clip ends in a body *state*;
//!   every request for a new animation type is looked up from that state, so
//!   the same request ("run", "punch") resolves to different clips depending
//!   on how the body is currently posed (standing, crouched, airborne, …).
//! * A request is either *queued* — remembered and played when the current
//!   clip ends — or *forced* in immediately with a short pose blend. Ordinary
//!   ("appropriate") requests decide for themselves: they queue while the
//!   current clip is in its uninterruptible (atomic) range, when they would
//!   just restart the same loop, or when the current clip is a throw, and
//!   force otherwise. A clip's shortcut may be marked to break atomicity.
//! * After a clip that imposes a pause, only resting/defensive clips may start
//!   until the pause runs out.
//! * Attacks are remembered (`last_attack`) so follow-up presses can chain;
//!   the memory lapses shortly after the attack ends.
//! * Entering a clip that starts from an airborne state launches the
//!   character (`please_jump`); the owner integrates the flight.

use std::rc::Rc;

use glam::{Quat, Vec3};

use crate::anim::collection::AnimCollection;
use crate::anim::reaction::{self, Victim};
use crate::anim::throw;
use crate::anim::types::{flag, shortcut, st, ty, var};
use crate::quirks::Quirks;
use crate::skel::{self, Character, Clip};

/// Default blend length when a request interrupts the current clip.
pub const BLEND_FRAMES: u16 = 8;
/// A late attack press may replace a clip this many frames into it.
const LATE_PRESS_FRAMES: u16 = 8;
/// The combo memory survives this many ticks after the attack ends.
pub const COMBO_WINDOW: u64 = 10;

/// How insistently an animation is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urge {
    /// Let the rules decide between queueing and interrupting.
    Appropriate,
    /// Play when the current clip ends.
    Queue,
    /// Interrupt now.
    Force,
}

/// Per-class tuning that drives the model (from `classes.json`).
#[derive(Clone, Copy, Debug)]
pub struct ClassTuning {
    /// Ticks the fight stance lingers after a fighting move.
    pub fight_mode_duration: u16,
    /// Base ticks of inactivity before an idle fidget.
    pub idle_delay: u32,
    /// Base ticks between repeated idle fidgets.
    pub idle_restart_delay: u32,
    pub max_hit_points: i32,
    /// Shrugs off light hits and is only staggered by knockdowns.
    pub knockdown_resistant: bool,
    /// Airborne integration (see `oni_geom::charclass::AirConstants`).
    pub jump_gravity: f32,
    pub jump_amount: f32,
    pub max_velocity: f32,
    pub jump_accel_amount: f32,
    pub num_jump_accel_frames: u16,
    pub damage_start: f32,
    pub damage_end: f32,
}

impl Default for ClassTuning {
    /// Konoko's values, used when the class catalogue is unavailable.
    fn default() -> Self {
        ClassTuning {
            fight_mode_duration: 120,
            idle_delay: 600,
            idle_restart_delay: 600,
            max_hit_points: 100,
            knockdown_resistant: false,
            jump_gravity: 0.06,
            jump_amount: 1.1,
            max_velocity: -4.0,
            jump_accel_amount: 0.03,
            num_jump_accel_frames: 20,
            damage_start: 45.0,
            damage_end: 135.0,
        }
    }
}

/// The pose blend after a switch: the pose at the switch, faded out.
struct Blend {
    saved: Vec<Quat>,
    height: f32,
    /// Root motion per tick at the switch (character-local), faded into the
    /// new clip's own root motion.
    velocity: Vec3,
    itr: u16,
    count: u16,
}

impl Blend {
    /// Progress into the new clip, 0 → 1.
    fn t(&self) -> f32 {
        f32::from(self.itr) / f32::from(self.count.max(1))
    }
}

/// Airborne state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Air {
    /// Ticks airborne (0 = on the ground).
    pub frames: u16,
    /// Velocity per tick, character-local (x left, z forward, y up).
    pub velocity: Vec3,
    /// Takeoff velocity prepared by a jump request, if any.
    pub build: Option<Vec3>,
    /// Height of the feet when the character left the ground.
    pub jump_height: f32,
    /// Grounded ticks with no floor under the feet (walking off a ledge).
    pub off_ground: u16,
}

/// One character's animation state.
pub struct Motion {
    pub coll: Rc<AnimCollection>,
    /// The collection the current clip belongs to: our own, except while
    /// playing another character's clip (the victim's side of its throw).
    playing: Rc<AnimCollection>,
    pub tuning: ClassTuning,
    /// Current clip (index into `playing`) and ticks into it.
    pub clip: usize,
    pub frame: u16,
    /// The state the current clip was entered from.
    pub entered_from: u16,
    /// The current clip's animation type.
    pub kind: u16,
    /// The type to play when the current clip ends (0 = none requested).
    pub queued: u16,
    /// The state the body is in for lookups (the current clip's end state).
    pub state: u16,
    pub varient: u16,
    blend: Option<Blend>,
    /// Remaining frames of the previous blend, carried into an overlapping one.
    blend_carry: u16,
    pub last_attack: u16,
    /// When the last attack ended (None while it is still playing).
    attack_ended: Option<u64>,
    pub soft_pause: u16,
    pub hard_pause: u16,
    pub hit_stun: u16,
    pub block_stun: u16,
    pub dizzy_stun: u16,
    pub stagger_stun: u16,
    pub fight_frames: u16,
    pub lock_frames: u16,
    pub min_blend: u16,
    /// A turn-in-place has been started in this direction.
    pub turn_left: bool,
    pub turn_right: bool,
    pub please_jump: bool,
    pub please_land: bool,
    pub please_land_hard: bool,
    pub air: Air,
    /// Feet height this tick (the owner keeps it current).
    pub feet_y: f32,
    /// Yaw a finished clip carried the body through, not yet applied to facing.
    facing_change: f32,
    /// Game time in ticks.
    pub now: u64,
    /// Whether this character plays idle fidgets.
    pub idle_enabled: bool,
    idle_delay: u32,
    last_active: u64,
    /// A clip started since the owner last checked (clears per-swing hit sets).
    pub new_clip: bool,
    /// Who this character could throw this tick (set by the world for
    /// characters whose strikes turn into throws: the player).
    pub throw_sense: Option<throw::Sense>,
    /// A throw this character began this tick (for the world to carry out).
    pub throw_started: Option<throw::Started>,
    /// An attack clip started since the owner last checked (auto-facing).
    pub attack_began: bool,
    /// How many attack clips have started (an AI watches its combo progress).
    pub attacks_started: u32,
    /// Killed: plays out its death reaction, makes no voluntary moves.
    pub dying: bool,
    /// The death reaction has finished: the body lies still.
    pub corpse: bool,
    rng: u32,
}

/// Animation types after which the body is at rest (no fidget pending).
fn is_rest(kind: u16) -> bool {
    matches!(kind, ty::STAND | ty::IDLE)
}

/// Clip types a pause still allows (resting / defensive / landing).
fn pause_allows(c: &Clip, allow_block: bool) -> bool {
    if !c.attacks.is_empty() {
        return false;
    }
    match c.anim_type {
        ty::BLOCK => allow_block,
        ty::STAND
        | ty::CROUCH
        | ty::LAND
        | ty::LAND_FORWARD
        | ty::LAND_RIGHT
        | ty::LAND_LEFT
        | ty::LAND_BACK => true,
        _ => false,
    }
}

/// Whether going from `cur` to `next` just continues the same motion (a loop
/// repeating, or a start clip flowing into its loop) — such requests are
/// always queued rather than blended.
fn continues(cur: u16, next: u16) -> bool {
    cur == next
        || matches!(
            (cur, next),
            (ty::RUN_START, ty::RUN)
                | (ty::WALK_START, ty::WALK)
                | (ty::RUN_BACKWARDS_START, ty::RUN_BACKWARDS)
                | (ty::WALK_BACKWARDS_START, ty::WALK_BACKWARDS)
                | (ty::RUN_SIDESTEP_LEFT_START, ty::RUN_SIDESTEP_LEFT)
                | (ty::RUN_SIDESTEP_RIGHT_START, ty::RUN_SIDESTEP_RIGHT)
        )
}

/// Wraps an angle into `[0, 2π)`.
fn wrap(a: f32) -> f32 {
    a.rem_euclid(std::f32::consts::TAU)
}

impl Motion {
    /// A character standing at rest.
    pub fn new(coll: Rc<AnimCollection>, tuning: ClassTuning) -> Motion {
        let clip = coll
            .lookup(ty::STAND, st::STANDING, 0)
            .or_else(|| coll.lookup(ty::IDLE, st::STANDING, 0))
            .unwrap_or(0);
        let (kind, state) = coll
            .entries
            .get(clip)
            .map(|e| (e.clip.anim_type, e.clip.to_state))
            .unwrap_or((ty::STAND, st::STANDING));
        Motion {
            playing: coll.clone(),
            coll,
            tuning,
            clip,
            frame: 0,
            entered_from: st::STANDING,
            kind,
            queued: ty::NONE,
            state,
            varient: 0,
            blend: None,
            blend_carry: 0,
            last_attack: ty::NONE,
            attack_ended: Some(0),
            soft_pause: 0,
            hard_pause: 0,
            hit_stun: 0,
            block_stun: 0,
            dizzy_stun: 0,
            stagger_stun: 0,
            fight_frames: 0,
            lock_frames: 0,
            min_blend: 0,
            turn_left: false,
            turn_right: false,
            please_jump: false,
            please_land: false,
            please_land_hard: false,
            air: Air::default(),
            feet_y: 0.0,
            facing_change: 0.0,
            now: 0,
            idle_enabled: true,
            idle_delay: tuning.idle_delay,
            last_active: 0,
            new_clip: false,
            throw_sense: None,
            throw_started: None,
            attack_began: false,
            attacks_started: 0,
            dying: false,
            corpse: false,
            rng: 0x2545_f491,
        }
    }

    // --- queries ---------------------------------------------------------

    pub fn clip_of(&self, i: usize) -> &Clip {
        &self.coll.entries[i].clip
    }

    /// The current clip.
    pub fn current(&self) -> &Clip {
        &self.playing.entries[self.clip].clip
    }

    /// The current clip's instance name.
    pub fn name(&self) -> &str {
        &self.playing.entries[self.clip].name
    }

    /// Whether the current clip is one of our own (not a borrowed one).
    fn playing_own(&self) -> bool {
        Rc::ptr_eq(&self.playing, &self.coll)
    }

    /// Length of the current clip in ticks.
    pub fn length(&self) -> u16 {
        self.current().frames()
    }

    /// The (hard, soft) pause the current clip leaves before our clip `to`.
    fn pauses_into(&self, to: usize) -> (u16, u16) {
        if self.playing_own() {
            (self.coll.hard_pause(self.clip, to), self.coll.soft_pause(self.clip, to))
        } else {
            (self.current().hard_pause, self.current().soft_pause)
        }
    }

    /// The blend length from the current clip into our clip `to` at its end.
    fn blend_into(&self, to: usize) -> u16 {
        if self.playing_own() {
            self.coll.end_interpolation(self.clip, to)
        } else {
            crate::anim::collection::blend_between(self.current(), self.clip_of(to), false)
        }
    }

    /// Seconds into the current clip (for sampling keyframes).
    pub fn frame_time(&self) -> f32 {
        f32::from(self.frame) / skel::FPS
    }

    /// The saved pose and its weight while a blend is in progress.
    pub fn blend_pose(&self) -> Option<(&[Quat], f32)> {
        self.blend.as_ref().map(|b| (b.saved.as_slice(), 1.0 - b.t()))
    }

    /// Pelvis height above the feet this tick (blended during a switch).
    pub fn height(&self) -> f32 {
        let h = self.current().height_at(self.frame_time());
        match &self.blend {
            Some(b) => b.height + (h - b.height) * b.t(),
            None => h,
        }
    }

    /// This tick's character-local root motion (blended during a switch).
    pub fn root_delta(&self) -> Vec3 {
        let own = self.root_at(self.frame);
        match &self.blend {
            Some(b) => b.velocity + (own - b.velocity) * b.t(),
            None => own,
        }
    }

    fn root_at(&self, frame: u16) -> Vec3 {
        let rm = &self.current().root_motion;
        rm.get(usize::from(frame).min(rm.len().saturating_sub(1)))
            .map(|&[x, z]| Vec3::new(x, 0.0, z))
            .unwrap_or(Vec3::ZERO)
    }

    /// Average root motion over the last few frames of the current clip.
    pub fn velocity_estimate(&self) -> Vec3 {
        let last = self.length().saturating_sub(1);
        let mut f = self.frame.min(last);
        let mut sum = Vec3::ZERO;
        let mut n = 0.0;
        for _ in 0..4 {
            sum += self.root_at(f);
            n += 1.0;
            if f == 0 {
                break;
            }
            f -= 1;
        }
        sum / n
    }

    /// The attacks whose damage window contains the current frame, with their
    /// index within the clip (a clip has up to two).
    pub fn active_attacks(&self) -> Vec<(usize, skel::Attack)> {
        let f = self.frame;
        self.current()
            .attacks
            .iter()
            .enumerate()
            .filter(|(_, a)| a.first_damage_frame <= f && f <= a.last_damage_frame)
            .map(|(k, a)| (k, *a))
            .collect()
    }

    /// Whether the current clip needs variant bits the character no longer has.
    pub fn bad_varient(&self) -> bool {
        self.current().varient & !self.varient != 0
    }

    /// Yaw finished clips have turned the body by, since the last call.
    pub fn take_facing_change(&mut self) -> f32 {
        std::mem::take(&mut self.facing_change)
    }

    /// A value in `[0, n)` from this character's generator.
    fn random_below(&mut self, n: u32) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        if n == 0 {
            0
        } else {
            x % n
        }
    }

    // --- requests --------------------------------------------------------

    /// Requests animation type `kind`. Returns the clip it resolved to (also
    /// when merely queued), or None when there is no such clip for the current
    /// state or a pause forbids it. `late_ok` lets an attack press that came
    /// just after a switch replace the clip that started.
    pub fn request(
        &mut self,
        ch: &Character,
        mut urge: Urge,
        late_ok: bool,
        kind: u16,
    ) -> Option<usize> {
        if self.dying {
            return None; // the dead make no voluntary moves
        }
        let mut found = self.coll.lookup(kind, self.state, self.varient);
        if found.is_none() && late_ok && self.frame < LATE_PRESS_FRAMES {
            found = self.coll.lookup(kind, self.entered_from, self.varient);
            urge = Urge::Force;
        }
        let clip = found?;

        let next = self.clip_of(clip);
        let (hard_after, soft_after) = self.pauses_into(clip);
        let hard = self.hard_pause > 0 || hard_after > 0;
        let soft = self.hit_stun > 0 || self.block_stun > 0 || self.soft_pause > 0 || soft_after > 0;
        if (hard && !pause_allows(next, false)) || (!hard && soft && !pause_allows(next, true)) {
            return None;
        }

        if urge == Urge::Appropriate {
            urge = self.decide_urge(clip);
        }
        let standing_still = kind == ty::STAND && self.state == st::STANDING;
        if !standing_still && self.fight_frames == 0 {
            self.varient &= !var::FIGHT;
        }

        if urge == Urge::Queue || (!self.bad_varient() && continues(self.kind, kind)) {
            self.queued = kind;
            return Some(clip);
        }

        // Interrupt: blend in over the default length, or the shortcut's.
        let clip = self.maybe_throw(clip);
        let from = self.state;
        let mut n = BLEND_FRAMES.max(self.min_blend);
        if let Some(s) = self.clip_of(clip).shortcut_from(from) {
            n = s.length;
        }
        n = n.min(self.clip_of(clip).max_interpolation);
        let turn = self.leave_clip(true);
        self.enter(ch, from, clip, n, turn);
        self.min_blend = 0;
        Some(clip)
    }

    /// Whether an ordinary request for `clip` should queue or interrupt.
    fn decide_urge(&self, clip: usize) -> Urge {
        let (cur, next) = (self.current(), self.clip_of(clip));
        if next.anim_type == self.kind && next.varient == cur.varient && next.to_state == cur.to_state
        {
            return Urge::Queue; // same loop: let it run on
        }
        // A hit reaction or a throw plays out; nothing ordinary cuts it short.
        if reaction::is_victim(self.kind)
            || cur.has_flag(flag::THROW_TARGET)
            || cur.has_flag(flag::THROW_SOURCE)
        {
            return Urge::Queue;
        }
        if !cur.is_atomic(self.frame) {
            return Urge::Force;
        }
        match next.shortcut_from(cur.to_state) {
            Some(s) if s.flags & shortcut::NUKE != 0 => Urge::Force,
            _ => Urge::Queue,
        }
    }

    /// The current clip is ending: settle its combo timing and the yaw it
    /// turned the body through. Returns that yaw.
    fn leave_clip(&mut self, apply_turn: bool) -> f32 {
        if self.kind == self.last_attack && self.attack_ended.is_none() {
            self.attack_ended = Some(self.now);
        }
        let turn = if apply_turn {
            wrap(self.current().final_rotation)
        } else {
            0.0
        };
        self.facing_change += turn;
        turn
    }

    /// Starts `clip`, entered from `from`, blending over `n` frames. `turn` is
    /// the yaw just committed to facing; the saved pose is counter-rotated so
    /// the blend starts exactly where the body was.
    fn enter(&mut self, ch: &Character, from: u16, clip: usize, n: u16, turn: f32) {
        let own = self.coll.clone();
        self.enter_in(ch, own, from, clip, n, turn);
    }

    /// [`Self::enter`] for clip `clip` of collection `coll` (ours, or another
    /// character's).
    fn enter_in(
        &mut self,
        ch: &Character,
        coll: Rc<AnimCollection>,
        from: u16,
        clip: usize,
        n: u16,
        turn: f32,
    ) {
        if n > 0 {
            let mut saved = ch.local_rotations(Some(self.current()), self.frame_time());
            if let Some((prev, w)) = self.blend_pose() {
                for (q, p) in saved.iter_mut().zip(prev) {
                    let p = if q.dot(*p) < 0.0 { -*p } else { *p };
                    *q = q.lerp(p, w).normalize();
                }
            }
            if turn != 0.0 {
                if let Some(root) = saved.first_mut() {
                    *root = Quat::from_rotation_y(-turn) * *root;
                }
            }
            let mut velocity = self.velocity_estimate();
            if coll.entries[clip].clip.has_flag(flag::NO_INTERP_VELOCITY) {
                velocity = Vec3::ZERO;
            }
            let carry = self.blend.as_ref().map_or(self.blend_carry, |b| b.count - b.itr);
            self.blend = Some(Blend {
                saved,
                height: self.height(),
                velocity,
                itr: 0,
                count: n.max(carry),
            });
        }

        self.playing = coll;
        self.clip = clip;
        self.frame = 0;
        self.entered_from = from;
        self.kind = self.current().anim_type;
        self.queued = ty::NONE;
        self.state = self.current().to_state;
        self.started();
    }

    /// Bookkeeping when a clip begins.
    fn started(&mut self) {
        self.new_clip = true;
        let c = self.current();
        let (is_attack, throws, from_state, clip_varient) = (
            !c.attacks.is_empty(),
            c.has_flag(flag::THROW_SOURCE),
            c.from_state,
            c.varient,
        );
        if is_attack && !throws {
            self.last_attack = self.kind;
            self.attack_ended = None;
            self.attack_began = true;
            self.attacks_started += 1;
        }
        if self.dying && !reaction::is_victim(self.kind) {
            self.corpse = true; // the death reaction is over: lie still
        }
        if !is_rest(self.kind) {
            self.last_active = self.now;
        } else if self.kind == ty::IDLE {
            self.reroll_idle_delay();
            let restart = self.tuning.idle_restart_delay;
            let jitter = self.random_below(restart / 2 + 1);
            self.last_active =
                (self.now + u64::from(restart) + u64::from(jitter)).saturating_sub(u64::from(self.idle_delay));
        }
        if st::is_flying(from_state) {
            self.please_jump = true;
            if self.air.frames == 0 {
                self.air.jump_height = self.feet_y;
            }
        }
        if self.fight_frames == 0 && clip_varient & var::FIGHT == 0 {
            self.varient &= !var::FIGHT;
        }
    }

    /// The current clip has run out: play what was queued (or stand).
    fn next_clip(&mut self, ch: &Character) {
        let next = if self.lock_frames > 0 && self.playing_own() {
            self.clip
        } else {
            self.coll
                .lookup(self.queued, self.state, self.varient)
                .or_else(|| self.coll.lookup(ty::STAND, st::STANDING, self.varient))
                .unwrap_or(self.clip)
        };
        let next = if self.lock_frames > 0 { next } else { self.maybe_throw(next) };
        let (hard_after, soft_after) = self.pauses_into(next);
        self.soft_pause = self.soft_pause.max(soft_after);
        self.hard_pause = self.hard_pause.max(hard_after);
        let n = self.blend_into(next);
        let turn = self.leave_clip(true);
        let from = self.state;
        self.enter(ch, from, next, n, turn);
    }

    /// A strike about to start may become a throw instead (see
    /// [`throw`]). Returns the clip to start.
    fn maybe_throw(&mut self, clip: usize) -> usize {
        let Some(sense) = &self.throw_sense else {
            return clip;
        };
        let strike = self.clip_of(clip).anim_type;
        match throw::attempt(&self.coll, strike, self.state, self.varient, sense) {
            Some((throw_clip, started)) => {
                self.throw_started = Some(started);
                self.fight_mode();
                throw_clip
            }
            None => clip,
        }
    }

    /// Being thrown: plays the victim's side of a throw — clip `clip` of the
    /// *thrower's* collection `coll`, since the two clips were animated
    /// together — cutting off whatever was playing. Takes the fight stance if
    /// the throw does.
    pub fn thrown(&mut self, ch: &Character, coll: Rc<AnimCollection>, clip: usize, fight: bool) {
        if fight {
            self.fight_mode();
        }
        self.last_attack = ty::NONE;
        self.attack_ended = Some(0);
        self.lock_frames = 0;
        self.leave_clip(false);
        let from = self.state;
        self.enter_in(ch, coll, from, clip, throw::VICTIM_BLEND, 0.0);
        self.queued = ty::STAND;
    }

    /// Damage the current clip deals its own player at this frame.
    pub fn self_damage(&self) -> i32 {
        self.current()
            .self_damage
            .iter()
            .filter(|&&(f, _)| f == self.frame)
            .map(|&(_, d)| i32::from(d))
            .sum()
    }

    /// Forces the airborne clip after walking off a ledge mid-stride.
    pub fn fall_into_flight(&mut self, ch: &Character, q: Quirks) {
        self.knock(ch, ty::FLY, q);
    }

    /// Forces reaction `kind` (a hit, knockdown or stagger — or the flight
    /// clip after a ledge) regardless of what is playing: enters the fight
    /// stance, drops the combo, adapts the reaction to the current situation,
    /// and falls back to more generic clips (or the standing pose) when the
    /// specific one is missing. Afterwards the character stands (or gets up).
    /// Returns whether a clip was found.
    pub fn knock(&mut self, ch: &Character, kind: u16, q: Quirks) -> bool {
        self.fight_mode();
        self.last_attack = ty::NONE;
        self.attack_ended = Some(0);
        self.lock_frames = 0;
        let victim = Victim {
            airborne: self.air.frames > 0,
            to_state: self.current().to_state,
            resistant: false,
        };
        let adapted = reaction::adapt(kind, victim, q);
        let general = reaction::generic(kind);
        let (state, v) = (self.state, self.varient);
        let mut found = [
            (adapted, state),
            (adapted, st::STANDING),
            (kind, state),
            (general, state),
            (kind, st::STANDING),
            (general, st::STANDING),
        ]
        .iter()
        .find_map(|&(t, s)| self.coll.lookup(t, s, v));
        if found.is_none() && st::is_flying(state) && state % 2 == 1 {
            // A flying state's falling counterpart is the next state up.
            found = self.coll.lookup(kind, state + 1, v);
        }
        let Some(clip) = found else {
            return false;
        };
        self.leave_clip(false);
        self.enter(ch, state, clip, BLEND_FRAMES, 0.0);
        self.queued = ty::STAND;
        true
    }

    /// Forces the block clip (a hit was just blocked).
    pub fn block(&mut self, ch: &Character) {
        self.fight_mode();
        self.lock_frames = 0;
        if self.kind != ty::BLOCK {
            self.request(ch, Urge::Force, false, ty::BLOCK);
        }
    }

    /// Whether frame `frame` of the current clip is immune to hits.
    pub fn invulnerable(&self) -> bool {
        let c = self.current();
        let (start, len) = (u16::from(c.invulnerable_start), u16::from(c.invulnerable_length));
        c.has_flag(flag::INVULNERABLE) || (len > 0 && self.frame >= start && self.frame < start + len)
    }

    /// Standing or crouching still, turning in place, blocking, or backing off:
    /// a character in one of these blocks attacks automatically.
    pub fn defensive(&self) -> bool {
        // The shipped game checks the clip's start state where it seemingly
        // meant to check its end state too; doing so would stop a character
        // who is already blocking from blocking again (block clips end in a
        // blocking state), so the start-state-only test is kept (Q3).
        let still = matches!(self.current().from_state, st::CROUCH | st::CROUCH_START | st::STANDING)
            && matches!(
                self.kind,
                ty::CROUCH_TURN_LEFT
                    | ty::CROUCH_TURN_RIGHT
                    | ty::STANDING_TURN_LEFT
                    | ty::STANDING_TURN_RIGHT
                    | ty::BLOCK
                    | ty::STAND
                    | ty::CROUCH
            );
        let backing = matches!(
            self.kind,
            ty::RUN_BACKWARDS_START
                | ty::WALK_BACKWARDS_START
                | ty::RUN_BACKWARDS
                | ty::WALK_BACKWARDS
                | ty::CROUCH_RUN_BACKWARDS
                | ty::CROUCH_WALK_BACKWARDS
        );
        still || backing
    }

    /// The block clip this character would use now, if it has one.
    pub fn block_clip(&self) -> Option<usize> {
        self.coll.lookup(ty::BLOCK, self.state, self.varient | var::FIGHT)
    }

    /// Snaps to the standing clip when nothing else fits.
    pub fn force_stand(&mut self, ch: &Character) {
        if let Some(clip) = self.coll.lookup(ty::STAND, st::STANDING, self.varient) {
            let turn = self.leave_clip(true);
            self.enter(ch, st::STANDING, clip, 12, turn);
        }
    }

    // --- fight stance / variants -------------------------------------------

    /// Plays clip `name` now, from the current state (a script's
    /// `chr_animate`): blending over `interp` frames (default: the usual),
    /// then holding off other clips for `frames` ticks (−1: the clip's
    /// length; 0: none). False if the collection has no such clip.
    /// `elsewhere`: a collection holding the clip when ours doesn't.
    pub fn animate(
        &mut self,
        ch: &Character,
        name: &str,
        frames: i64,
        interp: Option<u16>,
        elsewhere: Option<Rc<AnimCollection>>,
    ) -> bool {
        let find = |c: &AnimCollection| c.entries.iter().position(|e| e.name.eq_ignore_ascii_case(name));
        let (coll, clip) = match find(&self.coll) {
            Some(i) => (self.coll.clone(), i),
            None => match elsewhere.and_then(|c| find(&c).map(|i| (c, i))) {
                Some(x) => x,
                None => return false,
            },
        };
        let n = interp.unwrap_or_else(|| {
            if Rc::ptr_eq(&coll, &self.coll) {
                self.blend_into(clip)
            } else {
                crate::anim::collection::blend_between(self.current(), &coll.entries[clip].clip, false)
            }
        });
        let turn = self.leave_clip(true);
        let from = self.state;
        self.enter_in(ch, coll, from, clip, n, turn);
        self.lock_frames = match frames {
            -1 => self.length() + 1,
            f if f > 0 => (f as u16).saturating_add(1),
            _ => 0,
        };
        true
    }

    /// Enters (or refreshes) the fight stance (not while panicking).
    pub fn fight_mode(&mut self) {
        if self.varient & var::PANIC != 0 {
            return;
        }
        self.fight_frames = self.tuning.fight_mode_duration;
        self.varient |= var::FIGHT;
    }

    /// Whether it is crouched, or crouching down (where the current clip
    /// ends).
    pub fn crouching(&self) -> bool {
        matches!(
            oni_schema::name(&oni_schema::ANIM_STATES, self.current().to_state),
            Some(
                "crouch"
                    | "crouchWalk"
                    | "crouchStart"
                    | "crouchRunLeft"
                    | "crouchRunRight"
                    | "crouchRunBackLeft"
                    | "crouchRunBackRight"
                    | "crouchBlocking1"
            )
        )
    }

    /// Cowers (panic variants, no fight stance) or stops cowering.
    pub fn set_panic(&mut self, on: bool) {
        if on {
            self.varient = (self.varient | var::PANIC) & !var::FIGHT;
            self.fight_frames = 0;
        } else {
            self.varient &= !var::PANIC;
        }
    }

    /// If a better-matching variant of the current clip exists now (e.g. the
    /// fight stance just came on), blend over to it.
    pub fn upgrade_varient(&mut self, ch: &Character) {
        let c = self.current();
        if c.has_flag(flag::THROW_SOURCE) || c.has_flag(flag::THROW_TARGET) {
            return; // a throw's two clips stay as they were paired
        }
        if let Some(better) = self.coll.lookup(self.kind, self.entered_from, self.varient) {
            if self.clip_of(better).varient > self.current().varient {
                let from = self.entered_from;
                self.enter(ch, from, better, 8, 0.0);
            }
        }
    }

    // --- idle -------------------------------------------------------------

    fn reroll_idle_delay(&mut self) {
        let base = self.tuning.idle_delay;
        self.idle_delay = base + self.random_below(base / 2 + 1);
    }

    /// Whether the character has been inactive long enough to fidget.
    pub fn idle_due(&self) -> bool {
        self.idle_enabled && self.now >= self.last_active + u64::from(self.idle_delay)
    }

    pub fn after_idle_request(&mut self) {
        self.reroll_idle_delay();
    }

    // --- per tick ----------------------------------------------------------

    /// Forgets the last attack once its combo window has passed.
    pub fn expire_combo(&mut self) {
        if let Some(end) = self.attack_ended {
            if self.now.saturating_sub(end) > COMBO_WINDOW {
                self.last_attack = ty::NONE;
            }
        }
    }

    /// Start-of-frame countdowns (lock, pauses, stuns).
    pub fn count_down(&mut self, ch: &Character) {
        if self.lock_frames > 0 {
            self.lock_frames -= 1;
            if self.lock_frames == 0 {
                self.request(ch, Urge::Appropriate, false, ty::STAND);
            }
        }
        for c in [
            &mut self.soft_pause,
            &mut self.hard_pause,
            &mut self.block_stun,
            &mut self.hit_stun,
            &mut self.dizzy_stun,
            &mut self.stagger_stun,
        ] {
            *c = c.saturating_sub(1);
        }
    }

    /// End-of-frame: advance one tick, finishing the clip when it runs out.
    pub fn advance(&mut self, ch: &Character) {
        if self.corpse {
            return; // a lying body is posed, not animated
        }
        self.frame += 1;
        if let Some(b) = &mut self.blend {
            b.itr += 1;
            if b.itr >= b.count {
                self.blend = None;
                self.blend_carry = 0;
            }
        }
        self.fight_frames = self.fight_frames.saturating_sub(1);
        if self.frame >= self.length() {
            self.next_clip(ch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::collection::tests::konoko;

    fn rig() -> Option<(Motion, Character)> {
        let coll = konoko()?;
        let model = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/common/characters/konoko_body_high/model.glb");
        let ch = skel::load_character(&model).ok()?;
        Some((Motion::new(coll, ClassTuning::default()), ch))
    }

    #[test]
    fn starts_standing_and_interrupts_into_a_run() {
        let Some((mut m, ch)) = rig() else {
            eprintln!("SKIP: generated assets not found");
            return;
        };
        assert_eq!(m.state, st::STANDING);
        let r = m.request(&ch, Urge::Appropriate, false, ty::RUN_START);
        assert!(r.is_some(), "run start resolves from standing");
        assert_eq!(m.kind, ty::RUN_START, "a standing clip is not atomic: forced in");
        assert!(m.blend_pose().is_some(), "the switch blends");
    }

    #[test]
    fn same_loop_is_queued_not_restarted() {
        let Some((mut m, ch)) = rig() else {
            eprintln!("SKIP: generated assets not found");
            return;
        };
        let clip = m.clip;
        m.request(&ch, Urge::Appropriate, false, ty::STAND);
        assert_eq!(m.clip, clip, "requesting the playing loop keeps it");
        assert_eq!(m.queued, ty::STAND, "…and queues it for the clip end");
    }

    #[test]
    fn clip_end_plays_the_queued_type() {
        let Some((mut m, ch)) = rig() else {
            eprintln!("SKIP: generated assets not found");
            return;
        };
        m.request(&ch, Urge::Appropriate, false, ty::RUN_START);
        m.request(&ch, Urge::Appropriate, false, ty::RUN); // start → loop: queued
        assert_eq!(m.queued, ty::RUN);
        for _ in 0..m.length() {
            m.advance(&ch);
        }
        assert_eq!(m.kind, ty::RUN, "the run loop follows the run start");
    }
}
