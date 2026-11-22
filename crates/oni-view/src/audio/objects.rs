//! The level's sound objects (`docs/sound/ORIGINAL_CORE.md` §14): the
//! placed ambience — room tones, machine hums, wind.
//!
//! - A **sphere** plays its ambient, placed at the object over the object's
//!   own distances, while the listener is within its min distance: started
//!   silent and faded up over a quarter second to its muffled volume × the
//!   object's, then kept there; leaving it stops the ambient (it plays out).
//! - A **box** plays its ambient unplaced while the listener is inside: a
//!   2-second ease-in, a 2-second ease-out after leaving, then halted. A box
//!   entered while another box of the same ambient still plays takes that
//!   sound over (its fade carried on) rather than starting another.
//!
//! Both take channels at low priority: never another sound's.

use std::path::Path;

use glam::Vec3;

use super::system::LOW;
use super::Audio;
use crate::quads::Quads as Sight;

/// The box fades' length (ticks).
const FADE: f32 = 120.0;
/// A sphere's fade up as it starts (seconds).
const SPHERE_FADE: f32 = 0.25;
/// How far a sphere's muffled volume moves an update.
const VOLUME_ADJUST: f32 = 0.05;

enum Shape {
    /// Distances (feet).
    Sphere { max: f32, min: f32 },
    /// World corners.
    Box { min: Vec3, max: Vec3 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    Starting,
    Playing,
    Stopping,
}

pub(super) struct SoundObject {
    pub ambient: String,
    pub position: Vec3,
    shape: Shape,
    /// The object's own volume.
    object_volume: f32,
    pub play: Option<u32>,
    pub state: State,
    /// A box's fade, or a sphere's volume (× the object's).
    pub volume: f32,
    /// A sphere's muffled volume, moving toward the walls' verdict.
    transition: f32,
    /// When a box's fade started (ticks).
    start: u64,
    occlusion: (u64, u64, f32),
}

impl SoundObject {
    fn inside(&self, p: Vec3) -> bool {
        match self.shape {
            Shape::Sphere { min, .. } => p.distance_squared(self.position) / 9.0 <= min * min,
            Shape::Box { min, max } => p.cmpge(min).all() && p.cmple(max).all(),
        }
    }

    pub fn is_box(&self) -> bool {
        matches!(self.shape, Shape::Box { .. })
    }
}

/// Loads a level's sound objects (`levels/NN/sounds.json`).
pub(super) fn load(level_dir: &Path) -> Vec<SoundObject> {
    let Some(j) = crate::json::read(level_dir.join("sounds.json")) else {
        return Vec::new();
    };
    use crate::json::f32 as f;
    use crate::json::vec3 as v3;
    j["objects"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(k, o)| {
            let position = v3(&o["position"]);
            let shape = if o["sphere"].is_object() {
                Shape::Sphere {
                    max: f(&o["sphere"]["max"]),
                    min: f(&o["sphere"]["min"]),
                }
            } else if o["box"].is_object() {
                Shape::Box {
                    min: position + v3(&o["box"]["min"]),
                    max: position + v3(&o["box"]["max"]),
                }
            } else {
                return None;
            };
            Some(SoundObject {
                ambient: o["ambient"].as_str()?.to_string(),
                position,
                shape,
                object_volume: f(&o["volume"]),
                play: None,
                state: State::Off,
                volume: 0.0,
                transition: 0.0,
                start: 0,
                occlusion: (k as u64 % 12, 0, 1.0),
            })
        })
        .collect()
}

/// A box's ease-in after `t` ticks, and its ease-out.
fn fade_in(t: f32) -> f32 {
    1.0 - (t - FADE) * (t - FADE) / (FADE * FADE)
}

fn fade_out(t: f32) -> f32 {
    1.0 - t * t / (FADE * FADE)
}

/// How long ago an ease-in that has reached `v` began.
fn fade_in_start(v: f32) -> u64 {
    (FADE - FADE * (1.0 - v).max(0.0).sqrt()) as u64
}

impl Audio {
    /// Every sound object's update (none in script-only times).
    pub(super) fn update_objects(&mut self, sight: Option<&Sight>) {
        if self.script_only {
            return;
        }
        for k in 0..self.objects.len() {
            if self.objects[k].is_box() {
                self.update_box(k);
            } else {
                self.update_sphere(k, sight);
            }
        }
    }

    fn update_sphere(&mut self, k: usize, sight: Option<&Sight>) {
        let ear = self.listener.0;
        let o = &mut self.objects[k];
        if !o.inside(ear) {
            if let Some(id) = o.play.take() {
                self.stop(id);
            }
            return;
        }
        let Some(def) = self.defs.ambients.get_mut(&o.ambient) else {
            return;
        };
        // (Permanently: sound objects are low priority — as shipped.)
        def.priority = LOW;
        let min_occlusion = def.min_occlusion;
        let Shape::Sphere { max, min } = o.shape else {
            return;
        };
        let (at, name, mut cache, play) = (o.position, o.ambient.clone(), o.occlusion, o.play);
        if let Some(id) = play {
            if !self.active(id) {
                self.objects[k].play = None;
                return;
            }
        }
        let heard = self.earshot_at(at, min, min_occlusion, &mut cache, true, sight);
        let o = &mut self.objects[k];
        o.occlusion = cache;
        let Some(target) = heard else { return };
        o.transition = (o.transition
            + (target - o.transition).clamp(-VOLUME_ADJUST, VOLUME_ADJUST))
        .clamp(0.0, 1.0);
        o.volume = o.transition * o.object_volume;
        let volume = o.volume;
        match play {
            Some(id) => self.ambient_volume(id, volume, 0.0),
            None => {
                let id = self.start(&name, Some(at), 0.0, Some((max, min)));
                if let Some(id) = id {
                    self.ambient_volume(id, volume, SPHERE_FADE);
                }
                self.objects[k].play = id;
            }
        }
    }

    fn update_box(&mut self, k: usize) {
        let (ear, now) = (self.listener.0, self.tick);
        let o = &self.objects[k];
        let ticks = now.saturating_sub(o.start) as f32;
        let inside = o.inside(ear);
        if o.state == State::Off {
            if !inside || !self.defs.ambients.contains_key(&o.ambient) {
                return;
            }
            // Another box of the same ambient still playing: take it over.
            let name = o.ambient.clone();
            if let Some(j) = (0..self.objects.len()).find(|&j| {
                j != k
                    && self.objects[j].is_box()
                    && self.objects[j].play.is_some()
                    && self.objects[j].ambient == name
            }) {
                let (play, volume) = (self.objects[j].play.take(), self.objects[j].volume);
                self.objects[j].state = State::Off;
                self.objects[j].volume = 0.0;
                let o = &mut self.objects[k];
                o.play = play;
                o.state = State::Starting;
                o.volume = volume;
                o.start = now.saturating_sub(fade_in_start(volume));
                return;
            }
            if let Some(def) = self.defs.ambients.get_mut(&name) {
                def.priority = LOW;
            }
            let id = self.start(&name, None, 0.0, None);
            let o = &mut self.objects[k];
            o.state = State::Starting;
            o.volume = 0.0;
            o.play = id;
            o.start = now;
            return;
        }
        let o = &mut self.objects[k];
        match o.state {
            State::Starting => {
                o.volume = fade_in(ticks.min(FADE));
                if o.volume >= 1.0 {
                    o.volume = 1.0;
                    o.state = State::Playing;
                }
            }
            State::Stopping => {
                o.volume = fade_out(ticks.min(FADE));
                if o.volume <= 0.0 {
                    o.volume = 0.0;
                    o.state = State::Off;
                    if let Some(id) = o.play.take() {
                        self.halt(id);
                    }
                }
            }
            _ => {}
        }
        let o = &self.objects[k];
        let Some(id) = o.play.filter(|id| self.active(*id)) else {
            let o = &mut self.objects[k];
            (o.volume, o.state, o.play) = (0.0, State::Off, None);
            return;
        };
        let volume = o.volume * o.object_volume;
        self.ambient_volume(id, volume, 0.0);
        let o = &mut self.objects[k];
        if !inside {
            if o.state != State::Stopping {
                o.start = now;
            }
            o.state = State::Stopping;
        } else if o.state == State::Stopping {
            // (Back in while fading out: the ease-in starts over from when
            // the fade-out did — the volume jumps, as shipped.)
            o.state = State::Starting;
        }
    }

    /// The sound objects (for scripts and tests): ambient, place, box or
    /// sphere, state, volume, whether its ambient plays.
    pub fn sound_objects(&self) -> Vec<(String, Vec3, bool, State, f32, bool)> {
        self.objects
            .iter()
            .map(|o| {
                (
                    o.ambient.clone(),
                    o.position,
                    o.is_box(),
                    o.state,
                    o.volume,
                    o.play.is_some_and(|id| self.active(id)),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_fades() {
        assert_eq!(fade_in(0.0), 0.0);
        assert_eq!(fade_in(120.0), 1.0);
        assert!(
            (fade_in(60.0) - 0.75).abs() < 1e-6,
            "an ease-in: three quarters up halfway"
        );
        assert!((fade_out(60.0) - 0.75).abs() < 1e-6, "an ease-out");
        assert_eq!(fade_out(120.0), 0.0);
        assert_eq!(fade_in_start(0.75), 60, "a fade at 0.75 began 60 ticks ago");
    }
}
