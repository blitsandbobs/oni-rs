//! Neutral characters' interactions with the player (`docs/NEUTRAL.md`;
//! the original's `Oni_AI2_Neutral.c`, `AI2rTryNeutralInteraction`,
//! `ONiRemapKeys`): a civilian or ally with a neutral behaviour notices the
//! player nearby, runs up hailing, and — once the player presses Action by
//! it — says its lines, the player answering, with the animations each
//! line asks for; on the line that gives, it walks up and hands over its
//! items. Moving away, attacking or enemies close by break it off.
//!
//! The agent's goal machine holds only that it is in its interaction
//! ([`Goal::Neutral`]); its progress lives here, by character.

use std::rc::Rc;

use glam::Vec3;

use super::voices::{speech, Line as Speech};
use super::World;
use crate::ai::goals::Goal;
use crate::ai::Brain;
use crate::anim::input::key;
use crate::anim::motion::Urge;
use crate::anim::types::{st, var};
use crate::neutrals::Behavior;

/// How often an automatic trigger is looked at (`AI2cNeutral_CheckFrames`)
/// and the way to the player re-planned (`_RunToFrames`).
const CHECK_FRAMES: u64 = 30;
const RUN_TO_FRAMES: u64 = 20;
/// It gives from this close (`_GiveItemDist`), the player's receiving
/// animation starting this long after its own (`_ReceiveDelay`).
const GIVE_ITEM_DIST: f32 = 20.0;
const RECEIVE_DELAY: u32 = 15;
/// Enemies nearby put it off this long (`_EnemyScareFrames`); while it
/// runs, it looks for them this often (`_EnemyCheckFrames`).
const ENEMY_SCARE_FRAMES: u64 = 300;
const ENEMY_CHECK_FRAMES: u64 = 10;
/// It runs up to this close (`_ClosestRange`); Action starts it from this
/// close, the player facing it within this angle (`_ManualTrigger*`).
const CLOSEST_RANGE: f32 = 25.0;
const MANUAL_TRIGGER_RANGE: f32 = 35.0;
const MANUAL_TRIGGER_ANGLE: f32 = 60.0 * std::f32::consts::PI / 180.0;
/// Enemies count this close without a behaviour (`_DefaultEnemyRange`).
pub const DEFAULT_ENEMY_RANGE: f32 = 100.0;
/// Broken off, it won't start again by itself for this long
/// (`_DisableTimer`); it gives up waiting after `_GiveUpTimer`; it hails
/// first after 300–390 ticks, then every 360–450.
const DISABLE_TIMER: u32 = 180;
const GIVE_UP_TIMER: u32 = 600;
const HAIL_INITIAL: u32 = 300;
const HAIL_REPEAT: u32 = 360;
const HAIL_RANDOM: u32 = 90;
/// Held this long, keys that move end it (`AI2cNeutral_KeyAbortFrames`).
const KEY_ABORT_FRAMES: u32 = 15;

/// Keys that end it at once, that end it if held, and that do nothing
/// meanwhile (`LIc_BitMask_Neutral*`).
const ABORT_KEYS: u32 = key::FIRE1 | key::FIRE2 | key::FIRE3 | key::PUNCH | key::KICK;
const STOP_KEYS: u32 =
    key::FORWARD | key::BACKWARD | key::STEP_LEFT | key::STEP_RIGHT | key::CROUCH | key::JUMP;
const LOCKOUT_KEYS: u32 =
    ABORT_KEYS | STOP_KEYS | key::ACTION | key::HYPO | key::RELOAD | key::SWAP | key::DROP;

/// A character's neutral behaviour and how far it got (`ai2State.neutral*`,
/// kept through its interactions).
#[derive(Clone, Debug)]
pub struct Settings {
    pub behavior: Option<u16>,
    pub resume_at: usize,
    pub triggered: bool,
    pub begun: bool,
    pub items_given: bool,
    pub completed: bool,
    pub disable_resume: bool,
    pub last_check: u64,
    pub trigger_range: f32,
    pub enemy_range: f32,
    pub disable_timer: u32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            behavior: None,
            resume_at: 0,
            triggered: false,
            begun: false,
            items_given: false,
            completed: false,
            disable_resume: false,
            last_check: 0,
            trigger_range: 0.0,
            enemy_range: DEFAULT_ENEMY_RANGE,
            disable_timer: 0,
        }
    }
}

/// An interaction under way (`AI2tNeutralState`).
#[derive(Clone, Debug)]
pub struct Run {
    pub behavior: Rc<Behavior>,
    pub target: usize,
    /// The player pressed Action by it.
    pub accepted: bool,
    /// The line it is on (None: not yet talking).
    pub line: Option<usize>,
    played_line: bool,
    line_anim_started: bool,
    other_anim_started: bool,
    hail_timer: u32,
    give_up_timer: u32,
    last_run_to: u64,
    last_enemy_check: u64,
    triggered_at: Vec3,
    played_abort: bool,
    initiated_give: bool,
    completed_give: bool,
    give_anim_started: bool,
    receive_anim_started: bool,
    receive_delay: u32,
}

impl World {
    /// A character's name for logs and scripts: its placement's, else
    /// "fighter N".
    fn fighter_name(&self, i: usize) -> String {
        match self.fighters.get(i).and_then(|f| f.brain.as_ref()) {
            Some(Brain::Agent(a)) => a.name.clone(),
            _ => format!("fighter {i}"),
        }
    }

    fn anim_type(name: &str) -> u16 {
        oni_schema::value(&oni_schema::ANIM_TYPES, name).unwrap_or(0)
    }

    fn is_agent(&self, i: usize) -> bool {
        matches!(
            self.fighters.get(i).and_then(|f| f.brain.as_ref()),
            Some(Brain::Agent(_))
        )
    }

    fn in_neutral(&self, i: usize) -> bool {
        matches!(self.fighters.get(i).and_then(|f| f.brain.as_ref()), Some(Brain::Agent(a)) if matches!(a.goal, Goal::Neutral))
    }

    /// Gives fighter `who` neutral behaviour `id` (None: none;
    /// `AI2rNeutral_ChangeBehavior`, `ai2_neutralbehavior`): its progress
    /// starts over. False if there is no such behaviour.
    pub fn neutral_behavior(&mut self, who: usize, id: Option<u16>, initialize: bool) -> bool {
        if !self.is_agent(who) {
            return false;
        }
        let s = &self.fighters[who].neutral;
        if !initialize && s.behavior == id {
            return id.is_some();
        }
        let found = id.and_then(|id| self.neutrals.get(&id).cloned());
        let s = &mut self.fighters[who].neutral;
        *s = Settings {
            disable_timer: s.disable_timer,
            ..Settings::default()
        };
        match found {
            Some(b) => {
                s.behavior = Some(b.id);
                s.trigger_range = b.trigger_range;
                s.enemy_range = b.abort_enemy_range;
                true
            }
            None => false,
        }
    }

    /// Whether fighter `who` is someone `target` might fight
    /// (`AI2rCharacter_PotentiallyHostile`, previous knowledge checked).
    fn potentially_hostile(&self, who: usize, target: usize) -> bool {
        let (Some(f), Some(t)) = (self.fighters.get(who), self.fighters.get(target)) else {
            return false;
        };
        if who == target || f.passive {
            return false;
        }
        match &f.perception {
            Some(p) => p.potentially_hostile(target, t.team()),
            None => f.team().regards(t.team()) == crate::ai::perception::Relation::Hostile,
        }
    }

    /// Whether fighter `i` knows of a danger to it or to `with`
    /// (`AI2rCharacter_InDanger`, `_IsDangerousContact`): a hurt, or a
    /// fight's sound or a sighting of someone hostile to either, within its
    /// enemy range.
    fn neutral_in_danger(&self, i: usize, with: Option<usize>) -> bool {
        use crate::ai::perception::{Stimulus, Strength};
        let f = &self.fighters[i];
        let Some(p) = f.perception.as_ref() else {
            return false;
        };
        let (feet, range) = (f.feet(), f.neutral.enemy_range);
        p.contacts.iter().any(|c| {
            if c.strength <= Strength::Forgotten {
                return false;
            }
            if matches!(c.stimulus, Stimulus::HitWeapon | Stimulus::HitMelee) {
                return true;
            }
            if !matches!(
                c.stimulus,
                Stimulus::SoundMelee | Stimulus::SoundGunshot | Stimulus::SightDefinite
            ) {
                return false;
            }
            let Some(who) = c.who.filter(|&w| w != i) else {
                return false;
            };
            let hostile = self.potentially_hostile(who, i)
                || c.stimulus.combat_sound()
                || with.is_some_and(|t| self.potentially_hostile(who, t));
            hostile && c.location.distance_squared(feet) <= range * range
        })
    }

    /// Whether an AI fighting fighter `i` or `target` is within `i`'s enemy
    /// range of either (`AI2iNeutral_CheckEnemies`).
    fn neutral_enemies(&self, i: usize, target: usize) -> bool {
        let range = self.fighters[i].neutral.enemy_range;
        let (me, them) = (self.fighters[i].feet(), self.fighters[target].feet());
        self.fighters.iter().filter(|f| f.alive()).any(|f| {
            let Some(Brain::Agent(a)) = &f.brain else {
                return false;
            };
            let Some(t) = a.opponent().filter(|&t| t == i || t == target) else {
                return false;
            };
            let _ = t;
            let at = f.feet();
            at.distance_squared(me) <= range * range || at.distance_squared(them) <= range * range
        })
    }

    /// Whether something stands between fighter `i` and `point`
    /// (`AI2rManeuver_CheckBlockage`): the level as characters collide with
    /// it, or an animating object.
    fn neutral_blocked(&self, i: usize, point: Vec3) -> bool {
        let from = self.fighters[i].sight_points()[0];
        !self
            .quads
            .clear(from, point, crate::quads::flag::CHR_COL_SKIP)
            || self.segment_meets_object(from, point)
    }

    /// Whether fighter `i` would start its interaction with `target`
    /// (`AI2rNeutral_CheckTrigger`): by itself (`automatic`: the target
    /// within its trigger range, seen for sure, no enemies about; looked at
    /// every 30 ticks) or because the target pressed Action by it (within
    /// 35, in front of the target).
    fn neutral_trigger(&mut self, i: usize, target: usize, automatic: bool) -> bool {
        if !self.neutral_could_trigger(i, target, automatic) {
            return false;
        }
        if automatic {
            let now = self.now as u64;
            let s = &mut self.fighters[i].neutral;
            if now < s.last_check + CHECK_FRAMES {
                return false;
            }
            s.last_check = now;
            let seen = self.fighters[i]
                .perception
                .as_ref()
                .and_then(|p| p.about(target))
                .is_some_and(|c| c.strength == crate::ai::perception::Strength::Definite);
            if !seen {
                return false;
            }
            if self.fighters[i].neutral.enemy_range > 0.0
                && (self.neutral_in_danger(i, Some(target)) || self.neutral_enemies(i, target))
            {
                self.fighters[i].neutral.last_check = now + ENEMY_SCARE_FRAMES;
                return false;
            }
        }
        let at = self.fighters[target].sight_points()[0];
        !self.neutral_blocked(i, at)
    }

    /// The cheap part of the trigger test (no timers, no rays).
    fn neutral_could_trigger(&self, i: usize, target: usize, automatic: bool) -> bool {
        let (Some(f), Some(t)) = (self.fighters.get(i), self.fighters.get(target)) else {
            return false;
        };
        let s = &f.neutral;
        if !self.is_agent(i) || s.behavior.is_none() || f.neutral_with.is_some() || !f.alive() {
            return false;
        }
        if matches!(&f.brain, Some(Brain::Agent(a)) if a.panicking()) || s.disable_resume {
            return false;
        }
        if automatic && s.disable_timer > 0 {
            return false;
        }
        let delta = f.feet() - t.feet();
        if automatic {
            if delta.length_squared() > s.trigger_range * s.trigger_range {
                return false;
            }
        } else if delta.length_squared() > MANUAL_TRIGGER_RANGE * MANUAL_TRIGGER_RANGE
            || !in_front(t.facing.facing, delta)
        {
            return false;
        }
        !self.potentially_hostile(i, target)
    }

    /// Whether `target`'s Action would be taken by fighter `i`, already
    /// running up to it (`AI2rNeutral_CheckActionAcceptance`).
    fn neutral_accepts(&self, i: usize, target: usize) -> bool {
        if !self.in_neutral(i) || self.neutral_runs.get(&i).is_none_or(|r| r.target != target) {
            return false;
        }
        let delta = self.fighters[i].feet() - self.fighters[target].feet();
        delta.length_squared() <= MANUAL_TRIGGER_RANGE * MANUAL_TRIGGER_RANGE
            && in_front(self.fighters[target].facing.facing, delta)
    }

    /// The neutral character fighter `who`'s Action would talk to, and
    /// whether it is already running up (`AI2rTryNeutralInteraction`'s
    /// search; no timers or rays for one not yet triggered beyond the
    /// cheap tests).
    fn neutral_candidate(&self, who: usize) -> Option<(usize, bool)> {
        let mut found = None;
        for i in 0..self.fighters.len() {
            if !self.is_agent(i) || !self.fighters[i].alive() {
                continue;
            }
            if self.in_neutral(i) {
                if self.neutral_accepts(i, who) {
                    return Some((i, true));
                }
            } else if found.is_none() && self.neutral_could_trigger(i, who, false) {
                let at = self.fighters[who].sight_points()[0];
                if !self.neutral_blocked(i, at) {
                    found = Some((i, false));
                }
            }
        }
        found
    }

    /// Whether the player could talk to someone now (the "nci" prompt).
    pub(super) fn neutral_prompt(&self) -> bool {
        self.fighters
            .first()
            .is_some_and(|p| p.neutral_with.is_some())
            || self.neutral_candidate(0).is_some()
    }

    /// Fighter `who` pressed Action: a neutral character by it starts (or
    /// goes on to) its conversation. True if one did.
    pub(super) fn neutral_action(&mut self, who: usize) -> bool {
        if self.fighters[who].neutral_with.is_some() {
            return true;
        }
        let Some((i, already)) = self.neutral_candidate(who) else {
            return false;
        };
        if !already {
            self.neutral_start(i, who);
        }
        if let Some(r) = self.neutral_runs.get_mut(&i) {
            r.accepted = true;
        }
        eprintln!("neutral: {} talks to fighter {who}", self.fighter_name(i));
        true
    }

    /// Fighter `i` enters its neutral interaction with the player
    /// (`AI2rNeutral_Enter`): from where it stands, its trigger line said.
    fn neutral_start(&mut self, i: usize, target: usize) {
        let Some(b) = self.fighters[i]
            .neutral
            .behavior
            .and_then(|id| self.neutrals.get(&id).cloned())
        else {
            self.fighters[i].neutral.behavior = None;
            return;
        };
        self.with_agent(i, |a, cx| a.neutral_enter(cx));
        self.fighters[i].neutral.triggered = true;
        let hail = HAIL_INITIAL + (self.random() * HAIL_RANDOM as f32) as u32;
        let run = Run {
            behavior: b,
            target,
            accepted: false,
            line: None,
            played_line: false,
            line_anim_started: false,
            other_anim_started: false,
            hail_timer: hail,
            give_up_timer: GIVE_UP_TIMER,
            last_run_to: 0,
            last_enemy_check: 0,
            triggered_at: self.fighters[i].feet(),
            played_abort: false,
            initiated_give: false,
            completed_give: false,
            give_anim_started: false,
            receive_anim_started: false,
            receive_delay: 0,
        };
        let sound = run.behavior.trigger_sound.clone();
        self.neutral_runs.insert(i, run);
        self.neutral_say(i, &sound, speech::CALL);
    }

    fn neutral_say(&mut self, who: usize, sound: &str, kind: u8) {
        if !sound.is_empty() {
            self.say(
                who,
                Speech {
                    sound: sound.to_string(),
                    kind,
                    ..Speech::default()
                },
                false,
            );
        }
    }

    /// Stops the interaction fighter `who` is in (either one of the two;
    /// `AI2rNeutral_Stop`): not in the middle of a give unless `force`;
    /// broken off (`abort`: the character says its call to come back, or
    /// `hostile`: its enemy line) or done (its end script runs); `reset`
    /// sends the neutral character back to its job. True if stopped.
    pub fn neutral_stop(
        &mut self,
        who: usize,
        force: bool,
        abort: bool,
        hostile: bool,
        reset: bool,
    ) -> bool {
        let Some(other) = self.fighters.get(who).and_then(|f| f.neutral_with) else {
            // (Not yet talking: nothing to stop beyond the run itself.)
            if let Some(r) = self.neutral_runs.get(&who) {
                if !force && r.initiated_give && !r.completed_give {
                    return false;
                }
            }
            return true;
        };
        let dead = !self.fighters[who].alive() || !self.fighters[other].alive();
        let ai = if dead {
            None
        } else if self.neutral_runs.contains_key(&who) {
            Some(who)
        } else if self.neutral_runs.contains_key(&other) {
            Some(other)
        } else {
            None
        };
        if let Some(r) = ai.and_then(|a| self.neutral_runs.get(&a)) {
            if r.initiated_give && !r.completed_give && !force {
                return false;
            }
        }
        if let Some(a) = ai.filter(|_| abort) {
            self.fighters[a].neutral.disable_timer = DISABLE_TIMER;
        }
        if !abort {
            if let Some(script) = ai
                .and_then(|a| self.neutral_runs.get(&a))
                .map(|r| r.behavior.end_script.clone())
                .filter(|s| !s.is_empty())
            {
                let name = self.fighter_name(who);
                self.script_events.push((script, vec![name]));
            }
        }
        for c in [who, other] {
            if self.fighters[c].alive() {
                self.neutral_reset(c, abort, hostile, reset);
            }
        }
        for c in [who, other] {
            self.fighters[c].neutral_with = None;
            self.fighters[c].neutral_uninterruptable = false;
        }
        eprintln!(
            "neutral: {} and {} part{}",
            self.fighter_name(who),
            self.fighter_name(other),
            if abort { " (broken off)" } else { "" }
        );
        true
    }

    /// One side of a stopped interaction (`AI2iNeutral_ResetCharacterState`):
    /// its stances back; the neutral character, broken off, says its line
    /// once and stands (back to its job with `reset`).
    fn neutral_reset(&mut self, c: usize, abort: bool, hostile: bool, reset: bool) {
        self.set_weapon_varient(c);
        if !(abort && self.in_neutral(c)) {
            return;
        }
        let Some(r) = self.neutral_runs.get_mut(&c) else {
            return;
        };
        let (sound, kind) = if hostile {
            (r.behavior.enemy_sound.clone(), speech::YELL)
        } else if self.fighters[c].neutral.disable_resume {
            (String::new(), speech::NONE)
        } else {
            (r.behavior.abort_sound.clone(), speech::CALL)
        };
        if !r.played_abort {
            r.played_abort = true;
            self.neutral_say(c, &sound, kind);
        }
        let feet = self.fighters[c].feet();
        if let Some(Brain::Agent(a)) = self.fighters[c].brain.as_mut() {
            a.stop_looking();
            a.neutral_halt(feet, None);
        }
        if reset {
            self.with_agent(c, |a, cx| a.return_to_job(true, true, cx));
        }
    }

    /// Plays a conversation animation (`AI2iNeutral_ConversationAnimation`):
    /// once (blocking while it plays or waits to; `repeat`: again until
    /// told otherwise) or held for the line (not blocking). Returns whether
    /// it blocks.
    fn conversation_anim(
        &mut self,
        who: usize,
        kind: u16,
        once: bool,
        repeat: bool,
        started: &mut bool,
    ) -> bool {
        let f = &mut self.fighters[who];
        let ch = f.character.clone();
        if !once {
            if f.motion.kind == kind {
                f.motion.lock_frames = 2;
            } else {
                f.motion.request(&ch, Urge::Appropriate, false, kind);
            }
            return false;
        }
        if f.motion.kind == kind {
            *started = true;
            return true;
        }
        if *started && !repeat {
            return false;
        }
        if f.motion.queued == kind {
            return true;
        }
        if f.motion
            .request(&ch, Urge::Appropriate, false, kind)
            .is_none()
            && f.motion.state == st::STANDING
        {
            eprintln!(
                "neutral: fighter {who} can't play {} standing",
                oni_schema::name(&oni_schema::ANIM_TYPES, kind).unwrap_or("?")
            );
            *started = true;
            return false;
        }
        true
    }

    /// The items a neutral character gives (`AI2iNeutral_GiveItems`): ammo,
    /// cells and hypos (what the player can't carry dropped), a shield,
    /// invisibility, the LSI, and a gun — in the hand, or to the floor if
    /// the player already carries one.
    fn neutral_give(&mut self, giver: usize, to: usize, b: &Behavior) {
        for (kind, n) in [
            ("ammo", b.give_ballistic),
            ("cell", b.give_energy),
            ("hypo", b.give_hypos),
        ] {
            if n == 0 {
                continue;
            }
            let given = self.give_powerup(to, kind, Some(n), false);
            if n > given {
                self.drop_powerup_named(giver, kind, n - given);
            }
        }
        if b.give_shield {
            self.give_powerup(to, "shield", None, false);
        }
        if b.give_invisibility {
            self.give_powerup(to, "invis", None, false);
        }
        if b.give_lsi {
            self.give_powerup(to, "lsi", None, false);
            self.fighters[giver].inventory.lsi = false;
        }
        if !b.give_weapon.is_empty() && b.give_weapon != "none" {
            let carrying = self.fighters[to].hand.is_some() || self.fighters[to].holster.is_some();
            let at = self.fighters[giver].feet();
            if let Some(w) = self.spawn_weapon(&b.give_weapon, at, 0.0, false) {
                if carrying {
                    self.drop_weapon_free(w);
                } else {
                    self.take_weapon(to, w);
                }
            }
        }
        eprintln!(
            "neutral: {} gives its items ({})",
            self.fighter_name(giver),
            b.name
        );
    }

    /// Each tick: neutral characters notice the player and start (the
    /// automatic trigger, `AI2rCharacter_Update`), their disable timers run
    /// down, those under way take a step, and those whose goals left the
    /// interaction are stopped (`AI2rNeutral_Exit`).
    pub(super) fn neutral_tick(&mut self) {
        let player = 0;
        if self.fighters.is_empty() {
            return;
        }
        // (A death ends what either was in.)
        for c in 0..self.fighters.len() {
            if let Some(o) = self.fighters[c].neutral_with {
                if !self.fighters[c].alive() || !self.fighters.get(o).is_some_and(|f| f.alive()) {
                    self.neutral_stop(c, true, true, false, false);
                }
            }
        }
        self.neutral_runs
            .retain(|&i, _| self.fighters.get(i).is_some_and(|f| f.alive()));
        for i in 1..self.fighters.len() {
            let f = &self.fighters[i];
            if !self.is_agent(i) || !f.alive() || f.motion.dying || f.passive {
                continue;
            }
            self.neutral_exited(i);
            if !self.in_neutral(i) {
                self.neutral_runs.remove(&i);
                if self.neutral_trigger(i, player, true) {
                    self.neutral_start(i, player);
                } else if self.fighters[i].neutral.disable_timer > 0 {
                    self.fighters[i].neutral.disable_timer -= 1;
                }
                continue;
            }
            if self.neutral_runs.contains_key(&i) {
                self.neutral_update(i);
                self.neutral_exited(i);
            }
        }
    }

    /// Fighter `i`'s goals left its interaction (`AI2rNeutral_Exit`): it is
    /// stopped — broken off unless every line was said.
    fn neutral_exited(&mut self, i: usize) {
        let exit = match self.fighters[i].brain.as_mut() {
            Some(Brain::Agent(a)) if !matches!(a.goal, Goal::Neutral) => a.neutral_exit.take(),
            _ => None,
        };
        if let Some(hostile) = exit {
            let aborting = self
                .neutral_runs
                .get(&i)
                .is_some_and(|r| r.line.is_none_or(|l| l < r.behavior.lines.len()));
            self.neutral_stop(i, true, aborting, hostile, false);
            self.neutral_runs.remove(&i);
        }
    }

    /// The player's keys during an interaction (`ONiRemapKeys`): an attack
    /// ends it, moving held a while ends it, and what would act is held
    /// back meanwhile. Returns the keys left.
    pub(super) fn neutral_keys(&mut self, controls: u32) -> u32 {
        let Some(p) = self.fighters.first() else {
            return controls;
        };
        if p.neutral_with.is_none() {
            return controls;
        }
        if controls & ABORT_KEYS != 0 {
            self.neutral_stop(0, false, true, false, true);
        } else if controls & STOP_KEYS != 0 {
            let p = &mut self.fighters[0];
            if !p.neutral_uninterruptable {
                p.neutral_keyabort += 2;
            }
            if p.neutral_keyabort >= KEY_ABORT_FRAMES {
                self.neutral_stop(0, false, true, false, true);
            }
        } else {
            let p = &mut self.fighters[0];
            p.neutral_keyabort = p.neutral_keyabort.saturating_sub(2);
        }
        if self.fighters[0].neutral_with.is_some() {
            controls & !LOCKOUT_KEYS
        } else {
            controls
        }
    }

    /// One tick of fighter `i`'s interaction (`AI2rNeutral_Update`).
    fn neutral_update(&mut self, i: usize) {
        let Some(mut r) = self.neutral_runs.remove(&i) else {
            return;
        };
        let t = r.target;
        let now = self.now as u64;
        if !self.fighters[t].alive() || self.fighters[i].neutral.behavior.is_none() {
            self.with_agent(i, |a, cx| a.return_to_job(true, true, cx));
            return;
        }
        if r.initiated_give && !r.completed_give {
            // (Giving: enemies are ignored.)
        } else if now > r.last_enemy_check + ENEMY_CHECK_FRAMES {
            if self.neutral_enemies(i, t) {
                self.neutral_runs.insert(i, r);
                if self.neutral_stop(i, false, true, true, true) {
                    return;
                }
                r = self.neutral_runs.remove(&i).expect("still running");
            }
            r.last_enemy_check = now;
        }
        if r.line.is_none() {
            let keep = self.neutral_approach(i, &mut r);
            if keep {
                self.neutral_runs.insert(i, r);
            }
            return;
        }
        let n = r.line.unwrap_or(0);
        if n >= r.behavior.lines.len() {
            // Done.
            let s = &mut self.fighters[i].neutral;
            s.completed = true;
            s.disable_resume = true;
            self.neutral_runs.insert(i, r);
            self.with_agent(i, |a, cx| a.return_to_job(true, true, cx));
            return;
        }
        self.neutral_line(i, &mut r, n);
        self.neutral_runs.insert(i, r);
    }

    /// Before the conversation: running up to the player, hailing, giving
    /// up; on the player's Action, close enough and standing, it begins.
    /// False if it went back to its job.
    fn neutral_approach(&mut self, i: usize, r: &mut Run) -> bool {
        let t = r.target;
        let now = self.now as u64;
        let our = self.fighters[i].feet();
        let target = self.fighters[t].sight_points()[0];
        let d2 = our.distance_squared(target);
        let close = d2 < r.behavior.conversation_range * r.behavior.conversation_range
            && !self.neutral_blocked(i, target);
        let facing = yaw(our, target);
        if close && r.accepted {
            // Turn to the player, stand, look at it.
            if let Some(Brain::Agent(a)) = self.fighters[i].brain.as_mut() {
                a.neutral_halt(our, Some(facing));
                a.look_at(target);
            }
            if self.fighters[t].neutral_with.is_some()
                || self.fighters[i].motion.state != st::STANDING
            {
                return true; // wait for the last one to finish, or to stand
            }
            let b = r.behavior.clone();
            let s = &mut self.fighters[i].neutral;
            s.begun = true;
            if b.no_resume {
                s.disable_resume = true;
            }
            let resume = s.resume_at;
            self.fighters[i].neutral_with = Some(t);
            self.fighters[t].neutral_with = Some(i);
            if b.uninterruptable {
                self.fighters[i].neutral_uninterruptable = true;
                self.fighters[t].neutral_uninterruptable = true;
            }
            // The player turns to face it.
            self.fighters[t].facing.desired_facing = wrap(facing + std::f32::consts::PI);
            self.fighters[t].neutral_keyabort = 0;
            // (No fighting or gun stances while they talk.)
            for c in [i, t] {
                self.fighters[c].motion.varient &= !(var::FIGHT
                    | var::RIGHTY_PISTOL
                    | var::RIGHTY_RIFLE
                    | var::LEFTY_PISTOL
                    | var::LEFTY_RIFLE);
            }
            r.played_line = false;
            r.line = Some(resume);
            eprintln!(
                "neutral: {} begins its conversation (line {resume})",
                self.fighter_name(i)
            );
            return true;
        }
        // Not too far from where it set off.
        if our.distance_squared(r.triggered_at) > r.behavior.follow_range * r.behavior.follow_range
        {
            self.fighters[i].neutral.disable_timer = DISABLE_TIMER;
            self.with_agent(i, |a, cx| a.return_to_job(true, true, cx));
            return false;
        }
        if close {
            if let Some(Brain::Agent(a)) = self.fighters[i].brain.as_mut() {
                a.look_at(target);
                a.neutral_halt(our, None);
            }
            if r.give_up_timer > 0 {
                r.give_up_timer -= 1;
            } else {
                self.fighters[i].neutral.disable_timer = DISABLE_TIMER;
            }
            if r.hail_timer > 0 {
                r.hail_timer -= 1;
            } else {
                r.hail_timer = HAIL_REPEAT + (self.random() * HAIL_RANDOM as f32) as u32;
                let sound = r.behavior.trigger_sound.clone();
                self.neutral_say(i, &sound, speech::CALL);
                let f = &mut self.fighters[i];
                let ch = f.character.clone();
                f.motion
                    .request(&ch, Urge::Appropriate, false, Self::anim_type("hail"));
            }
        } else if d2 > (1.5 * r.behavior.trigger_range).powi(2) {
            self.with_agent(i, |a, cx| a.return_to_job(true, true, cx));
            return false;
        } else if now > r.last_run_to + RUN_TO_FRAMES {
            let alert = self.fighters[i]
                .perception
                .as_ref()
                .map_or(crate::ai::perception::Alert::Lull, |p| p.alert);
            if let Some(Brain::Agent(a)) = self.fighters[i].brain.as_mut() {
                a.script_move(target, CLOSEST_RANGE, None, None, alert);
            }
            r.last_run_to = now;
        }
        // (A way it can't find ends it quietly, `AI2rNeutral_PathfindingHandler`.)
        if matches!(&self.fighters[i].brain, Some(Brain::Agent(a)) if a.move_failed()) {
            self.neutral_stop(i, true, false, false, true);
            self.with_agent(i, |a, cx| a.return_to_job(true, true, cx));
            return false;
        }
        true
    }

    /// The conversation's line `n`: said by its speaker, with the
    /// animations it asks for (or the give: walk up, both give animations,
    /// the items at the giver's action frame); on to the next once nothing
    /// blocks.
    fn neutral_line(&mut self, i: usize, r: &mut Run, n: usize) {
        let t = r.target;
        let line = r.behavior.lines[n].clone();
        let (speaker, other) = if line.player { (t, i) } else { (i, t) };
        if !r.played_line {
            self.neutral_say(speaker, &line.sound, speech::SAY);
            if line.give {
                let alert = self.fighters[i]
                    .perception
                    .as_ref()
                    .map_or(crate::ai::perception::Alert::Lull, |p| p.alert);
                let to = self.fighters[t].feet();
                if let Some(Brain::Agent(a)) = self.fighters[i].brain.as_mut() {
                    a.neutral_walk();
                    a.script_move(to, GIVE_ITEM_DIST, None, None, alert);
                }
            } else {
                r.line_anim_started = false;
                r.other_anim_started = false;
            }
            r.played_line = true;
        }
        let mut block = false;
        if line.give {
            if !r.initiated_give {
                if self.fighters[i]
                    .feet()
                    .distance_squared(self.fighters[t].feet())
                    < GIVE_ITEM_DIST * GIVE_ITEM_DIST
                {
                    r.initiated_give = true;
                    r.give_anim_started = false;
                    r.receive_anim_started = false;
                    r.receive_delay = RECEIVE_DELAY;
                } else {
                    block = true;
                }
            }
            if r.initiated_give {
                let give = Self::anim_type("actGive");
                let mut started = r.give_anim_started;
                block |= self.conversation_anim(i, give, true, !r.completed_give, &mut started);
                r.give_anim_started = started;
                if r.give_anim_started {
                    r.receive_delay = r.receive_delay.saturating_sub(1);
                    if r.receive_delay > 0 {
                        block = true;
                    } else {
                        let mut started = r.receive_anim_started;
                        block |=
                            self.conversation_anim(t, give, true, !r.completed_give, &mut started);
                        r.receive_anim_started = started;
                        let m = &self.fighters[i].motion;
                        let fire = m.current().action_frame.unwrap_or(0);
                        if !r.completed_give && m.kind == give && m.frame >= fire {
                            let s = &mut self.fighters[i].neutral;
                            s.items_given = true;
                            if r.behavior.no_resume_after_items {
                                s.disable_resume = true;
                            }
                            let b = r.behavior.clone();
                            self.neutral_give(i, t, &b);
                            self.fighters[i].neutral.resume_at = n + 1;
                            r.initiated_give = false;
                            r.completed_give = true;
                        }
                    }
                }
            }
        } else {
            if line.anim != 0 {
                let mut started = r.line_anim_started;
                block |=
                    self.conversation_anim(speaker, line.anim, line.anim_once, false, &mut started);
                r.line_anim_started = started;
            }
            if line.other_anim != 0 {
                let mut started = r.other_anim_started;
                block |= self.conversation_anim(
                    other,
                    line.other_anim,
                    line.other_anim_once,
                    false,
                    &mut started,
                );
                r.other_anim_started = started;
            }
        }
        if !line.sound.is_empty() {
            block |= self.fighters[speaker].speech.current.is_some();
        }
        // Keep looking at each other.
        let at = self.fighters[t].sight_points()[0];
        if let Some(Brain::Agent(a)) = self.fighters[i].brain.as_mut() {
            a.look_at(at);
        }
        if !block {
            r.line = Some(n + 1);
            r.played_line = false;
            self.fighters[i].neutral.resume_at = n + 1;
        }
    }
}

/// The yaw from `a` toward `b` (Oni's: atan2(x, z)).
fn yaw(a: Vec3, b: Vec3) -> f32 {
    wrap((b.x - a.x).atan2(b.z - a.z))
}

use crate::angle::wrap_2pi as wrap;

/// Whether `delta` (from the target to the character) lies within the
/// manual trigger's angle of the target's facing.
fn in_front(facing: f32, delta: Vec3) -> bool {
    let a = crate::angle::yaw_of(delta) - facing;
    let a = crate::angle::wrap_pi(a);
    a.abs() <= MANUAL_TRIGGER_ANGLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_trigger_wants_the_character_in_front() {
        // Facing +z: someone ahead counts, someone behind doesn't; 50° off
        // does, 70° doesn't.
        assert!(in_front(0.0, Vec3::new(0.0, 0.0, 10.0)));
        assert!(!in_front(0.0, Vec3::new(0.0, 0.0, -10.0)));
        let at = |deg: f32| Vec3::new(deg.to_radians().sin(), 0.0, deg.to_radians().cos());
        assert!(in_front(0.0, at(50.0)) && !in_front(0.0, at(70.0)));
        assert!(in_front(std::f32::consts::FRAC_PI_2, at(80.0)));
    }
}
