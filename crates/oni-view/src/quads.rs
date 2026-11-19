//! The level's quads as the engine's collision queries see them
//! (`levels/NN/quads.bin`, `docs/ASSET_FORMAT.md`): every quad with its
//! corners, flags, plane and baked shades — what decals are built on
//! (`docs/PARTICLES.md`, *Decals*). The two queries are the original's
//! (`BFW_Akira_Collision.c`): a sphere swept along a vector, and a ray.

use std::path::Path;

use glam::Vec3;

/// `AKcGQ_Flag_*` (`BFW_Akira.h`).
pub mod flag {
    pub const JELLO: u32 = 1 << 5;
    pub const TRIANGLE: u32 = 1 << 6;
    pub const TRANSPARENT: u32 = 1 << 7;
    pub const DRAW_BOTH_SIDES: u32 = 1 << 9;
    pub const NO_COLLISION: u32 = 1 << 11;
    pub const INVISIBLE: u32 = 1 << 13;
    pub const NO_OBJECT_COLLISION: u32 = 1 << 14;
    pub const NO_CHARACTER_COLLISION: u32 = 1 << 15;
    /// Set by the original's importer: a floor or ceiling (`|n.y| > 0.5`),
    /// else a wall.
    #[allow(dead_code)] // (Everything that isn't a wall: queries skip walls.)
    pub const FLOOR_CEILING: u32 = 1 << 19;
    pub const WALL: u32 = 1 << 20;
    pub const BROKEN_GLASS: u32 = 1 << 18;
    pub const NO_DECAL: u32 = 1 << 23;
    /// What objects (and particles) pass through.
    pub const OBJ_COL_SKIP: u32 = NO_COLLISION | NO_OBJECT_COLLISION | BROKEN_GLASS;
    /// Drawn from both sides (the decal code's two-sided test: without jello).
    pub const TWO_SIDED: u32 = TRANSPARENT | DRAW_BOTH_SIDES;
    /// What characters pass through.
    pub const CHR_COL_SKIP: u32 = NO_COLLISION | NO_CHARACTER_COLLISION | BROKEN_GLASS;
    /// The room graph's portals (door, ghost, stairs up / down): the
    /// original's importer leaves them out of the octree every query uses,
    /// so nothing ever meets them (`AKcGQ_Flag_PathfindingMask`).
    pub const PATHFINDING: u32 = 0xF;
    /// Two-sided as collision counts it (`AKcGQ_Flag_2Sided`: with jello).
    pub const AK_2SIDED: u32 = TRANSPARENT | DRAW_BOTH_SIDES | JELLO;
}

/// `UUcEpsilon` and the "sloppy" one.
const EPSILON: f32 = 1.0 / 1024.0;
const SLOPPY: f32 = 0.1;
/// At most this many quads come back from a sphere (`AKcMaxNumCollisions`).
pub const MAX_COLLISIONS: usize = 255;

#[derive(Clone, Debug)]
// (Texture and object: for materials and moving objects, still to use.)
#[allow(dead_code)]
pub struct Quad {
    pub points: [u32; 4],
    pub flags: u32,
    /// Plane index; the top bit negates it.
    pub plane: u32,
    /// Baked corner shades (`0xAARRGGBB`).
    pub shades: [u32; 4],
    pub texture: u16,
    pub object_tag: u32,
    pub min: Vec3,
    pub max: Vec3,
    /// The script reference that shows and hides it (`env_show`).
    pub env_ref: Option<u32>,
}

impl Quad {
    /// Corners used: a triangle's three, else four.
    pub fn corners(&self) -> usize {
        if self.flags & flag::TRIANGLE != 0 {
            3
        } else {
            4
        }
    }
}

/// A quad a query found: which, where, how far (the sort key's distance).
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub quad: u32,
    pub point: Vec3,
    pub distance: f32,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct Quads {
    pub points: Vec<Vec3>,
    pub planes: Vec<[f32; 4]>,
    /// Per texture-map entry, the texture's name (as its PNG is named).
    pub textures: Vec<String>,
    pub quads: Vec<Quad>,
    /// Which quads lie over each square of the ground (a lookup only: the
    /// queries' answers don't depend on it).
    grid: Grid,
}

/// Quads by the squares (in x and z) their boxes cover.
#[derive(Clone, Debug, Default)]
struct Grid {
    origin: [f32; 2],
    size: [usize; 2],
    cells: Vec<Vec<u32>>,
}

/// A grid square's side.
const CELL: f32 = 32.0;

impl Grid {
    fn build(quads: &[Quad]) -> Grid {
        if quads.is_empty() {
            return Grid::default();
        }
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for q in quads {
            lo = [lo[0].min(q.min.x), lo[1].min(q.min.z)];
            hi = [hi[0].max(q.max.x), hi[1].max(q.max.z)];
        }
        let size = [((hi[0] - lo[0]) / CELL) as usize + 1, ((hi[1] - lo[1]) / CELL) as usize + 1];
        let mut g = Grid { origin: lo, size, cells: vec![Vec::new(); size[0] * size[1]] };
        for (k, q) in quads.iter().enumerate().filter(|(_, q)| q.flags & flag::PATHFINDING == 0) {
            let (x0, z0, x1, z1) = g.span(q.min, q.max);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    g.cells[z * size[0] + x].push(k as u32);
                }
            }
        }
        g
    }

    /// The squares a box covers (clamped to the grid).
    fn span(&self, lo: Vec3, hi: Vec3) -> (usize, usize, usize, usize) {
        let cell = |v: f32, o: f32, n: usize| (((v - o) / CELL).floor().max(0.0) as usize).min(n - 1);
        (
            cell(lo.x, self.origin[0], self.size[0]),
            cell(lo.z, self.origin[1], self.size[1]),
            cell(hi.x, self.origin[0], self.size[0]),
            cell(hi.z, self.origin[1], self.size[1]),
        )
    }
}

impl Quads {
    pub fn load(path: &Path) -> Option<Quads> {
        Quads::parse(&std::fs::read(path).ok()?)
    }

    pub fn parse(b: &[u8]) -> Option<Quads> {
        if b.get(0..4)? != b"ONIQ" {
            return None;
        }
        let mut at = 4;
        let mut u = || -> Option<u32> {
            let v = u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?);
            at += 4;
            Some(v)
        };
        let (np, npl, nt, nq) = (u()? as usize, u()? as usize, u()? as usize, u()? as usize);
        let f = |o: usize| -> Option<f32> { Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
        let points = (0..np).map(|i| Some(Vec3::new(f(at + i * 12)?, f(at + i * 12 + 4)?, f(at + i * 12 + 8)?))).collect::<Option<Vec<_>>>()?;
        at += np * 12;
        let planes = (0..npl).map(|i| Some([f(at + i * 16)?, f(at + i * 16 + 4)?, f(at + i * 16 + 8)?, f(at + i * 16 + 12)?])).collect::<Option<Vec<_>>>()?;
        at += npl * 16;
        let mut textures = Vec::with_capacity(nt);
        for _ in 0..nt {
            let n = *b.get(at)? as usize;
            textures.push(String::from_utf8_lossy(b.get(at + 1..at + 1 + n)?).into_owned());
            at += 1 + n;
        }
        let mut quads = Vec::with_capacity(nq);
        for _ in 0..nq {
            let w = |k: usize| u32::from_le_bytes(b[at + k * 4..at + k * 4 + 4].try_into().unwrap());
            if at + 76 > b.len() {
                return None;
            }
            quads.push(Quad {
                points: [w(0), w(1), w(2), w(3)],
                flags: w(4),
                plane: w(5),
                shades: [w(6), w(7), w(8), w(9)],
                texture: w(10) as u16,
                object_tag: w(11),
                min: Vec3::new(f(at + 48)?, f(at + 52)?, f(at + 56)?),
                max: Vec3::new(f(at + 60)?, f(at + 64)?, f(at + 68)?),
                env_ref: Some(w(18)).filter(|&r| r != u32::MAX),
            });
            at += 76;
        }
        let grid = Grid::build(&quads);
        Some(Quads { points, planes, textures, quads, grid })
    }

    /// Rebuilds the lookup grid (after adding quads by hand).
    #[cfg(test)]
    pub fn rebuild_grid(&mut self) {
        self.grid = Grid::build(&self.quads);
    }

    /// The quads whose boxes might meet the box `lo`–`hi`, in quad order.
    fn candidates(&self, lo: Vec3, hi: Vec3) -> Vec<u32> {
        if self.grid.cells.is_empty() {
            return (0..self.quads.len() as u32).filter(|&k| self.quads[k as usize].flags & flag::PATHFINDING == 0).collect();
        }
        let (x0, z0, x1, z1) = self.grid.span(lo, hi);
        let mut out: Vec<u32> = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                out.extend_from_slice(&self.grid.cells[z * self.grid.size[0] + x]);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// A plane by its index (negated if its top bit is set): normal, d.
    pub fn plane(&self, index: u32) -> (Vec3, f32) {
        let [a, b, c, d] = self.planes[(index & 0x7FFF_FFFF) as usize];
        if index & 0x8000_0000 != 0 {
            (Vec3::new(-a, -b, -c), -d)
        } else {
            (Vec3::new(a, b, c), d)
        }
    }

    /// A quad's corner.
    pub fn corner(&self, q: &Quad, k: usize) -> Vec3 {
        self.points[q.points[k] as usize]
    }

    /// The quads a sphere touches swept along `vector` (`AKrCollision_Sphere`
    /// with `AKrGQ_SphereVector`), skipping those with any of `skip`, nearest
    /// first by the touch point's distance from the centre in 1/256ths.
    /// (The original's ties fall in its octree's order; here by quad index.)
    pub fn sphere(&self, center: Vec3, radius: f32, vector: Vec3, skip: u32) -> Vec<Hit> {
        let mut hits = Vec::new();
        let end = center + vector;
        let pad = Vec3::splat(radius + 0.01);
        for k in self.candidates(center.min(end) - pad, center.max(end) + pad) {
            let q = &self.quads[k as usize];
            if q.flags & skip != 0 {
                continue;
            }
            if let Some(point) = self.sphere_vector(q, center, radius, vector) {
                if hits.len() >= MAX_COLLISIONS {
                    break;
                }
                hits.push(Hit { quad: k, point, distance: point.distance(center) });
            }
        }
        hits.sort_by_key(|h| (round_256(h.distance), h.quad));
        hits
    }

    /// The first quad a ray from `start` along `vector` hits
    /// (`AKrCollision_Point`), skipping those with any of `skip`: nearest by
    /// the hit's x + y + z distance from the start, in 1/256ths.
    pub fn ray(&self, start: Vec3, vector: Vec3, skip: u32) -> Option<Hit> {
        let end = start + vector;
        // (A quick reject on the boxes, wide enough never to change the
        // answer.)
        let (lo, hi) = (start.min(end) - Vec3::splat(0.25), start.max(end) + Vec3::splat(0.25));
        self.candidates(lo, hi)
            .into_iter()
            .map(|k| (k as usize, &self.quads[k as usize]))
            .filter(|(_, q)| q.flags & skip == 0 && q.min.cmple(hi).all() && q.max.cmpge(lo).all())
            .filter_map(|(k, q)| {
                let point = self.quad_line(q, start, end)?;
                let d = point - start;
                Some(Hit { quad: k as u32, point, distance: d.x.abs() + d.y.abs() + d.z.abs() })
            })
            .min_by_key(|h| (round_256(h.distance), h.quad))
    }

    /// `AKrGQ_SphereVector`: whether a sphere at `center` swept along
    /// `vector` touches quad `q`, and where.
    pub fn sphere_vector(&self, q: &Quad, center: Vec3, radius: f32, vector: Vec3) -> Option<Vec3> {
        let end = center + vector;
        // (The raw plane, its sign bit ignored.)
        let [a, b, c, d] = self.planes[(q.plane & 0x7FFF_FFFF) as usize];
        let n = Vec3::new(a, b, c);
        let (start_d, end_d) = (n.dot(center) + d, n.dot(end) + d);
        if (start_d > radius && end_d > radius) || (start_d < -radius && end_d < -radius) {
            return None;
        }
        let (lo, hi) = (q.min - Vec3::splat(radius + 0.001), q.max + Vec3::splat(radius + 0.001));
        for i in 0..3 {
            if (center[i] < lo[i] && end[i] < lo[i]) || (center[i] > hi[i] && end[i] > hi[i]) {
                return None;
            }
        }
        if start_d.abs() <= radius {
            let p = center - n * start_d;
            if self.point_in_quad(q, p) {
                return Some(p);
            }
        }
        if end_d.abs() <= radius {
            let p = end - n * end_d;
            if self.point_in_quad(q, p) {
                return Some(p);
            }
        }
        let r2 = radius * radius;
        let len2 = vector.length_squared();
        let v_dot_c = vector.dot(center);
        for i in 0..4 {
            let c = self.corner(q, i);
            if c.distance_squared(center) <= r2 || c.distance_squared(end) <= r2 {
                return Some(c);
            }
            let t = -(v_dot_c - vector.dot(c)) / len2;
            if (0.0..=1.0).contains(&t) && (center + vector * t).distance_squared(c) <= r2 {
                return Some(c);
            }
        }
        for i in 0..4 {
            let c = self.corner(q, i);
            let cd = self.corner(q, (i + 1) % 4) - c;
            let inv = 1.0 / cd.length_squared();
            let cd_dot_c = cd.dot(c);
            for at in [center, end] {
                let t = -(cd_dot_c - cd.dot(at)) * inv;
                if (0.0..=1.0).contains(&t) {
                    let p = c + cd * t;
                    if p.distance_squared(at) <= r2 {
                        return Some(p);
                    }
                }
            }
            // The closest approach of the edge and the sweep's axis (with
            // the same parameter on both, as the original has it).
            let t = -(v_dot_c - center.dot(cd) - vector.dot(c) + cd.dot(c)) / (len2 - 2.0 * vector.dot(cd) + cd.length_squared());
            if (0.0..=1.0).contains(&t) {
                let p = c + cd * t;
                if (center + vector * t).distance_squared(p) <= r2 {
                    return Some(p);
                }
            }
        }
        self.quad_line(q, center, end)
    }

    /// `CLrQuad_Line`: where segment `start → end` meets quad `q`.
    pub fn quad_line(&self, q: &Quad, start: Vec3, end: Vec3) -> Option<Vec3> {
        let (n, d) = self.plane(q.plane);
        let (ds, de) = (n.dot(start) + d, n.dot(end) + d);
        let m = end - start;
        if ds.abs() < EPSILON && de.abs() < EPSILON {
            // Along the plane: the end inside, or crossing an edge.
            if self.point_in_quad(q, end) {
                return Some(end);
            }
            let (u, v) = axes(projection(q.flags));
            let c = |k: usize| self.corner(q, k);
            let (ut, vt, umt, vmt) = (start[u], start[v], m[u], m[v]);
            for k in 0..4 {
                let (a, b) = (c(k), c((k + 1) % 4));
                let (ui, vi, um, vm) = (a[u], a[v], b[u] - a[u], b[v] - a[v]);
                let tt = (vt * um - vi * um - ut * vm + ui * vm) / (umt * vm - vmt * um);
                if (0.0..=1.0).contains(&tt) {
                    let tn = if um.abs() < EPSILON { (vt + tt * vmt - vi) / vm } else { (ut + tt * umt - ui) / um };
                    if (0.0..=1.0).contains(&tn) {
                        // (The original places the hit by the edge's
                        // parameter along the segment.)
                        return Some(start + m * tn);
                    }
                }
            }
            return None;
        }
        let denom = n.dot(m);
        if denom.abs() < EPSILON {
            return None;
        }
        let num = -ds;
        if num.abs() - denom.abs() > EPSILON || (denom < -EPSILON && num > EPSILON) || (denom > EPSILON && num < -EPSILON) {
            return None;
        }
        let p = start + m * (num / denom);
        self.point_in_quad(q, p).then_some(p)
    }

    /// `CLrQuad_PointInQuad` in the quad's projection.
    pub fn point_in_quad(&self, q: &Quad, p: Vec3) -> bool {
        let (u, v) = axes(projection(q.flags));
        let w = 3 - u - v;
        let c: [Vec3; 4] = std::array::from_fn(|k| self.corner(q, k));
        if c.iter().all(|c| p[w] - c[w] < -SLOPPY) || c.iter().all(|c| c[w] - p[w] < -SLOPPY) {
            return false;
        }
        let tr: [f32; 4] = std::array::from_fn(|k| {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            (a[u] - p[u]) * (b[v] - p[v]) - (a[v] - p[v]) * (b[u] - p[u])
        });
        tr.iter().all(|t| -t < EPSILON) || tr.iter().all(|t| *t < EPSILON)
    }
}

/// The quad's projection: 1 XY, 2 XZ, 3 YZ (every shipped quad has one).
fn projection(flags: u32) -> u32 {
    (flags >> 25) & 3
}

fn axes(projection: u32) -> (usize, usize) {
    match projection {
        1 => (0, 1),
        2 => (0, 2),
        _ => (1, 2),
    }
}

/// `MUrUnsignedSmallFloat_ShiftLeft_To_Uns_Round(d, 8)`.
fn round_256(d: f32) -> u32 {
    (d * 256.0).round_ties_even() as u32
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A box room's quads: `walls` as (corners, flags), each with its plane.
    pub(crate) fn room(walls: &[([Vec3; 4], u32)]) -> Quads {
        let mut q = Quads::default();
        for (corners, flags) in walls {
            let base = q.points.len() as u32;
            q.points.extend_from_slice(corners);
            let n = (corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize();
            q.planes.push([n.x, n.y, n.z, -n.dot(corners[0])]);
            let (mut min, mut max) = (corners[0], corners[0]);
            for c in corners {
                min = min.min(*c);
                max = max.max(*c);
            }
            let proj = if n.z.abs() >= n.x.abs() && n.z.abs() >= n.y.abs() {
                1
            } else if n.y.abs() >= n.x.abs() {
                2
            } else {
                3
            };
            let kind = if n.y.abs() > 0.5 { flag::FLOOR_CEILING } else { flag::WALL };
            q.quads.push(Quad {
                points: [base, base + 1, base + 2, base + 3],
                flags: flags | kind | (proj << 25),
                plane: q.planes.len() as u32 - 1,
                shades: [0xFFFF_FFFF; 4],
                texture: 0,
                object_tag: u32::MAX,
                min,
                max,
                env_ref: None,
            });
        }
        q.rebuild_grid();
        q
    }

    /// A 20×20 wall in the plane z = 0, facing +z.
    pub(crate) fn wall() -> [Vec3; 4] {
        [Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(-10.0, 10.0, 0.0)]
    }

    #[test]
    fn a_ray_hits_the_wall_and_skips_flagged_quads() {
        let q = room(&[(wall(), 0)]);
        let h = q.ray(Vec3::new(1.0, 2.0, 5.0), Vec3::new(0.0, 0.0, -10.0), flag::OBJ_COL_SKIP).unwrap();
        assert_eq!(h.quad, 0);
        assert!(h.point.distance(Vec3::new(1.0, 2.0, 0.0)) < 1e-4);
        assert!(q.ray(Vec3::new(11.0, 2.0, 5.0), Vec3::new(0.0, 0.0, -10.0), 0).is_none(), "beside it");
        let q = room(&[(wall(), flag::NO_COLLISION)]);
        assert!(q.ray(Vec3::new(1.0, 2.0, 5.0), Vec3::new(0.0, 0.0, -10.0), flag::OBJ_COL_SKIP).is_none());
    }

    #[test]
    fn a_sphere_finds_nearby_quads_nearest_first() {
        let far = wall().map(|p| p + Vec3::new(0.0, 0.0, -3.0));
        let side = [Vec3::new(10.0, -10.0, 0.0), Vec3::new(10.0, -10.0, 20.0), Vec3::new(10.0, 10.0, 20.0), Vec3::new(10.0, 10.0, 0.0)];
        let q = room(&[(far, 0), (side, 0), (wall(), 0)]);
        let hits = q.sphere(Vec3::new(9.0, 0.0, 0.0), 1.5, Vec3::new(0.0, 0.0, -10.0), 0);
        let order: Vec<u32> = hits.iter().map(|h| h.quad).collect();
        assert_eq!(order, vec![2, 1, 0], "the wall touched at once, the side 1 away, the far one 3 down");
    }
}
