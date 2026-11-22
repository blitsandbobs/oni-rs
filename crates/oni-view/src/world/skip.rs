//! Skipping a cutscene (`ONrGameState_SkipCutscene`; `docs/GAME_FLOW.md`,
//! *Skipping a cutscene*): asked for, the screen fades to black over 20
//! frames with the sound off; then the game runs on 32 ticks a frame, the
//! screen left black, until the scripts end the cutscene; then it fades back
//! in over 20 frames with the sound back.
//!
//! As shipped nothing asks for it — the one call, on Space, is commented
//! out (Q18); the port answers Space unless `--faithful`.

use super::World;

/// Frames of the fade each way (`ONcNumber_of_frames_for_cutscene_skip_timer`).
const FADE: u32 = 20;
/// Game ticks a frame while skipping.
pub const SKIP_TICKS: u32 = 32;
/// Ticks into a cutscene (or since a splash over it went) before it can be
/// skipped (the original's 10 sixtieths of a second).
const SETTLE: u32 = 10;

/// Where a skip is (`ONtCutsceneSkipMode`), the frames left of its fade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Skip {
    #[default]
    Idle,
    /// Fading to black.
    Starting(u32),
    /// Black: skipping from the next frame.
    Black,
    Skipping,
    /// The cutscene over: fading back in.
    FadingIn(u32),
}

impl World {
    /// A skip asked for (`ONrGameState_SkipCutscene`): in a cutscene settled
    /// for 10 ticks, with none under way, the fade to black starts and the
    /// Whether Esc may bring up the main menu over play: not in a
    /// cutscene, nor over a modal screen (`ONiGameState_ProcessMiscActions`;
    /// the front-end adds its own splash).
    pub fn menu_allowed(&self) -> bool {
        !self.in_cutscene && !self.hud.modal()
    }

    /// sound goes off (`OSrSetEnabled`).
    pub fn skip_cutscene(&mut self) {
        if self.quirks.cutscenes_unskippable() {
            return;
        }
        if self.in_cutscene && self.cutscene_ticks >= SETTLE && self.skip == Skip::Idle {
            self.skip = Skip::Starting(FADE);
            self.sound_enabled(false);
        }
    }

    /// The start of a frame (`ONrGameState_UpdateServerTime`): the fades
    /// count down. Returns how many game ticks this frame runs instead of
    /// the usual (skipping: 32).
    pub fn skip_frame(&mut self) -> Option<u32> {
        self.skip = match self.skip {
            Skip::Starting(n) if n <= 1 => Skip::Black,
            Skip::Starting(n) => Skip::Starting(n - 1),
            Skip::FadingIn(n) if n <= 1 => Skip::Idle,
            Skip::FadingIn(n) => Skip::FadingIn(n - 1),
            s => s,
        };
        (self.skip == Skip::Skipping).then_some(SKIP_TICKS)
    }

    /// The frame drawn (`ONi_GameState_Display_SkipCutsceneFade`): black
    /// drawn, the skip proper begins.
    pub fn skip_drawn(&mut self) {
        if self.skip == Skip::Black {
            self.skip = Skip::Skipping;
        }
    }

    /// How black the screen is for the skip (0 clear … 1 black; none: no
    /// skip). Fading, `ramp^1.9` (`get_cosmetic_ramp`); skipping, the draw
    /// engine draws nothing, so the black stays.
    pub fn skip_blanket(&self) -> Option<f32> {
        let ramp = |left: u32| (left as f32 / FADE as f32).powf(1.9);
        match self.skip {
            Skip::Idle => None,
            Skip::Starting(n) => Some(ramp(FADE - n)),
            Skip::Black | Skip::Skipping => Some(1.0),
            Skip::FadingIn(n) => Some(ramp(n)),
        }
    }

    /// Whether the game is running through a cutscene unseen: the HUD, the
    /// letterbox and the inserts aren't drawn.
    pub fn skipping(&self) -> bool {
        self.in_cutscene && self.skip == Skip::Skipping
    }

    /// The cutscene over (`ONrGameState_CancelSkipping`): a skip fades back
    /// in — from as far as it got, if it was still fading out.
    pub fn cancel_skipping(&mut self) {
        self.skip = match self.skip {
            Skip::Idle => Skip::Idle,
            Skip::Starting(n) => Skip::FadingIn(FADE - n),
            _ => Skip::FadingIn(FADE),
        };
    }

    /// The sound on or off (`OSrSetEnabled`): off, the overall volume to 0,
    /// kept; on again, back.
    pub fn sound_enabled(&mut self, on: bool) {
        match (self.sound_off_volume, on) {
            (None, false) => {
                self.sound_off_volume = Some(self.audio.volume());
                self.audio.set_volume(0.0);
            }
            (Some(v), true) => {
                self.audio.set_volume(v);
                self.sound_off_volume = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faithful_cutscenes_cannot_be_skipped() {
        // Q18: as shipped, Space's skip is commented out.
        let mut w = World::new();
        w.quirks = crate::quirks::Quirks::FAITHFUL;
        w.in_cutscene = true;
        w.cutscene_ticks = 10;
        w.skip_cutscene();
        assert_eq!(w.skip, Skip::Idle);
    }

    #[test]
    fn a_skip_fades_out_runs_fast_and_fades_back() {
        let mut w = World::new();
        // Not in a cutscene: nothing.
        w.skip_cutscene();
        assert_eq!(w.skip, Skip::Idle);
        w.in_cutscene = true;
        w.cutscene_ticks = 5;
        w.skip_cutscene();
        assert_eq!(w.skip, Skip::Idle, "too soon after it began");
        w.cutscene_ticks = 10;
        w.skip_cutscene();
        assert_eq!(w.skip, Skip::Starting(20));
        assert_eq!(w.skip_blanket(), Some(0.0));
        let mut frames = 0;
        while w.skip_frame().is_none() {
            w.skip_drawn();
            frames += 1;
            assert!(frames < 30);
        }
        // 20 frames of fade (the last drawn black), then skipping.
        assert_eq!(frames, 20);
        assert!(w.skipping());
        assert_eq!(w.skip_frame(), Some(32));
        // The cutscene ends: back in over 20 frames.
        w.in_cutscene = false;
        w.cancel_skipping();
        assert_eq!(w.skip_blanket(), Some(1.0));
        for _ in 0..20 {
            assert_eq!(w.skip_frame(), None);
        }
        assert_eq!(w.skip, Skip::Idle);
        // Ended while still fading out: back from as far as it got.
        w.in_cutscene = true;
        w.skip_cutscene();
        for _ in 0..5 {
            w.skip_frame();
        }
        w.cancel_skipping();
        assert_eq!(w.skip, Skip::FadingIn(5));
    }
}
