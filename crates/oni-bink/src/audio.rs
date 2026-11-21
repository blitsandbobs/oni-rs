//! Bink audio, as the MultimediaWiki's "Bink Audio" page and Kostya
//! Shishkov's "Encoding Bink Audio" describe it: frames of coefficients
//! quantised per critical band, turned back into samples by an inverse
//! real DFT, stereo interleaved in one transform, each frame overlapping
//! the last by a sixteenth. Only the RDFT variant of revisions after 'b'
//! is read: the game's movies have no other (`docs/original/movies.md`).

use std::f64::consts::PI;

use super::bits::Bits;
use super::{AudioTrack, BinkError};

/// The critical bands' upper frequencies.
const CRITICAL_FREQUENCIES: [u32; 25] = [
    100, 200, 300, 400, 510, 630, 770, 920, 1080, 1270, 1480, 1720, 2000, 2320, 2700, 3150, 3700,
    4400, 5300, 6400, 7700, 9500, 12000, 15500, 24500,
];

/// How many groups of eight a run code covers.
const RUN_LENGTHS: [usize; 16] = [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32, 64];

pub struct AudioDecoder {
    channels: usize,
    /// Samples a frame, all channels counted.
    frame_len: usize,
    overlap: usize,
    /// Where each band starts, and one past the last.
    bands: Vec<usize>,
    /// The scale applied to every coefficient.
    scale: f32,
    /// The last frame's tail, to overlap with.
    previous: Vec<f32>,
    first: bool,
}

impl AudioDecoder {
    pub fn new(track: &AudioTrack, revision: u8) -> Result<Self, BinkError> {
        if track.uses_dct() {
            return Err(BinkError::Unsupported("DCT audio"));
        }
        if revision == b'b' {
            return Err(BinkError::Unsupported("audio of revision 'b'"));
        }
        let channels = track.channels() as usize;
        // The frame's length by the track's rate, longer by the channels
        // interleaved in it.
        let bits = match track.sample_rate {
            ..22050 => 9,
            22050..44100 => 10,
            _ => 11,
        } + (channels as u32).ilog2();
        let frame_len = 1usize << bits;
        let half_rate = (track.sample_rate * channels as u32).div_ceil(2);
        let mut band_count = 1;
        while band_count < 25 && half_rate > CRITICAL_FREQUENCIES[band_count - 1] {
            band_count += 1;
        }
        let mut bands = vec![2];
        for &frequency in &CRITICAL_FREQUENCIES[..band_count - 1] {
            bands.push((frequency as usize * frame_len / half_rate as usize) & !1);
        }
        bands.push(frame_len);
        let scale = (2.0 / ((frame_len as f64).sqrt() * 32768.0)) as f32;
        Ok(Self {
            channels,
            frame_len,
            overlap: frame_len / 16,
            bands,
            scale,
            previous: vec![0.0; frame_len / 16],
            first: true,
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Decodes a packet (the u32 count of samples it makes, then its
    /// frames), adding its samples to `out`, channels interleaved, as
    /// floats from -1 to 1 (or a little past).
    pub fn decode(&mut self, packet: &[u8], out: &mut Vec<f32>) {
        let Some(data) = packet.get(4..) else { return };
        let mut bits = Bits::new(data);
        while bits.left() >= 32 {
            self.decode_frame(&mut bits, out);
            bits.align32();
        }
    }

    fn decode_frame(&mut self, bits: &mut Bits, out: &mut Vec<f32>) {
        let mut frame = inverse_rdft(&self.read_coefficients(bits));
        // The overlap with the last frame, a linear crossfade.
        if !self.first {
            let count = self.overlap as f32;
            for (i, sample) in frame[..self.overlap].iter_mut().enumerate() {
                let j = i as f32;
                *sample = (self.previous[i] * (count - j) + *sample * j) / count;
            }
        }
        let len = self.frame_len - self.overlap;
        self.previous.copy_from_slice(&frame[len..]);
        self.first = false;
        out.extend_from_slice(&frame[..len]);
    }

    fn read_coefficients(&self, bits: &mut Bits) -> Vec<f32> {
        let mut coefficients = vec![0.0f32; self.frame_len];
        for coefficient in &mut coefficients[..2] {
            *coefficient = read_float(bits) * self.scale;
        }
        let quantisers: Vec<f32> = (0..self.bands.len() - 1)
            .map(|_| {
                let value = bits.read(8).min(95) as f32;
                (value * 0.152_891_65).exp() * self.scale
            })
            .collect();
        let mut band = 0;
        let mut quantiser = quantisers[0];
        let mut i = 2;
        while i < self.frame_len {
            // A group of eight, or a run of groups, of one width.
            let end = if bits.bit() {
                i + RUN_LENGTHS[bits.read(4) as usize] * 8
            } else {
                i + 8
            }
            .min(self.frame_len);
            let width = bits.read(4);
            if width == 0 {
                i = end;
                while self.bands[band] < i {
                    quantiser = quantisers[band];
                    band += 1;
                }
                continue;
            }
            while i < end {
                if self.bands[band] == i {
                    quantiser = quantisers[band];
                    band += 1;
                }
                let value = bits.read(width);
                if value != 0 {
                    let value = quantiser * value as f32;
                    coefficients[i] = if bits.bit() { -value } else { value };
                }
                i += 1;
            }
        }
        coefficients
    }
}

/// A 29-bit float: 5 bits of exponent, 23 of mantissa, a sign.
fn read_float(bits: &mut Bits) -> f32 {
    let exponent = bits.read(5) as i32;
    let mantissa = bits.read(23) as f32;
    let value = mantissa * 2f32.powi(exponent - 23);
    if bits.bit() {
        -value
    } else {
        value
    }
}

/// The inverse real DFT of `n` values packed as the DC, the Nyquist term,
/// then (re, im) of each frequency between.
fn inverse_rdft(packed: &[f32]) -> Vec<f32> {
    let n = packed.len();
    let mut spectrum = vec![(0.0f64, 0.0f64); n];
    spectrum[0] = (f64::from(packed[0]), 0.0);
    spectrum[n / 2] = (f64::from(packed[1]), 0.0);
    for k in 1..n / 2 {
        let (re, im) = (f64::from(packed[2 * k]), f64::from(packed[2 * k + 1]));
        spectrum[k] = (re, im);
        spectrum[n - k] = (re, -im);
    }
    fft(&mut spectrum);
    spectrum.iter().map(|&(re, _)| (re * 0.5) as f32).collect()
}

/// An in-place radix-2 complex FFT, by exp(-2πi nk / N).
fn fft(data: &mut [(f64, f64)]) {
    let n = data.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            data.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f64).sin_cos();
                let (a, b) = (data[start + k], data[start + k + len / 2]);
                let t = (b.0 * c - b.1 * s, b.0 * s + b.1 * c);
                data[start + k] = (a.0 + t.0, a.1 + t.1);
                data[start + k + len / 2] = (a.0 - t.0, a.1 - t.1);
            }
        }
        len <<= 1;
    }
}
