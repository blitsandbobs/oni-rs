//! Windowed free-fly viewer (winit 0.30 `ApplicationHandler`).
//!
//! Controls: WASD to move, Q/E down/up, hold left mouse to look, Shift to go
//! faster, Esc to quit.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use glam::Vec3;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::camera::Camera;
use crate::gltf::Scene;
use crate::render::{Gpu, Renderer};

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
}

pub struct App {
    scene: Option<Scene>,
    charview: Option<crate::CharView>,
    particles: Vec<crate::particles::Particle>,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    state: Option<State>,
}

/// Runs the windowed viewer (consumes the scene, optional character, particles).
pub fn run(
    scene: Scene,
    charview: Option<crate::CharView>,
    particles: Vec<crate::particles::Particle>,
    eye: Option<Vec3>,
    target: Option<Vec3>,
) {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        scene: Some(scene),
        charview,
        particles,
        eye,
        target,
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
        // Loop the character's animation over elapsed time.
        let bones = match &self.charview {
            Some(cv) => {
                let dur = cv.clip.as_ref().map(|c| c.duration).unwrap_or(1.0);
                let t = self.start.elapsed().as_secs_f32() % dur.max(0.001);
                cv.bones(t)
            }
            None => Vec::new(),
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
