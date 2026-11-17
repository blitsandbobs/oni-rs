//! Decode Oni `M3GM` geometry into triangle meshes and export glTF (`.glb`).
//!
//! `M3tGeometry` (M3GM) is a container of references to separate array
//! instances (see `BFW_Motoko.h`):
//!
//! ```text
//! offset 0   geometryFlags  (enum, 4 bytes)
//! offset 4   pointArray          -> PNTA  (M3tPoint3DArray)
//! offset 8   vertexNormalArray   -> VCRA  (M3tVector3DArray)
//! offset 12  triNormalArray      -> VCRA
//! offset 16  texCoordArray       -> TXCA  (M3tTextureCoordArray)
//! offset 20  triStripArray       -> IDXA  (TMtIndexArray, u32)
//! offset 24  triNormalIndexArray -> IDXA  (its count == number of triangles)
//! offset 28  baseMap             -> TXMP  (the texture)
//! offset 32  animation           -> templateref
//! ```
//!
//! Points, texcoords and vertex-normals are **parallel** arrays (one entry per
//! vertex, shared index). Triangles come from walking `triStripArray` as a
//! triangle strip with `0x80000000` restart markers, producing exactly
//! `triNormalIndexArray.numIndices` triangles — this reproduces the engine's
//! software geometry path (`MS_GC_Method_Geometry.c`).

#![forbid(unsafe_code)]

pub mod character;
pub mod charclass;
pub mod glb;
pub mod level;
pub mod objanim;

use oni_dat::{Dat, FourCc, Instance};

/// The `M3GM` (geometry) template tag.
pub const TAG_M3GM: FourCc = FourCc(0x4D33_474D);

// Field offsets inside an M3GM instance.
const OFF_POINT_ARRAY: usize = 4;
const OFF_VERTEX_NORMAL_ARRAY: usize = 8;
const OFF_TEXCOORD_ARRAY: usize = 16;
const OFF_TRISTRIP_ARRAY: usize = 20;
const OFF_TRINORMAL_INDEX_ARRAY: usize = 24;
const OFF_BASE_MAP: usize = 28;

// Element-array field offsets.
const PNTA_NUM: usize = 52; // numPoints, then points[] at +56
const PNTA_DATA: usize = 56;
const VCRA_NUM: usize = 20; // numVectors, then vectors[] at +24
const VCRA_DATA: usize = 24;
const TXCA_NUM: usize = 20; // numTextureCoords, then coords[] at +24
const TXCA_DATA: usize = 24;
const IDXA_NUM: usize = 20; // numIndices, then indices[] at +24
const IDXA_DATA: usize = 24;

const STRIP_RESTART: u32 = 0x8000_0000;
const INDEX_MASK: u32 = 0x7FFF_FFFF;

/// A decoded triangle mesh.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (empty if absent or not parallel to positions).
    pub normals: Vec<[f32; 3]>,
    /// Vertex texture coordinates (empty if absent or not parallel).
    pub texcoords: Vec<[f32; 2]>,
    /// Per-vertex baked-lighting colors (empty if none), written as `COLOR_0`.
    pub colors: Vec<[f32; 3]>,
    /// Triangle list: 3 vertex indices per triangle.
    pub indices: Vec<u32>,
    /// Name of the base texture (TXMP) this mesh uses, if any.
    pub base_texture: Option<String>,
}

impl Mesh {
    /// Number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

fn read_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn read_f32(d: &[u8], o: usize) -> Option<f32> {
    read_u32(d, o).map(f32::from_bits)
}

/// Resolves a template-reference value to the referenced instance's data.
fn resolve<'a>(dat: &'a Dat<'a>, ref_val: u32) -> Option<&'a [u8]> {
    if ref_val & 1 == 0 {
        return None; // not an index-form placeholder
    }
    let idx = (ref_val >> 8) as usize;
    let inst = dat.instances.get(idx)?;
    dat.instance_data(inst)
}

/// Extracts a [`Mesh`] from an `M3GM` instance, resolving its array references.
pub fn extract_mesh(dat: &Dat<'_>, inst: &Instance) -> Option<Mesh> {
    if inst.tag != TAG_M3GM {
        return None;
    }
    let d = dat.instance_data(inst)?;

    // --- positions (PNTA) ---
    let pa = resolve(dat, read_u32(d, OFF_POINT_ARRAY)?)?;
    let num_points = read_u32(pa, PNTA_NUM)? as usize;
    let mut positions = Vec::with_capacity(num_points);
    for i in 0..num_points {
        let o = PNTA_DATA + i * 12;
        positions.push([read_f32(pa, o)?, read_f32(pa, o + 4)?, read_f32(pa, o + 8)?]);
    }

    let mut mesh = Mesh {
        positions,
        ..Default::default()
    };

    // --- normals (VCRA), only if parallel to points ---
    if let Some(va) = read_u32(d, OFF_VERTEX_NORMAL_ARRAY).and_then(|r| resolve(dat, r)) {
        if read_u32(va, VCRA_NUM) == Some(num_points as u32) {
            for i in 0..num_points {
                let o = VCRA_DATA + i * 12;
                if let (Some(x), Some(y), Some(z)) =
                    (read_f32(va, o), read_f32(va, o + 4), read_f32(va, o + 8))
                {
                    mesh.normals.push([x, y, z]);
                }
            }
            if mesh.normals.len() != num_points {
                mesh.normals.clear();
            }
        }
    }

    // --- texcoords (TXCA), only if parallel to points ---
    if let Some(ta) = read_u32(d, OFF_TEXCOORD_ARRAY).and_then(|r| resolve(dat, r)) {
        if read_u32(ta, TXCA_NUM).map(|n| n as usize >= num_points) == Some(true) {
            for i in 0..num_points {
                let o = TXCA_DATA + i * 8;
                if let (Some(u), Some(v)) = (read_f32(ta, o), read_f32(ta, o + 4)) {
                    mesh.texcoords.push([u, v]);
                }
            }
            if mesh.texcoords.len() != num_points {
                mesh.texcoords.clear();
            }
        }
    }

    // --- triangles: walk the strip ---
    let strip_arr = resolve(dat, read_u32(d, OFF_TRISTRIP_ARRAY)?)?;
    let num_strip = read_u32(strip_arr, IDXA_NUM)? as usize;
    let mut strip = Vec::with_capacity(num_strip);
    for i in 0..num_strip {
        strip.push(read_u32(strip_arr, IDXA_DATA + i * 4)?);
    }
    let tni = resolve(dat, read_u32(d, OFF_TRINORMAL_INDEX_ARRAY)?)?;
    let num_tris = read_u32(tni, IDXA_NUM)? as usize;

    mesh.indices = walk_strip(&strip, num_tris, num_points)?;

    // --- base texture name ---
    if let Some(r) = read_u32(d, OFF_BASE_MAP) {
        if r & 1 != 0 {
            if let Some(bm) = dat.instances.get((r >> 8) as usize) {
                mesh.base_texture = bm.name.clone();
            }
        }
    }

    Some(mesh)
}

/// Reproduces the engine's triangle-strip walk: `num_tris` triangles, each
/// consuming one strip index via a sliding window, or three after a
/// `0x80000000` restart marker.
fn walk_strip(strip: &[u32], num_tris: usize, num_points: usize) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(num_tris * 3);
    let mut p = 0usize;
    let (mut i0, mut i1) = (0u32, 0u32);
    for _ in 0..num_tris {
        let mut index2 = *strip.get(p)?;
        p += 1;
        if index2 & STRIP_RESTART != 0 {
            i0 = index2 & INDEX_MASK;
            i1 = *strip.get(p)?;
            p += 1;
            index2 = *strip.get(p)?;
            p += 1;
        }
        let (a, b, c) = (i0 & INDEX_MASK, i1 & INDEX_MASK, index2 & INDEX_MASK);
        if a as usize >= num_points || b as usize >= num_points || c as usize >= num_points {
            return None; // index out of range — layout/scheme error
        }
        out.push(a);
        out.push(b);
        out.push(c);
        i0 = i1;
        i1 = index2;
    }
    Some(out)
}

/// Iterates every `M3GM` mesh in `dat` that resolves successfully.
pub fn meshes<'a>(dat: &'a Dat<'a>) -> impl Iterator<Item = (usize, Mesh)> + 'a {
    dat.instances
        .iter()
        .enumerate()
        .filter(|(_, i)| i.tag == TAG_M3GM)
        .filter_map(move |(idx, i)| extract_mesh(dat, i).map(|m| (idx, m)))
}
