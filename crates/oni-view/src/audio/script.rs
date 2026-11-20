//! What level scripts play (`docs/sound/ORIGINAL_CORE.md` §12, §13): music
//! and script ambients — one pool of 16 named, unplaced slots — and the one
//! dialogue voice.

use super::system::Stage;
use super::Audio;

/// Script slots at once.
const SLOTS: usize = 16;
/// The highest priority: what scripts start takes it (for good — as shipped).
const HIGHEST: u8 = 3;
/// A line whose length is unknown, and wouldn't start, lasts this long.
const DEFAULT_DURATION: u64 = 120;

/// The dialogue voice: the line's playing ambient and when blocking ends.
#[derive(Default)]
pub(super) struct Dialog {
    playing: bool,
    play: Option<u32>,
    end: Option<u64>,
}

/// A dialogue request that has to wait for the line before it.
pub struct Busy;

impl Audio {
    fn live(&self, id: Option<u32>) -> bool {
        id.is_some_and(|id| self.playing.iter().any(|p| p.id == id && p.stage != Stage::Done))
    }

    /// `sound_music_start` / `sound_ambient_start`: ambient `name`, unplaced,
    /// at `volume` (0–1), in the first free slot. False: unknown, or no slot.
    pub fn script_start(&mut self, name: &str, volume: f32) -> bool {
        let Some(name) = self.defs.ambient_name(name) else { return false };
        let name = name.as_str();
        let Some(def) = self.defs.ambients.get_mut(name) else { return false };
        let Some(slot) = (0..SLOTS).find(|&k| self.script_slots.get(k).is_none_or(Option::is_none)) else { return false };
        def.priority = HIGHEST;
        let id = self.start(name, None, volume.clamp(0.0, 1.0), None);
        if self.script_slots.len() <= slot {
            self.script_slots.resize(slot + 1, None);
        }
        self.script_slots[slot] = Some((id, name.to_string()));
        true
    }

    /// `…_stop`: the first slot of that name still playing plays out.
    pub fn script_stop(&mut self, name: &str) {
        let id = self
            .script_slots
            .iter()
            .flatten()
            .find(|(id, n)| n.eq_ignore_ascii_case(name) && self.live(*id))
            .and_then(|(id, _)| *id);
        if let Some(id) = id {
            self.stop(id);
        }
    }

    /// `…_volume`: the first slot of that name — even one that has ended (as
    /// shipped) — fades to `volume` over `seconds`.
    pub fn script_volume(&mut self, name: &str, volume: f32, seconds: f32) {
        let id = self.script_slots.iter().flatten().find(|(_, n)| n.eq_ignore_ascii_case(name)).and_then(|(id, _)| *id);
        if let Some(id) = id {
            self.ambient_volume(id, volume, seconds.max(0.0));
        }
    }

    /// Plays dialogue line `name` (unplaced, at full volume): if a line is
    /// still on, `interrupt` cuts it off, else this one must wait (`Busy`).
    /// Its length in ticks (`OSiAmbient_GetSimpleDuration`, 120 when
    /// unknown); none: no such line.
    pub fn dialog_start(&mut self, name: &str, interrupt: bool) -> Result<Option<u32>, Busy> {
        if self.dialog.playing {
            if !interrupt {
                return Err(Busy);
            }
            if let Some(id) = self.dialog.play.take() {
                self.halt(id);
            }
            self.dialog = Dialog::default();
        }
        let Some(name) = self.defs.ambient_name(name) else { return Ok(None) };
        let name = name.as_str();
        let Some(def) = self.defs.ambients.get_mut(name) else { return Ok(None) };
        def.priority = HIGHEST;
        let play = self.start(name, None, 1.0, None);
        let duration = u64::from(self.ambient_duration(name));
        let end = match (duration, play) {
            (0, None) => Some(self.tick + DEFAULT_DURATION),
            (0, Some(_)) => None,
            (d, _) => Some(self.tick + d),
        };
        self.dialog = Dialog { playing: true, play, end };
        // (Its length for the subtitle: unknown, 120 ticks.)
        Ok(Some(if duration == 0 { DEFAULT_DURATION as u32 } else { duration as u32 }))
    }

    /// Whether a dialogue line is on.
    pub fn dialog_playing(&self) -> bool {
        self.dialog.playing
    }

    /// The script slots' and the dialogue's update: ended slots free, a line
    /// over when its sound ends or its length runs out (the sound may play
    /// on; scripts stop waiting).
    pub(super) fn script_tick(&mut self) {
        for k in 0..self.script_slots.len() {
            if let Some((id, _)) = &self.script_slots[k] {
                if !self.live(*id) {
                    self.script_slots[k] = None;
                }
            }
        }
        if self.dialog.playing {
            let over = !self.live(self.dialog.play) || self.dialog.end.is_some_and(|e| self.tick >= e);
            if over {
                self.dialog = Dialog::default();
            }
        }
    }
}
