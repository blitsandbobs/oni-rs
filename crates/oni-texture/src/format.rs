//! Oni pixel formats (`IMtPixelType`) and the storage-size formula.
//!
//! Ordinals and the size math are taken verbatim from `BFW_Image.h`
//! (`IMtPixelType`) and `BFW_Image.c` (`IMrImage_ComputeSize`).

/// A texture pixel format. Discriminants match the engine's `IMtPixelType`
/// enum (declared with `tm_enum`, stored as a little-endian `u32` on disk).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PixelType {
    /// 16-bit A4R4G4B4.
    Argb4444 = 0,
    /// 16-bit 0R5G5B5 (opaque).
    Rgb555 = 1,
    /// 16-bit A1R5G5B5.
    Argb1555 = 2,
    /// 8-bit intensity (grey).
    I8 = 3,
    /// 1-bit intensity (8 pixels/byte).
    I1 = 4,
    /// 8-bit alpha only.
    A8 = 5,
    /// 8-bit: 4-bit alpha + 4-bit intensity.
    A4i4 = 6,
    /// 32-bit A8R8G8B8.
    Argb8888 = 7,
    /// 32-bit X8R8G8B8 (stored 4 bytes/pixel; "RGB888").
    Rgb888 = 8,
    /// DXT1 / BC1 block compression (S3TC), opaque or 1-bit alpha.
    Dxt1 = 9,
    /// Byte-packed RGB (OpenGL path).
    RgbBytes = 10,
    /// Byte-packed RGBA (OpenGL path).
    RgbaBytes = 11,
    /// 16-bit R5G5B5A1 (OpenGL).
    Rgba5551 = 12,
    /// 16-bit R4G4B4A4 (OpenGL).
    Rgba4444 = 13,
    /// 16-bit R5G6B5.
    Rgb565 = 14,
    /// 16-bit A1B5G5R5.
    Abgr1555 = 15,
}

impl PixelType {
    /// Maps the on-disk `u32` ordinal to a [`PixelType`].
    pub fn from_u32(v: u32) -> Option<PixelType> {
        use PixelType::*;
        Some(match v {
            0 => Argb4444,
            1 => Rgb555,
            2 => Argb1555,
            3 => I8,
            4 => I1,
            5 => A8,
            6 => A4i4,
            7 => Argb8888,
            8 => Rgb888,
            9 => Dxt1,
            10 => RgbBytes,
            11 => RgbaBytes,
            12 => Rgba5551,
            13 => Rgba4444,
            14 => Rgb565,
            15 => Abgr1555,
            _ => return None,
        })
    }

    /// Bytes per pixel, or 0 for block/sub-byte formats (`Dxt1`, `I1`) whose
    /// size is computed specially. Mirrors `IMrPixel_GetSize`.
    pub fn bytes_per_pixel(self) -> u8 {
        use PixelType::*;
        match self {
            Argb8888 | Rgb888 | RgbaBytes => 4,
            RgbBytes => 3,
            Argb4444 | Rgb555 | Argb1555 | Rgba5551 | Rgba4444 | Rgb565 | Abgr1555 => 2,
            I8 | A8 | A4i4 => 1,
            Dxt1 | I1 => 0,
        }
    }

    /// Storage size in bytes for a single mip level of `width`×`height`.
    /// Matches the non-mipmap branch of `IMrImage_ComputeSize`.
    pub fn level_size(self, width: u32, height: u32) -> u32 {
        if width == 0 || height == 0 {
            return 0;
        }
        match self.bytes_per_pixel() {
            4 => width * height * 4,
            2 => width * height * 2,
            1 => ((width + 1) & !1) * height, // rows padded to 2 bytes
            _ => match self {
                // DXT1: 8 bytes per 4×4 block.
                PixelType::Dxt1 => (((width + 3) & !3) * ((height + 3) & !3) * 2) >> 2,
                // I1: 8 pixels/byte, rows padded to 2 bytes.
                PixelType::I1 => (((width + 15) & !15) >> 3) * height,
                _ => 0,
            },
        }
    }

    /// Total storage size for a texture, optionally including the full mipmap
    /// chain down to 1×1. Matches `IMrImage_ComputeSize`.
    pub fn compute_size(self, has_mipmap: bool, width: u32, height: u32) -> u32 {
        let mut size = self.level_size(width, height);
        if has_mipmap && size != 0 {
            let (mut nw, mut nh) = (width >> 1, height >> 1);
            if nw > 0 || nh > 0 {
                nw = nw.max(1);
                nh = nh.max(1);
                size += self.compute_size(true, nw, nh);
            }
        }
        size
    }
}

#[cfg(test)]
mod tests {
    use super::PixelType::*;

    #[test]
    fn dxt1_block_size() {
        // 64×64 DXT1, no mipmap = 2048 bytes (256 blocks × 8).
        assert_eq!(Dxt1.level_size(64, 64), 2048);
        // 4×4 = one block.
        assert_eq!(Dxt1.level_size(4, 4), 8);
        assert_eq!(Dxt1.level_size(1, 1), 8);
    }

    #[test]
    fn mipmap_chain_to_1x1() {
        // 64×64 DXT1 full chain: 2048+512+128+32+8+8+8 = 2744 (engine goes to 1×1).
        assert_eq!(Dxt1.compute_size(true, 64, 64), 2744);
    }

    #[test]
    fn sixteen_bit_sizes() {
        assert_eq!(Argb4444.level_size(128, 64), 128 * 64 * 2);
        assert_eq!(Rgb555.compute_size(false, 32, 32), 32 * 32 * 2);
    }
}
