//! Lens flares (`docs/PARTICLES.md`, *Lens flares*): particle classes drawn
//! over the scene, without the depth test, where their point is seen —
//! the doors' lock lights, emergency lights, lamps. Each tick each flare's
//! point is tested from the camera (`MSrGeomContext_Method_PointVisible`,
//! the shipped software path): from 5 in front of the camera to the point
//! pulled back toward it by the class's tolerance, it must meet no level
//! quad the player can't see through and no character. A flare with lens
//! frames fades in and out over them as it is seen or hidden
//! (`P3rLensFlare_Fade`); one without just shows or not.
//!
//! Before the ray, the original asks whether the point's octree leaf was
//! seen (`AKrEnvironment_PointIsVisible`: a leaf the camera's visibility
//! rays crossed lately) — off the screen, it isn't, and the flare fades
//! out. Here: within the view's frustum, widened by a leaf's side
//! (`AKcMinOTDim`, 16), and short of the far plane. (Without that cut every
//! flare cast a ray across the level every tick: most of a tick's time.)

use glam::Vec3;

use super::World;

/// An octree leaf's side at its smallest (`AKcMinOTDim`).
const LEAF: f32 = 16.0;
use crate::p3::Handle;

/// A flare's fade (`P3cLensFrames_*`): how it stands, and the frames into
/// a fade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LensState {
    #[default]
    Unset,
    Visible,
    Invisible,
    FadeIn(u16),
    FadeOut(u16),
}

impl LensState {
    /// One step of `ticks` (`P3rLensFlare_Fade`): the share of its
    /// opacity shown.
    pub fn step(&mut self, visible: bool, ticks: u16, fade_in: u16, fade_out: u16) -> f32 {
        let (fi, fo) = (fade_in.max(1), fade_out.max(1));
        if visible {
            let frames = match *self {
                LensState::Unset | LensState::Visible => {
                    *self = LensState::Visible;
                    return 1.0;
                }
                LensState::Invisible => 0,
                LensState::FadeIn(f) => f,
                // (As far into fading in as it was from fading out.)
                LensState::FadeOut(f) => fi - (f * fi) / fo,
            } + ticks;
            if frames >= fi {
                *self = LensState::Visible;
                1.0
            } else {
                *self = LensState::FadeIn(frames);
                f32::from(frames) / f32::from(fi)
            }
        } else {
            let frames = match *self {
                LensState::Unset | LensState::Invisible => {
                    *self = LensState::Invisible;
                    return 0.0;
                }
                LensState::Visible => 0,
                LensState::FadeOut(f) => f,
                LensState::FadeIn(f) => fo - (f * fo) / fi,
            } + ticks;
            if frames >= fo {
                *self = LensState::Invisible;
                0.0
            } else {
                *self = LensState::FadeOut(frames);
                f32::from(fo - frames) / f32::from(fo)
            }
        }
    }
}

/// A flare to test: its particle, point, tolerance and fade frames.
type Flare = (Handle, Vec3, f32, Option<(u16, u16)>);

/// The test starts this far in front of the camera (its near plane, at
/// least 5).
const NEAR: f32 = 5.0;

impl World {
    /// Whether a lens flare's point is seen from the camera, `tolerance`
    /// in front of it counting as seen.
    /// Whether a point `to` from the camera is on the screen, give or take
    /// an octree leaf (see the module notes).
    fn in_view(&self, to: Vec3) -> bool {
        let (v, cam) = (self.camera.view, &self.camera);
        let ahead = to.dot(v.view);
        if ahead <= NEAR || ahead > cam.far + LEAF {
            return false;
        }
        let right = v.view.cross(v.up).normalize_or_zero();
        let up = right.cross(v.view);
        let tan_y = (cam.fov * 0.5).tan();
        // A leaf's side, as a slack at right angles to the edge planes.
        let slack = |tan: f32| LEAF * (1.0 + tan * tan).sqrt();
        to.dot(up).abs() <= ahead * tan_y + slack(tan_y) && to.dot(right).abs() <= ahead * tan_y * cam.aspect + slack(tan_y * cam.aspect)
    }

    fn lens_visible(&self, at: Vec3, tolerance: f32, bodies: &[(usize, Vec<glam::Mat4>)]) -> bool {
        let v = self.camera.view;
        let to = at - v.location;
        if !self.in_view(to) {
            return false;
        }
        let from = v.location + v.view * NEAR;
        let end = at - v.view * tolerance;
        if !self.quads.clear(from, end, crate::quads::flag::LOS_SEE_PLAYER) {
            return false;
        }
        let ray = end - from;
        !bodies.iter().any(|(k, bones)| super::weapons::ray_body(&self.fighters[*k], bones, from, ray).is_some())
    }

    /// One tick of every lens flare's fade (after the camera has moved).
    pub(super) fn lens_tick(&mut self) {
        let sys = &self.armory.particles;
        let flares: Vec<Flare> = sys
            .iter()
            .filter_map(|(h, p)| {
                let c = sys.class(p.class);
                let lens = c.lens.as_ref()?;
                let tol = match &lens.dist {
                    crate::p3::class::Value::Float(x) => *x,
                    crate::p3::class::Value::Range(a, b) => (a + b) * 0.5,
                    _ => 0.0,
                };
                Some((h, sys.world_position(p), tol, lens.fade))
            })
            .collect();
        if flares.is_empty() {
            self.lens.clear();
            return;
        }
        // The characters' bodies, near the camera's line, once.
        let bodies: Vec<(usize, Vec<glam::Mat4>)> = (0..self.fighters.len()).filter(|&k| !self.fighters[k].gone).map(|k| (k, self.fighters[k].bones())).collect();
        let mut next = std::collections::HashMap::with_capacity(flares.len());
        for (h, at, tol, fade) in flares {
            let seen = self.lens_visible(at, tol, &bodies);
            let mut state = self.lens.get(&h).map_or(LensState::Unset, |s| s.0);
            let alpha = match fade {
                Some((fi, fo)) => state.step(seen, 1, fi, fo),
                None => f32::from(u8::from(seen)),
            };
            next.insert(h, (state, alpha));
        }
        self.lens = next;
    }

    /// A lens flare's opacity share now (None: not a flare).
    pub fn lens_alpha(&self, h: Handle) -> Option<f32> {
        self.lens.get(&h).map(|s| s.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flare_fades_in_and_out() {
        let mut s = LensState::Unset;
        assert_eq!(s.step(true, 1, 10, 10), 1.0, "seen from the start: at once");
        assert!((s.step(false, 1, 10, 10) - 0.9).abs() < 1e-6, "hidden: fading out");
        for _ in 0..20 {
            s.step(false, 1, 10, 10);
        }
        assert_eq!(s, LensState::Invisible);
        assert!((s.step(true, 1, 5, 10) - 0.2).abs() < 1e-6, "seen again: fading in");
    }
}
