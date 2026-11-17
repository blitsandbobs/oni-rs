//! A tiny, dependency-free, endian-aware cursor over a byte slice.
//!
//! Oni ships the same logical data in little-endian (PC) and big-endian
//! (Mac/PowerPC) form, so every multi-byte read is parameterised by an
//! [`Endian`] carried on the [`Reader`]. All reads are bounds-checked and
//! return [`Result`]; a parser never panics on malformed input.

#![forbid(unsafe_code)]

use core::fmt;

/// Byte order of the data being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    /// Little-endian: PC builds (and OniSplit's default output).
    Little,
    /// Big-endian: Mac / PowerPC builds.
    Big,
}

/// Error from a failed read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Wanted `need` bytes at `at`, but only `len` bytes exist in total.
    OutOfBounds { at: usize, need: usize, len: usize },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::OutOfBounds { at, need, len } => write!(
                f,
                "read of {need} byte(s) at offset {at} exceeds buffer length {len}"
            ),
        }
    }
}

impl std::error::Error for ReadError {}

/// Alias for results produced by this crate.
pub type Result<T> = core::result::Result<T, ReadError>;

/// A forward/seekable cursor over a borrowed byte slice.
#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    endian: Endian,
}

macro_rules! read_int {
    ($name:ident, $ty:ty, $n:literal) => {
        /// Reads one value in the reader's endianness and advances the cursor.
        pub fn $name(&mut self) -> Result<$ty> {
            let bytes = self.take($n)?;
            let arr: [u8; $n] = bytes.try_into().expect("slice length checked by take()");
            Ok(match self.endian {
                Endian::Little => <$ty>::from_le_bytes(arr),
                Endian::Big => <$ty>::from_be_bytes(arr),
            })
        }
    };
}

impl<'a> Reader<'a> {
    /// Creates a reader positioned at offset 0.
    pub fn new(data: &'a [u8], endian: Endian) -> Self {
        Reader {
            data,
            pos: 0,
            endian,
        }
    }

    /// The configured byte order.
    pub fn endian(&self) -> Endian {
        self.endian
    }

    /// Current cursor offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Total length of the underlying buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// True when the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Bytes between the cursor and the end of the buffer.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Moves the cursor to an absolute offset. Seeking to `len()` (one past the
    /// end) is allowed; beyond it is an error.
    pub fn seek(&mut self, to: usize) -> Result<()> {
        if to > self.data.len() {
            return Err(ReadError::OutOfBounds {
                at: to,
                need: 0,
                len: self.data.len(),
            });
        }
        self.pos = to;
        Ok(())
    }

    /// Advances the cursor by `n` bytes.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    /// Returns the next `n` bytes and advances the cursor.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(ReadError::OutOfBounds {
            at: self.pos,
            need: n,
            len: self.data.len(),
        })?;
        if end > self.data.len() {
            return Err(ReadError::OutOfBounds {
                at: self.pos,
                need: n,
                len: self.data.len(),
            });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    read_int!(u8, u8, 1);
    read_int!(u16, u16, 2);
    read_int!(u32, u32, 4);
    read_int!(u64, u64, 8);
    read_int!(i8, i8, 1);
    read_int!(i16, i16, 2);
    read_int!(i32, i32, 4);

    /// Reads a NUL-terminated string starting at absolute offset `at` from the
    /// underlying buffer (does not move the cursor). The bytes are interpreted
    /// as Latin-1, which is lossless for Oni's ASCII asset names and never
    /// fails. Returns the string up to, but excluding, the terminating NUL; if
    /// no NUL is found, returns the remainder of the buffer.
    pub fn cstr_at(&self, at: usize) -> Result<String> {
        if at > self.data.len() {
            return Err(ReadError::OutOfBounds {
                at,
                need: 1,
                len: self.data.len(),
            });
        }
        let rest = &self.data[at..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(rest[..end].iter().map(|&b| b as char).collect())
    }

    /// Borrows the underlying buffer.
    pub fn buffer(&self) -> &'a [u8] {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_endians() {
        let bytes = [0x31, 0x33, 0x52, 0x56]; // 'VR31' stored little-endian
        let mut le = Reader::new(&bytes, Endian::Little);
        assert_eq!(le.u32().unwrap(), 0x56523331);
        let mut be = Reader::new(&bytes, Endian::Big);
        assert_eq!(be.u32().unwrap(), 0x31335256);
    }

    #[test]
    fn bounds_checked() {
        let mut r = Reader::new(&[0, 1, 2], Endian::Little);
        assert!(r.u16().is_ok());
        assert!(matches!(r.u32(), Err(ReadError::OutOfBounds { .. })));
    }

    #[test]
    fn cstr_reads_until_nul() {
        let buf = b"hello\0world\0";
        let r = Reader::new(buf, Endian::Little);
        assert_eq!(r.cstr_at(0).unwrap(), "hello");
        assert_eq!(r.cstr_at(6).unwrap(), "world");
        assert!(r.cstr_at(999).is_err());
    }

    #[test]
    fn seek_to_end_ok_but_not_past() {
        let mut r = Reader::new(&[0, 1, 2, 3], Endian::Little);
        assert!(r.seek(4).is_ok());
        assert_eq!(r.remaining(), 0);
        assert!(r.seek(5).is_err());
    }
}
