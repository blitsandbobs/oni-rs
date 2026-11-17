//! Oni `SNDD` (sound data) parsing and WAV extraction.
//!
//! A `SNDD` instance's on-disk layout (recovered empirically and matched to
//! `BFW_SoundSystem2.h`'s `SStSoundData` + inlined `SStFormat`):
//!
//! ```text
//! offset 0    u32        flags
//! offset 4    SStFormat  waveformat (50 bytes, == a WAV `fmt ` chunk body):
//!                          u16 wFormatTag  (1 = PCM, 2 = MS-ADPCM)
//!                          u16 nChannels
//!                          u32 nSamplesPerSec
//!                          u32 nAvgBytesPerSec
//!                          u16 nBlockAlign
//!                          u16 wBitsPerSample
//!                          u16 cbSize
//!                          u16 wSamplesPerBlock
//!                          u16 wNumCoef
//!                          i16 aCoef[2*wNumCoef]   (7 pairs for MS-ADPCM)
//! offset 54   u16        (duration/flags; unused here)
//! offset 56   u32        num_bytes   (length of audio data in .raw)
//! offset 60   u32        data        (byte offset of audio data in .raw)
//! ```
//!
//! The audio bytes in `.raw` are the MS-ADPCM (or PCM) stream exactly as the
//! engine feeds DirectSound. OniSplit's `-extract:wav` simply wraps them in a
//! RIFF/WAVE container whose `fmt ` chunk is a verbatim copy of the 50-byte
//! `SStFormat`; [`Sound::to_wav`] reproduces that, byte-identical in the `data`
//! chunk (validated for all 2380 level0 sounds — see the crate tests).

#![forbid(unsafe_code)]

pub mod adpcm;

use oni_dat::{Dat, FourCc, Instance};

/// The `SNDD` template tag.
pub const TAG_SNDD: FourCc = FourCc(0x534E_4444); // 'SNDD'

const OFF_FORMAT: usize = 4;
const FORMAT_FIXED_LEN: usize = 50;
const OFF_NUM_BYTES: usize = 56;
const OFF_DATA: usize = 60;
const SNDD_MIN_SIZE: usize = 64;

/// WAVE format tag: uncompressed PCM.
pub const WAVE_FORMAT_PCM: u16 = 1;
/// WAVE format tag: Microsoft ADPCM.
pub const WAVE_FORMAT_ADPCM: u16 = 2;

/// Errors from interpreting a SNDD instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SndError {
    /// The instance is not a `SNDD`.
    NotSound(FourCc),
    /// The instance has no data (placeholder) or is too small.
    NoData,
    /// The audio region lies outside the `.raw` buffer.
    OutOfRange { offset: u32, len: u32 },
}

impl std::fmt::Display for SndError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SndError::NotSound(t) => write!(f, "instance tag {t} is not SNDD"),
            SndError::NoData => write!(f, "SNDD instance has no/short data"),
            SndError::OutOfRange { offset, len } => {
                write!(
                    f,
                    "audio [{offset}, {offset}+{len}) is outside the .raw file"
                )
            }
        }
    }
}

impl std::error::Error for SndError {}

/// A parsed sound.
#[derive(Clone, Debug)]
pub struct Sound {
    /// Instance name (without tag prefix), if any.
    pub name: Option<String>,
    /// The 50-byte `SStFormat` blob, usable verbatim as a WAV `fmt ` chunk body.
    pub format: Vec<u8>,
    /// Length of the audio data in `.raw`.
    pub num_bytes: u32,
    /// Byte offset of the audio data in `.raw`.
    pub raw_offset: u32,
}

impl Sound {
    /// Parses a SNDD instance using its data bytes from `dat`.
    pub fn from_instance(dat: &Dat<'_>, inst: &Instance) -> Result<Sound, SndError> {
        if inst.tag != TAG_SNDD {
            return Err(SndError::NotSound(inst.tag));
        }
        let d = dat.instance_data(inst).ok_or(SndError::NoData)?;
        if d.len() < SNDD_MIN_SIZE {
            return Err(SndError::NoData);
        }
        let u32at = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
        Ok(Sound {
            name: inst.name.clone(),
            format: d[OFF_FORMAT..OFF_FORMAT + FORMAT_FIXED_LEN].to_vec(),
            num_bytes: u32at(OFF_NUM_BYTES),
            raw_offset: u32at(OFF_DATA),
        })
    }

    /// WAVE format tag (1 = PCM, 2 = MS-ADPCM).
    pub fn format_tag(&self) -> u16 {
        u16::from_le_bytes([self.format[0], self.format[1]])
    }
    /// Channel count.
    pub fn channels(&self) -> u16 {
        u16::from_le_bytes([self.format[2], self.format[3]])
    }
    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        u32::from_le_bytes([
            self.format[4],
            self.format[5],
            self.format[6],
            self.format[7],
        ])
    }
    /// True for MS-ADPCM-compressed audio.
    pub fn is_compressed(&self) -> bool {
        self.format_tag() == WAVE_FORMAT_ADPCM
    }

    /// Borrows the audio bytes from the `.raw` buffer.
    pub fn raw_audio<'a>(&self, raw: &'a [u8]) -> Result<&'a [u8], SndError> {
        let start = self.raw_offset as usize;
        let end = start
            .checked_add(self.num_bytes as usize)
            .filter(|&e| e <= raw.len())
            .ok_or(SndError::OutOfRange {
                offset: self.raw_offset,
                len: self.num_bytes,
            })?;
        Ok(&raw[start..end])
    }

    /// Builds a RIFF/WAVE file: `fmt ` chunk (the verbatim `SStFormat`) plus a
    /// `data` chunk (the raw audio). The `data` chunk is byte-identical to
    /// OniSplit's `-extract:wav` output.
    pub fn to_wav(&self, raw: &[u8]) -> Result<Vec<u8>, SndError> {
        let audio = self.raw_audio(raw)?;
        let fmt = &self.format;
        let mut out = Vec::with_capacity(12 + 8 + fmt.len() + 8 + audio.len());
        let riff_size = 4 + (8 + fmt.len()) + (8 + audio.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(riff_size as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        out.extend_from_slice(fmt);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(audio.len() as u32).to_le_bytes());
        out.extend_from_slice(audio);
        Ok(out)
    }

    fn fmt_u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.format[o], self.format[o + 1]])
    }

    /// Block size in bytes (`nBlockAlign`).
    pub fn block_align(&self) -> usize {
        self.fmt_u16(12) as usize
    }
    /// Decoded samples per block per channel (`wSamplesPerBlock`).
    pub fn samples_per_block(&self) -> usize {
        self.fmt_u16(18) as usize
    }
    /// The ADPCM predictor coefficient pairs (`aCoef`).
    pub fn coefs(&self) -> Vec<(i16, i16)> {
        let n = self.fmt_u16(20) as usize;
        (0..n)
            .map(|i| {
                let o = 22 + i * 4;
                (
                    i16::from_le_bytes([self.format[o], self.format[o + 1]]),
                    i16::from_le_bytes([self.format[o + 2], self.format[o + 3]]),
                )
            })
            .collect()
    }

    /// Decodes the audio to interleaved 16-bit PCM (decoding MS-ADPCM, or
    /// reinterpreting PCM as-is).
    pub fn decode_pcm(&self, raw: &[u8]) -> Result<Vec<i16>, SndError> {
        let audio = self.raw_audio(raw)?;
        Ok(if self.is_compressed() {
            adpcm::decode(
                audio,
                self.channels() as usize,
                self.block_align(),
                self.samples_per_block(),
                &self.coefs(),
            )
        } else {
            (0..audio.len() / 2)
                .map(|i| i16::from_le_bytes([audio[i * 2], audio[i * 2 + 1]]))
                .collect()
        })
    }

    /// Builds a 16-bit PCM WAV (decoding MS-ADPCM so it plays everywhere).
    pub fn to_pcm_wav(&self, raw: &[u8]) -> Result<Vec<u8>, SndError> {
        let pcm = self.decode_pcm(raw)?;
        let channels = self.channels().max(1);
        let rate = self.sample_rate();
        let data_len = pcm.len() * 2;
        let byte_rate = rate * channels as u32 * 2;
        let mut out = Vec::with_capacity(44 + data_len);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for s in pcm {
            out.extend_from_slice(&s.to_le_bytes());
        }
        Ok(out)
    }
}

/// Iterates every SNDD sound in `dat`.
pub fn sounds<'a>(dat: &'a Dat<'a>) -> impl Iterator<Item = Sound> + 'a {
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_SNDD)
        .filter_map(move |i| Sound::from_instance(dat, i).ok())
}
