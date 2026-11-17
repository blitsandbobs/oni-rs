//! Decode one mip level of Oni pixel data into RGBA8888 (R,G,B,A byte order).
//!
//! Oni stores these little-endian (the `M3cTextureFlags_LittleEndian` bit is
//! set on PC data). Channel expansion uses bit replication, the conventional
//! lossless widening (e.g. 5→8 bits = `v<<3 | v>>2`).

use crate::format::PixelType;

/// Error from decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The input slice is shorter than the format/size requires.
    ShortInput { need: usize, got: usize },
    /// This format does not have a decoder yet.
    Unsupported(PixelType),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::ShortInput { need, got } => {
                write!(f, "need {need} bytes of pixel data, got {got}")
            }
            DecodeError::Unsupported(p) => write!(f, "no decoder for {p:?}"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[inline]
fn e4(v: u16) -> u8 {
    let v = (v & 0xF) as u8;
    (v << 4) | v
}
#[inline]
fn e5(v: u16) -> u8 {
    let v = (v & 0x1F) as u8;
    (v << 3) | (v >> 2)
}
#[inline]
fn e6(v: u16) -> u8 {
    let v = (v & 0x3F) as u8;
    (v << 2) | (v >> 4)
}

/// Decodes `width`×`height` texels of `format` from `data` into RGBA8888.
/// Only the single level described by `width`/`height` is read.
pub fn decode_rgba8888(
    format: PixelType,
    width: u32,
    height: u32,
    data: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    let need = format.level_size(width, height) as usize;
    if data.len() < need {
        return Err(DecodeError::ShortInput {
            need,
            got: data.len(),
        });
    }
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h * 4];

    match format {
        PixelType::Dxt1 => decode_dxt1(width, height, data, &mut out),
        PixelType::Argb4444
        | PixelType::Rgb555
        | PixelType::Argb1555
        | PixelType::Rgb565
        | PixelType::Abgr1555
        | PixelType::Rgba5551
        | PixelType::Rgba4444 => decode_16bit(format, w, h, data, &mut out),
        PixelType::Argb8888 | PixelType::Rgb888 => decode_32bit(format, w, h, data, &mut out),
        PixelType::I8 => {
            for i in 0..(w * h) {
                let v = data[i];
                out[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        PixelType::A8 => {
            for i in 0..(w * h) {
                out[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, data[i]]);
            }
        }
        other => return Err(DecodeError::Unsupported(other)),
    }
    Ok(out)
}

fn decode_16bit(format: PixelType, w: usize, h: usize, data: &[u8], out: &mut [u8]) {
    for i in 0..(w * h) {
        let px = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
        let (r, g, b, a) = match format {
            PixelType::Argb4444 => (e4(px >> 8), e4(px >> 4), e4(px), e4(px >> 12)),
            PixelType::Rgb555 => (e5(px >> 10), e5(px >> 5), e5(px), 255),
            PixelType::Argb1555 => (
                e5(px >> 10),
                e5(px >> 5),
                e5(px),
                if px & 0x8000 != 0 { 255 } else { 0 },
            ),
            PixelType::Rgb565 => (e5(px >> 11), e6(px >> 5), e5(px), 255),
            PixelType::Abgr1555 => (
                e5(px),
                e5(px >> 5),
                e5(px >> 10),
                if px & 0x8000 != 0 { 255 } else { 0 },
            ),
            PixelType::Rgba5551 => (
                e5(px >> 11),
                e5(px >> 6),
                e5(px >> 1),
                if px & 1 != 0 { 255 } else { 0 },
            ),
            PixelType::Rgba4444 => (e4(px >> 12), e4(px >> 8), e4(px >> 4), e4(px)),
            _ => unreachable!("decode_16bit called with non-16-bit format"),
        };
        out[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, a]);
    }
}

fn decode_32bit(format: PixelType, w: usize, h: usize, data: &[u8], out: &mut [u8]) {
    for i in 0..(w * h) {
        // Stored A8R8G8B8 little-endian => bytes [B, G, R, A].
        let b = data[i * 4];
        let g = data[i * 4 + 1];
        let r = data[i * 4 + 2];
        let a = if matches!(format, PixelType::Rgb888) {
            255
        } else {
            data[i * 4 + 3]
        };
        out[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, a]);
    }
}

/// DXT1 / BC1 block decode.
fn decode_dxt1(width: u32, height: u32, data: &[u8], out: &mut [u8]) {
    let (w, h) = (width as usize, height as usize);
    let blocks_x = w.div_ceil(4);
    let blocks_y = h.div_ceil(4);
    let mut colors = [[0u8; 4]; 4];

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let off = (by * blocks_x + bx) * 8;
            let c0 = u16::from_le_bytes([data[off], data[off + 1]]);
            let c1 = u16::from_le_bytes([data[off + 2], data[off + 3]]);
            let (r0, g0, b0) = (e5(c0 >> 11), e6(c0 >> 5), e5(c0));
            let (r1, g1, b1) = (e5(c1 >> 11), e6(c1 >> 5), e5(c1));
            colors[0] = [r0, g0, b0, 255];
            colors[1] = [r1, g1, b1, 255];
            if c0 > c1 {
                // 4-colour (opaque) block: 2/3 and 1/3 blends.
                colors[2] = [
                    (((2 * r0 as u16 + r1 as u16) / 3) as u8),
                    (((2 * g0 as u16 + g1 as u16) / 3) as u8),
                    (((2 * b0 as u16 + b1 as u16) / 3) as u8),
                    255,
                ];
                colors[3] = [
                    (((r0 as u16 + 2 * r1 as u16) / 3) as u8),
                    (((g0 as u16 + 2 * g1 as u16) / 3) as u8),
                    (((b0 as u16 + 2 * b1 as u16) / 3) as u8),
                    255,
                ];
            } else {
                // 3-colour block: 1/2 blend + transparent black.
                colors[2] = [
                    (((r0 as u16 + r1 as u16) / 2) as u8),
                    (((g0 as u16 + g1 as u16) / 2) as u8),
                    (((b0 as u16 + b1 as u16) / 2) as u8),
                    255,
                ];
                colors[3] = [0, 0, 0, 0];
            }
            let idx =
                u32::from_le_bytes([data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= w || y >= h {
                        continue;
                    }
                    let code = (idx >> (2 * (py * 4 + px))) & 0x3;
                    let o = (y * w + x) * 4;
                    out[o..o + 4].copy_from_slice(&colors[code as usize]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb555_white_and_black() {
        // two pixels: 0x7FFF (white, opaque), 0x0000 (black)
        let data = [0xFF, 0x7F, 0x00, 0x00];
        let out = decode_rgba8888(PixelType::Rgb555, 2, 1, &data).unwrap();
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn dxt1_solid_block() {
        // c0=white(0xFFFF) c1=black(0x0000), all indices 0 => all white.
        let mut data = vec![0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let out = decode_rgba8888(PixelType::Dxt1, 4, 4, &data).unwrap();
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        // index 1 (bits for first texel = 01) => black endpoint
        data[4] = 0b0000_0001;
        let out = decode_rgba8888(PixelType::Dxt1, 4, 4, &data).unwrap();
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn short_input_errors() {
        assert!(matches!(
            decode_rgba8888(PixelType::Rgb555, 4, 4, &[0u8; 4]),
            Err(DecodeError::ShortInput { .. })
        ));
    }
}
