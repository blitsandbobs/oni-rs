//! Characters against the level (`docs/MOVEMENT.md`, *Collision*): the
//! original's rules (`BFW_Physics.c` with the character callbacks of
//! `ONI_Character.c`), on the level's quads.
//!
//! A tick's motion is split: what doesn't go down (root motion, knockback, a
//! rising jump) is swept with the character's stack of spheres against the
//! level and the doors, and the part pushing into the nearest opposing
//! plane is taken off (once per pass: two passes for the player, one for the
//! AI; still pushing into anything after that, it stops). What goes down is
//! applied as it is, and the floor pass then lifts the character onto the
//! highest floor under a small square at its feet — up to half its body —
//! which is how it lands, steps onto ledges and follows the ground. There
//! is no contact solving: a character stops short of a wall by up to a
//! tick's motion.

use glam::Vec3;

use crate::quads::{flag, Quad, Quads};

/// A floor faces up more than this (`ONcMinFloorNormalY`).
pub const MIN_FLOOR_NORMAL_Y: f32 = 0.5;
/// The least downward pull a tick (`ONcInitialFallingVelocity`), and how far
/// below the feet the ground is looked for (`ONcMaxRampVelocity`).
pub const INITIAL_FALLING_VELOCITY: f32 = 0.5;
pub const MAX_RAMP_VELOCITY: f32 = 4.0;
/// The spheres' radius: the player's, an AI's.
pub const PLAYER_LEAF: f32 = 4.0;
pub const AI_LEAF: f32 = 3.0;
/// The floor pass: its sphere's radius, the half-width of the square the
/// floor is looked for under, how far above the feet it starts, and how far
/// up a floor may always be stepped onto.
const GROUND_SPHERE: f32 = 5.0 * 1.42;
const FOOTPRINT: f32 = 1.5;
const SOLID_OFFSET: f32 = 10.0;
const MIN_BUMP: f32 = 6.0;
/// Spawning: a floor is looked for this far above the feet.
const SNAP_HEIGHT: f32 = 12.0;
/// A slow mover's faces aren't discarded by its direction, on the first
/// pass, against moving objects (`PHcBackfacingMinVelocity`).
const BACKFACING_MIN_VELOCITY: f32 = 0.4 / 60.0;
/// `UUcEpsilon`.
const EPSILON: f32 = 1.0 / 1024.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
}

/// A character's spheres: one round the whole body, two halves, four in a
/// stack (`ONrCharacter_UpdateSphereTree`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub root: Sphere,
    pub mids: [Sphere; 2],
    pub leaves: [Sphere; 4],
}

impl Tree {
    /// The stack at `at`'s x, z from `bottom` to `top`, of spheres of radius
    /// `r`.
    pub fn new(at: Vec3, bottom: f32, top: f32, r: f32) -> Tree {
        let height = (top - bottom).clamp(0.0, 72.0);
        let s = |y: f32, radius: f32| Sphere { center: Vec3::new(at.x, y, at.z), radius };
        let step = (height - 2.0 * r) / 3.0;
        let leaves = std::array::from_fn(|k| s(bottom + r + k as f32 * step, r));
        let mids = std::array::from_fn(|m| {
            let (one, two): (Sphere, Sphere) = (leaves[2 * m], leaves[2 * m + 1]);
            let diff = two.center.y - one.center.y;
            s(one.center.y + diff / 2.0, one.radius + diff / 2.0)
        });
        Tree { root: s(bottom + height / 2.0, height / 2.0), mids, leaves }
    }

    /// Whether quad `q` stops the stack swept along `v`
    /// (`PHrCollision_Quad_SphereTreeVector`): the whole body, one half,
    /// and one of that half's spheres all touch it.
    fn touches(&self, quads: &Quads, q: &Quad, v: Vec3) -> bool {
        let hit = |s: &Sphere| quads.sphere_vector(q, s.center, s.radius, v).is_some();
        hit(&self.root) && (0..2).rev().any(|m| hit(&self.mids[m]) && (0..2).rev().any(|l| hit(&self.leaves[2 * m + l])))
    }

}

/// A door leaf as collision sees it: a box (centre, unit axes, half sizes),
/// solid or not.
#[derive(Clone, Debug)]
pub struct DoorBox {
    pub center: Vec3,
    pub axes: [Vec3; 3],
    pub half: Vec3,
    pub solid: bool,
}

impl DoorBox {
    /// The box round `points` in the frame `axes` (about `origin`).
    #[cfg(test)]
    pub fn around(points: &[Vec3], origin: Vec3, axes: [Vec3; 3]) -> DoorBox {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in points {
            let d = *p - origin;
            let l = Vec3::new(d.dot(axes[0]), d.dot(axes[1]), d.dot(axes[2]));
            lo = lo.min(l);
            hi = hi.max(l);
        }
        let mid = (lo + hi) / 2.0;
        DoorBox { center: origin + axes[0] * mid.x + axes[1] * mid.y + axes[2] * mid.z, axes, half: (hi - lo) / 2.0, solid: true }
    }

    /// Its six faces: outward normal and corners.
    fn faces(&self) -> [(Vec3, [Vec3; 4]); 6] {
        std::array::from_fn(|f| {
            let (k, sign) = (f / 2, if f % 2 == 0 { 1.0 } else { -1.0 });
            let (u, w) = ((k + 1) % 3, (k + 2) % 3);
            let n = self.axes[k] * sign;
            let c = self.center + n * self.half[k];
            let (a, b) = (self.axes[u] * self.half[u], self.axes[w] * self.half[w]);
            (n, [c - a - b, c + a - b, c + a + b, c - a + b])
        })
    }
}

/// What a character collides with this tick.
pub struct Env<'a> {
    pub quads: &'a Quads,
    pub doors: &'a [DoorBox],
    /// Where to note what it touches, each time it looks.
    pub touched: Option<&'a std::cell::RefCell<Contacts>>,
    /// The placed objects' collision spheres (centre, radius).
    pub spheres: &'a [(Vec3, f32)],
}

impl<'a> Env<'a> {
    pub fn new(quads: &'a Quads, doors: &'a [DoorBox]) -> Env<'a> {
        Env { quads, doors, touched: None, spheres: &[] }
    }
}

/// `CLrSphere_Sphere`: whether sphere `a` moving along `v` meets `b` — its
/// centre's path through `b`, or the two overlapping at the start or end.
fn sphere_sphere(a: &Sphere, v: Vec3, b: (Vec3, f32)) -> bool {
    let (c, r) = b;
    let end = a.center + v;
    let ab = end - a.center;
    let t = if ab.length_squared() > 0.0 { ((c - a.center).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    let reach = (a.radius + r) * (a.radius + r);
    (a.center + ab * t).distance_squared(c) < r * r || a.center.distance_squared(c) < reach || end.distance_squared(c) < reach
}

/// An object's sphere the stack meets (`PHrCollision_SphereTree_SphereTree`:
/// the whole body, then a half and one of its leaves), and the plane
/// halfway between the body's and the object's surfaces, facing the body.
fn sphere_collider(tree: &Tree, v: Vec3, b: (Vec3, f32)) -> Option<Collider> {
    if !sphere_sphere(&tree.root, v, b) {
        return None;
    }
    let leaf = (0..2).any(|m| sphere_sphere(&tree.mids[m], v, b) && (0..2).any(|l| sphere_sphere(&tree.leaves[2 * m + l], v, b)));
    if !leaf {
        return None;
    }
    let (c, r) = b;
    Some(match (tree.root.center - c).try_normalize() {
        Some(n) => Collider { normal: n, point: ((c + n * r) + (tree.root.center - n * tree.root.radius)) * 0.5 },
        None => Collider { normal: Vec3::Y, point: tree.root.center },
    })
}

/// Something a character's movement met (`PHtCollider`, for the
/// notifications): a wall's plane (its index; the top bit: the reversed
/// side), or a door leaf's box; where, and the plane's normal and d.
#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub wall: Option<u32>,
    pub door: Option<usize>,
    /// A placed object's sphere.
    pub object: Option<usize>,
    pub point: Vec3,
    pub normal: Vec3,
    pub d: f32,
}

/// What a character's movement met this tick: every wall (not floor) and
/// door face, each time it looked (`ONrCharacter_Callback_FindEnvCollisions`
/// / `_FindPhyCollisions`, before what it moves away from is left out), and
/// the counts the original keeps — walls met (each side once), objects met
/// the last time it looked.
#[derive(Clone, Debug, Default)]
pub struct Contacts {
    pub touches: Vec<Touch>,
    pub walls: u32,
    pub objects: u32,
}

/// Where a character stands (its location: the point on the ground under
/// its pelvis) and whether it is on the ground.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    pub feet: Vec3,
    pub grounded: bool,
}

/// A plane in the way (`PHtCollider`): its normal and a point on it (where
/// it was touched).
#[derive(Clone, Copy, Debug)]
pub struct Collider {
    pub normal: Vec3,
    pub point: Vec3,
}

/// Everything the stack swept along `v` meets
/// (`ONrCharacter_Callback_FindEnvCollisions` and `_FindPhyCollisions`):
/// the quads the whole body touches and the stack confirms (two-sided ones
/// twice, both ways), then the doors' faces.
fn gather(env: &Env, tree: &Tree, v: Vec3, initial: bool) -> Vec<Collider> {
    let mut out = Vec::new();
    let mut contacts = env.touched.map(|t| t.borrow_mut());
    for hit in env.quads.sphere(tree.root.center, tree.root.radius, v, flag::CHR_COL_SKIP) {
        let q = &env.quads.quads[hit.quad as usize];
        if !tree.touches(env.quads, q, v) {
            continue;
        }
        let (normal, d) = env.quads.plane(q.plane);
        out.push(Collider { normal, point: hit.point });
        let two_sided = q.flags & flag::AK_2SIDED != 0;
        if two_sided {
            out.push(Collider { normal: -normal, point: hit.point });
        }
        // (A wall, not a floor: counted once, noted both ways.)
        if let Some(c) = contacts.as_mut().filter(|_| normal.y <= MIN_FLOOR_NORMAL_Y) {
            c.walls += 1;
            c.touches.push(Touch { wall: Some(q.plane), door: None, object: None, point: hit.point, normal, d });
            if two_sided {
                c.touches.push(Touch { wall: Some(q.plane ^ 0x8000_0000), door: None, object: None, point: hit.point, normal: -normal, d: -d });
            }
        }
    }
    let objects = out.len();
    for (k, &b) in env.spheres.iter().enumerate() {
        if let Some(c) = sphere_collider(tree, v, b) {
            out.push(c);
            if let Some(t) = contacts.as_mut() {
                t.touches.push(Touch { wall: None, door: None, object: Some(k), point: c.point, normal: c.normal, d: -c.normal.dot(c.point) });
            }
        }
    }
    for (k, door) in env.doors.iter().enumerate().filter(|(_, d)| d.solid) {
        let at = out.len();
        door_colliders(door, tree, v, initial, &mut out);
        if let Some(c) = contacts.as_mut() {
            c.touches.extend(out[at..].iter().map(|o| Touch { wall: None, door: Some(k), object: None, point: o.point, normal: o.normal, d: -o.normal.dot(o.point) }));
        }
    }
    if let Some(c) = contacts.as_mut() {
        c.objects = (out.len() - objects) as u32;
    }
    out
}

/// A door's faces the stack meets (`PHrCollision_Volume_SphereTree`): each
/// sphere where it stands now (not swept), faces it moves away from left
/// out (unless it barely moves, on the first pass); kept only if one of the
/// four spheres is in.
fn door_colliders(door: &DoorBox, tree: &Tree, v: Vec3, initial: bool, out: &mut Vec<Collider>) {
    let speed = v.length();
    let discard = !(initial && speed < BACKFACING_MIN_VELOCITY);
    if discard && speed == 0.0 {
        return;
    }
    let dir = if discard { v / speed } else { v };
    let faces = door.faces();
    let touch = |s: &Sphere, out: &mut Vec<Collider>| -> bool {
        let mut any = false;
        for (n, corners) in &faces {
            let Some(point) = sphere_face(s, *n, corners) else { continue };
            // (Facing the way it moves — at under 90° — it is left out.)
            if discard && dir.dot(*n) > 0.0 {
                continue;
            }
            out.push(Collider { normal: *n, point });
            any = true;
        }
        any
    };
    let start = out.len();
    if !touch(&tree.root, out) {
        return;
    }
    for m in (0..2).rev() {
        let at = out.len();
        if touch(&tree.mids[m], out) {
            for l in (0..2).rev() {
                if touch(&tree.leaves[2 * m + l], out) {
                    return;
                }
            }
        }
        out.truncate(at);
    }
    out.truncate(start);
}

/// `CLrQuad_Sphere`: whether a sphere meets a face (within reach of its
/// plane, and a corner inside it, its centre over the face, or an edge
/// within reach), and the centre's foot on the plane.
fn sphere_face(s: &Sphere, n: Vec3, corners: &[Vec3; 4]) -> Option<Vec3> {
    let dist = n.dot(s.center) - n.dot(corners[0]);
    if dist.abs() > s.radius {
        return None;
    }
    let foot = s.center - n * dist;
    let r2 = s.radius * s.radius;
    if corners.iter().any(|c| c.distance_squared(s.center) < r2) {
        return Some(foot);
    }
    if in_face(n, corners, foot) {
        return Some(foot);
    }
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        let ab = b - a;
        let t = ((s.center - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        if (a + ab * t).distance_squared(s.center) <= r2 {
            return Some(foot);
        }
    }
    None
}

/// A point on a face's plane inside it (the dominant-axis projection, edges
/// counted in).
fn in_face(n: Vec3, c: &[Vec3; 4], p: Vec3) -> bool {
    let a = n.abs();
    let (u, v) = if a.z >= a.x && a.z >= a.y {
        (0, 1)
    } else if a.y >= a.x {
        (0, 2)
    } else {
        (1, 2)
    };
    let tr: [f32; 4] = std::array::from_fn(|k| {
        let (x, y) = (c[k], c[(k + 1) % 4]);
        (x[u] - p[u]) * (y[v] - p[v]) - (x[v] - p[v]) * (y[u] - p[u])
    });
    tr.iter().all(|t| -t < EPSILON) || tr.iter().all(|t| *t < EPSILON)
}

/// Leaves out what the mover is leaving or is already behind
/// (`PHiRemoveBackfacingCollisions`), then nearest first.
fn prepare(mut cols: Vec<Collider>, v: Vec3, root: Vec3) -> Vec<Collider> {
    cols.retain(|c| !(v.dot(c.normal) > 0.0 || (c.point - root).dot(c.normal) > 0.0));
    cols.sort_by(|a, b| a.point.distance_squared(root).total_cmp(&b.point.distance_squared(root)));
    cols
}

/// The part of `v` pushing into a plane (`MUrVector_PushBack`).
fn push_back(v: Vec3, n: Vec3) -> Vec3 {
    let d = v.dot(n);
    if d < 0.0 {
        n * d
    } else {
        Vec3::ZERO
    }
}

/// One pass (`OBrPhysics_Update_Reflect`): the push into the nearest
/// opposing plane taken off `v`; with a slide, `v` no longer goes down.
/// Returns whether it slid.
fn reflect(env: &Env, tree: &Tree, v: &mut Vec3, initial: bool) -> bool {
    for c in prepare(gather(env, tree, *v, initial), *v, tree.root.center) {
        let p = push_back(*v, c.normal);
        *v -= p;
        if p != Vec3::ZERO {
            v.y = v.y.max(0.0);
            return true;
        }
    }
    false
}

/// `OBrPhysics_Update_ReflectHalt`: still pushing into anything, it stops.
fn halt(env: &Env, tree: &Tree, v: &mut Vec3) {
    if prepare(gather(env, tree, *v, false), *v, tree.root.center).iter().any(|c| push_back(*v, c.normal) != Vec3::ZERO) {
        *v = Vec3::ZERO;
    }
}

/// The motion a tick's non-falling part `v` comes to (`OBiPhysics_Update`):
/// a pass (and for the player a second), and a halt if the last one slid.
pub fn resolve(env: &Env, tree: &Tree, mut v: Vec3, player: bool) -> Vec3 {
    let mut slid = reflect(env, tree, &mut v, true);
    if player {
        slid = reflect(env, tree, &mut v, false);
    }
    if slid {
        halt(env, tree, &mut v);
    }
    v
}

/// Moving straight to `dest` against the level (a throw's victim,
/// `ONrCharacter_Teleport_With_Collision`): one pass, a halt if it slid.
pub fn shove(env: &Env, tree: &Tree, from: Vec3, dest: Vec3) -> Vec3 {
    let mut v = dest - from;
    if reflect(env, tree, &mut v, false) {
        halt(env, tree, &mut v);
    }
    from + v
}

/// How far a grounded character is pulled down this tick
/// (`ONrGameState_CalculateGravity`): to the floor if it is within reach
/// below, at least a little; hit or attacking, just the little.
pub fn ground_gravity(quads: &Quads, location: Vec3, plain: bool) -> f32 {
    if !plain {
        return INITIAL_FALLING_VELOCITY;
    }
    match quads.ray(location, Vec3::Y * -MAX_RAMP_VELOCITY, flag::CHR_COL_SKIP) {
        Some(h) => {
            let d = location.y - h.point.y;
            if d < MAX_RAMP_VELOCITY {
                INITIAL_FALLING_VELOCITY.max(d)
            } else {
                INITIAL_FALLING_VELOCITY
            }
        }
        None => INITIAL_FALLING_VELOCITY,
    }
}

/// What the floor pass needs from the character.
#[derive(Clone, Copy, Debug)]
pub struct Footing {
    /// The location at the tick's start.
    pub start: Vec3,
    /// This tick's fall (≤ 0).
    pub gravity: f32,
    /// The pelvis's height above the location.
    pub height: f32,
    /// The lowest point of the body above the location (in the air).
    pub feet_offset: f32,
    /// In the air, or landing.
    pub airborne: bool,
}

/// The floor found: its height and quad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    pub height: f32,
    pub quad: u32,
}

/// The floor pass (`FindGroundCollision`): falls by the tick's gravity, then
/// lifts `location` so the feet (in the air: the lowest point of the body)
/// stand on the highest floor under a 3×3 square round it — if that floor
/// isn't above half the body (or 6 above the location, or 6 above the last
/// floor). `deferred` is the last floor's height, carried between ticks.
pub fn find_ground(quads: &Quads, location: &mut Vec3, f: &Footing, deferred: &mut f32) -> Option<Ground> {
    let l = f.start;
    let (fo, mut feet) = if f.airborne { (f.feet_offset, l.y + f.feet_offset.max(0.0)) } else { (0.0, l.y) };
    let mut max_h = (l.y + fo + (f.height - fo) / 2.0).max(l.y + MIN_BUMP);
    let center = Vec3::new(l.x, l.y + fo + SOLID_OFFSET, l.z);
    let hits = quads.sphere(center, GROUND_SPHERE, Vec3::new(0.0, f.gravity - fo - SOLID_OFFSET, 0.0), flag::CHR_COL_SKIP | flag::WALL);
    location.y += f.gravity;
    feet += f.gravity;
    if max_h < *deferred + MIN_BUMP {
        max_h = *deferred + MIN_BUMP;
    }
    *deferred = feet;
    let mut best: Option<Ground> = None;
    for h in hits {
        let q = &quads.quads[h.quad as usize];
        let (mut n, d) = quads.plane(q.plane);
        // (Glass floors work from both sides.)
        if q.flags & flag::AK_2SIDED != 0 && q.flags & flag::JELLO == 0 && n.dot(center) + d < 0.0 {
            n = -n;
        }
        // A ceiling (walls aren't looked at).
        if n.y <= MIN_FLOOR_NORMAL_Y {
            continue;
        }
        let pts: Vec<Vec3> = (0..q.corners()).map(|k| quads.corner(q, k)).collect();
        let Some(height) = floor_height(&pts, location.x - FOOTPRINT, location.x + FOOTPRINT, location.z - FOOTPRINT, location.z + FOOTPRINT, max_h) else {
            continue;
        };
        if best.is_none_or(|b| height > b.height) {
            best = Some(Ground { height, quad: h.quad });
        }
    }
    let g = best.filter(|g| feet <= g.height)?;
    *deferred = g.height;
    location.y += g.height - feet;
    Some(g)
}

/// Placing a character (`ONrCharacter_SnapToFloor`): the floor pass without
/// a fall (it only lifts), else a floor found from 12 above down to just
/// below the feet.
pub fn snap_to_floor(quads: &Quads, location: &mut Vec3, height: f32, fall_gravity: f32, deferred: &mut f32) -> Option<Ground> {
    let f = Footing { start: *location, gravity: 0.0, height, feet_offset: 0.0, airborne: false };
    if let Some(g) = find_ground(quads, location, &f, deferred) {
        return Some(g);
    }
    let from = *location + Vec3::Y * SNAP_HEIGHT;
    let h = quads.ray(from, Vec3::Y * -(SNAP_HEIGHT + fall_gravity), flag::CHR_COL_SKIP | flag::WALL)?;
    *location = h.point;
    Some(Ground { height: h.point.y, quad: h.quad })
}

/// `GetQuadRangeClipped`: a floor polygon cut to the square, and the height
/// of its highest point there that isn't wholly above `max_y` (at most
/// `max_y`).
fn floor_height(points: &[Vec3], min_x: f32, max_x: f32, min_z: f32, max_z: f32, max_y: f32) -> Option<f32> {
    // Each cut keeps one side of a line: x < max_x, z > min_z, x > min_x,
    // z < max_z, in that order.
    let cut = |pts: &[Vec3], axis: usize, limit: f32, keep_below: bool| -> Vec<Vec3> {
        let mut out = Vec::new();
        let n = pts.len();
        for k in 0..n {
            let (p1, p2) = (pts[k], pts[(k + 1) % n]);
            let (inside1_larger, larger, smaller) = if p1[axis] >= p2[axis] { (true, p1, p2) } else { (false, p2, p1) };
            let clipped = |l: Vec3, s: Vec3| {
                let t = (limit - s[axis]) / (l[axis] - s[axis]);
                s + (l - s) * t
            };
            if keep_below {
                // (`ONrClipLinePos*`.)
                if smaller[axis] >= limit {
                } else if larger[axis] <= limit {
                    out.push(p2);
                } else {
                    out.push(clipped(larger, smaller));
                    if inside1_larger {
                        out.push(p2);
                    }
                }
            } else if larger[axis] <= limit {
                // (`ONrClipLineNeg*`.)
            } else if smaller[axis] >= limit {
                out.push(p2);
            } else {
                out.push(clipped(larger, smaller));
                if !inside1_larger {
                    out.push(p2);
                }
            }
        }
        out
    };
    let mut pts = points.to_vec();
    for (axis, limit, below) in [(0, max_x, true), (2, min_z, false), (0, min_x, false), (2, max_z, true)] {
        pts = cut(&pts, axis, limit, below);
        if pts.is_empty() {
            return None;
        }
    }
    if pts.len() < 3 {
        return None;
    }
    let mut best: Option<f32> = None;
    for k in 0..pts.len() {
        let (a, b) = (pts[k].y, pts[(k + 1) % pts.len()].y);
        if a.min(b) < max_y {
            best = Some(best.map_or(a.max(b), |h: f32| h.max(a.max(b))));
        }
    }
    best.map(|h| h.min(max_y))
}

/// Looks ahead along the floor from `at` in horizontal direction `dir`
/// (unit), up to `max`: how far a sphere of the character's radius, at its
/// knees, could go before a wall or a closed door, and where (if anywhere
/// before that) the floor drops away by more than `drop` — a ledge (a ray
/// down every 2 units). (An AI's own look; the original's AI uses its grids.)
pub fn probe(env: &Env, at: Vec3, radius: f32, dir: Vec3, max: f32, drop: f32) -> crate::physics::Probe {
    const STEP: f32 = 2.0;
    const SAMPLE: f32 = 2.0;
    let dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
    let center = at + Vec3::Y * (radius + STEP);
    let mut free = max;
    for h in env.quads.sphere(center, radius, dir * max, flag::CHR_COL_SKIP) {
        let q = &env.quads.quads[h.quad as usize];
        let (n, d) = env.quads.plane(q.plane);
        let toward = -dir.dot(n);
        if n.y.abs() > MIN_FLOOR_NORMAL_Y || toward <= 0.0 {
            continue;
        }
        free = free.min(((n.dot(center) + d - radius) / toward).max(0.0));
    }
    for door in env.doors.iter().filter(|d| d.solid) {
        if let Some((t, _)) = door.ray(center, dir * max, radius) {
            free = free.min(t * max);
        }
    }
    let mut ledge = None;
    let mut d = SAMPLE;
    while d <= free {
        let p = at + dir * d + Vec3::Y * STEP;
        if env.quads.ray(p, Vec3::Y * -(STEP + drop), flag::CHR_COL_SKIP | flag::WALL).is_none() {
            ledge = Some(d);
            break;
        }
        d += SAMPLE;
    }
    crate::physics::Probe { free, ledge }
}

/// What a ray meets first in the level: how far along it (0–1), the
/// surface's normal, and the quad (none: a door).
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub fraction: f32,
    pub normal: Vec3,
    pub quad: Option<u32>,
}

/// A ray through the level and its closed doors (`AKrCollision_Point`
/// skipping what objects pass through; the doors as boxes).
pub fn level_ray(env: &Env, from: Vec3, ray: Vec3) -> Option<RayHit> {
    let len = ray.length();
    if len <= 0.0 {
        return None;
    }
    let mut best = env.quads.ray(from, ray, flag::OBJ_COL_SKIP).map(|h| {
        let q = &env.quads.quads[h.quad as usize];
        RayHit { fraction: (h.point - from).length() / len, normal: env.quads.plane(q.plane).0, quad: Some(h.quad) }
    });
    for door in env.doors.iter().filter(|d| d.solid) {
        if let Some((t, n)) = door.ray(from, ray, 0.0) {
            if best.is_none_or(|b| t < b.fraction) {
                best = Some(RayHit { fraction: t, normal: n, quad: None });
            }
        }
    }
    best
}

impl DoorBox {
    /// Where a ray (`from` along `ray`) enters the box grown by `grow`: the
    /// fraction along it and the face's normal.
    pub fn ray(&self, from: Vec3, ray: Vec3, grow: f32) -> Option<(f32, Vec3)> {
        let d = from - self.center;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let mut normal = Vec3::ZERO;
        for k in 0..3 {
            let (o, v, h) = (d.dot(self.axes[k]), ray.dot(self.axes[k]), self.half[k] + grow);
            if v.abs() < 1e-9 {
                if o.abs() > h {
                    return None;
                }
                continue;
            }
            let (mut a, mut b) = ((-h - o) / v, (h - o) / v);
            let mut n = -self.axes[k];
            if a > b {
                std::mem::swap(&mut a, &mut b);
                n = self.axes[k];
            }
            if a > t0 {
                t0 = a;
                normal = n;
            }
            t1 = t1.min(b);
            if t0 > t1 {
                return None;
            }
        }
        (normal != Vec3::ZERO).then_some((t0, normal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quads::tests::room;

    /// A floor at y = 0 (40 × 40 round the origin), facing up.
    fn floor(y: f32, half: f32, at: Vec3) -> [Vec3; 4] {
        let c = |x: f32, z: f32| Vec3::new(at.x + x, y, at.z + z);
        [c(-half, -half), c(-half, half), c(half, half), c(half, -half)]
    }

    /// A wall in the plane x = `x`, facing -x, from y 0 to 40.
    fn wall_at(x: f32) -> [Vec3; 4] {
        [Vec3::new(x, 0.0, 40.0), Vec3::new(x, 40.0, 40.0), Vec3::new(x, 40.0, -40.0), Vec3::new(x, 0.0, -40.0)]
    }

    fn standing(quads: &Quads, mut at: Vec3, mut deferred: f32) -> (Vec3, f32, Option<Ground>) {
        let g = ground_gravity(quads, at, true);
        let f = Footing { start: at, gravity: -g, height: 9.0, feet_offset: 0.0, airborne: false };
        let r = find_ground(quads, &mut at, &f, &mut deferred);
        (at, deferred, r)
    }

    #[test]
    fn the_floor_square_reaches_past_an_edge() {
        let q = room(&[(floor(0.0, 20.0, Vec3::ZERO), 0)]);
        assert!(q.quads[0].flags & flag::FLOOR_CEILING != 0);
        // The centre 1.4 past the edge (x 21.4): still on the floor.
        let (at, _, g) = standing(&q, Vec3::new(21.4, 0.0, 0.0), 0.0);
        assert!(g.is_some() && at.y.abs() < 1e-4, "{at} {g:?}");
        // 1.6 past: nothing under it; it falls 0.5.
        let (at, _, g) = standing(&q, Vec3::new(21.6, 0.0, 0.0), 0.0);
        assert!(g.is_none() && (at.y + 0.5).abs() < 1e-4, "{at} {g:?}");
    }

    #[test]
    fn a_ledge_within_half_the_body_is_stepped_onto() {
        // A platform 4 high beside the floor: with its edge in the square
        // the character goes up onto it.
        let q = room(&[(floor(0.0, 20.0, Vec3::ZERO), 0), (floor(4.0, 5.0, Vec3::new(26.0, 0.0, 0.0)), 0)]);
        let (at, _, g) = standing(&q, Vec3::new(20.0, 0.0, 0.0), 0.0);
        assert!((at.y - 4.0).abs() < 1e-4, "{at} {g:?}");
        // One above half the body (pelvis 9: 4.5) and over 6 doesn't.
        let q = room(&[(floor(0.0, 20.0, Vec3::ZERO), 0), (floor(6.5, 5.0, Vec3::new(26.0, 0.0, 0.0)), 0)]);
        let (at, _, _) = standing(&q, Vec3::new(20.0, 0.0, 0.0), 0.0);
        assert!(at.y.abs() < 1e-4, "{at}");
    }

    #[test]
    fn a_walker_stops_short_of_a_wall_and_slides_along_it() {
        let q = room(&[(floor(0.0, 40.0, Vec3::ZERO), 0), (wall_at(10.0), 0)]);
        let env = Env::new(&q, &[]);
        let mut at = Vec3::new(0.0, 0.0, 0.0);
        for _ in 0..60 {
            let tree = Tree::new(at, at.y, at.y + 18.0, PLAYER_LEAF);
            at += resolve(&env, &tree, Vec3::new(1.0, 0.0, 0.0), true);
        }
        let gap = 10.0 - at.x;
        assert!(gap > 4.0 && gap <= 5.0, "standoff {gap}");
        // At 45° to it: along it at 0.707 of the speed, as close as before.
        let dir = Vec3::new(1.0, 0.0, 1.0).normalize();
        for _ in 0..10 {
            let tree = Tree::new(at, at.y, at.y + 18.0, PLAYER_LEAF);
            at += resolve(&env, &tree, dir, true);
        }
        let z0 = at.z;
        for _ in 0..20 {
            let tree = Tree::new(at, at.y, at.y + 18.0, PLAYER_LEAF);
            at += resolve(&env, &tree, dir, true);
        }
        let gap = 10.0 - at.x;
        assert!(gap > 4.0 && gap <= 4.71, "standoff {gap}");
        assert!(((at.z - z0) / 20.0 - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3, "{}", (at.z - z0) / 20.0);
    }

    #[test]
    fn a_lifted_stack_passes_over_a_ledge_a_low_one_cannot() {
        // A box 10 high: standing, the stack (from the location) runs into
        // its side; raised (feet 10.5 up, as in a tucked jump) it passes.
        let side = [Vec3::new(10.0, 0.0, 10.0), Vec3::new(10.0, 10.0, 10.0), Vec3::new(10.0, 10.0, -10.0), Vec3::new(10.0, 0.0, -10.0)];
        let q = room(&[(side, 0)]);
        let env = Env::new(&q, &[]);
        let low = Tree::new(Vec3::new(5.5, 0.0, 0.0), 0.0, 18.0, PLAYER_LEAF);
        assert!(resolve(&env, &low, Vec3::X, true).x < 1e-5, "blocked");
        let high = Tree::new(Vec3::new(5.5, 0.0, 0.0), 10.5, 28.5, PLAYER_LEAF);
        assert!((resolve(&env, &high, Vec3::X, true).x - 1.0).abs() < 1e-5, "over it");
    }

    #[test]
    fn a_wall_is_passed_from_behind() {
        // (One-sided: from its back it is no wall.)
        let back = [Vec3::new(10.0, 0.0, -40.0), Vec3::new(10.0, 40.0, -40.0), Vec3::new(10.0, 40.0, 40.0), Vec3::new(10.0, 0.0, 40.0)];
        let q = room(&[(back, 0)]);
        let env = Env::new(&q, &[]);
        let tree = Tree::new(Vec3::new(6.0, 0.0, 0.0), 0.0, 18.0, PLAYER_LEAF);
        assert_eq!(resolve(&env, &tree, Vec3::X, true), Vec3::X);
    }

    #[test]
    fn an_objects_sphere_stops_a_walker() {
        let q = room(&[(floor(0.0, 200.0, Vec3::ZERO), 0)]);
        let spheres = [(Vec3::new(10.0, 5.0, 0.0), 4.0)];
        let mut env = Env::new(&q, &[]);
        env.spheres = &spheres;
        let mut at = Vec3::ZERO;
        for _ in 0..40 {
            let tree = Tree::new(at, at.y, at.y + 18.0, 3.0);
            at += resolve(&env, &tree, Vec3::new(0.5, 0.0, 0.0), false);
        }
        assert!(at.x < 10.0 - 4.0 && at.x > 0.5, "stopped short of it: {at}");
        let free = Env::new(&q, &[]);
        let mut at2 = Vec3::ZERO;
        for _ in 0..40 {
            let tree = Tree::new(at2, at2.y, at2.y + 18.0, 3.0);
            at2 += resolve(&free, &tree, Vec3::new(0.5, 0.0, 0.0), false);
        }
        assert!(at2.x > 19.0, "without it, on: {at2}");
    }

    #[test]
    fn a_closed_door_stops_a_walker() {
        let q = room(&[(floor(0.0, 40.0, Vec3::ZERO), 0)]);
        let pts = [Vec3::new(10.0, 0.0, -5.0), Vec3::new(11.0, 0.0, -5.0), Vec3::new(10.0, 20.0, 5.0), Vec3::new(11.0, 20.0, 5.0)];
        let mut door = DoorBox::around(&pts, Vec3::new(10.5, 0.0, 0.0), [Vec3::X, Vec3::Y, Vec3::Z]);
        let mut at = Vec3::ZERO;
        for _ in 0..30 {
            let doors = [door.clone()];
            let env = Env::new(&q, &doors);
            let tree = Tree::new(at, 0.0, 18.0, PLAYER_LEAF);
            at += resolve(&env, &tree, Vec3::X, true);
        }
        // (A door's faces are tested where the spheres stand: it stops a
        // little inside reach.)
        let gap = 10.0 - at.x;
        assert!(gap > 3.0 && gap <= 4.0, "{gap}");
        door.solid = false;
        let doors = [door];
        let env = Env::new(&q, &doors);
        let tree = Tree::new(at, 0.0, 18.0, PLAYER_LEAF);
        assert!((resolve(&env, &tree, Vec3::X, true).x - 1.0).abs() < 1e-5, "open: through");
    }

    #[test]
    fn the_floor_cut_takes_the_highest_point_under_the_square() {
        // A ramp rising 1 per unit along x: under the square round x = 5,
        // its highest point is at x = 6.5.
        let ramp = [Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0), Vec3::new(20.0, 20.0, 10.0), Vec3::new(20.0, 20.0, -10.0)];
        let h = floor_height(&ramp, 3.5, 6.5, -1.5, 1.5, 100.0).unwrap();
        assert!((h - 6.5).abs() < 1e-4, "{h}");
        // Capped at the most it may reach.
        assert_eq!(floor_height(&ramp, 3.5, 6.5, -1.5, 1.5, 5.0), Some(5.0));
        assert_eq!(floor_height(&ramp, 30.0, 33.0, -1.5, 1.5, 100.0), None);
    }
}
