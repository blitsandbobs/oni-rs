//! Windowed free-fly viewer (winit 0.30 `ApplicationHandler`).
//!
//! Controls: WASD to move, Q/E down/up, hold left mouse to look, Shift to go
//! faster, Esc to quit.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use glam::{Mat4, Vec3};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::camera::Camera;
use crate::gltf::Scene;
use crate::render::{Gpu, Renderer};

/// Play-mode controller: the [`World`](crate::world::World) (player = fighter 0,
/// plus any spawned dummies) driven at a fixed 60 Hz, with a third-person follow
/// camera. The per-tick driving now lives in [`Fighter`](crate::fighter::Fighter);
/// this holds the mouse/aim accumulators and the key press-edges.
struct PlayWorld {
    world: crate::world::World,
    /// Aim pitch (radians), accumulated from mouse-Y; clamped to ±π/2.
    aiming_ud: f32,
    /// Mouse delta accumulated since the last tick (applied once per frame).
    mouse_dx: f32,
    mouse_dy: f32,
    /// Player capsule height (for the follow camera framing).
    player_height: f32,
    accum: f32,
}

const TICK: f32 = 1.0 / 60.0;
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
    /// Mouse buttons held while the cursor is captured (play mode).
    mouse_held: HashSet<oni_config::MouseButton>,
    looking: bool,
    /// Play mode: whether the mouse is captured (locked + hidden, turning the
    /// character). Toggled with Esc so the cursor can be freed without quitting.
    captured: bool,
    /// Player key/mouse bindings (Oni defaults; rebindable later).
    bindings: oni_config::Bindings,
    tuning: oni_config::InputTuning,
    move_speed: f32,
    last: Instant,
    start: Instant,
    charview: Option<crate::CharView>,
    /// Uploaded render model index for `charview` (non-play static viewer).
    char_model: Option<usize>,
    play: Option<PlayWorld>,
}

pub struct App {
    scene: Option<Scene>,
    charview: Option<crate::CharView>,
    particles: Vec<crate::particles::Particle>,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    play: bool,
    classes: Option<crate::classes::Classes>,
    cast: crate::Cast,
    faithful: bool,
    spar: bool,
    state: Option<State>,
}

/// Play-mode settings: whether to run the character controller, and what it
/// drives the character with.
pub struct PlaySetup {
    /// Enable the third-person character controller (needs a character).
    pub play: bool,
    /// The character classes to spawn from.
    pub classes: Option<crate::classes::Classes>,
    /// The player's class and the opponent's.
    pub cast: crate::Cast,
    /// Reproduce the original's known bugs (`docs/QUIRKS.md`).
    pub faithful: bool,
    /// The character in front of the player fights back (sparring AI)
    /// instead of standing as a passive training dummy.
    pub spar: bool,
}

/// Runs the windowed viewer (consumes the scene, optional character, particles).
pub fn run(
    scene: Scene,
    charview: Option<crate::CharView>,
    particles: Vec<crate::particles::Particle>,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    setup: PlaySetup,
) {
    let PlaySetup {
        play,
        classes,
        cast,
        faithful,
        spar,
    } = setup;
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        scene: Some(scene),
        charview,
        particles,
        eye,
        target,
        play,
        classes,
        cast,
        faithful,
        spar,
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

        let mut charview = self.charview.take();
        let mut renderer = Renderer::new(&gpu, &scene, &self.particles, format);

        // Play mode consumes the character into a `World` of fighters (player =
        // fighter 0, plus a training dummy in front); the static viewer keeps the
        // single `CharView` for direct posing.
        let (char_model, play) = if self.play {
            let kinds = self.classes.as_mut().map(|classes| {
                let mut kind = |name: &str| classes.kind(name, |ch| renderer.add_character(&gpu, ch));
                let player = kind(&self.cast.player);
                let opponent = match &self.cast.opponent {
                    Some(o) => kind(o),
                    None => player.clone(),
                };
                (player, opponent)
            });
            match (charview.take(), kinds) {
                (Some(cv), Some((Ok(kind), Ok(opponent_kind)))) => {
                    let spawn_pos = cv.placement.transform_point3(Vec3::ZERO);
                    let (mn, mx) = cv.aabb(0.0);
                    let player_height = (mx.y - mn.y).max(2.0);
                    let (verts, idx) = crate::level_collision_mesh(&scene);
                    let mut world = crate::world::World::new(verts, idx);
                    if self.faithful {
                        world.set_quirks(crate::quirks::Quirks::FAITHFUL);
                    }
                    if let Some(c) = self.classes.as_ref() {
                        world.difficulty = c.difficulty(&self.cast.difficulty);
                    }
                    // Player faces +Z (yaw 0); a passive training dummy 14 units
                    // ahead faces back toward the player, within easy reach.
                    world.spawn(&kind, spawn_pos, 0.0, false);
                    let dummy_pos = spawn_pos + Vec3::new(0.0, 0.0, 14.0);
                    let opponent =
                        world.spawn(&opponent_kind, dummy_pos, std::f32::consts::PI, !self.spar);
                    if self.spar && !world.fight(opponent, 0) {
                        world.fighters[opponent].brain =
                            Some(crate::ai::Brain::Spar(crate::ai::Spar::new(0, 1)));
                    }
                    // Capture the cursor so mouse motion turns the character
                    // continuously (Oni grabs the mouse in gameplay); Esc releases it.
                    apply_capture(&window, true);
                    (
                        None,
                        Some(PlayWorld {
                            world,
                            aiming_ud: 0.0,
                            mouse_dx: 0.0,
                            mouse_dy: 0.0,
                            player_height,
                            accum: 0.0,
                        }),
                    )
                }
                (_, Some((Err(e), _) | (_, Err(e)))) => {
                    eprintln!("play: {e}");
                    (None, None)
                }
                _ => (None, None),
            }
        } else {
            // Static viewer: keep the single character for direct posing.
            let model = charview
                .as_ref()
                .map(|c| renderer.add_character(&gpu, &c.character));
            (model, None)
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
            mouse_held: HashSet::new(),
            looking: false,
            captured: self.play,
            bindings: oni_config::Bindings::oni_defaults(),
            tuning: oni_config::InputTuning::default(),
            move_speed,
            last: now,
            start: now,
            charview,
            char_model,
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
                    if code == KeyCode::Escape && event.state == ElementState::Pressed {
                        if st.play.is_some() {
                            // Play mode: Esc releases/recaptures the mouse (close
                            // the window to quit); otherwise Esc quits.
                            st.captured = !st.captured;
                            st.mouse_held.clear();
                            apply_capture(&st.window, st.captured);
                        } else {
                            event_loop.exit();
                        }
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
            WindowEvent::Focused(false) => {
                // Lost focus (e.g. alt-tab): free the cursor so it isn't trapped,
                // and drop any held buttons so none stays stuck down.
                st.mouse_held.clear();
                st.keys.clear();
                if st.play.is_some() {
                    apply_capture(&st.window, false);
                }
            }
            WindowEvent::Focused(true) => {
                if st.play.is_some() && st.captured {
                    apply_capture(&st.window, true);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if button == MouseButton::Left {
                    st.looking = pressed;
                }
                if st.play.is_some() && !st.captured {
                    // Click back into the window to re-capture (not a fire press).
                    if pressed && button == MouseButton::Left {
                        st.captured = true;
                        apply_capture(&st.window, true);
                    }
                } else if let Some(b) = to_config_mouse(button) {
                    // Captured: the buttons act through the bindings (fire 1/2/3
                    // by default — punch / kick / crouch while unarmed).
                    if pressed {
                        st.mouse_held.insert(b);
                    } else {
                        st.mouse_held.remove(&b);
                    }
                }
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
            if st.play.is_some() {
                // Play mode: the mouse turns the character (accumulated, applied
                // per tick) — only while the cursor is captured.
                if st.captured {
                    if let Some(play) = st.play.as_mut() {
                        play.mouse_dx += delta.0 as f32;
                        play.mouse_dy += delta.1 as f32;
                    }
                }
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

    /// Play-mode character controller: W/S move, A/D strafe, mouse turn; the
    /// camera follows the player third-person. Stepped at a fixed 60 Hz, driving
    /// every fighter in the world (player from keys, dummies idle for now).
    fn update_play(&mut self, dt: f32) {
        use oni_config::Action;
        // Resolve held keys → actions through the bindings (Oni defaults: WASD =
        // forward/back + strafe, Caps Lock = walk; rebindable later).
        let held = |a: Action| {
            self.keys
                .iter()
                .filter_map(|&k| to_config_key(k))
                .any(|k| self.bindings.key_action(k) == Some(a))
                || self.mouse_held.iter().any(|&b| self.bindings.mouse_action(b) == Some(a))
        };
        use crate::anim::input::key;
        let mut controls = 0u32;
        for (action, bit) in [
            (Action::Forward, key::FORWARD),
            (Action::Backward, key::BACKWARD),
            (Action::StepLeft, key::STEP_LEFT),
            (Action::StepRight, key::STEP_RIGHT),
            (Action::Walk, key::WALK),
            (Action::Crouch, key::CROUCH),
            (Action::Jump, key::JUMP),
            (Action::Punch, key::PUNCH),
            (Action::Kick, key::KICK),
            (Action::Fire1, key::FIRE1),
            (Action::Fire2, key::FIRE2),
            (Action::Fire3, key::FIRE3),
        ] {
            if held(action) {
                controls |= bit;
            }
        }
        let tuning = self.tuning;
        let sens = MOUSE_SENS * tuning.mouse_sensitivity;

        let Some(pw) = self.play.as_mut() else {
            return;
        };

        // Apply this frame's accumulated mouse motion to the player's facing
        // target (Oni: mouse-X → desiredFacing, negated; clamped ±π) and aim
        // pitch (mouse-Y → aimingUD, clamped ±π/2).
        let dx = std::mem::take(&mut pw.mouse_dx);
        let dy = std::mem::take(&mut pw.mouse_dy);
        let dy = if tuning.invert_y { -dy } else { dy };
        if let Some(player) = pw.world.fighters.first_mut() {
            player
                .facing
                .adjust_desired((-dx * sens).clamp(-std::f32::consts::PI, std::f32::consts::PI));
        }
        pw.aiming_ud = (pw.aiming_ud + dy * sens)
            .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);

        pw.accum += dt;
        while pw.accum >= TICK {
            pw.accum -= TICK;
            // The player from the controls, dummies idle (AI later); then
            // melee hits resolve and reactions play.
            pw.world.step(controls);
        }

        // Third-person follow camera (behind + above the player, at the torso).
        if let Some(player) = pw.world.player() {
            let feet = player.feet();
            // Follow the aim, not the body: a diagonal run leans the body
            // 45° but the view stays put.
            let yaw = player.facing.aim();
            let height = pw.player_height;
            let center = feet + Vec3::new(0.0, height * 0.55, 0.0);
            let fwd = Vec3::new(yaw.sin(), 0.0, yaw.cos());
            let eye = center - fwd * (height * 2.2) + Vec3::new(0.0, height * 0.4, 0.0);
            self.cam.look_at(eye, center);
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
        // Pose the characters. In play mode: every fighter in the world. In the
        // static viewer: the single character, looping over elapsed time.
        let bones: Vec<Vec<Mat4>> = match (&self.play, &self.charview) {
            (Some(pw), _) => pw.world.fighters.iter().map(|f| f.bones()).collect(),
            (None, Some(cv)) => {
                let dur = cv.clip.as_ref().map(|c| c.duration).unwrap_or(1.0);
                let t = self.start.elapsed().as_secs_f32() % dur.max(0.001);
                vec![cv.bones(t)]
            }
            (None, None) => Vec::new(),
        };
        let draws: Vec<crate::render::CharDraw> = match (&self.play, self.char_model) {
            (Some(pw), _) => pw
                .world
                .fighters
                .iter()
                .zip(&bones)
                .map(|(f, b)| f.draw(b))
                .collect(),
            (None, Some(m)) => bones
                .first()
                .filter(|b| !b.is_empty())
                .map(|b| {
                    vec![crate::render::CharDraw {
                        model: m,
                        bones: b,
                    }]
                })
                .unwrap_or_default(),
            (None, None) => Vec::new(),
        };
        let (cam_right, cam_up) = self.cam.billboard_basis();
        self.renderer.render(
            &self.gpu,
            &view,
            self.config.width,
            self.config.height,
            self.cam.view_proj(),
            &draws,
            cam_right,
            cam_up,
        );
        frame.present();
    }
}

/// Grabs or releases the mouse cursor for play mode. Tries OS cursor-lock
/// first (continuous turning), falling back to confine-to-window; hides the
/// cursor while captured.
fn apply_capture(window: &Window, on: bool) {
    use winit::window::CursorGrabMode;
    if on {
        let _ = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
        window.set_cursor_visible(false);
    } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
    }
}

/// Maps a `winit` physical key to the portable `oni_config::Key` the bindings
/// are keyed on (variant names mirror `winit`'s). Unmapped keys return `None`.
/// Maps a winit mouse button to the portable config identifier.
fn to_config_mouse(b: MouseButton) -> Option<oni_config::MouseButton> {
    match b {
        MouseButton::Left => Some(oni_config::MouseButton::Left),
        MouseButton::Right => Some(oni_config::MouseButton::Right),
        MouseButton::Middle => Some(oni_config::MouseButton::Middle),
        _ => None,
    }
}

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
        KeyCode::Semicolon => K::Semicolon,
        KeyCode::Quote => K::Quote,
        KeyCode::BracketLeft => K::BracketLeft,
        KeyCode::Comma => K::Comma,
        KeyCode::Backslash => K::Backslash,
        KeyCode::F13 => K::F13,
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
