//! Konoko's locomotion animation state machine (E3.1), ported from Oni's
//! Totoro system (`ONrCharacter_HandleHeartbeatInput` + `TRrCollection_Lookup`,
//! see `ENGINE_NOTES.md`).
//!
//! The model: a character has a current animation **state** (the body position
//! the last clip ended in). Each tick, input is turned into a desired animation
//! **type** via an ordered probe chain; a clip is chosen by looking it up by
//! `(type, from_state == current state, varient)`. Crucially the lookup is
//! **state-gated**: `Run_Start` clips only start from `Standing`, so from a
//! standing pose the probe picks `Run_Start`, and once running (state becomes a
//! `Running_*_Down`) the same probe falls through to the `Run` loop. State
//! gating, not special-casing, produces start → loop → stop.
//!
//! This is the ground-locomotion subset (idle ↔ run ↔ stop); weapons, crouch,
//! air and combat varients are future work. Turning is handled by yaw in the
//! caller, so the varient is always 0 (unarmed).

use std::path::Path;
use std::rc::Rc;

use glam::{Quat, Vec2};

use crate::skel::{self, Character, Clip};

/// Default stitch length when interrupting into a clip (`ONcNumStitchingFrames`).
const STITCH_FRAMES: f32 = 8.0;

/// An in-progress clip-to-clip blend ("stitch"): the pose at the moment of the
/// switch, interpolated out over `count` frames.
struct Stitch {
    saved: Vec<Quat>,
    /// Pelvis height at the moment of the switch (blended out alongside the
    /// pose, mirroring Oni's `stitch.height`).
    saved_height: f32,
    itr: f32,
    count: f32,
}

/// `TRtAnimType` values (from `Oni_Character_Animation.h`; validated against the
/// decoded data — e.g. `NINJArun1stepa` is `Run_Start` = 208).
pub mod anim_type {
    pub const NONE: u16 = 0;
    pub const WALK: u16 = 2;
    pub const RUN: u16 = 3;
    pub const JUMP: u16 = 5; // jump_start (takeoff)
    pub const STAND: u16 = 6;
    // Melee attack types (KONCOM set). The combo chains are in `COMBOS`.
    pub const KICK: u16 = 12; // comb_k
    pub const PUNCH: u16 = 22; // comb_p
    pub const KICK2: u16 = 36; // comb_k_k (KK)
    pub const KICK3: u16 = 37; // comb_k_k_k (KKK)
    pub const PUNCH2: u16 = 38; // comb_p_p (PP)
    pub const PUNCH3: u16 = 39; // comb_p_p_p (PPP)
    pub const PUNCH_PUNCH_KICK: u16 = 44; // comb_p_p_k (PPK)

    /// Whether `t` is a melee attack type (plays to completion, chains via the
    /// combo table, blocks locomotion while active).
    pub fn is_attack(t: u16) -> bool {
        matches!(t, PUNCH | KICK | PUNCH2 | PUNCH3 | KICK2 | KICK3 | PUNCH_PUNCH_KICK)
    }
    pub const STANDING_TURN_LEFT: u16 = 7;
    pub const STANDING_TURN_RIGHT: u16 = 8;
    pub const RUN_BACKWARDS: u16 = 9;
    pub const RUN_SIDESTEP_LEFT: u16 = 10;
    pub const RUN_SIDESTEP_RIGHT: u16 = 11;
    pub const WALK_SIDESTEP_LEFT: u16 = 13;
    pub const WALK_SIDESTEP_RIGHT: u16 = 14;
    pub const WALK_BACKWARDS: u16 = 15;
    pub const IDLE: u16 = 93;
    pub const RUN_STOP: u16 = 206;
    pub const WALK_STOP: u16 = 207;
    pub const RUN_START: u16 = 208;
    pub const WALK_START: u16 = 209;
    pub const RUN_BACKWARDS_START: u16 = 210;
    pub const WALK_BACKWARDS_START: u16 = 211;
    pub const ONE_STEP_STOP: u16 = 216;
    pub const RUN_SIDESTEP_LEFT_START: u16 = 217;
    pub const RUN_SIDESTEP_RIGHT_START: u16 = 218;
    pub const JUMP_LAND: u16 = 24; // jump_land (touchdown)
    pub const JUMP_AIR: u16 = 25; // jump_idle (airborne hold / falling)
}

/// `TRtAnimState` values.
pub mod state {
    pub const ANYTHING: u16 = 1;
    pub const STANDING: u16 = 7;
}

/// Variant bit required by combat clips (`ONcAnimVarientMask_Fight`), OR'd into
/// the lookup variant when selecting an attack so fight-stance clips resolve.
pub const FIGHT_VARIENT: u16 = 0x0200;

/// One combo-table row: pressing `kick` (vs punch) while the last attack was
/// `old` (`NONE` = any / a fresh start) selects `result`. A subset of Oni's
/// `ONgComboTable` — the core punch/kick chains; deeper chains list first so
/// they match before the starters.
struct Combo {
    kick: bool,
    old: u16,
    result: u16,
}

use anim_type::{KICK, KICK2, KICK3, NONE, PUNCH, PUNCH2, PUNCH3, PUNCH_PUNCH_KICK};
const COMBOS: &[Combo] = &[
    // continuations (matched against the previous attack)
    Combo { kick: true, old: PUNCH2, result: PUNCH_PUNCH_KICK }, // PP + K → PPK
    Combo { kick: false, old: PUNCH2, result: PUNCH3 },          // PP + P → PPP
    Combo { kick: true, old: KICK2, result: KICK3 },             // KK + K → KKK
    Combo { kick: false, old: PUNCH, result: PUNCH2 },           // P  + P → PP
    Combo { kick: true, old: KICK, result: KICK2 },              // K  + K → KK
    // fresh starters
    Combo { kick: false, old: NONE, result: PUNCH },
    Combo { kick: true, old: NONE, result: KICK },
];

/// A loaded clip plus its name (used by tests / debug output).
struct Entry {
    #[allow(dead_code)]
    name: String,
    clip: Clip,
}

/// A character's animation collection — the clips to look up by state machine.
pub struct AnimCollection {
    entries: Vec<Entry>,
}

impl AnimCollection {
    /// Loads every clip under `dir` whose name starts with any of `prefixes` — a
    /// character's locomotion set plus its combat (`*COM`) / weapon sets. The
    /// variant bits keep them apart at lookup time (combat clips carry `Fight`,
    /// weapon clips carry weapon bits, so neither is eligible for plain
    /// `varient=0` locomotion).
    pub fn load_sets(dir: &Path, prefixes: &[&str]) -> std::io::Result<Self> {
        let mut entries = Vec::new();
        for ent in std::fs::read_dir(dir)? {
            let path = ent?.path();
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if path.extension().and_then(|e| e.to_str()) != Some("glb")
                || !prefixes.iter().any(|p| name.starts_with(p))
            {
                continue;
            }
            if let Ok(clip) = skel::load_clip(&path) {
                entries.push(Entry {
                    name: name.to_string(),
                    clip,
                });
            }
        }
        Ok(AnimCollection { entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `TRrCollection_Lookup`: find a clip matching `(type, from_state, varient)`.
    /// First tries the exact `from_state`, then `Anything` (1). A clip's variant
    /// bits must be a subset of the character's (`clip.varient & !varient == 0`).
    fn lookup(&self, anim_type: u16, from_state: u16, varient: u16) -> Option<usize> {
        self.lookup_state(anim_type, from_state, varient)
            .or_else(|| self.lookup_state(anim_type, state::ANYTHING, varient))
    }

    fn lookup_state(&self, anim_type: u16, from_state: u16, varient: u16) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.clip.anim_type == anim_type
                && e.clip.from_state == from_state
                && (e.clip.varient & !varient) == 0
        })
    }

    /// First clip of `anim_type` with a compatible variant, ignoring `from_state`
    /// — used to force a jump/land clip regardless of the current body state.
    fn lookup_any(&self, anim_type: u16, varient: u16) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.clip.anim_type == anim_type && (e.clip.varient & !varient) == 0)
    }
}

/// Per-tick locomotion input (from the keyboard in play mode).
#[derive(Clone, Copy, Default)]
pub struct Input {
    pub forward: bool,
    pub backward: bool,
    /// Strafe left/right (Oni `stepleft`/`stepright` — A/D by default).
    pub step_left: bool,
    pub step_right: bool,
    /// Hold-to-walk modifier (Oni `walk` — Caps Lock): prefer walk clips.
    pub walk: bool,
    /// A large pending body turn while standing (past Oni's aiming dead-zone) →
    /// play a turn-in-place clip so the feet re-plant instead of sliding. The
    /// caller sets at most one, from the facing offset's sign.
    pub turn_left: bool,
    pub turn_right: bool,
    /// The character is off the ground: hold the airborne (jump-idle) clip until
    /// the caller lands it. Takeoff/landing are driven by the physics via
    /// [`Locomotion::start_jump`] / [`Locomotion::land`].
    pub airborne: bool,
    /// Melee attack triggers (Oni: Punch=F, Kick=C) — set on the press edge; the
    /// combo chain advances via the previous attack ([`anim_type::COMBOS`]).
    pub punch: bool,
    pub kick: bool,
}

/// The locomotion state machine driving one character. The animation set is
/// shared (`Rc`) so several fighters using the same character each run their own
/// state machine over one loaded collection.
pub struct Locomotion {
    coll: Rc<AnimCollection>,
    cur: usize,
    /// Current frame within the clip (fractional; advanced at 60 Hz).
    frame: f32,
    cur_type: u16,
    /// The `to_state` of the current clip = the `from_state` for the next lookup.
    next_state: u16,
    varient: u16,
    /// Active clip-to-clip blend, if any.
    stitch: Option<Stitch>,
    /// Combat: the type of the current/just-finished attack (0 = none), the tick
    /// its clip ended (for the 10-tick combo-drop window), a monotone tick count,
    /// and a buffered attack trigger pressed during the current attack.
    last_attack: u16,
    last_attack_end: Option<u32>,
    tick_count: u32,
    attack_buffer: Option<bool>,
}

impl Locomotion {
    /// Starts the machine on a standing/idle clip.
    pub fn new(coll: Rc<AnimCollection>) -> Self {
        let varient = 0;
        // Prefer a Stand clip from Standing, else Idle, else clip 0.
        let start = coll
            .lookup(anim_type::STAND, state::STANDING, varient)
            .or_else(|| coll.lookup(anim_type::IDLE, state::STANDING, varient))
            .unwrap_or(0);
        let (cur_type, next_state) = coll
            .entries
            .get(start)
            .map(|e| (e.clip.anim_type, e.clip.to_state))
            .unwrap_or((anim_type::STAND, state::STANDING));
        Locomotion {
            coll,
            cur: start,
            frame: 0.0,
            cur_type,
            next_state,
            varient,
            stitch: None,
            last_attack: 0,
            last_attack_end: None,
            tick_count: 0,
            attack_buffer: None,
        }
    }

    /// The combo result type for pressing punch/kick given the last attack, and
    /// the clip index that plays it (fight-variant, state-gated). `None` if no
    /// combo row matches or no clip exists for it in the current state.
    fn attack_lookup(&self, kick: bool) -> Option<usize> {
        let v = self.varient | FIGHT_VARIENT;
        for c in COMBOS {
            if c.kick == kick && (c.old == NONE || c.old == self.last_attack) {
                if let Some(i) = self.coll.lookup(c.result, self.next_state, v) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// The active stitch blend for posing: `(saved pose, weight)` where weight
    /// `1 → saved`, `0 → current clip`. `None` when not stitching.
    pub fn blend(&self) -> Option<(&[Quat], f32)> {
        self.stitch
            .as_ref()
            .map(|s| (s.saved.as_slice(), 1.0 - s.itr / s.count))
    }

    /// Pelvis height above the ground point this frame (Oni's `heightThisFrame`):
    /// the current clip's height, blended out of the saved height during a stitch
    /// (matching `ONrCharacter_DoAnimation`'s height interpolation).
    pub fn height(&self) -> f32 {
        let h = self.clip().height_at(self.frame_time());
        match &self.stitch {
            Some(s) => {
                let to = s.itr / s.count;
                s.saved_height * (1.0 - to) + h * to
            }
            None => h,
        }
    }

    /// Forces the takeoff clip (`jump_start`), blending out of the current pose.
    /// Called by the physics when a jump launches, so it works from any state
    /// (idle/run/walk). The airborne hold then chains via `choose`.
    pub fn start_jump(&mut self, character: Option<&Character>) {
        if let Some(i) = self.coll.lookup_any(anim_type::JUMP, self.varient) {
            let count = STITCH_FRAMES.min(self.coll.entries[i].clip.max_interpolation as f32);
            self.switch_to(i, count, character);
        }
    }

    /// Forces the landing clip (`jump_land`) on touchdown; after it ends the
    /// machine falls back to ground locomotion via `choose`.
    pub fn land(&mut self, character: Option<&Character>) {
        if let Some(i) = self.coll.lookup_any(anim_type::JUMP_LAND, self.varient) {
            let count = STITCH_FRAMES.min(self.coll.entries[i].clip.max_interpolation as f32);
            self.switch_to(i, count, character);
        }
    }

    /// The clip currently playing.
    pub fn clip(&self) -> &Clip {
        &self.coll.entries[self.cur].clip
    }

    /// The attack (if any) whose damage window contains the current frame — i.e.
    /// the current clip is an attack and we are within `[firstDamageFrame,
    /// lastDamageFrame]`. Drives hit detection (`docs/COMBAT.md` §3).
    pub fn active_attack(&self) -> Option<skel::Attack> {
        let f = self.frame;
        self.clip()
            .attacks
            .iter()
            .find(|a| (a.first_damage_frame as f32) <= f && f <= (a.last_damage_frame as f32))
            .copied()
    }

    /// Name of the clip currently playing (used by tests / debug output).
    #[allow(dead_code)]
    pub fn clip_name(&self) -> &str {
        &self.coll.entries[self.cur].name
    }

    /// Playback time (seconds) for posing the current clip.
    pub fn frame_time(&self) -> f32 {
        self.frame / skel::FPS
    }

    /// Number of frames in the current clip.
    fn frame_count(&self) -> f32 {
        let c = self.clip();
        let by_rm = c.root_motion.len() as f32;
        let by_dur = (c.duration * skel::FPS).round() + 1.0;
        by_rm.max(by_dur).max(1.0)
    }

    /// Advances one 60 Hz tick: picks/continues a clip per the probe chain and
    /// returns the character-local `[x, z]` root-motion for this frame.
    /// `character` (when given) lets a clip switch capture the current pose to
    /// blend out of — pass `None` to drive the machine without stitching.
    pub fn tick(&mut self, input: Input, character: Option<&Character>) -> Vec2 {
        // Advance any in-progress stitch (so a stitch begun this tick renders at
        // full strength this frame, then decays).
        if let Some(s) = &mut self.stitch {
            s.itr += 1.0;
            if s.itr >= s.count {
                self.stitch = None;
            }
        }

        self.tick_count += 1;
        // Combo-drop window: the chain survives the attack plus ~10 ticks after
        // it ends (Oni's `lastAttackTime` + 10), then the next press is a fresh
        // start (`Oni_GameState.c:3721`).
        if self.last_attack != 0 {
            if let Some(end) = self.last_attack_end {
                if self.tick_count.saturating_sub(end) > 10 {
                    self.last_attack = 0;
                }
            }
        }

        let finished = self.frame + 1.0 >= self.frame_count();

        // Attack probe (above locomotion, as in Oni's chain). An attack plays to
        // completion; a trigger pressed during it is buffered and chains on the
        // frame it ends.
        if anim_type::is_attack(self.cur_type) {
            if input.punch {
                self.attack_buffer = Some(false);
            }
            if input.kick {
                self.attack_buffer = Some(true);
            }
            if finished {
                if let Some(kick) = self.attack_buffer.take() {
                    if let Some(i) = self.attack_lookup(kick) {
                        let count =
                            STITCH_FRAMES.min(self.coll.entries[i].clip.max_interpolation as f32);
                        self.switch_to(i, count, character);
                        self.last_attack = self.cur_type;
                        self.last_attack_end = None;
                        return self.current_root_motion();
                    }
                }
                // No continuation: the attack ends; start the drop timer and fall
                // through to the locomotion logic (settle to idle / resume moving).
                self.last_attack_end = Some(self.tick_count);
            } else {
                self.frame += 1.0;
                return self.current_root_motion();
            }
        } else if input.punch || input.kick {
            // Fresh attack: interrupt idle/locomotion with a starter.
            let kick = input.kick && !input.punch;
            if let Some(i) = self.attack_lookup(kick) {
                let count = STITCH_FRAMES.min(self.coll.entries[i].clip.max_interpolation as f32);
                self.switch_to(i, count, character);
                self.last_attack = self.cur_type;
                self.last_attack_end = None;
                self.attack_buffer = None;
                return self.current_root_motion();
            }
        }

        let resting = matches!(
            self.cur_type,
            anim_type::STAND | anim_type::IDLE | anim_type::NONE
        );
        let chosen = self.choose(input);

        if finished {
            // Clip ended: chain to the chosen clip (or settle to standing). A
            // natural chain blends out over the old clip's `endInterpolation`.
            let next = chosen
                .or_else(|| {
                    self.coll
                        .lookup(anim_type::STAND, state::STANDING, self.varient)
                })
                .unwrap_or(self.cur);
            let count = self.clip().end_interpolation as f32;
            self.switch_to(next, count, character);
        } else if resting {
            // Idling: interrupt immediately if the input wants movement. An
            // interrupt blends over min(8, maxInterpolation) of the new clip.
            match chosen {
                Some(n) if n != self.cur => {
                    let count =
                        STITCH_FRAMES.min(self.coll.entries[n].clip.max_interpolation as f32);
                    self.switch_to(n, count, character);
                }
                _ => self.frame += 1.0,
            }
        } else {
            // A movement clip plays out fully before the next lookup.
            self.frame += 1.0;
        }

        self.current_root_motion()
    }

    /// The current frame's character-local `[x, z]` root motion.
    fn current_root_motion(&self) -> Vec2 {
        let f = (self.frame as usize).min(self.clip().root_motion.len().saturating_sub(1));
        self.clip()
            .root_motion
            .get(f)
            .map(|&[x, z]| Vec2::new(x, z))
            .unwrap_or(Vec2::ZERO)
    }

    /// Switches to clip `i`, optionally starting a `count`-frame blend out of the
    /// current pose (captured from `character` at the current clip/frame).
    fn switch_to(&mut self, i: usize, count: f32, character: Option<&Character>) {
        if count >= 1.0 {
            if let Some(ch) = character {
                let saved = ch.local_rotations(Some(self.clip()), self.frame_time());
                let saved_height = self.clip().height_at(self.frame_time());
                self.stitch = Some(Stitch {
                    saved,
                    saved_height,
                    itr: 0.0,
                    count,
                });
            }
        }
        self.cur = i;
        self.frame = 0.0;
        let e = &self.coll.entries[i];
        self.cur_type = e.clip.anim_type;
        self.next_state = e.clip.to_state;
    }

    /// The probe chain: input → the first clip whose state-gated lookup succeeds.
    fn choose(&self, input: Input) -> Option<usize> {
        use anim_type::*;
        let st = self.next_state;
        let v = self.varient;

        // Airborne: hold the jump-idle (air) clip. Takeoff plays jump_start
        // (forced by `start_jump`), which chains here once it ends; the air clip
        // then loops until `land` is called. None keeps the current clip.
        if input.airborne {
            return self.coll.lookup(JUMP_AIR, st, v);
        }

        // HandleStartRunning: forward/backward → run (start then loop), then
        // walk. The Walk modifier (Caps Lock) tries the walk clips first; the
        // fromState gating picks start-vs-loop (as for run).
        if input.forward || input.backward {
            let types: &[u16] = match (input.backward && !input.forward, input.walk) {
                (true, false) => &[
                    RUN_BACKWARDS_START,
                    RUN_BACKWARDS,
                    WALK_BACKWARDS_START,
                    WALK_BACKWARDS,
                ],
                (true, true) => &[
                    WALK_BACKWARDS_START,
                    WALK_BACKWARDS,
                    RUN_BACKWARDS_START,
                    RUN_BACKWARDS,
                ],
                (false, false) => &[RUN_START, RUN, WALK_START, WALK],
                (false, true) => &[WALK_START, WALK, RUN_START, RUN],
            };
            for &t in types {
                if let Some(i) = self.coll.lookup(t, st, v) {
                    return Some(i);
                }
            }
        }

        // HandleStartSidestep: step-left/right → sidestep (start then loop). The
        // sidestep clips carry sideways root motion, so the body keeps facing
        // forward (true strafe). No forward/back required.
        if input.step_left || input.step_right {
            let right = input.step_right && !input.step_left;
            let types: &[u16] = match (right, input.walk) {
                (false, false) => &[RUN_SIDESTEP_LEFT_START, RUN_SIDESTEP_LEFT, WALK_SIDESTEP_LEFT],
                (false, true) => &[WALK_SIDESTEP_LEFT],
                (true, false) => {
                    &[RUN_SIDESTEP_RIGHT_START, RUN_SIDESTEP_RIGHT, WALK_SIDESTEP_RIGHT]
                }
                (true, true) => &[WALK_SIDESTEP_RIGHT],
            };
            for &t in types {
                if let Some(i) = self.coll.lookup(t, st, v) {
                    return Some(i);
                }
            }
        }

        let moving = input.forward || input.backward || input.step_left || input.step_right;

        // HandleStandingTurn: a large pending turn while standing plays a
        // turn-in-place clip (feet re-plant); the body yaw is still slewed by the
        // caller. Konoko's turn clips carry finalRotation=0, so there is no
        // rotation to double-count — the clip is purely visual.
        if !moving && (input.turn_left || input.turn_right) {
            let t = if input.turn_right {
                STANDING_TURN_RIGHT
            } else {
                STANDING_TURN_LEFT
            };
            if let Some(i) = self.coll.lookup(t, st, v) {
                return Some(i);
            }
        }

        // HandleStop: in a locomotion loop with no move key → settle.
        if !moving && is_locomotion(self.cur_type) {
            for &t in &[ONE_STEP_STOP, RUN_STOP, WALK_STOP, STAND] {
                if let Some(i) = self.coll.lookup(t, st, v) {
                    return Some(i);
                }
            }
        }

        // HandleFallThrough: the default resting pose.
        for &t in &[STAND, IDLE] {
            if let Some(i) = self.coll.lookup(t, st, v) {
                return Some(i);
            }
        }
        None
    }
}

/// Whether a type is a ground-locomotion loop/start (so releasing input triggers
/// a stop rather than an immediate idle).
fn is_locomotion(t: u16) -> bool {
    use anim_type::*;
    matches!(
        t,
        WALK | RUN
            | RUN_BACKWARDS
            | WALK_BACKWARDS
            | RUN_START
            | WALK_START
            | RUN_BACKWARDS_START
            | WALK_BACKWARDS_START
            | RUN_SIDESTEP_LEFT
            | RUN_SIDESTEP_RIGHT
            | WALK_SIDESTEP_LEFT
            | WALK_SIDESTEP_RIGHT
            | RUN_SIDESTEP_LEFT_START
            | RUN_SIDESTEP_RIGHT_START
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn konoko_collection() -> Option<Rc<AnimCollection>> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/common/animations");
        if !dir.exists() {
            eprintln!("SKIP: converted animations not found");
            return None;
        }
        let coll = AnimCollection::load_sets(&dir, &["KONOKO", "KONCOM"]).ok()?;
        (!coll.is_empty()).then(|| Rc::new(coll))
    }

    /// The locomotion machine goes stand → run (via a start) → stop → stand,
    /// driven only by a held/released "forward" input, with motion forward.
    #[test]
    fn locomotion_idle_run_stop() {
        let Some(coll) = konoko_collection() else {
            return;
        };
        assert!(coll.len() > 50, "full KONOKO set, got {}", coll.len());
        let mut loco = Locomotion::new(coll);

        // Resting at start.
        assert!(
            matches!(loco.cur_type, anim_type::STAND | anim_type::IDLE),
            "starts resting, got type {} ({})",
            loco.cur_type,
            loco.clip_name()
        );

        // Hold forward; collect which anim types play and total forward motion.
        let fwd = Input {
            forward: true,
            ..Default::default()
        };
        let mut types = Vec::new();
        let mut forward_motion = 0.0f32;
        for _ in 0..90 {
            forward_motion += loco.tick(fwd, None).y;
            types.push((loco.cur_type, loco.clip_name().to_string()));
        }
        assert!(
            types.iter().any(|(t, _)| *t == anim_type::RUN),
            "enters the Run loop while holding forward; saw {:?}",
            dedup(&types)
        );
        assert!(
            types
                .iter()
                .any(|(t, _)| *t == anim_type::RUN_START || *t == anim_type::WALK_START),
            "plays a start clip first; saw {:?}",
            dedup(&types)
        );
        assert!(
            forward_motion.abs() > 5.0,
            "accumulates locomotion distance, got {forward_motion}"
        );

        // Release: should settle back to a resting pose within a couple seconds.
        let idle = Input::default();
        let mut rested = false;
        for _ in 0..180 {
            loco.tick(idle, None);
            if matches!(loco.cur_type, anim_type::STAND | anim_type::IDLE) {
                rested = true;
                break;
            }
        }
        assert!(rested, "returns to rest after releasing forward");
        eprintln!(
            "locomotion ok: {} clips, run cycle {:?}",
            loco.coll.len(),
            dedup(&types)
        );
    }

    fn dedup(seq: &[(u16, String)]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, n) in seq {
            if out.last() != Some(n) {
                out.push(n.clone());
            }
        }
        out
    }

    /// Holding step-left enters a left sidestep (start then loop), with sideways
    /// root motion but little forward motion.
    #[test]
    fn strafe_left_sidesteps() {
        let Some(coll) = konoko_collection() else {
            return;
        };
        let mut loco = Locomotion::new(coll);
        let step = Input {
            step_left: true,
            ..Default::default()
        };
        let mut types = Vec::new();
        let (mut side, mut fwd) = (0.0f32, 0.0f32);
        for _ in 0..60 {
            let d = loco.tick(step, None);
            side += d.x;
            fwd += d.y;
            types.push((loco.cur_type, loco.clip_name().to_string()));
        }
        assert!(
            types.iter().any(|(t, _)| *t == anim_type::RUN_SIDESTEP_LEFT),
            "enters the left sidestep loop; saw {:?}",
            dedup(&types)
        );
        assert!(
            side.abs() > fwd.abs(),
            "sidesteps sideways, not forward: side={side} fwd={fwd}"
        );
    }

    /// Holding punch runs the P → PP → PPP combo chain (the three clips all play
    /// within a bounded window; the buffered trigger chains on each clip's end).
    #[test]
    fn punch_combo_chains() {
        let Some(coll) = konoko_collection() else {
            return;
        };
        let mut loco = Locomotion::new(coll);
        for _ in 0..10 {
            loco.tick(Input::default(), None); // settle standing
        }
        let punch = Input {
            punch: true,
            ..Default::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..300 {
            loco.tick(punch, None);
            if anim_type::is_attack(loco.cur_type) {
                seen.insert(loco.cur_type);
            }
        }
        use anim_type::*;
        assert!(seen.contains(&PUNCH), "played the P starter; saw {seen:?}");
        assert!(seen.contains(&PUNCH2), "chained to PP; saw {seen:?}");
        assert!(seen.contains(&PUNCH3), "chained to PPP; saw {seen:?}");
    }

    /// A kick starter plays, and after the combo-drop window a fresh press starts
    /// a new combo rather than continuing.
    #[test]
    fn kick_starts_and_combo_drops() {
        let Some(coll) = konoko_collection() else {
            return;
        };
        let mut loco = Locomotion::new(coll);
        for _ in 0..10 {
            loco.tick(Input::default(), None);
        }
        loco.tick(
            Input {
                kick: true,
                ..Default::default()
            },
            None,
        );
        assert_eq!(loco.cur_type, anim_type::KICK, "kick starter plays");
        // Let it finish and idle well past the 10-tick drop window.
        for _ in 0..60 {
            loco.tick(Input::default(), None);
        }
        assert_eq!(loco.last_attack, 0, "combo dropped after the window");
    }

    /// With the walk modifier held, forward input selects a Walk clip, not Run.
    #[test]
    fn walk_modifier_prefers_walk() {
        let Some(coll) = konoko_collection() else {
            return;
        };
        let mut loco = Locomotion::new(coll);
        let walk_fwd = Input {
            forward: true,
            walk: true,
            ..Default::default()
        };
        let mut saw_walk = false;
        let mut saw_run = false;
        for _ in 0..90 {
            loco.tick(walk_fwd, None);
            match loco.cur_type {
                anim_type::WALK | anim_type::WALK_START => saw_walk = true,
                anim_type::RUN => saw_run = true,
                _ => {}
            }
        }
        assert!(saw_walk, "walk modifier reaches the Walk loop");
        assert!(!saw_run, "walk modifier does not fall into the Run loop");
    }
}
