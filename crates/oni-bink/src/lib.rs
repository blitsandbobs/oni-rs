//! Bink movies (`.bik`): the container, its video ([`VideoDecoder`]) and
//! its audio ([`AudioDecoder`]). Written from the format's public
//! descriptions, held against FFmpeg's decoding of Red Alert 2's movies.
//!
//! Vendored from ra2-engine (`crates/ra2-formats/src/bink`, commit
//! 14640554), unchanged but for this note and rustfmt's 2021 style;
//! GPL-3.0-or-later, as there. Oni's one movie, `outro.bik` (revision `i`, 512 × 384, 15 fps, 44.1 kHz stereo
//! RDFT audio), decodes as FFmpeg does it: every frame to the pixel, the
//! audio to 2.4e-7 (`tests/outro.rs`).
//!
//! The container, all numbers little-endian:
//!
//! ```text
//! "BIK" and the revision letter ('b' to 'i')
//! u32 file size less 8, u32 frames, u32 largest frame, u32 frames again
//! u32 width, height, u32 frame rate numerator, denominator, u32 video flags
//! u32 audio tracks
//! per track: u16 largest decoded audio size, u16 channels
//! per track: u16 sample rate, u16 flags (0x2000 stereo, 0x1000 DCT)
//! per track: u32 track id
//! frames + 1 x u32: where each frame starts, bit 0 marking a keyframe;
//!   the last is the end of the file
//! ```
//!
//! A frame holds, per audio track, a u32 size and that many bytes of audio
//! packet (0 for none), then the video packet in what is left.

mod audio;
mod bits;
#[rustfmt::skip]
mod tables;
mod video;

pub use audio::AudioDecoder;
pub use video::{Picture, Plane, VideoDecoder};

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinkError {
    NotBink,
    Truncated,
    Unsupported(&'static str),
    /// The data is not as the format has it.
    Malformed(&'static str),
}

impl fmt::Display for BinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBink => write!(f, "not a Bink movie"),
            Self::Truncated => write!(f, "the movie is cut short"),
            Self::Unsupported(what) => write!(f, "not supported: {what}"),
            Self::Malformed(what) => write!(f, "malformed movie: {what}"),
        }
    }
}

impl std::error::Error for BinkError {}

/// The video flags' bit for an alpha plane.
const FLAG_ALPHA: u32 = 0x0010_0000;
/// An audio track's flags: stereo, and the DCT variant (else RDFT).
const AUDIO_STEREO: u16 = 0x2000;
const AUDIO_DCT: u16 = 0x1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTrack {
    pub sample_rate: u32,
    pub flags: u16,
    pub id: u32,
}

impl AudioTrack {
    pub fn channels(&self) -> u32 {
        if self.flags & AUDIO_STEREO != 0 {
            2
        } else {
            1
        }
    }

    /// Whether the track is coded by the DCT, else by the RDFT.
    pub fn uses_dct(&self) -> bool {
        self.flags & AUDIO_DCT != 0
    }
}

/// One frame's packets.
pub struct Packets<'a> {
    /// Per audio track, its packet (empty for none).
    pub audio: Vec<&'a [u8]>,
    pub video: &'a [u8],
}

pub struct Bink {
    /// The revision letter, `b'b'` to `b'i'`.
    pub revision: u8,
    pub width: u32,
    pub height: u32,
    /// Frames a second, as numerator and denominator.
    pub rate: (u32, u32),
    pub video_flags: u32,
    pub tracks: Vec<AudioTrack>,
    /// Where each frame starts and ends, and whether it is a keyframe.
    frames: Vec<(usize, usize, bool)>,
    data: Vec<u8>,
}

impl Bink {
    pub fn parse(data: Vec<u8>) -> Result<Self, BinkError> {
        let u32_at = |at: usize| -> Result<u32, BinkError> {
            let bytes = data.get(at..at + 4).ok_or(BinkError::Truncated)?;
            Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
        };
        let u16_at = |at: usize| -> Result<u16, BinkError> {
            let bytes = data.get(at..at + 2).ok_or(BinkError::Truncated)?;
            Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
        };
        if data.get(0..3) != Some(b"BIK") {
            return Err(BinkError::NotBink);
        }
        let revision = data[3];
        if !(b'b'..=b'i').contains(&revision) {
            return Err(BinkError::Unsupported("revision"));
        }
        let count = u32_at(8)? as usize;
        let (width, height) = (u32_at(20)?, u32_at(24)?);
        let rate = (u32_at(28)?, u32_at(32)?);
        let video_flags = u32_at(36)?;
        let track_count = u32_at(40)? as usize;
        if track_count > 256 || width == 0 || height == 0 || width > 32767 || height > 32767 {
            return Err(BinkError::Malformed("header"));
        }
        let mut at = 44 + 4 * track_count;
        let mut tracks = Vec::with_capacity(track_count);
        for _ in 0..track_count {
            tracks.push(AudioTrack {
                sample_rate: u32::from(u16_at(at)?),
                flags: u16_at(at + 2)?,
                id: 0,
            });
            at += 4;
        }
        for track in &mut tracks {
            track.id = u32_at(at)?;
            at += 4;
        }
        let mut frames = Vec::with_capacity(count);
        let mut start = u32_at(at)?;
        for number in 0..count {
            let next = if number + 1 == count {
                data.len() as u32
            } else {
                u32_at(at + 4 * (number + 1))?
            };
            let (from, to) = ((start & !1) as usize, (next & !1) as usize);
            if from > to || to > data.len() {
                return Err(BinkError::Malformed("frame index"));
            }
            frames.push((from, to, start & 1 != 0));
            start = next;
        }
        Ok(Self {
            revision,
            width,
            height,
            rate,
            video_flags,
            tracks,
            frames,
            data,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn is_keyframe(&self, number: usize) -> bool {
        self.frames[number].2
    }

    pub fn has_alpha(&self) -> bool {
        self.video_flags & FLAG_ALPHA != 0
    }

    /// Frames a second.
    pub fn frames_per_second(&self) -> f64 {
        f64::from(self.rate.0) / f64::from(self.rate.1.max(1))
    }

    /// The packets of frame `number`.
    pub fn packets(&self, number: usize) -> Result<Packets<'_>, BinkError> {
        let (from, to, _) = self.frames[number];
        let mut rest = &self.data[from..to];
        let mut audio = Vec::with_capacity(self.tracks.len());
        for _ in &self.tracks {
            let size = rest.get(0..4).ok_or(BinkError::Truncated)?;
            let size = u32::from_le_bytes(size.try_into().unwrap()) as usize;
            let packet = rest.get(4..4 + size).ok_or(BinkError::Truncated)?;
            audio.push(packet);
            rest = &rest[4 + size..];
        }
        Ok(Packets { audio, video: rest })
    }

    /// A decoder of the video.
    pub fn video_decoder(&self) -> Result<VideoDecoder, BinkError> {
        VideoDecoder::new(self.revision, self.width, self.height, self.has_alpha())
    }

    /// A decoder of audio track `track`.
    pub fn audio_decoder(&self, track: usize) -> Result<AudioDecoder, BinkError> {
        AudioDecoder::new(&self.tracks[track], self.revision)
    }
}
