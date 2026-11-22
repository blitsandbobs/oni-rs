//! What blocks a path search: the
//! world's side of the rooms' obstruction stamps (`nav::dynamic`) — what
//! there is to stamp, who may pass what (`PHrIgnoreObstruction`), and
//! planning paths over them.

use std::collections::HashSet;

use crate::nav::dynamic::{Blocking, DoorSource, Mode, Obstruction, Sources};

use super::objects::oflag;
use super::World;

impl World {
    /// What there is to stamp now.
    pub(super) fn obstruction_sources(&mut self) -> Sources {
        let doors = self.door_sources();
        let nav = self.nav.as_deref();
        let characters = self
            .fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.gone && !f.death.settled && !f.effects.teleporting)
            .map(|(i, f)| (i, f.feet(), nav.and_then(|n| n.room_at(f.feet()))))
            .collect();
        let objects = (0..self.placed.list.len())
            .filter(|&k| {
                self.placed.list[k].flags & (oflag::IN_USE | oflag::INVISIBLE) == oflag::IN_USE
            })
            .map(|k| {
                let o = &self.placed.list[k];
                (
                    k,
                    self.object_matrix(k).w_axis.truncate(),
                    self.placed.setups[o.setup].radius,
                )
            })
            .collect();
        Sources {
            characters,
            objects,
            doors,
            object_row_only: self.quirks.object_stamp_one_row(),
        }
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

    /// Whether obstruction `o` blocks fighter `asker` in `mode`
    /// (`PHrIgnoreObstruction`).
    pub(super) fn obstruction_blocks(&self, asker: usize, mode: Mode, o: Obstruction) -> bool {
        match o {
            Obstruction::Character(j) => {
                if j == asker {
                    return false;
                }
                if mode == Mode::ClearSpace {
                    return true;
                }
                // (`AI2rTryToPathfindThrough`.)
                let through = match self.fighters[asker].brain.as_ref() {
                    Some(crate::ai::Brain::Agent(a)) => a.paths_through(j, j == 0),
                    Some(b) => b.navigator().is_some_and(|n| n.ignore == Some(j)),
                    None => false,
                };
                !through
            }
            Obstruction::Object(k) => {
                self.placed.list.get(k).is_some_and(|ob| {
                    ob.flags & (oflag::IN_USE | oflag::INVISIBLE) == oflag::IN_USE
                })
            }
            Obstruction::Door(d) => {
                let Some(door) = self.doors.list.get(d) else {
                    return false;
                };
                if mode == Mode::ClearSpace {
                    return true;
                }
                let open = matches!(
                    door.state,
                    crate::doors::State::Opening(_) | crate::doors::State::Open(_)
                );
                // Open (its leaves stand where they swung to) or not to be
                // opened: it blocks; a manual one blocks casual movement.
                open || !door.may_open_either_side() || (mode == Mode::Casual && door.manual)
            }
        }
    }

    /// Room `room`'s stamps as fighter `asker` sees them in `mode`.
    pub(super) fn blocking(
        &mut self,
        room: usize,
        sources: &Sources,
        asker: usize,
        mode: Mode,
    ) -> Option<Blocking> {
        let nav = self.nav.clone()?;
        let stamps = self
            .obstructions
            .room(&nav, room, self.now as u64, sources)?;
        Some(Blocking::new(stamps, |o| {
            self.obstruction_blocks(asker, mode, o)
        }))
    }

    /// What is stamped at world point `p` (in its room, made now if old),
    /// and whether it blocks fighter `asker` planning a path.
    pub fn obstruction_at(&mut self, p: glam::Vec3, asker: usize) -> Option<(Obstruction, bool)> {
        let nav = self.nav.clone()?;
        let room = nav.room_at(p)?;
        let cell = nav.rooms[room].grid.as_ref()?.cell_of(p.x, p.z)?;
        let sources = self.obstruction_sources();
        let stamps = self
            .obstructions
            .room(&nav, room, self.now as u64, &sources)?;
        let o = stamps.at(cell)?;
        Some((o, self.obstruction_blocks(asker, Mode::Directed, o)))
    }

    /// Plans a path for every navigator that needs one, round what blocks
    /// it.
    pub(super) fn plan_paths(&mut self) {
        let Some(nav) = self.nav.clone() else { return };
        let now = self.now as u64;
        let mut sources: Option<Sources> = None;
        for i in 0..self.fighters.len() {
            let from = self.fighters[i].feet();
            let Some(n) = self.fighters[i].brain.as_ref().and_then(|b| b.navigator()) else {
                continue;
            };
            if !n.wants_plan(now) {
                continue;
            }
            let (careful, goal) = (n.careful, n.goal);
            if sources.is_none() {
                sources = Some(self.obstruction_sources());
            }
            let sources = sources.as_ref().expect("just made");
            // Doors it may not open shut their portals to it.
            let links = self.doors.portals(&nav).clone();
            let shut: HashSet<(usize, usize)> = links
                .iter()
                .enumerate()
                .filter(|&(k, _)| !self.doors.list[k].may_open(from, false))
                .flat_map(|(_, ps)| ps.iter().copied())
                .collect();
            let plan = crate::nav::plan::plan(
                &nav,
                from,
                goal,
                &mut |room| self.blocking(room, sources, i, Mode::Directed),
                careful,
                &shut,
            );
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
