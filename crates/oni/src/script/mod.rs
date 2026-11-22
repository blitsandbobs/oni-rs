//! A tiny headless Lua driver for the engine (dev/test harness).
//!
//! This exists so scenario validation — "spawn Konoko, run forward 1s, put the
//! camera here, screenshot" — lives in `.lua` files instead of accreting bespoke
//! CLI flags and throwaway Rust test binaries. It drives the real engine (the
//! [`World`] of fighters + physics + the locomotion/combat state machine +
//! offscreen render); the binding surface grows as the engine does, and later
//! becomes the substrate for the gameplay-script layer (E4).
//!
//! Lua is confined to `oni` (the app/harness crate); the portable
//! preservation crates (`oni-geom`, `oni-convert`) stay dependency-light.
//!
//! The functions themselves live by area in this module's children, each
//! made a Lua global by [`Reg::def`]; `docs/lua-api.md` lists them all
//! (generated from their comments by `build.rs`).

mod ai;
mod effects;
mod fighters;
mod harness;
mod level;
mod mission;
mod player;
mod ui;
mod weapons;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use glam::Vec3;
use mlua::Lua;

use crate::anim::input::key;
use crate::classes::Classes;
use crate::quirks::Quirks;
use crate::render::{Gpu, Renderer};
use crate::world::{CharKind, World};
use crate::CharView;

/// Everything a script drives: one level, a world of fighters, a camera, and a
/// reusable offscreen renderer.
struct ScriptHost {
    gpu: Gpu,
    renderer: Renderer,
    format: wgpu::TextureFormat,
    scene: crate::gltf::Scene,
    world: World,
    /// The character classes, loaded (and uploaded) on demand.
    classes: Classes,
    /// The class `spawn` uses unless told otherwise.
    opponent: String,
    /// Whether frames draw the decals (`show_decals`).
    show_decals: bool,
    /// How many magic effects the last screenshot drew.
    magic_drawn: usize,
    /// Controls the player holds across steps (`anim::input::key` bits).
    held: u32,
    /// Buttons tapped for the next step's first tick only.
    taps: u32,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    /// The level's own scripts, once begun (`mission_begin()`).
    mission: Option<crate::mission::Mission>,
    /// The converted assets' root (`…/oni`).
    oni_root: std::path::PathBuf,
    /// The main menu up (`menu_open`), and its data.
    menu: Option<(crate::hud::menu::Data, crate::hud::menu::MainMenu)>,
    /// Where `menu_press()` holds the left button down (screen, 640 × 480),
    /// and the timer's clock it advances (`menu_timer`).
    menu_held: Option<([f32; 2], std::time::Instant)>,
    /// The level-load dialog shown alone (`menu_loading`).
    loading: Option<(crate::hud::menu::Data, crate::hud::menu::Window)>,
}

impl crate::mission::Host for ScriptHost {
    fn world(&mut self) -> &mut World {
        &mut self.world
    }
    fn kind(&mut self, class: &str) -> Result<CharKind, String> {
        ScriptHost::kind(self, class)
    }
    fn melee(&mut self, id: u16) -> Option<Rc<crate::ai::melee::profile::Profile>> {
        self.classes.melee_by_id(id)
    }
    fn clip(&mut self, name: &str) -> Option<std::rc::Rc<crate::anim::collection::AnimCollection>> {
        self.classes.clip(name)
    }
    fn classes(&self) -> Option<&Classes> {
        Some(&self.classes)
    }
}

impl ScriptHost {
    fn new(
        scene: crate::gltf::Scene,
        charview: CharView,
        particles: &[crate::particles::Particle],
        mut classes: Classes,
        cast: crate::Cast,
        faithful: bool,
    ) -> Result<Self, String> {
        let gpu = pollster::block_on(Gpu::new(None));
        eprintln!("  GPU: {}", gpu.adapter.get_info().name);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = Renderer::new(&gpu, &scene, particles, format);
        let kind = classes.kind(&cast.player, |ch| renderer.add_character(&gpu, ch))?;
        // Player = fighter 0, facing +Z.
        let world = crate::opening::open_world(
            &gpu,
            &mut renderer,
            &scene,
            &mut classes,
            crate::opening::Opening {
                level_dir: &cast.level_dir,
                faithful,
                difficulty: &cast.difficulty,
                player: &kind,
                at: charview.placement.transform_point3(Vec3::ZERO),
            },
            &mut |_| {},
        );
        let oni_root = cast.level_dir.join("../..");

        Ok(ScriptHost {
            gpu,
            renderer,
            format,
            scene,
            world,
            classes,
            opponent: cast.opponent.unwrap_or_else(|| cast.player.clone()),
            show_decals: true,
            magic_drawn: 0,
            held: 0,
            taps: 0,
            eye: None,
            target: None,
            mission: None,
            oni_root,
            menu: None,
            menu_held: None,
            loading: None,
        })
    }

    /// The spawnable kind for class `name` (loaded on first use).
    fn kind(&mut self, name: &str) -> Result<CharKind, String> {
        let (renderer, gpu) = (&mut self.renderer, &self.gpu);
        self.classes
            .kind(name, |ch| renderer.add_character(gpu, ch))
    }

    /// Advances `n` ticks: drive the player from the held controls (plus any
    /// tapped buttons on the first tick), the dummies idle, then resolve melee
    /// hits — exactly as play mode does.
    fn step(&mut self, n: u32) {
        let mut taps = std::mem::take(&mut self.taps);
        for _ in 0..n {
            let controls = self.held | taps;
            taps = 0;
            // (A data console's text up: the game stands still.)
            if self.world.hud.modal() {
                // (The sound runs on.)
                self.world.audio.tick(Some(&self.world.quads));
                continue;
            }
            // (Each tick a frame: a cutscene being skipped runs 32 a frame.)
            let ticks = self.world.skip_frame().unwrap_or(1);
            for _ in 0..ticks {
                let mut m = self.mission.take();
                crate::mission::game_tick(self, m.as_mut(), controls);
                self.mission = m;
            }
            self.world.skip_drawn();
        }
    }

    /// Sets or clears held control bits.
    fn hold(&mut self, bits: u32, on: bool) {
        if on {
            self.held |= bits;
        } else {
            self.held &= !bits;
        }
    }

    fn screenshot(&mut self, path: &Path, w: u32, h: u32) {
        let rgba = self.frame_rgba(w, h);
        crate::write_png(path, w, h, &rgba);
        eprintln!("wrote {} ({w}x{h})", path.display());
    }

    /// A frame as the player would see it, drawn offscreen and read back.
    fn frame_rgba(&mut self, w: u32, h: u32) -> Vec<u8> {
        let aspect = w as f32 / h as f32;
        // (The screen's shape: the lens flares' view test reads it, as in
        // the play app.)
        self.world.camera.aspect = aspect;
        // (What the player sees: the game camera — once it is placed.)
        let mut cam = if self.world.camera.placed {
            self.world.camera.render_camera(aspect)
        } else {
            crate::default_camera(&self.scene, aspect)
        };
        match (self.eye, self.target) {
            (Some(e), Some(tg)) => cam.look_at(e, tg),
            (Some(e), None) => cam.look_at(e, self.scene.center()),
            _ => {}
        }
        // (The 2D screens scaled as the game scales them, #97.)
        let ui = crate::display::ui_scale(None, h, 1.0);
        let (uw, uh) = (w as f32 / ui, h as f32 / ui);
        self.renderer.overlay = match (&self.loading, &self.menu) {
            (Some((data, d)), _) => std::iter::once(crate::frame::black(uw, uh, 1.0))
                .chain(d.draw(&data.look, uw, uh))
                .collect(),
            (None, Some((data, m))) => m.draw(data, uw, uh),
            (None, None) => crate::frame::world_overlay(&mut self.world, uw, uh),
        };
        self.renderer.overlay_size = [uw, uh];
        let frame = crate::frame::WorldFrame::build(
            &self.world,
            &mut self.renderer,
            &self.gpu,
            cam.eye,
            self.show_decals,
        );
        self.magic_drawn = self.renderer.scene.magic.len();
        let draws = frame.draws(&self.world);
        let basis = cam.billboard_basis();
        crate::render_to_rgba(
            &self.gpu,
            &mut self.renderer,
            self.format,
            (w, h),
            cam.view_proj(),
            &draws,
            basis,
        )
    }
}

/// Runs a Lua scenario script headlessly against the given level + character.
pub fn run_script(
    path: &Path,
    scene: crate::gltf::Scene,
    charview: CharView,
    particles: Vec<crate::particles::Particle>,
    classes: Option<Classes>,
    cast: crate::Cast,
    faithful: bool,
) {
    let Some(classes) = classes else {
        eprintln!("--script requires the character classes (none loaded)");
        return;
    };
    let host = match ScriptHost::new(scene, charview, &particles, classes, cast, faithful) {
        Ok(h) => Rc::new(RefCell::new(h)),
        Err(e) => {
            eprintln!("script setup error: {e}");
            return;
        }
    };
    let lua = Lua::new();
    if let Err(e) = register(&lua, &host) {
        eprintln!("script setup error: {e}");
        return;
    }
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read script {}: {e}", path.display());
            return;
        }
    };
    if let Err(e) = lua.load(&src).set_name(path.to_string_lossy()).exec() {
        eprintln!("script error: {e}");
        // (A failed scenario fails the run: callers check the exit code.)
        std::process::exit(1);
    }
}

/// Binds the host methods as Lua global functions.
fn register(lua: &Lua, host: &Rc<RefCell<ScriptHost>>) -> mlua::Result<()> {
    let r = Reg {
        lua,
        host: host.clone(),
    };
    player::register(&r)?;
    fighters::register(&r)?;
    ai::register(&r)?;
    weapons::register(&r)?;
    level::register(&r)?;
    effects::register(&r)?;
    mission::register(&r)?;
    ui::register(&r)?;
    harness::register(&r)?;
    Ok(())
}

/// A matrix as Lua sees it: its axes and place.
fn mat(lua: &mlua::Lua, m: glam::Mat4) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    for (k, c) in [
        ("x", m.x_axis),
        ("y", m.y_axis),
        ("z", m.z_axis),
        ("at", m.w_axis),
    ] {
        t.set(k, vec![c.x, c.y, c.z])?;
    }
    Ok(t)
}

/// Registers scenario functions: [`Reg::def`] makes one a Lua global.
struct Reg<'a> {
    lua: &'a Lua,
    host: Rc<RefCell<ScriptHost>>,
}

impl Reg<'_> {
    /// Lua function `name`: `f` called with the host, borrowed for the call
    /// (no scenario function calls back into Lua, so none is ever re-entered).
    fn def<A, R>(
        &self,
        name: &str,
        f: impl Fn(&Lua, &mut ScriptHost, A) -> mlua::Result<R> + 'static,
    ) -> mlua::Result<()>
    where
        A: mlua::FromLuaMulti,
        R: mlua::IntoLuaMulti,
    {
        let h = self.host.clone();
        let fun = self
            .lua
            .create_function(move |lua, a: A| f(lua, &mut h.borrow_mut(), a))?;
        self.lua.globals().set(name, fun)
    }
}
