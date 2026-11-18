//! A room's grid (behaviour per `docs/PATHFINDING.md`, *The room grid*):
//! square costs, A\* across it, turning the result into waypoints, and the
//! straight "local line" probe.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use glam::Vec2;

use super::Grid;

/// The side of a grid square (units).
pub const SQUARE: f32 = 4.0;
/// A\* gives up after expanding this many squares.
const MAX_NODES: usize = 10240;
/// A path may start inside a wall and step out by about one square.
const ESCAPE_COST: f32 = 3.0;
/// An unreachable end moves to the nearest passable square this many rings
/// out.
const END_SEARCH: i32 = 3;
const DIAGONAL: f32 = 1.4;

/// Square types, in data order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Square {
    Clear,
    NearWall,
    Border1,
    Border2,
    SemiPassable,
    Border3,
    Border4,
    Stairs,
    Danger,
    Impassable,
}

impl From<u8> for Square {
    fn from(v: u8) -> Square {
        use Square::*;
        [Clear, NearWall, Border1, Border2, SemiPassable, Border3, Border4, Stairs, Danger, Impassable]
            .get(usize::from(v))
            .copied()
            .unwrap_or(Impassable)
    }
}

impl Square {
    /// The cost of crossing this square.
    pub fn cost(self) -> f32 {
        [1.0, 2.0, 2.5, 3.0, 3.5, 5.0, 10.0, 20.0, 400.0, 1000.0][self as usize]
    }

    /// How close a waypoint on this square must be reached (clear 12, tight
    /// 8, else 3).
    pub fn arrive(self) -> f32 {
        match self {
            Square::Clear | Square::NearWall | Square::Border1 => 12.0,
            Square::SemiPassable => 8.0,
            _ => 3.0,
        }
    }
}

/// What a path search may cross.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules<'a> {
    /// The next room is a stair room: stair squares are allowed.
    pub stairs: bool,
    /// Squares held by other bodies (not crossable).
    pub obstructed: Option<&'a HashSet<(i32, i32)>>,
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    cell: (i32, i32),
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f)
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

fn passable(g: &Grid, c: (i32, i32), rules: &Rules) -> bool {
    g.square(c) != Square::Impassable && !rules.obstructed.is_some_and(|o| o.contains(&c))
}

/// A\* across the grid from world point `from` to `to` (x, z): the squares of
/// the path, or None. Danger is passable, only costly; walls and other
/// bodies are not (a path starting inside one may step out of it).
pub fn search(g: &Grid, from: Vec2, to: Vec2, rules: Rules) -> Option<Vec<(i32, i32)>> {
    let clamp = |p: Vec2| {
        (
            (((p.x - g.origin.x) / SQUARE).floor() as i32).clamp(0, g.width - 1),
            (((p.y - g.origin.y) / SQUARE).floor() as i32).clamp(0, g.height - 1),
        )
    };
    let start = clamp(from);
    let mut end = clamp(to);
    if !passable(g, end, &rules) {
        end = nearest_passable(g, end, &rules)?;
    }
    let h = |c: (i32, i32)| ((c.0 - end.0).abs() + (c.1 - end.1).abs()) as f32;
    let mut open = BinaryHeap::new();
    let mut cost: HashMap<(i32, i32), f32> = HashMap::new();
    let mut came: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    cost.insert(start, 0.0);
    open.push(Open { f: h(start), cell: start });
    let start_blocked = !passable(g, start, &rules);
    let mut expanded = 0;
    while let Some(Open { cell, .. }) = open.pop() {
        if cell == end {
            let mut path = vec![cell];
            let mut c = cell;
            while let Some(&p) = came.get(&c) {
                path.push(p);
                c = p;
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded > MAX_NODES {
            return None;
        }
        let here = cost[&cell];
        let cell_ok = passable(g, cell, &rules);
        let open_to = |c: (i32, i32)| -> bool {
            if c.0 < 0 || c.1 < 0 || c.0 >= g.width || c.1 >= g.height {
                return false;
            }
            if g.square(c) == Square::Stairs && g.square(cell) != Square::Stairs && !rules.stairs {
                return false;
            }
            if passable(g, c, &rules) {
                return true;
            }
            // Escaping: from a blocked start, about one step through walls
            // (never from another body into a wall).
            start_blocked && !cell_ok && here < ESCAPE_COST && g.square(c) != Square::Impassable
        };
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let next = (cell.0 + dx, cell.1 + dy);
            if !open_to(next) {
                continue;
            }
            let diagonal = dx != 0 && dy != 0;
            if diagonal && !open_to((cell.0 + dx, cell.1)) && !open_to((cell.0, cell.1 + dy)) {
                continue; // no cutting through a diagonal wall
            }
            let step = g.square(next).cost() * if diagonal { DIAGONAL } else { 1.0 };
            let c = here + step;
            if cost.get(&next).is_none_or(|&old| c < old) {
                cost.insert(next, c);
                came.insert(next, cell);
                open.push(Open { f: c + h(next), cell: next });
            }
        }
    }
    None
}

fn nearest_passable(g: &Grid, c: (i32, i32), rules: &Rules) -> Option<(i32, i32)> {
    for ring in 1..=END_SEARCH {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs() != ring && dy.abs() != ring {
                    continue;
                }
                let n = (c.0 + dx, c.1 + dy);
                if n.0 >= 0 && n.1 >= 0 && n.0 < g.width && n.1 < g.height && passable(g, n, rules) {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// A waypoint: where to head and how close counts as there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waypoint {
    pub at: Vec2,
    pub arrive: f32,
}

/// Turns a square path into waypoints: a point wherever it turns (in a stair
/// room, none), then dropping each point whose neighbours see each other
/// along a clear line; the last point is the exact end.
pub fn waypoints(g: &Grid, path: &[(i32, i32)], end: Vec2, stairs_room: bool, careful: bool) -> Vec<Waypoint> {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    if !stairs_room {
        for w in path.windows(3) {
            let d1 = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let d2 = (w[2].0 - w[1].0, w[2].1 - w[1].1);
            if d1 != d2 {
                cells.push(w[1]);
            }
        }
    }
    // Smooth: from the last kept point, skip any point whose successor is
    // in clear view.
    if !careful && !cells.is_empty() {
        let mut kept = Vec::new();
        let mut from = path[0];
        for k in 0..cells.len() {
            let after = cells.get(k + 1).copied().unwrap_or(*path.last().unwrap_or(&cells[k]));
            let line = line(g, from, after, Square::Border2, 3);
            if !(line.reached && !line.escaping && line.worst < Square::Border2) {
                kept.push(cells[k]);
                from = cells[k];
            }
        }
        cells = kept;
    }
    let mut out: Vec<Waypoint> = cells
        .into_iter()
        .map(|c| Waypoint { at: g.centre(c), arrive: g.square(c).arrive() })
        .collect();
    let last = path.last().map_or(Square::Clear, |&c| g.square(c));
    out.push(Waypoint { at: end, arrive: last.arrive() });
    out
}

/// What a straight line across the grid found.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    /// It got all the way.
    pub reached: bool,
    /// It started in a bad area and walked out of it.
    pub escaping: bool,
    /// The worst square it crossed (including the one it stopped at).
    pub worst: Square,
    /// The square it stopped at, and the last good square before it.
    pub stop: (i32, i32),
    pub last_good: (i32, i32),
}

/// Walks a 4-connected line of squares from `a` to `b`. It may *leave* a bad
/// area (squares at or above `stop`) for up to `escape` squares at its
/// start, but never *enter* one.
pub fn line(g: &Grid, a: (i32, i32), b: (i32, i32), stop: Square, escape: u32) -> Line {
    let mut out = Line { reached: false, escaping: false, worst: Square::Clear, stop: a, last_good: a };
    let mut budget = escape;
    let mut was_good = false;
    for c in squares_between(a, b) {
        let sq = g.square(c);
        out.worst = out.worst.max(sq);
        if sq >= stop {
            if was_good || budget == 0 || c == b {
                out.stop = c;
                return out;
            }
            out.escaping = true;
            budget -= 1;
        } else {
            was_good = true;
            out.last_good = c;
        }
    }
    out.reached = true;
    out.stop = b;
    out
}

/// The squares a straight line from `a` to `b` crosses, 4-connected (a
/// diagonal step goes through the square beside it first).
fn squares_between(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let (mut x, mut y, mut err) = (a.0, a.1, dx + dy);
    let mut out = vec![a];
    while (x, y) != b {
        let e2 = 2 * err;
        let (mx, my) = (e2 >= dy, e2 <= dx);
        if mx {
            err += dy;
            x += sx;
        }
        if mx && my {
            out.push((x, y)); // the square beside the diagonal step
        }
        if my {
            err += dx;
            y += sy;
        }
        out.push((x, y));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10×10 clear grid with a wall across x = 5 except at y = 9.
    fn walled() -> Grid {
        let mut cells = vec![0u8; 100];
        for y in 0..9 {
            cells[5 + y * 10] = 9;
        }
        Grid { origin: Vec2::ZERO, width: 10, height: 10, cells }
    }

    #[test]
    fn a_path_goes_round_a_wall() {
        let g = walled();
        let path = search(&g, Vec2::new(2.0, 2.0), Vec2::new(38.0, 2.0), Rules::default()).expect("a path");
        assert!(path.contains(&(5, 9)), "through the gap");
        let w = waypoints(&g, &path, Vec2::new(38.0, 2.0), false, false);
        assert!(w.len() >= 2 && w.last().unwrap().at == Vec2::new(38.0, 2.0));
    }

    #[test]
    fn a_line_stops_at_a_wall_and_can_leave_one() {
        let g = walled();
        let l = line(&g, (0, 2), (9, 2), Square::Danger, 3);
        assert!(!l.reached && l.stop == (5, 2) && l.last_good == (4, 2));
        let out = line(&g, (5, 2), (9, 2), Square::Danger, 3);
        assert!(out.reached && out.escaping);
    }
}
