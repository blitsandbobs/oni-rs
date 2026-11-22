//! Aiming screens (`docs/WEAPONS.md`, *Aim poses*): a class's grids of
//! upper-body poses — a clip whose frames are yaw × pitch steps away from
//! straight ahead — which bend a character toward where it aims, layered on
//! whatever the body plays.

use std::path::Path;
use std::rc::Rc;

use glam::Quat;

use crate::skel::{Character, Clip, Layer, FPS};

/// One screen: its pose grid and how it is laid out.
pub struct Screen {
    pub clip: Rc<Clip>,
    /// Radians between frames, and frames, to each side: `[negative, positive]`.
    pub yaw: ([f32; 2], [u16; 2]),
    pub pitch: ([f32; 2], [u16; 2]),
}

impl Screen {
    /// The animation type it serves (its clip's), and the variant it needs.
    pub fn kind(&self) -> u16 {
        self.clip.anim_type
    }
    pub fn varient(&self) -> u16 {
        self.clip.varient
    }
}

/// Loads a class's screens (`classes.json` `aiming`) from `anim_dir`.
pub fn load(list: &[oni_schema::files::AimingScreen], anim_dir: &Path) -> Vec<Screen> {
    let side = |j: &oni_schema::files::AimSteps| {
        (
            [j.negative_delta, j.positive_delta],
            [
                j.negative_frames.max(0) as u16,
                j.positive_frames.max(0) as u16,
            ],
        )
    };
    list.iter()
        .filter_map(|s| {
            let clip = crate::skel::load_clip(&anim_dir.join(format!("{}.glb", s.clip))).ok()?;
            Some(Screen {
                clip: Rc::new(clip),
                yaw: side(&s.yaw),
                pitch: side(&s.pitch),
            })
        })
        .collect()
}

/// Animation types the screen search walks through (`TRtAnimType`).
mod ty {
    pub const ANYTHING: u16 = 1;
    pub const STAND: u16 = 6;
}

/// The screen for a body clip of `aiming_type` and variant `varient`: the
/// clip's own aiming type, then standing, then any — at each step the screen
/// whose variant the clip has, most specific first.
pub fn find(screens: &[Screen], aiming_type: u16, varient: u16) -> Option<&Screen> {
    if aiming_type == 0 {
        return None;
    }
    for kind in [aiming_type, ty::STAND, ty::ANYTHING] {
        let best = screens
            .iter()
            .filter(|s| s.kind() == kind && s.varient() & !varient == 0)
            .max_by_key(|s| s.varient().count_ones());
        if best.is_some() {
            return best;
        }
    }
    None
}

/// Where along one axis an angle falls: the frame step below it (−negative
/// frames … positive frames) and how far toward the next.
fn step(d: f32, deltas: [f32; 2], frames: [u16; 2]) -> (i32, f32) {
    if d < 0.0 {
        let x = -d / deltas[0].max(1e-6);
        let n = f32::from(frames[0]);
        if x >= n {
            return (-i32::from(frames[0]), 0.0);
        }
        // Between −ceil(x) and −ceil(x) + 1.
        let lo = x.ceil();
        (-(lo as i32), lo - x)
    } else {
        let x = d / deltas[1].max(1e-6);
        let n = f32::from(frames[1]);
        if x >= n {
            // (At the edge: the last frame.)
            return ((n as i32 - 1).max(0), if n > 0.0 { 1.0 } else { 0.0 });
        }
        (x.floor() as i32, x.fract())
    }
}

/// Two screens' layers blended, `t` of the way from `a` to `b`
/// (`TRrAimingScreen_Apply_Lerp`): a part both turn, from one turn to the
/// other; a part only one turns, from or to no turn; none of either, nothing.
pub fn blend(a: Option<Layer>, b: Option<Layer>, t: f32) -> Option<Layer> {
    let (used_a, used_b) = (
        a.as_ref().map_or(0, |l| l.used),
        b.as_ref().map_or(0, |l| l.used),
    );
    let used = used_a | used_b;
    if used == 0 {
        return None;
    }
    let n = a.as_ref().or(b.as_ref()).map_or(0, |l| l.rotations.len());
    let turn = |l: &Option<Layer>, bits: u32, i: usize| {
        l.as_ref()
            .filter(|_| bits & (1 << i) != 0)
            .and_then(|l| l.rotations.get(i).copied())
            .unwrap_or(Quat::IDENTITY)
    };
    let rotations = (0..n)
        .map(|i| {
            let (x, y) = (turn(&a, used_a, i), turn(&b, used_b, i));
            let y = if x.dot(y) < 0.0 { -y } else { y };
            x.lerp(y, t).normalize()
        })
        .collect();
    Some(Layer {
        rotations,
        used,
        replace: 0,
        weight: 1.0,
    })
}

/// The screen's layer for an aim `yaw` / `pitch` away from straight ahead
/// (radians; up positive): the four frames around it blended, each part's
/// rotation a turn to put on top of the body's.
pub fn layer(ch: &Character, s: &Screen, yaw: f32, pitch: f32) -> Layer {
    // (The grid runs the other way from the aim — `TRrAimingScreen_Prepare`
    // negates both.)
    let (yi, yf) = step(-yaw, s.yaw.0, s.yaw.1);
    let (pi, pf) = step(-pitch, s.pitch.0, s.pitch.1);
    let width = i32::from(s.yaw.1[0] + s.yaw.1[1]) + 1;
    let height = i32::from(s.pitch.1[0] + s.pitch.1[1]) + 1;
    let frame = |y: i32, p: i32| -> Vec<Quat> {
        let col = (i32::from(s.yaw.1[0]) + y).clamp(0, width - 1);
        let row = (i32::from(s.pitch.1[0]) + p).clamp(0, height - 1);
        ch.local_rotations(Some(&s.clip), (row * width + col) as f32 / FPS)
    };
    let (a, b) = (frame(yi, pi), frame(yi + 1, pi));
    let (c, d) = (frame(yi, pi + 1), frame(yi + 1, pi + 1));
    let lerp = |x: Quat, y: Quat, t: f32| {
        let y = if x.dot(y) < 0.0 { -y } else { y };
        x.lerp(y, t).normalize()
    };
    let rotations = (0..a.len())
        .map(|i| lerp(lerp(a[i], b[i], yf), lerp(c[i], d[i], yf), pf))
        .collect();
    Layer {
        rotations,
        used: s.clip.used_parts,
        replace: 0,
        weight: 1.0,
    }
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    fn layer(used: u32, angle: f32) -> Layer {
        Layer {
            rotations: vec![Quat::from_rotation_y(angle); 3],
            used,
            replace: 0,
            weight: 1.0,
        }
    }

    #[test]
    fn blends_from_the_old_screen_to_the_new() {
        // Part 0 turned by both, part 1 by the old only, part 2 by the new only.
        let old = layer(0b011, 0.4);
        let new = layer(0b101, 0.8);
        let half = blend(Some(old), Some(new), 0.5).unwrap();
        assert_eq!(half.used, 0b111);
        let angle = |q: Quat| q.to_axis_angle().1;
        assert!(
            (angle(half.rotations[0]) - 0.6).abs() < 0.01,
            "between the two"
        );
        assert!(
            (angle(half.rotations[1]) - 0.2).abs() < 0.01,
            "the old's, half gone"
        );
        assert!(
            (angle(half.rotations[2]) - 0.4).abs() < 0.01,
            "the new's, half come"
        );
        // From no screen: the new one's turn coming in.
        let from_none = blend(None, Some(layer(0b001, 0.8)), 0.25).unwrap();
        assert!((angle(from_none.rotations[0]) - 0.2).abs() < 0.01);
        assert!(blend(None, None, 0.5).is_none());
    }
}
