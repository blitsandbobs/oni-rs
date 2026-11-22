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

mod front;
mod mouse;
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
    /// Mission play's flow: which level, a splash up,
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
    /// The overall volume (the options').
    volume: f32,
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
    progress: &mut dyn FnMut(u32),
) -> Result<(Renderer, PlayWorld), String> {
    // (The level-load dialog's steps, `ONrLevel_Load`: the original's
    // percents at the matching points of this loading order.)
    progress(0);
    progress(10);
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
    progress(20);
    let mut renderer = Renderer::new(gpu, &scene, &[], format);
    progress(28);
    let mut classes =
        crate::classes::Classes::open(oni_root, level).map_err(|e| format!("classes: {e}"))?;
    let kind = classes.kind(&set.player, |ch| renderer.add_character(gpu, ch))?;
    // (A level begun: the place last played, the level unlocked.)
    crate::game::level_begun(level);
    let mut world = crate::opening::open_world(
        gpu,
        &mut renderer,
        &scene,
        &mut classes,
        crate::opening::Opening {
            level_dir: &level_dir,
            faithful: set.faithful,
            difficulty: &set.difficulty,
            player: &kind,
            at: Vec3::ZERO,
        },
        progress,
    );
    world.hud.subtitles_on = set.subtitles;
    world.progress.saved = saved;
    world.progress.persist_continues = true;
    // What the player has met in earlier runs (the pause screen's pages);
    // a level begun afresh flags its new moves.
    if let Some((items, weapons)) = crate::game::load_unlocks() {
        world.hud.items_seen = items;
        world.hud.weapon_classes_seen = weapons;
    }
    if save_point == 0 {
        world.hud.level_started();
    }
    let mut m = crate::mission::Mission::load(&level_dir)?;
    m.state
        .vars
        .insert("save_point".into(), mlua::Value::Integer(save_point));
    {
        let mut host = PlayHost {
            world: &mut world,
            classes: &mut classes,
            renderer: &mut renderer,
            gpu,
        };
        m.begin(&mut host);
    }
    progress(85);
    let audio = open_audio(&mut world, set.sound_law, set.volume);
    progress(90);
    progress(100);
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
            flow: Some(Flow {
                oni_root: oni_root.to_path_buf(),
                level,
                splash: None,
                key: false,
            }),
        },
    ))
}

/// The world's sound to the audio device (none: played silently).
fn open_audio(
    world: &mut crate::world::World,
    law: crate::audio::mixer::Law,
    volume: f32,
) -> Option<crate::audio::output::Output> {
    world.audio.mixer.lock().unwrap().law = law;
    world.audio.set_volume(volume);
    let audio = crate::audio::output::Output::open(world.audio.mixer.clone());
    world.audio.realtime = audio.is_some();
    if audio.is_none() {
        eprintln!("audio: no output device; playing silently");
    }
    audio
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
        self.classes
            .kind(class, |ch| renderer.add_character(gpu, ch))
    }
    fn melee(&mut self, id: u16) -> Option<std::rc::Rc<crate::ai::melee::profile::Profile>> {
        self.classes.melee_by_id(id)
    }
    fn clip(&mut self, name: &str) -> Option<std::rc::Rc<crate::anim::collection::AnimCollection>> {
        self.classes.clip(name)
    }
    fn classes(&self) -> Option<&crate::classes::Classes> {
        Some(self.classes)
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
    /// The mouse as set: focus, grab, the arrow (`app/mouse.rs`).
    mouse: mouse::Mouse,
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
    /// The out-of-game menus (`app/front.rs`): at start-up with no level
    /// given, and over mission play (Esc).
    front: Option<front::Front>,
    /// The gamma option (`gamma.rs`).
    gamma: f32,
    /// The game is over: close.
    quit: bool,
    /// The ending movie playing.
    movie: Option<MoviePlay>,
}

/// The ending movie playing: the player, its sound's own mixer and output,
/// whether a key or click asked it to stop, and where the game goes after
/// (the main menu, else closing).
struct MoviePlay {
    movie: crate::movie::Movie,
    mixer: std::sync::Arc<std::sync::Mutex<crate::audio::mixer::Mixer>>,
    output: Option<crate::audio::output::Output>,
    volume: f32,
    skip: bool,
    to_menu: bool,
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
    /// Start at the main menu, the assets here (no level given).
    front_end: Option<std::path::PathBuf>,
    /// First the ending movie (`--outro`, for testing).
    outro: bool,
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
    /// Reproduce the original's known bugs (`docs/deviations.md`).
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
    /// Start at the main menu (the converted assets' root): the player's
    /// default, `oni` with no arguments.
    pub front_end: Option<std::path::PathBuf>,
    /// With the menu: play the ending movie first (`oni --outro`).
    pub outro: bool,
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
        front_end,
        outro,
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
        front_end,
        outro,
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
                .create_window(Window::default_attributes().with_title("oni — level"))
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
        // The player's options: at the menu they set the game up; given on
        // the command line, the line's choices win.
        let user = crate::settings::Settings::load();
        renderer.flares_behind_characters = user.flares_behind_characters;
        let settings = Settings {
            player: self.cast.player.clone(),
            difficulty: if self.front_end.is_some() {
                user.difficulty.clone()
            } else {
                self.cast.difficulty.clone()
            },
            faithful: self.faithful,
            subtitles: self.subtitles || (self.front_end.is_some() && user.subtitles),
            sound_law: self.sound_law,
            volume: user.overall_volume,
        };
        let mut front = self
            .front_end
            .clone()
            .map(|root| front::Front::new(root, user.clone(), true));
        // Mission play: the level as the game plays it (its scripts, its
        // flow from level to level).
        let mission_play = if self.play && self.mission {
            let level_dir = self.cast.level_dir.clone();
            let level = level_dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            let oni_root = level_dir
                .parent()
                .and_then(|p| p.parent())
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            // (Started at a save point: its continue, if one was kept.)
            let saved = (self.save_point > 0)
                .then(|| crate::game::load_continue(level, self.save_point))
                .flatten();
            if self.save_point > 0 && saved.is_none() {
                eprintln!(
                    "game: no continue kept for save point {} (the level's start)",
                    self.save_point
                );
            }
            // (The game's mode kept with it, for its continues.)
            crate::game::set_game_faithful(settings.faithful);
            match mission_level(
                &gpu,
                format,
                &oni_root,
                level,
                Some(scene.clone()),
                &settings,
                self.save_point,
                saved,
                &mut |_| {},
            ) {
                Ok((r, pw)) => {
                    renderer = r;
                    charview = None;
                    // (Esc brings up the main menu over it.)
                    front = Some(front::Front::new(oni_root.clone(), user.clone(), false));
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
                let mut kind =
                    |name: &str| classes.kind(name, |ch| renderer.add_character(&gpu, ch));
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
                    // (The kinds came from the classes.)
                    let classes = self.classes.as_mut().expect("classes");
                    // Player faces +Z (yaw 0); a passive training dummy 14 units
                    // ahead faces back toward the player, within easy reach.
                    let mut world = crate::opening::open_world(
                        &gpu,
                        &mut renderer,
                        &scene,
                        classes,
                        crate::opening::Opening {
                            level_dir: &self.cast.level_dir,
                            faithful: self.faithful,
                            difficulty: &self.cast.difficulty,
                            player: &kind,
                            at: spawn_pos,
                        },
                        &mut |_| {},
                    );
                    let mut mission = None;
                    world.hud.subtitles_on = self.subtitles;
                    if self.mission {
                        // The level's own scripts populate it.
                        match (
                            crate::mission::Mission::load(&self.cast.level_dir),
                            self.classes.take(),
                        ) {
                            (Ok(mut m), Some(mut classes)) => {
                                m.state.vars.insert(
                                    "save_point".into(),
                                    mlua::Value::Integer(self.save_point),
                                );
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
                        let opponent = world.spawn(
                            &opponent_kind,
                            dummy_pos,
                            std::f32::consts::PI,
                            !self.spar,
                        );
                        if self.spar && !world.fight(opponent, 0) {
                            world.fighters[opponent].brain =
                                Some(crate::ai::Brain::Spar(crate::ai::Spar::new(0, 1)));
                        }
                    }
                    let audio = open_audio(&mut world, self.sound_law, settings.volume);
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
        let focused = window.has_focus();
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
            mouse: mouse::Mouse::new(focused),
            bindings: oni_config::Bindings::oni_defaults(),
            tuning: oni_config::InputTuning {
                invert_y: user.invert_mouse,
                ..oni_config::InputTuning::default()
            },
            move_speed,
            last: now,
            start: now,
            charview,
            char_model,
            play,
            settings,
            front,
            gamma: user.gamma,
            quit: false,
            movie: None,
        });
        // No level given: the main menu (after the ending movie, asked to).
        if let Some(st) = self.state.as_mut().filter(|_| self.front_end.is_some()) {
            let outro = self
                .front_end
                .as_ref()
                .filter(|_| self.outro)
                .map(|r| r.join("movies/outro.bik"));
            match outro.map(|p| crate::movie::Movie::open(&p)) {
                Some(Ok(movie)) => st.start_movie(movie, true),
                Some(Err(e)) => {
                    eprintln!("movie: {e}");
                    st.open_menu();
                }
                None => st.open_menu(),
            }
        }
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
                // The movie: a key ends it (`BFW_Bink.c`).
                if let Some(m) = st.movie.as_mut() {
                    if event.state == ElementState::Pressed && !event.repeat {
                        m.skip = true;
                    }
                    return;
                }
                if event.state == ElementState::Pressed && !event.repeat {
                    if let Some(f) = st.play.as_mut().and_then(|p| p.flow.as_mut()) {
                        f.key = true;
                    }
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    // The main menu up: its dialogs take the keys.
                    if !event.repeat && st.menu_key(code, event.state == ElementState::Pressed) {
                        return;
                    }
                    // Esc over mission play: the main menu (not in a
                    // cutscene, nor over a splash: `ONiGameState_ProcessMiscActions`).
                    if code == KeyCode::Escape
                        && event.state == ElementState::Pressed
                        && !event.repeat
                        && st.front.is_some()
                    {
                        let free = st.play.as_ref().is_some_and(|p| {
                            p.world.menu_allowed()
                                && p.flow.as_ref().is_some_and(|f| f.splash.is_none())
                        });
                        if free {
                            st.open_menu();
                        }
                        return;
                    }
                    // A data console's text up (modal: `ONiTC_HandleKeyDown`):
                    // Space for the next page, Esc to close; nothing else.
                    if let Some(pw) = st
                        .play
                        .as_mut()
                        .filter(|p| p.world.hud.text_console.is_some())
                    {
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
                                // (Letters typed: the cheat codes.)
                                _ => {
                                    if let Some(c) = format!("{code:?}")
                                        .strip_prefix("Key")
                                        .and_then(|k| k.chars().next())
                                        .filter(|_| format!("{code:?}").len() == 4)
                                    {
                                        pw.world.cheat_key(c.to_ascii_lowercase());
                                    }
                                }
                            }
                        }
                        st.keys.clear();
                        return;
                    }
                    // Space in a cutscene skips it (restored: as shipped the
                    // call is commented out, Q18).
                    if code == KeyCode::Space
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        if let Some(pw) = st.play.as_mut().filter(|p| p.world.cutscene.on) {
                            pw.world.skip_cutscene();
                        }
                    }
                    // F1: the pause screen (the cursor freed for its buttons).
                    if code == KeyCode::F1 && event.state == ElementState::Pressed && !event.repeat
                    {
                        if let Some(pw) = st.play.as_mut() {
                            pw.world.pause_toggle();
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
                st.mouse.focused = false;
                st.sync_mouse();
            }
            WindowEvent::Focused(true) => st.mouse.focused = true,
            WindowEvent::CursorEntered { .. } => st.mouse.inside = true,
            WindowEvent::CursorLeft { .. } => st.mouse.inside = false,
            WindowEvent::CursorMoved { position, .. } => {
                let scale = st.window.scale_factor() as f32;
                st.cursor = [position.x as f32 / scale, position.y as f32 / scale];
                st.menu_cursor();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if st.menu_open() {
                    let lines = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y.round() as i32,
                        winit::event::MouseScrollDelta::PixelDelta(p) => {
                            (p.y / 16.0).round() as i32
                        }
                    };
                    st.menu_wheel(lines);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                // The movie: so does a left click.
                if let Some(m) = st.movie.as_mut() {
                    m.skip |= pressed && button == MouseButton::Left;
                    return;
                }
                // The main menu up: the left button is on its dialogs.
                if st.menu_open() {
                    if button == MouseButton::Left {
                        st.menu_mouse(pressed);
                    }
                    return;
                }
                // The pause screen up: a click is on its buttons.
                if let Some(pw) = st.play.as_mut().filter(|p| p.world.hud.pause.is_some()) {
                    if pressed && button == MouseButton::Left {
                        let scale = st.window.scale_factor() as f32;
                        let (w, h) = (
                            st.config.width as f32 / scale,
                            st.config.height as f32 / scale,
                        );
                        if let Some(id) = pw.world.hud.pause_hit(w, h, st.cursor[0], st.cursor[1]) {
                            pw.world.pause_press(id, true);
                        }
                    }
                    return;
                }
                // A console's text up: a click is on its button, Next or
                // Close (`ONcTC_Btn_Next`, `_Close`).
                if let Some(pw) = st
                    .play
                    .as_mut()
                    .filter(|p| p.world.hud.text_console.is_some())
                {
                    if pressed && button == MouseButton::Left {
                        let scale = st.window.scale_factor() as f32;
                        let (w, h) = (
                            st.config.width as f32 / scale,
                            st.config.height as f32 / scale,
                        );
                        if pw
                            .world
                            .hud
                            .text_console_hit(w, h, st.cursor[0], st.cursor[1])
                        {
                            pw.world.text_console_click();
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
                if st.play.is_some() && !st.mouse.grabbed {
                    // Click into the window to grab the mouse (not a fire press).
                    if pressed && button == MouseButton::Left {
                        st.captured = true;
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
                // per tick) — only while the mouse is grabbed.
                if st.mouse.grabbed {
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
        if self.movie.is_some() {
            self.movie_tick(dt);
            return;
        }
        // The menus: their sound, and what they started.
        self.front_tick(dt);
        if self.play.is_none() && self.front.is_some() {
            return;
        }

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
                || self
                    .mouse_held
                    .iter()
                    .any(|&b| self.bindings.mouse_action(b) == Some(a))
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
        if let Some(pw) = self
            .play
            .as_mut()
            .filter(|p| p.flow.as_ref().is_some_and(|f| f.splash.is_some()))
        {
            audio_only(pw, dt);
        }
        if let Some(after) = self.splash_tick(dt) {
            self.after_splash(after);
            return;
        }
        if self
            .play
            .as_ref()
            .and_then(|p| p.flow.as_ref())
            .is_some_and(|f| f.splash.is_some())
        {
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
        // (A data console's text up, or the main menu: the game stands
        // still.)
        let frozen = pw.world.hud.modal() || self.front.as_ref().is_some_and(|f| f.menu.is_some());
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
                player.aim.lr = (player.aim.lr + turn)
                    .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
            } else {
                player.facing.adjust_desired(turn);
            }
            player.aim.ud = (player.aim.ud + dy * sens)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        }

        // (Slow motion: the game's clock runs at half the real one.)
        pw.accum += dt / pw.world.time_scale();
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
        // Else the ticks passed on the clock — at most 6 a frame
        // (`iComputeDeltaTicks`; a cutscene owes the rest to the next).
        let mut ticks = 0;
        if fast.is_none() && !frozen {
            let available = (pw.accum / TICK) as u32;
            pw.accum -= available as f32 * TICK;
            ticks = pw.world.frame_ticks(available);
        }
        loop {
            match fast.as_mut() {
                Some(0) => break,
                Some(n) => *n -= 1,
                None if ticks > 0 => ticks -= 1,
                None => break,
            }
            match pw.mission.as_mut() {
                Some((m, classes)) => {
                    let mut host = PlayHost {
                        world: &mut pw.world,
                        classes,
                        renderer: &mut self.renderer,
                        gpu: &self.gpu,
                    };
                    crate::mission::game_tick(&mut host, Some(m), controls);
                }
                None => {
                    pw.world.step(controls);
                    crate::game::persist(std::mem::take(&mut pw.world.persist));
                }
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
        if let Some(name) = pw.world.progress.pending_splash.take() {
            if let Some(t) = splash_texture(&flow.oni_root, flow.level, &name) {
                flow.splash = Some(Splash::new(t, After::Resume));
            }
        }
        match pw.world.progress.victory.take() {
            Some(true) => {
                eprintln!("game: level {} won", flow.level);
                pw.world.progress.saved = None;
                let after = match next_level(&flow.oni_root, flow.level) {
                    Some(n) if flow.level != 19 => After::Next(n),
                    _ => After::End,
                };
                flow.splash = Some(Splash::new(
                    splash_texture(&flow.oni_root, flow.level, "win_splash_screen")
                        .unwrap_or_default(),
                    after,
                ));
                splash_music(&mut pw.world.audio, flow.splash.as_mut(), "main_menu_win");
            }
            Some(false) => {
                eprintln!("game: level {} lost", flow.level);
                flow.splash = Some(Splash::new(
                    splash_texture(&flow.oni_root, flow.level, "fail_splash_screen")
                        .unwrap_or_default(),
                    After::Restart,
                ));
                splash_music(
                    &mut pw.world.audio,
                    flow.splash.as_mut(),
                    "loss_screen_music",
                );
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
        pw.world.cutscene.ticks = 0;
        Some(after)
    }

    /// The ending movie begins: its sound on a mixer of its own (the
    /// level's is gone), at the options' volume.
    fn start_movie(&mut self, movie: crate::movie::Movie, to_menu: bool) {
        let mixer = std::sync::Arc::new(std::sync::Mutex::new(crate::audio::mixer::Mixer::new(
            44100.0,
        )));
        let output = crate::audio::output::Output::open(mixer.clone());
        eprintln!("movie: the ending ({} frames)", movie.frame_count());
        self.movie = Some(MoviePlay {
            movie,
            mixer,
            output,
            volume: self.settings.volume,
            skip: false,
            to_menu,
        });
    }

    /// The movie, `dt` on: its picture to the frame due, its sound queued
    /// half a second at a time on the first stereo channel (played through
    /// by hand without an output device); at its end, or asked to stop,
    /// the game goes on as after the last level.
    fn movie_tick(&mut self, dt: f32) {
        use crate::audio::mixer::{Clip, Play, Side, Voice};
        let Some(m) = self.movie.as_mut() else { return };
        let done = match m.movie.advance(f64::from(dt)) {
            Ok(done) => done || m.skip,
            Err(e) => {
                eprintln!("movie: {e}");
                true
            }
        };
        if let Some((rate, channels)) = m.movie.audio_format() {
            let mut mixer = m.mixer.lock().unwrap();
            let room = mixer.voice(0).is_none_or(|v| v.next.is_none());
            if room {
                if let Some(samples) = m.movie.take_audio() {
                    let clip = std::sync::Arc::new(Clip {
                        name: "outro".into(),
                        rate: rate as f32,
                        channels,
                        samples,
                    });
                    let play = Play {
                        clip,
                        pitch: 1.0,
                        volume: 1.0,
                        looping: false,
                        part: 0,
                    };
                    match mixer.voice_mut(0) {
                        Some(v) => v.next = Some(play),
                        None => mixer.start(0, Voice::new(play, m.volume, (Side::Centre, 1.0))),
                    }
                }
            }
            if m.output.is_none() {
                let frames = (f64::from(dt) * f64::from(mixer.rate)) as usize;
                mixer.mix(None, frames);
            }
        }
        if done {
            let to_menu = m.to_menu;
            eprintln!("movie: over");
            self.movie = None;
            if to_menu {
                self.open_menu();
            } else {
                self.quit = true;
            }
        }
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
                // (Won: the cheat codes work from now on.)
                crate::game::mark_won_game();
                let to_menu = self.front.as_ref().is_some_and(|f| f.started_here);
                // The ending movie, then on (`Oni.c`: after level 19's win).
                let outro = oni_root.join("movies/outro.bik");
                match crate::movie::Movie::open(&outro) {
                    Ok(movie) => {
                        self.play = None;
                        self.start_movie(movie, to_menu);
                        return;
                    }
                    Err(e) => eprintln!("movie: no ending movie ({e})"),
                }
                // (Begun at the menu: back to it.)
                if to_menu {
                    self.play = None;
                    self.open_menu();
                } else {
                    self.quit = true;
                }
                return;
            }
            After::Restart => {
                let saved = pw.world.progress.saved.take();
                (level, saved.as_ref().map_or(0, |c| c.save_point), saved)
            }
            After::Next(n) => (n, 0, None),
        };
        // (The old level's sound stops with its world.)
        self.play = None;
        match self.load_level(&oni_root, target, save_point, saved) {
            Ok(()) => eprintln!("game: level {target}, save point {save_point}"),
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
        // The world's part (in play mode), shared with the scenario host.
        let mut world_frame = None;
        if let Some(pw) = &mut self.play {
            // The HUD, in the window's logical pixels (its size on a 640 × 480
            // screen of the time on a high-density display).
            let scale = self.window.scale_factor() as f32;
            let (w, h) = (
                self.config.width as f32 / scale,
                self.config.height as f32 / scale,
            );
            self.renderer.overlay = crate::frame::world_overlay(&mut pw.world, w, h);
            self.renderer.overlay_size = [w, h];
            pw.world.skip_drawn();
            // A splash screen over everything: black, its picture in the
            // middle (640 × 480), the fade's black over it.
            if let Some(sp) = pw.flow.as_ref().and_then(|f| f.splash.as_ref()) {
                self.renderer.overlay.push(crate::frame::black(w, h, 1.0));
                let (x, y) = ((w - 640.0) * 0.5, (h - 480.0) * 0.5);
                self.renderer.overlay.push(crate::render::OverlayDraw {
                    texture: Some(sp.texture.clone()),
                    tris: quad([x, y, x + 640.0, y + 480.0]),
                    color: [1.0; 4],
                    additive: false,
                });
                self.renderer
                    .overlay
                    .push(crate::frame::black(w, h, sp.blanket()));
            }
            world_frame = Some(crate::frame::WorldFrame::build(
                &pw.world,
                &mut self.renderer,
                &self.gpu,
                self.cam.eye,
                true,
            ));
        } else {
            self.renderer.scene.magic.clear();
        }
        self.renderer.gamma = self.gamma;
        // The movie over everything: on black, as big as fits, centred
        // (`BKcScale_Fill_Window`).
        if let Some(m) = self.movie.as_mut() {
            let (mw, mh) = m.movie.size();
            if let Some(rgba) = m.movie.take_picture() {
                self.renderer.movie_frame(&self.gpu, mw, mh, rgba);
            }
            let scale = self.window.scale_factor() as f32;
            let (w, h) = (
                self.config.width as f32 / scale,
                self.config.height as f32 / scale,
            );
            let s = (w / mw as f32).min(h / mh as f32);
            let (dw, dh) = (mw as f32 * s, mh as f32 * s);
            let (x, y) = ((w - dw) * 0.5, (h - dh) * 0.5);
            self.renderer.overlay = vec![
                crate::render::OverlayDraw {
                    texture: None,
                    tris: quad([0.0, 0.0, w, h]),
                    color: [0.0, 0.0, 0.0, 1.0],
                    additive: false,
                },
                crate::render::OverlayDraw {
                    texture: (m.movie.frame().is_some())
                        .then(|| std::path::PathBuf::from(crate::render::MOVIE_TEXTURE)),
                    tris: quad([x, y, x + dw, y + dh]),
                    color: [1.0; 4],
                    additive: false,
                },
            ];
            self.renderer.overlay_size = [w, h];
        }
        // The main menu over everything (black behind it).
        if let Some(o) = self.menu_overlay() {
            let scale = self.window.scale_factor() as f32;
            self.renderer.overlay = o;
            self.renderer.overlay_size = [
                self.config.width as f32 / scale,
                self.config.height as f32 / scale,
            ];
        }
        // The pointer over the game's own screens: the original's arrow.
        self.sync_mouse();
        self.draw_arrow();
        // The static viewer: the single character, looping over elapsed time.
        let posed: Vec<Mat4> = match (&self.play, &self.charview) {
            (None, Some(cv)) => {
                let dur = cv.clip.as_ref().map(|c| c.duration).unwrap_or(1.0);
                let t = self.start.elapsed().as_secs_f32() % dur.max(0.001);
                cv.bones(t)
            }
            _ => Vec::new(),
        };
        let draws: Vec<crate::render::CharDraw> = match (&self.play, &world_frame, self.char_model)
        {
            (Some(pw), Some(f), _) => f.draws(&pw.world),
            (None, _, Some(m)) if !posed.is_empty() => vec![crate::render::CharDraw {
                model: m,
                bones: &posed,
                see_through: false,
                alpha: 1.0,
                shade: None,
            }],
            _ => Vec::new(),
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

use crate::frame::quad;

/// The win or fail splash's music (`ONrGameState_SplashScreen`): every
/// other sound stopped, `music` started silent and faded in over a second.
fn splash_music(
    audio: &mut crate::audio::Audio,
    splash: Option<&mut crate::game::Splash>,
    music: &'static str,
) {
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
