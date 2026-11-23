//! Movies: Oni's one, the ending (`outro.bik`, played
//! after level 19's win splash). Bink decoded by `oni-bink`; this keeps the
//! picture on its frame for the time played and hands the sound on in
//! stretches for the mixer to queue (`Oni_Bink.c`, `BFW_Bink.c`).

use std::path::Path;

use oni_bink::{AudioDecoder, Bink, VideoDecoder};

/// How much sound is handed on at a time, and how far ahead of the picture
/// it is kept (seconds).
const AUDIO_CHUNK: f64 = 0.5;

pub struct Movie {
    bink: Bink,
    video: VideoDecoder,
    audio: Option<AudioDecoder>,
    /// Seconds played.
    time: f64,
    /// The frame the picture is on (none yet), and the next whose sound is
    /// to be decoded.
    frame: Option<usize>,
    audio_frame: usize,
    /// The picture as RGBA, and whether it changed since last taken.
    rgba: Vec<u8>,
    fresh: bool,
}

impl Movie {
    pub fn open(path: &Path) -> Result<Movie, String> {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bink = Bink::parse(data).map_err(|e| e.to_string())?;
        let video = bink.video_decoder().map_err(|e| e.to_string())?;
        let audio = if bink.tracks.is_empty() {
            None
        } else {
            Some(bink.audio_decoder(0).map_err(|e| e.to_string())?)
        };
        let rgba = vec![0; (bink.width * bink.height * 4) as usize];
        Ok(Movie {
            bink,
            video,
            audio,
            time: 0.0,
            frame: None,
            audio_frame: 0,
            rgba,
            fresh: false,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.bink.width, self.bink.height)
    }

    /// The frame the picture is on.
    pub fn frame(&self) -> Option<usize> {
        self.frame
    }

    pub fn frame_count(&self) -> usize {
        self.bink.frame_count()
    }

    /// The sound's rate and channels (none: a silent movie).
    pub fn audio_format(&self) -> Option<(u32, usize)> {
        let t = self.bink.tracks.first()?;
        Some((t.sample_rate, t.channels() as usize))
    }

    /// `dt` seconds on: the picture decoded up to the frame now due (every
    /// one between, Bink predicting each from the last). True once the last
    /// frame has been shown (`BFW_Bink.c`: the loop ends on the last frame).
    pub fn advance(&mut self, dt: f64) -> Result<bool, String> {
        let last = self.frame_count().saturating_sub(1);
        if self.frame == Some(last) {
            return Ok(true);
        }
        self.time += dt;
        // (A hair over, so steps summing to a whole frame reach it.)
        let due = ((self.time * self.bink.frames_per_second() + 1e-6) as usize).min(last);
        let from = self.frame.map_or(0, |f| f + 1);
        for n in from..=due {
            let packets = self.bink.packets(n).map_err(|e| e.to_string())?;
            let picture = self
                .video
                .decode(packets.video)
                .map_err(|e| format!("frame {n}: {e}"))?;
            if n == due {
                for (out, rgb) in self
                    .rgba
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(picture.to_rgb().as_chunks::<3>().0)
                {
                    out[..3].copy_from_slice(rgb);
                    out[3] = 255;
                }
                self.fresh = true;
            }
            self.frame = Some(n);
        }
        Ok(false)
    }

    /// The picture, if it changed since last taken.
    pub fn take_picture(&mut self) -> Option<&[u8]> {
        std::mem::take(&mut self.fresh).then_some(self.rgba.as_slice())
    }

    /// The next stretch of sound (interleaved floats), once the sound handed
    /// on so far runs less than a stretch ahead of the picture; none at the
    /// end, or when not yet wanted.
    pub fn take_audio(&mut self) -> Option<Vec<f32>> {
        let fps = self.bink.frames_per_second();
        let audio = self.audio.as_mut()?;
        let ahead = self.audio_frame as f64 / fps - self.time;
        if ahead > AUDIO_CHUNK || self.audio_frame >= self.bink.frame_count() {
            return None;
        }
        let until = ((self.audio_frame as f64 + AUDIO_CHUNK * fps).ceil() as usize)
            .min(self.bink.frame_count());
        let mut out = Vec::new();
        for n in self.audio_frame..until {
            if let Ok(p) = self.bink.packets(n) {
                audio.decode(p.audio[0], &mut out);
            }
        }
        self.audio_frame = until;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outro() -> Option<std::path::PathBuf> {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../generated/oni/movies/outro.bik");
        p.is_file().then_some(p)
    }

    #[test]
    fn plays_through_on_time_with_its_sound_ahead() {
        let Some(path) = outro() else { return };
        let mut m = Movie::open(&path).unwrap();
        assert_eq!(m.size(), (512, 384));
        assert!(!m.advance(0.0).unwrap());
        assert_eq!(m.frame(), Some(0));
        assert!(m.take_picture().is_some());
        assert!(m.take_picture().is_none(), "taken once");
        // Two seconds at 15 frames a second.
        for _ in 0..120 {
            m.advance(1.0 / 60.0).unwrap();
        }
        assert_eq!(m.frame(), Some(30));
        // Its sound handed on in half-second stretches, kept ahead.
        let (rate, channels) = m.audio_format().unwrap();
        let mut sound = 0;
        while let Some(chunk) = m.take_audio() {
            sound += chunk.len();
        }
        let seconds = sound as f64 / f64::from(rate) / channels as f64;
        // (Ahead of the picture's two seconds, but not by much: the first
        // frames carry a little extra, Bink's preload.)
        assert!((2.4..=4.0).contains(&seconds), "{seconds} s of sound");
        // To the end.
        let mut done = false;
        for _ in 0..(m.frame_count() * 4 + 10) {
            if m.advance(1.0 / 60.0).unwrap() {
                done = true;
                break;
            }
        }
        assert!(done && m.frame() == Some(m.frame_count() - 1));
    }
}
