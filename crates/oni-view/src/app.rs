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
    yaw: f32,
    foot_offset: f32,
    height: f32,
    accum: f32,
}

const TICK: f32 = 1.0 / 60.0;
const TURN_SPEED: f32 = 0.05; // radians per tick

struct State {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    cam: Camera,
    window: Arc<Window>,
    keys: HashSet<KeyCode>,
    looking: bool,
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
                Some(PlayState {
                    physics: crate::physics::Physics::new(&verts, &idx, feet, height, radius),
                    loco,
                    yaw: 0.0,
                    foot_offset,
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
            if st.looking {
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
        let input = crate::anim_state::Input {
            forward: self.keys.contains(&KeyCode::KeyW),
            backward: self.keys.contains(&KeyCode::KeyS),
        };
        let turn = (self.keys.contains(&KeyCode::KeyD) as i32
            - self.keys.contains(&KeyCode::KeyA) as i32) as f32;

        let Some(play) = self.play.as_mut() else {
            return;
        };
        play.accum += dt;
        while play.accum >= TICK {
            play.accum -= TICK;
            play.yaw += turn * TURN_SPEED;
            // The animation state machine chooses the clip and yields this
            // frame's character-local root motion; facing rotates it to world.
            let local = play.loco.tick(input);
            let world_move = Quat::from_rotation_y(play.yaw) * Vec3::new(local.x, 0.0, local.y);
            play.physics.step(world_move);
        }

        let (feet, yaw, foot_offset, height) =
            (play.physics.feet, play.yaw, play.foot_offset, play.height);
        if let Some(cv) = self.charview.as_mut() {
            let origin = Vec3::new(feet.x, feet.y - foot_offset, feet.z);
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
            (Some(cv), Some(play)) => cv.bones_with(play.loco.clip(), play.loco.frame_time()),
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
