//! Level navigation: the room volumes
//! and their portals, finding the room a point is in, and planning a path —
//! a search over rooms ([`plan`]) and, inside each room, over its grid of
//! 4-unit squares ([`grid`]).

pub mod dynamic;
pub mod grid;
pub mod plan;

use std::path::Path;

use glam::{Vec2, Vec3};

pub use grid::Square;

/// Room flags (`oni_schema::ROOM_FLAGS`).
pub const STAIRS: u32 = 1 << 0;
pub const NON_AI: u32 = 1 << 3;

/// A room's walkability grid.
#[derive(Clone, Debug)]
pub struct Grid {
    pub origin: Vec2,
    pub width: i32,
    pub height: i32,
    pub cells: Vec<u8>,
}

impl Grid {
    /// The cell a world point (x, z) falls in, if on the grid.
    pub fn cell_of(&self, x: f32, z: f32) -> Option<(i32, i32)> {
        let i = ((x - self.origin.x) / grid::SQUARE).floor() as i32;
        let j = ((z - self.origin.y) / grid::SQUARE).floor() as i32;
        (i >= 0 && j >= 0 && i < self.width && j < self.height).then_some((i, j))
    }

    /// A cell's centre (x, z).
    pub fn centre(&self, (i, j): (i32, i32)) -> Vec2 {
        self.origin
            + Vec2::new(
                (i as f32 + 0.5) * grid::SQUARE,
                (j as f32 + 0.5) * grid::SQUARE,
            )
    }

    pub fn square(&self, (i, j): (i32, i32)) -> Square {
        if i < 0 || j < 0 || i >= self.width || j >= self.height {
            return Square::Impassable;
        }
        Square::from(self.cells[(i + j * self.width) as usize])
    }
}

/// Whether square `c` of room `r` is held by an obstruction that blocks
/// whoever is probing (`world/obstructions.rs`).
pub type Obstructed<'a> = &'a dyn Fn(usize, (i32, i32)) -> bool;

/// What a local probe found ([`Navigation::probe`]).
#[derive(Clone, Copy, Debug)]
pub struct LocalProbe {
    /// How far it is free.
    pub free: f32,
    /// The square it stopped at (None: it got all the way).
    pub stopped: Option<Square>,
    /// The worst square it crossed (on good ground).
    pub worst: Square,
    /// It started in a bad area and walked out of it (an escape path).
    pub escaped: bool,
}

/// A way out of a room into another.
#[derive(Clone, Debug)]
pub struct Portal {
    pub to: usize,
    /// The portal's base edge (its quad's two lowest corners).
    pub a: Vec3,
    pub b: Vec3,
    pub mid: Vec3,
    /// Horizontal length of the edge.
    pub width: f32,
    /// Ends a body can't pass close to (no neighbouring opening there).
    /// (Stair portals are obstructed at both ends.)
    pub obstructed: [bool; 2],
    /// A door stands in it (doors themselves come with #17).
    #[allow(dead_code)]
    pub door: bool,
    /// Stretches along the edge (from `a`) blocked by the grids.
    pub blocked: Vec<(f32, f32)>,
    /// Blocked all the way across.
    pub impassable: bool,
}

#[derive(Clone, Debug)]
pub struct Room {
    pub flags: u32,
    /// Normalised planes; inside when `n·p + d ≤ 0` for all.
    pub planes: Vec<[f32; 4]>,
    pub min: Vec3,
    pub max: Vec3,
    pub stair: Option<([f32; 4], f32)>,
    pub grid: Option<Grid>,
    pub portals: Vec<Portal>,
}

impl Room {
    pub fn is_stairs(&self) -> bool {
        self.flags & STAIRS != 0
    }

    /// How far outside the hull `p` is (≤ 0: inside), and the plane that
    /// says so.
    fn outside(&self, p: Vec3) -> (f32, [f32; 4]) {
        self.planes
            .iter()
            .map(|pl| (pl[0] * p.x + pl[1] * p.y + pl[2] * p.z + pl[3], *pl))
            .fold((f32::MIN, [0.0; 4]), |a, b| if b.0 > a.0 { b } else { a })
    }

    /// The square type at world point (x, z) (a room without a grid is all
    /// clear; off its grid is impassable).
    pub fn square_at(&self, x: f32, z: f32) -> Square {
        match &self.grid {
            None => Square::Clear,
            Some(g) => g.cell_of(x, z).map_or(Square::Impassable, |c| g.square(c)),
        }
    }
}

/// A link as read: the room it leads to, its portal flags, its quad.
type RawLink = (usize, Vec<String>, [Vec3; 4]);

/// A level's navigation data.
#[derive(Clone, Debug, Default)]
pub struct Navigation {
    pub rooms: Vec<Room>,
}

/// A point this far outside a room still counts as in it (not through a
/// floor or ceiling).
const OUTSIDE_ACCEPT: f32 = 5.0;

impl Navigation {
    /// Loads `navigation.json` (+ its grids) from a level folder.
    pub fn load(level_dir: &Path) -> std::io::Result<Navigation> {
        let text = std::fs::read_to_string(level_dir.join("navigation.json"))?;
        let doc: oni_schema::files::NavigationFile = serde_json::from_str(&text)
            .map_err(|e| std::io::Error::other(format!("navigation.json: {e}")))?;
        let bin = std::fs::read(level_dir.join(&doc.grids))?;
        let mut rooms: Vec<Room> = Vec::new();
        let mut raw_links: Vec<Vec<RawLink>> = Vec::new();
        for r in &doc.rooms {
            let flags =
                oni_schema::bits(&oni_schema::ROOM_FLAGS, r.flags.iter().map(String::as_str));
            let planes = r
                .planes
                .iter()
                .map(|p| {
                    let n = Vec3::new(p[0], p[1], p[2]).length().max(1e-6);
                    [p[0] / n, p[1] / n, p[2] / n, p[3] / n]
                })
                .collect();
            let grid = r.grid.as_ref().map(|g| {
                let (w, h) = (g.width as i32, g.height as i32);
                Grid {
                    origin: Vec2::from(g.origin),
                    width: w,
                    height: h,
                    cells: bin
                        .get(g.offset..g.offset + (w * h) as usize)
                        .map(<[u8]>::to_vec)
                        .unwrap_or_default(),
                }
            });
            rooms.push(Room {
                flags,
                planes,
                min: Vec3::from(r.min),
                max: Vec3::from(r.max),
                stair: r.stair.as_ref().map(|s| (s.plane, s.height)),
                grid,
                portals: Vec::new(),
            });
            raw_links.push(
                r.links
                    .iter()
                    .map(|l| (l.to as usize, l.portal.clone(), l.quad.map(Vec3::from)))
                    .collect(),
            );
        }
        let mut nav = Navigation { rooms };
        nav.build_portals(&raw_links);
        Ok(nav)
    }

    /// The portals.
    fn build_portals(&mut self, links: &[Vec<RawLink>]) {
        const TOUCH: f32 = 0.1;
        for (r, room_links) in links.iter().enumerate() {
            let mut portals = Vec::new();
            for (k, (to, flags, quad)) in room_links.iter().enumerate() {
                let is = |f: &str| flags.iter().any(|x| x == f);
                if !(is("ghost") || is("stairsUp") || is("stairsDown")) || *to >= self.rooms.len() {
                    continue;
                }
                // The base edge: the two lowest corners.
                let mut corners = quad.to_vec();
                corners.sort_by(|p, q| p.y.total_cmp(&q.y));
                let (a, b) = (corners[0], corners[1]);
                let width = Vec2::new(b.x - a.x, b.z - a.z).length();
                // An end is open only where another opening of this room meets it.
                let touched = |end: Vec3| {
                    room_links
                        .iter()
                        .enumerate()
                        .any(|(m, (_, _, q))| m != k && q.iter().any(|c| c.distance(end) <= TOUCH))
                };
                let stairs = self.rooms[r].is_stairs() || self.rooms[*to].is_stairs();
                let obstructed = if stairs {
                    [true, true]
                } else {
                    [!touched(a), !touched(b)]
                };
                portals.push(Portal {
                    to: *to,
                    a,
                    b,
                    mid: (a + b) * 0.5,
                    width,
                    obstructed,
                    door: is("door"),
                    blocked: Vec::new(),
                    impassable: false,
                });
            }
            self.rooms[r].portals = portals;
        }
        // Blocked stretches: walk each edge through both rooms' grids.
        for r in 0..self.rooms.len() {
            for k in 0..self.rooms[r].portals.len() {
                let p = &self.rooms[r].portals[k];
                let (blocked, impassable) = self.blockages(r, p);
                let p = &mut self.rooms[r].portals[k];
                p.blocked = blocked;
                p.impassable = impassable;
            }
        }
    }

    fn blockages(&self, r: usize, p: &Portal) -> (Vec<(f32, f32)>, bool) {
        const HALF: f32 = 4.0;
        const STEP: f32 = 1.0;
        let dir = Vec2::new(p.b.x - p.a.x, p.b.z - p.a.z).normalize_or_zero();
        let mut spans: Vec<(f32, f32)> = Vec::new();
        let mut t = -2.0 * grid::SQUARE;
        while t <= p.width + 2.0 * grid::SQUARE {
            let at = Vec2::new(p.a.x, p.a.z) + dir * t;
            let bad = [r, p.to].iter().any(|&room| {
                matches!(
                    self.rooms[room].square_at(at.x, at.y),
                    Square::Border4 | Square::Danger | Square::Impassable
                ) && self.rooms[room]
                    .grid
                    .as_ref()
                    .is_some_and(|g| g.cell_of(at.x, at.y).is_some())
            });
            if bad {
                let s = (t - HALF, t + HALF);
                match spans.last_mut() {
                    Some(last) if s.0 <= last.1 => last.1 = last.1.max(s.1),
                    _ => spans.push(s),
                }
            }
            t += STEP;
        }
        spans.truncate(16);
        let impassable = spans.len() == 1 && spans[0].0 < HALF && spans[0].1 > p.width - HALF;
        (spans, impassable)
    }

    /// Looks along the floor from `from` in direction `dir` (unit, x / z) up to
    /// `dist`, square by square across rooms (the original's local probe):
    /// it may walk *out* of a bad area for a few squares at its start but
    /// stops on entering danger or a wall. Returns as a [`crate::chrphys::Probe`]: how far it
    /// is free, and whether (and where) the stop is danger rather than a wall.
    pub fn look(
        &self,
        from: Vec3,
        dir: Vec2,
        dist: f32,
        obstructed: Option<Obstructed>,
    ) -> crate::chrphys::Probe {
        let l = self.probe(from, dir, dist, Square::Danger, obstructed);
        match l.stopped {
            Some(sq) => crate::chrphys::Probe {
                free: l.free,
                ledge: (sq == Square::Danger).then_some(l.free),
            },
            None => crate::chrphys::Probe {
                free: dist,
                ledge: None,
            },
        }
    }

    /// The local probe (`AI2rDoLocalMovement`;,
    /// *Local probe*): along the floor from `from` in direction `dir` up to
    /// `dist`, square by square across rooms, stopping at the first square
    /// at or above `stop` once on good ground (it may leave a bad area for 3
    /// squares at its start). How far it is free, the square it stopped at
    /// (None: it got there), and the worst square crossed. Squares
    /// `obstructed` says are held count as walls; leaving one straight into
    /// a real wall stops it (`PHrLocalPathWeight`).
    pub fn probe(
        &self,
        from: Vec3,
        dir: Vec2,
        dist: f32,
        stop: Square,
        obstructed: Option<Obstructed>,
    ) -> LocalProbe {
        self.probe_escaping(from, dir, dist, stop, obstructed, 3)
    }

    /// [`Self::probe`], leaving a bad area for up to `escape` squares.
    pub fn probe_escaping(
        &self,
        from: Vec3,
        dir: Vec2,
        dist: f32,
        stop: Square,
        obstructed: Option<Obstructed>,
        escape: u32,
    ) -> LocalProbe {
        const STEP: f32 = grid::SQUARE * 0.5;
        let escape_dist = escape as f32 * grid::SQUARE;
        let mut room = self.room_at(from);
        let mut worst = Square::Clear;
        let mut escaped = false;
        let mut was_good = false;
        let mut was_held = false;
        let mut last_good = 0.0;
        let mut t = 0.0;
        while t <= dist {
            let p = Vec3::new(from.x + dir.x * t, from.y, from.z + dir.y * t);
            // Stay in the room while its grid covers the point; else find the next.
            let in_room = room.is_some_and(|r| {
                self.rooms[r]
                    .grid
                    .as_ref()
                    .is_none_or(|g| g.cell_of(p.x, p.z).is_some())
            });
            if !in_room {
                room = self.room_at(p);
            }
            let held = room.is_some_and(|r| {
                let cell = self.rooms[r]
                    .grid
                    .as_ref()
                    .and_then(|g| g.cell_of(p.x, p.z));
                cell.is_some_and(|c| obstructed.is_some_and(|o| o(r, c)))
            });
            let sq = if held {
                Square::Impassable
            } else {
                room.map_or(Square::Impassable, |r| self.rooms[r].square_at(p.x, p.z))
            };
            let into_wall = was_held && !held && sq == Square::Impassable;
            was_held = held;
            if sq >= stop {
                if was_good || t > escape_dist || into_wall {
                    return LocalProbe {
                        free: last_good,
                        stopped: Some(sq),
                        worst,
                        escaped,
                    };
                }
                escaped = true;
            } else {
                was_good = true;
                last_good = t;
                worst = worst.max(sq);
            }
            t += STEP;
        }
        LocalProbe {
            free: dist,
            stopped: None,
            worst,
            escaped,
        }
    }

    /// The room a point is in: inside
    /// beats a little outside (≤ 5, not through a floor or ceiling), a stair
    /// room beats another, then the nearer, then the smaller room. Rooms
    /// whose floor is more than 5 above the point don't count.
    pub fn room_at(&self, p: Vec3) -> Option<usize> {
        // (A body pressed a little into the floor is still in its room.)
        self.room_at_exactly(p)
            .or_else(|| self.room_at_exactly(p + Vec3::Y * 4.0))
    }

    fn room_at_exactly(&self, p: Vec3) -> Option<usize> {
        let mut best: Option<(bool, bool, f32, f32, usize)> = None;
        for (i, r) in self.rooms.iter().enumerate() {
            if r.flags & NON_AI != 0 || r.min.y > p.y + OUTSIDE_ACCEPT {
                continue;
            }
            if p.x < r.min.x - OUTSIDE_ACCEPT
                || p.x > r.max.x + OUTSIDE_ACCEPT
                || p.z < r.min.z - OUTSIDE_ACCEPT
                || p.z > r.max.z + OUTSIDE_ACCEPT
                || p.y > r.max.y + OUTSIDE_ACCEPT
            {
                continue;
            }
            let (out, plane) = r.outside(p);
            let inside = out <= 0.0;
            if !inside && (out > OUTSIDE_ACCEPT || plane[1].abs() > 0.5) {
                continue;
            }
            if let Some((sp, h)) = r.stair {
                let s = sp[0] * p.x + sp[1] * p.y + sp[2] * p.z + sp[3];
                if s < 0.0 || s > h {
                    continue;
                }
            }
            let dist = out.max(0.0) + (p.y - r.min.y).abs();
            let size = (r.max - r.min).x * (r.max - r.min).y * (r.max - r.min).z;
            let key = (inside, r.is_stairs(), dist, size, i);
            let better = match best {
                None => true,
                Some(b) => {
                    (key.0, key.1) > (b.0, b.1)
                        || ((key.0, key.1) == (b.0, b.1) && (key.2, key.3) < (b.2, b.3))
                }
            };
            if better {
                best = Some(key);
            }
        }
        best.map(|b| b.4)
    }
}
