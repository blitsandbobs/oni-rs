//! Pathfinding obstructions: the dynamic layer over each room's grid
//! (`PHtDynamicSquare`, `PHrPrepareRoomForPath`). When a search uses a
//! room, the characters in it, the placed objects and the door leaves on
//! its portals are stamped into a copy of its grid, if the last stamps are
//! [`AGE`] ticks old or the room was flushed. Each stamped square holds the
//! index of what stamped it; whether that blocks is decided per search, by
//! who asks ([`Blocking`], `PHrIgnoreObstruction`).

use std::collections::HashMap;
use std::rc::Rc;

use glam::Vec3;

use super::{grid, Grid, Navigation};

/// Stamps this old are made again (`PHcDynamicDataAgeTime`).
pub const AGE: u64 = 20;
/// A room holds at most this many obstructions (`PHcMaxObstructions`, less
/// the count in entry 0).
pub const MAX: usize = 31;

/// What stamped a square.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obstruction {
    /// A fighter (by index).
    Character(usize),
    /// A placed object (by index in `Placed::list`).
    Object(usize),
    /// A door (by index in `Doors::list`).
    Door(usize),
}

/// Why a path is wanted (`PHtPathMode`): it decides which obstructions a
/// character may pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Is the space clear (nothing is passable).
    ClearSpace,
    /// Local movement (melee, dodging): no walking into manual doors.
    Casual,
    /// Following a planned path.
    Directed,
}

/// What there is to stamp this tick.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    /// The characters (not settled dead, teleporting or gone): index, feet,
    /// room.
    pub characters: Vec<(usize, Vec3, Option<usize>)>,
    /// The placed objects in use and visible: index, position, sphere
    /// radius.
    pub objects: Vec<(usize, Vec3, f32)>,
    /// The doors: index, a room for each end of each portal it gates, and
    /// its leaves' base edges.
    pub doors: Vec<DoorSource>,
    /// Stamp an object's last row only (shipped, Q28).
    pub object_row_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DoorSource {
    pub door: usize,
    /// The rooms at either end of each portal it gates (a room once per
    /// portal end there).
    pub rooms: Vec<usize>,
    pub lines: Vec<(Vec3, Vec3)>,
}

/// A room's stamps.
#[derive(Clone, Debug)]
pub struct Stamps {
    origin: glam::Vec2,
    width: i32,
    height: i32,
    /// 0: nothing; k: `table[k - 1]`.
    cells: Vec<u8>,
    table: Vec<Obstruction>,
    built: u64,
}

impl Stamps {
    fn new(g: &Grid, now: u64) -> Stamps {
        Stamps {
            origin: g.origin,
            width: g.width,
            height: g.height,
            cells: vec![0; (g.width * g.height) as usize],
            table: Vec::new(),
            built: now,
        }
    }

    /// The square a world point (x, z) falls in, on the grid or not
    /// (`PHrWorldToGridSpaceDangerous`).
    fn cell(&self, p: Vec3) -> (i32, i32) {
        (
            ((p.x - self.origin.x) / grid::SQUARE).floor() as i32,
            ((p.z - self.origin.y) / grid::SQUARE).floor() as i32,
        )
    }

    fn on_grid(&self, (i, j): (i32, i32)) -> bool {
        i >= 0 && j >= 0 && i < self.width && j < self.height
    }

    /// The obstruction index stamped at a square (1-based), if any.
    fn index(&self, c: (i32, i32)) -> Option<u8> {
        if !self.on_grid(c) {
            return None;
        }
        Some(self.cells[(c.0 + c.1 * self.width) as usize]).filter(|&k| k != 0)
    }

    /// What stamped a square, if anything.
    pub fn at(&self, c: (i32, i32)) -> Option<Obstruction> {
        self.index(c).map(|k| self.table[usize::from(k) - 1])
    }

    /// Enters an obstruction in the table: its index, or None when full.
    fn add(&mut self, o: Obstruction) -> Option<u8> {
        if self.table.len() >= MAX {
            eprintln!("nav: too many obstructions in one room ({o:?} left out)");
            return None;
        }
        self.table.push(o);
        Some(self.table.len() as u8)
    }

    /// Stamps a square (over whatever is there; `PHrPutObstruction`).
    fn put(&mut self, c: (i32, i32), k: u8) {
        if self.on_grid(c) {
            self.cells[(c.0 + c.1 * self.width) as usize] = k;
        }
    }

    /// A character (`PHrAddCharacterToGrid`): a 5×5 block less its corners.
    fn character(&mut self, who: usize, feet: Vec3) {
        let (gx, gy) = self.cell(feet);
        if gx < -2 || gx >= self.width + 2 || gy < -2 || gy >= self.height + 2 {
            return;
        }
        let Some(k) = self.add(Obstruction::Character(who)) else {
            return;
        };
        for dy in -2..=2i32 {
            for dx in -2..=2i32 {
                if dx.abs() == 2 && dy.abs() == 2 {
                    continue;
                }
                self.put((gx + dx, gy + dy), k);
            }
        }
    }

    /// A placed object (`PHrAddObjectToGrid`): the squares round its
    /// sphere, `round(r / 4) + 1` out. As shipped the row loop starts at
    /// its last row, so only that row is stamped (`row_only`, Q28).
    fn object(&mut self, index: usize, at: Vec3, radius: f32, row_only: bool) {
        let (gx, gy) = self.cell(at);
        let r = (radius / grid::SQUARE).round() as i32 + 1;
        if gx < -r || gx >= self.width + r || gy < -r || gy >= self.height + r {
            return;
        }
        let Some(k) = self.add(Obstruction::Object(index)) else {
            return;
        };
        let rows = if row_only { r..=r } else { -r..=r };
        for v in rows {
            for h in -r..=r {
                self.put((gx + h, gy + v), k);
            }
        }
    }

    /// A door's leaves (`PHrAddDoorToGrid`): a line along each base edge,
    /// over nothing already stamped. One entry for all its lines.
    fn door(&mut self, index: usize, lines: &[(Vec3, Vec3)]) {
        let mut k = None;
        for &(a, b) in lines {
            let (p0, p1) = (self.cell(a), self.cell(b));
            if (p0.0 < 0 && p1.0 < 0)
                || (p0.0 >= self.width && p1.0 >= self.width)
                || (p0.1 < 0 && p1.1 < 0)
                || (p0.1 >= self.height && p1.1 >= self.height)
            {
                continue;
            }
            if k.is_none() {
                k = self.add(Obstruction::Door(index));
                if k.is_none() {
                    return;
                }
            }
            for c in bresenham(p0, p1) {
                if self.index(c).is_none() {
                    self.put(c, k.unwrap_or(0));
                }
            }
        }
    }
}

/// The squares of a line from `a` to `b` (`PHrDynamicBresenham2`: plain,
/// diagonal steps and all).
fn bresenham(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (ax, ay) = (dx.abs(), dy.abs());
    let (xdir, ydir) = (dx.signum(), dy.signum());
    let (mut x, mut y) = a;
    let mut out = Vec::new();
    if ax > ay {
        let mut e = (ay - ax) / 2;
        loop {
            out.push((x, y));
            if x == b.0 {
                return out;
            }
            if e >= 0 {
                y += ydir;
                e -= ax;
            }
            x += xdir;
            e += ay;
        }
    } else {
        let mut e = (ax - ay) / 2;
        loop {
            out.push((x, y));
            if y == b.1 {
                return out;
            }
            if e >= 0 {
                x += xdir;
                e -= ay;
            }
            y += ydir;
            e += ax;
        }
    }
}

/// A room's stamps as one character sees them: which obstructions block it
/// (each decided once, as the original's bit vector remembers).
#[derive(Clone, Debug)]
pub struct Blocking {
    stamps: Rc<Stamps>,
    /// Bit k: obstruction k blocks.
    mask: u32,
}

impl Blocking {
    /// Decides each of a room's obstructions with `blocks`.
    pub fn new(stamps: Rc<Stamps>, blocks: impl Fn(Obstruction) -> bool) -> Blocking {
        let mask = stamps
            .table
            .iter()
            .enumerate()
            .filter(|(_, o)| blocks(**o))
            .fold(0, |m, (k, _)| m | 1 << (k + 1));
        Blocking { stamps, mask }
    }

    /// Whether square `c` is obstructed for it.
    pub fn blocks(&self, c: (i32, i32)) -> bool {
        self.stamps
            .index(c)
            .is_some_and(|k| self.mask & (1 << k) != 0)
    }
}

/// The rooms' stamps, made as searches need them.
#[derive(Clone, Debug, Default)]
pub struct Obstructions {
    rooms: HashMap<usize, Rc<Stamps>>,
}

impl Obstructions {
    /// Room `room`'s stamps must be made again before they are next used
    /// (`PHrFlushDynamicGrid`: someone has just appeared there).
    pub fn flush(&mut self, room: usize) {
        self.rooms.remove(&room);
    }

    /// Forgets every room's stamps.
    pub fn clear(&mut self) {
        self.rooms.clear();
    }

    /// Room `room`'s stamps at tick `now` (`PHrPrepareRoomForPath`): made
    /// again from `sources` if [`AGE`] old or flushed. None: it has no grid.
    pub fn room(
        &mut self,
        nav: &Navigation,
        room: usize,
        now: u64,
        sources: &Sources,
    ) -> Option<Rc<Stamps>> {
        let g = nav.rooms.get(room)?.grid.as_ref()?;
        if let Some(s) = self.rooms.get(&room).filter(|s| now < s.built + AGE) {
            return Some(s.clone());
        }
        let s = Rc::new(stamp(g, room, now, sources));
        self.rooms.insert(room, s.clone());
        Some(s)
    }
}

/// Stamps a room: characters, then objects, then doors.
fn stamp(g: &Grid, room: usize, now: u64, sources: &Sources) -> Stamps {
    let mut s = Stamps::new(g, now);
    for &(who, feet, in_room) in &sources.characters {
        if in_room == Some(room) {
            s.character(who, feet);
        }
    }
    for &(k, at, r) in &sources.objects {
        s.object(k, at, r, sources.object_row_only);
    }
    for d in &sources.doors {
        // (Once per portal end here, as the original stamps per connection.)
        for _ in d.rooms.iter().filter(|&&r| r == room) {
            s.door(d.door, &d.lines);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: i32, h: i32) -> Grid {
        Grid {
            origin: glam::Vec2::ZERO,
            width: w,
            height: h,
            cells: vec![0; (w * h) as usize],
        }
    }

    /// The world point at the centre of square (i, j).
    fn at(i: i32, j: i32) -> Vec3 {
        Vec3::new((i as f32 + 0.5) * 4.0, 0.0, (j as f32 + 0.5) * 4.0)
    }

    fn count(s: &Stamps) -> usize {
        s.cells.iter().filter(|&&c| c != 0).count()
    }

    #[test]
    fn a_character_is_a_block_without_corners() {
        let mut s = Stamps::new(&grid(10, 10), 0);
        s.character(3, at(5, 5));
        assert_eq!(count(&s), 21);
        assert_eq!(s.at((5, 5)), Some(Obstruction::Character(3)));
        assert_eq!(s.at((7, 7)), None);
        assert_eq!(s.at((7, 6)), Some(Obstruction::Character(3)));
    }

    #[test]
    fn an_object_covers_its_square_or_as_shipped_one_row() {
        let mut s = Stamps::new(&grid(20, 20), 0);
        s.object(0, at(10, 10), 4.0, false); // r = 1 + 1 = 2: 5×5
        assert_eq!(count(&s), 25);
        let mut s = Stamps::new(&grid(20, 20), 0);
        s.object(0, at(10, 10), 4.0, true);
        assert_eq!(count(&s), 5);
        assert!(s.at((10, 12)).is_some() && s.at((10, 10)).is_none());
    }

    #[test]
    fn a_door_line_doesnt_overwrite() {
        let mut s = Stamps::new(&grid(10, 10), 0);
        s.character(0, at(2, 5));
        s.door(7, &[(at(0, 5), at(9, 5))]);
        assert_eq!(s.at((2, 5)), Some(Obstruction::Character(0)));
        assert_eq!(s.at((8, 5)), Some(Obstruction::Door(7)));
        assert_eq!(
            bresenham((0, 0), (3, 1)),
            vec![(0, 0), (1, 0), (2, 1), (3, 1)]
        );
    }

    #[test]
    fn a_room_holds_31() {
        let mut s = Stamps::new(&grid(100, 100), 0);
        for k in 0..40 {
            s.character(k, at(2 + (k as i32 % 10) * 5, 2 + (k as i32 / 10) * 5));
        }
        assert_eq!(s.table.len(), MAX);
    }

    #[test]
    fn blocking_is_per_asker() {
        let mut s = Stamps::new(&grid(10, 10), 0);
        s.character(0, at(2, 2));
        s.character(1, at(7, 7));
        let b = Blocking::new(Rc::new(s), |o| o != Obstruction::Character(0));
        assert!(!b.blocks((2, 2)));
        assert!(b.blocks((7, 7)));
    }
}
