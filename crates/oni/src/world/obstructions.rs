//! What blocks a path search: the
//! world's side of the rooms' obstruction stamps (`nav::dynamic`) — what
//! there is to stamp, who may pass what (`PHrIgnoreObstruction`), planning
//! paths over them, and the local probes' view of them.

use std::collections::HashSet;
use std::rc::Rc;

use glam::Vec3;

use crate::nav::dynamic::{Blocking, DoorSource, Mode, Obstruction, Obstructions, Sources};
use crate::nav::Navigation;

use super::objects::oflag;
use super::World;

/// What decides whether a stamp blocks, all but who asks: taken once a
/// tick (`World::tick_snapshot`), or fresh.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub sources: Sources,
    /// Each door: whether a path may not cross it at all (open — its
    /// leaves stand where they swung to — or nobody may open it), and
    /// whether it is manual.
    doors: Vec<(bool, bool)>,
    /// Each placed object: in use and visible.
    objects: Vec<bool>,
}

impl Snapshot {
    /// Whether obstruction `o` blocks fighter `asker` in `mode`
    /// (`PHrIgnoreObstruction`); `through(j)`: it may path through
    /// character `j` (`AI2rTryToPathfindThrough`).
    pub fn blocks(
        &self,
        asker: usize,
        mode: Mode,
        through: &dyn Fn(usize) -> bool,
        o: Obstruction,
    ) -> bool {
        match o {
            Obstruction::Character(j) => j != asker && (mode == Mode::ClearSpace || !through(j)),
            Obstruction::Object(k) => self.objects.get(k).copied().unwrap_or(false),
            Obstruction::Door(d) => {
                let Some(&(shut, manual)) = self.doors.get(d) else {
                    return false;
                };
                mode == Mode::ClearSpace || shut || (mode == Mode::Casual && manual)
            }
        }
    }
}

/// The local probes' view of the stamps for fighter `asker`
/// (`PHrLocalPathWeight` in casual movement): whether square `c` of room
/// `r` is obstructed for it. `through`: the characters it may pass.
pub fn local_view<'a>(
    obstructions: &'a Obstructions,
    nav: &'a Navigation,
    now: u64,
    snap: &'a Snapshot,
    asker: usize,
    through: Vec<usize>,
) -> impl Fn(usize, (i32, i32)) -> bool + 'a {
    view_in(obstructions, nav, now, snap, asker, through, Mode::Casual)
}

/// [`local_view`] in path mode `mode`.
fn view_in<'a>(
    obstructions: &'a Obstructions,
    nav: &'a Navigation,
    now: u64,
    snap: &'a Snapshot,
    asker: usize,
    through: Vec<usize>,
    mode: Mode,
) -> impl Fn(usize, (i32, i32)) -> bool + 'a {
    move |r, c| {
        obstructions
            .room(nav, r, now, &snap.sources)
            .and_then(|s| s.at(c))
            .is_some_and(|o| snap.blocks(asker, mode, &|j| through.contains(&j), o))
    }
}

impl World {
    /// What decides whether stamps block, now.
    pub(super) fn snapshot(&mut self) -> Snapshot {
        let doors = self.door_sources();
        let nav = self.nav.as_deref();
        let characters = self
            .fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.gone && !f.death.settled && !f.effects.teleporting)
            .map(|(i, f)| (i, f.feet(), nav.and_then(|n| n.room_at(f.feet()))))
            .collect();
        let live = |k: usize| {
            self.placed.list[k].flags & (oflag::IN_USE | oflag::INVISIBLE) == oflag::IN_USE
        };
        let objects = (0..self.placed.list.len())
            .filter(|&k| live(k))
            .map(|k| {
                let o = &self.placed.list[k];
                (
                    k,
                    self.object_matrix(k).w_axis.truncate(),
                    self.placed.setups[o.setup].radius,
                )
            })
            .collect();
        Snapshot {
            sources: Sources {
                characters,
                objects,
                doors,
                object_row_only: self.quirks.object_stamp_one_row(),
            },
            doors: self
                .doors
                .list
                .iter()
                .map(|d| {
                    let open = matches!(
                        d.state,
                        crate::doors::State::Opening(_) | crate::doors::State::Open(_)
                    );
                    (open || !d.may_open_either_side(), d.manual)
                })
                .collect(),
            objects: (0..self.placed.list.len()).map(live).collect(),
        }
    }

    /// This tick's snapshot (taken at its first use in the tick).
    pub(super) fn tick_snapshot(&mut self) -> Rc<Snapshot> {
        let now = self.now as u64;
        if let Some((_, s)) = self.obstruction_tick.as_ref().filter(|(t, _)| *t == now) {
            return s.clone();
        }
        let s = Rc::new(self.snapshot());
        self.obstruction_tick = Some((now, s.clone()));
        s
    }

    /// The doors' leaves, each with the rooms at the ends of the portals
    /// it gates (`PHiAddDoorConnectionToGrid`: stamped by each connection
    /// into or out of a room).
    fn door_sources(&mut self) -> Vec<DoorSource> {
        let Some(nav) = self.nav.clone() else {
            return Vec::new();
        };
        let links = self.doors.portals(&nav).clone();
        links
            .iter()
            .enumerate()
            .filter(|(_, ps)| !ps.is_empty())
            .filter_map(|(d, ps)| {
                let lines = self.doors.list[d].obstruction();
                (!lines.is_empty()).then(|| DoorSource {
                    door: d,
                    rooms: ps
                        .iter()
                        .flat_map(|&(r, k)| [r, nav.rooms[r].portals[k].to])
                        .collect(),
                    lines,
                })
            })
            .collect()
    }

    /// The characters fighter `asker` may path through
    /// (`AI2rTryToPathfindThrough`): whom it goes to or follows, its
    /// combat (or melee) target, the player while it talks to them.
    pub(super) fn paths_through(&self, asker: usize) -> Vec<usize> {
        let Some(b) = self.fighters.get(asker).and_then(|f| f.brain.as_ref()) else {
            return Vec::new();
        };
        (0..self.fighters.len())
            .filter(|&j| match b {
                crate::ai::Brain::Agent(a) => a.paths_through(j, j == 0),
                b => {
                    b.navigator().is_some_and(|n| n.ignore == Some(j))
                        || b.melee().is_some_and(|m| m.target == j)
                }
            })
            .collect()
    }

    /// What is stamped at world point `p` (in its room, made now if old),
    /// and whether it blocks fighter `asker` planning a path (as things
    /// stand now).
    pub fn obstruction_at(&mut self, p: glam::Vec3, asker: usize) -> Option<(Obstruction, bool)> {
        let nav = self.nav.clone()?;
        let room = nav.room_at(p)?;
        let cell = nav.rooms[room].grid.as_ref()?.cell_of(p.x, p.z)?;
        let snap = self.snapshot();
        let stamps = self
            .obstructions
            .room(&nav, room, self.now as u64, &snap.sources)?;
        let o = stamps.at(cell)?;
        let through = self.paths_through(asker);
        Some((
            o,
            snap.blocks(asker, Mode::Directed, &|j| through.contains(&j), o),
        ))
    }

    /// Fighter `id`'s local probe along the floor in direction `dir` up to
    /// `dist`, as its AI's local movement sees it (`AI2rTryLocalMovement`:
    /// obstructions counted, stopping at border 3).
    pub fn local_probe(
        &mut self,
        id: usize,
        dir: glam::Vec2,
        dist: f32,
    ) -> Option<crate::nav::LocalProbe> {
        let nav = self.nav.clone()?;
        let from = self.fighters.get(id)?.feet() + glam::Vec3::Y * 2.0;
        let snap = self.tick_snapshot();
        let through = self.paths_through(id);
        let view = local_view(
            &self.obstructions,
            &nav,
            self.now as u64,
            &snap,
            id,
            through,
        );
        Some(nav.probe(from, dir, dist, crate::nav::Square::Border3, Some(&view)))
    }

    /// An AI just come into play standing inside an obstruction steps out
    /// of it (`AI2iMovementState_FindClearPoint`): unless it is a body more
    /// than 6 from where it really stands, or a door it may not open (it
    /// mustn't come out on the wrong side), it moves to the first clear
    /// point 15 away — straight ahead, then 30° at a time either side, six
    /// tries a side. Returns whether it moved.
    pub(super) fn step_out_of_obstruction(&mut self, id: usize) -> bool {
        const DISTANCE: f32 = 15.0; // AI2cMovementState_ObstructionCheckDistance
        const ATTEMPTS: u32 = 6; // AI2cMovementState_ObstructionCheckAttempts
        const BODY_CLEAR: f32 = 6.0;
        let Some(nav) = self.nav.clone() else {
            return false;
        };
        let feet = self.fighters[id].feet();
        let Some(room) = nav.room_at(feet) else {
            return false;
        };
        let Some(g) = nav.rooms[room].grid.as_ref() else {
            return false;
        };
        let cell = (
            (((feet.x - g.origin.x) / crate::nav::grid::SQUARE).floor() as i32)
                .clamp(0, g.width - 1),
            (((feet.z - g.origin.y) / crate::nav::grid::SQUARE).floor() as i32)
                .clamp(0, g.height - 1),
        );
        let snap = self.snapshot();
        let now = self.now as u64;
        let Some(o) = self
            .obstructions
            .room(&nav, room, now, &snap.sources)
            .and_then(|s| s.at(cell))
        else {
            return false;
        };
        if !snap.blocks(id, Mode::ClearSpace, &|_| false, o) {
            return false;
        }
        match o {
            Obstruction::Character(j) => {
                let other = self.fighters[j].feet();
                if (other - feet).with_y(0.0).length() > BODY_CLEAR {
                    return false;
                }
            }
            Obstruction::Door(d) if !self.doors.list[d].may_open_either_side() => return false,
            _ => {}
        }
        let view = view_in(
            &self.obstructions,
            &nav,
            now,
            &snap,
            id,
            Vec::new(),
            Mode::ClearSpace,
        );
        let facing = self.fighters[id].facing.facing;
        let step = std::f32::consts::PI / ATTEMPTS as f32;
        let from = feet + glam::Vec3::Y * 2.0;
        let found = std::iter::once(0.0)
            .chain((1..=ATTEMPTS).flat_map(|k| [k as f32 * step, -(k as f32) * step]))
            .map(|off| facing + off)
            .find(|&a| {
                let dir = glam::Vec2::new(a.sin(), a.cos());
                nav.probe_escaping(
                    from,
                    dir,
                    DISTANCE,
                    crate::nav::Square::Danger,
                    Some(&view),
                    5,
                )
                .stopped
                .is_none()
            });
        drop(view);
        let Some(a) = found else { return false };
        let to = feet + glam::Vec3::new(a.sin(), 0.0, a.cos()) * DISTANCE;
        self.teleport(id, to, None);
        true
    }

    /// Plans for every navigator that needs it, round what blocks it: a
    /// whole new path (the rooms, and the first room's stretch), the
    /// stretch of a room just come into, or — given a new destination while
    /// following — whether the path can be kept.
    pub(super) fn plan_paths(&mut self) {
        let Some(nav) = self.nav.clone() else { return };
        let now = self.now as u64;
        for i in 0..self.fighters.len() {
            let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
                continue;
            };
            if n.retarget && !self.keep_path(&nav, i) {
                if let Some(n) = self.navigator_of(i) {
                    n.retarget = false;
                    n.state = crate::ai::navigate::State::Planning;
                }
            }
            let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
                continue;
            };
            if n.wants_plan(now) {
                self.plan_whole(&nav, i);
            } else if n.wants_leg() {
                self.plan_leg(&nav, i);
            }
        }
    }

    fn navigator_of(&mut self, i: usize) -> Option<&mut crate::ai::navigate::Navigator> {
        self.fighters[i]
            .brain
            .as_mut()
            .and_then(|b| b.navigator_mut())
    }

    /// What blocks fighter `i` planning a path in a room.
    fn planning_view<'a>(
        &'a self,
        nav: &'a crate::nav::Navigation,
        snap: &'a Snapshot,
        i: usize,
    ) -> impl FnMut(usize) -> Option<Blocking> + 'a {
        let through = self.paths_through(i);
        let now = self.now as u64;
        move |room: usize| {
            let stamps = self.obstructions.room(nav, room, now, &snap.sources)?;
            Some(Blocking::new(stamps, |o| {
                snap.blocks(i, Mode::Directed, &|j| through.contains(&j), o)
            }))
        }
    }

    /// A whole new path for fighter `i` (`AI2iPath_MakePath`).
    fn plan_whole(&mut self, nav: &crate::nav::Navigation, i: usize) {
        use crate::nav::plan;
        let now = self.now as u64;
        let from = self.fighters[i].feet();
        let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
            return;
        };
        let (careful, follow, mut goal) = (n.careful, n.follow, n.goal);
        // Following someone: the path heads for where they are now
        // (`AI2iPath_MakePath` reads the followed character's place).
        if let Some(at) = follow.and_then(|j| self.fighters.get(j)).map(|f| f.feet()) {
            goal = at;
        }
        let snap = self.tick_snapshot();
        // Doors it may not open shut their portals to it.
        let links = self.doors.portals(nav).clone();
        let shut: HashSet<(usize, usize)> = links
            .iter()
            .enumerate()
            .filter(|&(k, _)| !self.doors.list[k].may_open(from, false))
            .flat_map(|(_, ps)| ps.iter().copied())
            .collect();
        let result = plan::global(nav, from, goal, &shut).and_then(|g| {
            let mut view = self.planning_view(nav, &snap, i);
            plan::leg(nav, &g, 0, from, &mut view, careful).map(|first| (g, first))
        });
        if let Err(e) = &result {
            eprintln!("nav: fighter {i}: no path {from:?} → {goal:?}: {e:?}");
        }
        if let Some(n) = self.navigator_of(i) {
            n.goal = goal;
            n.take_plan(result, now);
        }
    }

    /// The stretch of the room fighter `i` has come into (`AI2iPath_NextNode`).
    fn plan_leg(&mut self, nav: &crate::nav::Navigation, i: usize) {
        let now = self.now as u64;
        let from = self.fighters[i].feet();
        let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
            return;
        };
        let (Some(g), k, careful) = (n.global.clone(), n.leg, n.careful) else {
            return;
        };
        let snap = self.tick_snapshot();
        let result = {
            let mut view = self.planning_view(nav, &snap, i);
            crate::nav::plan::leg(nav, &g, k, from, &mut view, careful)
        };
        if let Err(e) = &result {
            eprintln!(
                "nav: fighter {i}: no way across room {}: {e:?}",
                g.room(nav, k)
            );
        }
        if let Some(n) = self.navigator_of(i) {
            n.take_leg(result, now);
        }
    }

    /// Given a new destination, whether fighter `i` keeps its path
    /// (`AI2iPath_MakePath`, `AI2rMovementState_MakePath`): planned under 90
    /// ticks ago for a place under 20 from the new one, still in the room it
    /// started from and going to the same room — and then its first room's
    /// stretch too, if its ends move little (`AI2iMovementState_AdjustPathPoint`);
    /// else that stretch is planned again. False: it needs a new path.
    fn keep_path(&mut self, nav: &crate::nav::Navigation, i: usize) -> bool {
        const MAX_REUSE_TIME: u64 = 90; // AI2cNewPath_MaxReuseTime
        const MAX_REUSE_DIST: f32 = 20.0; // AI2cNewPath_MaxReuseDist
        let now = self.now as u64;
        let feet = self.fighters[i].feet();
        let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
            return false;
        };
        let Some(mut g) = n.global.clone() else {
            return false;
        };
        let reusable = n.leg == 0
            && !n.leg_due
            && now < n.planned.0 + MAX_REUSE_TIME
            && n.goal.distance(n.planned.1) < MAX_REUSE_DIST
            && nav.room_at(feet) == Some(g.start)
            && nav.room_at(n.goal + Vec3::Y * 0.5) == Some(g.goal);
        if !reusable {
            return false;
        }
        let (goal, (old_start, old_end), careful) = (n.goal, n.leg_ends, n.careful);
        g.to = goal;
        let new_end = crate::nav::plan::leg_end(nav, &g, 0, feet);
        let snap = self.tick_snapshot();
        let grid_ok = {
            let mut view = self.planning_view(nav, &snap, i);
            let blocked = view(g.start);
            let adjust =
                |old: Vec3, new: Vec3| adjust_point(nav, g.start, old, new, blocked.as_ref());
            adjust(old_start, feet) && adjust(old_end, new_end)
        };
        if grid_ok {
            if let Some(n) = self.navigator_of(i) {
                n.keep_leg(new_end);
            }
            return true;
        }
        // The rooms kept; the first room's stretch planned again.
        let result = {
            let mut view = self.planning_view(nav, &snap, i);
            crate::nav::plan::leg(nav, &g, 0, feet, &mut view, careful)
        };
        if let Some(n) = self.navigator_of(i) {
            n.global = Some(g);
            n.retarget = false;
            n.take_leg(result, now);
        }
        true
    }
}

/// Whether a stretch's end can move from `old` to `new` without planning it
/// again (`AI2iMovementState_AdjustPathPoint`): within 4 always, beyond 25
/// never, else if the grid line between is clear (no worse than border 2,
/// no escaping).
fn adjust_point(
    nav: &crate::nav::Navigation,
    room: usize,
    old: Vec3,
    new: Vec3,
    blocked: Option<&Blocking>,
) -> bool {
    const MIN: f32 = 4.0; // AI2cMovementState_Reuse_MinDist
    const MAX: f32 = 25.0; // AI2cMovementState_Reuse_MaxDist
    let d = old.distance(new);
    if d < MIN {
        return true;
    }
    if d > MAX {
        return false;
    }
    let Some(g) = nav.rooms.get(room).and_then(|r| r.grid.as_ref()) else {
        return true;
    };
    let (Some(a), Some(b)) = (g.cell_of(old.x, old.z), g.cell_of(new.x, new.z)) else {
        return false;
    };
    let line = crate::nav::grid::line(g, a, b, crate::nav::Square::Border2, 3, blocked);
    line.reached && !line.escaping
}
