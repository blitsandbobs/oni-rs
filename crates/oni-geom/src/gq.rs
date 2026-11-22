//! A level's quads as the engine's collision and decals see them (`AKtGQ_*`,
//! `BFW_Akira.h`): every quad of the `AKEV` — drawn or not — with its
//! corners, flags, plane, baked corner shades and texture.

use oni_bytes::at;
use oni_dat::{Dat, Instance};

use crate::level::TAG_AKEV;

/// One quad (`AKtGQ_General` + `_Render` + `_Collision`).
#[derive(Clone, Debug)]
pub struct Quad {
    /// Corner point indices (a triangle uses the first three).
    pub points: [u32; 4],
    /// `AKcGQ_Flag_*`.
    pub flags: u32,
    /// Its plane: the low 31 bits index the planes, the top bit negates it.
    pub plane: u32,
    /// Baked corner shades (`0x00RRGGBB`).
    pub shades: [u32; 4],
    /// Its texture-map entry.
    pub texture: u16,
    /// `0xFFFFFFFF` for the static world, else the object it belongs to.
    pub object_tag: u32,
    /// Its bounding box (min, max).
    pub bbox: [[f32; 3]; 2],
    /// The script reference that shows and hides it (`env_show`), if any.
    pub env_ref: Option<u32>,
}

/// A level's quads with the points, planes and textures they use.
#[derive(Clone, Debug, Default)]
pub struct Quads {
    pub points: Vec<[f32; 3]>,
    /// `a·x + b·y + c·z + d = 0`.
    pub planes: Vec<[f32; 4]>,
    /// Per texture-map entry: its `TXMP` instance.
    pub textures: Vec<Option<usize>>,
    pub quads: Vec<Quad>,
}

fn array<'a>(dat: &'a Dat<'a>, akev: &[u8], field: usize) -> Option<&'a [u8]> {
    let r = at::u32(akev, field)?;
    if r & 1 == 0 {
        return None;
    }
    dat.instance_data(dat.instances.get((r >> 8) as usize)?)
}

/// The level's quads (`None` if `akev` is not one).
pub fn extract_quads(dat: &Dat<'_>, akev: &Instance) -> Option<Quads> {
    if akev.tag != TAG_AKEV {
        return None;
    }
    let d = dat.instance_data(akev)?;
    // AKtEnvironment: points, planes, texcoords, quads general / render /
    // collision, debug, texture maps.
    let (pa, pl, gg, gr, gc, tm) = (
        array(dat, d, 0)?,
        array(dat, d, 4)?,
        array(dat, d, 12)?,
        array(dat, d, 16)?,
        array(dat, d, 20)?,
        array(dat, d, 28)?,
    );
    let n_points = at::u32(pa, 52)? as usize;
    let points = (0..n_points)
        .map(|i| {
            let o = 56 + i * 12;
            Some([at::f32(pa, o)?, at::f32(pa, o + 4)?, at::f32(pa, o + 8)?])
        })
        .collect::<Option<Vec<_>>>()?;
    let n_planes = at::u32(pl, 20)? as usize;
    let planes = (0..n_planes)
        .map(|i| {
            let o = 24 + i * 16;
            Some([
                at::f32(pl, o)?,
                at::f32(pl, o + 4)?,
                at::f32(pl, o + 8)?,
                at::f32(pl, o + 12)?,
            ])
        })
        .collect::<Option<Vec<_>>>()?;
    let n_tex = at::u32(tm, 20)? as usize;
    let textures = (0..n_tex)
        .map(|i| {
            at::u32(tm, 24 + i * 4)
                .filter(|r| r & 1 != 0)
                .map(|r| (r >> 8) as usize)
        })
        .collect();
    let n = at::u32(gg, 20)?.min(at::u32(gr, 20)?).min(at::u32(gc, 20)?) as usize;
    // The quads scripts show and hide: parallel index arrays of quads and
    // their references (the first reference of a quad counts).
    let mut refs: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    if let (Some(ri), Some(rr)) = (array(dat, d, 40), array(dat, d, 44)) {
        let count = at::u32(ri, 20)
            .unwrap_or(0)
            .min(at::u32(rr, 20).unwrap_or(0)) as usize;
        for i in 0..count {
            if let (Some(q), Some(r)) = (at::u32(ri, 24 + i * 4), at::u32(rr, 24 + i * 4)) {
                refs.entry(q).or_insert(r);
            }
        }
    }
    let quads = (0..n)
        .map(|q| {
            // General: 4 point indices, 4 texcoord indices, 4 shades, flags,
            // object tag (56 bytes); render: texture, alpha (4); collision:
            // plane, box (28).
            let g = 24 + q * 56;
            let word = |k: usize| at::u32(gg, g + k * 4);
            Some(Quad {
                points: [word(0)?, word(1)?, word(2)?, word(3)?],
                shades: [word(8)?, word(9)?, word(10)?, word(11)?],
                flags: word(12)?,
                object_tag: word(13)?,
                texture: at::u16(gr, 24 + q * 4)?,
                plane: at::u32(gc, 24 + q * 28)?,
                env_ref: refs.get(&(q as u32)).copied(),
                bbox: {
                    let c = |k: usize| at::f32(gc, 28 + q * 28 + k * 4);
                    [[c(0)?, c(1)?, c(2)?], [c(3)?, c(4)?, c(5)?]]
                },
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Quads {
        points,
        planes,
        textures,
        quads,
    })
}
