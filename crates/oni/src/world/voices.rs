//! Voices (
//! §3): what characters say — their speech, one line at a time with one
//! waiting, by priority — their grunts when hurt and cries when killed, and
//! their AI's vocalizations (alerts, taunts, super-move cries).

use glam::Vec3;

use super::World;

/// Speech priorities, lowest first (the original's full set).
#[allow(dead_code)]
pub mod speech {
    pub const NONE: u8 = 0;
    pub const IDLE: u8 = 1;
    pub const CHATTER: u8 = 2;
    pub const SAY: u8 = 3;
    pub const CALL: u8 = 4;
    pub const PAIN: u8 = 5;
    pub const YELL: u8 = 6;
    pub const OVERRIDE: u8 = 7;
}

/// The vocalization kinds (`oni_schema::VOCALIZATIONS` order).
pub mod vocal {
    pub const TAUNT: usize = 0;
    pub const ALERT: usize = 1;
    pub const STARTLE: usize = 2;
    pub const CHECK_BODY: usize = 3;
    pub const PURSUE: usize = 4;
    pub const COWER: usize = 5;
}

/// Each vocalization kind's speech priority.
const VOCAL_PRIORITY: [u8; 10] = [
    speech::CALL,
    speech::SAY,
    speech::YELL,
    speech::SAY,
    speech::SAY,
    speech::SAY,
    speech::OVERRIDE,
    speech::OVERRIDE,
    speech::OVERRIDE,
    speech::OVERRIDE,
];

/// A grunt waits this long before it is heard (ticks).
const PAIN_DELAY: u32 = 7;
/// A line whose length is unknown lasts this long (ticks).
const DEFAULT_DURATION: u64 = 120;

/// A line to say.
#[derive(Clone, Default)]
pub struct Line {
    pub sound: String,
    pub kind: u8,
    pub pre_pause: u32,
    pub post_pause: u32,
}

/// A character's speech: the line it is saying and one waiting.
#[derive(Default)]
pub struct Speech {
    pub current: Option<Line>,
    pub next: Option<Line>,
    played: bool,
    finished: bool,
    /// The line's ambient (an audio handle) and when it is over.
    handle: Option<usize>,
    end: u64,
}

impl Speech {
    pub fn saying(&self) -> Option<&str> {
        self.current.as_ref().map(|l| l.sound.as_str())
    }
}

/// A character's run of pain (`painState`).
#[derive(Default)]
pub struct PainState {
    total: u32,
    decay: u32,
    num_light: u32,
    num_medium: u32,
    last_kind: u32,
    last_time: u64,
    queue_timer: u32,
    queue_kind: u32,
    queue_volume: f32,
}

impl World {
    /// Fighter `who` says `line`: at once if it outranks what it says now
    /// (or overrides), else instead of a lower waiting line (unless
    /// `dont_queue`), else not at all. True unless dropped.
    pub fn say(&mut self, who: usize, line: Line, dont_queue: bool) -> bool {
        let cur = self.fighters[who]
            .speech
            .current
            .as_ref()
            .map_or(speech::NONE, |l| l.kind);
        let next = self.fighters[who]
            .speech
            .next
            .as_ref()
            .map_or(speech::NONE, |l| l.kind);
        if line.kind == speech::OVERRIDE || line.kind > cur {
            self.speech_stop(who, false);
            let s = &mut self.fighters[who].speech;
            s.current = Some(line);
            (s.played, s.finished) = (false, false);
            true
        } else if !dont_queue && line.kind > next {
            self.fighters[who].speech.next = Some(line);
            true
        } else {
            false
        }
    }

    /// Fighter `who` stops talking (`abort`: cut off, else the line plays
    /// out); both lines are dropped.
    pub fn speech_stop(&mut self, who: usize, abort: bool) {
        let s = &mut self.fighters[who].speech;
        if let Some(h) = s.handle.take() {
            if abort {
                self.audio.ambient_halt(h);
            } else {
                self.audio.ambient_stop(h);
            }
        }
        let s = &mut self.fighters[who].speech;
        (s.current, s.next, s.played, s.finished) = (None, None, false, false);
    }

    /// Fighter `who`'s speech this tick: the pause before, the line (from
    /// its eye, then following it), the pause after, then the waiting line.
    fn speech_tick(&mut self, who: usize) {
        let now = self.now as u64;
        let f = &mut self.fighters[who];
        let Some(line) = f.speech.current.as_mut() else {
            return;
        };
        if line.pre_pause > 0 {
            line.pre_pause -= 1;
            if line.pre_pause > 0 {
                return;
            }
        }
        let sound = line.sound.clone();
        if !f.speech.finished {
            if f.speech.played {
                let feet = f.feet();
                let alive = f.speech.handle.is_some_and(|h| self.audio.ambient_alive(h));
                let f = &mut self.fighters[who];
                if alive && now <= f.speech.end {
                    if let Some(h) = f.speech.handle {
                        // (From the second tick on, at its feet — as shipped.)
                        self.audio.ambient_move(h, feet);
                    }
                } else {
                    f.speech.handle = None;
                    f.speech.finished = true;
                }
            } else {
                let eye = f.feet() + Vec3::Y * f.pelvis_height() * 2.0;
                let eye = f
                    .bones()
                    .get(crate::fighter::part::HEAD)
                    .map_or(eye, |m| m.w_axis.truncate());
                let handle = self.audio.ambient_start(&sound, eye, Some(&self.quads));
                let duration = match self.audio.ambient_duration(&sound) {
                    0 => DEFAULT_DURATION,
                    d => u64::from(d),
                };
                if handle.is_some() {
                    eprintln!("say: fighter {who}: {sound}");
                }
                let f = &mut self.fighters[who];
                f.speech.handle = handle;
                f.speech.end = now + duration;
                f.speech.played = true;
                f.speech.finished = handle.is_none();
            }
        }
        let f = &mut self.fighters[who];
        if !f.speech.finished {
            return;
        }
        if let Some(line) = f.speech.current.as_mut() {
            if line.post_pause > 0 {
                line.post_pause -= 1;
                if line.post_pause > 0 {
                    return;
                }
            }
        }
        let s = &mut f.speech;
        (s.played, s.finished) = (false, false);
        s.current = s.next.take();
    }

    /// Fighter `who`'s AI says its line of kind `kind` — if its class rolls
    /// for it (unless `force`) and has one.
    pub fn vocalize(&mut self, who: usize, kind: usize, force: bool) -> bool {
        let f = &self.fighters[who];
        let Some((chance, sound)) = f.sounds.vocalizations.get(kind).cloned() else {
            return false;
        };
        // (Not its taunt line while it taunts over a body it checked.)
        if kind == vocal::TAUNT && f.brain.as_ref().is_some_and(|b| b.body_taunted()) {
            return false;
        }
        if !force && (self.audio.chance() * 100.0) as u8 >= chance {
            return false;
        }
        if sound.is_empty() {
            return false;
        }
        let kind = if force {
            speech::OVERRIDE
        } else {
            VOCAL_PRIORITY[kind]
        };
        self.say(
            who,
            Line {
                sound,
                kind,
                pre_pause: 0,
                post_pause: 0,
            },
            false,
        )
    }

    /// Fighter `who` is hurt for `damage` (not killed): maybe a grunt, by
    /// how its run of pain has gone.
    fn pain(&mut self, who: usize, damage: u32, least: u32) {
        let now = self.now as u64;
        let f = &self.fighters[who];
        let p = f.sounds.pain.clone();
        if damage == 0 {
            return;
        }
        let casual = f.brain.as_ref().is_some_and(|b| b.casual());
        if casual {
            // (A casual AI always cries out; its alert is said.)
            if let Some(c) = self.fighters[who].perception.as_mut() {
                c.said_alert = true;
            }
        } else if f.pain.total == 0 {
            // A run of pain starts: maybe ignored.
            let chance = if damage >= p.percentage_threshold {
                p.max_percentage
            } else {
                p.base_percentage
                    + damage * p.max_percentage.saturating_sub(p.base_percentage)
                        / p.percentage_threshold.max(1)
            };
            if self.audio.chance() * 100.0 >= chance as f32 {
                return;
            }
        }
        let s = &mut self.fighters[who].pain;
        s.total += damage;
        s.decay = p.timer;
        if s.last_time + u64::from(p.min_timer) > now {
            return;
        }
        let mut kind = least.max(s.last_kind);
        if kind == 0 && (s.num_light >= p.max_light || s.total >= p.medium_threshold) {
            kind = 1;
        }
        if kind == 1 && (s.num_medium >= p.max_medium || s.total >= p.heavy_threshold) {
            kind = 2;
        }
        let volume = if s.total < p.volume_threshold {
            p.min_volume + s.total as f32 * (1.0 - p.min_volume) / p.volume_threshold.max(1) as f32
        } else {
            1.0
        };
        if s.queue_timer > 0 {
            s.queue_kind = s.queue_kind.max(kind);
        } else {
            (s.queue_timer, s.queue_kind, s.queue_volume) = (PAIN_DELAY, kind, volume);
        }
    }

    /// Fighter `who` killed: mid-scream it screams, else maybe its death
    /// cry.
    fn death_cry(&mut self, who: usize) {
        let f = &self.fighters[who];
        if f.pain.num_medium > 0 {
            self.grunt(who, 2, None);
        } else if self.audio.chance() * 100.0 < f.sounds.pain.death_chance as f32 {
            self.grunt(who, 3, None);
        }
    }

    /// Plays grunt `kind` (light, medium, heavy, death) from fighter `who`'s
    /// eye, cutting off what it was saying.
    fn grunt(&mut self, who: usize, kind: usize, volume: Option<f32>) {
        let name = self.fighters[who].sounds.pain.sounds[kind].clone();
        if name.is_empty() {
            return;
        }
        self.speech_stop(who, true);
        let f = &self.fighters[who];
        let eye = f
            .bones()
            .get(crate::fighter::part::HEAD)
            .map_or(f.feet(), |m| m.w_axis.truncate());
        self.audio
            .impulse_play(&name, Some(eye), volume, Some(&self.quads));
    }

    /// Fighter `who`'s run of pain this tick: it fades, and a waiting grunt
    /// is heard.
    fn pain_tick(&mut self, who: usize) {
        let now = self.now as u64;
        let min_timer = u64::from(self.fighters[who].sounds.pain.min_timer);
        let s = &mut self.fighters[who].pain;
        if s.decay > 0 {
            s.decay -= 1;
            if s.decay == 0 {
                *s = PainState::default();
            }
        }
        let s = &mut self.fighters[who].pain;
        if s.queue_timer > 0 {
            s.queue_timer -= 1;
            if s.queue_timer == 0 {
                let (kind, volume) = (s.queue_kind, s.queue_volume);
                self.grunt(who, kind as usize, Some(volume));
                let s = &mut self.fighters[who].pain;
                match kind {
                    0 => (s.num_light, s.last_kind, s.last_time) = (s.num_light + 1, 0, now),
                    1 => {
                        (s.num_medium, s.last_kind, s.last_time) =
                            (s.num_medium + 1, 1, now + min_timer / 2)
                    }
                    _ => {
                        *s = PainState::default();
                        (s.last_kind, s.last_time) = (2, now + min_timer);
                    }
                }
                let s = &mut self.fighters[who].pain;
                (s.queue_kind, s.queue_volume) = (0, 0.0);
            }
        }
    }

    /// Everyone's voices this tick: clips started (their vocalizations),
    /// hurts and deaths, the AI's lines, pain and speech.
    pub(super) fn voices_tick(&mut self) {
        for who in 0..self.fighters.len() {
            if let Some(clip) = self.fighters[who].started.take() {
                if let Some(kind) = self.fighters[who].motion.clip_of(clip).vocalization {
                    self.vocalize(who, kind, false);
                }
            }
            for (damage, least, killed) in std::mem::take(&mut self.fighters[who].hurts) {
                if killed {
                    self.death_cry(who);
                } else {
                    self.pain(who, damage, least);
                }
            }
            let asked: Vec<usize> = self.fighters[who]
                .perception
                .as_mut()
                .map(|p| p.vocal_tick())
                .unwrap_or_default();
            for kind in asked {
                self.vocalize(who, kind, false);
            }
            self.pain_tick(who);
            if self.fighters[who].motion.dying || !self.fighters[who].alive() {
                // (The dead say nothing more.)
                if self.fighters[who].speech.current.is_some() {
                    self.speech_stop(who, false);
                }
                continue;
            }
            self.speech_tick(who);
        }
    }
}
