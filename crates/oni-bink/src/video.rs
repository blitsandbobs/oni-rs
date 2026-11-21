//! Bink 1 video, revisions 'f' to 'i' (the game's are all 'i'): a frame
//! decoded to Y, U and V planes (4:2:0).
//!
//! Our own decoder, written from the format's public descriptions; only
//! the constant tables (`super::tables`) are taken from FFmpeg. Which
//! description each part follows:
//!
//! - Planes, Huffman trees chosen per plane, and the tree's symbol order
//!   (explicit, or pairs swapped and merged to a depth): the MultimediaWiki
//!   page "Bink Video" (sections "Huffman trees" and "Bundle reading").
//! - The nine bundles, their kinds (4-bit RLE, 4-bit signed, 8-bit with
//!   the high nibble's tree chosen by the last high nibble, pairs of
//!   nibbles, 16-bit deltas in groups of eight) and their order: the same
//!   page; the refill of a bundle before a row of blocks only once all
//!   it held is used: Kostya Shishkov, "Bink bitstream bundling"
//!   (codecs.multimedia.cx, 2023-10).
//! - The block types 0 to 9 and what each does: the wiki page ("Block
//!   types"), and Kostya, "Bink encoding: format and encoder designs"
//!   (2023-10): the previous frame is a separate picture from 'f' on.
//! - DCT coefficients and residue coded by a tree of nodes over the scan
//!   order, a pass per bit width from the top, nodes expanded into a list
//!   with single coefficients put in front: Kostya, "Bink encoder:
//!   coefficients coding" (2023-10).
//! - The IDCT: the Arai-Agui-Nakajima factorisation, as the IJG's
//!   `jidctfst.c` lays it out, with the factors in 11-bit fixed point
//!   (the transform's matrix is in Kostya, "Bink encoder: doing DCT",
//!   2023-10, and it uses 12-bit constants).
//!
//! What the write-ups leave out (the widths of a bundle refill's count,
//! where the quantiser comes in a DCT block, the IDCT's rounding, the
//! order of the chroma planes) is as our output, held against FFmpeg's
//! decoding of every one of the game's movies, shows it
//! (`crates/ra2-formats/tests/bink.rs`, `docs/original/movies.md`).

use super::bits::Bits;
use super::tables::{INTER_QUANT, INTRA_QUANT, PATTERNS, SCAN, TREE_CODES, TREE_LENGTHS};
use super::BinkError;

/// The bundles, in the order their trees are read in.
const BLOCK_TYPES: usize = 0;
const SUB_BLOCK_TYPES: usize = 1;
const COLORS: usize = 2;
const PATTERN: usize = 3;
const X_OFF: usize = 4;
const Y_OFF: usize = 5;
const INTRA_DC: usize = 6;
const INTER_DC: usize = 7;
const RUN: usize = 8;

/// The block types.
const SKIP: i32 = 0;
const SCALED: i32 = 1;
const MOTION: i32 = 2;
const RUN_FILL: i32 = 3;
const RESIDUE: i32 = 4;
const INTRA: i32 = 5;
const FILL: i32 = 6;
const INTER: i32 = 7;
const PATTERN_FILL: i32 = 8;
const RAW: i32 = 9;

/// A run-length coded block type's runs, for the codes 12 to 15.
const RUN_LENGTHS: [usize; 4] = [4, 8, 12, 32];

/// One plane of a picture, padded to whole blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plane {
    /// The visible size.
    pub width: usize,
    pub height: usize,
    /// Bytes from one row to the next.
    pub stride: usize,
    pub data: Vec<u8>,
}

impl Plane {
    fn new(width: usize, height: usize, padded_width: usize, padded_height: usize) -> Self {
        Self {
            width,
            height,
            stride: padded_width,
            data: vec![0; padded_width * padded_height],
        }
    }

    /// The visible rows.
    pub fn rows(&self) -> impl Iterator<Item = &[u8]> {
        self.data
            .chunks(self.stride)
            .take(self.height)
            .map(|row| &row[..self.width])
    }
}

/// A decoded picture: Y at full size, U and V at half.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// Y, U, V.
    pub planes: [Plane; 3],
}

impl Picture {
    fn new(width: usize, height: usize) -> Self {
        let luma = Plane::new(width, height, (width + 7) & !7, (height + 7) & !7);
        let chroma = || {
            Plane::new(
                width.div_ceil(2),
                height.div_ceil(2),
                ((width + 15) & !15) / 2,
                ((height + 15) & !15) / 2,
            )
        };
        Self {
            width,
            height,
            planes: [luma, chroma(), chroma()],
        }
    }

    /// The picture as RGB, three bytes a pixel, row by row, converted as
    /// BT.601 with Y from 16 to 235.
    pub fn to_rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(self.width * self.height * 3);
        let [y, u, v] = &self.planes;
        let at = |plane: &Plane, x: usize, y: usize| i32::from(plane.data[y * plane.stride + x]);
        for row in 0..self.height {
            for column in 0..self.width {
                let luma = at(y, column, row) - 16;
                let cb = at(u, column / 2, row / 2) - 128;
                let cr = at(v, column / 2, row / 2) - 128;
                // 1.164, 1.596, 0.391, 0.813, 2.018 in 16-bit fixed point.
                let l = 76284 * luma + 32768;
                let r = (l + 104595 * cr) >> 16;
                let g = (l - 25624 * cb - 53280 * cr) >> 16;
                let b = (l + 132252 * cb) >> 16;
                rgb.extend([r, g, b].map(|c| c.clamp(0, 255) as u8));
            }
        }
        rgb
    }
}

/// A Huffman tree as a plane chooses it: one of the 16 fixed ones, and
/// which symbol each of its codes stands for.
#[derive(Clone, Copy, Default)]
struct Tree {
    number: usize,
    symbols: [u8; 16],
}

/// The fixed trees as tables of the next 7 bits: the code's index and
/// length.
struct Codebooks([[(u8, u8); 128]; 16]);

impl Codebooks {
    fn new() -> Self {
        let mut books = [[(0u8, 0u8); 128]; 16];
        for (book, (codes, lengths)) in books.iter_mut().zip(TREE_CODES.iter().zip(&TREE_LENGTHS)) {
            for index in 0..16 {
                let (code, length) = (usize::from(codes[index]), lengths[index]);
                for high in 0..1usize << (7 - length) {
                    book[code | high << length] = (index as u8, length);
                }
            }
        }
        Self(books)
    }

    fn read(&self, bits: &mut Bits, tree: &Tree) -> u8 {
        let (index, length) = self.0[tree.number][(bits.peek32() & 127) as usize];
        bits.skip(usize::from(length));
        tree.symbols[usize::from(index)]
    }
}

/// Reads which tree, and the order of its symbols.
fn read_tree(bits: &mut Bits) -> Tree {
    let mut tree = Tree {
        number: bits.read(4) as usize,
        symbols: std::array::from_fn(|i| i as u8),
    };
    if tree.number == 0 {
        return tree;
    }
    if bits.bit() {
        // Some symbols first, the rest after them in order.
        let count = bits.read(3) as usize + 1;
        let mut used = [false; 16];
        let mut order = Vec::with_capacity(16 + count);
        for _ in 0..count {
            let symbol = bits.read(4) as u8;
            order.push(symbol);
            used[usize::from(symbol)] = true;
        }
        order.extend((0..16u8).filter(|&s| !used[usize::from(s)]));
        tree.symbols.copy_from_slice(&order[..16]);
    } else {
        // Runs of 1, 2, 4 and 8 merged in pairs, as deep as asked.
        let depth = bits.read(2) as usize;
        let mut symbols = tree.symbols;
        for level in 0..=depth {
            let size = 1 << level;
            let mut merged = [0u8; 16];
            for start in (0..16).step_by(2 * size) {
                let (mut a, mut b) = (start, start + size);
                let (a_end, b_end) = (start + size, start + 2 * size);
                let mut out = start;
                while a < a_end && b < b_end {
                    if bits.bit() {
                        merged[out] = symbols[b];
                        b += 1;
                    } else {
                        merged[out] = symbols[a];
                        a += 1;
                    }
                    out += 1;
                }
                for &s in symbols[a..a_end].iter().chain(&symbols[b..b_end]) {
                    merged[out] = s;
                    out += 1;
                }
            }
            symbols = merged;
        }
        tree.symbols = symbols;
    }
    tree
}

/// One of a plane's streams of values.
#[derive(Default)]
struct Bundle {
    /// How many bits the count of a refill has.
    count_bits: u32,
    tree: Tree,
    values: Vec<i32>,
    /// The next value to use.
    next: usize,
    /// A refill of none ends the bundle for the plane.
    ended: bool,
}

impl Bundle {
    /// Whether it is time to refill: all it held is used.
    fn wants(&self) -> bool {
        !self.ended && self.next >= self.values.len()
    }

    /// The count of a refill, or `None` (and the bundle ended) for none.
    fn count(&mut self, bits: &mut Bits) -> Option<usize> {
        let count = bits.read(self.count_bits) as usize;
        if count == 0 {
            self.ended = true;
            return None;
        }
        Some(count)
    }

    fn take(&mut self) -> Result<i32, BinkError> {
        let value = *self
            .values
            .get(self.next)
            .ok_or(BinkError::Malformed("a bundle ran out"))?;
        self.next += 1;
        Ok(value)
    }
}

/// The highest bit set: floor of log2.
fn log2(n: usize) -> u32 {
    n.ilog2()
}

pub struct VideoDecoder {
    revision: u8,
    width: usize,
    height: usize,
    books: Codebooks,
    /// The picture being decoded, and the last one.
    current: Picture,
    last: Picture,
}

impl VideoDecoder {
    pub fn new(revision: u8, width: u32, height: u32, alpha: bool) -> Result<Self, BinkError> {
        if revision < b'f' {
            return Err(BinkError::Unsupported("video before revision 'f'"));
        }
        if alpha {
            return Err(BinkError::Unsupported("an alpha plane"));
        }
        let (width, height) = (width as usize, height as usize);
        Ok(Self {
            revision,
            width,
            height,
            books: Codebooks::new(),
            current: Picture::new(width, height),
            last: Picture::new(width, height),
        })
    }

    /// Decodes the next frame's video packet.
    pub fn decode(&mut self, packet: &[u8]) -> Result<&Picture, BinkError> {
        let mut bits = Bits::new(packet);
        if self.revision >= b'i' {
            // The size of the luma plane's data.
            bits.skip(32);
        }
        self.current.clone_from(&self.last);
        for plane in 0..3 {
            // From revision 'h' the chroma planes come V first.
            let index = if plane == 0 || self.revision < b'h' {
                plane
            } else {
                plane ^ 3
            };
            self.decode_plane(&mut bits, index)?;
            if bits.position() >= bits.len() {
                break;
            }
        }
        std::mem::swap(&mut self.current, &mut self.last);
        Ok(&self.last)
    }

    fn decode_plane(&mut self, bits: &mut Bits, index: usize) -> Result<(), BinkError> {
        let chroma = index != 0;
        let (bw, bh) = if chroma {
            ((self.width + 15) >> 4, (self.height + 15) >> 4)
        } else {
            ((self.width + 7) >> 3, (self.height + 7) >> 3)
        };
        let width = (self.width >> usize::from(chroma)).max(8);
        let mut bundles: [Bundle; 9] = Default::default();
        let wide = log2((width >> 3) + 511) + 1;
        let counts = [
            wide,
            log2((width >> 4) + 511) + 1,
            log2(bw * 64 + 511) + 1,
            log2((bw << 3) + 511) + 1,
            wide,
            wide,
            wide,
            wide,
            log2(bw * 48 + 511) + 1,
        ];
        for (bundle, count) in bundles.iter_mut().zip(counts) {
            bundle.count_bits = count;
        }
        let mut color_high = [Tree::default(); 16];
        for (number, bundle) in bundles.iter_mut().enumerate() {
            if number == INTRA_DC || number == INTER_DC {
                continue;
            }
            if number == COLORS {
                for tree in &mut color_high {
                    *tree = read_tree(bits);
                }
            }
            bundle.tree = read_tree(bits);
        }
        let mut last_high = 0usize;

        let stride = self.current.planes[index].stride;
        for by in 0..bh {
            self.read_block_types(bits, &mut bundles[BLOCK_TYPES]);
            self.read_block_types(bits, &mut bundles[SUB_BLOCK_TYPES]);
            self.read_colors(bits, &mut bundles[COLORS], &color_high, &mut last_high);
            self.read_patterns(bits, &mut bundles[PATTERN]);
            self.read_motion(bits, &mut bundles[X_OFF]);
            self.read_motion(bits, &mut bundles[Y_OFF]);
            read_dcs(bits, &mut bundles[INTRA_DC], false)?;
            read_dcs(bits, &mut bundles[INTER_DC], true)?;
            self.read_runs(bits, &mut bundles[RUN]);

            let mut bx = 0;
            while bx < bw {
                let at = by * 8 * stride + bx * 8;
                let kind = bundles[BLOCK_TYPES].take()?;
                // The second row of a scaled block's: decoded with it.
                if by & 1 == 1 && kind == SCALED {
                    bx += 2;
                    continue;
                }
                let current = &mut self.current.planes[index].data;
                let last = &self.last.planes[index].data;
                match kind {
                    SKIP => copy_block(current, at, last, at, stride),
                    SCALED => {
                        if bx + 1 >= bw || by + 1 >= bh {
                            return Err(BinkError::Malformed("a scaled block at the edge"));
                        }
                        let sub = bundles[SUB_BLOCK_TYPES].take()?;
                        let mut small = [0u8; 64];
                        match sub {
                            FILL => small = [bundles[COLORS].take()? as u8; 64],
                            _ => decode_intra_kind(bits, &mut bundles, sub, &mut small, 0, 8)?,
                        }
                        for y in 0..16 {
                            for x in 0..16 {
                                current[at + y * stride + x] = small[(y / 2) * 8 + x / 2];
                            }
                        }
                        bx += 1;
                    }
                    MOTION | RESIDUE | INTER => {
                        let dx = bundles[X_OFF].take()? as isize;
                        let dy = bundles[Y_OFF].take()? as isize;
                        let (x, y) = (bx as isize * 8 + dx, by as isize * 8 + dy);
                        if x < 0 || y < 0 || x as usize > (bw - 1) * 8 || y as usize > (bh - 1) * 8
                        {
                            return Err(BinkError::Malformed("motion out of the plane"));
                        }
                        let from = y as usize * stride + x as usize;
                        copy_block(current, at, last, from, stride);
                        if kind == RESIDUE {
                            let masks = bits.read(7) as i32;
                            let mut block = [0i32; 64];
                            read_residue(bits, &mut block, masks);
                            add_block(current, at, stride, &block);
                        } else if kind == INTER {
                            let mut block = [0i32; 64];
                            block[0] = bundles[INTER_DC].take()?;
                            read_dct(bits, &mut block, &INTER_QUANT);
                            add_block(current, at, stride, &idct(&block));
                        }
                    }
                    FILL => {
                        let value = bundles[COLORS].take()? as u8;
                        for y in 0..8 {
                            current[at + y * stride..][..8].fill(value);
                        }
                    }
                    RUN_FILL | INTRA | PATTERN_FILL | RAW => {
                        decode_intra_kind(bits, &mut bundles, kind, current, at, stride)?
                    }
                    _ => return Err(BinkError::Malformed("block type")),
                }
                bx += 1;
            }
        }
        bits.align32();
        Ok(())
    }

    /// Block types: a run of one value, or Huffman-coded types with codes
    /// 12 to 15 repeating the last.
    fn read_block_types(&self, bits: &mut Bits, bundle: &mut Bundle) {
        if !bundle.wants() {
            return;
        }
        let Some(count) = bundle.count(bits) else {
            return;
        };
        if bits.bit() {
            let value = bits.read(4) as i32;
            bundle.values.extend(std::iter::repeat_n(value, count));
            return;
        }
        let end = bundle.values.len() + count;
        let mut last = 0;
        while bundle.values.len() < end {
            let value = self.books.read(bits, &bundle.tree);
            if value < 12 {
                last = i32::from(value);
                bundle.values.push(last);
            } else {
                let run = RUN_LENGTHS[usize::from(value - 12)];
                bundle.values.extend(std::iter::repeat_n(last, run));
            }
        }
    }

    /// Colours: a high nibble by the tree the last high nibble chooses, then
    /// a low one; all of a refill the same if its first bit is set.
    fn read_colors(
        &self,
        bits: &mut Bits,
        bundle: &mut Bundle,
        high: &[Tree; 16],
        last: &mut usize,
    ) {
        if !bundle.wants() {
            return;
        }
        let Some(count) = bundle.count(bits) else {
            return;
        };
        let tree = bundle.tree;
        let mut one = |bits: &mut Bits| {
            *last = usize::from(self.books.read(bits, &high[*last]));
            let low = self.books.read(bits, &tree);
            let value = (*last as i32) << 4 | i32::from(low);
            if self.revision < b'i' {
                // Before 'i' a colour is a sign and magnitude about 128.
                let magnitude = value & 0x7F;
                (if value & 0x80 != 0 {
                    -magnitude
                } else {
                    magnitude
                } + 128)
                    & 0xFF
            } else {
                value
            }
        };
        if bits.bit() {
            let value = one(bits);
            bundle.values.extend(std::iter::repeat_n(value, count));
        } else {
            for _ in 0..count {
                let value = one(bits);
                bundle.values.push(value);
            }
        }
    }

    /// Patterns: two nibbles a byte, low first.
    fn read_patterns(&self, bits: &mut Bits, bundle: &mut Bundle) {
        if !bundle.wants() {
            return;
        }
        let Some(count) = bundle.count(bits) else {
            return;
        };
        for _ in 0..count {
            let low = self.books.read(bits, &bundle.tree);
            let high = self.books.read(bits, &bundle.tree);
            bundle.values.push(i32::from(low | high << 4));
        }
    }

    /// Motion: a nibble with a sign bit after it unless 0.
    fn read_motion(&self, bits: &mut Bits, bundle: &mut Bundle) {
        if !bundle.wants() {
            return;
        }
        let Some(count) = bundle.count(bits) else {
            return;
        };
        let signed = |bits: &mut Bits, value: i32| {
            if value != 0 && bits.bit() {
                -value
            } else {
                value
            }
        };
        if bits.bit() {
            let value = bits.read(4) as i32;
            let value = signed(bits, value);
            bundle.values.extend(std::iter::repeat_n(value, count));
        } else {
            for _ in 0..count {
                let value = i32::from(self.books.read(bits, &bundle.tree));
                let value = signed(bits, value);
                bundle.values.push(value);
            }
        }
    }

    /// Run lengths: nibbles, all the same if the first bit is set.
    fn read_runs(&self, bits: &mut Bits, bundle: &mut Bundle) {
        if !bundle.wants() {
            return;
        }
        let Some(count) = bundle.count(bits) else {
            return;
        };
        if bits.bit() {
            let value = bits.read(4) as i32;
            bundle.values.extend(std::iter::repeat_n(value, count));
        } else {
            for _ in 0..count {
                let value = i32::from(self.books.read(bits, &bundle.tree));
                bundle.values.push(value);
            }
        }
    }
}

/// Decodes a block of one of the kinds a scaled block may also be: run,
/// intra DCT, pattern or raw.
fn decode_intra_kind(
    bits: &mut Bits,
    bundles: &mut [Bundle; 9],
    kind: i32,
    out: &mut [u8],
    at: usize,
    stride: usize,
) -> Result<(), BinkError> {
    let pixel = |i: usize| at + (i / 8) * stride + i % 8;
    match kind {
        RUN_FILL => {
            let pattern = &PATTERNS[bits.read(4) as usize];
            let mut done = 0;
            while done < 63 {
                let run = bundles[RUN].take()? as usize + 1;
                if done + run > 64 {
                    return Err(BinkError::Malformed("a run past the block"));
                }
                if bits.bit() {
                    let value = bundles[COLORS].take()? as u8;
                    for &p in &pattern[done..done + run] {
                        out[pixel(usize::from(p))] = value;
                    }
                } else {
                    for &p in &pattern[done..done + run] {
                        out[pixel(usize::from(p))] = bundles[COLORS].take()? as u8;
                    }
                }
                done += run;
            }
            if done == 63 {
                out[pixel(usize::from(pattern[63]))] = bundles[COLORS].take()? as u8;
            }
        }
        INTRA => {
            let mut block = [0i32; 64];
            block[0] = bundles[INTRA_DC].take()?;
            read_dct(bits, &mut block, &INTRA_QUANT);
            for (i, value) in idct(&block).iter().enumerate() {
                out[pixel(i)] = *value as u8;
            }
        }
        PATTERN_FILL => {
            let colors = [bundles[COLORS].take()? as u8, bundles[COLORS].take()? as u8];
            for y in 0..8 {
                let mut mask = bundles[PATTERN].take()?;
                for x in 0..8 {
                    out[pixel(y * 8 + x)] = colors[(mask & 1) as usize];
                    mask >>= 1;
                }
            }
        }
        RAW => {
            for i in 0..64 {
                out[pixel(i)] = bundles[COLORS].take()? as u8;
            }
        }
        _ => return Err(BinkError::Malformed("sub-block type")),
    }
    Ok(())
}

/// DC values: a first one of 11 bits (10 and a sign for inter blocks), then
/// differences in groups of up to eight, each group with its own width.
fn read_dcs(bits: &mut Bits, bundle: &mut Bundle, signed: bool) -> Result<(), BinkError> {
    if !bundle.wants() {
        return Ok(());
    }
    let Some(count) = bundle.count(bits) else {
        return Ok(());
    };
    let read_signed = |bits: &mut Bits, width: u32| {
        let value = bits.read(width) as i32;
        if value != 0 && bits.bit() {
            -value
        } else {
            value
        }
    };
    let mut value = if signed {
        read_signed(bits, 10)
    } else {
        bits.read(11) as i32
    };
    bundle.values.push(value);
    let rest = count - 1;
    let mut done = 0;
    while done < rest {
        let group = (rest - done).min(8);
        let width = bits.read(4);
        for _ in 0..group {
            if width != 0 {
                value += read_signed(bits, width);
                if !(-32768..=32767).contains(&value) {
                    return Err(BinkError::Malformed("a DC out of range"));
                }
            }
            bundle.values.push(value);
        }
        done += 8;
    }
    Ok(())
}

/// An entry of the coefficient tree's list: a node of 20 coefficients
/// (four, then a node of the 16 after them), a node of 16 (four nodes of
/// four), a node of four, or one coefficient; by its first coefficient in
/// scan order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    Twenty(usize),
    Sixteen(usize),
    Four(usize),
    One(usize),
}

/// The entries still to visit, `start..end`, with room before `start` for
/// the single coefficients put in front. A used entry is `None`.
struct List {
    entries: [Option<Entry>; 128],
    start: usize,
    end: usize,
}

impl List {
    fn new(first: &[Entry]) -> Self {
        let mut list = Self {
            entries: [None; 128],
            start: 64,
            end: 64,
        };
        for &entry in first {
            list.push_back(entry);
        }
        list
    }

    fn push_back(&mut self, entry: Entry) {
        self.entries[self.end] = Some(entry);
        self.end += 1;
    }

    fn push_front(&mut self, entry: Entry) {
        self.start -= 1;
        self.entries[self.start] = Some(entry);
    }

    /// One pass over the list for one bit width, each entry with a bit
    /// saying whether it is used now: a node used is expanded (its four
    /// coefficients each either coded now or put in front, and the rest
    /// of a larger node kept or added at the end for this same pass), a
    /// coefficient used is coded. `code` codes a coefficient by its scan
    /// position and says whether to stop.
    fn pass(&mut self, bits: &mut Bits, mut code: impl FnMut(&mut Bits, usize) -> bool) -> bool {
        let mut pos = self.start;
        while pos < self.end {
            let Some(entry) = self.entries[pos] else {
                pos += 1;
                continue;
            };
            if !bits.bit() {
                pos += 1;
                continue;
            }
            match entry {
                Entry::Twenty(first) | Entry::Four(first) => {
                    if let Entry::Twenty(_) = entry {
                        // The node goes on as the 16 after these four,
                        // asked about again at once.
                        self.entries[pos] = Some(Entry::Sixteen(first + 4));
                    } else {
                        self.entries[pos] = None;
                        pos += 1;
                    }
                    for coefficient in first..first + 4 {
                        if bits.bit() {
                            self.push_front(Entry::One(coefficient));
                        } else if code(bits, coefficient) {
                            return true;
                        }
                    }
                }
                Entry::Sixteen(first) => {
                    self.entries[pos] = Some(Entry::Four(first));
                    for i in 1..4 {
                        self.push_back(Entry::Four(first + 4 * i));
                    }
                }
                Entry::One(coefficient) => {
                    self.entries[pos] = None;
                    pos += 1;
                    if code(bits, coefficient) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Reads a DCT block's coefficients, a pass a bit width from the widest
/// down, then its quantiser, and dequantises. `block[0]` holds the DC on
/// entry.
fn read_dct(bits: &mut Bits, block: &mut [i32; 64], quant: &[[i32; 64]; 16]) {
    let mut list = List::new(&[
        Entry::Twenty(4),
        Entry::Twenty(24),
        Entry::Twenty(44),
        Entry::One(1),
        Entry::One(2),
        Entry::One(3),
    ]);
    let mut coded = Vec::with_capacity(64);
    let widths = bits.read(4) as i32;
    for width in (0..widths).rev() {
        list.pass(bits, |bits, coefficient| {
            // The top bit is known; the bits below it, then a sign.
            let value = if width == 0 {
                1
            } else {
                bits.read(width as u32) as i32 | 1 << width
            };
            block[SCAN[coefficient] as usize] = if bits.bit() { -value } else { value };
            coded.push(coefficient);
            false
        });
    }
    let quant = &quant[bits.read(4) as usize];
    block[0] = block[0].wrapping_mul(quant[0]) >> 11;
    for coefficient in coded {
        let at = SCAN[coefficient] as usize;
        block[at] = block[at].wrapping_mul(quant[coefficient]) >> 11;
    }
}

/// Reads a block of residue: the values' bits from the top down, a pass a
/// bit, each pass first giving the values already begun their bit, then
/// walking the tree for new ones. At most `masks` bits are read.
fn read_residue(bits: &mut Bits, block: &mut [i32; 64], mut masks: i32) {
    let mut list = List::new(&[
        Entry::Twenty(4),
        Entry::Twenty(24),
        Entry::Twenty(44),
        Entry::Four(0),
    ]);
    let mut begun: Vec<usize> = Vec::with_capacity(64);
    let mut mask = 1i32 << bits.read(3);
    while mask != 0 {
        for &at in &begun {
            if !bits.bit() {
                continue;
            }
            block[at] += if block[at] < 0 { -mask } else { mask };
            masks -= 1;
            if masks < 0 {
                return;
            }
        }
        let stop = list.pass(bits, |bits, coefficient| {
            let at = SCAN[coefficient] as usize;
            begun.push(at);
            block[at] = if bits.bit() { -mask } else { mask };
            masks -= 1;
            masks < 0
        });
        if stop {
            return;
        }
        mask >>= 1;
    }
}

fn copy_block(to: &mut [u8], at: usize, from: &[u8], from_at: usize, stride: usize) {
    for y in 0..8 {
        to[at + y * stride..][..8].copy_from_slice(&from[from_at + y * stride..][..8]);
    }
}

fn add_block(to: &mut [u8], at: usize, stride: usize, block: &[i32; 64]) {
    for y in 0..8 {
        for x in 0..8 {
            let pixel = &mut to[at + y * stride + x];
            *pixel = (i32::from(*pixel) + block[y * 8 + x]) as u8;
        }
    }
}

/// `x` times a factor in 11-bit fixed point.
fn mul(x: i32, factor: i32) -> i32 {
    (x * factor) >> 11
}

/// The Arai-Agui-Nakajima IDCT's factors, 1.414214, 1.082392, 1.847759
/// and -2.613126, in 11-bit fixed point.
const F1_414: i32 = 2896;
const F1_082: i32 = 2217;
const F1_848: i32 = 3784;
const F_2_613: i32 = -5352;

/// The one-dimensional inverse DCT of eight values by the
/// Arai-Agui-Nakajima factorisation (as the IJG's `jidctfst.c` lays it
/// out); the inputs' scale is carried by the quantisers.
fn idct8(x: [i32; 8]) -> [i32; 8] {
    // Even part.
    let a0 = x[0] + x[4];
    let a1 = x[0] - x[4];
    let a2 = x[2] + x[6];
    let a3 = mul(x[2] - x[6], F1_414) - a2;
    let e0 = a0 + a2;
    let e3 = a0 - a2;
    let e1 = a1 + a3;
    let e2 = a1 - a3;
    // Odd part.
    let z13 = x[5] + x[3];
    let z10 = x[5] - x[3];
    let z11 = x[1] + x[7];
    let z12 = x[1] - x[7];
    let o7 = z11 + z13;
    let o11 = mul(z11 - z13, F1_414);
    let z5 = mul(z10 + z12, F1_848);
    let o10 = mul(z12, F1_082) - z5;
    let o12 = mul(z10, F_2_613) + z5;
    let o6 = o12 - o7;
    let o5 = o11 - o6;
    let o4 = o10 + o5;
    [
        e0 + o7,
        e1 + o6,
        e2 + o5,
        e3 - o4,
        e3 + o4,
        e2 - o5,
        e1 - o6,
        e0 - o7,
    ]
}

/// The 8x8 inverse DCT: columns, then rows, rounded to whole pixels.
fn idct(block: &[i32; 64]) -> [i32; 64] {
    let mut columns = [0i32; 64];
    for x in 0..8 {
        let out = idct8(std::array::from_fn(|y| block[y * 8 + x]));
        for y in 0..8 {
            columns[y * 8 + x] = out[y];
        }
    }
    let mut out = [0i32; 64];
    for y in 0..8 {
        let mut row: [i32; 8] = columns[y * 8..][..8].try_into().unwrap();
        row[0] += 0x7F;
        let row = idct8(row);
        for x in 0..8 {
            out[y * 8 + x] = row[x] >> 8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dc_alone_fills_the_block() {
        let mut block = [0i32; 64];
        block[0] = 100 * 256;
        assert!(idct(&block).iter().all(|&v| v == 100));
    }

    #[test]
    fn the_codebooks_cover_every_code() {
        let books = Codebooks::new();
        for book in &books.0 {
            assert!(book.iter().all(|&(_, length)| length > 0));
        }
    }
}
