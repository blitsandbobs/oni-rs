//! Oni `TXMP` (texture map) parsing and decoding.
//!
//! A `TXMP` instance's field layout (PC, 32-bit) comes from `BFW_Motoko.h`:
//!
//! ```text
//! offset 0    char      debugName[128]
//! offset 128  u32       flags            (M3cTextureFlags_*)
//! offset 132  u16       width
//! offset 134  u16       height
//! offset 136  u32       texelType        (IMtPixelType)
//! offset 140  tmptr     animation        (TXAN ref, 4 bytes)
//! offset 144  ptr       envMap           (4 bytes)
//! offset 148  rawptr    pixels           (offset into .raw, 0 = none)
//! offset 152  sepidx    pixelStorage     (offset into .sep, 'None' = none)
//! ```
//!
//! On PC (no-sep) builds the pixel data lives in the companion `.raw` at byte
//! offset `pixels`; the texture loader reads `IMrImage_ComputeSize` bytes from
//! there (`Motoko_Texture.c`). All of this is validated byte-for-byte against
//! OniSplit — see the crate tests.

#![forbid(unsafe_code)]

pub mod decode;
pub mod format;

pub use decode::{decode_rgba8888, DecodeError};
pub use format::PixelType;

use oni_dat::{Dat, FourCc, Instance};

/// The `TXMP` template tag.
pub const TAG_TXMP: FourCc = FourCc(0x5458_4D50); // 'TXMP'

// Field offsets within a TXMP instance's data.
const OFF_FLAGS: usize = 128;
const OFF_WIDTH: usize = 132;
const OFF_HEIGHT: usize = 134;
const OFF_TEXEL: usize = 136;
const OFF_PIXELS: usize = 148;
const OFF_PIXELSTORAGE: usize = 152;
const TXMP_MIN_SIZE: usize = 156;

/// `M3cTextureFlags_HasMipMap` — texture stores a full mip chain.
pub const FLAG_HAS_MIPMAP: u32 = 1 << 0;
/// `M3cTextureFlags_Blend_Additive` — drawn with additive blending (glow).
pub const FLAG_BLEND_ADDITIVE: u32 = 1 << 10;
/// `M3cTextureFlags_SelfIlluminent` — full-bright, ignores scene lighting.
pub const FLAG_SELF_ILLUMINENT: u32 = 1 << 11;
/// `M3cTextureFlags_LittleEndian` — pixel data is little-endian.
pub const FLAG_LITTLE_ENDIAN: u32 = 1 << 12;

/// Errors from interpreting a TXMP instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxmpError {
    /// The instance is not a `TXMP`.
    NotTexture(FourCc),
    /// The instance has no data (placeholder) or is too small.
    NoData,
    /// `texelType` was not a known [`PixelType`].
    UnknownFormat(u32),
}

impl std::fmt::Display for TxmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxmpError::NotTexture(t) => write!(f, "instance tag {t} is not TXMP"),
            TxmpError::NoData => write!(f, "TXMP instance has no/short data"),
            TxmpError::UnknownFormat(v) => write!(f, "unknown texelType {v}"),
        }
    }
}

impl std::error::Error for TxmpError {}

/// A parsed texture map.
#[derive(Clone, Debug)]
pub struct TextureMap {
    /// Instance name (without the tag prefix), if any.
    pub name: Option<String>,
    /// Width in texels.
    pub width: u16,
    /// Height in texels.
    pub height: u16,
    /// Pixel format.
    pub format: PixelType,
    /// Raw `M3cTextureFlags_*` bits.
    pub flags: u32,
    /// Byte offset of pixel data in the `.raw` file (0 when the data is in the
    /// separate file instead).
    pub raw_offset: u32,
    /// Byte offset of pixel data in the `.sep` file (`0xFFFFFFFF` = none).
    pub separate_offset: u32,
}

impl TextureMap {
    /// Parses a TXMP instance using its data bytes from `dat`.
    pub fn from_instance(dat: &Dat<'_>, inst: &Instance) -> Result<TextureMap, TxmpError> {
        if inst.tag != TAG_TXMP {
            return Err(TxmpError::NotTexture(inst.tag));
        }
        let d = dat.instance_data(inst).ok_or(TxmpError::NoData)?;
        if d.len() < TXMP_MIN_SIZE {
            return Err(TxmpError::NoData);
        }
        let u16at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
        let u32at = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);

        let texel = u32at(OFF_TEXEL);
        let format = PixelType::from_u32(texel).ok_or(TxmpError::UnknownFormat(texel))?;
        Ok(TextureMap {
            name: inst.name.clone(),
            width: u16at(OFF_WIDTH),
            height: u16at(OFF_HEIGHT),
            format,
            flags: u32at(OFF_FLAGS),
            raw_offset: u32at(OFF_PIXELS),
            separate_offset: u32at(OFF_PIXELSTORAGE),
        })
    }

    /// True when the texture stores a full mipmap chain.
    pub fn has_mipmap(&self) -> bool {
        self.flags & FLAG_HAS_MIPMAP != 0
    }

    /// True when the texture is drawn with additive blending (glowing screens,
    /// energy, fire — `M3cTextureFlags_Blend_Additive`).
    pub fn is_additive(&self) -> bool {
        self.flags & FLAG_BLEND_ADDITIVE != 0
    }

    /// True when the texture is self-illuminated (full-bright, ignores scene
    /// lighting — `M3cTextureFlags_SelfIlluminent`).
    pub fn is_self_illuminated(&self) -> bool {
        self.flags & FLAG_SELF_ILLUMINENT != 0
    }

    /// Total stored size of the pixel data (full mip chain if present), matching
    /// the engine's `IMrImage_ComputeSize`.
    pub fn stored_size(&self) -> u32 {
        self.format
            .compute_size(self.has_mipmap(), self.width as u32, self.height as u32)
    }

    /// Size of just the top (largest) mip level.
    pub fn top_level_size(&self) -> u32 {
        self.format
            .level_size(self.width as u32, self.height as u32)
    }

    /// Borrows the full pixel-data region (all mip levels) from the `.raw`
    /// buffer, or `None` if the data is not in `.raw` or is out of range.
    pub fn raw_pixels<'a>(&self, raw: &'a [u8]) -> Option<&'a [u8]> {
        if self.raw_offset == 0 {
            return None;
        }
        let start = self.raw_offset as usize;
        let end = start.checked_add(self.stored_size() as usize)?;
        raw.get(start..end)
    }

    /// Borrows the pixel-data region from the `.sep` separate file (retail PC /
    /// Mac CDs), where `pixelStorage` is a byte offset. `0` means "none".
    pub fn sep_pixels<'a>(&self, sep: &'a [u8]) -> Option<&'a [u8]> {
        if self.separate_offset == 0 {
            return None;
        }
        let start = self.separate_offset as usize;
        let end = start.checked_add(self.stored_size() as usize)?;
        sep.get(start..end)
    }

    /// Returns the pixel data from whichever source holds it: `.raw` (no-sep PC,
    /// via `pixels`) or the `.sep` file (retail PC / Mac, via `pixelStorage`).
    pub fn pixels<'a>(&self, raw: &'a [u8], sep: Option<&'a [u8]>) -> Option<&'a [u8]> {
        self.raw_pixels(raw)
            .or_else(|| sep.and_then(|s| self.sep_pixels(s)))
    }

    /// Decodes the top mip level to RGBA8888 from `.raw`, falling back to `.sep`.
    pub fn decode_top_level_with(
        &self,
        raw: &[u8],
        sep: Option<&[u8]>,
    ) -> Result<Vec<u8>, DecodeError> {
        let pixels = self
            .pixels(raw, sep)
            .ok_or(DecodeError::ShortInput { need: 1, got: 0 })?;
        decode_rgba8888(self.format, self.width as u32, self.height as u32, pixels)
    }

    /// Decodes the top mip level to RGBA8888 using pixel data from `.raw`.
    pub fn decode_top_level(&self, raw: &[u8]) -> Result<Vec<u8>, DecodeError> {
        self.decode_top_level_with(raw, None)
    }
}

/// Iterates every TXMP texture in `dat` that has pixel data in `.raw`.
pub fn textures<'a>(dat: &'a Dat<'a>) -> impl Iterator<Item = TextureMap> + 'a {
    dat.instances
        .iter()
        .filter(|i| i.tag == TAG_TXMP)
        .filter_map(move |i| TextureMap::from_instance(dat, i).ok())
}
