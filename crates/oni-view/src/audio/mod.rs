//! Sound (`docs/AUDIO.md`): the sound definitions (`common/sounds.json`),
//! ambient sounds as the original plays them — an in sound, a looping body,
//! an out sound, a detail track; heard by distance, panned, muffled by walls —
//! and the mixer and output device under them.
//!
//! Two layers, as in the original: a game-side **handle** per started sound
//! (`ambient_start` / `_stop` / `_move`) that decides each tick whether it
//! is in earshot and how muffled, (re)starting or halting the sound-system
//! **playing ambient**, which steps through its stages on one voice.

pub mod mixer;
pub mod output;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use glam::Vec3;
use serde_json::Value;

use mixer::{Clip, Mixer, Play, Voice, VoiceId};

/// World units → the definitions' distances ("feet").
const FOOT_TO_DIST: f32 = 1.0 / 3.0;
/// Closer than this (squared, world units, flat) a sound isn't panned.
const NO_PAN_DISTANCE_SQUARED: f32 = 49.0;
/// How far the muffled volume moves a tick.
const VOLUME_ADJUST: f32 = 0.05;
/// Ticks between a handle's occlusion rays.
const OCCLUSION_INTERVAL: u64 = 12;
/// Wall hits counted at most.
const MAX_COLLISIONS: usize = 3;

const IN: u8 = 0;
const BODY: u8 = 1;
const OUT: u8 = 2;
const DETAIL: u8 = 3;

struct Permutation {
    weight: u32,
    volume: (f32, f32),
    pitch: (f32, f32),
    sound: String,
}

struct Group {
    volume: f32,
    pitch: f32,
    prevent_repeats: bool,
    /// The last permutation picked.
    last: usize,
    permutations: Vec<Permutation>,
}

struct AmbientDef {
    flags: u32,
    sphere_radius: f32,
    detail_time: (f32, f32),
    max_distance: f32,
    min_distance: f32,
    detail: Option<String>,
    base: Option<String>,
    in_sound: Option<String>,
    out_sound: Option<String>,
    threshold: usize,
    min_occlusion: f32,
}

const INTERRUPT_ON_STOP: u32 = 1;
const PLAY_ONCE: u32 = 2;
const PAN: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    In,
    Body,
    Stopping,
    Out,
    Done,
}

/// A sound-system playing ambient.
struct Playing {
    id: u32,
    name: String,
    stage: Stage,
    voice: Option<VoiceId>,
    position: Vec3,
    /// The game side's (muffled) volume.
    channel_volume: f32,
    /// Its distance volume and pan gains, as last set.
    gain: [f32; 2],
    detail_at: Option<u64>,
}

/// A game-side handle on a started ambient.
struct Handle {
    name: String,
    position: Vec3,
    /// Its playing ambient, while in earshot.
    play: Option<u32>,
    volume: f32,
    occlusion: (u64, u64, f32),
}

/// What a script sees of a playing ambient.
pub struct Heard {
    pub name: String,
    pub stage: Stage,
    /// The clip playing now (none between parts).
    pub clip: Option<String>,
    pub gain: [f32; 2],
}

pub struct Audio {
    ambients: HashMap<String, AmbientDef>,
    groups: HashMap<String, Group>,
    clips: HashMap<String, Option<Arc<Clip>>>,
    dirs: Vec<PathBuf>,
    pub mixer: Arc<Mutex<Mixer>>,
    /// An output device drives the mixer (else each tick advances it).
    pub realtime: bool,
    playing: Vec<Playing>,
    handles: Vec<Option<Handle>>,
    next_id: u32,
    listener: (Vec3, Vec3),
    tick: u64,
    rng: u64,
}

fn f(j: &Value) -> f32 {
    j.as_f64().unwrap_or(0.0) as f32
}

fn pair(j: &Value) -> (f32, f32) {
    (f(&j[0]), f(&j[1]))
}

impl Audio {
    /// Nothing to play.
    pub fn empty() -> Audio {
        Audio {
            ambients: HashMap::new(),
            groups: HashMap::new(),
            clips: HashMap::new(),
            dirs: Vec::new(),
            mixer: Arc::new(Mutex::new(Mixer::new(44100.0))),
            realtime: false,
            playing: Vec::new(),
            handles: Vec::new(),
            next_id: 1,
            listener: (Vec3::ZERO, Vec3::Z),
            tick: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// The definitions in `common/sounds.json`; sounds from `common/audio`
    /// then the level's `audio`.
    pub fn load(common: &Path, level_dir: &Path) -> Audio {
        let mut a = Audio::empty();
        a.dirs = vec![common.join("audio"), level_dir.join("audio")];
        let Some(j) = std::fs::read_to_string(common.join("sounds.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return a;
        };
        let name = |v: &Value| v.as_str().map(str::to_string);
        for (k, d) in j["ambients"].as_object().into_iter().flatten() {
            let flags = d["flags"].as_array().into_iter().flatten().fold(0, |m, f| {
                m | match f.as_str() {
                    Some("interruptOnStop") => INTERRUPT_ON_STOP,
                    Some("playOnce") => PLAY_ONCE,
                    Some("pan") => PAN,
                    _ => 0,
                }
            });
            a.ambients.insert(
                k.clone(),
                AmbientDef {
                    flags,
                    sphere_radius: f(&d["sphereRadius"]),
                    detail_time: pair(&d["detailTime"]),
                    max_distance: f(&d["maxVolumeDistance"]),
                    min_distance: f(&d["minVolumeDistance"]),
                    detail: name(&d["detail"]),
                    base: name(&d["base1"]),
                    in_sound: name(&d["in"]),
                    out_sound: name(&d["out"]),
                    threshold: d["threshold"].as_u64().unwrap_or(3) as usize,
                    min_occlusion: f(&d["minOcclusion"]),
                },
            );
        }
        for (k, g) in j["groups"].as_object().into_iter().flatten() {
            a.groups.insert(
                k.clone(),
                Group {
                    volume: f(&g["volume"]),
                    pitch: f(&g["pitch"]),
                    prevent_repeats: g["preventRepeats"].as_bool().unwrap_or(false),
                    last: 0,
                    permutations: g["permutations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|p| Permutation {
                            weight: p["weight"].as_u64().unwrap_or(0) as u32,
                            volume: pair(&p["volume"]),
                            pitch: pair(&p["pitch"]),
                            sound: p["sound"].as_str().unwrap_or_default().to_string(),
                        })
                        .collect(),
                },
            );
        }
        a
    }

    /// A random number in [0, 1).
    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + self.random() * (hi - lo)
    }

    fn clip(&mut self, sound: &str) -> Option<Arc<Clip>> {
        if let Some(c) = self.clips.get(sound) {
            return c.clone();
        }
        let c = self.dirs.iter().find_map(|d| Clip::load(&d.join(format!("{sound}.wav")), sound)).map(Arc::new);
        self.clips.insert(sound.to_string(), c.clone());
        c
    }

    /// A clip of group `name`: a permutation picked by weight, its volume
    /// and pitch rolled.
    fn group_play(&mut self, name: &str, looping: bool, part: u8) -> Option<Play> {
        let g = self.groups.get(name)?;
        let n = g.permutations.len();
        if n == 0 {
            return None;
        }
        let total: u32 = g.permutations.iter().map(|p| p.weight).sum();
        let (weights, prevent, last): (Vec<u32>, bool, usize) =
            (g.permutations.iter().map(|p| p.weight).collect(), g.prevent_repeats, g.last);
        let mut pick = 0;
        if n > 1 {
            // (Once a repeat is drawn it draws on to the tenth try, keeping
            // whatever that gives — as shipped.)
            let mut done = true;
            for _ in 0..10 {
                let r = (self.random() * total as f32) as u32;
                let mut sum = 0;
                pick = weights
                    .iter()
                    .position(|w| {
                        sum += w;
                        r <= sum
                    })
                    .unwrap_or(n - 1);
                if prevent && pick == last {
                    done = false;
                }
                if done {
                    break;
                }
            }
        }
        let g = self.groups.get_mut(name)?;
        g.last = pick;
        let (gv, gp) = (g.volume, g.pitch);
        let p = &g.permutations[pick];
        let (vol, pitch, sound) = (p.volume, p.pitch, p.sound.clone());
        let volume = self.range(vol) * gv;
        let pitch = self.range(pitch) * gp;
        let clip = self.clip(&sound)?;
        Some(Play { clip, pitch, volume, looping, part })
    }

    /// The listener: the player's ear and facing.
    pub fn listen(&mut self, ear: Vec3, facing: Vec3) {
        self.listener = (ear, facing);
    }

    /// Distance volume and left / right gains of a sound at `p`.
    fn spatialize(&self, p: Vec3, max: f32, min: f32, pan: bool) -> [f32; 2] {
        let (ear, facing) = self.listener;
        let d = p.distance(ear) * FOOT_TO_DIST;
        let v = if d < max {
            1.0
        } else if min - max <= 0.0 {
            0.0
        } else {
            ((min - d) / (min - max)).clamp(0.0, 2.0)
        };
        if v <= 0.0 || !pan {
            return [v, v];
        }
        let flat = Vec3::new(p.x - ear.x, 0.0, p.z - ear.z);
        if flat.length_squared() <= NO_PAN_DISTANCE_SQUARED {
            return [v, v];
        }
        let to = p - ear;
        let dot = Vec3::new(-facing.z, facing.y, facing.x).dot(to);
        let theta = Vec3::new(facing.x, 0.0, facing.z).angle_between(flat).clamp(0.0, std::f32::consts::PI);
        let pan = 0.5 + ((theta - std::f32::consts::FRAC_PI_2) / std::f32::consts::PI).abs();
        if dot < 0.0 {
            [v, v * pan]
        } else if dot > 0.0 {
            [v * pan, v]
        } else {
            [v, v]
        }
    }

    /// Starts an ambient at `position` (the game side); `None`: unknown, or
    /// a play-once sound out of earshot.
    pub fn ambient_start(&mut self, name: &str, position: Vec3, sight: Option<&crate::physics::Sight>) -> Option<usize> {
        let def = self.ambients.get(name)?;
        let (min_occlusion, play_once) = (def.min_occlusion, def.flags & PLAY_ONCE != 0);
        let slot = self.handles.iter().position(Option::is_none).unwrap_or(self.handles.len());
        let mut h =
            Handle { name: name.to_string(), position, play: None, volume: 1.0, occlusion: (slot as u64 % OCCLUSION_INTERVAL, 0, 1.0) };
        match self.earshot(&mut h, min_occlusion, false, sight) {
            Some(volume) => {
                h.play = self.start(name, position, volume);
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

    /// Stops a started ambient: it plays out (its loop's pass, then its out
    /// sound) unless it interrupts on stop.
    pub fn ambient_stop(&mut self, handle: usize) {
        let Some(h) = self.handles.get_mut(handle).and_then(Option::take) else { return };
        if let Some(id) = h.play {
            self.stop(id);
        }
    }

    /// Whether `h` is in earshot, and its muffled volume.
    fn earshot(&self, h: &mut Handle, min_occlusion: f32, cached: bool, sight: Option<&crate::physics::Sight>) -> Option<f32> {
        let def = self.ambients.get(&h.name)?;
        let ear = self.listener.0;
        if h.position.distance(ear) * FOOT_TO_DIST > def.min_distance {
            return None;
        }
        let volume = if min_occlusion > 0.99 {
            1.0
        } else {
            let (offset, last, cached_volume) = h.occlusion;
            let due = last - last % OCCLUSION_INTERVAL + OCCLUSION_INTERVAL + offset;
            if cached && self.tick < due {
                cached_volume
            } else {
                let hits = sight.map_or(0, |s| s.hits(ear, h.position, MAX_COLLISIONS));
                h.occlusion.1 = self.tick;
                if hits > 0 {
                    min_occlusion.max(0.8 - (hits * hits) as f32 / 12.0)
                } else {
                    1.0
                }
            }
        };
        h.occlusion.2 = volume;
        Some(volume)
    }

    /// Starts a sound-system playing ambient (`None`: its threshold of
    /// copies is reached).
    fn start(&mut self, name: &str, position: Vec3, volume: f32) -> Option<u32> {
        let def = self.ambients.get(name)?;
        let copies = self.playing.iter().filter(|p| p.name == name && p.stage < Stage::Out).count();
        if copies >= def.threshold {
            return None;
        }
        let (in_sound, base, out_sound, looping) =
            (def.in_sound.clone(), def.base.clone(), def.out_sound.clone(), def.flags & PLAY_ONCE == 0);
        let id = self.next_id;
        self.next_id += 1;
        let mut p = Playing {
            id,
            name: name.to_string(),
            stage: Stage::In,
            voice: None,
            position,
            channel_volume: volume,
            gain: [0.0; 2],
            detail_at: None,
        };
        // In sound, then the body — queued, so they follow without a gap.
        let first = in_sound.as_deref().and_then(|g| self.group_play(g, false, IN));
        let body = base.as_deref().and_then(|g| self.group_play(g, looping, BODY));
        let (now, next) = match (first, body) {
            (Some(i), b) => (Some(i), b),
            (None, b) => (b, None),
        };
        let now = now.or_else(|| {
            // (Nothing to start with: an out sound alone plays at once.)
            out_sound.as_deref().and_then(|g| self.group_play(g, false, OUT))
        });
        if let Some(now) = now {
            let mut v = Voice::new(now, [0.0; 2]);
            v.next = next;
            p.voice = Some(self.mixer.lock().unwrap().play(v));
        }
        p.detail_at = self.detail_due(name);
        self.playing.push(p);
        let k = self.playing.len() - 1;
        self.update_one(k);
        Some(id)
    }

    /// When ambient `name`'s next detail sound is due.
    fn detail_due(&mut self, name: &str) -> Option<u64> {
        let def = &self.ambients[name];
        let t = def.detail_time;
        def.detail.is_some().then(|| self.tick + (self.range(t) * 60.0) as u64)
    }

    /// Stops playing ambient `id` (gracefully, unless it interrupts).
    fn stop(&mut self, id: u32) {
        let Some(k) = self.playing.iter().position(|p| p.id == id) else { return };
        if self.playing[k].stage >= Stage::Stopping {
            return;
        }
        let def = &self.ambients[&self.playing[k].name];
        let (interrupt, out_sound) = (def.flags & INTERRUPT_ON_STOP != 0, def.out_sound.clone());
        let out = out_sound.as_deref().and_then(|g| self.group_play(g, false, OUT));
        let p = &mut self.playing[k];
        p.stage = Stage::Stopping;
        let mut mixer = self.mixer.lock().unwrap();
        match p.voice.and_then(|v| mixer.get_mut(v)) {
            Some(v) if !interrupt => {
                // The loop's pass (or the in sound) ends, then the out sound.
                v.now.looping = false;
                v.next = out;
            }
            _ => {
                if let Some(v) = p.voice.take() {
                    mixer.stop(v);
                }
                if let Some(out) = out {
                    p.voice = Some(mixer.play(Voice::new(out, p.gain)));
                }
            }
        }
    }

    /// Halts playing ambient `id` at once (out of earshot): no out sound.
    fn halt(&mut self, id: u32) {
        if let Some(k) = self.playing.iter().position(|p| p.id == id) {
            let p = self.playing.remove(k);
            if let Some(v) = p.voice {
                self.mixer.lock().unwrap().stop(v);
            }
        }
    }

    /// One playing ambient's stage, detail track and gains.
    fn update_one(&mut self, k: usize) {
        let (part, alive, ends) = {
            let mut mixer = self.mixer.lock().unwrap();
            match self.playing[k].voice.and_then(|v| mixer.get_mut(v)) {
                Some(v) => (Some(v.now.part), true, v.now.part == BODY && !v.now.looping && v.next.is_none()),
                None => (None, false, false),
            }
        };
        if ends {
            // A body that won't loop (play once, or stopping) runs into the
            // out sound.
            let out = self.ambients[&self.playing[k].name].out_sound.clone();
            if let Some(play) = out.as_deref().and_then(|g| self.group_play(g, false, OUT)) {
                let mut mixer = self.mixer.lock().unwrap();
                if let Some(v) = self.playing[k].voice.and_then(|v| mixer.get_mut(v)) {
                    v.next = Some(play);
                }
            }
        }
        let p = &mut self.playing[k];
        p.stage = match (p.stage, part) {
            (_, None) if !alive => Stage::Done,
            (Stage::In, Some(BODY)) => Stage::Body,
            (Stage::Stopping | Stage::Body | Stage::In, Some(OUT)) => Stage::Out,
            (s, _) => s,
        };
        if p.stage == Stage::Done {
            return;
        }
        // The detail track: now and then a sound somewhere around it.
        if p.stage == Stage::Body && p.detail_at.is_some_and(|t| t <= self.tick) {
            let name = p.name.clone();
            let (radius, max, min, pan, detail) = {
                let d = &self.ambients[&name];
                (d.sphere_radius, d.max_distance, d.min_distance, d.flags & PAN != 0, d.detail.clone())
            };
            let at = self.playing[k].position
                + Vec3::new(self.range((-radius, radius)), 0.0, self.range((-radius, radius)));
            if let Some(play) = detail.as_deref().and_then(|g| self.group_play(g, false, DETAIL)) {
                let g = self.spatialize(at, max, min, pan);
                let cv = self.playing[k].channel_volume;
                self.mixer.lock().unwrap().play(Voice::new(play, [g[0] * cv, g[1] * cv]));
            }
            self.playing[k].detail_at = self.detail_due(&name);
        }
        let p = &self.playing[k];
        let d = &self.ambients[&p.name];
        let g = self.spatialize(p.position, d.max_distance, d.min_distance, d.flags & PAN != 0);
        let cv = p.channel_volume;
        let p = &mut self.playing[k];
        p.gain = g;
        if let Some(id) = p.voice {
            if let Some(v) = self.mixer.lock().unwrap().get_mut(id) {
                v.gain = [g[0] * cv, g[1] * cv];
            }
        }
    }

    /// A tick: without an output device the mixer first plays a tick's
    /// worth; handles follow their sounds in and out of earshot and muffle
    /// them; playing ambients step their stages.
    pub fn tick(&mut self, sight: Option<&crate::physics::Sight>) {
        self.tick += 1;
        if !self.realtime {
            let mut m = self.mixer.lock().unwrap();
            let frames = (m.rate / 60.0).round() as usize;
            m.mix(None, frames);
        }
        for slot in 0..self.handles.len() {
            let Some(mut h) = self.handles[slot].take() else { continue };
            let (min_occlusion, play_once) = {
                let d = &self.ambients[&h.name];
                (d.min_occlusion, d.flags & PLAY_ONCE != 0)
            };
            match self.earshot(&mut h, min_occlusion, true, sight) {
                None => {
                    if let Some(id) = h.play {
                        if !play_once {
                            self.halt(id);
                            h.play = None;
                        } else if !self.playing.iter().any(|p| p.id == id) {
                            continue;
                        }
                    }
                }
                Some(target) => match h.play {
                    Some(id) if self.playing.iter().any(|p| p.id == id) => {
                        let volume = h.volume + (target - h.volume).clamp(-VOLUME_ADJUST, VOLUME_ADJUST);
                        let volume = volume.clamp(0.0, 1.0);
                        if let Some(p) = self.playing.iter_mut().find(|p| p.id == id) {
                            p.channel_volume = volume;
                            p.position = h.position;
                        }
                        h.volume = volume;
                    }
                    Some(_) if play_once => continue,
                    _ => {
                        h.play = self.start(&h.name, h.position, target);
                        h.volume = target;
                    }
                },
            }
            self.handles[slot] = Some(h);
        }
        for k in 0..self.playing.len() {
            self.update_one(k);
        }
        self.playing.retain(|p| p.stage != Stage::Done);
    }

    /// The playing ambients (for scripts and tests).
    pub fn heard(&self) -> Vec<Heard> {
        let mut mixer = self.mixer.lock().unwrap();
        self.playing
            .iter()
            .map(|p| Heard {
                name: p.name.clone(),
                stage: p.stage,
                clip: p.voice.and_then(|v| mixer.get_mut(v)).map(|v| v.now.clip.name.clone()),
                gain: p.gain.map(|g| g * p.channel_volume),
            })
            .collect()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// An ambient with in / loop / out sounds (6, 12, 6 ticks long at 60
    /// Hz output), full volume to 10 ft, silent from 50.
    fn audio(flags: u32) -> Audio {
        let mut a = Audio::empty();
        a.mixer.lock().unwrap().rate = 60.0;
        for (name, frames) in [("i", 6), ("b", 12), ("o", 6)] {
            let clip = Clip { name: name.into(), rate: 60.0, channels: 1, samples: vec![0.1; frames] };
            a.clips.insert(name.into(), Some(Arc::new(clip)));
            let p = Permutation { weight: 10, volume: (1.0, 1.0), pitch: (1.0, 1.0), sound: name.into() };
            a.groups.insert(name.into(), Group { volume: 1.0, pitch: 1.0, prevent_repeats: false, last: 0, permutations: vec![p] });
        }
        a.ambients.insert(
            "hum".into(),
            AmbientDef {
                flags,
                sphere_radius: 0.0,
                detail_time: (1.0, 1.0),
                max_distance: 10.0,
                min_distance: 50.0,
                detail: None,
                base: Some("b".into()),
                in_sound: Some("i".into()),
                out_sound: Some("o".into()),
                threshold: 3,
                min_occlusion: 0.0,
            },
        );
        a.listen(Vec3::ZERO, Vec3::Z);
        a
    }

    fn stage(a: &Audio) -> Option<(Stage, String)> {
        a.heard().first().map(|h| (h.stage, h.clip.clone().unwrap_or_default()))
    }

    #[test]
    fn in_loop_out_and_done() {
        let mut a = audio(PAN);
        let h = a.ambient_start("hum", Vec3::new(0.0, 0.0, 30.0), None).unwrap();
        assert_eq!(stage(&a), Some((Stage::In, "i".into())));
        for _ in 0..8 {
            a.tick(None);
        }
        assert_eq!(stage(&a), Some((Stage::Body, "b".into())));
        for _ in 0..40 {
            a.tick(None);
        }
        assert_eq!(stage(&a), Some((Stage::Body, "b".into())), "it loops");
        a.ambient_stop(h);
        a.tick(None);
        assert_eq!(stage(&a).map(|s| s.0), Some(Stage::Stopping));
        // The loop finishes its pass, the out sound plays whole, then done.
        let mut seen: Vec<Option<(Stage, String)>> = Vec::new();
        for _ in 0..30 {
            a.tick(None);
            if seen.last() != Some(&stage(&a)) {
                seen.push(stage(&a));
            }
        }
        assert_eq!(seen, vec![Some((Stage::Stopping, "b".into())), Some((Stage::Out, "o".into())), None]);
    }

    #[test]
    fn heard_by_distance_halted_out_of_earshot_and_restarted() {
        let mut a = audio(0);
        // 30 units = 10 ft: full volume; 90 = 30 ft: halfway down.
        a.ambient_start("hum", Vec3::new(0.0, 0.0, 30.0), None).unwrap();
        a.tick(None);
        assert_eq!(a.heard()[0].gain, [1.0, 1.0]);
        a.listen(Vec3::new(0.0, 0.0, -60.0), Vec3::Z);
        a.tick(None);
        assert!((a.heard()[0].gain[0] - 0.5).abs() < 1e-4);
        // Beyond 50 ft (150 units): halted, no out sound.
        a.listen(Vec3::new(0.0, 0.0, -200.0), Vec3::Z);
        a.tick(None);
        assert!(a.heard().is_empty());
        // Back in earshot: it starts over, in sound first.
        a.listen(Vec3::ZERO, Vec3::Z);
        a.tick(None);
        assert_eq!(stage(&a), Some((Stage::In, "i".into())));
    }

    #[test]
    fn pans_toward_the_side_it_is_on() {
        let mut a = audio(PAN);
        // Facing +Z, right is -X: a sound off there is full right, half left.
        let g = a.spatialize(Vec3::new(-20.0, 0.0, 0.0), 10.0, 50.0, true);
        assert!((g[1] - 1.0).abs() < 1e-5 && (g[0] - 0.5).abs() < 1e-5, "{g:?}");
        // Straight ahead: no pan.
        let g = a.spatialize(Vec3::new(0.0, 0.0, 20.0), 10.0, 50.0, true);
        assert!((g[0] - g[1]).abs() < 1e-5, "{g:?}");
        // A threshold of 3 copies.
        for _ in 0..4 {
            a.ambient_start("hum", Vec3::new(0.0, 0.0, 10.0), None);
        }
        a.tick(None);
        assert_eq!(a.heard().len(), 3);
    }
}
