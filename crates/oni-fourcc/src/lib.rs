//! A four-character code (4CC) as used by Oni / the Bungie Framework for
//! template tags such as `TXMP`, `SNDD`, `ONCC`.
//!
//! The engine builds a tag with
//! `UUm4CharToUns32(c1,c2,c3,c4) = (c1<<24)|(c2<<16)|(c3<<8)|c4`
//! (see `BFW.h`), i.e. the first character is the most-significant byte of the
//! logical `u32`. [`FourCc`] stores exactly that logical value, so it is
//! independent of how the tag was laid out on disk — the reader in `oni-bytes`
//! decodes the on-disk `u32` in the file's byte order and hands us the logical
//! value directly.

#![forbid(unsafe_code)]

use core::fmt;

/// A four-character code. The wrapped `u32` is the *logical* value where the
/// first character occupies the most-significant byte (matching
/// `UUm4CharToUns32`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FourCc(pub u32);

impl FourCc {
    /// Build a tag from four bytes in reading order (`c1` is the first/most
    /// significant character).
    pub const fn from_bytes(c: [u8; 4]) -> Self {
        FourCc(((c[0] as u32) << 24) | ((c[1] as u32) << 16) | ((c[2] as u32) << 8) | (c[3] as u32))
    }

    /// The four characters in reading order (most significant byte first).
    pub const fn to_bytes(self) -> [u8; 4] {
        [
            (self.0 >> 24) as u8,
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
            self.0 as u8,
        ]
    }

    /// The raw logical `u32` value.
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// True when every character is printable ASCII (a well-formed tag).
    pub fn is_printable(self) -> bool {
        self.to_bytes().iter().all(|&b| (0x20..=0x7e).contains(&b))
    }
}

impl From<[u8; 4]> for FourCc {
    fn from(c: [u8; 4]) -> Self {
        FourCc::from_bytes(c)
    }
}

impl From<u32> for FourCc {
    fn from(v: u32) -> Self {
        FourCc(v)
    }
}

/// Renders the tag as its four characters, replacing any non-printable byte
/// with `.` so output stays readable. Use [`FourCc::to_bytes`] for exact bytes.
impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.to_bytes() {
            let c = if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            };
            f.write_fmt(format_args!("{c}"))?;
        }
        Ok(())
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc({self}:0x{:08X})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_value_matches_engine_macro() {
        // UUm4CharToUns32('V','R','3','1')
        let v = (b'V' as u32) << 24 | (b'R' as u32) << 16 | (b'3' as u32) << 8 | b'1' as u32;
        assert_eq!(FourCc::from_bytes(*b"VR31").as_u32(), v);
        assert_eq!(v, 0x56523331);
    }

    #[test]
    fn roundtrip_and_display() {
        let fc = FourCc::from_bytes(*b"TXMP");
        assert_eq!(fc.to_bytes(), *b"TXMP");
        assert_eq!(fc.to_string(), "TXMP");
        assert!(fc.is_printable());
    }

    #[test]
    fn non_printable_renders_as_dots() {
        let fc = FourCc(0x00_01_02_03);
        assert_eq!(fc.to_string(), "....");
        assert!(!fc.is_printable());
    }
}
