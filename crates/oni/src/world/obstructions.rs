//! What blocks a path search: the
//! world's side of the rooms' obstruction stamps (`nav::dynamic`) — what
//! there is to stamp, who may pass what (`PHrIgnoreObstruction`), planning
//! paths over them, and the local probes' view of them.

use std::collections::HashSet;
use std::rc::Rc;

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
    move |r, c| {
        obstructions
            .room(nav, r, now, &snap.sources)
            .and_then(|s| s.at(c))
            .is_some_and(|o| snap.blocks(asker, Mode::Casual, &|j| through.contains(&j), o))
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

    /// Plans a path for every navigator that needs one, round what blocks
    /// it.
    pub(super) fn plan_paths(&mut self) {
        let Some(nav) = self.nav.clone() else { return };
        let now = self.now as u64;
        for i in 0..self.fighters.len() {
            let from = self.fighters[i].feet();
            let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
                continue;
            };
            if !n.wants_plan(now) {
                continue;
            }
            let (careful, follow) = (n.careful, n.follow);
            // Following someone: the path heads for where they are now
            // (`AI2iPath_MakePath` reads the followed character's place).
            if let Some(at) = follow.and_then(|j| self.fighters.get(j)).map(|f| f.feet()) {
                if let Some(n) = self.fighters[i]
                    .brain
                    .as_mut()
                    .and_then(|b| b.navigator_mut())
                {
                    n.goal = at;
                }
            }
            let goal = self.fighters[i]
                .brain
                .as_ref()
                .and_then(|b| b.navigator())
                .map_or(from, |n| n.goal);
            let snap = self.tick_snapshot();
            let through = self.paths_through(i);
            // Doors it may not open shut their portals to it.
            let links = self.doors.portals(&nav).clone();
            let shut: HashSet<(usize, usize)> = links
                .iter()
                .enumerate()
                .filter(|&(k, _)| !self.doors.list[k].may_open(from, false))
                .flat_map(|(_, ps)| ps.iter().copied())
                .collect();
            let obstructions = &self.obstructions;
            let mut blocking = |room: usize| {
                let stamps = obstructions.room(&nav, room, now, &snap.sources)?;
                Some(Blocking::new(stamps, |o| {
                    snap.blocks(i, Mode::Directed, &|j| through.contains(&j), o)
                }))
            };
            let plan = crate::nav::plan::plan(&nav, from, goal, &mut blocking, careful, &shut);
            if let Err(e) = &plan {
                eprintln!("nav: fighter {i}: no path {from:?} → {goal:?}: {e:?}");
            }
            if let Some(n) = self.fighters[i]
                .brain
                .as_mut()
                .and_then(|b| b.navigator_mut())
            {
                n.take_plan(plan, now);
            }
        }
    }
}
