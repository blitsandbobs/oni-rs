//! Decode a level's navigation data from its `AKEV` environment
//!: the convex **room volumes** (`AKVA`)
//! with their sides (`AKBA`, planes) and BSP chain (`AKBP`), the **links**
//! between rooms (`AKAA`) through portal quads (`AGQG`), and each room's
//! run-length-encoded **walkability grid** in the `.raw`.

use oni_bytes::at;
use oni_dat::{Dat, Instance};

use crate::level::TAG_AKEV;

// AKEV array references.
const OFF_POINTS: usize = 0;
const OFF_PLANES: usize = 4;
const OFF_QUADS: usize = 12;
const OFF_ROOMS: usize = 32;
const OFF_SIDES: usize = 36;
const OFF_BSP: usize = 48;
const OFF_LINKS: usize = 60;

// Array layouts: pad, count, elements (points have their own header).
const POINTS_NUM: usize = 52;
const POINTS_DATA: usize = 56;
const NUM: usize = 20;
const NUM16: usize = 22;
const DATA: usize = 24;

const ROOM_SIZE: usize = 116;
const SIDE_SIZE: usize = 28;
const BSP_SIZE: usize = 12;
const LINK_SIZE: usize = 12;
const QUAD_SIZE: usize = 56;
const QUAD_FLAGS: usize = 48;

/// Grid squares are 4 units; a grid starts 2 cells before its room's box.
pub const SQUARE: f32 = 4.0;

/// Room flags (`AKcBNV_Flag_*`).
pub mod room_flag {
    pub const STAIRS: u32 = 0x01;
    pub const SPIRAL_STAIRS: u32 = 0x02;
    pub const ROOM: u32 = 0x04;
    pub const NON_AI: u32 = 0x08;
    pub const SIMPLE: u32 = 0x10;
}

/// Portal quad flags (`AKcGQ_Flag_*`) that matter to pathfinding.
pub mod quad_flag {
    pub const DOOR: u32 = 0x1;
    pub const GHOST: u32 = 0x2;
    pub const STAIRS_UP: u32 = 0x4;
    pub const STAIRS_DOWN: u32 = 0x8;
}

/// One room volume.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    pub flags: u32,
    /// The hull's planes `[a, b, c, d]`, oriented so a point is inside when
    /// `a·x + b·y + c·z + d ≤ 0` for every one (BSP order).
    pub planes: Vec<[f32; 4]>,
    /// The hull's bounding box.
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// For a stair room: the stair plane and the slab height (inside only
    /// where `0 ≤ plane·p ≤ height`).
    pub stair: Option<([f32; 4], f32)>,
    pub grid: Option<Grid>,
    pub links: Vec<Link>,
}

/// A room's walkability grid: `width × height` square types (row-major,
/// `x + z·width`), cell `(i, j)` covering world x `[origin.0 + 4i, +4)`, z
/// `[origin.1 + 4j, +4)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub width: u32,
    pub height: u32,
    pub origin: [f32; 2],
    pub cells: Vec<u8>,
}

/// A way from one room into another, through a quad (a portal when it is a
/// ghost or stair entry).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub to: u32,
    pub quad_flags: u32,
    pub quad: [[f32; 3]; 4],
}

fn resolve<'a>(dat: &'a Dat<'a>, v: u32) -> Option<&'a [u8]> {
    if v & 1 == 0 {
        return None;
    }
    dat.instance_data(dat.instances.get((v >> 8) as usize)?)
}

/// Expands a run-length-encoded grid: each byte's low nibble is a square
/// type, its high nibble a run of 1–15 — or 0, meaning the next byte is the
/// run.
pub fn decode_grid(rle: &[u8], cells: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(cells);
    let mut i = 0;
    while i < rle.len() && out.len() < cells {
        let value = rle[i] & 0x0F;
        let mut run = usize::from(rle[i] >> 4);
        i += 1;
        if run == 0 {
            run = usize::from(*rle.get(i).unwrap_or(&0));
            i += 1;
        }
        out.extend(std::iter::repeat_n(value, run.min(cells - out.len())));
    }
    out.resize(cells, 0);
    out
}

/// Decodes the navigation data of an `AKEV` environment; `raw` is the
/// level's `.raw` (holding the grids).
pub fn extract_navigation(dat: &Dat<'_>, akev: &Instance, raw: &[u8]) -> Option<Vec<Room>> {
    if akev.tag != TAG_AKEV {
        return None;
    }
    let d = dat.instance_data(akev)?;
    let points = resolve(dat, at::u32(d, OFF_POINTS)?)?;
    let planes = resolve(dat, at::u32(d, OFF_PLANES)?)?;
    let quads = resolve(dat, at::u32(d, OFF_QUADS)?)?;
    let rooms = resolve(dat, at::u32(d, OFF_ROOMS)?)?;
    let sides = resolve(dat, at::u32(d, OFF_SIDES)?)?;
    let bsp = resolve(dat, at::u32(d, OFF_BSP)?)?;
    let links = resolve(dat, at::u32(d, OFF_LINKS)?)?;

    let point = |i: u32| -> Option<[f32; 3]> {
        if i >= at::u32(points, POINTS_NUM)? {
            return None;
        }
        let o = POINTS_DATA + i as usize * 12;
        Some([
            at::f32(points, o)?,
            at::f32(points, o + 4)?,
            at::f32(points, o + 8)?,
        ])
    };
    // A plane reference: index, with bit 31 meaning "negated".
    let plane = |r: u32| -> Option<[f32; 4]> {
        let o = DATA + (r & 0x7FFF_FFFF) as usize * 16;
        let p = [
            at::f32(planes, o)?,
            at::f32(planes, o + 4)?,
            at::f32(planes, o + 8)?,
            at::f32(planes, o + 12)?,
        ];
        Some(if r & 0x8000_0000 != 0 {
            p.map(|v| -v)
        } else {
            p
        })
    };
    let num_bsp = at::u16(bsp, NUM16)? as u32;
    let num_links = at::u32(links, NUM)?;

    let n = at::u32(rooms, NUM)? as usize;
    let mut out = Vec::with_capacity(n);
    for r in 0..n {
        let o = DATA + r * ROOM_SIZE;
        let field = |at: usize| at::u32(rooms, o + at);
        let flags = field(92)?;

        // The hull: the BSP chain from the room's root (each node's
        // "positive" child continues inside).
        let mut hull = Vec::new();
        let mut node = field(0)?;
        while node != u32::MAX && node < num_bsp && hull.len() < 64 {
            let b = DATA + node as usize * BSP_SIZE;
            hull.push(plane(at::u32(bsp, b)?)?);
            node = at::u32(bsp, b + 4)?;
        }

        let vec3 = |at: usize| -> Option<[f32; 3]> {
            Some([
                at::f32(rooms, o + at)?,
                at::f32(rooms, o + at + 4)?,
                at::f32(rooms, o + at + 8)?,
            ])
        };
        let (min, max) = (vec3(48)?, vec3(60)?);
        let stair = (flags & room_flag::STAIRS != 0)
            .then(|| -> Option<([f32; 4], f32)> {
                Some((
                    [
                        at::f32(rooms, o + 96)?,
                        at::f32(rooms, o + 100)?,
                        at::f32(rooms, o + 104)?,
                        at::f32(rooms, o + 108)?,
                    ],
                    at::f32(rooms, o + 112)?,
                ))
            })
            .flatten();

        let (gw, gh) = (field(28)?, field(32)?);
        let (rle_at, rle_len) = (field(36)? as usize, field(40)? as usize);
        let (gox, goy) = (at::i16(rooms, o + 72)?, at::i16(rooms, o + 74)?);
        let grid = (rle_at != 0 && gw > 0 && gh > 0)
            .then(|| -> Option<Grid> {
                let rle = raw.get(rle_at..rle_at + rle_len)?;
                Some(Grid {
                    width: gw,
                    height: gh,
                    origin: [
                        min[0] + f32::from(gox) * SQUARE,
                        min[2] + f32::from(goy) * SQUARE,
                    ],
                    cells: decode_grid(rle, (gw * gh) as usize),
                })
            })
            .flatten();

        // Links: every side's range of adjacencies.
        let (side_start, side_end) = (field(8)?, field(12)?);
        let mut room_links = Vec::new();
        for s in side_start..side_end {
            let so = DATA + s as usize * SIDE_SIZE;
            let (a0, a1) = (at::u32(sides, so + 4)?, at::u32(sides, so + 8)?);
            for a in a0..a1.min(num_links) {
                let ao = DATA + a as usize * LINK_SIZE;
                let (to, q) = (at::u32(links, ao)?, at::u32(links, ao + 4)?);
                let qo = DATA + q as usize * QUAD_SIZE;
                let quad_flags = at::u32(quads, qo + QUAD_FLAGS)?;
                let mut corners = [[0.0; 3]; 4];
                for (k, c) in corners.iter_mut().enumerate() {
                    *c = point(at::u32(quads, qo + k * 4)?)?;
                }
                room_links.push(Link {
                    to,
                    quad_flags,
                    quad: corners,
                });
            }
        }

        out.push(Room {
            flags,
            planes: hull,
            min,
            max,
            stair,
            grid,
            links: room_links,
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_runs_expand() {
        // 3 × clear, then a long run (0 high nibble → next byte) of 20 × danger.
        assert_eq!(
            decode_grid(&[0x30, 0x08, 20], 23),
            [vec![0; 3], vec![8; 20]].concat()
        );
    }
}
