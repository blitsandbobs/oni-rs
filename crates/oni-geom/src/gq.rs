//! A level's quads as the engine's collision and decals see them (`AKtGQ_*`,
//! `BFW_Akira.h`): every quad of the `AKEV` — drawn or not — with its
//! corners, flags, plane, baked corner shades and texture.

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

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32_at(d: &[u8], o: usize) -> Option<f32> {
    u32_at(d, o).map(f32::from_bits)
}

fn array<'a>(dat: &'a Dat<'a>, akev: &[u8], field: usize) -> Option<&'a [u8]> {
    let r = u32_at(akev, field)?;
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
    let n_points = u32_at(pa, 52)? as usize;
    let points = (0..n_points)
        .map(|i| {
            let o = 56 + i * 12;
            Some([f32_at(pa, o)?, f32_at(pa, o + 4)?, f32_at(pa, o + 8)?])
        })
        .collect::<Option<Vec<_>>>()?;
    let n_planes = u32_at(pl, 20)? as usize;
    let planes = (0..n_planes)
        .map(|i| {
            let o = 24 + i * 16;
            Some([
                f32_at(pl, o)?,
                f32_at(pl, o + 4)?,
                f32_at(pl, o + 8)?,
                f32_at(pl, o + 12)?,
            ])
        })
        .collect::<Option<Vec<_>>>()?;
    let n_tex = u32_at(tm, 20)? as usize;
    let textures = (0..n_tex)
        .map(|i| {
            u32_at(tm, 24 + i * 4)
                .filter(|r| r & 1 != 0)
                .map(|r| (r >> 8) as usize)
        })
        .collect();
    let n = u32_at(gg, 20)?.min(u32_at(gr, 20)?).min(u32_at(gc, 20)?) as usize;
    // The quads scripts show and hide: parallel index arrays of quads and
    // their references (the first reference of a quad counts).
    let mut refs: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    if let (Some(ri), Some(rr)) = (array(dat, d, 40), array(dat, d, 44)) {
        let count = u32_at(ri, 20).unwrap_or(0).min(u32_at(rr, 20).unwrap_or(0)) as usize;
        for i in 0..count {
            if let (Some(q), Some(r)) = (u32_at(ri, 24 + i * 4), u32_at(rr, 24 + i * 4)) {
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
            let at = |k: usize| u32_at(gg, g + k * 4);
            Some(Quad {
                points: [at(0)?, at(1)?, at(2)?, at(3)?],
                shades: [at(8)?, at(9)?, at(10)?, at(11)?],
                flags: at(12)?,
                object_tag: at(13)?,
                texture: gr
                    .get(24 + q * 4..26 + q * 4)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))?,
                plane: u32_at(gc, 24 + q * 28)?,
                env_ref: refs.get(&(q as u32)).copied(),
                bbox: {
                    let c = |k: usize| f32_at(gc, 28 + q * 28 + k * 4);
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
