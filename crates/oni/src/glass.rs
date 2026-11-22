//! The level's breakable glass panes: solid and drawn until broken (by a
//! shot, a blast, or a script hiding their level part), then neither.

use std::collections::HashSet;

use glam::Vec3;

use crate::quads::{flag, Quads};

/// A breakable glass pane.
pub struct Pane {
    /// Its level quad.
    pub quad: u32,
    /// The script reference of the level part it belongs to.
    pub env_ref: Option<u32>,
    pub broken: bool,
    /// Its middle and how far its corners reach from it.
    pub centre: Vec3,
    pub radius: f32,
}

/// The level's panes, and the broken ones by quad (not drawn).
#[derive(Default)]
pub struct Glass {
    pub panes: Vec<Pane>,
    pub broken: HashSet<u32>,
}

impl Glass {
    /// The level's panes (`crate::glass_parts`: quad, part reference,
    /// geometry), solid until broken.
    pub fn new(panes: Vec<(u32, Option<u32>, crate::Mesh)>) -> Glass {
        let panes = panes
            .into_iter()
            .map(|(quad, env_ref, mesh)| {
                let n = mesh.0.len().max(1) as f32;
                let centre = mesh.0.iter().fold(Vec3::ZERO, |a, v| a + Vec3::from(*v)) / n;
                let radius = mesh
                    .0
                    .iter()
                    .map(|v| Vec3::from(*v).distance(centre))
                    .fold(0.0, f32::max);
                Pane {
                    quad,
                    env_ref,
                    broken: false,
                    centre,
                    radius,
                }
            })
            .collect();
        Glass {
            panes,
            broken: HashSet::new(),
        }
    }

    /// Flags the panes' quads breakable (`AKiComputeBreakableFlags`: by
    /// their material), the broken ones broken — once the quads are loaded.
    pub fn mark_quads(&self, quads: &mut Quads) {
        for p in &self.panes {
            if let Some(q) = quads.quads.get_mut(p.quad as usize) {
                q.flags |= flag::BREAKABLE;
                if p.broken {
                    q.flags |= flag::BROKEN_GLASS;
                }
            }
        }
    }

    /// Breaks (or, `broken` false, mends) pane `k`: not drawn, not solid.
    /// Whether it changed.
    pub fn set(&mut self, k: usize, broken: bool, quads: &mut Quads) -> bool {
        let Some(p) = self.panes.get_mut(k) else {
            return false;
        };
        if p.broken == broken {
            return false;
        }
        p.broken = broken;
        if broken {
            self.broken.insert(p.quad);
        } else {
            self.broken.remove(&p.quad);
        }
        if let Some(q) = quads.quads.get_mut(p.quad as usize) {
            q.flags = if broken {
                q.flags | flag::BROKEN_GLASS
            } else {
                q.flags & !flag::BROKEN_GLASS
            };
        }
        true
    }

    /// Breaks (`on` false) or mends the panes of level part `r` (hiding
    /// glass is breaking it, as in the original).
    pub fn show_part(&mut self, r: u32, on: bool, quads: &mut Quads) {
        for k in 0..self.panes.len() {
            if self.panes[k].env_ref == Some(r) {
                self.set(k, !on, quads);
            }
        }
    }

    /// How many of the script references `from..=to` have no unbroken
    /// pane left (`env_broken`; a reference with no glass counts as broken).
    pub fn env_broken(&self, from: u32, to: u32) -> u32 {
        let intact: HashSet<u32> = self
            .panes
            .iter()
            .filter(|p| !p.broken)
            .filter_map(|p| p.env_ref)
            .filter(|r| (from..=to).contains(r))
            .collect();
        (to.saturating_sub(from) + 1).saturating_sub(intact.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(env_ref: u32) -> (u32, Option<u32>, crate::Mesh) {
        (0, Some(env_ref), (vec![[0.0; 3], [2.0, 0.0, 0.0]], vec![]))
    }

    #[test]
    fn broken_panes_count_by_part_and_mend() {
        let mut g = Glass::new(vec![pane(5), pane(6)]);
        let mut quads = Quads::default();
        assert_eq!(g.env_broken(5, 7), 1, "7 has no glass: counts as broken");
        assert!(g.set(0, true, &mut quads));
        assert!(!g.set(0, true, &mut quads), "already broken");
        assert_eq!(g.env_broken(5, 7), 2);
        g.show_part(5, true, &mut quads);
        assert_eq!(g.env_broken(5, 7), 1, "shown again: mended");
        assert_eq!(g.panes[0].radius, 1.0);
    }
}
