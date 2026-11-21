//! Reading a Bink bitstream: bits are taken lowest first from 32-bit
//! little-endian words, which comes to the same as lowest first from each
//! byte in turn. Reading past the end gives zeros.

pub struct Bits<'a> {
    data: &'a [u8],
    /// The next bit to read, counted from the start.
    pos: usize,
}

impl<'a> Bits<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// How many bits have been read.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// How many bits the data holds.
    pub fn len(&self) -> usize {
        self.data.len() * 8
    }

    /// How many bits are left to read.
    pub fn left(&self) -> isize {
        self.len() as isize - self.pos as isize
    }

    /// The next 32 bits without reading them, the first in the lowest bit.
    pub fn peek32(&self) -> u32 {
        let byte = self.pos >> 3;
        let mut word = [0u8; 8];
        if byte < self.data.len() {
            let end = (byte + 8).min(self.data.len());
            word[..end - byte].copy_from_slice(&self.data[byte..end]);
        }
        (u64::from_le_bytes(word) >> (self.pos & 7)) as u32
    }

    /// Reads `n` bits, at most 32, the first read the lowest.
    pub fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let value = self.peek32();
        self.pos += n as usize;
        if n == 32 {
            value
        } else {
            value & ((1 << n) - 1)
        }
    }

    pub fn bit(&mut self) -> bool {
        self.read(1) != 0
    }

    pub fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    /// Skips to the next multiple of 32 bits.
    pub fn align32(&mut self) {
        self.pos = (self.pos + 31) & !31;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lowest_bits_first() {
        let mut bits = Bits::new(&[0b1010_1101, 0xFF, 0x01]);
        assert_eq!(bits.read(1), 1);
        assert_eq!(bits.read(3), 0b110);
        assert_eq!(bits.read(8), 0xFA);
        assert_eq!(bits.read(5), 0b11111);
        assert_eq!(bits.position(), 17);
        assert_eq!(bits.read(32), 0);
    }
}
