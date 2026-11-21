//! The mixer: the original's 16 sound channels (6 stereo, then 10 mono),
//! each playing a decoded clip with a queued next one (so an ambient's
//! passes and stages follow without a gap). The simulation decides
//! everything — what plays where, levels, pans; the mixer only renders, turning
//! the original's linear volumes into output gains with the PC (DirectSound)
//! law or, as an option, the Mac's. The output device's callback drives it in
//! play mode; headless runs advance it a tick's worth at a time.

use std::path::Path;
use std::sync::Arc;

/// Channels in all, and how many of them (the first) are stereo.
pub const CHANNELS: usize = 16;
pub const STEREO: usize = 6;

/// A decoded sound: 16-bit PCM read into floats, interleaved.
pub struct Clip {
    pub name: String,
    pub rate: f32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl Clip {
    /// Reads a PCM WAV (the converter writes 16-bit mono or stereo).
    pub fn load(path: &Path, name: &str) -> Option<Clip> {
        Clip::parse(&std::fs::read(path).ok()?, name)
    }

    pub fn parse(b: &[u8], name: &str) -> Option<Clip> {
        if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return None;
        }
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let (mut at, mut fmt, mut data) = (12, None, None);
        while at + 8 <= b.len() {
            let len = u32_at(at + 4) as usize;
            let body = at + 8..(at + 8 + len).min(b.len());
            match &b[at..at + 4] {
                b"fmt " if len >= 16 => {
                    fmt = Some((
                        u16_at(at + 8),
                        u16_at(at + 10),
                        u32_at(at + 12),
                        u16_at(at + 22),
                    ))
                }
                b"data" => data = Some(body),
                _ => {}
            }
            at += 8 + len + (len & 1);
        }
        let (format, channels, rate, bits) = fmt?;
        if format != 1 || bits != 16 || !(1..=2).contains(&channels) {
            return None;
        }
        let samples = b[data?]
            .chunks_exact(2)
            .map(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0)
            .collect();
        Some(Clip {
            name: name.to_string(),
            rate: rate as f32,
            channels: channels as usize,
            samples,
        })
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    pub fn stereo(&self) -> bool {
        self.channels == 2
    }

    fn frame(&self, i: usize) -> [f32; 2] {
        let k = i * self.channels;
        if self.channels == 2 {
            [self.samples[k], self.samples[k + 1]]
        } else {
            [self.samples[k]; 2]
        }
    }
}

/// How a linear volume becomes an output gain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Law {
    /// The PC build (DirectSound): v ↦ −33.3·(1−v) dB, silent at 0.05 or
    /// below; the far ear loses −33.3·(1−p) dB.
    #[default]
    Pc,
    /// The Mac build: gains are the volumes; the far ear is × p.
    Mac,
}

/// Which side a panned sound is on.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Side {
    #[default]
    Centre,
    Left,
    Right,
}

/// Hundredths of a dB (DirectSound's unit) → amplitude.
fn centibels(c: i32) -> f32 {
    10f32.powf(c as f32 / 2000.0)
}

impl Law {
    /// Left / right output gains for a composed volume `v` and pan.
    pub fn gains(self, v: f32, side: Side, p: f32) -> [f32; 2] {
        let (g, far) = match self {
            Law::Pc => {
                // (Above 1 DirectSound refuses the value: taken as 1.)
                let g = if v <= 0.05 {
                    0.0
                } else {
                    centibels((3333.0 * v.min(1.0)) as i32 - 3333)
                };
                let far = if p <= 0.05 {
                    0.0
                } else {
                    centibels((3333.0 * p.min(1.0)) as i32 - 3333)
                };
                (g, far)
            }
            Law::Mac => (v.max(0.0), p),
        };
        match side {
            Side::Centre => [g, g],
            Side::Left => [g, g * far],
            Side::Right => [g * far, g],
        }
    }
}

/// One clip to play on a channel.
#[derive(Clone)]
pub struct Play {
    pub clip: Arc<Clip>,
    /// Playback speed (the permutation's pitch × the group's).
    pub pitch: f32,
    /// The permutation's volume × the group's.
    pub volume: f32,
    /// Played again when it ends with nothing queued.
    pub looping: bool,
    /// What it is to its owner (e.g. an ambient's in sound, loop, out sound).
    pub part: u8,
}

pub struct Voice {
    pub now: Play,
    pub next: Option<Play>,
    /// The channel's volume without the clip's own: overall × channel ×
    /// distance.
    pub level: f32,
    pub pan: (Side, f32),
    /// The gains being heard (ramping toward the target to avoid clicks).
    heard: Option<[f32; 2]>,
    pos: f64,
    /// Paused (`SSrPlayingChannels_Pause`): heard no more, kept where it is.
    pub paused: bool,
}

impl Voice {
    pub fn new(now: Play, level: f32, pan: (Side, f32)) -> Voice {
        Voice {
            now,
            next: None,
            level,
            pan,
            heard: None,
            pos: 0.0,
            paused: false,
        }
    }

    /// The composed (linear) volume — what stealing compares.
    pub fn volume(&self) -> f32 {
        self.level * self.now.volume
    }
}

#[derive(Default)]
pub struct Mixer {
    voices: [Option<Voice>; CHANNELS],
    /// The output's sample rate.
    pub rate: f32,
    pub law: Law,
}

impl Mixer {
    pub fn new(rate: f32) -> Mixer {
        Mixer {
            voices: Default::default(),
            rate,
            law: Law::Pc,
        }
    }

    /// Channel `c`'s voice, while it plays.
    pub fn voice(&self, c: usize) -> Option<&Voice> {
        self.voices.get(c).and_then(Option::as_ref)
    }

    pub fn voice_mut(&mut self, c: usize) -> Option<&mut Voice> {
        self.voices.get_mut(c).and_then(Option::as_mut)
    }

    pub fn start(&mut self, c: usize, v: Voice) {
        self.voices[c] = Some(v);
    }

    pub fn stop(&mut self, c: usize) {
        self.voices[c] = None;
    }

    /// Every voice playing paused, or every paused one going on
    /// (`SSrPlayingChannels_Pause`, `_Resume`).
    pub fn pause_all(&mut self, on: bool) {
        for v in self.voices.iter_mut().flatten() {
            v.paused = on;
        }
    }

    /// The output gains channel `c` is heading for.
    pub fn gains(&self, c: usize) -> [f32; 2] {
        self.voice(c)
            .map_or([0.0; 2], |v| self.law.gains(v.volume(), v.pan.0, v.pan.1))
    }

    /// Mixes `frames` stereo frames into `out` (interleaved, added to), or
    /// just plays them through when there is no output.
    pub fn mix(&mut self, mut out: Option<&mut [f32]>, frames: usize) {
        let (rate, law) = (self.rate, self.law);
        for slot in &mut self.voices {
            let Some(v) = slot else { continue };
            if v.paused {
                v.heard = None;
                continue;
            }
            let mut target = law.gains(v.volume(), v.pan.0, v.pan.1);
            let start = v.heard.unwrap_or(target);
            let mut ended = false;
            for f in 0..frames {
                let t = (f + 1) as f32 / frames as f32;
                let g = [
                    start[0] + (target[0] - start[0]) * t,
                    start[1] + (target[1] - start[1]) * t,
                ];
                let clip = &v.now.clip;
                let n = clip.frames();
                if n == 0 {
                    ended = true;
                    break;
                }
                if let Some(out) = out.as_deref_mut() {
                    let i = v.pos as usize;
                    let a = clip.frame(i.min(n - 1));
                    let b = clip.frame((i + 1).min(n - 1));
                    let w = (v.pos - i as f64) as f32;
                    for c in 0..2 {
                        out[f * 2 + c] += (a[c] + (b[c] - a[c]) * w) * g[c];
                    }
                }
                v.pos += f64::from(clip.rate * v.now.pitch / rate);
                if v.pos >= n as f64 {
                    v.pos -= n as f64;
                    match v.next.take() {
                        Some(next) => {
                            v.now = next;
                            target = law.gains(v.volume(), v.pan.0, v.pan.1);
                        }
                        None if v.now.looping => {}
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
            }
            v.heard = Some(target);
            if ended {
                *slot = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(name: &str, frames: usize) -> Arc<Clip> {
        Arc::new(Clip {
            name: name.into(),
            rate: 100.0,
            channels: 1,
            samples: vec![0.5; frames],
        })
    }

    fn play(c: &Arc<Clip>, looping: bool, part: u8) -> Play {
        Play {
            clip: c.clone(),
            pitch: 1.0,
            volume: 1.0,
            looping,
            part,
        }
    }

    #[test]
    fn in_then_loop_then_out_without_a_gap() {
        let mut m = Mixer::new(100.0);
        let (a, b, c) = (clip("in", 10), clip("loop", 10), clip("out", 10));
        let mut v = Voice::new(play(&a, false, 0), 1.0, (Side::Centre, 1.0));
        v.next = Some(play(&b, true, 1));
        m.start(3, v);
        let mut out = vec![0.0; 2 * 15];
        m.mix(Some(&mut out), 15);
        assert!(
            out.iter().all(|s| (*s - 0.5).abs() < 1e-6),
            "no silent frame between in and loop"
        );
        assert_eq!(m.voice(3).unwrap().now.part, 1);
        m.mix(None, 100);
        assert_eq!(m.voice(3).unwrap().now.part, 1, "the loop keeps going");
        let v = m.voice_mut(3).unwrap();
        v.now.looping = false;
        v.next = Some(play(&c, false, 2));
        m.mix(None, 10);
        assert_eq!(
            m.voice(3).unwrap().now.part,
            2,
            "the loop's pass ends, the out sound follows"
        );
        m.mix(None, 10);
        assert!(m.voice(3).is_none(), "and then the channel is free");
    }

    #[test]
    fn the_pc_law_is_logarithmic() {
        let pc = Law::Pc;
        assert_eq!(pc.gains(1.0, Side::Centre, 1.0), [1.0, 1.0]);
        let half = pc.gains(0.5, Side::Centre, 1.0)[0];
        assert!(
            (half - 0.1468).abs() < 1e-3,
            "half volume is −16.7 dB: {half}"
        );
        assert_eq!(
            pc.gains(0.05, Side::Centre, 1.0),
            [0.0, 0.0],
            "silent at 0.05"
        );
        // Straight to the left: the right ear loses 16.7 dB.
        let g = pc.gains(1.0, Side::Left, 0.5);
        assert!(g[0] == 1.0 && (g[1] - 0.1468).abs() < 1e-3, "{g:?}");
        // The Mac's is linear.
        assert_eq!(Law::Mac.gains(0.5, Side::Right, 0.5), [0.25, 0.5]);
    }

    #[test]
    fn reads_a_pcm_wav() {
        let mut w = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x22\x56\0\0\x44\xac\0\0\x02\0\x10\0data\x04\0\0\0".to_vec();
        w.extend_from_slice(&[0x00, 0x40, 0x00, 0xC0]);
        let c = Clip::parse(&w, "t").unwrap();
        assert_eq!((c.rate, c.channels, c.frames()), (22050.0, 1, 2));
        assert_eq!(c.samples, vec![0.5, -0.5]);
    }
}
