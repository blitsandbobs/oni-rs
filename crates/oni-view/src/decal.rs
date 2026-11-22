//! Building a decal (`BFW_Decal.c`): a
//! rectangle of texture laid on the level's quads around a point — projected
//! onto the quad struck, clipped to it, and carried over the edges it reaches
//! onto the neighbouring quads (unfolded about each shared edge) as long as
//! they face the way the decal came from closely enough; lit by the quads'
//! baked shades, welded where pieces meet, and lifted a little off the wall.

use glam::Vec3;

use crate::quads::{flag, Quad, Quads};

/// Decal flags (`P3cDecalFlag_*`).
pub mod dflag {
    pub const STATIC: u32 = 0x1;
    pub const CULL_BACK_FACING: u32 = 0x4;
    pub const IGNORE_ADJACENCY: u32 = 0x8;
    pub const FULL_BRIGHT: u32 = 0x10;
}

const CLIP_BUFFER: usize = 16;
const BUILD_BUFFER: usize = 512;
const MAX_TRI_INDICES: usize = 512;
const MAX_EDGES: usize = 64;
/// Quads around a decal considered at most (`P3cDecal_GQBitVectorSize`).
const MAX_QUADS: usize = 512;
const MAX_ADJACENCIES: usize = 16;
const MAX_SHARED_PLANES: usize = 16;
/// How far a decal sits off its surface.
pub const POINT_FUDGE: f32 = 0.05;
const ON_PLANE: f32 = 1e-2;
const EDGE_OVERLAP: f32 = 1e-3;
const COPLANAR_EDGE: f32 = 1e-4;
const COPLANAR_ADJ_EDGE_LINEAR: f32 = 1e-2;
const COLLINEAR_EDGE_OVERLAP: f32 = 1e-2;
const PARALLEL_EDGE_DELTA: f32 = 1e-3;
const SHARED_POINT: f32 = 0.1;
const SHARED_UV: f32 = 1e-2;
const OFFSET_NORMAL_LENGTH: f32 = 1e-3;
const EDGE_CLIP: f32 = 0.1;
const REVERSE_2_SIDED: f32 = 1e-3;
const PLANE_DENOM: f32 = 1e-6;
/// `UUcEpsilon`: a point is behind a plane by more than this.
const EPSILON: f32 = 1.0 / 1024.0;
/// What the decal's own sphere query skips.
pub const SKIP: u32 = flag::OBJ_COL_SKIP | flag::INVISIBLE | flag::NO_DECAL;
const NONE: usize = usize::MAX;

/// A character class's shadow (`ONtShadowConstants`): its texture; the
/// heights where it starts to fade and where it is gone; its half-size on
/// the ground, at the fade and at the top; its opacity (0–255) on the
/// ground and at the fade.
#[derive(Clone, Debug, Default)]
pub struct Shadow {
    pub texture: String,
    pub height_max: f32,
    pub height_fade: f32,
    pub size_max: f32,
    pub size_fade: f32,
    pub size_min: f32,
    pub alpha_max: u16,
    pub alpha_fade: u16,
}

impl Shadow {
    /// A class's (`classes.json`'s `shadow`; none without a texture).
    pub fn from_class(j: Option<&oni_schema::files::ClassShadow>) -> Option<Shadow> {
        let j = j?;
        Some(Shadow {
            texture: j.texture.clone()?,
            height_max: j.height_max,
            height_fade: j.height_fade,
            size_max: j.size_max,
            size_fade: j.size_fade,
            size_min: j.size_min,
            alpha_max: j.alpha_max as u16,
            alpha_fade: j.alpha_fade as u16,
        })
    }

    /// Where and how big and dark a character's shadow is (the original's
    /// character display): flat under the pelvis on the floor it stands on
    /// (`floor`), else on whatever a sphere swept down from the feet meets
    /// — smaller and fainter the higher, gone above the top height. `None`:
    /// no shadow.
    pub fn place(
        &self,
        quads: &Quads,
        pelvis: Vec3,
        feet_y: f32,
        floor: Option<u32>,
    ) -> Option<(Vec3, u32, f32, u16)> {
        let at = Vec3::new(pelvis.x, feet_y, pelvis.z);
        if let Some(q) = floor {
            return Some((at, q, self.size_max, self.alpha_max));
        }
        let r = self.size_max;
        let hit = *quads
            .sphere(
                at - Vec3::Y * r,
                r,
                Vec3::Y * -(self.height_max + r),
                flag::OBJ_COL_SKIP | flag::INVISIBLE,
            )
            .first()?;
        let height = (at.y - hit.point.y).max(0.0);
        let round = |a: f32| a.max(0.0).round_ties_even() as u16;
        if height < self.height_fade {
            let t = height / self.height_fade;
            let size = self.size_max + (self.size_fade - self.size_max) * t;
            let alpha = f32::from(self.alpha_max)
                + (f32::from(self.alpha_fade) - f32::from(self.alpha_max)) * t;
            Some((hit.point, hit.quad, size, round(alpha)))
        } else if height < self.height_max {
            let t = (self.height_max - height) / (self.height_max - self.height_fade);
            let size = self.size_min + (self.size_fade - self.size_min) * t;
            Some((
                hit.point,
                hit.quad,
                size,
                round(f32::from(self.alpha_fade) * t),
            ))
        } else {
            None
        }
    }

    /// The shadow's decal: laid straight down onto every floor around (no
    /// wrapping), unlit; none if it would overflow the original's 2 KB
    /// buffer.
    pub fn build(quads: &Quads, quad: u32, at: Vec3, size: f32) -> Option<Mesh> {
        let p = Params {
            flags: dflag::IGNORE_ADJACENCY | dflag::CULL_BACK_FACING | dflag::FULL_BRIGHT,
            wrap: 80f32.to_radians(),
            direction: None,
            position: at,
            fwd: Vec3::Y,
            up: Vec3::Z,
            right: Vec3::X,
            xscale: size,
            yscale: size,
        };
        build(quads, Some(quad), &p).filter(|m| m.bytes() <= SHADOW_BUFFER)
    }
}

/// The character shadow's decal buffer (`ONcShadowDecalStaticBufferSize`).
const SHADOW_BUFFER: usize = 2048;

/// What to build: where, which way, how big.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub flags: u32,
    /// How far round a corner it may wrap (radians).
    pub wrap: f32,
    /// The way it came (e.g. a bullet's velocity); `None`: straight in.
    pub direction: Option<Vec3>,
    pub position: Vec3,
    /// Out of the surface.
    pub fwd: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    /// Half its width and height.
    pub xscale: f32,
    pub yscale: f32,
}

/// A built decal: its triangles over the level, and the quad it was made on
/// (whose visibility decides whether it is drawn).
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub quad: Option<u32>,
    pub positions: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    /// `0xAARRGGBB`; white when full-bright.
    pub shades: Vec<u32>,
    pub indices: Vec<u16>,
}

impl Mesh {
    /// Its size in the dynamic decals' memory (header, vertices, indices
    /// padded to four).
    pub fn bytes(&self) -> usize {
        let indices = (self.indices.len() + 3) & !3;
        20 + self.positions.len() * 24 + indices * 2
    }
}

/// `IMrShade_Interpolate`: per channel, in 256ths.
pub fn shade_lerp(a: u32, b: u32, t: f32) -> u32 {
    let to = (t * 256.0).round_ties_even() as u32;
    let from = 256 - to;
    let mut out = 0;
    for shift in [24, 16, 8, 0] {
        let c = ((a >> shift) & 0xFF) * from + ((b >> shift) & 0xFF) * to;
        out |= ((c & 0xFF00) >> 8) << shift;
    }
    out
}

/// `IMrShade_IsEqual`: every channel within 2.
fn shade_eq(a: u32, b: u32) -> bool {
    [24, 16, 8, 0]
        .iter()
        .all(|s| (((a >> s) & 0xFF) as i32 - ((b >> s) & 0xFF) as i32).abs() <= 2)
}

#[derive(Clone, Debug, Default)]
struct Buf {
    points: Vec<Vec3>,
    uvs: Vec<[f32; 2]>,
    codes: Vec<u8>,
    shades: Vec<u32>,
    max: usize,
}

impl Buf {
    fn new(max: usize) -> Buf {
        Buf {
            max,
            ..Buf::default()
        }
    }
    fn len(&self) -> usize {
        self.points.len()
    }
    fn truncate(&mut self, n: usize) {
        self.points.truncate(n);
        self.uvs.truncate(n);
        self.codes.truncate(n);
        self.shades.truncate(n);
    }
    fn push(&mut self, p: Vec3, uv: [f32; 2], code: u8, shade: u32) {
        self.points.push(p);
        self.uvs.push(uv);
        self.codes.push(code);
        self.shades.push(shade);
    }
    fn full(&self) -> bool {
        self.len() >= self.max
    }
}

/// `P3iDecal_ClipToPlane`: keeps the part of `input` on `side`'s side of the
/// plane through `p0 → p1` perpendicular to the polygon, appending it to
/// `out`; new points take `code` (with what both ends share) and a shade
/// along the edge. Returns how many were added.
#[allow(clippy::too_many_arguments)]
fn clip_to_plane(
    code: u8,
    p0: Vec3,
    p1: Vec3,
    s0: u32,
    s1: u32,
    side: Vec3,
    input: &Buf,
    out: &mut Buf,
) -> usize {
    let start = out.len();
    if out.full() || input.len() == 0 {
        return 0;
    }
    let line = p1 - p0;
    let normal = line.cross(line.cross(side - p1));
    if normal.length_squared() < 1e-20 {
        // A degenerate edge: copy across.
        for j in 0..input.len() {
            out.push(
                input.points[j],
                input.uvs[j],
                input.codes[j],
                input.shades[j],
            );
            if out.full() {
                break;
            }
        }
        return out.len() - start;
    }
    let normal = normal.normalize();
    let d = -normal.dot(p1);
    let behind = |p: Vec3| normal.dot(p) + d < -EPSILON;
    let line_dot = line.dot(line);
    let cross = |a: usize, b: usize, out: &mut Buf| {
        let (a_p, b_p) = (input.points[a], input.points[b]);
        let m = b_p - a_p;
        let denom = normal.dot(m);
        let t = if denom.abs() < PLANE_DENOM {
            0.5
        } else {
            (-(normal.dot(a_p) + d) / denom).clamp(0.0, 1.0)
        };
        let p = a_p + m * t;
        let (ua, ub) = (input.uvs[a], input.uvs[b]);
        let uv = [ub[0] * t + ua[0] * (1.0 - t), ub[1] * t + ua[1] * (1.0 - t)];
        let interp = ((p - p0).dot(line) / line_dot).clamp(0.0, 1.0);
        out.push(
            p,
            uv,
            (input.codes[a] & input.codes[b]) | code,
            shade_lerp(s0, s1, interp),
        );
    };
    let n = input.len();
    let mut i0 = n - 1;
    let mut in0 = behind(input.points[i0]);
    for j in 0..n {
        let in1 = behind(input.points[j]);
        if in0 {
            if in1 {
                out.push(
                    input.points[j],
                    input.uvs[j],
                    input.codes[j],
                    input.shades[j],
                );
            } else {
                cross(i0, j, out);
            }
        } else if in1 {
            cross(i0, j, out);
            if out.full() {
                break;
            }
            out.push(
                input.points[j],
                input.uvs[j],
                input.codes[j],
                input.shades[j],
            );
        }
        if out.full() {
            break;
        }
        i0 = j;
        in0 = in1;
    }
    out.len() - start
}

/// `P3iDecal_ClipToPolygon`: clips `source` to the polygon's edges (each
/// edge's bit in the new points' codes), appending to `dest`. Returns how
/// many were added and the codes they carry.
fn clip_to_polygon(pts: &[Vec3], shades: &[u32], source: &Buf, dest: &mut Buf) -> (usize, u32) {
    let n = pts.len();
    let start = dest.len();
    let mut input = source.clone();
    input.codes.iter_mut().for_each(|c| *c = 0);
    let (mut c0, mut c1) = (n - 2, n - 1);
    let mut edge = n - 2;
    for i in 0..n {
        if i == n - 1 {
            clip_to_plane(
                1 << edge,
                pts[c0],
                pts[c1],
                shades[c0],
                shades[c1],
                pts[i],
                &input,
                dest,
            );
        } else {
            let mut out = Buf::new(CLIP_BUFFER);
            clip_to_plane(
                1 << edge,
                pts[c0],
                pts[c1],
                shades[c0],
                shades[c1],
                pts[i],
                &input,
                &mut out,
            );
            input = out;
        }
        c0 = c1;
        c1 = i;
        edge = (edge + 1) % n;
    }
    let code = dest.codes[start..].iter().fold(0u32, |a, c| a | *c as u32);
    (dest.len() - start, code)
}

/// `P3iDecal_BarycentricShadeInterpolate`: a point's shade inside triangle
/// `p`, from its corners' (edge `e[0]` opposite corner 2, `e[1]` opposite 0,
/// `e[2]` opposite 1). RGB only.
fn barycentric(p: [Vec3; 3], e: [Vec3; 3], s: [u32; 3], normal: Vec3, at: Vec3) -> u32 {
    let b2 = normal.dot(e[0].cross(at - p[0]));
    let b0 = normal.dot(e[1].cross(at - p[1]));
    let b1 = normal.dot(e[2].cross(at - p[2]));
    let sum = b0 + b1 + b2;
    let mut out = 0;
    for shift in [16, 8, 0] {
        let ch = |x: u32| ((x >> shift) & 0xFF) as f32;
        let v = b0 * ch(s[0]) + b1 * ch(s[1]) + b2 * ch(s[2]);
        let c = if v < 0.0 {
            0
        } else {
            ((v / sum).round_ties_even() as u32).min(255)
        };
        out |= c << shift;
    }
    out
}

/// A quad's shade at a point inside it (the corner triangle it falls in).
fn shade_at(pts: &[Vec3], edges: &[Vec3], shades: &[u32], cross: Vec3, at: Vec3) -> u32 {
    if pts.len() <= 3 {
        return barycentric(
            [pts[0], pts[1], pts[2]],
            [edges[0], edges[1], edges[2]],
            [shades[0], shades[1], shades[2]],
            cross,
            at,
        );
    }
    let mid = pts[2] - pts[0];
    if (at - pts[0]).cross(mid).dot(cross) > 0.0 {
        barycentric(
            [pts[0], pts[1], pts[2]],
            [edges[0], edges[1], -mid],
            [shades[0], shades[1], shades[2]],
            cross,
            at,
        )
    } else {
        barycentric(
            [pts[0], pts[2], pts[3]],
            [mid, edges[2], edges[3]],
            [shades[0], shades[2], shades[3]],
            cross,
            at,
        )
    }
}

/// An edge to cross from one quad to the next (`P3tDecalEdge`).
#[derive(Clone, Debug, Default)]
struct Edge {
    depth: u32,
    from_index: usize,
    from_quad: usize,
    to_index: usize,
    to_quad: usize,
    from_clip: bool,
    to_clip: bool,
    pos_u: f32,
    pos_r: f32,
    up_u: f32,
    up_r: f32,
    right_u: f32,
    right_r: f32,
    pts: [Vec3; 2],
    entry_t: f32,
    exit_t: f32,
    decal_mid: Vec3,
    parent_normal: Vec3,
    code: u32,
    child_code: u32,
}

/// The edges waiting: a fixed pool, and the order they are taken in (by
/// depth, first come first served within one).
struct Edges {
    pool: Vec<Option<Edge>>,
    order: Vec<usize>,
}

impl Edges {
    fn new() -> Edges {
        Edges {
            pool: vec![None; MAX_EDGES],
            order: Vec::new(),
        }
    }
    /// A new edge at `depth` (after every edge no deeper), if one is free.
    fn make(&mut self, depth: u32) -> Option<usize> {
        let k = self.pool.iter().position(Option::is_none)?;
        let at = self
            .order
            .iter()
            .position(|&e| self.pool[e].as_ref().unwrap().depth > depth)
            .unwrap_or(self.order.len());
        self.order.insert(at, k);
        self.pool[k] = Some(Edge {
            depth,
            ..Edge::default()
        });
        Some(k)
    }
    fn remove(&mut self, k: usize) {
        self.order.retain(|&e| e != k);
        self.pool[k] = None;
    }
    fn get(&mut self, k: usize) -> &mut Edge {
        self.pool[k].as_mut().unwrap()
    }
}

/// The decal being built.
struct Build<'a> {
    quads: &'a Quads,
    slots: &'a [u32],
    p: &'a Params,
    lighting: bool,
    direction: Vec3,
    wrap_dot: f32,
    buf: Buf,
    planes: Vec<u32>,
    indices: Vec<u16>,
    edges: Edges,
}

impl Build<'_> {
    fn quad(&self, slot: usize) -> &Quad {
        &self.quads.quads[self.slots[slot] as usize]
    }

    fn corners(&self, q: &Quad) -> Vec<Vec3> {
        (0..q.corners()).map(|k| self.quads.corner(q, k)).collect()
    }

    /// `P3iDecal_ProcessAdjacency`: lays the decal on the quad edge `e`
    /// leads to, queueing the edges it goes on over.
    fn process(&mut self, e: usize) -> bool {
        let mut adjacencies: Vec<usize> = Vec::new();
        let (from_index, to_index, to_quad, from_quad) = {
            let edge = self.edges.get(e);
            (edge.from_index, edge.to_index, edge.to_quad, edge.from_quad)
        };
        if from_index != NONE {
            // The way in: clipped against, and kept for the seams.
            let edge = self.edges.get(e);
            edge.to_index = NONE;
            edge.from_clip = edge.to_clip;
            adjacencies.push(e);
        }
        let q = self.quads.quads[to_quad].clone();
        let pts = self.corners(&q);
        let n = pts.len();
        let quad_edges: Vec<Vec3> = (0..n).map(|j| pts[(j + 1) % n] - pts[j]).collect();
        let (mut normal, mut d) = self.quads.plane(q.plane);
        let mut negate = false;
        if q.flags & flag::TWO_SIDED != 0 {
            let reference = if from_quad == NONE {
                self.p.fwd
            } else {
                self.edges.get(e).parent_normal
            };
            negate = normal.dot(reference) < -REVERSE_2_SIDED;
            if negate {
                normal = -normal;
                d = -d;
            }
        }
        // The decal's frame on this quad: the first as given, the rest
        // unfolded about the edge they were reached over.
        let (position, fwd, up, right) = if from_quad == NONE {
            (self.p.position, self.p.fwd, self.p.up, self.p.right)
        } else {
            let edge = self.edges.get(e).clone();
            let u = (edge.pts[1] - edge.pts[0]).normalize();
            let r = u.cross(normal).normalize();
            (
                edge.pts[0] + u * edge.pos_u + r * edge.pos_r,
                normal,
                u * edge.up_u + r * edge.up_r,
                u * edge.right_u + r * edge.right_r,
            )
        };
        let quad_cross = quad_edges[0].cross(quad_edges[1]);

        for itr in 0..self.slots.len() {
            if itr == to_index || itr == from_index {
                continue;
            }
            let adj_quad = self.slots[itr] as usize;
            let a = self.quad(itr).clone();
            let na = a.corners();
            let adj_pts: Vec<Vec3> = (0..na).map(|k| self.quads.corner(&a, k)).collect();
            let (mut code, mut child_code) = (0u32, 0u32);
            let (depth, from_clip, to_clip, edge_pts);
            if a.plane & 0x7FFF_FFFF == q.plane & 0x7FFF_FFFF {
                // Coplanar: do they touch or overlap?
                depth = self.edges.get(e).depth;
                let mut clip_child = false;
                let mut found: Vec<Vec3> = Vec::new();
                for j in 0..na {
                    let adj_edge = adj_pts[(j + 1) % na] - adj_pts[j];
                    for k in 0..n {
                        if adj_edge.cross(quad_edges[k]).length_squared() < COPLANAR_EDGE {
                            // Parallel: collinear and overlapping?
                            let delta = pts[k] - adj_pts[j];
                            if adj_edge.cross(delta).length_squared() > COPLANAR_EDGE {
                                continue;
                            }
                            let t_length = adj_edge.length_squared();
                            let mut qp = [pts[k], pts[(k + 1) % n]];
                            let mut qt = [delta.dot(adj_edge), (qp[1] - adj_pts[j]).dot(adj_edge)];
                            if qt[0] > qt[1] {
                                qt.swap(0, 1);
                                qp.swap(0, 1);
                            }
                            if qt[1] < COLLINEAR_EDGE_OVERLAP
                                || qt[0] > t_length - COLLINEAR_EDGE_OVERLAP
                            {
                                continue;
                            }
                            code |= 1 << k;
                            child_code |= 1 << j;
                            if found.len() < 2 {
                                found.push(if qt[0] < 0.0 { adj_pts[j] } else { qp[0] });
                            }
                            if found.len() < 2 {
                                found.push(if qt[1] < t_length {
                                    adj_pts[(j + 1) % na]
                                } else {
                                    qp[1]
                                });
                            }
                        } else {
                            // Crossing?
                            let c1 = adj_edge.cross(pts[k] - adj_pts[j]);
                            let c2 = adj_edge.cross(pts[(k + 1) % n] - adj_pts[j]);
                            if c1.dot(c2) >= 0.0 {
                                continue;
                            }
                            let (t0, t1) = (c1.dot(quad_cross), c2.dot(quad_cross));
                            let at =
                                pts[k] * (t1 / (t1 - t0)) + pts[(k + 1) % n] * (-t0 / (t1 - t0));
                            let adj_t = (at - adj_pts[j]).dot(adj_edge) / adj_edge.length_squared();
                            if !(COPLANAR_ADJ_EDGE_LINEAR..=1.0 - COPLANAR_ADJ_EDGE_LINEAR)
                                .contains(&adj_t)
                            {
                                continue;
                            }
                            code |= 1 << k;
                            child_code |= 1 << j;
                            if found.len() < 2 {
                                found.push(at);
                            }
                            clip_child = true;
                        }
                    }
                }
                if found.len() < 2 {
                    continue;
                }
                from_clip = false;
                to_clip = clip_child;
                edge_pts = [found[0], found[1]];
            } else {
                depth = self.edges.get(e).depth + 1;
                let (mut an, mut ad) = self.quads.plane(a.plane);
                if a.flags & flag::TWO_SIDED != 0 && normal.dot(an) < REVERSE_2_SIDED {
                    an = -an;
                    ad = -ad;
                }
                // Each side of the other's plane.
                let mut quad_d = [0.0f32; 4];
                let (mut q_pos, mut q_neg, mut q_zero, mut q_edge) = (0, 0, 0, 0u32);
                for j in 0..n {
                    quad_d[j] = pts[j].dot(an) + ad;
                    if quad_d[j] < -ON_PLANE {
                        q_neg += 1;
                    } else if quad_d[j] > ON_PLANE {
                        q_pos += 1;
                    } else {
                        q_zero += 1;
                        q_edge |= 1 << j;
                    }
                }
                if q_zero < 2 && (q_pos == 0 || q_neg == 0) {
                    continue;
                }
                let mut adj_d = [0.0f32; 4];
                let (mut a_pos, mut a_neg, mut a_zero, mut a_edge) = (0, 0, 0, 0u32);
                for j in 0..na {
                    adj_d[j] = adj_pts[j].dot(normal) + d;
                    if adj_d[j] < -ON_PLANE {
                        a_neg += 1;
                    } else if adj_d[j] > ON_PLANE {
                        a_pos += 1;
                    } else {
                        a_zero += 1;
                        a_edge |= 1 << j;
                    }
                }
                if a_zero < 2 && (a_pos == 0 || a_neg == 0) {
                    continue;
                }
                // A surface-like join: both on the same side of each other.
                if (a_pos == 0 || q_pos == 0) && (a_neg == 0 || q_neg == 0) {
                    if a.flags & flag::TWO_SIDED == 0 {
                        continue;
                    }
                    std::mem::swap(&mut q_pos, &mut q_neg);
                    if (a_pos == 0 || q_pos == 0) && (a_neg == 0 || q_neg == 0) {
                        continue;
                    }
                    // (The original flips the neighbour's plane once per
                    // neighbour corner, and this quad's distances up to the
                    // neighbour's corner count.)
                    for v in quad_d.iter_mut().take(na) {
                        *v = -*v;
                    }
                    if na % 2 == 1 {
                        an = -an;
                    }
                }
                // Facing the way the decal came closely enough?
                let facing = self.direction.dot(an);
                if (self.p.flags & dflag::CULL_BACK_FACING != 0 && facing < -1e-3)
                    || facing < self.wrap_dot
                {
                    continue;
                }
                let overlap =
                    |pts: &[Vec3], ds: &[f32; 4], edge: u32, code: &mut u32| -> Vec<Vec3> {
                        let n = pts.len();
                        let mut found = Vec::new();
                        let mut prev_edge = true;
                        let mut prev = 0;
                        for j in 0..=n {
                            if edge & (1 << j) != 0 {
                                if edge & (1 << ((j + 1) % n)) != 0 {
                                    *code |= 1 << j;
                                }
                                found.push(pts[j]);
                                prev_edge = true;
                            } else {
                                let this = j % n;
                                if !prev_edge && ds[prev] * ds[this] < 0.0 {
                                    *code |= 1 << prev;
                                    found.push(
                                        pts[prev] * (ds[this] / (ds[this] - ds[prev]))
                                            + pts[this] * (-ds[prev] / (ds[this] - ds[prev])),
                                    );
                                }
                                prev_edge = false;
                                prev = this;
                            }
                        }
                        found
                    };
                let qp = overlap(&pts, &quad_d, q_edge, &mut code);
                if qp.len() < 2 {
                    continue;
                }
                let ap = overlap(&adj_pts, &adj_d, a_edge, &mut child_code);
                if ap.len() < 2 {
                    continue;
                }
                // Joins inside a quad don't need the decal to reach an edge.
                if q_pos > 0 && q_neg > 0 {
                    code = 0;
                }
                if a_pos > 0 && a_neg > 0 {
                    child_code = 0;
                }
                let line = normal.cross(an);
                let (mut qp, mut ap) = ([qp[0], qp[1]], [ap[0], ap[1]]);
                let (mut qt, mut at) = (
                    [qp[0].dot(line), qp[1].dot(line)],
                    [ap[0].dot(line), ap[1].dot(line)],
                );
                if at[0] > at[1] {
                    at.swap(0, 1);
                    ap.swap(0, 1);
                }
                if qt[0] > qt[1] {
                    qt.swap(0, 1);
                    qp.swap(0, 1);
                }
                if at[1] - qt[0] < EDGE_OVERLAP || qt[1] - at[0] < EDGE_OVERLAP {
                    continue;
                }
                from_clip = q_pos > 0 && q_neg > 0;
                to_clip = a_pos > 0 && a_neg > 0;
                edge_pts = [
                    if qt[0] > at[0] { qp[0] } else { ap[0] },
                    if qt[1] < at[1] { qp[1] } else { ap[1] },
                ];
            }

            let Some(k) = self.edges.make(depth) else {
                break;
            };
            // Where the edge crosses the decal's rectangle (in its frame).
            let loc: [[f32; 2]; 2] =
                edge_pts.map(|p| [(p - position).dot(right), (p - position).dot(up)]);
            let delta = [loc[1][0] - loc[0][0], loc[1][1] - loc[0][1]];
            let scale = [self.p.xscale, self.p.yscale];
            let mut entry = [0.0f32; 2];
            let mut exit = [0.0f32; 2];
            let mut invalid = false;
            for axis in 0..2 {
                if invalid {
                    break;
                }
                let s = scale[axis];
                if delta[axis] < -PARALLEL_EDGE_DELTA {
                    entry[axis] = (s - loc[0][axis]) / delta[axis];
                    exit[axis] = (-s - loc[0][axis]) / delta[axis];
                    invalid = entry[axis] >= 1.0 || exit[axis] <= 0.0;
                } else if delta[axis] > PARALLEL_EDGE_DELTA {
                    entry[axis] = (-s - loc[0][axis]) / delta[axis];
                    exit[axis] = (s - loc[0][axis]) / delta[axis];
                    invalid = entry[axis] >= 1.0 || exit[axis] <= 0.0;
                } else {
                    entry[axis] = -1e9;
                    exit[axis] = 1e9;
                    invalid = loc[0][axis] <= -s || loc[0][axis] >= s;
                }
            }
            let (entry_t, exit_t) = (entry[0].max(entry[1]), exit[0].min(exit[1]));
            if invalid || exit_t < entry_t {
                self.edges.remove(k);
                continue;
            }
            let vec = edge_pts[1] - edge_pts[0];
            let u = vec.normalize();
            let r = u.cross(normal).normalize();
            let rel = position - edge_pts[0];
            *self.edges.get(k) = Edge {
                depth,
                from_quad: to_quad,
                from_index: to_index,
                to_quad: adj_quad,
                to_index: itr,
                from_clip,
                to_clip,
                pts: edge_pts,
                code,
                child_code,
                parent_normal: normal,
                entry_t,
                exit_t,
                decal_mid: edge_pts[0] + vec * ((entry_t + exit_t) / 2.0),
                pos_u: rel.dot(u),
                pos_r: rel.dot(r),
                up_u: up.dot(u),
                up_r: up.dot(r),
                right_u: right.dot(u),
                right_r: right.dot(r),
            };
            adjacencies.push(k);
            if adjacencies.len() >= MAX_ADJACENCIES {
                break;
            }
        }

        let success = self.lay(
            e,
            &q,
            &pts,
            &quad_edges,
            quad_cross,
            normal,
            d,
            negate,
            position,
            fwd,
            up,
            right,
            &mut adjacencies,
        );
        // Done with the edges out: all of them on failure, else those over
        // edges the decal didn't reach.
        let (ok, quad_code) = success.map_or((false, 0), |c| (true, c));
        for &k in &adjacencies {
            let edge = self.edges.get(k);
            if edge.to_index == NONE {
                continue;
            }
            if !ok || (edge.code != 0 && quad_code & edge.code == 0) {
                self.edges.remove(k);
            }
        }
        ok
    }

    /// Projects the decal onto the quad, clips it to the quad and to the
    /// joins that cut it, and adds the result. Returns the quad edges it
    /// reached, or `None` if nothing of it lies on this quad.
    #[allow(clippy::too_many_arguments)]
    fn lay(
        &mut self,
        e: usize,
        q: &Quad,
        pts: &[Vec3],
        quad_edges: &[Vec3],
        quad_cross: Vec3,
        normal: Vec3,
        d: f32,
        negate: bool,
        position: Vec3,
        fwd: Vec3,
        up: Vec3,
        right: Vec3,
        adjacencies: &mut [usize],
    ) -> Option<u32> {
        let (x, y) = (self.p.xscale, self.p.yscale);
        let corners = [
            (-x, y, [1.0, 0.0]),
            (x, y, [0.0, 0.0]),
            (x, -y, [0.0, 1.0]),
            (-x, -y, [1.0, 1.0]),
        ];
        let fwd_dot = fwd.dot(normal);
        if fwd_dot.abs() < 1e-3 {
            return None;
        }
        let mut source = Buf::new(4);
        for (cx, cy, uv) in corners {
            let p = position + right * cx + up * cy;
            source.push(p - fwd * ((p.dot(normal) + d) / fwd_dot), uv, 0, 0);
        }
        let shades = &q.shades[..pts.len()];
        let mut clip_edges = 0;
        for &k in adjacencies.iter() {
            let edge = self.edges.get(k);
            if !edge.from_clip {
                continue;
            }
            if edge.entry_t < -EDGE_CLIP || edge.exit_t > 1.0 + EDGE_CLIP {
                edge.from_clip = false;
                continue;
            }
            clip_edges += 1;
        }
        let (from_index, child_code, entered) = {
            let edge = self.edges.get(e);
            let entered = if edge.from_index == NONE {
                self.p.position
            } else {
                edge.decal_mid
            };
            (edge.from_index, edge.child_code, entered)
        };
        let base = self.buf.len();
        let (mut remaining, quad_code, mut current) = if clip_edges == 0 {
            let (n, c) = clip_to_polygon(pts, shades, &source, &mut self.buf);
            (n, c, None)
        } else {
            let mut out = Buf::new(CLIP_BUFFER);
            let (n, c) = clip_to_polygon(pts, shades, &source, &mut out);
            (n, c, Some(out))
        };
        if from_index != NONE && child_code != 0 && quad_code & child_code == 0 {
            // Reached over an edge it doesn't touch here.
            self.truncate(base);
            return None;
        }
        let mut clipped = 0;
        let mut itr = 0;
        while remaining > 0 && clipped < clip_edges {
            let input = current.take().unwrap();
            while !self.edges.get(adjacencies[itr]).from_clip {
                itr += 1;
                if itr >= adjacencies.len() {
                    self.truncate(base);
                    return None;
                }
            }
            let edge = self.edges.get(adjacencies[itr]).clone();
            // The way in keeps the side its parent was on; a way out, the
            // side the decal came in from.
            let side = if edge.to_index == NONE {
                edge.parent_normal + edge.decal_mid
            } else {
                entered
            };
            if clipped + 1 >= clip_edges {
                remaining = clip_to_plane(
                    0,
                    edge.pts[0],
                    edge.pts[1],
                    0,
                    0,
                    side,
                    &input,
                    &mut self.buf,
                );
            } else {
                let mut out = Buf::new(CLIP_BUFFER);
                remaining =
                    clip_to_plane(0, edge.pts[0], edge.pts[1], 0, 0, side, &input, &mut out);
                current = Some(out);
            }
            itr += 1;
            clipped += 1;
        }
        let count = self.buf.len() - base;
        if count == 0 {
            return None;
        }
        let plane = if negate {
            q.plane ^ 0x8000_0000
        } else {
            q.plane
        };
        for j in base..self.buf.len() {
            self.planes.push(plane);
            if self.lighting && self.buf.shades[j] == 0 {
                self.buf.shades[j] =
                    shade_at(pts, quad_edges, shades, quad_cross, self.buf.points[j]);
            }
        }
        for j in 0..count.saturating_sub(2) {
            if self.indices.len() + 3 > MAX_TRI_INDICES {
                break;
            }
            self.indices.extend_from_slice(&[
                base as u16,
                (base + j + 1) as u16,
                (base + j + 2) as u16,
            ]);
        }
        Some(quad_code)
    }

    fn truncate(&mut self, n: usize) {
        self.buf.truncate(n);
        self.planes.truncate(n);
    }
}

/// `P3rCreateDecal`: builds a decal on `quad` (or the first quad a ray into
/// the wall from `position` finds). `None` if nothing could be laid.
pub fn build(quads: &Quads, quad: Option<u32>, p: &Params) -> Option<Mesh> {
    let direction = match p.direction.and_then(|d| d.try_normalize()) {
        Some(d) => -d,
        None => p.fwd,
    };
    let ray = p.fwd * -10.0;
    let (quad, position) = match quad {
        None => {
            let h = quads.ray(p.position, ray, SKIP)?;
            (h.quad, h.point)
        }
        Some(q) if quads.quads.get(q as usize)?.flags & flag::NO_DECAL != 0 => return None,
        Some(q) => (q, p.position),
    };
    let radius = (p.xscale * p.xscale + p.yscale * p.yscale).sqrt();
    let mut hits = quads.sphere(position, radius, ray, SKIP);
    if hits.is_empty() {
        return None;
    }
    hits.truncate(MAX_QUADS);
    let slots: Vec<u32> = hits.iter().map(|h| h.quad).collect();
    let lighting = p.flags & dflag::FULL_BRIGHT == 0;
    let mut b = Build {
        quads,
        slots: &slots,
        p,
        lighting,
        direction,
        wrap_dot: p
            .wrap
            .clamp(0.5f32.to_radians(), std::f32::consts::PI)
            .cos(),
        buf: Buf::new(BUILD_BUFFER),
        planes: Vec::new(),
        indices: Vec::new(),
        edges: Edges::new(),
    };
    if p.flags & dflag::IGNORE_ADJACENCY != 0 {
        ignore_adjacency(&mut b, position);
    } else {
        // Start from the quad struck (or, missing from the list, the
        // nearest), and go over its edges, the nearest planes first.
        let start = slots.iter().position(|&s| s == quad).unwrap_or(0);
        let mut processed = vec![false; slots.len()];
        if let Some(k) = b.edges.make(0) {
            *b.edges.get(k) = Edge {
                depth: 0,
                from_index: NONE,
                from_quad: NONE,
                to_index: start,
                to_quad: slots[start] as usize,
                parent_normal: p.fwd,
                ..Edge::default()
            };
        }
        while let Some(&k) = b.edges.order.first() {
            let slot = b.edges.get(k).to_index;
            if !std::mem::replace(&mut processed[slot], true) {
                b.process(k);
            }
            b.edges.remove(k);
        }
    }
    if b.buf.len() == 0 {
        return None;
    }
    Some(weld(&b, quad))
}

/// The pass for character shadows: every quad around, straight on, no
/// wrapping.
fn ignore_adjacency(b: &mut Build<'_>, position: Vec3) {
    let p = *b.p;
    let corners = [
        (-p.xscale, p.yscale, [1.0, 0.0]),
        (p.xscale, p.yscale, [0.0, 0.0]),
        (p.xscale, -p.yscale, [0.0, 1.0]),
        (-p.xscale, -p.yscale, [1.0, 1.0]),
    ];
    for slot in 0..b.slots.len() {
        let q = b.quad(slot).clone();
        let (mut n, mut d) = b.quads.plane(q.plane);
        let mut fwd_dot = p.fwd.dot(n);
        if q.flags & flag::TWO_SIDED != 0 && fwd_dot < 0.0 {
            n = -n;
            d = -d;
            fwd_dot = -fwd_dot;
        }
        if p.flags & dflag::CULL_BACK_FACING != 0 && fwd_dot < 1e-3 {
            continue;
        }
        let pts = b.corners(&q);
        let mut source = Buf::new(4);
        for (cx, cy, uv) in corners {
            let mut at = position + p.right * cx + p.up * cy;
            if fwd_dot.abs() >= 1e-3 {
                at -= p.fwd * ((at.dot(n) + d) / fwd_dot);
            }
            source.push(at, uv, 0, 0);
        }
        let base = b.buf.len();
        let shades = &q.shades[..pts.len()];
        let (count, _) = clip_to_polygon(&pts, shades, &source, &mut b.buf);
        if count == 0 {
            continue;
        }
        let edges: Vec<Vec3> = (0..pts.len())
            .map(|j| pts[(j + 1) % pts.len()] - pts[j])
            .collect();
        let cross = edges[0].cross(edges[1]);
        for j in base..b.buf.len() {
            b.buf.points[j] += p.fwd * POINT_FUDGE;
            b.planes.push(q.plane);
            if b.lighting && b.buf.shades[j] == 0 {
                // (Here the corner triangle is chosen against the plane.)
                let s = if pts.len() > 3 {
                    let mid = pts[2] - pts[0];
                    if (b.buf.points[j] - pts[0]).cross(mid).dot(n) > 0.0 {
                        barycentric(
                            [pts[0], pts[1], pts[2]],
                            [edges[0], edges[1], -mid],
                            [shades[0], shades[1], shades[2]],
                            cross,
                            b.buf.points[j],
                        )
                    } else {
                        barycentric(
                            [pts[0], pts[2], pts[3]],
                            [mid, edges[2], edges[3]],
                            [shades[0], shades[2], shades[3]],
                            cross,
                            b.buf.points[j],
                        )
                    }
                } else {
                    shade_at(&pts, &edges, shades, cross, b.buf.points[j])
                };
                b.buf.shades[j] = s;
            }
        }
        for j in 0..count.saturating_sub(2) {
            if b.indices.len() + 3 > MAX_TRI_INDICES {
                break;
            }
            b.indices.extend_from_slice(&[
                base as u16,
                (base + j + 1) as u16,
                (base + j + 2) as u16,
            ]);
        }
    }
}

/// Welds the points: exact duplicates (same place, texture coordinate and
/// shade) share a vertex; points merely in the same place snap to the first
/// one's; each kept point lifts off the wall along the mean of the planes
/// that meet there.
fn weld(b: &Build<'_>, quad: u32) -> Mesh {
    let buf = &b.buf;
    let n = buf.len();
    let adjacency = b.p.flags & dflag::IGNORE_ADJACENCY == 0;
    let mut duplicate = vec![false; n];
    let mut coincident = vec![false; n];
    let mut index = vec![0u32; n];
    let mut points = buf.points.clone();
    let mut vert_count = 0u32;
    for i in 0..n {
        if duplicate[i] {
            continue;
        }
        let mut planes = vec![b.planes[i]];
        for j in i + 1..n {
            if duplicate[j] {
                continue;
            }
            let dp = (buf.points[j] - buf.points[i]).abs();
            if dp.x > SHARED_POINT || dp.y > SHARED_POINT || dp.z > SHARED_POINT {
                continue;
            }
            let same = (buf.uvs[j][0] - buf.uvs[i][0]).abs() < SHARED_UV
                && (buf.uvs[j][1] - buf.uvs[i][1]).abs() < SHARED_UV
                && (!b.lighting || shade_eq(buf.shades[j], buf.shades[i]));
            if adjacency {
                planes.push(b.planes[j]);
            }
            if same {
                duplicate[j] = true;
                index[j] = vert_count;
                coincident[j] = false;
            } else if !std::mem::replace(&mut coincident[j], true) {
                index[j] |= (i as u32) << 16;
            }
            if adjacency && planes.len() >= MAX_SHARED_PLANES {
                break;
            }
        }
        index[i] |= vert_count & 0xFFFF;
        vert_count += 1;
        if adjacency {
            let offset: Vec3 = planes.iter().map(|&pl| b.quads.plane(pl).0).sum();
            let len2 = offset.length_squared();
            if len2 > OFFSET_NORMAL_LENGTH * OFFSET_NORMAL_LENGTH {
                points[i] += offset * (POINT_FUDGE / len2.sqrt());
            }
        }
    }
    // Snap each coincident point to its master's place (in chains).
    for i in 0..n {
        let mut k = i;
        while coincident[k] {
            let next = (index[k] >> 16) as usize;
            if next >= k {
                break;
            }
            k = next;
        }
        if k != i {
            points[i] = points[k];
        }
    }
    let count = vert_count as usize;
    let mut mesh = Mesh {
        quad: Some(quad),
        positions: vec![Vec3::ZERO; count],
        uvs: vec![[0.0; 2]; count],
        shades: vec![0; count],
        indices: Vec::with_capacity(b.indices.len()),
    };
    let mut stored = vec![false; count];
    for i in 0..n {
        let k = (index[i] & 0xFFFF) as usize;
        if std::mem::replace(&mut stored[k], true) {
            continue;
        }
        mesh.positions[k] = points[i];
        mesh.uvs[k] = buf.uvs[i];
        mesh.shades[k] = if b.lighting {
            buf.shades[i]
        } else {
            0xFFFF_FFFF
        };
    }
    mesh.indices = b
        .indices
        .iter()
        .map(|&t| (index[t as usize] & 0xFFFF) as u16)
        .collect();
    mesh
}

/// The decal's frame from a surface normal and an up hint
/// (`P3rCreateEffect`'s decal block): right = normal × up (or × world up),
/// up = right × normal, optionally turned by `rotation` radians. `None` if
/// the normal is zero.
pub fn frame(normal: Vec3, upvector: Vec3, rotation: Option<f32>) -> Option<(Vec3, Vec3)> {
    let mut right = normal
        .cross(upvector)
        .try_normalize()
        .or_else(|| Vec3::new(-normal.z, 0.0, normal.x).try_normalize());
    if right.is_none() {
        if normal.y == 0.0 {
            return None;
        }
        right = Some(Vec3::Z);
    }
    let mut right = right.unwrap();
    let mut up = right.cross(normal);
    if let Some(theta) = rotation {
        let (s, c) = theta.sin_cos();
        let (u, r) = (up, right);
        up = u * c + r * s;
        right = r * c - u * s;
    }
    Some((up, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quads::tests::{room, wall};

    fn params(position: Vec3, fwd: Vec3, half: f32, wrap_deg: f32) -> Params {
        let (up, right) = frame(fwd, Vec3::ZERO, None).unwrap();
        Params {
            flags: dflag::FULL_BRIGHT,
            wrap: wrap_deg.to_radians(),
            direction: None,
            position,
            fwd,
            up,
            right,
            xscale: half,
            yscale: half,
        }
    }

    #[test]
    fn a_decal_on_a_flat_wall_is_one_quad_lifted_off_it() {
        let q = room(&[(wall(), 0)]);
        let p = params(Vec3::new(1.0, 2.0, 0.0), Vec3::Z, 1.0, 60.0);
        let m = build(&q, Some(0), &p).unwrap();
        assert_eq!(m.positions.len(), 4);
        assert_eq!(m.indices.len(), 6);
        for (k, (cx, cy, uv)) in [
            (-1.0, 1.0, [1.0, 0.0]),
            (1.0, 1.0, [0.0, 0.0]),
            (1.0, -1.0, [0.0, 1.0]),
            (-1.0, -1.0, [1.0, 1.0]),
        ]
        .into_iter()
        .enumerate()
        {
            let want = p.position + p.right * cx + p.up * cy + Vec3::Z * POINT_FUDGE;
            let got = m
                .positions
                .iter()
                .position(|v| v.distance(want) < 1e-4)
                .unwrap_or_else(|| panic!("corner {k} missing: {:?}", m.positions));
            assert_eq!(m.uvs[got], uv);
        }
    }

    #[test]
    fn a_decal_over_a_coplanar_seam_shares_the_seam() {
        let left = [
            Vec3::new(-10.0, -10.0, 0.0),
            Vec3::new(0.0, -10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(-10.0, 10.0, 0.0),
        ];
        let right = left.map(|v| v + Vec3::new(10.0, 0.0, 0.0));
        let mut q = room(&[(left, 0), (right, 0)]);
        // The same plane, as the level's coplanar quads share one.
        q.quads[1].plane = 0;
        let m = build(
            &q,
            Some(0),
            &params(Vec3::new(0.0, 0.0, 0.0), Vec3::Z, 1.0, 60.0),
        )
        .unwrap();
        assert_eq!(m.indices.len(), 12, "two quads' pieces");
        assert_eq!(
            m.positions.len(),
            6,
            "the seam's points shared: {:?}",
            m.positions
        );
        assert!(m.positions.iter().all(|p| (p.z - POINT_FUDGE).abs() < 1e-4));
    }

    /// A wall z = 0 (x < 10) and a side wall x = 10 going back toward +z,
    /// meeting at an inside corner; a decal by the corner.
    fn corner_room() -> Quads {
        let side = [
            Vec3::new(10.0, -10.0, 0.0),
            Vec3::new(10.0, -10.0, 20.0),
            Vec3::new(10.0, 10.0, 20.0),
            Vec3::new(10.0, 10.0, 0.0),
        ];
        room(&[(wall(), 0), (side, 0)])
    }

    #[test]
    fn a_decal_wraps_into_a_corner_within_its_wrap_angle() {
        let q = corner_room();
        // Inside corner: the side wall faces -x, at 90° to the wall.
        let p = params(Vec3::new(9.5, 0.0, 0.0), Vec3::Z, 1.0, 120.0);
        let m = build(&q, Some(0), &p).unwrap();
        let on_side: Vec<&Vec3> = m.positions.iter().filter(|v| v.z > 0.1).collect();
        assert!(
            !on_side.is_empty(),
            "wraps onto the side wall: {:?}",
            m.positions
        );
        // Folded: the 0.5 beyond the corner runs up the side wall.
        let far = on_side.iter().map(|v| v.z).fold(0.0, f32::max);
        assert!((far - 0.5).abs() < 0.06, "{far}");
        // With a 60° wrap it stops at the corner.
        let m = build(
            &q,
            Some(0),
            &params(Vec3::new(9.5, 0.0, 0.0), Vec3::Z, 1.0, 60.0),
        )
        .unwrap();
        assert!(
            m.positions.iter().all(|v| v.x <= 10.0 + 1e-3 && v.z < 0.1),
            "{:?}",
            m.positions
        );
    }

    #[test]
    fn an_oblique_shot_wraps_by_its_own_direction() {
        let q = corner_room();
        // A shot travelling toward +x into the corner: the side wall faces
        // back at it, so even a 60° wrap carries onto it…
        let mut p = params(Vec3::new(9.5, 0.0, 0.0), Vec3::Z, 1.0, 60.0);
        p.direction = Some(Vec3::new(1.0, 0.0, -1.0));
        let m = build(&q, Some(0), &p).unwrap();
        assert!(m.positions.iter().any(|v| v.z > 0.1), "{:?}", m.positions);
        // …while one travelling toward -x doesn't wrap even at 120°.
        let mut p = params(Vec3::new(9.5, 0.0, 0.0), Vec3::Z, 1.0, 120.0);
        p.direction = Some(Vec3::new(-1.0, 0.0, -1.0));
        let m = build(&q, Some(0), &p).unwrap();
        assert!(m.positions.iter().all(|v| v.z < 0.1), "{:?}", m.positions);
    }

    #[test]
    fn no_decal_and_invisible_quads_take_none() {
        let q = room(&[(wall(), flag::NO_DECAL)]);
        assert!(build(&q, Some(0), &params(Vec3::ZERO, Vec3::Z, 1.0, 60.0)).is_none());
        let q = room(&[(wall(), flag::INVISIBLE)]);
        assert!(build(
            &q,
            None,
            &params(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, 1.0, 60.0)
        )
        .is_none());
    }

    #[test]
    fn a_lit_decal_takes_the_quads_shades() {
        let mut q = room(&[(wall(), 0)]);
        // Corners: black, red, white, green (the shade is opaque).
        q.quads[0].shades = [0xFF00_0000, 0xFFFF_0000, 0xFFFF_FFFF, 0xFF00_FF00];
        let mut p = params(Vec3::new(0.0, 0.0, 0.0), Vec3::Z, 1.0, 60.0);
        p.flags = 0;
        let m = build(&q, Some(0), &p).unwrap();
        // The centre of the wall: on the 0-2 diagonal, halfway: grey-ish.
        for (v, s) in m.positions.iter().zip(&m.shades) {
            let r = (s >> 16) & 0xFF;
            assert!(r > 100 && r < 160, "{v:?} {s:08x}");
        }
        assert_eq!(
            shade_lerp(0xFF00_0000, 0xFFFF_FFFF, 0.5),
            0xFF7F_7F7F,
            "256ths, truncated"
        );
    }
}
