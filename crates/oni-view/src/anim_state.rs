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
    pub const STAND: u16 = 6;
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
}

/// `TRtAnimState` values.
pub mod state {
    pub const ANYTHING: u16 = 1;
    pub const STANDING: u16 = 7;
}

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
    /// Loads every `{prefix}*.glb` clip under `dir` (a character's animation set).
    pub fn load(dir: &Path, prefix: &str) -> std::io::Result<Self> {
        let mut entries = Vec::new();
        for ent in std::fs::read_dir(dir)? {
            let path = ent?.path();
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !name.starts_with(prefix) || path.extension().and_then(|e| e.to_str()) != Some("glb")
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
}

/// The locomotion state machine driving one character.
pub struct Locomotion {
    coll: AnimCollection,
    cur: usize,
    /// Current frame within the clip (fractional; advanced at 60 Hz).
    frame: f32,
    cur_type: u16,
    /// The `to_state` of the current clip = the `from_state` for the next lookup.
    next_state: u16,
    varient: u16,
    /// Active clip-to-clip blend, if any.
    stitch: Option<Stitch>,
}

impl Locomotion {
    /// Starts the machine on a standing/idle clip.
    pub fn new(coll: AnimCollection) -> Self {
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
        }
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

    /// The clip currently playing.
    pub fn clip(&self) -> &Clip {
        &self.coll.entries[self.cur].clip
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

        let finished = self.frame + 1.0 >= self.frame_count();
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

    fn konoko_collection() -> Option<AnimCollection> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/common/animations");
        if !dir.exists() {
            eprintln!("SKIP: converted animations not found");
            return None;
        }
        let coll = AnimCollection::load(&dir, "KONOKO").ok()?;
        (!coll.is_empty()).then_some(coll)
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
