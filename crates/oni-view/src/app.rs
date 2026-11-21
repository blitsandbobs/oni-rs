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
    /// Mouse delta accumulated since the last tick (applied once per frame).
    mouse_dx: f32,
    mouse_dy: f32,
    accum: f32,
    /// Real time banked for the sound alone (the game standing still).
    audio_accum: f32,
    /// The level's own scripts (`--mission`), and the classes they spawn
    /// from.
    mission: Option<(crate::mission::Mission, crate::classes::Classes)>,
    /// The sound output (none without a device).
    _audio: Option<crate::audio::output::Output>,
    /// Mission play's flow (`docs/GAME_FLOW.md`): which level, a splash up,
    /// a key pressed since the last frame.
    flow: Option<Flow>,
}

/// Where mission play is, and what it goes on to.
struct Flow {
    oni_root: std::path::PathBuf,
    level: u16,
    splash: Option<crate::game::Splash>,
    key: bool,
}

/// What a level is played with (the command line's choices).
#[derive(Clone)]
struct Settings {
    player: String,
    difficulty: String,
    faithful: bool,
    subtitles: bool,
    sound_law: crate::audio::mixer::Law,
}

/// Builds level `level` for mission play (`ONrLevel_Load`): its scene
/// drawn, its world set up, its scripts begun from `save_point` with the
/// save kept (`saved`) for them to restore. The scene can be given when it
/// is already loaded.
#[allow(clippy::too_many_arguments)]
fn mission_level(
    gpu: &Gpu,
    format: wgpu::TextureFormat,
    oni_root: &std::path::Path,
    level: u16,
    scene: Option<Scene>,
    set: &Settings,
    save_point: i64,
    saved: Option<crate::game::Continue>,
) -> Result<(Renderer, PlayWorld), String> {
    let level_dir = oni_root.join(format!("levels/{level:02}"));
    let scene = match scene {
        Some(s) => s,
        None => {
            let path = level_dir.join("environment.glb");
            eprintln!("loading {}...", path.display());
            let mut s = crate::gltf::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            crate::doors::split_leaves(&mut s, &crate::doors::load(&level_dir));
            s
        }
    };
    let mut renderer = Renderer::new(gpu, &scene, &[], format);
    let mut classes = crate::classes::Classes::open(oni_root, level).map_err(|e| format!("classes: {e}"))?;
    let kind = classes.kind(&set.player, |ch| renderer.add_character(gpu, ch))?;
    let mut world = crate::world::World::new();
    if set.faithful {
        world.set_quirks(crate::quirks::Quirks::FAITHFUL);
    }
    world.difficulty = classes.difficulty(&set.difficulty);
    world.nav = crate::nav::Navigation::load(&level_dir).ok().map(std::rc::Rc::new);
    world.spawn(&kind, Vec3::ZERO, 0.0, false);
    world.load_level(&level_dir);
    world.hud.subtitles_on = set.subtitles;
    world.saved = saved;
    world.persist_continues = true;
    // What the player has met in earlier runs (the pause screen's pages);
    // a level begun afresh flags its new moves.
    if let Some((items, weapons)) = crate::game::load_unlocks() {
        world.hud.items_seen = items;
        world.hud.weapon_classes_seen = weapons;
    }
    if save_point == 0 {
        world.hud.level_started();
    }
    world.attach_doors(crate::door_meshes(&scene));
    world.attach_glass(crate::glass_parts(&scene));
    world.attach_weapons(&oni_root.join("common"));
    // (After the weapons: that is where the level quads load.)
    renderer.attach_quads(&scene, &world.quads);
    renderer.attach_texture_anims(gpu, world.texture_anims());
    let mut m = crate::mission::Mission::load(&level_dir)?;
    m.state.vars.insert("save_point".into(), mlua::Value::Integer(save_point));
    {
        let mut host = PlayHost { world: &mut world, classes: &mut classes, renderer: &mut renderer, gpu };
        m.begin(&mut host);
    }
    world.audio.mixer.lock().unwrap().law = set.sound_law;
    let audio = crate::audio::output::Output::open(world.audio.mixer.clone());
    world.audio.realtime = audio.is_some();
    Ok((
        renderer,
        PlayWorld {
            world,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            accum: 0.0,
            audio_accum: 0.0,
            mission: Some((m, classes)),
            _audio: audio,
            flow: Some(Flow { oni_root: oni_root.to_path_buf(), level, splash: None, key: false }),
        },
    ))
}

/// What the level's scripts reach in play mode.
struct PlayHost<'a> {
    world: &'a mut crate::world::World,
    classes: &'a mut crate::classes::Classes,
    renderer: &'a mut Renderer,
    gpu: &'a Gpu,
}

impl crate::mission::Host for PlayHost<'_> {
    fn world(&mut self) -> &mut crate::world::World {
        self.world
    }
    fn kind(&mut self, class: &str) -> Result<crate::world::CharKind, String> {
        let (renderer, gpu) = (&mut *self.renderer, self.gpu);
        self.classes.kind(class, |ch| renderer.add_character(gpu, ch))
    }
    fn melee(&self, id: u16) -> Option<std::rc::Rc<crate::ai::melee::profile::Profile>> {
        self.classes.melee_by_id(id)
    }    fn clip(&mut self, name: &str) -> Option<std::rc::Rc<crate::anim::collection::AnimCollection>> {
        self.classes.clip(name)
    }
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
    /// The cursor (logical pixels), for the pause screen's buttons.
    cursor: [f32; 2],
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
    /// What mission play's levels are built with.
    settings: Settings,
    /// The game is over: close.
    quit: bool,
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
    mission: bool,
    subtitles: bool,
    save_point: i64,
    sound_law: crate::audio::mixer::Law,
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
    /// Run the level's own scripts (the player at its placement, the
    /// level's characters as its scripts spawn them) instead of a dummy.
    pub mission: bool,
    /// Show dialogue subtitles (the game's option).
    pub subtitles: bool,
    /// Where the level's scripts start it (`save_point`: 0 its start; on
    /// level 1, 1 the warehouse after the training).
    pub save_point: i64,
    /// How volumes become loudness: the PC build's (default) or the Mac's.
    pub sound_law: crate::audio::mixer::Law,
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
        mission,
        subtitles,
        save_point,
        sound_law,
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
        mission,
        subtitles,
        save_point,
        sound_law,
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
        let settings = Settings {
            player: self.cast.player.clone(),
            difficulty: self.cast.difficulty.clone(),
            faithful: self.faithful,
            subtitles: self.subtitles,
            sound_law: self.sound_law,
        };
        // Mission play: the level as the game plays it (its scripts, its
        // flow from level to level).
        let mission_play = if self.play && self.mission {
            let level_dir = self.cast.level_dir.clone();
            let level = level_dir.file_name().and_then(|n| n.to_str()).and_then(|n| n.parse().ok()).unwrap_or(1);
            let oni_root = level_dir.parent().and_then(|p| p.parent()).map(std::path::Path::to_path_buf).unwrap_or_default();
            // (Started at a save point: its continue, if one was kept.)
            let saved = (self.save_point > 0).then(|| crate::game::load_continue(level, self.save_point)).flatten();
            if self.save_point > 0 && saved.is_none() {
                eprintln!("game: no continue kept for save point {} (the level's start)", self.save_point);
            }
            match mission_level(&gpu, format, &oni_root, level, Some(scene.clone()), &settings, self.save_point, saved) {
                Ok((r, pw)) => {
                    renderer = r;
                    charview = None;
                    apply_capture(&window, true);
                    Some(pw)
                }
                Err(e) => {
                    eprintln!("mission: {e}");
                    None
                }
            }
        } else {
            None
        };
        let (char_model, play) = if let Some(pw) = mission_play {
            (None, Some(pw))
        } else if self.play {
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
                    let mut world = crate::world::World::new();
                    if self.faithful {
                        world.set_quirks(crate::quirks::Quirks::FAITHFUL);
                    }
                    if let Some(c) = self.classes.as_ref() {
                        world.difficulty = c.difficulty(&self.cast.difficulty);
                    }
                    world.nav = crate::nav::Navigation::load(&self.cast.level_dir).ok().map(std::rc::Rc::new);
                    // Player faces +Z (yaw 0); a passive training dummy 14 units
                    // ahead faces back toward the player, within easy reach.
                    world.spawn(&kind, spawn_pos, 0.0, false);
                    let mut mission = None;
                    world.load_level(&self.cast.level_dir);
                    world.hud.subtitles_on = self.subtitles;
                    world.attach_doors(crate::door_meshes(&scene));
                    world.attach_glass(crate::glass_parts(&scene));
                    world.attach_weapons(&self.cast.level_dir.join("../../common"));
                    // (After the weapons: that is where the level quads load.)
                    renderer.attach_quads(&scene, &world.quads);
                    renderer.attach_texture_anims(&gpu, world.texture_anims());
                    if self.mission {
                        // The level's own scripts populate it.
                        match (crate::mission::Mission::load(&self.cast.level_dir), self.classes.take()) {
                            (Ok(mut m), Some(mut classes)) => {
                                m.state.vars.insert("save_point".into(), mlua::Value::Integer(self.save_point));
                                let mut host = PlayHost {
                                    world: &mut world,
                                    classes: &mut classes,
                                    renderer: &mut renderer,
                                    gpu: &gpu,
                                };
                                m.begin(&mut host);
                                mission = Some((m, classes));
                            }
                            (Err(e), classes) => {
                                eprintln!("mission: {e}");
                                self.classes = classes;
                            }
                            (_, None) => eprintln!("mission: no character classes"),
                        }
                    } else {
                        let dummy_pos = spawn_pos + Vec3::new(0.0, 0.0, 14.0);
                        let opponent =
                            world.spawn(&opponent_kind, dummy_pos, std::f32::consts::PI, !self.spar);
                        if self.spar && !world.fight(opponent, 0) {
                            world.fighters[opponent].brain =
                                Some(crate::ai::Brain::Spar(crate::ai::Spar::new(0, 1)));
                        }
                    }
                    // Capture the cursor so mouse motion turns the character
                    // continuously (Oni grabs the mouse in gameplay); Esc releases it.
                    apply_capture(&window, true);
                    world.audio.mixer.lock().unwrap().law = self.sound_law;
                    let audio = crate::audio::output::Output::open(world.audio.mixer.clone());
                    world.audio.realtime = audio.is_some();
                    if audio.is_none() {
                        eprintln!("audio: no output device; playing silently");
                    }
                    (
                        None,
                        Some(PlayWorld {
                            world,
                            mouse_dx: 0.0,
                            mouse_dy: 0.0,
                            accum: 0.0,
                            audio_accum: 0.0,
                            mission,
                            _audio: audio,
                            flow: None,
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
            cursor: [0.0, 0.0],
            captured: self.play,
            bindings: oni_config::Bindings::oni_defaults(),
            tuning: oni_config::InputTuning::default(),
            move_speed,
            last: now,
            start: now,
            charview,
            char_model,
            play,
            settings,
            quit: false,
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
                if event.state == ElementState::Pressed && !event.repeat {
                    if let Some(f) = st.play.as_mut().and_then(|p| p.flow.as_mut()) {
                        f.key = true;
                    }
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    // A data console's text up (modal: `ONiTC_HandleKeyDown`):
                    // Space for the next page, Esc to close; nothing else.
                    if let Some(pw) = st.play.as_mut().filter(|p| p.world.hud.text_console.is_some()) {
                        if event.state == ElementState::Pressed && !event.repeat {
                            match code {
                                KeyCode::Space => pw.world.hud.text_console_next(),
                                KeyCode::Escape => pw.world.hud.text_console_close(),
                                _ => {}
                            }
                        }
                        st.keys.clear();
                        return;
                    }
                    // The pause screen up (modal): F1 closes it once it has
                    // been up a second, Esc is its Close; nothing else.
                    if let Some(pw) = st.play.as_mut().filter(|p| p.world.hud.pause.is_some()) {
                        if event.state == ElementState::Pressed && !event.repeat {
                            match code {
                                KeyCode::F1 => pw.world.pause_toggle(),
                                KeyCode::Escape => pw.world.pause_press(2, false),
                                _ => {}
                            }
                            if pw.world.hud.pause.is_none() {
                                apply_capture(&st.window, st.captured);
                            }
                        }
                        st.keys.clear();
                        return;
                    }
                    // Space in a cutscene skips it (restored: as shipped the
                    // call is commented out, Q18).
                    if code == KeyCode::Space && event.state == ElementState::Pressed && !event.repeat {
                        if let Some(pw) = st.play.as_mut().filter(|p| p.world.in_cutscene && !p.world.quirks.faithful) {
                            pw.world.skip_cutscene();
                        }
                    }
                    // F1: the pause screen (the cursor freed for its buttons).
                    if code == KeyCode::F1 && event.state == ElementState::Pressed && !event.repeat {
                        if let Some(pw) = st.play.as_mut() {
                            pw.world.pause_toggle();
                            if pw.world.hud.pause.is_some() {
                                st.mouse_held.clear();
                                apply_capture(&st.window, false);
                            }
                        }
                        st.keys.clear();
                        return;
                    }
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
            WindowEvent::CursorMoved { position, .. } => {
                let scale = st.window.scale_factor() as f32;
                st.cursor = [position.x as f32 / scale, position.y as f32 / scale];
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                // The pause screen up: a click is on its buttons.
                if let Some(pw) = st.play.as_mut().filter(|p| p.world.hud.pause.is_some()) {
                    if pressed && button == MouseButton::Left {
                        let scale = st.window.scale_factor() as f32;
                        let (w, h) = (st.config.width as f32 / scale, st.config.height as f32 / scale);
                        if let Some(id) = pw.world.hud.pause_hit(w, h, st.cursor[0], st.cursor[1]) {
                            pw.world.pause_press(id, true);
                        }
                        if pw.world.hud.pause.is_none() {
                            apply_capture(&st.window, st.captured);
                        }
                    }
                    return;
                }
                if pressed {
                    if let Some(f) = st.play.as_mut().and_then(|p| p.flow.as_mut()) {
                        f.key = true;
                    }
                }
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.as_ref().is_some_and(|st| st.quit) {
            event_loop.exit();
            return;
        }
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
            (Action::Use, key::ACTION),
            (Action::Swap, key::SWAP),
            (Action::Reload, key::RELOAD),
            (Action::Drop, key::DROP),
            (Action::LookMode, key::LOOK),
            (Action::Hypo, key::HYPO),
        ] {
            if held(action) {
                controls |= bit;
            }
        }
        let tuning = self.tuning;
        let sens = MOUSE_SENS * tuning.mouse_sensitivity;

        // A splash screen up: the game stands still until it is over, then
        // goes on as the splash says (its sound still running).
        if let Some(pw) = self.play.as_mut().filter(|p| p.flow.as_ref().is_some_and(|f| f.splash.is_some())) {
            audio_only(pw, dt);
        }
        if let Some(after) = self.splash_tick(dt) {
            self.after_splash(after);
            return;
        }
        if self.play.as_ref().and_then(|p| p.flow.as_ref()).is_some_and(|f| f.splash.is_some()) {
            return;
        }
        let Some(pw) = self.play.as_mut() else {
            return;
        };

        // Apply this frame's accumulated mouse motion to the player's facing
        // target (Oni: mouse-X → desiredFacing, negated; clamped ±π) and aim
        // pitch (mouse-Y → aimingUD, clamped ±π/2).
        let dx = std::mem::take(&mut pw.mouse_dx);
        let dy = std::mem::take(&mut pw.mouse_dy);
        // (A data console's text up: the game stands still.)
        let frozen = pw.world.hud.modal();
        let (dx, dy) = if frozen { (0.0, 0.0) } else { (dx, dy) };
        // (As the game ships — its "invert mouse" option on — the mouse
        // moved down aims down.)
        let dy = if tuning.invert_y { dy } else { -dy };
        // (With a gun the mouse moves at the gun's aiming speed × ½.)
        let sens = sens * pw.world.aiming_speed(0).map_or(1.0, |s| s * 0.5);
        let look = controls & crate::anim::input::key::LOOK != 0;
        if let Some(player) = pw.world.fighters.first_mut() {
            let turn = (-dx * sens).clamp(-std::f32::consts::PI, std::f32::consts::PI);
            if look {
                // Look mode: the mouse turns the aim off the facing.
                player.aiming_lr = (player.aiming_lr + turn).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            } else {
                player.facing.adjust_desired(turn);
            }
            player.aiming_ud = (player.aiming_ud + dy * sens)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        }

        pw.accum += dt;
        // A cutscene being skipped: 32 ticks this frame, whatever the clock.
        let mut fast = if frozen { None } else { pw.world.skip_frame() };
        if frozen {
            // (The sound runs on: loops, fades, the volume.)
            pw.accum = 0.0;
            audio_only(pw, dt);
        }
        if fast.is_some() {
            pw.accum = 0.0;
        }
        loop {
            match fast.as_mut() {
                Some(0) => break,
                Some(n) => *n -= 1,
                None if pw.accum >= TICK => pw.accum -= TICK,
                None => break,
            }
            // The player from the controls, dummies idle (AI later); then
            // melee hits resolve and reactions play.
            pw.world.step(controls);
            if let Some((m, classes)) = pw.mission.as_mut() {
                let mut host = PlayHost {
                    world: &mut pw.world,
                    classes,
                    renderer: &mut self.renderer,
                    gpu: &self.gpu,
                };
                m.tick(&mut host);
            }
        }

        // The game camera (`world/camera.rs`).
        let aspect = self.config.width as f32 / self.config.height as f32;
        pw.world.camera.aspect = aspect;
        self.cam = pw.world.camera.render_camera(aspect);
        self.flow_check();
    }

    /// After the level's ticks (`Oni.c`'s loop): a splash a script asked
    /// for; the level won (the win splash, then the next level — or the
    /// end) or lost (the fail splash, then the level again from its save
    /// point).
    fn flow_check(&mut self) {
        use crate::game::{next_level, splash_texture, After, Splash};
        let Some(pw) = self.play.as_mut() else { return };
        let Some(flow) = pw.flow.as_mut() else { return };
        if let Some(name) = pw.world.pending_splash.take() {
            if let Some(t) = splash_texture(&flow.oni_root, flow.level, &name) {
                flow.splash = Some(Splash::new(t, After::Resume));
            }
        }
        match pw.world.victory.take() {
            Some(true) => {
                eprintln!("game: level {} won", flow.level);
                pw.world.saved = None;
                let after = match next_level(&flow.oni_root, flow.level) {
                    Some(n) if flow.level != 19 => After::Next(n),
                    _ => After::End,
                };
                flow.splash = Some(Splash::new(splash_texture(&flow.oni_root, flow.level, "win_splash_screen").unwrap_or_default(), after));
                splash_music(&mut pw.world.audio, flow.splash.as_mut(), "main_menu_win");
            }
            Some(false) => {
                eprintln!("game: level {} lost", flow.level);
                flow.splash = Some(Splash::new(splash_texture(&flow.oni_root, flow.level, "fail_splash_screen").unwrap_or_default(), After::Restart));
                splash_music(&mut pw.world.audio, flow.splash.as_mut(), "loss_screen_music");
            }
            None => {}
        }
        flow.key = false;
    }

    /// Moves the splash on by real time — its music fading out with it,
    /// halted at its end; what follows once it is over.
    fn splash_tick(&mut self, dt: f32) -> Option<crate::game::After> {
        let pw = self.play.as_mut()?;
        let flow = pw.flow.as_mut()?;
        let key = std::mem::take(&mut flow.key);
        let splash = flow.splash.as_mut()?;
        let was_out = splash.fading_out();
        let after = splash.tick(dt * 60.0, key);
        if let Some(music) = splash.music {
            if splash.fading_out() && !was_out {
                pw.world.audio.script_volume(music, 0.0, 1.0);
            }
            if after.is_some() {
                pw.world.audio.script_halt(music);
            }
        }
        let after = after?;
        flow.splash = None;
        // (A splash over a cutscene: its skip waits to settle again.)
        pw.world.cutscene_ticks = 0;
        Some(after)
    }

    /// The level again from its save point, or the next level from its
    /// start, or the end.
    fn after_splash(&mut self, after: crate::game::After) {
        use crate::game::After;
        let Some(pw) = self.play.as_mut() else { return };
        let Some(flow) = pw.flow.as_ref() else { return };
        let (oni_root, level) = (flow.oni_root.clone(), flow.level);
        let (target, save_point, saved) = match after {
            After::Resume => return,
            After::End => {
                eprintln!("game: the end");
                self.quit = true;
                return;
            }
            After::Restart => {
                let saved = pw.world.saved.take();
                (level, saved.as_ref().map_or(0, |c| c.save_point), saved)
            }
            After::Next(n) => (n, 0, None),
        };
        // (The old level's sound stops with its world.)
        self.play = None;
        let format = self.config.format;
        match mission_level(&self.gpu, format, &oni_root, target, None, &self.settings, save_point, saved) {
            Ok((r, pw)) => {
                self.renderer = r;
                self.play = Some(pw);
                eprintln!("game: level {target}, save point {save_point}");
            }
            Err(e) => {
                eprintln!("game: can't load level {target}: {e}");
                self.quit = true;
            }
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
        if let Some(pw) = &mut self.play {
            // The HUD, in the window's logical pixels (its size on a 640 × 480
            // screen of the time on a high-density display).
            let scale = self.window.scale_factor() as f32;
            let (w, h) = (self.config.width as f32 / scale, self.config.height as f32 / scale);
            self.renderer.overlay = if pw.world.skipping() { Vec::new() } else { pw.world.hud_draw(w, h) };
            self.renderer.overlay_size = [w, h];
            // A cutscene skip's fade (black while it skips).
            if let Some(a) = pw.world.skip_blanket() {
                self.renderer.overlay.push(crate::render::OverlayDraw { texture: None, tris: quad([0.0, 0.0, w, h]), color: [0.0, 0.0, 0.0, a], additive: false });
            }
            pw.world.skip_drawn();
            // A splash screen over everything: black, its picture in the
            // middle (640 × 480), the fade's black over it.
            if let Some(sp) = pw.flow.as_ref().and_then(|f| f.splash.as_ref()) {
                let black = |a: f32| crate::render::OverlayDraw { texture: None, tris: quad([0.0, 0.0, w, h]), color: [0.0, 0.0, 0.0, a], additive: false };
                self.renderer.overlay.push(black(1.0));
                let (x, y) = ((w - 640.0) * 0.5, (h - 480.0) * 0.5);
                self.renderer.overlay.push(crate::render::OverlayDraw { texture: Some(sp.texture.clone()), tris: quad([x, y, x + 640.0, y + 480.0]), color: [1.0; 4], additive: false });
                self.renderer.overlay.push(black(sp.blanket()));
            }
            self.renderer.door_motion = pw.world.door_motions();
            self.renderer.beams = pw.world.beams.clone();
            self.renderer.sprites = pw.world.sprites();
            self.renderer.decals = pw.world.decal_draws(self.cam.eye);
            self.renderer.hidden_refs = pw.world.env_hidden.clone();
            self.renderer.broken_glass = pw.world.broken_glass.clone();
            self.renderer.jello = pw.world.jello.clone();
            self.renderer.anim_time = pw.world.game_tick();
            self.renderer.eye = self.cam.eye;
            self.renderer.fog = pw.world.fog.fog;
        }
        let bones: Vec<Vec<Mat4>> = match (&self.play, &self.charview) {
            (Some(pw), _) => pw.world.fighters.iter().map(|f| f.bones()).collect(),
            (None, Some(cv)) => {
                let dur = cv.clip.as_ref().map(|c| c.duration).unwrap_or(1.0);
                let t = self.start.elapsed().as_secs_f32() % dur.max(0.001);
                vec![cv.bones(t)]
            }
            (None, None) => Vec::new(),
        };
        // Guns: one-bone models placed by their matrices.
        let guns: Vec<(usize, Vec<Mat4>, bool)> = match &self.play {
            Some(pw) => {
                let mut v: Vec<_> = pw.world.gun_draws().into_iter().chain(pw.world.particle_models()).chain(pw.world.turret_draws()).chain(pw.world.object_draws()).chain(pw.world.powerup_draws()).filter_map(|(path, m)| Some((self.renderer.model_from(&self.gpu, &path)?, vec![m], false))).collect();
                // (Objects in the camera's way, see-through.)
                for (path, m) in pw.world.object_jello_draws() {
                    if let Some(k) = self.renderer.model_from(&self.gpu, &path) {
                        v.push((k, vec![m], true));
                    }
                }
                // (Consoles' screens, textured by their state.)
                for (path, tex, m) in pw.world.console_draws() {
                    if let Some(k) = self.renderer.model_textured(&self.gpu, &path, &tex) {
                        v.push((k, vec![m], false));
                    }
                }
                v
            }
            None => Vec::new(),
        };
        let draws: Vec<crate::render::CharDraw> = match (&self.play, self.char_model) {
            (Some(pw), _) => pw
                .world
                .fighters
                .iter()
                .zip(&bones)
                .map(|(f, b)| f.draw(b))
                .chain(guns.iter().map(|(m, b, see_through)| crate::render::CharDraw { model: *m, bones: b, see_through: *see_through }))
                .collect(),
            (None, Some(m)) => bones
                .first()
                .filter(|b| !b.is_empty())
                .map(|b| {
                    vec![crate::render::CharDraw {
                        model: m,
                        bones: b,
                        see_through: false,
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

/// A screen rectangle's two triangles (x, y, u, v).
fn quad(r: [f32; 4]) -> Vec<[f32; 4]> {
    let [l, t, rr, b] = r;
    vec![[l, t, 0.0, 0.0], [rr, t, 1.0, 0.0], [rr, b, 1.0, 1.0], [l, t, 0.0, 0.0], [rr, b, 1.0, 1.0], [l, b, 0.0, 1.0]]
}

/// Grabs or releases the mouse cursor for play mode. Tries OS cursor-lock
/// first (continuous turning), falling back to confine-to-window; hides the
/// cursor while captured.
/// The win or fail splash's music (`ONrGameState_SplashScreen`): every
/// other sound stopped, `music` started silent and faded in over a second.
fn splash_music(audio: &mut crate::audio::Audio, splash: Option<&mut crate::game::Splash>, music: &'static str) {
    let Some(splash) = splash else { return };
    audio.stop_all();
    if audio.script_start(music, 0.0) {
        audio.script_volume(music, 1.0, 1.0);
        splash.music = Some(music);
    }
}

/// The sound alone ticked on by real time (the game standing still: a
/// splash, the pause screen, a console's text).
fn audio_only(pw: &mut PlayWorld, dt: f32) {
    pw.audio_accum += dt;
    while pw.audio_accum >= TICK {
        pw.audio_accum -= TICK;
        pw.world.audio.tick(Some(&pw.world.quads));
    }
}

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
