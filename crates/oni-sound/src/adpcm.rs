//! Microsoft ADPCM decoder → 16-bit PCM.
//!
//! Oni's compressed sounds (`wFormatTag == 2`) are standard MS-ADPCM: block
//! based, with a per-block, per-channel preamble (`predictor`, `delta`, two
//! seed samples) followed by 4-bit nibbles (high nibble first). For stereo,
//! nibble bytes alternate between the two channels. Predictor coefficients come
//! from the sound's `aCoef` table.

/// MS-ADPCM adaptation table (fixed).
const ADAPT: [i32; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];

#[inline]
fn clamp_i16(v: i32) -> i16 {
    v.clamp(-32768, 32767) as i16
}

struct ChannelState {
    coef1: i32,
    coef2: i32,
    delta: i32,
    s1: i32,
    s2: i32,
}

impl ChannelState {
    fn expand(&mut self, nibble: u8) -> i16 {
        let signed = if nibble >= 8 {
            nibble as i64 - 16
        } else {
            nibble as i64
        };
        // Predictor uses truncating (toward-zero) division, per the MS-ADPCM
        // reference; the adaptation step below uses an arithmetic shift. Widen
        // to i64 so intermediate products can't overflow.
        let predict =
            (self.s1 as i64 * self.coef1 as i64 + self.s2 as i64 * self.coef2 as i64) / 256;
        let sample = clamp_i16(
            (predict + signed * self.delta as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        );
        self.delta = ((ADAPT[nibble as usize] as i64 * self.delta as i64) >> 8) as i32;
        if self.delta < 16 {
            self.delta = 16;
        }
        self.s2 = self.s1;
        self.s1 = sample as i32;
        sample
    }
}

/// Decodes MS-ADPCM `data` to interleaved 16-bit PCM.
pub fn decode(
    data: &[u8],
    channels: usize,
    block_align: usize,
    samples_per_block: usize,
    coefs: &[(i16, i16)],
) -> Vec<i16> {
    let mut out = Vec::new();
    if !(1..=2).contains(&channels) || block_align < 7 * channels || samples_per_block < 2 {
        return out;
    }
    let i16at = |b: &[u8], o: usize| i16::from_le_bytes([b[o], b[o + 1]]) as i32;

    for block in data.chunks(block_align) {
        let header = 7 * channels;
        if block.len() < header {
            break;
        }
        // Parse the per-channel preamble: predictors, deltas, then s1s, then s2s.
        let mut p = 0;
        let mut st: Vec<ChannelState> = Vec::with_capacity(channels);
        let preds: Vec<usize> = (0..channels)
            .map(|_| {
                let v = block[p] as usize;
                p += 1;
                v.min(coefs.len().saturating_sub(1))
            })
            .collect();
        let deltas: Vec<i32> = (0..channels)
            .map(|_| {
                let v = i16at(block, p);
                p += 2;
                v
            })
            .collect();
        let s1s: Vec<i32> = (0..channels)
            .map(|_| {
                let v = i16at(block, p);
                p += 2;
                v
            })
            .collect();
        let s2s: Vec<i32> = (0..channels)
            .map(|_| {
                let v = i16at(block, p);
                p += 2;
                v
            })
            .collect();
        for c in 0..channels {
            let (c1, c2) = coefs.get(preds[c]).copied().unwrap_or((256, 0));
            st.push(ChannelState {
                coef1: c1 as i32,
                coef2: c2 as i32,
                delta: deltas[c],
                s1: s1s[c],
                s2: s2s[c],
            });
        }

        // Per-channel decoded samples, seeded with [s2, s1] (chronological order).
        let mut chans: Vec<Vec<i16>> = (0..channels)
            .map(|c| vec![clamp_i16(s2s[c]), clamp_i16(s1s[c])])
            .collect();

        // Nibbles, high first. Mono: both nibbles feed the one channel. Stereo:
        // the two channels interleave per nibble (high → left, low → right).
        for &byte in &block[header..] {
            let hi = (byte >> 4) & 0xF;
            let lo = byte & 0xF;
            if channels == 1 {
                if chans[0].len() < samples_per_block {
                    chans[0].push(st[0].expand(hi));
                }
                if chans[0].len() < samples_per_block {
                    chans[0].push(st[0].expand(lo));
                }
            } else {
                if chans[0].len() < samples_per_block {
                    chans[0].push(st[0].expand(hi));
                }
                if chans[1].len() < samples_per_block {
                    chans[1].push(st[1].expand(lo));
                }
            }
        }

        // Interleave channels up to samples_per_block (clamp to the shortest).
        let n = chans
            .iter()
            .map(|c| c.len())
            .min()
            .unwrap_or(0)
            .min(samples_per_block);
        for i in 0..n {
            for c in chans.iter() {
                out.push(c[i]);
            }
        }
    }
    out
}
