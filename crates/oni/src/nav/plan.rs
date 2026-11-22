//! Planning a path: the rooms to pass
//! through (A\* over portals), where to cross each portal (looking ahead and
//! projecting back — cheap string pulling), and the grid path inside each
//! room, joined into one list of waypoints.

use std::collections::{BinaryHeap, HashMap, HashSet};

use glam::{Vec2, Vec3};

use super::grid::{self, Rules, Square, Waypoint};
use super::{Navigation, Portal};

/// Keep this far from a portal's obstructed ends.
const COMFORT: f32 = 10.0;
/// How many portals ahead the crossing point looks.
const LOOKAHEAD: usize = 4;
/// At most this many portals in a path.
const MAX_PORTALS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The start isn't in any room.
    StartOutside,
    /// The destination isn't in any room.
    DestinationOutside,
    /// No chain of portals leads there.
    Unreachable,
    /// A room's grid has no way through.
    GridBlocked,
}

/// A planned path: waypoints (world x / z, plus the height of the room they
/// are in) and how close each must be reached.
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub struct Path {
    pub points: Vec<(Vec3, f32)>,
    /// The rooms it passes through, in order.
    pub rooms: Vec<usize>,
}

/// A search state: the room reached, the portal taken into it, and where.
type SearchState = (usize, Option<(usize, usize)>, Vec3);

#[derive(PartialEq)]
struct Node {
    f: f32,
    id: usize,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.f.total_cmp(&self.f)
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// How far a sound goes from `from` (in room `start`) to `to` (in room
/// `goal`) through the rooms (`PHrGeneratePath` with no character): the
/// shortest way portal to portal, where `gate(room, portal)` says whether a
/// portal passes it and how much a door there stretches everything beyond
/// (a muffled door: each later step counts that many times over). None:
/// no way.
pub fn sound_route(
    nav: &Navigation,
    start: usize,
    from: Vec3,
    goal: usize,
    to: Vec3,
    gate: &dyn Fn(usize, usize) -> Option<f32>,
) -> Option<f32> {
    if start == goal {
        return Some(from.distance(to));
    }
    // (State: arrived through a portal — its place and the stretch so far.)
    type Heard = (usize, Option<(usize, usize)>, Vec3, f32);
    let mut states: Vec<Heard> = vec![(start, None, from, 1.0)];
    let mut index: HashMap<(usize, usize), usize> = HashMap::new();
    let mut cost = vec![0.0f32];
    let mut open = BinaryHeap::new();
    open.push(Node {
        f: from.distance(to),
        id: 0,
    });
    let mut best: Option<f32> = None;
    while let Some(Node { f, id }) = open.pop() {
        if best.is_some_and(|b| f >= b) {
            break;
        }
        let (room, via, at, stretch) = states[id];
        if room == goal {
            let total = cost[id] + stretch * at.distance(to);
            best = Some(best.map_or(total, |b: f32| b.min(total)));
            continue;
        }
        let back = via.map(|(r, _)| r);
        for (k, p) in nav.rooms[room].portals.iter().enumerate() {
            if Some(p.to) == back {
                continue;
            }
            let Some(more) = gate(room, k) else { continue };
            let g = cost[id] + stretch * at.distance(p.mid);
            let sid = *index.entry((room, k)).or_insert_with(|| {
                states.push((p.to, Some((room, k)), p.mid, stretch * more));
                cost.push(f32::MAX);
                states.len() - 1
            });
            if g < cost[sid] {
                cost[sid] = g;
                states[sid].3 = stretch * more;
                open.push(Node {
                    f: g + stretch * more * p.mid.distance(to),
                    id: sid,
                });
            }
        }
    }
    best
}

/// The portals to take from room `start` (at `from`) to room `goal` (at
/// `to`): `(room, portal index)` pairs. A\* from portal midpoint to portal
/// midpoint, never straight back, avoiding portals blocked all the way.
/// (`shut`: portals a door closes to this character — `(room, portal)`.)
pub fn route(
    nav: &Navigation,
    start: usize,
    from: Vec3,
    goal: usize,
    to: Vec3,
    shut: &HashSet<(usize, usize)>,
) -> Option<Vec<(usize, usize)>> {
    if start == goal {
        return Some(Vec::new());
    }
    // Search states: arriving through a portal. State 0 is the start.
    let mut states: Vec<SearchState> = vec![(start, None, from)];
    let mut index: HashMap<(usize, usize), usize> = HashMap::new();
    let mut cost = vec![0.0f32];
    let mut came: Vec<Option<usize>> = vec![None];
    let mut open = BinaryHeap::new();
    open.push(Node {
        f: from.distance(to),
        id: 0,
    });
    while let Some(Node { id, .. }) = open.pop() {
        let (room, via, at) = states[id];
        if room == goal {
            let mut out = Vec::new();
            let mut s = Some(id);
            while let Some(i) = s {
                if let Some(v) = states[i].1 {
                    out.push(v);
                }
                s = came[i];
            }
            out.reverse();
            return (out.len() <= MAX_PORTALS).then_some(out);
        }
        let back = via.map(|(r, _)| r);
        for (k, p) in nav.rooms[room].portals.iter().enumerate() {
            if p.impassable || Some(p.to) == back || shut.contains(&(room, k)) {
                continue;
            }
            let g = cost[id] + at.distance(p.mid);
            let sid = *index.entry((room, k)).or_insert_with(|| {
                states.push((p.to, Some((room, k)), p.mid));
                cost.push(f32::MAX);
                came.push(None);
                states.len() - 1
            });
            if g < cost[sid] {
                cost[sid] = g;
                came[sid] = Some(id);
                open.push(Node {
                    f: g + p.mid.distance(to),
                    id: sid,
                });
            }
        }
    }
    None
}

/// The point along a portal's edge (distance from `a`) a body should cross
/// at near `want`: clear of obstructed ends and blocked stretches.
fn clamp_on(p: &Portal, want: f32) -> f32 {
    let lo = if p.obstructed[0] { COMFORT } else { 0.0 };
    let hi = p.width - if p.obstructed[1] { COMFORT } else { 0.0 };
    if lo > hi {
        return p.width * 0.5;
    }
    let mut t = want.clamp(lo, hi);
    for &(b0, b1) in &p.blocked {
        if t > b0 && t < b1 {
            let (down, up) = (b0, b1);
            t = if (down >= lo) && (t - down <= up - t || up > hi) {
                down
            } else if up <= hi {
                up
            } else {
                p.width * 0.5
            };
        }
    }
    t
}

fn along(p: &Portal, t: f32) -> Vec3 {
    let d = Vec2::new(p.b.x - p.a.x, p.b.z - p.a.z).normalize_or_zero();
    Vec3::new(
        p.a.x + d.x * t,
        p.a.y + (p.b.y - p.a.y) * (t / p.width.max(1e-3)),
        p.a.z + d.y * t,
    )
}

/// Where to cross the `i`th portal of `portals`, coming from `at`: look up
/// to four portals ahead (or at the destination), then project that back
/// portal by portal along the line from `at`.
fn crossing(nav: &Navigation, portals: &[(usize, usize)], i: usize, at: Vec3, to: Vec3) -> Vec3 {
    let portal = |m: usize| &nav.rooms[portals[m].0].portals[portals[m].1];
    let ahead = i + LOOKAHEAD - 1;
    let mut target = if ahead >= portals.len() {
        to
    } else {
        // A comfortable point on that portal, nearest to us.
        let p = portal(ahead);
        let t = if p.width <= 2.0 * COMFORT {
            p.width * 0.5
        } else {
            let d = Vec2::new(p.b.x - p.a.x, p.b.z - p.a.z).normalize_or_zero();
            Vec2::new(at.x - p.a.x, at.z - p.a.z)
                .dot(d)
                .clamp(COMFORT, p.width - COMFORT)
        };
        along(p, t)
    };
    let last = ahead.min(portals.len() - 1);
    for m in (i..=last).rev() {
        let p = portal(m);
        // Where the line at → target crosses the portal's line.
        let (e, d) = (
            Vec2::new(p.b.x - p.a.x, p.b.z - p.a.z),
            Vec2::new(target.x - at.x, target.z - at.z),
        );
        let denom = e.perp_dot(d);
        let t = if denom.abs() < 1e-4 || p.width < 1e-3 {
            p.width * 0.5
        } else {
            let w = Vec2::new(at.x - p.a.x, at.z - p.a.z);
            w.perp_dot(d) / denom * p.width
        };
        target = along(p, clamp_on(p, t));
    }
    target
}

/// A path through the rooms (`AI2iPath_FindGlobalPath`): the portals to
/// take from the start room to the goal room, and the destination. Each
/// room's grid is planned only on entering it ([`leg`]).
#[derive(Clone, Debug)]
pub struct Global {
    pub start: usize,
    pub goal: usize,
    pub portals: Vec<(usize, usize)>,
    pub to: Vec3,
}

impl Global {
    /// How many rooms it passes through.
    pub fn legs(&self) -> usize {
        self.portals.len() + 1
    }

    /// The `i`th room.
    pub fn room(&self, nav: &Navigation, i: usize) -> usize {
        match i.checked_sub(1) {
            None => self.start,
            Some(k) => {
                let (r, p) = self.portals[k];
                nav.rooms[r].portals[p].to
            }
        }
    }
}

/// The rooms to pass from `from` to `to` (`AI2iPath_MakePath`): none in
/// either, or no way through, is an error. (`shut`: portals a door closes
/// to this character.)
pub fn global(
    nav: &Navigation,
    from: Vec3,
    to: Vec3,
    shut: &HashSet<(usize, usize)>,
) -> Result<Global, Error> {
    let start = nav.room_at(from).ok_or(Error::StartOutside)?;
    let goal = nav
        .room_at(to + Vec3::Y * 0.5)
        .ok_or(Error::DestinationOutside)?;
    let portals = route(nav, start, from, goal, to, shut).ok_or(Error::Unreachable)?;
    Ok(Global {
        start,
        goal,
        portals,
        to,
    })
}

/// One room's stretch of a path: its waypoints (with how close each must
/// be reached), and where it starts and ends.
#[derive(Clone, Debug, Default)]
pub struct Leg {
    pub points: Vec<(Vec3, f32)>,
    pub start: Vec3,
    pub end: Vec3,
}

/// Where leg `i` of `g` begins for a body at `at` (`AI2iMovementState_MakePath`):
/// there in the first room; after, the nearest point of the portal it came
/// through (it may still stand a little short of it).
pub fn leg_start(nav: &Navigation, g: &Global, i: usize, at: Vec3) -> Vec3 {
    match i.checked_sub(1) {
        None => at,
        Some(k) => {
            let p = &nav.rooms[g.portals[k].0].portals[g.portals[k].1];
            let d = Vec2::new(p.b.x - p.a.x, p.b.z - p.a.z).normalize_or_zero();
            let t = Vec2::new(at.x - p.a.x, at.z - p.a.z)
                .dot(d)
                .clamp(0.0, p.width);
            along(p, t)
        }
    }
}

/// Where leg `i` ends, starting from `start`: where it crosses into the next
/// room (looking ahead, `AI2rPath_FindGhostWaypoint`), or the destination.
pub fn leg_end(nav: &Navigation, g: &Global, i: usize, start: Vec3) -> Vec3 {
    if i < g.portals.len() {
        crossing(nav, &g.portals, i, start, g.to)
    } else {
        g.to
    }
}

/// Plans room `i` of `g` for a body at `at` (`AI2iPath_NewNode` →
/// `ASrPath_Generate`): A\* across its grid round what `obstructed` says
/// blocks there, to where it crosses into the next room or to the
/// destination. `careful`: don't smooth (after a failure).
pub fn leg(
    nav: &Navigation,
    g: &Global,
    i: usize,
    at: Vec3,
    obstructed: &mut dyn FnMut(usize) -> Option<super::dynamic::Blocking>,
    careful: bool,
) -> Result<Leg, Error> {
    let room = g.room(nav, i);
    let start = leg_start(nav, g, i, at);
    let end = leg_end(nav, g, i, start);
    let next_stairs = i < g.portals.len() && nav.rooms[g.room(nav, i + 1)].is_stairs();
    let r = &nav.rooms[room];
    let points = match &r.grid {
        None => vec![(end, Square::Clear.arrive())],
        Some(grid) => {
            let blocked = obstructed(room);
            let rules = Rules {
                stairs: next_stairs || r.is_stairs(),
                obstructed: blocked.as_ref(),
            };
            let (a, b) = (Vec2::new(start.x, start.z), Vec2::new(end.x, end.z));
            let cells = grid::search(grid, a, b, rules).ok_or(Error::GridBlocked)?;
            grid::waypoints(grid, &cells, b, r.is_stairs(), careful, blocked.as_ref())
                .into_iter()
                .map(|Waypoint { at: w, arrive }| (Vec3::new(w.x, end.y.max(r.min.y), w.y), arrive))
                .collect()
        }
    };
    Ok(Leg { points, start, end })
}

/// Plans a whole path at once, room after room (for tests).
#[cfg(test)]
pub fn plan(
    nav: &Navigation,
    from: Vec3,
    to: Vec3,
    obstructed: &mut dyn FnMut(usize) -> Option<super::dynamic::Blocking>,
    careful: bool,
    shut: &HashSet<(usize, usize)>,
) -> Result<Path, Error> {
    let g = global(nav, from, to, shut)?;
    let mut path = Path::default();
    let mut at = from;
    for i in 0..g.legs() {
        path.rooms.push(g.room(nav, i));
        let l = leg(nav, &g, i, at, obstructed, careful)?;
        path.points.extend(l.points);
        at = l.end;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn level1() -> Option<Navigation> {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../generated/oni/levels/01");
        Navigation::load(&dir).ok()
    }

    #[test]
    fn plans_across_level_1() {
        let Some(nav) = level1() else {
            eprintln!("SKIP: generated navigation not found");
            return;
        };
        let from = Vec3::new(200.0, 0.1, 300.0);
        let start = nav.room_at(from).expect("the start is in a room");
        // Every grid room's middle at floor height that's walkable: most
        // must be reachable, and paths never stand on walls.
        let (mut tried, mut found, mut longest) = (0, 0, 0);
        for (i, r) in nav.rooms.iter().enumerate() {
            if r.is_stairs() || i == start {
                continue;
            }
            let mid = (r.min + r.max) * 0.5;
            let to = Vec3::new(mid.x, r.min.y + 0.5, mid.z);
            if r.square_at(to.x, to.z) != Square::Clear || nav.room_at(to) != Some(i) {
                continue;
            }
            tried += 1;
            if let Ok(p) = plan(&nav, from, to, &mut |_| None, false, &HashSet::new()) {
                found += 1;
                longest = longest.max(p.rooms.len());
                for (w, _) in &p.points {
                    let room = nav.room_at(*w + Vec3::Y);
                    if let Some(room) = room {
                        assert_ne!(
                            nav.rooms[room].square_at(w.x, w.z),
                            Square::Impassable,
                            "waypoint in a wall"
                        );
                    }
                }
            }
        }
        eprintln!("level 1: {found}/{tried} rooms reachable from the start, longest through {longest} rooms");
        // (The rest are parts of the level no portal leads to.)
        assert!(found * 10 >= tried * 8, "most rooms are reachable");
    }
}
