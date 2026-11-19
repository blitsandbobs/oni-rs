//! Decode Oni `AKEV` (Akira environment) level geometry with material binding
//! and per-object separation.
//!
//! An `AKEV` (`BFW_Akira.h`) references parallel arrays of "gunk quads":
//!   * `pointArray` (PNTA) — world vertices,
//!   * `textureCoordArray` (TXCA) — texture coordinates,
//!   * `gqGeneralArray` (AGQG) — per-quad `M3tQuadSplit` (4 vertex indices + 4
//!     UV indices), flags, and an **`object_tag`**,
//!   * `gqRenderArray` (AGQR) — per-quad `textureMapIndex` (+ alpha),
//!   * `textureMapArray` (TXMA) — the `TXMP` textures the quads index into.
//!
//! Each quad binds geometry to a texture via `textureMapIndex`, so we group
//! quads by texture into material-bound submeshes. A quad's `object_tag` is
//! `0xFFFFFFFF` for the static world, or `(OBJcTypeIndex << 24) | id` for a
//! placed object (`OBJcTypeIndex`: 3=door, 5=furniture, 13=trigger, 15=console,
//! …). We split those off into per-object groups so doors/consoles/furniture
//! become separate, transformable entities in a modern engine.

use std::collections::BTreeMap;

use oni_dat::{Dat, FourCc, Instance};

/// The `AKEV` (environment) template tag.
pub const TAG_AKEV: FourCc = FourCc(0x414B_4556);

const OBJECT_TAG_NONE: u32 = 0xFFFF_FFFF;

// AKEV field offsets (array references).
const OFF_POINT_ARRAY: usize = 0;
const OFF_TEXCOORD_ARRAY: usize = 8;
const OFF_GQ_GENERAL: usize = 12;
const OFF_GQ_RENDER: usize = 16;
const OFF_TEXMAP_ARRAY: usize = 28;
/// The quads scripts show and hide (`env_show`): parallel index arrays of
/// quads and the reference each belongs to.
const OFF_REMAP_INDICES: usize = 40;
const OFF_REMAPS: usize = 44;
const IDXA_NUM: usize = 20;
const IDXA_DATA: usize = 24;

// Array element offsets.
const PNTA_NUM: usize = 52;
const PNTA_DATA: usize = 56;
const TXCA_NUM: usize = 20;
const TXCA_DATA: usize = 24;
const ARR20_NUM: usize = 20; // AGQG/AGQR/TXMA: pad[20] then count then data
const ARR20_DATA: usize = 24;

const GQ_GENERAL_SIZE: usize = 56; // M3tQuadSplit(48) + flags(4) + object_tag(4)
const GQ_SHADES: usize = 32; // shades[4] within M3tQuadSplit
const GQ_FLAGS: usize = 48;
const GQ_OBJECT_TAG: usize = 52;
const GQ_RENDER_SIZE: usize = 4; // textureMapIndex(2) + alpha(2)

// AKtGQ flags (BFW_Akira.h).
const GQ_FLAG_TRIANGLE: u32 = 1 << 6;
const GQ_FLAG_TRANSPARENT: u32 = 1 << 7;
const GQ_FLAG_INVISIBLE: u32 = 1 << 13;
/// What characters don't collide with (`AKcGQ_Flag_Chr_Col_Skip`): no
/// collision, no character collision, broken glass. (Stairs are drawn as
/// steps characters pass through, over an invisible-or-not ramp they walk.)
const GQ_FLAG_CHR_COL_SKIP: u32 = (1 << 11) | (1 << 15) | (1 << 18);
/// Door | ghost | stairs up | stairs down: the portals of the room graph.
const GQ_FLAG_PATHFINDING: u32 = 0xF;

/// Maps an `OBJcTypeIndex` (the high byte of an `object_tag`) to a short name.
pub fn object_type_name(type_index: u8) -> &'static str {
    match type_index {
        1 => "character",
        2 => "patrol_path",
        3 => "door",
        4 => "flag",
        5 => "furniture",
        6 => "machine",
        7 => "item",
        8 => "particle",
        9 => "powerup",
        10 => "sound",
        11 => "trigger_volume",
        12 => "weapon",
        13 => "trigger",
        14 => "turret",
        15 => "console",
        16 => "combat",
        17 => "melee",
        18 => "neutral",
        _ => "object",
    }
}

/// One texture's geometry (a material-bound submesh).
#[derive(Clone, Debug, Default)]
pub struct TextureGroup {
    /// The texture (TXMP) name, if the texture is named.
    pub texture: Option<String>,
    /// The texture's TXMP instance index (so the caller can name/route it
    /// consistently with standalone textures).
    pub tex_instance: Option<usize>,
    /// Whether these quads are transparent (alpha-blended).
    pub blend: bool,
    /// Drawn (invisible quads only collide).
    pub visible: bool,
    /// Characters collide with them.
    pub collide: bool,
    /// The reference scripts show and hide them by (`env_show`), if any.
    pub env_ref: Option<u32>,
    /// A breakable pane (its texture's material is glass): each breakable
    /// quad is its own group, numbered by its quad index.
    pub glass: Option<u32>,
    /// Vertex positions (per quad corner).
    pub positions: Vec<[f32; 3]>,
    /// Vertex texture coordinates.
    pub texcoords: Vec<[f32; 2]>,
    /// Flat per-corner normals.
    pub normals: Vec<[f32; 3]>,
    /// Per-corner baked-lighting vertex colors (from the quad's `shades[4]`).
    pub colors: Vec<[f32; 3]>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

/// A placed object's baked geometry, separated out of the static world.
#[derive(Clone, Debug)]
pub struct LevelObject {
    /// The full `object_tag`.
    pub tag: u32,
    /// `OBJcTypeIndex` (the tag's high byte).
    pub type_index: u8,
    /// Human-readable type (`door`, `console`, `furniture`, …).
    pub type_name: &'static str,
    /// World-space pivot (centroid); the groups' positions are relative to it.
    pub pivot: [f32; 3],
    /// Material-bound submeshes, positions relative to `pivot`.
    pub groups: Vec<TextureGroup>,
}

impl LevelObject {
    /// A stable node name, e.g. `console_0x002c77`.
    pub fn node_name(&self) -> String {
        format!("{}_{:#08x}", self.type_name, self.tag & 0x00FF_FFFF)
    }
}

/// A decoded level environment: static world geometry plus placed objects.
#[derive(Clone, Debug)]
pub struct Level {
    /// Environment name.
    pub name: Option<String>,
    /// Static world geometry, one submesh per texture.
    pub environment: Vec<TextureGroup>,
    /// Placed objects (doors, consoles, furniture, …), each separated out.
    pub objects: Vec<LevelObject>,
}

fn read_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn read_u16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn read_f32(d: &[u8], o: usize) -> Option<f32> {
    read_u32(d, o).map(f32::from_bits)
}
fn resolve<'a>(dat: &'a Dat<'a>, v: u32) -> Option<&'a [u8]> {
    if v & 1 == 0 {
        return None;
    }
    dat.instance_data(dat.instances.get((v >> 8) as usize)?)
}

/// Extracts a [`Level`] from an `AKEV` instance.
pub fn extract_level(dat: &Dat<'_>, akev: &Instance) -> Option<Level> {
    extract_level_with(dat, akev, &|_| false)
}

/// [`extract_level`], told which textures (by `TXMP` instance index) break
/// like glass: their quads come out one group each.
pub fn extract_level_with(dat: &Dat<'_>, akev: &Instance, breakable: &dyn Fn(usize) -> bool) -> Option<Level> {
    if akev.tag != TAG_AKEV {
        return None;
    }
    let d = dat.instance_data(akev)?;

    let pa = resolve(dat, read_u32(d, OFF_POINT_ARRAY)?)?;
    let ta = resolve(dat, read_u32(d, OFF_TEXCOORD_ARRAY)?)?;
    let gg = resolve(dat, read_u32(d, OFF_GQ_GENERAL)?)?;
    let gr = resolve(dat, read_u32(d, OFF_GQ_RENDER)?)?;
    let tm = resolve(dat, read_u32(d, OFF_TEXMAP_ARRAY)?)?;

    let num_points = read_u32(pa, PNTA_NUM)? as usize;
    let num_tc = read_u32(ta, TXCA_NUM)? as usize;
    let num_gq = read_u32(gg, ARR20_NUM)?.min(read_u32(gr, ARR20_NUM)?) as usize;
    let num_tex = read_u32(tm, ARR20_NUM)? as usize;

    let point = |i: usize| -> Option<[f32; 3]> {
        let o = PNTA_DATA + i * 12;
        Some([read_f32(pa, o)?, read_f32(pa, o + 4)?, read_f32(pa, o + 8)?])
    };
    let tc = |i: usize| -> Option<[f32; 2]> {
        let o = TXCA_DATA + i * 8;
        // glTF's V axis points down vs Oni's up — flip V.
        Some([read_f32(ta, o)?, 1.0 - read_f32(ta, o + 4)?])
    };
    let tex_lookup = |ti: usize| -> (Option<usize>, Option<String>) {
        match read_u32(tm, ARR20_DATA + ti * 4) {
            Some(r) if r & 1 != 0 => {
                let idx = (r >> 8) as usize;
                (
                    Some(idx),
                    dat.instances.get(idx).and_then(|i| i.name.clone()),
                )
            }
            _ => (None, None),
        }
    };

    // Bucket quads by object_tag; within each bucket, by (texture, blend mode).
    // Which quads scripts can show and hide, and by what reference.
    let mut remap: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    if let (Some(ri), Some(rr)) = (
        read_u32(d, OFF_REMAP_INDICES).and_then(|v| resolve(dat, v)),
        read_u32(d, OFF_REMAPS).and_then(|v| resolve(dat, v)),
    ) {
        let n = read_u32(ri, IDXA_NUM).unwrap_or(0).min(read_u32(rr, IDXA_NUM).unwrap_or(0)) as usize;
        for i in 0..n {
            if let (Some(q), Some(r)) = (read_u32(ri, IDXA_DATA + i * 4), read_u32(rr, IDXA_DATA + i * 4)) {
                remap.entry(q as usize).or_insert(r);
            }
        }
    }

    // (Groups: texture, blend, visible, collide, script reference, pane.)
    type Groups = BTreeMap<(u16, bool, bool, bool, Option<u32>, Option<u32>), TextureGroup>;
    let mut buckets: BTreeMap<u32, Groups> = BTreeMap::new();

    for q in 0..num_gq {
        let go = ARR20_DATA + q * GQ_GENERAL_SIZE;
        let flags = read_u32(gg, go + GQ_FLAGS)?;
        let visible = flags & GQ_FLAG_INVISIBLE == 0;
        let collide = flags & GQ_FLAG_CHR_COL_SKIP == 0;
        if !visible && !collide {
            continue; // neither drawn nor collided with
        }
        if flags & GQ_FLAG_PATHFINDING != 0 {
            // Portals between rooms (ghost quads, stair entries, door ways):
            // pathfinding data only — the original's importer leaves them out
            // of everything drawn or collided with (`AKcGQ_Flag_NoTextureMask`).
            continue;
        }
        let mut vidx = [0usize; 4];
        let mut uvidx = [0usize; 4];
        let mut ok = true;
        for k in 0..4 {
            vidx[k] = read_u32(gg, go + k * 4)? as usize;
            uvidx[k] = read_u32(gg, go + 16 + k * 4)? as usize;
            if vidx[k] >= num_points || uvidx[k] >= num_tc {
                ok = false;
            }
        }
        if !ok {
            continue;
        }
        let tex_index = read_u16(gr, ARR20_DATA + q * GQ_RENDER_SIZE)?;
        if tex_index as usize >= num_tex {
            continue;
        }
        let object_tag = read_u32(gg, go + GQ_OBJECT_TAG)?;
        let blend = flags & GQ_FLAG_TRANSPARENT != 0;
        let is_triangle = flags & GQ_FLAG_TRIANGLE != 0;

        let glass = tex_lookup(tex_index as usize).0.filter(|&i| breakable(i)).map(|_| q as u32);
        let grp = buckets
            .entry(object_tag)
            .or_default()
            .entry((tex_index, blend, visible, collide, remap.get(&q).copied(), glass))
            .or_default();
        if grp.tex_instance.is_none() {
            grp.glass = glass;
            let (idx, name) = tex_lookup(tex_index as usize);
            grp.tex_instance = idx;
            grp.texture = name;
            grp.blend = blend;
            grp.visible = visible;
            grp.collide = collide;
            grp.env_ref = remap.get(&q).copied();
        }
        let base = grp.positions.len() as u32;
        // A triangle quad uses only its first 3 corners.
        let corner_count = if is_triangle { 3 } else { 4 };
        let corners: [[f32; 3]; 4] = [
            point(vidx[0])?,
            point(vidx[1])?,
            point(vidx[2])?,
            point(vidx[3])?,
        ];
        // Per-corner baked-lighting shades (M3tQuadSplit.shades[4] @ +32).
        let e1 = sub(corners[1], corners[0]);
        let e2 = sub(corners[2], corners[0]);
        let n = normalize(cross(e1, e2));
        for k in 0..corner_count {
            grp.positions.push(corners[k]);
            grp.texcoords.push(tc(uvidx[k])?);
            grp.normals.push(n);
            let shade = read_u32(gg, go + GQ_SHADES + k * 4).unwrap_or(0x00FF_FFFF);
            grp.colors.push(shade_rgb(shade));
        }
        if is_triangle {
            grp.indices.extend_from_slice(&[base, base + 1, base + 2]);
        } else {
            grp.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    // Static world.
    let environment: Vec<TextureGroup> = buckets
        .remove(&OBJECT_TAG_NONE)
        .map(|m| m.into_values().collect())
        .unwrap_or_default();

    // Placed objects: re-origin each to its own centroid.
    let mut objects = Vec::with_capacity(buckets.len());
    for (tag, groups) in buckets {
        let mut groups: Vec<TextureGroup> = groups.into_values().collect();
        let pivot = centroid(&groups);
        for g in &mut groups {
            for p in &mut g.positions {
                p[0] -= pivot[0];
                p[1] -= pivot[1];
                p[2] -= pivot[2];
            }
        }
        let type_index = (tag >> 24) as u8;
        objects.push(LevelObject {
            tag,
            type_index,
            type_name: object_type_name(type_index),
            pivot,
            groups,
        });
    }

    Some(Level {
        name: akev.name.clone(),
        environment,
        objects,
    })
}

/// Mean of all vertex positions across the groups.
fn centroid(groups: &[TextureGroup]) -> [f32; 3] {
    let mut sum = [0.0f64; 3];
    let mut count = 0u64;
    for g in groups {
        for p in &g.positions {
            sum[0] += p[0] as f64;
            sum[1] += p[1] as f64;
            sum[2] += p[2] as f64;
            count += 1;
        }
    }
    if count == 0 {
        return [0.0; 3];
    }
    [
        (sum[0] / count as f64) as f32,
        (sum[1] / count as f64) as f32,
        (sum[2] / count as f64) as f32,
    ]
}

/// Decodes a packed `IMtShade` (`0x00RRGGBB`, high byte ignored) to linear
/// `[r,g,b]` in `0..=1` — Oni's per-vertex baked lighting.
fn shade_rgb(s: u32) -> [f32; 3] {
    [
        ((s >> 16) & 0xFF) as f32 / 255.0,
        ((s >> 8) & 0xFF) as f32 / 255.0,
        (s & 0xFF) as f32 / 255.0,
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 1.0, 0.0]
    }
}
