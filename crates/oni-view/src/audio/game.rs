//! The game's sound layer (`docs/sound/ORIGINAL_CORE.md` §7, §8.2, §10):
//! placed ambients kept in and out of earshot and muffled by walls, and the
//! gate every impulse passes.

use glam::Vec3;

use super::defs::PLAY_ONCE;
use super::Audio;
use crate::physics::Sight;

/// How far the muffled volume moves an update.
const VOLUME_ADJUST: f32 = 0.05;
/// Ticks between a handle's occlusion rays.
const OCCLUSION_INTERVAL: u64 = 12;
/// Wall hits counted at most.
const MAX_COLLISIONS: usize = 3;
/// World units → feet.
const FOOT_TO_DIST: f32 = 1.0 / 3.0;

/// A game-side handle on a started (placed) ambient.
pub(super) struct Handle {
    pub name: String,
    pub position: Vec3,
    /// Its playing ambient, while in earshot.
    pub play: Option<u32>,
    pub volume: f32,
    /// (phase, last ray tick, last volume).
    pub occlusion: (u64, u64, f32),
}

impl Audio {
    /// The muffling of a sound at `at` by walls between it and the ear.
    fn occlusion(&self, at: Vec3, min_occlusion: f32, sight: Option<&Sight>) -> f32 {
        if min_occlusion > 0.99 {
            return 1.0;
        }
        let hits = sight.map_or(0, |s| s.hits(at, self.listener.0, MAX_COLLISIONS));
        if hits > 0 {
            min_occlusion.max(0.8 - (hits * hits) as f32 / 12.0)
        } else {
            1.0
        }
    }

    /// The muffled volume of a sound at `at` heard within `min` feet
    /// (`None`: out of earshot), its ray cast only on its turn when `cached`
    /// (`cache`: phase, last ray tick, last volume).
    pub(super) fn earshot_at(
        &self,
        at: Vec3,
        min: f32,
        min_occlusion: f32,
        cache: &mut (u64, u64, f32),
        cached: bool,
        sight: Option<&Sight>,
    ) -> Option<f32> {
        if at.distance(self.listener.0) * FOOT_TO_DIST > min {
            return None;
        }
        let (offset, last, cached_volume) = *cache;
        let due = last - last % OCCLUSION_INTERVAL + OCCLUSION_INTERVAL + offset;
        let volume = if cached && self.tick < due && min_occlusion <= 0.99 {
            cached_volume
        } else {
            cache.1 = self.tick;
            self.occlusion(at, min_occlusion, sight)
        };
        cache.2 = volume;
        Some(volume)
    }

    /// Whether `h` is in earshot, and its muffled volume.
    fn earshot(&self, h: &mut Handle, cached: bool, sight: Option<&Sight>) -> Option<f32> {
        let def = self.defs.ambients.get(&h.name)?;
        let (min, min_occlusion) = (def.min_distance, def.min_occlusion);
        self.earshot_at(h.position, min, min_occlusion, &mut h.occlusion, cached, sight)
    }

    /// Starts an ambient at `position` (the game side); `None`: unknown, or
    /// a play-once sound out of earshot.
    pub fn ambient_start(&mut self, name: &str, position: Vec3, sight: Option<&Sight>) -> Option<usize> {
        let play_once = self.defs.ambients.get(name)?.flags & PLAY_ONCE != 0;
        let slot = self.handles.iter().position(Option::is_none).unwrap_or(self.handles.len());
        let mut h =
            Handle { name: name.to_string(), position, play: None, volume: 1.0, occlusion: (slot as u64 % OCCLUSION_INTERVAL, 0, 1.0) };
        match self.earshot(&mut h, false, sight) {
            Some(volume) => {
                if !self.script_only {
                    h.play = self.start(name, Some(position), volume, None);
                }
                h.volume = volume;
            }
            None if play_once => return None,
            None => {}
        }
        if slot == self.handles.len() {
            self.handles.push(Some(h));
        } else {
            self.handles[slot] = Some(h);
        }
        Some(slot)
    }

    /// Stops a started ambient (it plays out unless it interrupts on stop).
    pub fn ambient_stop(&mut self, handle: usize) {
        let Some(h) = self.handles.get_mut(handle).and_then(Option::take) else { return };
        if let Some(id) = h.play {
            self.stop(id);
        }
    }

    /// The handles' update: in and out of earshot, muffling.
    pub(super) fn update_handles(&mut self, sight: Option<&Sight>) {
        for slot in 0..self.handles.len() {
            let Some(mut h) = self.handles[slot].take() else { continue };
            let play_once = self.defs.ambients.get(&h.name).is_some_and(|d| d.flags & PLAY_ONCE != 0);
            let alive = |a: &Audio, id: u32| a.playing.iter().any(|p| p.id == id);
            match self.earshot(&mut h, true, sight) {
                None => {
                    if let Some(id) = h.play {
                        if !play_once {
                            self.halt(id);
                            h.play = None;
                        } else if !alive(self, id) {
                            continue;
                        }
                    }
                }
                Some(target) => match h.play {
                    Some(id) if alive(self, id) => {
                        let volume = (h.volume + (target - h.volume).clamp(-VOLUME_ADJUST, VOLUME_ADJUST)).clamp(0.0, 1.0);
                        if let Some(p) = self.playing.iter_mut().find(|p| p.id == id) {
                            p.channel_volume = volume;
                            p.volume_set = true;
                            p.position = Some(h.position);
                        }
                        h.volume = volume;
                    }
                    Some(_) if play_once => continue,
                    _ => {
                        h.play = if self.script_only { None } else { self.start(&h.name, Some(h.position), target, None) };
                        h.volume = target;
                    }
                },
            }
            self.handles[slot] = Some(h);
        }
    }

    /// Plays impulse sound `name` at `at` (`None`: not placed), its volume ×
    /// `volume`; the channel, or `None` (unknown, out of earshot, no channel
    /// to be had).
    pub fn impulse_play(&mut self, name: &str, at: Option<Vec3>, volume: Option<f32>, sight: Option<&Sight>) -> Option<usize> {
        let imp = self.defs.impulses.get(name)?;
        let mut v = 1.0;
        if let Some(p) = at {
            if p.distance(self.listener.0) * FOOT_TO_DIST > imp.min_distance {
                return None;
            }
            v = self.occlusion(p, imp.min_occlusion, sight);
        }
        v *= volume.unwrap_or(1.0);
        if self.script_only {
            return None;
        }
        self.impulse_system(name, at, v, 0)
    }
}
