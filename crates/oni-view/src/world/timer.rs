//! The countdown timer (//! `ONrGameState_Timer_*`): a script's `timer_start seconds script` counts
//! down; at zero the script runs once and the display blinks 0:00.0 for five
//! seconds; `timer_stop` ends it (a success, with its sound, while it still
//! ran). Shown in the right ring's middle, blinking faster as it runs out.

use super::World;

/// Ticks a second.
const SECOND: u32 = 60;
/// The longest timer (an hour).
const MAX_TICKS: u32 = 60 * 60 * SECOND;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Counting down.
    On,
    /// Ran out: 0:00.0 blinks a while.
    Fail,
}

#[derive(Clone, Debug)]
pub struct Timer {
    pub mode: Mode,
    pub ticks: u32,
    pub script: String,
    /// Ticks since the display last blinked, and whether it shows now.
    drawn: u32,
    shown: bool,
}

impl World {
    /// `timer_start seconds script` (`AIiScript_Timer_Start`): `seconds`
    /// (held to an hour) counted down, then `script`.
    pub fn timer_start(&mut self, seconds: f32, script: &str) {
        let ticks = (seconds * SECOND as f32)
            .clamp(0.0, MAX_TICKS as f32)
            .round() as u32;
        self.timer = Some(Timer {
            mode: Mode::On,
            ticks,
            script: script.to_string(),
            drawn: 0,
            shown: true,
        });
    }

    /// `timer_stop`: off — a success, with its sound, if it was still running.
    pub fn timer_stop(&mut self) {
        if self.timer.as_ref().is_some_and(|t| t.mode == Mode::On) {
            self.event_sound("timerSuccess", None);
        }
        self.timer = None;
    }

    /// Whether the countdown is under its last ten seconds (or blinking
    /// out): the critical loop plays (`timerCritical`).
    pub fn timer_critical(&self) -> bool {
        self.timer.as_ref().is_some_and(|t| t.ticks <= 10 * SECOND)
    }

    /// One tick (`ONrGameState_Timer_Update`): a tick sound each whole
    /// second above the last ten; at zero, the script (the mission takes it
    /// from [`World::timer_expired`]) and five seconds of 0:00.0.
    pub(super) fn timer_tick(&mut self) {
        let critical = self.timer_critical();
        let Some(t) = self.timer.as_mut() else { return };
        let tick_sound = t.mode == Mode::On && !critical && t.ticks % SECOND == 0;
        t.ticks = t.ticks.saturating_sub(1);
        t.drawn += 1;
        if t.ticks == 0 {
            match t.mode {
                Mode::On => {
                    t.mode = Mode::Fail;
                    t.ticks = 5 * SECOND;
                    self.timer_expired = Some(t.script.clone());
                }
                Mode::Fail => self.timer = None,
            }
        }
        // The blink (`ONrGameState_Timer_Display`): every 15 ticks under 20
        // seconds (and while it blinks out), 8 under 10, 4 under 5.
        if let Some(t) = self.timer.as_mut() {
            let every = match t.mode {
                Mode::On if t.ticks < 5 * SECOND => 4,
                Mode::On if t.ticks < 10 * SECOND => 8,
                Mode::On if t.ticks < 20 * SECOND => 15,
                Mode::On => 0,
                Mode::Fail => 15,
            };
            if every == 0 {
                t.shown = true;
            } else if t.drawn > every {
                t.shown = !t.shown;
                t.drawn = 0;
            }
        }
        if tick_sound {
            self.event_sound("timerTick", None);
        }
    }

    /// What the timer shows now (None: nothing, or blinked off): `m:ss`,
    /// under ten seconds `m:ss.t`; run out, `0:00.0`.
    pub fn timer_text(&self) -> Option<String> {
        let t = self.timer.as_ref().filter(|t| t.shown)?;
        Some(show(t.mode, t.ticks))
    }
}

/// A timer's display: `m:ss`, under ten seconds `m:ss.t`; run out, `0:00.0`.
fn show(mode: Mode, ticks: u32) -> String {
    match mode {
        Mode::Fail => "0:00.0".to_string(),
        Mode::On => {
            let (minutes, seconds, tenths) = (
                ticks / (SECOND * 60),
                (ticks / SECOND) % 60,
                (ticks / (SECOND / 10)) % 10,
            );
            if minutes > 0 || seconds >= 10 {
                format!("{minutes}:{seconds:02}")
            } else {
                format!("{minutes}:{seconds:02}.{tenths}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_shows_tenths_under_ten_seconds() {
        assert_eq!(show(Mode::On, 90 * 60), "1:30");
        assert_eq!(show(Mode::On, 61 * 60), "1:01");
        assert_eq!(show(Mode::On, 10 * 60), "0:10");
        assert_eq!(show(Mode::On, 9 * 60 + 30), "0:09.5");
        assert_eq!(show(Mode::Fail, 200), "0:00.0");
    }
}
