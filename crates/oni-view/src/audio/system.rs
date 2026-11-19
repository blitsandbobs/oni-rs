//! The sound system (`docs/sound/ORIGINAL_CORE.md` §3, §5, §6, §8, §9): the
//! 16 channels and how a sound gets one (the original's four passes), group
//! plays, impulses, and the playing ambients' stages.

use glam::Vec3;

use super::defs::{INTERRUPT_ON_STOP, PAN, PLAY_ONCE};
use super::mixer::{Play, Side, Voice, CHANNELS, STEREO};
use super::Audio;

/// What every channel's priority stays at: the original never sets it.
const CHANNEL_PRIORITY: u8 = 1;
pub(super) const LOW: u8 = 0;
/// Playing ambients at once.
const AMBIENT_SLOTS: usize = 16;
/// World units → the definitions' distances ("feet").
const FOOT_TO_DIST: f32 = 1.0 / 3.0;
/// Closer than this (squared, world units, flat) a sound isn't panned.
const NO_PAN_DISTANCE_SQUARED: f32 = 49.0;

pub(super) const IN: u8 = 0;
pub(super) const BODY: u8 = 1;
pub(super) const OUT: u8 = 2;
pub(super) const ONE_SHOT: u8 = 3;

/// The simulation's side of a channel.
#[derive(Clone, Default)]
pub(super) struct Chan {
    /// Held by a playing ambient (between its clips too).
    pub locked: bool,
    pub owner: Option<u32>,
    /// The group last played on it.
    pub group: Option<String>,
    pub channel_volume: f32,
    pub distance_volume: f32,
    pub can_pan: bool,
    pub pan: (Side, f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    In,
    Body,
    Stopping,
    Out,
    /// No channel (or only a detail track): details, forever.
    DetailOnly,
    Done,
}

/// A playing ambient.
pub(super) struct Playing {
    pub id: u32,
    pub name: String,
    pub stage: Stage,
    pub ch1: Option<usize>,
    pub ch2: Option<usize>,
    /// `None`: not placed (music, script ambients).
    pub position: Option<Vec3>,
    pub channel_volume: f32,
    /// Its channels take `channel_volume` at the next update.
    pub volume_set: bool,
    pub detail_at: Option<u64>,
    /// Base 2 still loops.
    pub ch2_looping: bool,
}

fn pool(stereo: bool) -> std::ops::Range<usize> {
    if stereo {
        0..STEREO
    } else {
        STEREO..CHANNELS
    }
}

impl Audio {
    /// A free channel for a sound, by the original's passes: a free one; for
    /// a higher priority, the first unlocked one; one near silent; the
    /// quietest (an ambient's too — it loses it). `None`: the sound is
    /// dropped.
    pub(super) fn acquire(&mut self, stereo: bool, priority: u8) -> Option<usize> {
        let (playing, volume): (Vec<bool>, Vec<f32>) = {
            let m = self.mixer.lock().unwrap();
            (0..CHANNELS).map(|c| (m.voice(c).is_some(), m.voice(c).map_or(0.0, Voice::volume))).unzip()
        };
        let range = pool(stereo);
        let free = |c: &usize| !self.chans[*c].locked;
        let found = range
            .clone()
            .find(|c| free(c) && !playing[*c])
            .or_else(|| (CHANNEL_PRIORITY < priority).then(|| range.clone().find(free)).flatten())
            .or_else(|| (CHANNEL_PRIORITY <= priority).then(|| range.clone().find(|c| free(c) && volume[*c] <= 0.1)).flatten())
            .or_else(|| {
                (CHANNEL_PRIORITY <= priority)
                    .then(|| range.clone().filter(|c| volume[*c] < 1.0).min_by(|a, b| volume[*a].total_cmp(&volume[*b])))
                    .flatten()
            })?;
        if self.chans[found].owner.is_some() {
            self.stall(found);
        }
        self.mixer.lock().unwrap().stop(found);
        self.chans[found] = Chan::default();
        Some(found)
    }

    /// A playing ambient loses channel `c` to another sound.
    fn stall(&mut self, c: usize) {
        for p in &mut self.playing {
            if p.ch1 == Some(c) {
                p.ch1 = None;
            }
            if p.ch2 == Some(c) {
                p.ch2 = None;
            }
        }
        self.release(c);
    }

    /// Stops channel `c` and frees it.
    pub(super) fn release(&mut self, c: usize) {
        self.mixer.lock().unwrap().stop(c);
        self.chans[c] = Chan::default();
    }

    /// Channel `c`'s level: overall × channel × distance volume.
    pub(super) fn level(&self, c: usize) -> f32 {
        let ch = &self.chans[c];
        self.overall * ch.channel_volume.clamp(0.0, 2.0) * ch.distance_volume.clamp(0.0, 2.0)
    }

    /// Distance volume and pan of a sound at `at` (`None`: not placed — full,
    /// centred).
    pub(super) fn spatialize(&self, at: Option<Vec3>, max: f32, min: f32, can_pan: bool) -> (f32, (Side, f32)) {
        let Some(p) = at else { return (1.0, (Side::Centre, 1.0)) };
        let (ear, facing) = self.listener;
        let d = p.distance(ear) * FOOT_TO_DIST;
        let v = if d < max {
            1.0
        } else if min - max <= 0.0 {
            0.0
        } else {
            ((min - d) / (min - max)).clamp(0.0, 2.0)
        };
        if v <= 0.0 || !can_pan {
            return (v, (Side::Centre, 1.0));
        }
        let flat = Vec3::new(p.x - ear.x, 0.0, p.z - ear.z);
        if flat.length_squared() <= NO_PAN_DISTANCE_SQUARED {
            return (v, (Side::Centre, 1.0));
        }
        let dot = Vec3::new(-facing.z, facing.y, facing.x).dot(p - ear);
        let theta = Vec3::new(facing.x, 0.0, facing.z).angle_between(flat).clamp(0.0, std::f32::consts::PI);
        let pan = 0.5 + ((theta - std::f32::consts::FRAC_PI_2) / std::f32::consts::PI).abs();
        let side = if dot < 0.0 {
            Side::Left
        } else if dot > 0.0 {
            Side::Right
        } else {
            return (v, (Side::Centre, 1.0));
        };
        (v, (side, pan))
    }

    /// A clip of group `name`: a permutation picked by weight, its volume
    /// and pitch rolled.
    pub(super) fn roll(&mut self, name: &str, looping: bool, part: u8) -> Option<Play> {
        let g = self.defs.groups.get(name)?;
        let n = g.permutations.len();
        if n == 0 {
            return None;
        }
        let weights: Vec<u32> = g.permutations.iter().map(|p| p.weight).collect();
        let (total, prevent, last) = (weights.iter().sum::<u32>(), g.prevent_repeats, g.last);
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
        let g = self.defs.groups.get_mut(name)?;
        g.last = pick;
        let (gv, gp) = (g.volume, g.pitch);
        let p = &g.permutations[pick];
        let (vol, pitch, sound) = (p.volume, p.pitch, p.sound.clone());
        let volume = self.range(vol) * gv;
        let pitch = self.range(pitch) * gp;
        let clip = self.clip(&sound)?;
        Some(Play { clip, pitch, volume, looping, part })
    }

    /// Starts `play` (then `next`) on channel `c`, of group `group`.
    fn play_on(&mut self, c: usize, group: &str, play: Play, next: Option<Play>) {
        self.chans[c].group = Some(group.to_string());
        let (level, pan) = (self.level(c), self.chans[c].pan);
        let mut v = Voice::new(play, level, pan);
        v.next = next;
        self.mixer.lock().unwrap().start(c, v);
    }

    /// Plays one clip of group `name` at `at` (spatialized once), on a
    /// channel got at `priority`; `None`: no channel, or out of earshot (the
    /// channel was taken all the same — as shipped).
    pub(super) fn play_spatialized(
        &mut self,
        name: &str,
        max: f32,
        min: f32,
        at: Option<Vec3>,
        volume: f32,
        priority: u8,
    ) -> Option<usize> {
        let play = self.roll(name, false, ONE_SHOT)?;
        let c = self.acquire(play.clip.stereo(), priority)?;
        let (dv, pan) = self.spatialize(at, max, min, true);
        if dv <= 0.0 {
            return None;
        }
        self.chans[c] =
            Chan { locked: false, owner: None, group: None, channel_volume: volume.clamp(0.0, 2.0), distance_volume: dv, can_pan: true, pan };
        self.play_on(c, name, play, None);
        Some(c)
    }

    /// The sound system's side of an impulse: past its alt threshold, its
    /// alt impulse instead (keeping this one's volume).
    pub(super) fn impulse_system(&mut self, name: &str, at: Option<Vec3>, volume: f32, depth: u8) -> Option<usize> {
        let imp = self.defs.impulses.get(name)?.clone();
        if let Some((threshold, alt)) = &imp.alt {
            let stereo = self.defs.groups.get(&imp.group).is_some_and(|g| g.channels == 2);
            let busy = {
                let m = self.mixer.lock().unwrap();
                pool(stereo).filter(|c| m.voice(*c).is_some() && self.chans[*c].group.as_deref() == Some(&imp.group)).count()
            };
            if busy >= *threshold {
                return if depth < 8 { self.impulse_system(alt, at, volume, depth + 1) } else { None };
            }
        }
        self.play_spatialized(&imp.group, imp.max_distance, imp.min_distance, at, volume, imp.priority)
    }

    /// Reserves (and locks) a channel for ambient `id`'s group `group`.
    fn reserve(&mut self, id: u32, group: &str, priority: u8, can_pan: bool, volume: f32) -> Option<usize> {
        let stereo = self.defs.groups.get(group).is_some_and(|g| g.channels == 2);
        let c = self.acquire(stereo, priority)?;
        self.chans[c] = Chan {
            locked: true,
            owner: Some(id),
            group: None,
            channel_volume: volume,
            distance_volume: 1.0,
            can_pan,
            pan: (Side::Centre, 1.0),
        };
        Some(c)
    }

    /// Starts a playing ambient (`None`: its threshold of copies, or all 16
    /// slots, taken).
    pub(super) fn start(&mut self, name: &str, position: Option<Vec3>, volume: f32) -> Option<u32> {
        let def = self.defs.ambients.get(name)?;
        let copies = self.playing.iter().filter(|p| p.name == name && p.stage < Stage::Out).count();
        if copies >= def.threshold || self.playing.len() >= AMBIENT_SLOTS {
            return None;
        }
        let first = def.base.clone().or_else(|| def.in_sound.clone()).or_else(|| def.out_sound.clone());
        let (base2, detail, priority, can_pan) =
            (def.base2.clone(), def.detail.is_some(), def.priority, def.flags & PAN != 0);
        let id = self.next_id;
        self.next_id += 1;
        let ch1 = first.and_then(|g| self.reserve(id, &g, priority, can_pan, volume));
        let ch2 = base2.and_then(|g| self.reserve(id, &g, priority, can_pan, volume));
        let mut p = Playing {
            id,
            name: name.to_string(),
            stage: Stage::In,
            ch1,
            ch2,
            position,
            channel_volume: volume,
            volume_set: false,
            detail_at: None,
            ch2_looping: false,
        };
        if ch1.is_none() && ch2.is_none() {
            // (Detail-only ambients, or no channel to be had: details, the
            // first at once.)
            if detail {
                p.stage = Stage::DetailOnly;
                p.detail_at = Some(0);
            } else {
                p.stage = Stage::Done;
            }
        }
        self.playing.push(p);
        let k = self.playing.len() - 1;
        if self.playing[k].stage == Stage::In {
            let def = &self.defs.ambients[name];
            let (in_sound, base, looping) = (def.in_sound.clone(), def.base.clone(), def.flags & PLAY_ONCE == 0);
            match (in_sound, ch1) {
                (Some(g), Some(c)) => {
                    // In sound, then the body queued behind it.
                    if let Some(play) = self.roll(&g, false, IN) {
                        let next = base.as_deref().and_then(|b| self.roll(b, looping, BODY));
                        self.play_on(c, &g, play, next);
                    }
                }
                _ => self.body_start(k),
            }
        }
        self.update_one(k);
        Some(id)
    }

    /// Playing ambient `k`'s body starts: base 1 (unless already queued in
    /// after the in sound) and base 2, looping unless played once.
    fn body_start(&mut self, k: usize) {
        let p = &self.playing[k];
        let def = &self.defs.ambients[&p.name];
        let (base, base2, looping, name) = (def.base.clone(), def.base2.clone(), def.flags & PLAY_ONCE == 0, p.name.clone());
        let (ch1, ch2) = (p.ch1, p.ch2);
        if let (Some(g), Some(c)) = (base, ch1) {
            let playing_body = self.mixer.lock().unwrap().voice(c).is_some_and(|v| v.now.part == BODY);
            if !playing_body {
                if let Some(play) = self.roll(&g, looping, BODY) {
                    self.play_on(c, &g, play, None);
                }
            }
        }
        if let (Some(g), Some(c)) = (base2, ch2) {
            if let Some(play) = self.roll(&g, looping, BODY) {
                self.play_on(c, &g, play, None);
                self.playing[k].ch2_looping = looping;
            }
        }
        let due = self.detail_due(&name);
        let p = &mut self.playing[k];
        p.detail_at = due;
        p.stage = if p.ch1.is_some() || p.ch2.is_some() { Stage::Body } else { Stage::Stopping };
    }

    /// When ambient `name`'s next detail sound is due.
    pub(super) fn detail_due(&mut self, name: &str) -> Option<u64> {
        let def = &self.defs.ambients[name];
        let t = def.detail_time;
        def.detail.is_some().then(|| self.tick + (self.range(t) * 60.0) as u64)
    }

    /// Plays playing ambient `k`'s detail: somewhere around it (or the
    /// listener), always panned, only on a free channel.
    fn detail(&mut self, k: usize) {
        let p = &self.playing[k];
        let def = &self.defs.ambients[&p.name];
        let (r, max, min, detail, name) = (def.sphere_radius, def.max_distance, def.min_distance, def.detail.clone(), p.name.clone());
        let base = p.position.unwrap_or(self.listener.0);
        let volume = p.channel_volume;
        let at = base + Vec3::new(self.range((-r, r)), 0.0, self.range((-r, r)));
        if let Some(g) = detail {
            self.play_spatialized(&g, max, min, Some(at), volume, LOW);
        }
        self.playing[k].detail_at = self.detail_due(&name);
    }

    /// Stops playing ambient `id`: it plays out (its loop's pass, then its
    /// out sound) unless it interrupts on stop.
    pub(super) fn stop(&mut self, id: u32) {
        let Some(k) = self.playing.iter().position(|p| p.id == id) else { return };
        let p = &self.playing[k];
        if matches!(p.stage, Stage::Stopping | Stage::Out | Stage::Done) {
            return;
        }
        let def = &self.defs.ambients[&p.name];
        let (interrupt, out) = (def.flags & INTERRUPT_ON_STOP != 0, def.out_sound.clone());
        let (ch1, ch2) = (p.ch1, p.ch2);
        if interrupt {
            let mut m = self.mixer.lock().unwrap();
            for c in [ch1, ch2].into_iter().flatten() {
                m.stop(c);
            }
        } else {
            let ch2_playing = ch2.is_some_and(|c| self.mixer.lock().unwrap().voice(c).is_some());
            if let (true, Some(c)) = (ch2_playing, ch2) {
                // (Only base 2 stops looping; the stage follows when it
                // ends — as shipped.)
                if let Some(v) = self.mixer.lock().unwrap().voice_mut(c) {
                    v.now.looping = false;
                    v.next = None;
                }
                self.playing[k].ch2_looping = false;
                return;
            }
            if let Some(c) = ch1 {
                let out = out.as_deref().and_then(|g| self.roll(g, false, OUT));
                if let Some(v) = self.mixer.lock().unwrap().voice_mut(c) {
                    v.now.looping = false;
                    v.next = out;
                }
            }
        }
        self.playing[k].stage = Stage::Stopping;
    }

    /// Halts playing ambient `id` at once: no out sound.
    pub(super) fn halt(&mut self, id: u32) {
        if let Some(k) = self.playing.iter().position(|p| p.id == id) {
            let p = self.playing.remove(k);
            for c in [p.ch1, p.ch2].into_iter().flatten() {
                self.release(c);
            }
        }
    }

    fn voice_state(&self, c: Option<usize>) -> (bool, Option<u8>, bool, bool) {
        let m = self.mixer.lock().unwrap();
        match c.and_then(|c| m.voice(c)) {
            Some(v) => (true, Some(v.now.part), v.now.looping, v.next.is_some()),
            None => (false, None, false, false),
        }
    }

    /// One playing ambient's update: its stages, volume, place and pan.
    pub(super) fn update_one(&mut self, k: usize) {
        for _ in 0..6 {
            if !self.step_stage(k) {
                break;
            }
        }
        let p = &self.playing[k];
        if p.stage == Stage::Done {
            for c in [p.ch1, p.ch2].into_iter().flatten() {
                self.release(c);
            }
            let p = &mut self.playing[k];
            (p.ch1, p.ch2) = (None, None);
            return;
        }
        let def = &self.defs.ambients[&p.name];
        let (max, min) = (def.max_distance, def.min_distance);
        let (ch1, ch2, at, cv, set) = (p.ch1, p.ch2, p.position, p.channel_volume, p.volume_set);
        for c in [ch1, ch2].into_iter().flatten() {
            if set {
                self.chans[c].channel_volume = cv;
            }
            let (dv, pan) = self.spatialize(at, max, min, self.chans[c].can_pan);
            self.chans[c].distance_volume = dv;
            self.chans[c].pan = pan;
        }
        self.playing[k].volume_set = false;
    }

    /// One step of playing ambient `k`'s stages; whether it moved on.
    fn step_stage(&mut self, k: usize) -> bool {
        let p = &self.playing[k];
        let def = &self.defs.ambients[&p.name];
        let (base, base2, out, play_once, has_detail) =
            (def.base.clone(), def.base2.clone(), def.out_sound.clone(), def.flags & PLAY_ONCE != 0, def.detail.is_some());
        let (ch1, ch2, stage, name, id) = (p.ch1, p.ch2, p.stage, p.name.clone(), p.id);
        let (p1, part1, loop1, _) = self.voice_state(ch1);
        let (p2, _, _, _) = self.voice_state(ch2);
        match stage {
            Stage::In => {
                if ch1.is_none() || !p1 || part1 == Some(BODY) {
                    self.body_start(k);
                    return true;
                }
            }
            Stage::Body => {
                if base.is_some() && ch1.is_none() {
                    // Its channel was taken: another is reserved at no
                    // volume, plays one pass, and the ambient ends — as
                    // shipped (the handle restarts a placed one).
                    let def = &self.defs.ambients[&name];
                    let (priority, can_pan) = (def.priority, def.flags & PAN != 0);
                    if let Some(g) = &base {
                        if let Some(c) = self.reserve(id, g, priority, can_pan, 0.0) {
                            self.playing[k].ch1 = Some(c);
                            if let Some(play) = self.roll(g, false, BODY) {
                                self.play_on(c, g, play, None);
                            }
                        }
                    }
                    if !p2 {
                        self.playing[k].stage = Stage::Stopping;
                        return true;
                    }
                    return false;
                }
                if let (Some(g), Some(c), false, false) = (&base, ch1, p1, play_once) {
                    if let Some(play) = self.roll(g, true, BODY) {
                        self.play_on(c, g, play, None);
                    }
                }
                if let (Some(g), Some(c), false, false) = (&base2, ch2, p2, play_once) {
                    if self.playing[k].ch2_looping {
                        if let Some(play) = self.roll(g, true, BODY) {
                            self.play_on(c, g, play, None);
                        }
                    } else if loop1 {
                        // Base 2 has played out: base 1 stops looping too.
                        if let Some(c) = ch1 {
                            if let Some(v) = self.mixer.lock().unwrap().voice_mut(c) {
                                v.now.looping = false;
                                v.next = None;
                            }
                        }
                        self.playing[k].stage = Stage::Stopping;
                        return true;
                    }
                }
                let (p1, part1, loop1, next1) = self.voice_state(ch1);
                if !p1 && !self.voice_state(ch2).0 {
                    self.playing[k].stage = Stage::Stopping;
                    return true;
                }
                if part1 == Some(OUT) {
                    self.playing[k].stage = Stage::Out;
                    if let Some(c) = self.playing[k].ch2.take() {
                        self.release(c);
                    }
                    return true;
                }
                // Keep the next pass rolled (a fresh permutation each pass),
                // or — played once — the out sound queued behind the body.
                if let (Some(c), true) = (ch1, part1 == Some(BODY) && !next1) {
                    let next = match (loop1, &base, &out) {
                        (true, Some(g), _) => self.roll(g, true, BODY),
                        (false, _, Some(o)) if !self.voice_state(ch2).0 => self.roll(o, false, OUT),
                        _ => None,
                    };
                    if let (Some(n), Some(v)) = (next, self.mixer.lock().unwrap().voice_mut(c)) {
                        v.next = Some(n);
                    }
                }
                if self.playing[k].ch2_looping {
                    if let Some(c) = ch2 {
                        let need = self.mixer.lock().unwrap().voice(c).is_some_and(|v| v.next.is_none() && v.now.looping);
                        if need {
                            let n = base2.as_deref().and_then(|g| self.roll(g, true, BODY));
                            if let Some(v) = self.mixer.lock().unwrap().voice_mut(c) {
                                v.next = n;
                            }
                        }
                    }
                }
                if self.playing[k].detail_at.is_some_and(|t| t <= self.tick) && !self.script_only {
                    self.detail(k);
                }
            }
            Stage::Stopping => {
                for c in [ch1, ch2].into_iter().flatten() {
                    if let Some(v) = self.mixer.lock().unwrap().voice_mut(c) {
                        if v.now.looping {
                            v.now.looping = false;
                            if v.next.as_ref().is_some_and(|n| n.part != OUT) {
                                v.next = None;
                            }
                        }
                    }
                }
                let (p1, part1, _, next1) = self.voice_state(ch1);
                if part1 == Some(OUT) {
                    self.playing[k].stage = Stage::Out;
                    if let Some(c) = self.playing[k].ch2.take() {
                        self.release(c);
                    }
                    return true;
                }
                if !p1 && !p2 {
                    // The out sound.
                    if let Some(c) = self.playing[k].ch2.take() {
                        self.release(c);
                    }
                    if let (Some(c), Some(o)) = (ch1, &out) {
                        if let Some(play) = self.roll(o, false, OUT) {
                            self.play_on(c, o, play, None);
                        }
                    }
                    self.playing[k].stage = Stage::Out;
                    return true;
                }
                if let (Some(c), true, false, Some(o)) = (ch1, p1 && !p2, next1, &out) {
                    // (Queued, so it follows without a gap.)
                    let play = self.roll(o, false, OUT);
                    if let Some(v) = self.mixer.lock().unwrap().voice_mut(c) {
                        v.next = play;
                    }
                }
            }
            Stage::Out => {
                if !p1 {
                    self.playing[k].stage = Stage::Done;
                    return true;
                }
            }
            Stage::DetailOnly => {
                if has_detail && self.playing[k].detail_at.is_some_and(|t| t <= self.tick) {
                    self.detail(k);
                }
            }
            Stage::Done => {}
        }
        false
    }
}
