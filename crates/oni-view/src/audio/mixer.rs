//! The mixer: voices playing decoded clips, each with a queued next clip
//! (so an ambient's in sound, loop and out sound follow without a gap) and
//! left / right gains the simulation sets every tick. The output device's
//! callback drives it in play mode; headless runs advance it a tick's worth
//! of samples at a time, so voices end on the same ticks either way.

use std::path::Path;
use std::sync::Arc;

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
                b"fmt " if len >= 16 => fmt = Some((u16_at(at + 8), u16_at(at + 10), u32_at(at + 12), u16_at(at + 22))),
                b"data" => data = Some(body),
                _ => {}
            }
            at += 8 + len + (len & 1);
        }
        let (format, channels, rate, bits) = fmt?;
        if format != 1 || bits != 16 || !(1..=2).contains(&channels) {
            return None;
        }
        let samples = b[data?].chunks_exact(2).map(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0).collect();
        Some(Clip { name: name.to_string(), rate: rate as f32, channels: channels as usize, samples })
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
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

/// One clip to play on a voice.
#[derive(Clone)]
pub struct Play {
    pub clip: Arc<Clip>,
    /// Playback speed (the pitch factor).
    pub pitch: f32,
    /// The permutation's and the group's volume.
    pub volume: f32,
    pub looping: bool,
    /// What it is to its owner (e.g. an ambient's in sound, loop, out sound).
    pub part: u8,
}

pub struct Voice {
    pub now: Play,
    pub next: Option<Play>,
    /// Left / right gains (the simulation's: distance, pan, channel volume).
    pub gain: [f32; 2],
    /// The gains being heard (ramping toward `gain` to avoid clicks).
    heard: [f32; 2],
    pos: f64,
}

impl Voice {
    pub fn new(now: Play, gain: [f32; 2]) -> Voice {
        Voice { now, next: None, gain, heard: gain, pos: 0.0 }
    }
}

/// A voice's slot and generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceId {
    slot: usize,
    generation: u32,
}

#[derive(Default)]
pub struct Mixer {
    voices: Vec<(u32, Option<Voice>)>,
    /// The output's sample rate.
    pub rate: f32,
}

impl Mixer {
    pub fn new(rate: f32) -> Mixer {
        Mixer { voices: Vec::new(), rate }
    }

    pub fn play(&mut self, v: Voice) -> VoiceId {
        let slot = match self.voices.iter().position(|(_, v)| v.is_none()) {
            Some(s) => s,
            None => {
                self.voices.push((0, None));
                self.voices.len() - 1
            }
        };
        let e = &mut self.voices[slot];
        e.0 = e.0.wrapping_add(1);
        e.1 = Some(v);
        VoiceId { slot, generation: e.0 }
    }

    /// A voice still playing.
    pub fn get_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.get_mut(id.slot).filter(|(g, _)| *g == id.generation).and_then(|(_, v)| v.as_mut())
    }

    pub fn stop(&mut self, id: VoiceId) {
        if let Some(e) = self.voices.get_mut(id.slot).filter(|(g, _)| *g == id.generation) {
            e.1 = None;
        }
    }


    /// Mixes `frames` stereo frames into `out` (interleaved, added to), or
    /// just plays them through when there is no output.
    pub fn mix(&mut self, mut out: Option<&mut [f32]>, frames: usize) {
        let rate = self.rate;
        for (_, slot) in &mut self.voices {
            let Some(v) = slot else { continue };
            let start = v.heard;
            let mut ended = false;
            for f in 0..frames {
                let t = (f + 1) as f32 / frames as f32;
                let g = [start[0] + (v.gain[0] - start[0]) * t, start[1] + (v.gain[1] - start[1]) * t];
                let clip = &v.now.clip;
                let n = clip.frames();
                if n == 0 {
                    ended = true;
                    break;
                }
                if let Some(out) = out.as_deref_mut() {
                    let i = v.pos as usize;
                    let a = clip.frame(i.min(n - 1));
                    let b = clip.frame(if i + 1 < n { i + 1 } else if v.now.looping { 0 } else { n - 1 });
                    let w = (v.pos - i as f64) as f32;
                    for c in 0..2 {
                        out[f * 2 + c] += (a[c] + (b[c] - a[c]) * w) * v.now.volume * g[c];
                    }
                }
                v.pos += f64::from(clip.rate * v.now.pitch / rate);
                if v.pos >= n as f64 {
                    v.pos -= n as f64;
                    if v.now.looping {
                        continue;
                    }
                    match v.next.take() {
                        Some(next) => v.now = next,
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
            }
            v.heard = v.gain;
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
        Arc::new(Clip { name: name.into(), rate: 100.0, channels: 1, samples: vec![0.5; frames] })
    }

    fn play(c: &Arc<Clip>, looping: bool, part: u8) -> Play {
        Play { clip: c.clone(), pitch: 1.0, volume: 1.0, looping, part }
    }

    #[test]
    fn in_then_loop_then_out_without_a_gap() {
        let mut m = Mixer::new(100.0);
        let (a, b, c) = (clip("in", 10), clip("loop", 10), clip("out", 10));
        let mut v = Voice::new(play(&a, false, 0), [1.0, 1.0]);
        v.next = Some(play(&b, true, 1));
        let id = m.play(v);
        let mut out = vec![0.0; 2 * 15];
        m.mix(Some(&mut out), 15);
        assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-6), "no silent frame between in and loop");
        assert_eq!(m.get_mut(id).unwrap().now.part, 1);
        m.mix(None, 100);
        assert_eq!(m.get_mut(id).unwrap().now.part, 1, "the loop keeps going");
        let v = m.get_mut(id).unwrap();
        v.now.looping = false;
        v.next = Some(play(&c, false, 2));
        m.mix(None, 10);
        assert_eq!(m.get_mut(id).unwrap().now.part, 2, "the loop's pass ends, the out sound follows");
        m.mix(None, 10);
        assert!(m.get_mut(id).is_none(), "and then the voice is free");
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
