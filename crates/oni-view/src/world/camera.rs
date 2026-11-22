//! The game camera (`Oni_Camera.c`; behaviour per `docs/CAMERA.md`): it
//! follows the player (from behind and above, along the aim, eased), or a
//! script moves it — easing to a clip's first frame (`cm_interpolate`),
//! riding a clip (`cm_anim`), circling the player (`cm_orbit`), holding
//! still (`cm_detach`). Following, it makes the walls between it and the
//! player see-through ("jello"; drawn at 20/255 opacity, `CAgJelloAmt`).
//!
//! It updates once a tick, after everyone has moved (`CArUpdate` after the
//! heartbeat).

use std::rc::Rc;

use glam::{Mat4, Vec3};
use oni_geom::objanim::ObjectAnimation;

use super::World;
use crate::quads::flag;

/// `CAgHeight`: the focus's height above the player's feet.
pub const HEIGHT: f32 = 15.0;
/// `CAgDistance`: how far behind it follows.
pub const DISTANCE: f32 = 33.0;
/// `CAgCanter_Weapon` / `CAgCanter_Unarmed`: how much higher than the focus
/// it sits.
pub const CANTER_WEAPON: f32 = 7.0;
pub const CANTER_UNARMED: f32 = 8.0;
/// `CAgJello_Radius`: the sphere swept toward the player for see-through
/// walls.
pub const JELLO_RADIUS: f32 = 12.0;
/// A see-through quad's opacity, of 255 (`CAgJelloAmt`; the renderer's
/// `fs_jello` draws at it, and decals on the quad fade to it).
pub const JELLO_AMT: u32 = 20;
/// `ONcCharacterHeight`.
const CHARACTER_HEIGHT: f32 = 15.0;

/// Where the camera is and which way it looks (`CAtViewData`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub location: Vec3,
    pub view: Vec3,
    pub up: Vec3,
}

impl Default for View {
    fn default() -> Self {
        View {
            location: Vec3::ZERO,
            view: Vec3::Z,
            up: Vec3::Y,
        }
    }
}

/// An up vector for a view direction, level with the world
/// (`CArBuildUpVector`).
pub fn build_up(view: Vec3) -> Vec3 {
    let side = Vec3::new(view.z, 0.0, -view.x);
    view.cross(side).normalize_or(Vec3::Y)
}

impl View {
    /// A clip's matrix as a view: its place, looking along its −Z, up its
    /// Y made level (`CArMatrix_To_ViewData`).
    pub fn from_matrix(m: Mat4) -> View {
        let view = m.transform_vector3(Vec3::NEG_Z);
        View {
            location: m.w_axis.truncate(),
            view,
            up: build_up(view),
        }
    }
}

/// What drives the camera (`CAtCameraMode`).
#[derive(Clone, Debug)]
pub enum Mode {
    /// Following the player: its canter and distance now, and whether it
    /// snaps (`rigid`) instead of easing.
    Follow {
        canter: f32,
        distance: f32,
        rigid: bool,
    },
    /// Riding an object clip, a frame a tick.
    Animate {
        clip: Rc<ObjectAnimation>,
        frame: i32,
    },
    /// Easing from one view to another over `end` ticks.
    Interpolate {
        from: View,
        to: View,
        frame: u32,
        end: u32,
    },
    /// Circling the player: the angle now, the one it stops at (none:
    /// negative), radians a tick.
    Orbit { angle: f32, stop: f32, speed: f32 },
}

/// The game camera (`CAtCamera`).
#[derive(Clone, Debug)]
pub struct GameCamera {
    pub mode: Mode,
    pub view: View,
    /// A script waits on it (`CArIsBusy`).
    pub busy: bool,
    /// See-through walls are on (`cm_jello`).
    pub jello: bool,
    /// Tunables scripts set (`cm_distance`, …).
    pub height: f32,
    pub distance: f32,
    pub canter_weapon: f32,
    pub canter_unarmed: f32,
    /// Its projection: the vertical field of view (`gs_fov_set`) and the
    /// far plane (`gs_farclipplane_set`).
    pub fov: f32,
    pub far: f32,
    /// The screen's width over its height, as last drawn (16:9 until then).
    pub aspect: f32,
    /// Placed behind the player yet (at the level's start).
    pub placed: bool,
}

impl Default for GameCamera {
    fn default() -> Self {
        GameCamera {
            mode: Mode::Follow {
                canter: CANTER_UNARMED,
                distance: DISTANCE,
                rigid: false,
            },
            view: View::default(),
            busy: false,
            jello: true,
            height: HEIGHT,
            distance: DISTANCE,
            canter_weapon: CANTER_WEAPON,
            canter_unarmed: CANTER_UNARMED,
            fov: crate::camera::GAME_FOV_Y,
            far: crate::camera::GAME_FAR,
            aspect: 16.0 / 9.0,
            placed: false,
        }
    }
}

/// How far a followed point moves toward its goal in a tick
/// (`CArFollow_Calculate_Delay`): the gap over a delay that shrinks with
/// the log of the gap's square (so a far camera catches up fast), never
/// under 2.
fn ease(at: Vec3, goal: Vec3, base: f32, distance_scale: f32, scale: f32) -> Vec3 {
    let gap = goal - at;
    let delay =
        (base - scale * 0.5 * (gap.length_squared() * distance_scale).max(1.0).ln()).max(2.0);
    at + gap / delay
}

impl World {
    /// The player's camera direction (`cameraVector` of
    /// `ONiCharacter_DoAiming_Vector`): its aim turned by any look-mode
    /// turn and the share of its clip's turn played so far, raised by the
    /// aim pitch — built, as shipped, as (sin yaw, sin pitch, cos yaw)
    /// normalized (the correct aim vector is kept apart from it, for the
    /// camera's sake).
    fn camera_vector(&self) -> Option<Vec3> {
        let p = self.fighters.first()?;
        let quarter = std::f32::consts::FRAC_PI_2;
        let lr = p.aim.lr.clamp(-quarter, quarter);
        let ud = p.aim.ud.clamp(-quarter, quarter);
        let clip = p.motion.current();
        let mut turn = clip.final_rotation;
        if turn > std::f32::consts::PI {
            turn -= std::f32::consts::TAU;
        }
        if turn != 0.0 {
            turn *= f32::from(p.motion.frame) / f32::from(clip.frames().max(1));
        }
        let yaw = p.facing.aim() + lr + turn;
        Some(Vec3::new(yaw.sin(), ud.sin(), yaw.cos()).normalize())
    }

    /// The follow camera's goal (`CArFollow_Get_GoalPosition`): `distance`
    /// back along the camera direction (looking steeply up, swung toward
    /// straight up), raised by the height and canter, never below the feet.
    fn follow_goal(&self, canter: f32, distance: f32) -> Vec3 {
        let Some(p) = self.fighters.first() else {
            return Vec3::ZERO;
        };
        let at = p.feet();
        let mut v = self.camera_vector().unwrap_or(Vec3::Z);
        let ud = p.aim.ud;
        if ud > 0.9 {
            // (As shipped: multiplied where it means divided.)
            let up = ((ud - 0.9) * (std::f32::consts::FRAC_PI_2 - 0.9)).clamp(0.0, 1.0);
            v = v.lerp(Vec3::Y, up).normalize_or(Vec3::Y);
        }
        let mut goal = at - v * distance;
        goal.y += self.camera.height + canter;
        goal.y = goal.y.max(at.y + 0.5);
        goal
    }

    fn follow_focus(&self) -> Vec3 {
        self.fighters
            .first()
            .map_or(Vec3::ZERO, |p| p.feet() + Vec3::Y * self.camera.height)
    }

    /// Following again, snapped into place (`CArFollow_Enter`, `cm_reset`).
    pub fn camera_follow(&mut self) {
        let armed = self.fighters.first().is_some_and(|p| p.hand.is_some());
        let canter = if armed {
            self.camera.canter_weapon
        } else {
            self.camera.canter_unarmed
        };
        let distance = self.camera.distance;
        let (goal, focus) = (self.follow_goal(canter, distance), self.follow_focus());
        let view = (focus - goal).normalize_or(Vec3::Z);
        self.camera.view = View {
            location: goal,
            view,
            up: build_up(view),
        };
        self.camera.mode = Mode::Follow {
            canter,
            distance,
            rigid: false,
        };
        self.camera.busy = false;
        self.camera.placed = true;
    }

    /// Eases from where it is to `to` over `ticks` (`CArInterpolate_Enter_ViewData`).
    pub fn camera_interpolate(&mut self, to: View, ticks: u32) {
        self.camera.placed = true;
        self.camera.mode = Mode::Interpolate {
            from: self.camera.view,
            to,
            frame: 0,
            end: ticks.max(1),
        };
        self.camera.busy = true;
    }

    /// `cm_interpolate clip ticks`: eases to the clip's first frame. False
    /// if there is no such clip.
    pub fn camera_interpolate_to(&mut self, clip: &str, ticks: i64) -> bool {
        let Some(a) = self.object_anim(clip) else {
            return false;
        };
        let to = View::from_matrix(clip_matrix(&a, 0));
        self.camera_interpolate(to, ticks.max(1) as u32);
        true
    }

    /// `cm_anim … clip`: rides the clip from its start. False if there is
    /// no such clip.
    pub fn camera_animate(&mut self, clip: &str) -> bool {
        let Some(clip) = self.object_anim(clip) else {
            return false;
        };
        self.camera.mode = Mode::Animate { clip, frame: 0 };
        self.camera.busy = true;
        self.camera.placed = true;
        true
    }

    /// `cm_detach`: holds still where it is (an ease to here, over a tick).
    pub fn camera_detach(&mut self) {
        self.camera_interpolate(self.camera.view, 1);
    }

    /// `cm_orbit speed [stop]` (degrees): circles the player.
    pub fn camera_orbit(&mut self, speed: f32, stop: Option<f32>) {
        self.camera.mode = Mode::Orbit {
            angle: 0.0,
            stop: stop.unwrap_or(-1.0).to_radians(),
            speed: speed.to_radians(),
        };
        self.camera.busy = false;
    }

    /// `cm_barabus who away up ticks`: eases to a view of `who` from `away`
    /// back along the line from the camera, `up` higher.
    pub fn camera_barabus(&mut self, who: usize, away: f32, up: f32, ticks: i64) {
        let Some(f) = self.fighters.get(who) else {
            return;
        };
        let target = f.feet() + Vec3::Y * 4.0;
        let from = Vec3::new(
            self.camera.view.location.x,
            target.y,
            self.camera.view.location.z,
        );
        let dir = (target - from).normalize_or(Vec3::Z);
        let location = target - dir * away + Vec3::Y * up;
        let view = (target - location).normalize_or(Vec3::Z);
        self.camera_interpolate(
            View {
                location,
                view,
                up: build_up(view),
            },
            ticks.max(0) as u32,
        );
    }

    /// One tick of the camera (`CArUpdate_Internal`).
    pub(super) fn camera_tick(&mut self) {
        if !self.camera.placed && !self.fighters.is_empty() {
            // (The level's start puts it behind the player: `CArFollow_Enter`.)
            self.camera_follow();
        }
        self.jello_clear();
        match self.camera.mode.clone() {
            Mode::Follow {
                canter,
                distance,
                rigid,
            } => self.follow_tick(canter, distance, rigid),
            Mode::Animate { clip, mut frame } => {
                // (A frame a tick, as objects' clips run; held on its last.)
                frame = (frame + 1).min(i32::from(clip.num_frames) - 1).max(0);
                let m = clip_matrix(&clip, frame);
                let view = m.transform_vector3(Vec3::NEG_Z);
                let up = m.transform_vector3(Vec3::Y);
                self.camera.view = View {
                    location: m.w_axis.truncate(),
                    view,
                    up,
                };
                self.camera.busy = frame < i32::from(clip.num_frames) - 1;
                self.camera.mode = Mode::Animate { clip, frame };
            }
            Mode::Interpolate {
                from,
                to,
                frame,
                end,
            } => {
                let frame = (frame + 1).min(end);
                if frame >= end {
                    self.camera.busy = false;
                }
                // Eased: slow at both ends (half a sine wave).
                let t = ((frame as f32 / end as f32) * std::f32::consts::PI
                    - std::f32::consts::FRAC_PI_2)
                    .sin()
                    * 0.5
                    + 0.5;
                let view = from.view.lerp(to.view, t).normalize_or(to.view);
                self.camera.view = View {
                    location: from.location.lerp(to.location, t),
                    view,
                    up: build_up(view),
                };
                self.camera.mode = Mode::Interpolate {
                    from,
                    to,
                    frame,
                    end,
                };
            }
            Mode::Orbit {
                mut angle,
                stop,
                speed,
            } => {
                let tau = std::f32::consts::TAU;
                let old = angle;
                angle += speed.clamp(-tau, tau);
                if stop >= 0.0 {
                    let crossed = |s: f32| (old <= s && angle >= s) || (old >= s && angle <= s);
                    if crossed(stop) || crossed(stop + tau) || crossed(stop - tau) {
                        angle = stop;
                    }
                }
                angle = angle.rem_euclid(tau);
                if let Some(p) = self.fighters.first() {
                    let yaw = angle + p.aim.lr + p.facing.aim();
                    let mut back = Vec3::new(yaw.sin(), 0.0, yaw.cos()) * self.camera.distance;
                    back.y -= self.camera.canter_unarmed;
                    let mut location = p.feet() - back;
                    location.y += self.camera.height;
                    let view = back.normalize_or(Vec3::Z);
                    self.camera.view = View {
                        location,
                        view,
                        up: build_up(view),
                    };
                }
                self.camera.mode = Mode::Orbit { angle, stop, speed };
            }
        }
    }

    /// Following (`CArFollow_Update`): the canter eases toward the armed or
    /// unarmed one (less looking up, more looking down with a gun), the
    /// distance shortens looking down with a gun, and the camera and its
    /// focus ease toward their goals.
    fn follow_tick(&mut self, mut canter: f32, mut distance: f32, rigid: bool) {
        if let Some(p) = self.fighters.first() {
            let armed = p.hand.is_some();
            let mut want = if armed {
                self.camera.canter_weapon
            } else {
                self.camera.canter_unarmed
            };
            let (full, zero) = (0.5, 1.2);
            let ud = p.aim.ud.min(zero);
            if ud > full {
                // (As shipped: scaled by 1.2 − share, not 1 − share.)
                want *= zero - (ud - full) / (zero - full);
            } else if armed && ud < -full {
                want *= 1.0 + (-ud - full) / (zero - full);
            }
            canter = if canter < want {
                want.min(canter + 0.12)
            } else {
                want.max(canter - 0.12)
            };
            distance = if armed && p.aim.ud <= -0.5 {
                self.camera.distance * (0.5 + (1.0 + p.aim.ud).max(0.0))
            } else {
                self.camera.distance
            };
        }
        let (goal, focus_goal) = (self.follow_goal(canter, distance), self.follow_focus());
        let (location, focus) = if rigid {
            (goal, focus_goal)
        } else {
            let v = self.camera.view;
            let focus = v.location + v.view * distance;
            (
                ease(v.location, goal, 8.0, 1.0, 0.85),
                ease(focus, focus_goal, 8.0, 3.0, 0.85),
            )
        };
        let view = (focus - location).normalize_or(self.camera.view.view);
        self.camera.view = View {
            location,
            view,
            up: build_up(view),
        };
        self.camera.mode = Mode::Follow {
            canter,
            distance,
            rigid,
        };
        if self.camera.jello {
            self.jello_track();
        }
    }

    /// Clears last tick's see-through walls (`M3rGeom_Clear_Jello`).
    fn jello_clear(&mut self) {
        self.see_through.objects.clear();
        self.see_through.doors.clear();
        for q in std::mem::take(&mut self.see_through.quads) {
            if let Some(q) = self.quads.quads.get_mut(q as usize) {
                q.flags &= !flag::JELLO;
            }
        }
    }

    /// The walls between the camera and the player go see-through
    /// (`CAiTrackJello`): a sphere swept from the camera toward a point a
    /// little above and beside the player's head meets them — but not
    /// what is wholly below the feet, nor (the plane test) what the camera
    /// and the player are on the same side of.
    fn jello_track(&mut self) {
        let Some(p) = self.fighters.first() else {
            return;
        };
        let v = self.camera.view;
        let feet = p.feet();
        let mut side = v.up.cross(v.view);
        side.y += 2.0;
        side *= 3.0;
        let focus = feet + Vec3::Y * (CHARACTER_HEIGHT - 3.0);
        let to_focus = focus + side - v.location;
        let sweep = to_focus.normalize_or_zero() * (to_focus.length() - JELLO_RADIUS).max(1.0);
        let lowest = feet.y + 1.5;
        let head = feet + Vec3::Y * CHARACTER_HEIGHT;
        let faithful = self.quirks.jello_plane_test_feet_twice();
        let mut jello = Vec::new();
        for h in self.quads.sphere(
            v.location,
            JELLO_RADIUS,
            sweep,
            flag::INVISIBLE | flag::BROKEN_GLASS,
        ) {
            let q = &self.quads.quads[h.quad as usize];
            if q.max.y < lowest {
                continue;
            }
            // The plane test, on the raw plane.
            let (n, d) = self.quads.plane(q.plane & 0x7FFF_FFFF);
            let above = |p: Vec3| n.dot(p) + d > 0.0;
            let (cam, foot, top) = (above(v.location), above(feet), above(head));
            // (As shipped, the "all on one side" test reads the feet's side
            // twice for the head's — so a wall between the camera and the
            // head, but not the feet, stays solid.)
            let same = if faithful {
                (cam && foot) == (cam || foot || top)
            } else {
                (cam && foot && top) == (cam || foot || top)
            };
            if same {
                continue;
            }
            jello.push(h.quad);
        }
        for &q in &jello {
            self.quads.quads[q as usize].flags |= flag::JELLO;
        }
        self.see_through.quads = jello;
        // The objects the ray from the camera to the same point passes
        // through (each in use, by its sphere): drawn see-through this
        // frame.
        let end = if self.quirks.jello_object_ray_as_point() {
            to_focus
        } else {
            v.location + to_focus
        };
        self.see_through.objects = (0..self.placed.list.len())
            .filter(|&k| {
                let r = self.placed.setups[self.placed.list[k].setup].radius;
                segment_meets_sphere(v.location, end, self.object_matrix(k).w_axis.truncate(), r)
            })
            .collect();
        // (A door not closed: its leaves are objects, met the same way.)
        self.see_through.doors = self.doors.leaves_met(v.location, end);
    }
}

use crate::geom::segment_meets_sphere;

/// What the camera sees through this tick (its jello).
#[derive(Clone, Debug, Default)]
pub struct SeeThrough {
    /// Level quads.
    pub quads: Vec<u32>,
    /// Placed objects (`OBcFlags_JelloObjects`).
    pub objects: Vec<usize>,
    /// Door leaves (object id, leaf): open or moving, their leaves are
    /// objects too.
    pub doors: Vec<(u32, u8)>,
}

impl GameCamera {
    /// The render camera for its view, with its projection.
    pub fn render_camera(&self, aspect: f32) -> crate::camera::Camera {
        let mut c = crate::camera::Camera::game(aspect);
        c.fov_y = self.fov;
        c.far = self.far;
        c.look_to(self.view.location, self.view.view, self.view.up);
        c
    }
}

use crate::geom::clip_matrix;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_is_level_with_the_world() {
        let up = build_up(Vec3::new(1.0, -0.5, 0.0).normalize());
        assert!(up.y > 0.8, "{up}");
        assert!(up.dot(Vec3::new(1.0, -0.5, 0.0).normalize()).abs() < 1e-5);
    }

    #[test]
    fn easing_catches_up_faster_from_far() {
        let near = ease(Vec3::ZERO, Vec3::X * 2.0, 8.0, 1.0, 0.85).x / 2.0;
        let far = ease(Vec3::ZERO, Vec3::X * 200.0, 8.0, 1.0, 0.85).x / 200.0;
        assert!(far > near, "{near} {far}");
        assert!(
            ease(Vec3::ZERO, Vec3::X * 1e6, 8.0, 1.0, 0.85).x <= 5e5 + 1.0,
            "never more than half the gap"
        );
    }
}
