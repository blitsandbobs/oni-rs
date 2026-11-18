//! Windowed free-fly viewer (winit 0.30 `ApplicationHandler`).
//!
//! Controls: WASD to move, Q/E down/up, hold left mouse to look, Shift to go
//! faster, Esc to quit.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use glam::{Mat4, Quat, Vec3};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::camera::Camera;
use crate::gltf::Scene;
use crate::render::{Gpu, Renderer};

/// Play-mode character controller state (E3.1): the character is driven by the
/// 60 Hz physics + its animation root motion, with a third-person follow camera.
struct PlayState {
    physics: crate::physics::Physics,
    loco: crate::anim_state::Locomotion,
    facing: crate::control::Facing,
    /// Aim pitch (radians), accumulated from mouse-Y; clamped to ±π/2.
    aiming_ud: f32,
    /// Mouse delta accumulated since the last tick (applied once per frame).
    mouse_dx: f32,
    mouse_dy: f32,
    height: f32,
    accum: f32,
}

const TICK: f32 = 1.0 / 60.0;
use crate::control::{MED_TURN, SLOW_TURN, TURN_DEADZONE};
/// Mouse sensitivity: radians of desired-facing per pixel of motion (tuned to
/// feel like Oni's `/64` base; rebindable scale lives in `oni_config`).
const MOUSE_SENS: f32 = 0.0022;

struct State {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    cam: Camera,
    window: Arc<Window>,
    keys: HashSet<KeyCode>,
    looking: bool,
    /// Player key/mouse bindings (Oni defaults; rebindable later).
    bindings: oni_config::Bindings,
    tuning: oni_config::InputTuning,
    move_speed: f32,
    last: Instant,
    start: Instant,
    charview: Option<crate::CharView>,
    play: Option<PlayState>,
}

pub struct App {
    scene: Option<Scene>,
    charview: Option<crate::CharView>,
    particles: Vec<crate::particles::Particle>,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    play: bool,
    loco: Option<crate::anim_state::Locomotion>,
    state: Option<State>,
}

/// Runs the windowed viewer (consumes the scene, optional character, particles).
/// `play` enables the third-person character controller (needs a character).
pub fn run(
    scene: Scene,
    charview: Option<crate::CharView>,
    particles: Vec<crate::particles::Particle>,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    play: bool,
    loco: Option<crate::anim_state::Locomotion>,
) {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        scene: Some(scene),
        charview,
        particles,
        eye,
        target,
        play,
        loco,
        state: None,
    };
    event_loop.run_app(&mut app).expect("run");
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let scene = self.scene.take().expect("scene");
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("oni-view — level"))
                .expect("window"),
        );

        let gpu = pollster::block_on(Gpu::new(None));
        eprintln!("GPU: {}", gpu.adapter.get_info().name);
        let surface = gpu
            .instance
            .create_surface(window.clone())
            .expect("surface");

        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let sz = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: sz.width.max(1),
            height: sz.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);

        let charview = self.charview.take();
        let renderer = Renderer::new(
            &gpu,
            &scene,
            charview.as_ref().map(|c| &c.character),
            &self.particles,
            format,
        );

        // Build the play-mode controller (level trimesh + character capsule +
        // the locomotion state machine).
        let play = match (self.play, charview.as_ref(), self.loco.take()) {
            (true, Some(cv), Some(loco)) => {
                let (verts, idx) = crate::level_collision_mesh(&scene);
                let (mn, mx) = cv.aabb(0.0);
                let origin = cv.placement.transform_point3(Vec3::ZERO);
                let foot_offset = mn.y - origin.y;
                let height = (mx.y - mn.y).max(2.0);
                let radius = ((mx.x - mn.x).max(mx.z - mn.z) * 0.5).clamp(0.5, 3.0);
                let feet = Vec3::new(origin.x, origin.y + foot_offset, origin.z);
                // Capture the cursor so mouse motion turns the character
                // continuously (Oni grabs the mouse in gameplay).
                let _ = window
                    .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                    .or_else(|_| window.set_cursor_grab(winit::window::CursorGrabMode::Confined));
                window.set_cursor_visible(false);
                Some(PlayState {
                    physics: crate::physics::Physics::new(&verts, &idx, feet, height, radius),
                    loco,
                    facing: crate::control::Facing::new(0.0),
                    aiming_ud: 0.0,
                    mouse_dx: 0.0,
                    mouse_dy: 0.0,
                    height,
                    accum: 0.0,
                })
            }
            _ => None,
        };

        let move_speed = scene.extent() * 0.4;
        let aspect = config.width as f32 / config.height as f32;
        let mut cam = match &charview {
            Some(cv) => crate::frame_character(cv, aspect, 0.0),
            None => crate::default_camera(&scene, aspect),
        };
        match (self.eye, self.target) {
            (Some(e), Some(t)) => cam.look_at(e, t),
            (Some(e), None) => cam.look_at(e, scene.center()),
            _ => {}
        }

        let now = Instant::now();
        self.state = Some(State {
            gpu,
            surface,
            config,
            renderer,
            cam,
            window,
            keys: HashSet::new(),
            looking: false,
            bindings: oni_config::Bindings::oni_defaults(),
            tuning: oni_config::InputTuning::default(),
            move_speed,
            last: now,
            start: now,
            charview,
            play,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(st) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new) => {
                st.config.width = new.width.max(1);
                st.config.height = new.height.max(1);
                st.surface.configure(&st.gpu.device, &st.config);
                st.cam.aspect = st.config.width as f32 / st.config.height as f32;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape {
                        event_loop.exit();
                    }
                    match event.state {
                        ElementState::Pressed => {
                            st.keys.insert(code);
                        }
                        ElementState::Released => {
                            st.keys.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                st.looking = state == ElementState::Pressed;
            }
            WindowEvent::RedrawRequested => {
                st.update();
                st.render();
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (Some(st), DeviceEvent::MouseMotion { delta }) = (self.state.as_mut(), event) {
            if let Some(play) = st.play.as_mut() {
                // Play mode: the mouse turns the character (accumulated, applied
                // per tick) — no button to hold.
                play.mouse_dx += delta.0 as f32;
                play.mouse_dy += delta.1 as f32;
            } else if st.looking {
                let s = 0.0025;
                st.cam.yaw += delta.0 as f32 * s;
                st.cam.pitch = (st.cam.pitch - delta.1 as f32 * s).clamp(-1.54, 1.54);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(st) = self.state.as_ref() {
            st.window.request_redraw();
        }
    }
}

impl State {
    fn update(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;

        // Play mode: drive the character at a fixed 60 Hz.
        if self.play.is_some() {
            self.update_play(dt);
            return;
        }

        let fast =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        let speed = self.move_speed * dt * if fast { 4.0 } else { 1.0 };
        let fwd = self.cam.forward();
        let right = self.cam.right();
        let mut mv = Vec3::ZERO;
        if self.keys.contains(&KeyCode::KeyW) {
            mv += fwd;
        }
        if self.keys.contains(&KeyCode::KeyS) {
            mv -= fwd;
        }
        if self.keys.contains(&KeyCode::KeyD) {
            mv += right;
        }
        if self.keys.contains(&KeyCode::KeyA) {
            mv -= right;
        }
        if self.keys.contains(&KeyCode::KeyE) {
            mv += Vec3::Y;
        }
        if self.keys.contains(&KeyCode::KeyQ) {
            mv -= Vec3::Y;
        }
        if mv != Vec3::ZERO {
            self.cam.eye += mv.normalize() * speed;
        }
    }

    /// Play-mode character controller: W/S move (root motion), A/D turn; the
    /// camera follows third-person. Stepped at a fixed 60 Hz.
    fn update_play(&mut self, dt: f32) {
        // Resolve held keys → actions through the bindings (Oni defaults: WASD =
        // forward/back + strafe, Caps Lock = walk; rebindable later).
        use oni_config::Action;
        let held = |a: Action| {
            self.keys
                .iter()
                .filter_map(|&k| to_config_key(k))
                .any(|k| self.bindings.key_action(k) == Some(a))
        };
        let walk = held(Action::Walk);
        let mut input = crate::anim_state::Input {
            forward: held(Action::Forward),
            backward: held(Action::Backward),
            step_left: held(Action::StepLeft),
            step_right: held(Action::StepRight),
            walk,
            turn_left: false,
            turn_right: false,
        };
        let moving = input.forward || input.backward || input.step_left || input.step_right;
        let turn_cap = if walk { SLOW_TURN } else { MED_TURN };
        let tuning = self.tuning;

        // Step the simulation (character immutably borrowed for stitch capture,
        // play state mutably — disjoint fields).
        {
            let Some(cv) = self.charview.as_ref() else {
                return;
            };
            let Some(play) = self.play.as_mut() else {
                return;
            };
            // Apply this frame's accumulated mouse motion to the facing target
            // (Oni: mouse-X → desiredFacing, negated; clamped ±π) and aim pitch
            // (mouse-Y → aimingUD, clamped ±π/2).
            let sens = MOUSE_SENS * tuning.mouse_sensitivity;
            let dx = std::mem::take(&mut play.mouse_dx);
            let dy = std::mem::take(&mut play.mouse_dy);
            let dy = if tuning.invert_y { -dy } else { dy };
            play.facing
                .adjust_desired((-dx * sens).clamp(-std::f32::consts::PI, std::f32::consts::PI));
            play.aiming_ud = (play.aiming_ud + dy * sens)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);

            play.accum += dt;
            while play.accum >= TICK {
                play.accum -= TICK;
                // Turn-in-place intent: a large pending body turn while standing
                // (Oni's HandleStandingTurn, evaluated before the slew).
                let offset = play.facing.desired_offset();
                input.turn_left = !moving && offset > TURN_DEADZONE;
                input.turn_right = !moving && offset < -TURN_DEADZONE;
                // The animation state machine chooses the clip and yields this
                // frame's character-local root motion; facing rotates it to world.
                let local = play.loco.tick(input, Some(&cv.character));
                let world_move = Quat::from_rotation_y(play.facing.facing)
                    * Vec3::new(local.x, 0.0, local.y);
                play.physics.step(world_move);
                // Then slew the body yaw toward the mouse target (Oni runs
                // HandleTurning at the end of the heartbeat).
                play.facing.turn_toward(turn_cap);
            }
        }

        let Some(play) = self.play.as_ref() else {
            return;
        };
        let (feet, yaw, height, pelvis) =
            (play.physics.feet, play.facing.facing, play.height, play.loco.height());
        if let Some(cv) = self.charview.as_mut() {
            // Oni places the character base matrix at `location` (the ground
            // point) then lifts it by `heightThisFrame`; the model origin is the
            // pelvis, so pelvis world Y = feet + height-this-frame. This carries
            // the body's vertical bob (and the per-clip resting elevation).
            let origin = Vec3::new(feet.x, feet.y + pelvis, feet.z);
            cv.placement = Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), origin);
        }
        // Third-person follow camera (behind + above, looking at the torso).
        let center = feet + Vec3::new(0.0, height * 0.55, 0.0);
        let fwd = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let eye = center - fwd * (height * 2.2) + Vec3::new(0.0, height * 0.4, 0.0);
        self.cam.look_at(eye, center);
    }

    fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                self.surface.configure(&self.gpu.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // Pose the character: at the play-mode animation time, or looping over
        // elapsed time in the free-fly viewer.
        let bones = match (&self.charview, &self.play) {
            (Some(cv), Some(play)) => {
                cv.bones_with(play.loco.clip(), play.loco.frame_time(), play.loco.blend())
            }
            (Some(cv), None) => {
                let dur = cv.clip.as_ref().map(|c| c.duration).unwrap_or(1.0);
                let t = self.start.elapsed().as_secs_f32() % dur.max(0.001);
                cv.bones(t)
            }
            _ => Vec::new(),
        };
        let (cam_right, cam_up) = self.cam.billboard_basis();
        self.renderer.render(
            &self.gpu,
            &view,
            self.config.width,
            self.config.height,
            self.cam.view_proj(),
            &bones,
            cam_right,
            cam_up,
        );
        frame.present();
    }
}

/// Maps a `winit` physical key to the portable `oni_config::Key` the bindings
/// are keyed on (variant names mirror `winit`'s). Unmapped keys return `None`.
fn to_config_key(code: KeyCode) -> Option<oni_config::Key> {
    use oni_config::Key as K;
    Some(match code {
        KeyCode::KeyA => K::KeyA,
        KeyCode::KeyB => K::KeyB,
        KeyCode::KeyC => K::KeyC,
        KeyCode::KeyD => K::KeyD,
        KeyCode::KeyE => K::KeyE,
        KeyCode::KeyF => K::KeyF,
        KeyCode::KeyG => K::KeyG,
        KeyCode::KeyH => K::KeyH,
        KeyCode::KeyI => K::KeyI,
        KeyCode::KeyJ => K::KeyJ,
        KeyCode::KeyK => K::KeyK,
        KeyCode::KeyL => K::KeyL,
        KeyCode::KeyM => K::KeyM,
        KeyCode::KeyN => K::KeyN,
        KeyCode::KeyO => K::KeyO,
        KeyCode::KeyP => K::KeyP,
        KeyCode::KeyQ => K::KeyQ,
        KeyCode::KeyR => K::KeyR,
        KeyCode::KeyS => K::KeyS,
        KeyCode::KeyT => K::KeyT,
        KeyCode::KeyU => K::KeyU,
        KeyCode::KeyV => K::KeyV,
        KeyCode::KeyW => K::KeyW,
        KeyCode::KeyX => K::KeyX,
        KeyCode::KeyY => K::KeyY,
        KeyCode::KeyZ => K::KeyZ,
        KeyCode::Space => K::Space,
        KeyCode::Enter => K::Enter,
        KeyCode::Escape => K::Escape,
        KeyCode::Tab => K::Tab,
        KeyCode::CapsLock => K::CapsLock,
        KeyCode::ShiftLeft => K::ShiftLeft,
        KeyCode::ShiftRight => K::ShiftRight,
        KeyCode::ControlLeft => K::ControlLeft,
        KeyCode::ControlRight => K::ControlRight,
        KeyCode::AltLeft => K::AltLeft,
        KeyCode::AltRight => K::AltRight,
        KeyCode::F1 => K::F1,
        _ => return None,
    })
}
