//! A tiny headless Lua driver for the engine (dev/test harness).
//!
//! This exists so scenario validation — "spawn Konoko, run forward 1s, put the
//! camera here, screenshot" — lives in `.lua` files instead of accreting bespoke
//! CLI flags and throwaway Rust test binaries. It drives the real engine (the
//! [`World`] of fighters + physics + the locomotion/combat state machine +
//! offscreen render); the binding surface grows as the engine does, and later
//! becomes the substrate for the gameplay-script layer (E4).
//!
//! Lua is confined to `oni-view` (the app/harness crate); the portable
//! preservation crates (`oni-geom`, `oni-convert`) stay dependency-light.
//!
//! ## Lua API (all headless)
//! Movement/combat below act on the **player** (fighter 0):
//! - `input(forward, backward)` — set the move keys (W/S)
//! - `strafe(left, right)` — set the strafe keys (A/D sidestep)
//! - `turn_keys(left, right)` — hold the turn keys (lying down: roll up)
//! - `action()` — tap the action key (in a fight: taunt)
//! - `walk(on)` / `crouch(on)` — hold the walk / crouch modifier
//! - `jump()` / `airborne()` — tap jump (one tick) / read airborne state
//! - `punch()` / `kick()` / `crouch_tap()` — tap a button for one tick
//! - `fire(n)` — tap fire 1/2/3 (mouse left/right/middle; unarmed: punch/kick/crouch)
//! - `state()` / `anim_type()` — the body state / current animation type
//! - `faithful(on)` — reproduce the original's known bugs (`docs/QUIRKS.md`)
//! - `seed(n)` — reseed the weighted animation pick (repeatable runs)
//! - `type_of(id)` / `state_of(id)` / `facing_of(id)` / `dead(id)` / `set_hp(id, n)` — any fighter
//! - `knock(id, anim_type)` / `set_facing(id, deg)` — test hooks on any fighter
//! - `patrol(id, {{x,z},…} [, pace, loop])` / `route_done(id)` — walk an AI route
//! - `ai_order(id, dir [, pace, face_deg])` — hold fixed AI orders (forward/backpedal/left/right/stop)
//! - `spar(id, target)` — make a fighter fight another (sparring AI)
//! - `facing(deg)` — snap the body + target to an absolute yaw
//! - `turn(delta_deg)` — nudge the facing *target* (body slews toward it)
//! - `facing_deg()` / `desired_deg()` — read the body yaw / target yaw
//! - `pos()` — the player's feet `x, y, z`
//! - `grounded()` — whether the player is on the ground
//! - `stitch()` — the current clip-blend weight (0 = not blending)
//! - `anim()` — the current clip name
//!
//! World / multi-character:
//! - `spawn(x, y, z [, facing_deg [, class]])` — spawn a training dummy of a
//!   character class (default: `--opponent`, else the player's), returns its id
//! - `class_of(id)` — a fighter's character class name
//! - `watch(id)` / `alert_of(id)` / `contact_of(id, other)` / `team(id, name)` / `noise(x, y, z, kind, volume [, maker])` — perception
//! - `probe(id, deg, dist)` — free distance before a wall, distance to a ledge (or nil)
//! - `count()` — number of fighters (player + dummies)
//! - `hp(id)` — a fighter's hit points (0 = player)
//! - `pos_of(id)` — a fighter's feet `x, y, z`
//! - `anim_of(id)` — a fighter's current clip name
//!
//! Driver:
//! - `step(n)` — advance `n` 60 Hz ticks (every fighter + hit resolution)
//! - `cam(ex,ey,ez, tx,ty,tz)` — place the camera (eye, look-at target)
//! - `screenshot(path [, w, h])` — render one frame to a PNG

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use mlua::Lua;

use crate::anim::input::key;
use crate::classes::Classes;
use crate::quirks::Quirks;
use crate::render::{CharDraw, Gpu, Renderer};
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
    /// Each fighter's class name, by fighter index.
    class_names: Vec<String>,
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
    /// The level's own scripts, once begun (`mission_begin`).
    mission: Option<crate::mission::Mission>,
    /// The converted assets' root (`…/oni`).
    oni_root: std::path::PathBuf,
    /// The main menu up (`menu_open`), and its data.
    menu: Option<(crate::hud::menu::Data, crate::hud::menu::MainMenu)>,
    /// Where `menu_press` holds the left button down (screen, 640 × 480),
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
        let k = ScriptHost::kind(self, class)?;
        self.class_names.push(class.to_string());
        Ok(k)
    }
    fn melee(&self, id: u16) -> Option<Rc<crate::ai::melee::profile::Profile>> {
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
        let spawn_pos = charview.placement.transform_point3(Vec3::ZERO);
        let mut world = World::new();
        if faithful {
            world.set_quirks(Quirks::FAITHFUL);
        }
        world.difficulty = classes.difficulty(&cast.difficulty);
        world.nav = crate::nav::Navigation::load(&cast.level_dir)
            .ok()
            .map(Rc::new);
        world.load_level(&cast.level_dir);
        world.attach_corpse_models(|c| {
            classes
                .kind(c, |ch| renderer.add_character(&gpu, ch))
                .ok()
                .map(|k| k.model)
        });
        world.attach_doors(crate::door_meshes(&scene));
        world.attach_glass(crate::glass_parts(&scene));
        world.attach_weapons(&cast.level_dir.join("../../common"));
        // (After the weapons: that is where the level quads load.)
        renderer.attach_quads(&scene, &world.quads);
        renderer.attach_texture_anims(&gpu, world.texture_anims());
        if world.nav.is_none() {
            eprintln!("  (no navigation data for this level)");
        }
        // Player = fighter 0, facing +Z.
        world.spawn(&kind, spawn_pos, 0.0, false);
        let oni_root = cast.level_dir.join("../..");

        Ok(ScriptHost {
            gpu,
            renderer,
            format,
            scene,
            world,
            classes,
            opponent: cast.opponent.unwrap_or_else(|| cast.player.clone()),
            class_names: vec![cast.player],
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
                self.world.step(controls);
                if let Some(mut m) = self.mission.take() {
                    m.tick(self);
                    self.mission = Some(m);
                }
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
        let aspect = w as f32 / h as f32;
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
        self.renderer.door_motion = self.world.door_motions();
        self.renderer.beams = self.world.beams.clone();
        self.renderer.sprites = self.world.sprites();
        self.renderer.decals = if self.show_decals {
            self.world.decal_draws(cam.eye)
        } else {
            Vec::new()
        };
        self.renderer.hidden_refs = self.world.env_hidden.clone();
        self.renderer.env_shades = self.world.env_shades.clone();
        self.renderer.door_shades = self.world.door_shades.clone();
        self.renderer.broken_glass = self.world.broken_glass.clone();
        self.renderer.jello = self.world.jello.clone();
        self.renderer.jello_doors = self.world.jello_doors.iter().copied().collect();
        self.renderer.anim_time = self.world.game_tick();
        self.renderer.eye = cam.eye;
        self.renderer.fog = self.world.fog.fog;
        self.renderer.overlay = match (&self.loading, &self.menu) {
            (Some((data, d)), _) => {
                let (w, h) = (w as f32, h as f32);
                let black = crate::render::OverlayDraw {
                    texture: None,
                    tris: vec![
                        [0.0, 0.0, 0.0, 0.0],
                        [w, 0.0, 1.0, 0.0],
                        [w, h, 1.0, 1.0],
                        [0.0, 0.0, 0.0, 0.0],
                        [w, h, 1.0, 1.0],
                        [0.0, h, 0.0, 1.0],
                    ],
                    color: [0.0, 0.0, 0.0, 1.0],
                    additive: false,
                };
                std::iter::once(black)
                    .chain(d.draw(&data.look, w, h))
                    .collect()
            }
            (None, Some((data, m))) => m.draw(data, w as f32, h as f32),
            (None, None) => self.world.hud_draw(w as f32, h as f32),
        };
        self.renderer.overlay_size = [w as f32, h as f32];
        let bones: Vec<Vec<Mat4>> = self.world.fighters.iter().map(|f| f.bones()).collect();
        self.renderer.magic = self.world.magic_draws(&bones);
        self.magic_drawn = self.renderer.magic.len();
        let mut guns: Vec<crate::render::ModelDraw> = self
            .world
            .gun_draws()
            .into_iter()
            .chain(self.world.particle_models())
            .chain(self.world.turret_draws())
            .chain(self.world.powerup_draws())
            .filter_map(|(path, m)| {
                Some((
                    self.renderer.model_from(&self.gpu, &path)?,
                    vec![m],
                    false,
                    1.0,
                    None,
                ))
            })
            .collect();
        // (Placed objects, some lit flat.)
        for (path, m, shade) in self.world.object_draws() {
            if let Some(k) = self.renderer.model_from(&self.gpu, &path) {
                guns.push((k, vec![m], false, 1.0, shade));
            }
        }
        // (Objects in the camera's way, see-through.)
        for (path, m) in self.world.object_jello_draws() {
            if let Some(k) = self.renderer.model_from(&self.gpu, &path) {
                guns.push((k, vec![m], true, 1.0, None));
            }
        }
        // (Guns held by bodies fading out.)
        for (path, m, a) in self.world.faded_gun_draws() {
            if let Some(k) = self.renderer.model_from(&self.gpu, &path) {
                guns.push((k, vec![m], false, a, None));
            }
        }
        // (Consoles' screens, textured by their state.)
        for (path, tex, m) in self.world.console_draws() {
            if let Some(k) = self.renderer.model_textured(&self.gpu, &path, &tex) {
                guns.push((k, vec![m], false, 1.0, None));
            }
        }
        let draws: Vec<CharDraw> = self
            .world
            .fighters
            .iter()
            .zip(&bones)
            .map(|(f, b)| f.draw(b))
            .chain(self.world.corpse_draws())
            .chain(
                guns.iter()
                    .map(|(m, b, see_through, alpha, shade)| CharDraw {
                        model: *m,
                        bones: b,
                        see_through: *see_through,
                        alpha: *alpha,
                        shade: *shade,
                    }),
            )
            .collect();
        let basis = cam.billboard_basis();
        let rgba = crate::render_to_rgba(
            &self.gpu,
            &mut self.renderer,
            self.format,
            (w, h),
            cam.view_proj(),
            &draws,
            basis,
        );
        crate::write_png(path, w, h, &rgba);
        eprintln!("wrote {} ({w}x{h})", path.display());
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
    let g = lua.globals();

    let h = host.clone();
    g.set(
        "input",
        lua.create_function(move |_, (f, b): (bool, bool)| {
            let mut host = h.borrow_mut();
            host.hold(key::FORWARD, f);
            host.hold(key::BACKWARD, b);
            Ok(())
        })?,
    )?;

    // feet_offset() → how far the player's posed body's lowest point (its
    // parts' bounding spheres, arms left out) is above its location.
    let h = host.clone();
    g.set(
        "feet_offset",
        lua.create_function(move |_, ()| {
            let host = h.borrow();
            let Some(f) = host.world.player() else {
                return Ok(0.0);
            };
            Ok(f.feet_offset())
        })?,
    )?;

    // Holds (or lets go of) jump (a press, then held: the higher jump).
    let h = host.clone();
    g.set(
        "hold_jump",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().hold(key::JUMP, on);
            Ok(())
        })?,
    )?;

    // Holds (or lets go of) fire 1.
    let h = host.clone();
    g.set(
        "hold_fire",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().hold(key::FIRE1, on);
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "turn_keys",
        lua.create_function(move |_, (l, r): (bool, bool)| {
            let mut host = h.borrow_mut();
            host.hold(key::TURN_LEFT, l);
            host.hold(key::TURN_RIGHT, r);
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "action",
        lua.create_function(move |_, ()| {
            h.borrow_mut().taps |= key::ACTION;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "strafe",
        lua.create_function(move |_, (l, r): (bool, bool)| {
            let mut host = h.borrow_mut();
            host.hold(key::STEP_LEFT, l);
            host.hold(key::STEP_RIGHT, r);
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "walk",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().hold(key::WALK, on);
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "crouch",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().hold(key::CROUCH, on);
            Ok(())
        })?,
    )?;

    // Tap a fire button (1 = left mouse, 2 = right, 3 = middle by default):
    // unarmed, these are punch / kick / crouch.
    let h = host.clone();
    g.set(
        "fire",
        lua.create_function(move |_, n: u8| {
            let bit = match n {
                1 => key::FIRE1,
                2 => key::FIRE2,
                3 => key::FIRE3,
                _ => 0,
            };
            h.borrow_mut().taps |= bit;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "crouch_tap",
        lua.create_function(move |_, ()| {
            h.borrow_mut().taps |= key::CROUCH;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "faithful",
        // faithful(on) sets the mode; faithful() tells it.
        lua.create_function(move |_, on: Option<bool>| {
            if let Some(on) = on {
                h.borrow_mut()
                    .world
                    .set_quirks(if on { Quirks::FAITHFUL } else { Quirks::FIXED });
            }
            Ok(h.borrow().world.quirks.faithful)
        })?,
    )?;

    let h = host.clone();
    g.set(
        "seed",
        lua.create_function(move |_, n: u32| {
            for f in &h.borrow().world.fighters {
                f.motion.coll.seed(n);
            }
            Ok(())
        })?,
    )?;

    // Per-fighter inspection (any id) and a test hook to set health.
    // Whether fighter `id` is still in the level (not removed).
    let h = host.clone();
    g.set(
        "present",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow().world.fighters.get(id).is_some_and(|f| !f.gone))
        })?,
    )?;

    // Deals damage to fighter `id` (no reaction unless it dies), with a
    // small shove along (dx, dz) if given — the blow's way, attacker to it.
    let h = host.clone();
    g.set(
        "damage",
        lua.create_function(
            move |_, (id, n, dx, dz): (usize, i32, Option<f32>, Option<f32>)| {
                if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                    let shove = Vec3::new(dx.unwrap_or(0.0), 0.0, dz.unwrap_or(0.0))
                        .normalize_or_zero()
                        * 0.01;
                    f.take_damage(n, shove);
                }
                Ok(())
            },
        )?,
    )?;
    // lift(id, by, ticks): fighter `id` is lifted toward `by` as a weapon's
    // "picked up" damage does, for `ticks` (at most 60).
    let h = host.clone();
    g.set(
        "lift",
        lua.create_function(move |_, (id, by, ticks): (usize, usize, u16)| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.lift.timer = (f.lift.timer + ticks).min(60);
                f.lift.owner = Some(by);
            }
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "set_hp",
        lua.create_function(move |_, (id, v): (usize, i32)| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.hp = v;
            }
            Ok(())
        })?,
    )?;

    // AI: walk fighter `id` along `points` ({{x, z}, …}) at `pace` ("run",
    // "walk" or "creep"), optionally looping; `route_done(id)` reads progress.
    let h = host.clone();
    g.set(
        "patrol",
        lua.create_function(
            move |_,
                  (id, points, pace, looping): (
                usize,
                mlua::Table,
                Option<String>,
                Option<bool>,
            )| {
                let mut pts = Vec::new();
                for p in points.sequence_values::<mlua::Table>() {
                    let p = p?;
                    pts.push(Vec3::new(p.get(1)?, 0.0, p.get(2)?));
                }
                let pace = match pace.as_deref() {
                    Some("walk") => crate::ai::Pace::Walk,
                    Some("creep") => crate::ai::Pace::Creep,
                    _ => crate::ai::Pace::Run,
                };
                if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                    let route = crate::ai::Route::new(pts, pace, looping.unwrap_or(false));
                    f.brain = Some(crate::ai::Brain::Route(route));
                }
                Ok(())
            },
        )?,
    )?;

    // AI: hold fixed orders — direction "forward" | "backpedal" | "left" |
    // "right" | "stop", pace "run" | "walk" | "creep", optional facing (deg).
    let h = host.clone();
    g.set(
        "ai_order",
        lua.create_function(
            move |_, (id, dir, pace, face): (usize, String, Option<String>, Option<f32>)| {
                use crate::ai::{Brain, Direction, Orders, Pace};
                let direction = match dir.as_str() {
                    "forward" => Direction::Forward,
                    "backpedal" => Direction::Backpedal,
                    "left" => Direction::SidestepLeft,
                    "right" => Direction::SidestepRight,
                    _ => Direction::Stopped,
                };
                let pace = match pace.as_deref() {
                    Some("walk") => Pace::Walk,
                    Some("creep") => Pace::Creep,
                    _ => Pace::Run,
                };
                let face = face.map(f32::to_radians);
                if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                    f.brain = Some(Brain::Hold(Orders {
                        direction,
                        pace,
                        face,
                    }));
                }
                Ok(())
            },
        )?,
    )?;

    // AI: make fighter `id` fight fighter `target` (a sparring partner).
    let h = host.clone();
    g.set(
        "spar",
        lua.create_function(move |_, (id, target): (usize, usize)| {
            let mut host = h.borrow_mut();
            if let Some(f) = host.world.fighters.get_mut(id) {
                f.passive = false;
            }
            // The class's melee AI; the stand-in sparring partner if it has none.
            if !host.world.fight(id, target) {
                if let Some(f) = host.world.fighters.get_mut(id) {
                    f.brain = Some(crate::ai::Brain::Spar(crate::ai::Spar::new(
                        target,
                        id as u32 + 1,
                    )));
                }
            }
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "route_done",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .is_some_and(|f| match &f.brain {
                    Some(crate::ai::Brain::Route(r)) => r.finished(),
                    _ => true,
                }))
        })?,
    )?;

    // Test hook: turn any fighter to an absolute yaw (body and target).
    let h = host.clone();
    g.set(
        "set_facing",
        lua.create_function(move |_, (id, deg): (usize, f32)| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.facing = crate::control::Facing::new(deg.to_radians());
            }
            Ok(())
        })?,
    )?;

    // Test hook: force a reaction (e.g. a knockdown) on any fighter.
    let h = host.clone();
    g.set(
        "knock",
        lua.create_function(move |_, (id, kind): (usize, u16)| {
            let mut host = h.borrow_mut();
            if let Some(f) = host.world.fighters.get_mut(id) {
                f.pending_reaction = kind;
                f.apply_reaction();
            }
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "type_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map(|f| f.motion.kind)
                .unwrap_or(0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "state_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map(|f| f.motion.state)
                .unwrap_or(0))
        })?,
    )?;

    // image_mean(path, x0, y0, x1, y1) → r, g, b (0–1): a saved screenshot's
    // mean colour over a region (fractions of its width and height).
    g.set(
        "image_mean",
        lua.create_function(
            move |_, (path, x0, y0, x1, y1): (String, f32, f32, f32, f32)| {
                let (w, h, rgba) = crate::render::load_png(std::path::Path::new(&path))
                    .map_err(mlua::Error::external)?;
                let (ax, bx) = ((x0 * w as f32) as u32, ((x1 * w as f32) as u32).min(w));
                let (ay, by) = ((y0 * h as f32) as u32, ((y1 * h as f32) as u32).min(h));
                let mut sum = [0.0f64; 3];
                let mut n = 0.0;
                for y in ay..by {
                    for x in ax..bx {
                        let k = ((y * w + x) * 4) as usize;
                        for c in 0..3 {
                            sum[c] += f64::from(rgba[k + c]) / 255.0;
                        }
                        n += 1.0;
                    }
                }
                let n = f64::max(n, 1.0);
                Ok((sum[0] / n, sum[1] / n, sum[2] / n))
            },
        )?,
    )?;
    // fog() → r, g, b, start, end: the fog now (screen depth: −1 near, 1 far).
    let h = host.clone();
    g.set(
        "fog",
        lua.create_function(move |_, ()| {
            let f = h.borrow().world.fog.fog;
            Ok((f.color[0], f.color[1], f.color[2], f.start, f.end))
        })?,
    )?;
    // jello_objects() → {name, …}: the placed objects see-through this tick
    // (in the camera's way).
    let h = host.clone();
    g.set(
        "jello_objects",
        lua.create_function(move |_, ()| {
            let host = h.borrow();
            let w = &host.world;
            let names = w.object_states();
            Ok(w.jello_objects
                .iter()
                .filter_map(|&k| names.get(k).map(|n| n.0.clone()))
                .collect::<Vec<_>>())
        })?,
    )?;
    // jello_doors() → {quads, leaves}: how many of the see-through quads this
    // tick are closed doors' own (their "gunk"), and the leaves of open or
    // moving doors see-through as objects ("door id:leaf").
    let h = host.clone();
    g.set(
        "jello_doors",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let w = &host.world;
            let t = lua.create_table()?;
            t.set(
                "quads",
                w.jello
                    .iter()
                    .filter(|&&q| {
                        w.quads
                            .quads
                            .get(q as usize)
                            .is_some_and(|q| q.object_tag >> 24 == 3)
                    })
                    .count(),
            )?;
            let leaves: Vec<String> = w
                .jello_doors
                .iter()
                .filter_map(|&(o, l)| {
                    w.doors
                        .iter()
                        .find(|d| d.object_id == o)
                        .map(|d| format!("{}:{l}", d.id))
                })
                .collect();
            t.set("leaves", leaves)?;
            Ok(t)
        })?,
    )?;
    // object_in_way(ax, ay, az, bx, by, bz) → whether the segment between
    // the points meets an animating object (a placed object, an open or
    // moving door's leaf: `AMrRayToObject`, as a lens flare's test asks).
    let h = host.clone();
    g.set(
        "object_in_way",
        lua.create_function(
            move |_, (ax, ay, az, bx, by, bz): (f32, f32, f32, f32, f32, f32)| {
                Ok(h.borrow()
                    .world
                    .segment_meets_object(Vec3::new(ax, ay, az), Vec3::new(bx, by, bz)))
            },
        )?,
    )?;
    // overall_volume() → the sound's overall volume now (0–1).
    let h = host.clone();
    g.set(
        "overall_volume",
        lua.create_function(move |_, ()| Ok(h.borrow().world.audio.volume()))?,
    )?;
    // pause_toggle(): F1 (the pause screen opened, or closed once up a
    // second). pause_screen() → {tab, page (from 1), pages, lines, hint,
    // previous, next} or nil. pause_press(button): "objective", "items",
    // "weapons", "diary", "help", "previous", "next" or "close".
    let h = host.clone();
    g.set(
        "pause_toggle",
        lua.create_function(move |_, ()| {
            h.borrow_mut().world.pause_toggle();
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "pause_screen",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let Some(s) = host.world.hud.pause_state() else {
                return Ok(None);
            };
            let t = lua.create_table()?;
            t.set("tab", format!("{:?}", s.tab).to_ascii_lowercase())?;
            t.set("page", s.page + 1)?;
            t.set("pages", s.pages)?;
            t.set("lines", s.text)?;
            t.set("hint", s.hint)?;
            t.set("previous", s.previous)?;
            t.set("next", s.next)?;
            Ok(Some(t))
        })?,
    )?;
    // cheats(on): typing the cheat codes allowed (as once the game is won).
    // type_keys(s) → the last message: letters typed on the pause screen.
    let h = host.clone();
    g.set(
        "cheats",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().world.cheats.allowed = on;
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "type_keys",
        lua.create_function(move |_, s: String| {
            let mut host = h.borrow_mut();
            let mut last = None;
            for c in s.chars() {
                if let Some(m) = host.world.cheat_key(c) {
                    last = Some(m.to_string());
                }
            }
            Ok(last)
        })?,
    )?;
    // effects(id) → {shield, shield_shown, shield_active, shield_parts,
    // daodan, invisibility, invisible_for, invis_active, invis_alpha,
    // seen}: a fighter's shield and invisibility (`seen`: the AI can see
    // it).
    let h = host.clone();
    g.set(
        "effects",
        lua.create_function(move |lua, id: usize| {
            let host = h.borrow();
            let f = host
                .world
                .fighters
                .get(id)
                .ok_or_else(|| mlua::Error::runtime("no such fighter"))?;
            let t = lua.create_table()?;
            t.set("shield", f.inventory.shield)?;
            t.set("shield_shown", f.inventory.shield_display)?;
            t.set("shield_active", f.effects.shield_active)?;
            t.set("shield_parts", f.effects.shield_parts.to_vec())?;
            t.set("daodan", f.effects.daodan)?;
            t.set("invisibility", f.inventory.invisibility)?;
            t.set("invisible_for", f.inventory.invisible_frames)?;
            t.set("invis_active", f.effects.invis_active)?;
            t.set("invis_alpha", f.effects.invis_alpha)?;
            t.set("seen", host.world.can_be_seen(id))?;
            t.set("super", f.super_amount)?;
            t.set("super_shield", f.has_super_shield())?;
            t.set(
                "super_particles",
                f.super_handles.as_ref().map_or(0, Vec::len),
            )?;
            t.set("boss_shield", f.boss_shield)?;
            Ok(t)
        })?,
    )?;
    // frame_ticks(n) → the ticks a frame runs with n passed on the clock;
    // time_scale() → 2 in slow motion, else 1; env_shade_of(ref) → {r, g, b}
    // or nil; obj_shades() → each live object's flat shade ({r, g, b} or
    // false).
    let h = host.clone();
    g.set(
        "frame_ticks",
        lua.create_function(move |_, n: u32| Ok(h.borrow_mut().world.frame_ticks(n)))?,
    )?;
    let h = host.clone();
    g.set(
        "time_scale",
        lua.create_function(move |_, ()| Ok(h.borrow().world.time_scale()))?,
    )?;
    let h = host.clone();
    g.set(
        "env_shade_of",
        lua.create_function(move |_, r: u32| {
            Ok(h.borrow().world.env_shades.get(&r).map(|c| c.to_vec()))
        })?,
    )?;
    // door_shade_of(object id) → {r, g, b}: the door's flat shade (0–1).
    let h = host.clone();
    g.set(
        "door_shade_of",
        lua.create_function(move |_, id: u32| {
            Ok(h.borrow().world.door_shades.get(&id).map(|c| c.to_vec()))
        })?,
    )?;
    // door_ids() → {object id, …}: the level's doors.
    let h = host.clone();
    g.set(
        "door_ids",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .doors
                .iter()
                .map(|d| d.object_id)
                .collect::<Vec<_>>())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "obj_shades",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let out = lua.create_table()?;
            for o in &host.world.objects {
                match o.shade {
                    Some(c) => out.set(out.raw_len() + 1, c.to_vec())?,
                    None => out.set(out.raw_len() + 1, false)?,
                }
            }
            Ok(out)
        })?,
    )?;
    // boss_of(id) → a boss's mode now ("ShootBeam", "Regenerate", …) or nil.
    let h = host.clone();
    g.set(
        "boss_of",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            Ok(
                match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                    Some(crate::ai::Brain::Agent(a)) => a.boss_mode().map(|m| format!("{m:?}")),
                    _ => None,
                },
            )
        })?,
    )?;
    // magic_drawn() → how many magic effects the last screenshot drew
    // (shields, invisibility's shimmer).
    let h = host.clone();
    g.set(
        "magic_drawn",
        lua.create_function(move |_, ()| Ok(h.borrow().magic_drawn))?,
    )?;
    let h = host.clone();
    g.set(
        "pause_press",
        lua.create_function(move |_, button: String| {
            let id = match button.as_str() {
                "objective" => 101,
                "items" => 102,
                "weapons" => 103,
                "diary" => 104,
                "help" => 105,
                "previous" => 106,
                "next" => 107,
                "close" => 2,
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "no pause screen button {button}"
                    )))
                }
            };
            h.borrow_mut().world.pause_press(id, true);
            Ok(())
        })?,
    )?;
    // text_console() → {name, page, pages, last, lines} or nil: the data
    // console showing its text (page from 1; lines: the page's strings).
    // text_console_key("next" | "close"): Space / Esc on it.
    // text_console_click(x, y): the left button at screen x, y (640 × 480;
    // `button` in text_console() is its button's {x, y, w, h}) → whether it
    // was on the button (Next / Close).
    let h = host.clone();
    g.set(
        "text_console",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let Some(c) = host.world.hud.text_console.as_ref() else {
                return Ok(None);
            };
            let t = lua.create_table()?;
            t.set("name", c.name.clone())?;
            t.set("page", c.page + 1)?;
            t.set("pages", c.pages.len())?;
            t.set("last", c.last())?;
            t.set(
                "lines",
                c.pages
                    .get(c.page)
                    .map(|p| p.text.iter().map(|l| l.text.clone()).collect::<Vec<_>>())
                    .unwrap_or_default(),
            )?;
            if let Some(b) = host.world.hud.text_console_button(640.0, 480.0) {
                t.set("button", b.to_vec())?;
            }
            Ok(Some(t))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "text_console_key",
        lua.create_function(move |_, key: String| {
            let hud = &mut h.borrow_mut().world.hud;
            match key.as_str() {
                "next" => hud.text_console_next(),
                _ => hud.text_console_close(),
            }
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "text_console_click",
        lua.create_function(move |_, (x, y): (f32, f32)| {
            let world = &mut h.borrow_mut().world;
            let hit = world.hud.text_console_hit(640.0, 480.0, x, y);
            if hit {
                world.text_console_click();
            }
            Ok(hit)
        })?,
    )?;
    // lock_of(id) → the frames fighter `id`'s scripted animation still holds
    // it (`animationLockFrames`; 0: free).
    let h = host.clone();
    g.set(
        "lock_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map_or(0, |f| f.motion.lock_frames))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "facing_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map(|f| f.facing.facing.to_degrees())
                .unwrap_or(0.0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "dead",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .is_some_and(|f| f.motion.dying))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "state",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.motion.state)
                .unwrap_or(0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "anim_type",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.motion.kind)
                .unwrap_or(0))
        })?,
    )?;

    // Queue a jump (fires on the next grounded tick); `airborne()` reads state.
    let h = host.clone();
    g.set(
        "jump",
        lua.create_function(move |_, ()| {
            h.borrow_mut().taps |= key::JUMP;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "airborne",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.airborne())
                .unwrap_or(false))
        })?,
    )?;

    // Melee: queue a punch / kick (fires on the next step's first tick).
    let h = host.clone();
    g.set(
        "punch",
        lua.create_function(move |_, ()| {
            h.borrow_mut().taps |= key::PUNCH;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "kick",
        lua.create_function(move |_, ()| {
            h.borrow_mut().taps |= key::KICK;
            Ok(())
        })?,
    )?;

    // `facing(deg)` snaps the player body (and its target); `turn(delta)` nudges
    // the target so the body slews toward it over subsequent ticks (mouse turn).
    let h = host.clone();
    g.set(
        "facing",
        lua.create_function(move |_, deg: f32| {
            if let Some(p) = h.borrow_mut().world.fighters.first_mut() {
                p.facing = crate::control::Facing::new(deg.to_radians());
            }
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "turn",
        lua.create_function(move |_, delta: f32| {
            if let Some(p) = h.borrow_mut().world.fighters.first_mut() {
                p.facing.adjust_desired(delta.to_radians());
            }
            Ok(())
        })?,
    )?;

    // Read-back of the player body yaw and the (mouse) target, in degrees.
    let h = host.clone();
    g.set(
        "facing_deg",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.facing.facing.to_degrees())
                .unwrap_or(0.0))
        })?,
    )?;

    // The aim direction: where the player points, without the body lean a
    // diagonal run or turned sidestep adds.
    let h = host.clone();
    g.set(
        "aim_deg",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.facing.aim().to_degrees())
                .unwrap_or(0.0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "desired_deg",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.facing.desired_facing.to_degrees())
                .unwrap_or(0.0))
        })?,
    )?;

    // Spawn a training dummy; returns its fighter id.
    let h = host.clone();
    g.set(
        "spawn",
        lua.create_function(
            move |_, (x, y, z, facing_deg, class): (f32, f32, f32, Option<f32>, Option<String>)| {
                let mut host = h.borrow_mut();
                let class = class.unwrap_or_else(|| host.opponent.clone());
                let kind = host.kind(&class).map_err(mlua::Error::runtime)?;
                let yaw = facing_deg.unwrap_or(180.0).to_radians();
                let id = host.world.spawn(&kind, Vec3::new(x, y, z), yaw, true);
                host.class_names.push(class);
                Ok(id as u32)
            },
        )?,
    )?;

    // Goals (`docs/GOALS.md`): place one of the level's characters by name
    // (its class, position, job, alerts, profiles) and return its id; give a
    // spawned character default goals (idle, its class's combat profile);
    // read what an agent is doing.
    let h = host.clone();
    g.set(
        "place",
        lua.create_function(move |_, name: String| {
            let mut host = h.borrow_mut();
            let placed = host
                .world
                .level_ai
                .characters
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(&name))
                .cloned()
                .ok_or_else(|| mlua::Error::runtime(format!("no placed character {name:?}")))?;
            let kind = host.kind(&placed.class).map_err(mlua::Error::runtime)?;
            // (A placed position is the feet; spawning takes the pelvis:
            // start it a little above and let it settle.)
            let id = host
                .world
                .spawn(&kind, placed.at + Vec3::Y * 10.0, placed.facing, false);
            let melee = placed.melee_id.and_then(|m| host.classes.melee_by_id(m));
            host.world.give_goals(id, Some(&placed), melee);
            host.class_names.push(placed.class.clone());
            Ok(id as u32)
        })?,
    )?;
    // The level's own scripts (`scripting/PORT.md`): begin the level (the
    // player at its placement, the characters present from the start,
    // `main`); what the scripts are doing; a character by name; run a
    // script function.
    let h = host.clone();
    g.set(
        "mission_begin",
        lua.create_function(move |_, (run, save_point): (Option<bool>, Option<i64>)| {
            let mut host = h.borrow_mut();
            let mut m = crate::mission::Mission::load(&host.world.level_dir)
                .map_err(mlua::Error::runtime)?;
            // (`mission_begin(true, n)`: from the level's save point n.)
            if let Some(n) = save_point {
                m.state
                    .vars
                    .insert("save_point".into(), mlua::Value::Integer(n));
            }
            // (A level begun afresh flags its new moves.)
            if save_point.unwrap_or(0) == 0 && run != Some(false) {
                host.world.hud.level_started();
            }
            // (`mission_begin(false)`: the scripts loaded, the level's own
            // start not run.)
            if run != Some(false) {
                m.begin(&mut *host);
            }
            host.mission = Some(m);
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "mission_info",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let t = lua.create_table()?;
            if let Some(m) = &host.mission {
                t.set("tasks", m.tasks())?;
                t.set("errors", m.state.errors.clone())?;
                t.set("objective", m.state.objective)?;
                t.set("outcome", m.state.outcome)?;
                t.set("spawned", m.state.spawned.len())?;
                t.set("messages", m.state.messages.clone())?;
                let mut stubs: Vec<_> = m.state.stubbed.keys().cloned().collect();
                stubs.sort();
                t.set("stubbed", stubs)?;
            }
            Ok(t)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "named",
        lua.create_function(move |_, name: String| {
            Ok(h.borrow().world.fighters.iter().position(|f| {
                !f.gone && matches!(&f.brain, Some(crate::ai::Brain::Agent(a)) if a.name == name)
            }))
        })?,
    )?;
    // Plays a level film on fighter `id` (mode "", "fromhere", "interp");
    // returns its length in ticks, or nil.
    let h = host.clone();
    g.set(
        "play_film",
        lua.create_function(
            move |_, (id, name, mode): (usize, String, Option<String>)| {
                Ok(h.borrow_mut()
                    .world
                    .play_film(id, &name, mode.as_deref().unwrap_or(""), 30))
            },
        )?,
    )?;

    // Sends an AI to raise the alarm at console `console` (nil: the nearest).
    let h = host.clone();
    g.set(
        "alarm",
        lua.create_function(move |_, (id, console): (usize, Option<u16>)| {
            h.borrow_mut().world.order_alarm(id, console);
            Ok(())
        })?,
    )?;

    // Resets a console (unused; active as authored).
    let h = host.clone();
    g.set(
        "console_reset",
        lua.create_function(move |_, id: u16| {
            for k in h
                .borrow_mut()
                .world
                .consoles
                .iter_mut()
                .filter(|k| k.id == id)
            {
                k.triggered = false;
                k.active = k.initial_active;
            }
            Ok(())
        })?,
    )?;

    // collisions(id) → walls, objects, characters, records, serious: what
    // fighter id's movement met last tick, and its AI's collision records.
    let h = host.clone();
    g.set(
        "collisions",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let Some(f) = host.world.fighters.get(id) else {
                return Ok((0, 0, 0, 0, 0));
            };
            let (r, s) = f.avoid.count();
            Ok((f.collisions.0, f.collisions.1, f.collisions.2, r, s))
        })?,
    )?;
    // objects() → {{name, at={x,y,z}, frame, animating}, …}: the live placed
    // objects.
    let h = host.clone();
    g.set(
        "objects",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let t = lua.create_table()?;
            for (k, (name, at, frame, animating)) in
                host.world.object_states().into_iter().enumerate()
            {
                let o = lua.create_table()?;
                o.set("name", name)?;
                o.set("at", vec![at.x, at.y, at.z])?;
                o.set("frame", frame)?;
                o.set("animating", animating)?;
                t.set(k + 1, o)?;
            }
            Ok(t)
        })?,
    )?;
    // door_leaves(id) → {{at={x,y,z}, half={x,y,z}, solid, paused}, …}: door
    // id's leaf boxes this tick.
    let h = host.clone();
    g.set(
        "door_leaves",
        lua.create_function(move |lua, id: u16| {
            let host = h.borrow();
            let t = lua.create_table()?;
            for (k, (at, half, solid, paused)) in host.world.door_leaves(id).into_iter().enumerate()
            {
                let l = lua.create_table()?;
                l.set("at", vec![at.x, at.y, at.z])?;
                l.set("half", vec![half.x, half.y, half.z])?;
                l.set("solid", solid)?;
                l.set("paused", paused)?;
                t.set(k + 1, l)?;
            }
            Ok(t)
        })?,
    )?;
    // Orders door `id`: "open", "close", "lock", "unlock", "jam", "unjam".
    let h = host.clone();
    g.set(
        "door",
        lua.create_function(move |_, (id, what): (u16, String)| {
            let mut host = h.borrow_mut();
            let w = &mut host.world;
            match what.as_str() {
                "lock" => w.door_set(i64::from(id), Some(true), None),
                "unlock" => w.door_set(i64::from(id), Some(false), None),
                "jam" => w.door_set(i64::from(id), None, Some(true)),
                "unjam" => w.door_set(i64::from(id), None, Some(false)),
                _ => {
                    for d in w.doors.iter_mut().filter(|d| d.id == id) {
                        if what == "open" {
                            d.open();
                        } else {
                            d.close();
                        }
                    }
                }
            }
            Ok(())
        })?,
    )?;

    // Weapons (`docs/WEAPONS.md`): give_weapon(id, class) puts a new gun in
    // a fighter's hand; weapon(id) → class, ammo, firing (nil: unarmed);
    // aim(id, pitch) sets its aim pitch (radians, up positive);
    // reload(id), drop_weapon(id[, knock]); give_ammo(id, clips, cells);
    // shots() → live particles; weapons_lying() → guns on the floor.
    let h = host.clone();
    g.set(
        "give_weapon",
        lua.create_function(move |_, (id, class): (usize, String)| {
            Ok(h.borrow_mut().world.give_weapon(id, &class))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "weapon",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let w = &host.world;
            let gun = w
                .fighters
                .get(id)
                .and_then(|f| f.hand)
                .map(|k| &w.weapons()[k]);
            Ok((
                gun.map(|g| g.class.name.clone()),
                gun.map(|g| g.ammo),
                gun.map(|g| g.firing),
            ))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "aim",
        lua.create_function(move |_, (id, pitch): (usize, f32)| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.aiming_ud = pitch;
            }
            Ok(())
        })?,
    )?;
    // The HUD (docs/HUD.md): the lines in its message area, subtitles and
    // console strip; the objective (and whether it is unread); the compass
    // target; the letterbox's depth; the fade's opacity; the elements filled
    // and flashing; the rings shown; the player's hit ring.
    let h = host.clone();
    g.set(
        "hud",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let hud = &host.world.hud;
            let t = lua.create_table()?;
            let lines = |a: &crate::hud::text::TextArea| {
                a.entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>()
            };
            t.set("messages", lines(&hud.messages))?;
            t.set("subtitles", lines(&hud.subtitles))?;
            t.set("console", lines(&hud.console))?;
            t.set("objective", hud.objective)?;
            t.set("new_objective", hud.new_objective)?;
            if let Some(tg) = hud.target {
                t.set("target", vec![tg.at.x, tg.at.y, tg.at.z])?;
            }
            t.set("letterbox", hud.letterbox)?;
            t.set("fade", hud.fade_alpha())?;
            t.set("filled", hud.filled)?;
            t.set("flashing", hud.flashing)?;
            t.set("show", vec![hud.show[0], hud.show[1]])?;
            t.set(
                "hits",
                host.world
                    .fighters
                    .first()
                    .map(|p| p.hits.to_vec())
                    .unwrap_or_default(),
            )?;
            Ok(t)
        })?,
    )?;
    // The game flow (docs/GAME_FLOW.md): the level's outcome (true won,
    // false lost), a splash screen asked for, the save point kept.
    let h = host.clone();
    g.set(
        "game",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let w = &host.world;
            let t = lua.create_table()?;
            t.set("victory", w.victory)?;
            t.set("splash", w.pending_splash.clone())?;
            t.set("save_point", w.saved.as_ref().map(|c| c.save_point))?;
            Ok(t)
        })?,
    )?;
    // Lens flares: how many there are, and how many are shown now
    // (docs/PARTICLES.md, *Lens flares*).
    let h = host.clone();
    g.set(
        "lens_flares",
        lua.create_function(move |_, ()| {
            let host = h.borrow();
            let l = &host.world.lens;
            Ok((l.len(), l.values().filter(|v| v.1 > 0.0).count()))
        })?,
    )?;
    // lens_flare_near(x, y, z) → the opacity share now of the lens flare
    // nearest that point (0 hidden, 1 fully seen).
    let h = host.clone();
    g.set(
        "lens_flare_near",
        lua.create_function(move |_, (x, y, z): (f32, f32, f32)| {
            let host = h.borrow();
            let w = &host.world;
            let sys = &w.armory.particles;
            let at = Vec3::new(x, y, z);
            Ok(w.lens
                .iter()
                .filter_map(|(h, v)| {
                    sys.get(*h)
                        .map(|p| (sys.world_position(p).distance(at), v.1))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|v| v.1))
        })?,
    )?;
    // use_hypo(id) → whether fighter `id` used a hypo (as Tab does);
    // hypo_left(id) → the health its hypo still has to give.
    let h = host.clone();
    g.set(
        "use_hypo",
        lua.create_function(move |_, id: usize| Ok(h.borrow_mut().world.use_hypo(id)))?,
    )?;
    let h = host.clone();
    g.set(
        "hypo_left",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map_or(0, |f| f.inventory.hypo_remaining))
        })?,
    )?;
    // The player reads the objective (F1).
    let h = host.clone();
    g.set(
        "read_objective",
        lua.create_function(move |_, ()| {
            h.borrow_mut().world.hud.read_objective();
            Ok(())
        })?,
    )?;
    // Dialogue subtitles on or off (the game's option).
    let h = host.clone();
    g.set(
        "subtitles",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().world.hud.subtitles_on = on;
            Ok(())
        })?,
    )?;
    // How many overlay triangles the HUD draws now (on a 640 × 480 screen).
    let h = host.clone();
    g.set(
        "hud_triangles",
        lua.create_function(move |_, ()| {
            let draws = h.borrow_mut().world.hud_draw(640.0, 480.0);
            Ok(draws.iter().map(|d| d.tris.len() / 3).sum::<usize>())
        })?,
    )?;
    // The game camera: where it is, which way it looks, whether a script
    // waits on it, and how many walls it makes see-through.
    let h = host.clone();
    g.set(
        "camera",
        lua.create_function(move |_, ()| {
            let host = h.borrow();
            let c = &host.world.camera;
            let (p, v) = (c.view.location, c.view.view);
            Ok((p.x, p.y, p.z, v.x, v.y, v.z, c.busy))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "jello",
        lua.create_function(move |_, ()| Ok(h.borrow().world.jello.len()))?,
    )?;
    // Holds (or lets go of) the look-mode key.
    let h = host.clone();
    g.set(
        "look_mode",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().hold(key::LOOK, on);
            Ok(())
        })?,
    )?;
    // The player's look-mode turn (radians).
    let h = host.clone();
    g.set(
        "aim_lr",
        lua.create_function(move |_, v: Option<f32>| {
            let mut host = h.borrow_mut();
            let Some(p) = host.world.fighters.first_mut() else {
                return Ok(0.0);
            };
            if let Some(v) = v {
                p.aiming_lr = v;
            }
            Ok(p.aiming_lr)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "aim_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map_or(0.0, |f| f.aiming_ud))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "reload",
        lua.create_function(move |_, id: usize| Ok(h.borrow_mut().world.reload(id)))?,
    )?;
    let h = host.clone();
    g.set(
        "drop_weapon",
        lua.create_function(move |_, (id, knock): (usize, Option<bool>)| {
            h.borrow_mut().world.drop_weapon(id, knock.unwrap_or(false));
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "give_ammo",
        lua.create_function(move |_, (id, clips, cells): (usize, u16, Option<u16>)| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.inventory.ammo += clips;
                f.inventory.cells += cells.unwrap_or(0);
            }
            Ok(())
        })?,
    )?;
    // corpses() → {{class, x, y, z, drawn, dynamic}, …}: the level's
    // corpses (its own, then the dead of play), where each one's pelvis is,
    // and whether it is drawn (has a model, not cleared).
    let h = host.clone();
    g.set(
        "corpses",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let out = lua.create_table()?;
            let fixed = host.world.corpses.fixed;
            for (k, c) in host.world.corpses.list.iter().enumerate() {
                let at = c.bones.first().map_or(Vec3::ZERO, |m| m.w_axis.truncate());
                let t = lua.create_table()?;
                t.set("class", c.class.clone())?;
                t.set("x", at.x)?;
                t.set("y", at.y)?;
                t.set("z", at.z)?;
                t.set("drawn", c.model.is_some())?;
                t.set("dynamic", k >= fixed)?;
                out.set(out.raw_len() + 1, t)?;
            }
            Ok(out)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "shots",
        lua.create_function(move |_, ()| Ok(h.borrow().world.particles().iter().count()))?,
    )?;
    let h = host.clone();
    g.set(
        "swap",
        lua.create_function(move |_, ()| {
            h.borrow_mut().taps |= key::SWAP;
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "spawn_weapon",
        lua.create_function(
            move |_, (class, x, y, z, falling): (String, f32, f32, f32, Option<bool>)| {
                Ok(h.borrow_mut()
                    .world
                    .spawn_weapon(&class, Vec3::new(x, y, z), 0.0, !falling.unwrap_or(false))
                    .is_some())
            },
        )?,
    )?;
    // loose_guns() → {{class, at={x,y,z}, falling}, …}: the guns no one holds.
    let h = host.clone();
    g.set(
        "loose_guns",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let t = lua.create_table()?;
            for (k, g) in host
                .world
                .weapons()
                .iter()
                .filter(|g| g.owner.is_none() && !g.gone)
                .enumerate()
            {
                let o = lua.create_table()?;
                o.set("class", g.class.name.clone())?;
                o.set("at", vec![g.position.x, g.position.y, g.position.z])?;
                o.set("moving", g.velocity.length() > 1e-3)?;
                t.set(k + 1, o)?;
            }
            Ok(t)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "holster_of",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let w = &host.world;
            Ok(w.fighters
                .get(id)
                .and_then(|f| f.holster)
                .map(|k| w.weapons()[k].class.name.clone()))
        })?,
    )?;
    // Sound: sounds() → the playing ambients, each {name, stage ("in",
    // "body", "stopping", "out"), clip (the sound playing now), left,
    // right (gains)}.
    let h = host.clone();
    g.set(
        "sounds",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let list = lua.create_table()?;
            for (i, s) in host.world.audio.heard().into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("name", s.name)?;
                t.set("stage", format!("{:?}", s.stage).to_lowercase())?;
                t.set("clip", s.clip)?;
                t.set("left", s.gain[0])?;
                t.set("right", s.gain[1])?;
                list.set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    // impulse(name [, x, y, z [, volume]]) plays an impulse sound (placed,
    // or not) → its channel (0–15) or nil; channels() → the 16 channels,
    // each {clip, locked, volume (linear), left, right (output gains)};
    // sound_volume(v) sets the overall volume to head for.
    let h = host.clone();
    g.set(
        "impulse",
        lua.create_function(
            move |_,
                  (name, x, y, z, volume): (
                String,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
            )| {
                let mut host = h.borrow_mut();
                let at = match (x, y, z) {
                    (Some(x), Some(y), Some(z)) => Some(Vec3::new(x, y, z)),
                    _ => None,
                };
                let w = &mut host.world;
                Ok(w.audio.impulse_play(&name, at, volume, Some(&w.quads)))
            },
        )?,
    )?;
    let h = host.clone();
    g.set(
        "channels",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let list = lua.create_table()?;
            for (i, c) in host.world.audio.channels().into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("clip", c.clip)?;
                t.set("locked", c.locked)?;
                t.set("volume", c.volume)?;
                t.set("left", c.gain[0])?;
                t.set("right", c.gain[1])?;
                list.set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    // sound_objects() → the level's sound objects, each {ambient, x, y, z,
    // box (else a sphere), state ("Off", "Starting", "Playing",
    // "Stopping"), volume, playing}.
    let h = host.clone();
    g.set(
        "sound_objects",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let list = lua.create_table()?;
            for (i, (ambient, at, is_box, state, volume, playing)) in
                host.world.audio.sound_objects().into_iter().enumerate()
            {
                let t = lua.create_table()?;
                t.set("ambient", ambient)?;
                t.set("x", at.x)?;
                t.set("y", at.y)?;
                t.set("z", at.z)?;
                t.set("box", is_box)?;
                t.set("state", format!("{state:?}"))?;
                t.set("volume", volume)?;
                t.set("playing", playing)?;
                list.set(i + 1, t)?;
            }
            Ok(list)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "sound_volume",
        lua.create_function(move |_, v: f32| {
            h.borrow_mut().world.audio.set_volume(v);
            Ok(())
        })?,
    )?;
    // powerup(id, kind [, amount]) gives a powerup the game's way (`ammo`,
    // `cell`, `hypo`, `shield`, `invis`, `lsi`) → how much it took.
    let h = host.clone();
    g.set(
        "powerup",
        lua.create_function(move |_, (id, kind, amount): (usize, String, Option<u16>)| {
            Ok(h.borrow_mut().world.give_powerup(id, &kind, amount, false))
        })?,
    )?;
    // env_particles(tag) → how many of the level's environmental particles
    // tagged `tag` are alive; particle(tag, command [, event]) sends them a
    // command as scripts do.
    let h = host.clone();
    g.set(
        "env_particles",
        lua.create_function(move |_, tag: String| Ok(h.borrow().world.env_particles_alive(&tag)))?,
    )?;
    // decals([class]) → { {class, static, alpha, vertices, triangles,
    // positions = {{x,y,z}…}, quad}, … }: the live decals (of a class),
    // static first, oldest first.
    let h = host.clone();
    g.set(
        "decals",
        lua.create_function(move |lua, class: Option<String>| {
            let host = h.borrow();
            let particles = &host.world.armory.particles;
            let out = lua.create_table()?;
            let fixed = particles.decals.fixed.len();
            for (k, d) in particles.decals.iter().enumerate() {
                let Some(p) = particles.get(d.particle) else {
                    continue;
                };
                let name = &particles.classes.list[p.class].name;
                if class.as_ref().is_some_and(|c| c != name) {
                    continue;
                }
                let t = lua.create_table()?;
                t.set("class", name.as_str())?;
                t.set("static", k < fixed)?;
                t.set("alpha", d.alpha)?;
                t.set("vertices", d.mesh.positions.len())?;
                t.set("triangles", d.mesh.indices.len() / 3)?;
                t.set("quad", d.mesh.quad)?;
                let ps = lua.create_table()?;
                for (i, v) in d.mesh.positions.iter().enumerate() {
                    ps.set(i + 1, vec![v.x, v.y, v.z])?;
                }
                t.set("positions", ps)?;
                out.set(out.raw_len() + 1, t)?;
            }
            Ok(out)
        })?,
    )?;
    // particles_of(class) → { {x, y, z, dx, dy, dz [, tint]}, … }: where
    // each live particle of a class is, its forward (Y) axis, and its colour
    // when that is a variable.
    // health_shade(id) → the colour of a fighter's health (melee flashes'
    // tint).
    let h = host.clone();
    g.set(
        "health_shade",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let f = host
                .world
                .fighters
                .get(id)
                .ok_or_else(|| mlua::Error::runtime("no such fighter"))?;
            Ok(host.world.health_shade(f.hp, f.max_hp))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "particles_of",
        lua.create_function(move |lua, class: String| {
            let host = h.borrow();
            let sys = &host.world.armory.particles;
            let out = lua.create_table()?;
            for (_, p) in sys.iter() {
                if sys.classes.list[p.class].name != class {
                    continue;
                }
                let (at, o) = (sys.world_position(p), sys.world_orientation(p).y_axis);
                let mut v = vec![
                    f64::from(at.x),
                    f64::from(at.y),
                    f64::from(at.z),
                    f64::from(o.x),
                    f64::from(o.y),
                    f64::from(o.z),
                ];
                if let crate::p3::class::Value::Var(k) = sys.classes.list[p.class].tint {
                    if let Some(crate::p3::Val::U(t)) = p.vars.get(k) {
                        v.push(f64::from(*t));
                    }
                }
                out.set(out.raw_len() + 1, v)?;
            }
            Ok(out)
        })?,
    )?;
    // animated_textures() → {{name, frame}, …}: the animated textures the
    // level and its models draw, and the frame each shows at the game time
    // (as of the last screenshot).
    let h = host.clone();
    g.set(
        "animated_textures",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let out = lua.create_table()?;
            for (k, (name, frame)) in host.renderer.animated_textures().into_iter().enumerate() {
                let t = lua.create_table()?;
                t.set("name", name)?;
                t.set("frame", frame)?;
                out.set(k + 1, t)?;
            }
            Ok(out)
        })?,
    )?;
    // particle_textures(class) → {file stem, …}: the texture each live
    // particle of a class shows now (an animated one's current frame).
    let h = host.clone();
    g.set(
        "particle_textures",
        lua.create_function(move |_, class: String| {
            let host = h.borrow();
            let sys = &host.world.armory.particles;
            Ok(sys
                .iter()
                .filter(|(_, p)| sys.classes.list[p.class].name == class)
                .map(|(_, p)| {
                    host.world
                        .particle_texture(p)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect::<Vec<String>>())
        })?,
    )?;
    // shadow(id) → {alpha, quad, vertices, positions} or nil: fighter
    // `id`'s shadow this frame (and the quad it was placed by).
    let h = host.clone();
    g.set(
        "shadow",
        lua.create_function(move |lua, id: usize| {
            let host = h.borrow();
            let Some((mesh, alpha, _)) = host.world.shadow(id) else {
                return Ok(None);
            };
            let t = lua.create_table()?;
            t.set("alpha", alpha)?;
            t.set("quad", mesh.quad)?;
            t.set("vertices", mesh.positions.len())?;
            let ps = lua.create_table()?;
            for (i, v) in mesh.positions.iter().enumerate() {
                ps.set(i + 1, vec![v.x, v.y, v.z])?;
            }
            t.set("positions", ps)?;
            Ok(Some(t))
        })?,
    )?;
    // bone(id, k) / held_gun(id) → {x={…}, y={…}, z={…}, at={…}}: a
    // fighter's posed bone k, or the gun in its hand (nil without one).
    let mat = |lua: &mlua::Lua, m: glam::Mat4| -> mlua::Result<mlua::Table> {
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
    };
    let h = host.clone();
    g.set(
        "bone",
        lua.create_function(move |lua, (id, k): (usize, usize)| {
            let host = h.borrow();
            let Some(m) = host
                .world
                .fighters
                .get(id)
                .and_then(|f| f.bones().get(k).copied())
            else {
                return Ok(None);
            };
            mat(lua, m).map(Some)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "held_gun",
        lua.create_function(move |lua, id: usize| {
            let host = h.borrow();
            let w = &host.world;
            let Some(k) = w.fighters.get(id).and_then(|f| f.hand) else {
                return Ok(None);
            };
            mat(lua, w.armory.weapons[k].matrix.get()).map(Some)
        })?,
    )?;
    // sight(id) → {from, to, target, laser, tunnel}: fighter id's gun sight
    // this tick (nil without one); target "none" | "environment" | "object" |
    // "character", laser its {r, g, b} (nil without one), tunnel how many
    // sprites. gun_through_wall(id) → whether its gun pokes through a wall.
    let h = host.clone();
    g.set(
        "sight",
        lua.create_function(move |lua, id: usize| {
            let host = h.borrow();
            let Some(s) = host.world.gun_sight(id) else {
                return Ok(None);
            };
            let t = lua.create_table()?;
            t.set("from", vec![s.from.x, s.from.y, s.from.z])?;
            t.set("to", vec![s.to.x, s.to.y, s.to.z])?;
            t.set(
                "target",
                match s.target {
                    crate::world::SightTarget::None => "none",
                    crate::world::SightTarget::Object => "object",
                    crate::world::SightTarget::Environment => "environment",
                    crate::world::SightTarget::Character => "character",
                },
            )?;
            t.set("laser", s.laser.map(|c| c.to_vec()))?;
            t.set("tunnel", s.tunnel.len())?;
            Ok(Some(t))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "gun_through_wall",
        lua.create_function(move |_, id: usize| Ok(h.borrow().world.gun_through_wall(id)))?,
    )?;
    // show_decals(on): whether screenshots draw the decals (to compare).
    let h = host.clone();
    g.set(
        "show_decals",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().show_decals = on;
            Ok(())
        })?,
    )?;
    // quad_plane(q) → a, b, c, d: a level quad's plane.
    // jello_drawn() → how many see-through (jello) triangles the last
    // screenshot drew, and how many of them were doors'.
    let h = host.clone();
    g.set(
        "jello_drawn",
        lua.create_function(move |_, ()| {
            Ok((
                h.borrow().renderer.jello_triangles(),
                h.borrow().renderer.jello_door_triangles(),
            ))
        })?,
    )?;
    // decals_drawn(x, y, z) → {{quad, alpha}, …}: the decals drawn from
    // that eye this frame (alpha 0..1, as drawn).
    let h = host.clone();
    g.set(
        "decals_drawn",
        lua.create_function(move |lua, (x, y, z): (f32, f32, f32)| {
            let host = h.borrow();
            let out = lua.create_table()?;
            for (k, d) in host
                .world
                .decal_draws(Vec3::new(x, y, z))
                .iter()
                .enumerate()
            {
                let t = lua.create_table()?;
                t.set("quad", d.positions.quad)?;
                t.set("alpha", d.alpha)?;
                out.set(k + 1, t)?;
            }
            Ok(out)
        })?,
    )?;
    // set_jello(quad, on): marks a level quad see-through or not, as the
    // camera does (until the camera's next update clears its own).
    let h = host.clone();
    g.set(
        "set_jello",
        lua.create_function(move |_, (q, on): (u32, bool)| {
            let mut host = h.borrow_mut();
            if let Some(quad) = host.world.quads.quads.get_mut(q as usize) {
                if on {
                    quad.flags |= crate::quads::flag::JELLO;
                } else {
                    quad.flags &= !crate::quads::flag::JELLO;
                }
            }
            Ok(())
        })?,
    )?;
    // ray(x1, y1, z1, x2, y2, z2) → the first level quad between two points
    // (any flags): {quad, at={x,y,z}, flags, object_tag, jello} — jello: it is
    // see-through for the camera now; nil if nothing is in the way.
    let h = host.clone();
    g.set(
        "ray",
        lua.create_function(
            move |lua, (x1, y1, z1, x2, y2, z2): (f32, f32, f32, f32, f32, f32)| {
                let host = h.borrow();
                let w = &host.world;
                let (a, b) = (Vec3::new(x1, y1, z1), Vec3::new(x2, y2, z2));
                let Some(hit) = w.quads.ray(a, b - a, 0) else {
                    return Ok(None);
                };
                let q = &w.quads.quads[hit.quad as usize];
                let t = lua.create_table()?;
                t.set("quad", hit.quad)?;
                t.set("at", vec![hit.point.x, hit.point.y, hit.point.z])?;
                t.set("flags", q.flags)?;
                t.set("object_tag", q.object_tag)?;
                t.set("jello", w.jello.contains(&hit.quad))?;
                Ok(Some(t))
            },
        )?,
    )?;
    let h = host.clone();
    g.set(
        "quad_plane",
        lua.create_function(move |_, q: u32| {
            let host = h.borrow();
            let quads = &host.world.quads;
            let (n, d) = quads.plane(quads.quads.get(q as usize).map_or(0, |x| x.plane));
            Ok((n.x, n.y, n.z, d))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "particle",
        lua.create_function(
            move |_, (tag, command, event): (String, String, Option<String>)| {
                Ok(h.borrow_mut()
                    .world
                    .env_particle_command(&tag, &command, event.as_deref()))
            },
        )?,
    )?;
    // console_screen(id) → the texture console `id`'s screen shows now (by
    // its state), or nil.
    let h = host.clone();
    g.set(
        "console_screen",
        lua.create_function(move |_, id: u16| {
            let host = h.borrow();
            Ok(host
                .world
                .consoles
                .iter()
                .find(|c| c.id == id)
                .map(crate::world::World::screen_of))
        })?,
    )?;
    // dialog() → whether a scripted dialogue line is on.
    let h = host.clone();
    g.set(
        "dialog",
        lua.create_function(move |_, ()| Ok(h.borrow().world.audio.dialog_playing()))?,
    )?;
    // sounds_playing() → {name, …}: the sounds playing now, in start order.
    let h = host.clone();
    g.set(
        "sounds_playing",
        lua.create_function(move |_, ()| Ok(h.borrow().world.audio.playing_names()))?,
    )?;
    // saying(id) → the line a character is saying (an ambient's name) or nil.
    let h = host.clone();
    g.set(
        "saying",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .and_then(|f| f.speech.saying().map(str::to_string)))
        })?,
    )?;
    // Turrets: turret(id, "activate" | "deactivate" | "reset");
    // turret_state(id) → active, yaw, pitch (degrees), target, firing, shots
    // (the first turret with that script id; nil: none).
    let h = host.clone();
    g.set(
        "turret",
        lua.create_function(move |_, (id, what): (i64, String)| {
            h.borrow_mut().world.turret_set(id, &what);
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "turret_state",
        lua.create_function(move |lua, id: u16| {
            let host = h.borrow();
            let Some(t) = host.world.turrets.iter().find(|t| t.id == id) else {
                return Ok(mlua::Value::Nil);
            };
            let r = lua.create_table()?;
            r.set("active", t.active)?;
            r.set("yaw", t.yaw.to_degrees())?;
            r.set("pitch", t.pitch.to_degrees())?;
            r.set("target", t.target)?;
            r.set("firing", t.firing)?;
            r.set("shots", t.shots)?;
            Ok(mlua::Value::Table(r))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "dodge_of",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let d = match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::Agent(a)) => a.dodging(),
                _ => None,
            };
            Ok((d.map(|(v, _)| v.x), d.map(|(v, _)| v.z), d.map(|(_, w)| w)))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "gun_chance",
        lua.create_function(move |_, (id, percent): (usize, u32)| {
            if let Some(crate::ai::Brain::Agent(a)) = h
                .borrow_mut()
                .world
                .fighters
                .get_mut(id)
                .and_then(|f| f.brain.as_mut())
            {
                a.class.gun_pickup_percent = percent;
            }
            Ok(())
        })?,
    )?;
    // running_pickup_chance(id, percent): its chance of picking a gun up on
    // the run (`gun_runningpickup_percentage`).
    let h = host.clone();
    g.set(
        "running_pickup_chance",
        lua.create_function(move |_, (id, percent): (usize, u32)| {
            if let Some(crate::ai::Brain::Agent(a)) = h
                .borrow_mut()
                .world
                .fighters
                .get_mut(id)
                .and_then(|f| f.brain.as_mut())
            {
                a.class.gun_running_pickup_percent = percent;
            }
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "env_show",
        lua.create_function(move |_, (r, on): (u32, bool)| {
            h.borrow_mut().world.env_show(r, on);
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "env_broken",
        lua.create_function(move |_, (from, to): (u32, Option<u32>)| {
            Ok(h.borrow().world.env_broken(from, to.unwrap_or(from)))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "inventory",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let i = host
                .world
                .fighters
                .get(id)
                .map(|f| f.inventory)
                .unwrap_or_default();
            Ok((i.ammo, i.cells, i.hypos))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "powerups_lying",
        lua.create_function(move |_, ()| Ok(h.borrow().world.powerups().len()))?,
    )?;
    // powerups() → {{kind, at={x,y,z}, falling}, …}: the powerups lying
    // about (falling: still with physics).
    let h = host.clone();
    g.set(
        "powerups",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let t = lua.create_table()?;
            for (k, p) in host.world.powerups().iter().enumerate() {
                let o = lua.create_table()?;
                o.set("kind", crate::world::POWERUPS[p.kind])?;
                o.set("at", vec![p.position.x, p.position.y, p.position.z])?;
                o.set("falling", p.body.is_some())?;
                t.set(k + 1, o)?;
            }
            Ok(t)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "weapons_lying",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .weapons()
                .iter()
                .filter(|g| g.owner.is_none())
                .count())
        })?,
    )?;

    // Orders laser trigger `id` ("activate", "deactivate", "reset", "hide",
    // "show", "speed" with a factor); laser_state(id): active, on,
    // triggered, and how many beams are out (nil: no such laser).
    let h = host.clone();
    g.set(
        "laser",
        lua.create_function(move |_, (id, what, factor): (i64, String, Option<f32>)| {
            h.borrow_mut()
                .world
                .laser_set(id, &what, factor.unwrap_or(1.0));
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "laser_state",
        lua.create_function(move |_, id: u16| {
            let host = h.borrow();
            let l = host.world.lasers.iter().find(|l| l.id == id);
            Ok((
                l.map(|l| l.active),
                l.map(|l| l.laser_on),
                l.map(|l| l.triggered),
                l.map(|l| if l.beaming() { l.emitters.len() } else { 0 }),
            ))
        })?,
    )?;
    // Laser beams out now: a list of {x1, y1, z1, x2, y2, z2}.
    let h = host.clone();
    g.set(
        "beams",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .beams
                .iter()
                .map(|(a, b, _)| vec![a.x, a.y, a.z, b.x, b.y, b.z])
                .collect::<Vec<_>>())
        })?,
    )?;

    // A door's state ("closed" / "opening" / "open" / "closing") and
    // whether it is locked (nil: no such door).
    let h = host.clone();
    g.set(
        "door_state",
        lua.create_function(move |_, id: u16| {
            let host = h.borrow();
            let d = host.world.doors.iter().find(|d| d.id == id);
            Ok((d.map(|d| d.state_name()), d.map(|d| d.locked)))
        })?,
    )?;

    // A console's state: "inactive", "active" or "triggered" (nil: none).
    let h = host.clone();
    g.set(
        "console_state",
        lua.create_function(move |_, id: u16| {
            Ok(h.borrow()
                .world
                .consoles
                .iter()
                .find(|k| k.id == id)
                .map(|k| k.screen()))
        })?,
    )?;

    // Moves fighter `id` (feet) to a point.
    let h = host.clone();
    g.set(
        "teleport",
        lua.create_function(move |_, (id, x, y, z): (usize, f32, f32, f32)| {
            h.borrow_mut().world.teleport(id, Vec3::new(x, y, z), None);
            Ok(())
        })?,
    )?;
    // Defines script functions of the scenario's own (translated-Lua form)
    // in the running mission, to call with mission_call.
    let h = host.clone();
    g.set(
        "mission_exec",
        lua.create_function(move |_, code: String| {
            let host = h.borrow();
            match host.mission.as_ref() {
                Some(m) => m.exec(&code).map_err(mlua::Error::external),
                None => Err(mlua::Error::external(
                    "no mission (call mission_begin first)",
                )),
            }
        })?,
    )?;
    let h = host.clone();
    g.set(
        "mission_call",
        lua.create_function(move |_, (name, arg): (String, Option<String>)| {
            let mut host = h.borrow_mut();
            let args: Vec<&str> = arg.as_deref().into_iter().collect();
            Ok(host.mission.as_mut().is_some_and(|m| m.start(&name, &args)))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "goals",
        lua.create_function(move |_, id: usize| {
            h.borrow_mut().world.give_goals(id, None, None);
            Ok(())
        })?,
    )?;
    // skip_cutscene() asks to skip the cutscene (as Space does);
    // cutscene_skip() → "idle" / "starting" / "black" / "skipping" /
    // "fading in", and the screen's black (0–1; nil: none).
    let h = host.clone();
    g.set(
        "skip_cutscene",
        lua.create_function(move |_, ()| {
            h.borrow_mut().world.skip_cutscene();
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "cutscene_skip",
        lua.create_function(move |_, ()| {
            use crate::world::skip::Skip;
            let host = h.borrow();
            let w = &host.world;
            let mode = match w.skip {
                Skip::Idle => "idle",
                Skip::Starting(_) => "starting",
                Skip::Black => "black",
                Skip::Skipping => "skipping",
                Skip::FadingIn(_) => "fading in",
            };
            Ok((mode, w.skip_blanket()))
        })?,
    )?;
    // set_gamma(g): the gamma option for frames drawn (0–1; 0.5 as drawn).
    let h = host.clone();
    g.set(
        "set_gamma",
        lua.create_function(move |_, v: f32| {
            h.borrow_mut().renderer.gamma = v;
            Ok(())
        })?,
    )?;
    // The main menu (docs/MENU.md): menu_open([in_game]) brings it up;
    // menu_state() → {dialog, depth, rows, selected, checks = {id = on},
    // sliders = {id = value}, popups = {id = item}} of the dialog on top (nil:
    // none up); menu_click(id) clicks a control (its middle) and
    // menu_key(name) presses a key ("enter", "escape", "up", "down", …), each
    // returning what the menu asks for ("new_game", "new_game faithful",
    // "load L S", "resume", "quit", "options"; nil: nothing); menu_close().
    let h = host.clone();
    g.set(
        "menu_open",
        lua.create_function(move |_, in_game: Option<bool>| {
            let mut host = h.borrow_mut();
            let data = crate::hud::menu::Data::load(&host.oni_root.join("common"));
            if !data.has("mainmenu") {
                return Err(mlua::Error::external("no menus (common/menus.json)"));
            }
            let mut m = crate::hud::menu::MainMenu::open(
                &data,
                in_game.unwrap_or(false),
                crate::settings::Settings::default(),
            );
            m.quirks = host.world.quirks;
            host.menu = Some((data, m));
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "menu_close",
        lua.create_function(move |_, ()| {
            h.borrow_mut().menu = None;
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "menu_state",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let Some((_, m)) = &host.menu else {
                return Ok(None);
            };
            let Some(w) = m.top() else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("dialog", w.name.clone())?;
            t.set("depth", m.stack.len())?;
            let (checks, sliders, popups, visible) = (
                lua.create_table()?,
                lua.create_table()?,
                lua.create_table()?,
                lua.create_table()?,
            );
            for c in &w.controls {
                if c.visible {
                    visible.set(c.def.id, c.def.title.clone())?;
                }
                match &c.ctl {
                    crate::hud::menu::Ctl::Check(on) => checks.set(c.def.id, *on)?,
                    crate::hud::menu::Ctl::Slider { value, .. } => sliders.set(c.def.id, *value)?,
                    crate::hud::menu::Ctl::Popup { .. } => {
                        popups.set(c.def.id, w.popup(c.def.id))?
                    }
                    crate::hud::menu::Ctl::List { .. } => {
                        t.set("rows", w.rows(c.def.id))?;
                        t.set(
                            "selected",
                            w.selected_data(c.def.id)
                                .map(|d| (d & 0xFF, d >> 8))
                                .map(|(l, s)| format!("{l} {s}")),
                        )?;
                    }
                    _ => {}
                }
            }
            t.set("visible", visible)?;
            t.set("checks", checks)?;
            t.set("sliders", sliders)?;
            t.set("popups", popups)?;
            Ok(Some(t))
        })?,
    )?;
    fn act(a: Option<crate::hud::menu::Action>) -> Option<String> {
        use crate::hud::menu::Action;
        a.map(|a| match a {
            Action::NewGame { faithful } => {
                if faithful {
                    "new_game faithful".into()
                } else {
                    "new_game".into()
                }
            }
            Action::Load { level, save_point } => format!("load {level} {save_point}"),
            Action::Resume => "resume".into(),
            Action::Quit => "quit".into(),
            Action::Options(_) => "options".into(),
        })
    }
    fn run(host: &mut ScriptHost, cmds: Vec<crate::hud::menu::Command>) -> Option<String> {
        let root = host.oni_root.clone();
        let (data, m) = host.menu.as_mut()?;
        let rows = || crate::game::load_rows(&data.levels, &root);
        let mut out = None;
        for c in cmds {
            out = act(m.command(data, c, &rows)).or(out);
        }
        out
    }
    let h = host.clone();
    g.set(
        "menu_click",
        lua.create_function(move |_, id: i64| {
            let mut host = h.borrow_mut();
            let (w, hh) = (640.0, 480.0);
            let cmds = {
                let Some((data, m)) = host.menu.as_mut() else {
                    return Ok(None);
                };
                let Some(win) = m.top_mut() else {
                    return Ok(None);
                };
                let Some(c) = win.control(id).filter(|c| c.visible) else {
                    return Err(mlua::Error::external(format!("no control {id} showing")));
                };
                let at = win.at(w, hh);
                let [x, y, cw, ch] = c.def.rect;
                let (px, py) = (at[0] + x + (cw * 0.5).min(20.0), at[1] + y + ch * 0.5);
                let mut cmds = win.mouse_down(&data.look, w, hh, px, py);
                cmds.extend(win.mouse_up(&data.look, w, hh, px, py));
                cmds
            };
            Ok(run(&mut host, cmds))
        })?,
    )?;
    // menu_press(id, f): the left button down on a control, a fraction `f`
    // of the way across it (held: menu_timer(n) fires the window manager's
    // timer n times, a slider pressing on); menu_release() lets go. Each
    // returns what the menu asks for, as menu_click.
    let h = host.clone();
    g.set(
        "menu_press",
        lua.create_function(move |_, (id, f): (i64, f32)| {
            let mut host = h.borrow_mut();
            let (w, hh) = (640.0, 480.0);
            let (cmds, at) = {
                let Some((data, m)) = host.menu.as_mut() else {
                    return Ok(None);
                };
                let Some(win) = m.top_mut() else {
                    return Ok(None);
                };
                let Some(c) = win.control(id).filter(|c| c.visible) else {
                    return Err(mlua::Error::external(format!("no control {id} showing")));
                };
                let o = win.at(w, hh);
                let [x, y, cw, ch] = c.def.rect;
                let at = [o[0] + x + cw * f, o[1] + y + ch * 0.5];
                (win.mouse_down(&data.look, w, hh, at[0], at[1]), at)
            };
            host.menu_held = Some((at, std::time::Instant::now()));
            Ok(run(&mut host, cmds))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "menu_timer",
        lua.create_function(move |_, n: u32| {
            let mut host = h.borrow_mut();
            let Some((at, mut clock)) = host.menu_held else {
                return Ok(None);
            };
            let mut cmds = Vec::new();
            if let Some((data, m)) = host.menu.as_mut() {
                if let Some(win) = m.top_mut() {
                    for _ in 0..n {
                        clock += std::time::Duration::from_millis(200);
                        cmds.extend(win.tick(&data.look, 640.0, 480.0, at[0], at[1], clock));
                    }
                }
            }
            host.menu_held = Some((at, clock));
            Ok(run(&mut host, cmds))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "menu_release",
        lua.create_function(move |_, ()| {
            let mut host = h.borrow_mut();
            let Some((at, _)) = host.menu_held.take() else {
                return Ok(None);
            };
            let cmds = host
                .menu
                .as_mut()
                .and_then(|(data, m)| {
                    Some(
                        m.top_mut()?
                            .mouse_up(&data.look, 640.0, 480.0, at[0], at[1]),
                    )
                })
                .unwrap_or_default();
            Ok(run(&mut host, cmds))
        })?,
    )?;
    // menu_loading(percent): the level-load dialog alone over black, its
    // bar at `percent` (screenshots draw it); menu_loading() hides it.
    let h = host.clone();
    g.set(
        "menu_loading",
        lua.create_function(move |_, percent: Option<u32>| {
            let mut host = h.borrow_mut();
            let Some(p) = percent else {
                host.loading = None;
                return Ok(());
            };
            if host.loading.is_none() {
                let data = crate::hud::menu::Data::load(&host.oni_root.join("common"));
                if !data.has("Progress") {
                    return Err(mlua::Error::external(
                        "no level-load dialog (common/menus.json); reconvert the assets",
                    ));
                }
                let w = crate::hud::menu::Window::new(&data, "Progress");
                host.loading = Some((data, w));
            }
            if let Some((_, w)) = host.loading.as_mut() {
                w.set_progress(100, p);
            }
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "menu_key",
        lua.create_function(move |_, name: String| {
            use crate::hud::menu::Key;
            let key = match name.as_str() {
                "enter" => Key::Enter,
                "escape" => Key::Escape,
                "up" => Key::Up,
                "down" => Key::Down,
                "left" => Key::Left,
                "right" => Key::Right,
                "home" => Key::Home,
                "end" => Key::End,
                "pageup" => Key::PageUp,
                "pagedown" => Key::PageDown,
                _ => return Err(mlua::Error::external(format!("no key {name}"))),
            };
            let mut host = h.borrow_mut();
            let cmds = host
                .menu
                .as_mut()
                .and_then(|(_, m)| m.top_mut())
                .map(|w| w.key(key))
                .unwrap_or_default();
            Ok(run(&mut host, cmds))
        })?,
    )?;
    // look_at(id, x, y, z): an agent looks at a point (as `ai2_lookatme`);
    // look_at(id): it stops looking.
    let h = host.clone();
    g.set(
        "look_at",
        lua.create_function(
            move |_, (id, x, y, z): (usize, Option<f32>, Option<f32>, Option<f32>)| {
                if let Some(crate::ai::Brain::Agent(a)) = h
                    .borrow_mut()
                    .world
                    .fighters
                    .get_mut(id)
                    .and_then(|f| f.brain.as_mut())
                {
                    match (x, y, z) {
                        (Some(x), Some(y), Some(z)) => a.look_at(glam::Vec3::new(x, y, z)),
                        _ => a.stop_looking(),
                    }
                }
                Ok(())
            },
        )?,
    )?;
    // aim_off(id) → its aim off the body, left-right and up-down (degrees).
    let h = host.clone();
    g.set(
        "aim_off",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let f = host.world.fighters.get(id);
            Ok((
                f.map(|f| f.aiming_lr.to_degrees()),
                f.map(|f| f.aiming_ud.to_degrees()),
            ))
        })?,
    )?;
    // Where an agent's eyes look (degrees; nil: along its body).
    let h = host.clone();
    g.set(
        "eye_of",
        lua.create_function(move |_, id: usize| {
            Ok(
                match h
                    .borrow()
                    .world
                    .fighters
                    .get(id)
                    .and_then(|f| f.brain.as_ref())
                {
                    Some(crate::ai::Brain::Agent(a)) => {
                        a.eye.map(|e| e.to_degrees().rem_euclid(360.0))
                    }
                    _ => None,
                },
            )
        })?,
    )?;
    // cinematics() → the cutscene inserts showing: a list of {name, x, y,
    // w, h} (top left and size on the 1024 × 768 virtual screen).
    let h = host.clone();
    g.set(
        "cinematics",
        lua.create_function(move |lua, ()| {
            let host = h.borrow();
            let t = lua.create_table()?;
            for c in &host.world.hud.cinematics.list {
                let e = lua.create_table()?;
                e.set("name", c.name.clone())?;
                e.set("x", c.at[0])?;
                e.set("y", c.at[1])?;
                e.set("w", c.size[0])?;
                e.set("h", c.size[1])?;
                t.push(e)?;
            }
            Ok(t)
        })?,
    )?;
    // overlays_of(id) → the partial-body clips playing on fighter id: a list
    // of {slot = "recoil" | "inventory" | "hit", clip, type, frame}.
    let h = host.clone();
    g.set(
        "overlays_of",
        lua.create_function(move |lua, id: usize| {
            let host = h.borrow();
            let t = lua.create_table()?;
            if let Some(f) = host.world.fighters.get(id) {
                for (slot, o) in f.overlays.iter().enumerate() {
                    let Some(o) = o else { continue };
                    let c = f.motion.clip_of(o.clip);
                    let e = lua.create_table()?;
                    e.set("slot", ["recoil", "inventory", "hit"][slot])?;
                    e.set("clip", f.motion.coll.entries[o.clip].name.clone())?;
                    e.set("type", c.anim_type)?;
                    e.set("frame", o.frame)?;
                    t.push(e)?;
                }
            }
            Ok(t)
        })?,
    )?;
    // timer() → (what it shows now or nil, ticks left, mode "on" | "fail"),
    // or nil with no countdown running (`world/timer.rs`).
    let h = host.clone();
    g.set(
        "timer",
        lua.create_function(move |_, ()| {
            let host = h.borrow();
            let w = &host.world;
            Ok(match w.timer.as_ref() {
                Some(t) => (
                    w.timer_text(),
                    Some(t.ticks),
                    Some(if t.mode == crate::world::timer::Mode::On {
                        "on"
                    } else {
                        "fail"
                    }),
                ),
                None => (None, None, None),
            })
        })?,
    )?;
    // passive_of(id) → whether fighter id's AI is passive (`ai2_passive`, a
    // cutscene); script_id_of(id) → the id the scripts know it by (the
    // player 0, a `chr_create`d character its setup's; nil for the rest).
    let h = host.clone();
    g.set(
        "passive_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow().world.fighters.get(id).map(|f| f.passive))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "script_id_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow().world.fighters.get(id).and_then(|f| {
                if id == 0 {
                    Some(0)
                } else {
                    f.script_id
                }
            }))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "goal_of",
        lua.create_function(move |_, id: usize| {
            Ok(
                match h
                    .borrow()
                    .world
                    .fighters
                    .get(id)
                    .and_then(|f| f.brain.as_ref())
                {
                    Some(crate::ai::Brain::Agent(a)) => a.describe(),
                    Some(_) => "other".to_string(),
                    None => "none".to_string(),
                },
            )
        })?,
    )?;

    // neutral_of(id) → fighter id's neutral interaction (`docs/NEUTRAL.md`):
    // {behavior (id), with (whom it talks with), line (0-based, nil before
    // the conversation), accepted, begun, items_given, completed, resume_at}
    // — nil without a behaviour or an interaction.
    let h = host.clone();
    g.set(
        "neutral_of",
        lua.create_function(move |lua, id: usize| {
            let host = h.borrow();
            let w = &host.world;
            let Some(f) = w.fighters.get(id) else {
                return Ok(None);
            };
            if f.neutral.behavior.is_none() && f.neutral_with.is_none() {
                return Ok(None);
            }
            let t = lua.create_table()?;
            t.set("behavior", f.neutral.behavior)?;
            t.set("with", f.neutral_with)?;
            t.set("begun", f.neutral.begun)?;
            t.set("items_given", f.neutral.items_given)?;
            t.set("completed", f.neutral.completed)?;
            t.set("resume_at", f.neutral.resume_at)?;
            if let Some(r) = w.neutral_runs.get(&id) {
                t.set("line", r.line)?;
                t.set("accepted", r.accepted)?;
            }
            Ok(Some(t))
        })?,
    )?;

    // Looks along the floor from fighter `id` toward yaw `deg` up to `dist`:
    // the free distance before a wall, and the distance to a ledge (or nil).
    let h = host.clone();
    g.set(
        "probe",
        lua.create_function(move |_, (id, deg, dist): (usize, f32, f32)| {
            let host = h.borrow();
            let Some(f) = host.world.fighters.get(id) else {
                return Ok((0.0, None));
            };
            let a = deg.to_radians();
            let env = crate::chrphys::Env::new(&host.world.quads, &[]);
            let radius = if f.player {
                crate::chrphys::PLAYER_LEAF
            } else {
                crate::chrphys::AI_LEAF
            };
            let p = crate::chrphys::probe(
                &env,
                f.feet(),
                radius,
                Vec3::new(a.sin(), 0.0, a.cos()),
                dist,
                10.0,
            );
            Ok((p.free, p.ledge))
        })?,
    )?;

    // Navigation: fighter `id` walks (pace "run" / "walk") to a point
    // through the level; nav_state(id) reads "planning" / "following" /
    // "arrived" / "failed: …".
    let h = host.clone();
    g.set(
        "go_to",
        lua.create_function(
            move |_, (id, x, y, z, pace): (usize, f32, f32, f32, Option<String>)| {
                let pace = match pace.as_deref() {
                    Some("walk") => crate::ai::Pace::Walk,
                    _ => crate::ai::Pace::Run,
                };
                if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                    f.passive = false;
                    let n = crate::ai::navigate::Navigator::new(Vec3::new(x, y, z), 3.0, pace);
                    f.brain = Some(crate::ai::Brain::GoTo(Box::new(n)));
                }
                Ok(())
            },
        )?,
    )?;
    let h = host.clone();
    g.set(
        "nav_path",
        lua.create_function(move |lua, id: usize| {
            let t = lua.create_table()?;
            let host = h.borrow();
            let nav = match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::GoTo(n)) => Some(&**n),
                Some(crate::ai::Brain::Agent(a)) => a.nav.as_ref(),
                _ => None,
            };
            if let Some(n) = nav {
                let (points, next) = n.waypoints();
                for (i, (p, r)) in points.iter().enumerate() {
                    let w = lua.create_table()?;
                    w.set(1, p.x)?;
                    w.set(2, p.z)?;
                    w.set(3, *r)?;
                    w.set(4, i == next)?;
                    t.set(i + 1, w)?;
                }
            }
            Ok(t)
        })?,
    )?;
    let h = host.clone();
    g.set(
        "nav_state",
        lua.create_function(move |_, id: usize| {
            use crate::ai::navigate::State;
            let host = h.borrow();
            let nav = match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::GoTo(n)) => Some(&**n),
                Some(crate::ai::Brain::Agent(a)) => a.nav.as_ref(),
                _ => None,
            };
            Ok(match nav {
                Some(n) => match &n.state {
                    State::Planning => "planning".to_string(),
                    State::Following => "following".to_string(),
                    State::Arrived => "arrived".to_string(),
                    State::Failed(why) => format!("failed: {why}"),
                },
                _ => "none".to_string(),
            })
        })?,
    )?;

    // Perception: make a placed character an AI that watches and listens
    // (not a passive dummy); its alert; what it knows about another; teams;
    // making a noise.
    let h = host.clone();
    g.set(
        "watch",
        lua.create_function(move |_, id: usize| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.passive = false;
            }
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "alert_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .and_then(|f| f.perception.as_ref())
                .map(|p| p.alert.name()))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "contact_of",
        lua.create_function(move |_, (id, other): (usize, usize)| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .and_then(|f| f.perception.as_ref())
                .and_then(|p| p.about(other))
                .map(|c| c.strength.name()))
        })?,
    )?;
    let h = host.clone();
    g.set(
        "team",
        lua.create_function(move |_, (id, name): (usize, String)| {
            if let Some(p) = h
                .borrow_mut()
                .world
                .fighters
                .get_mut(id)
                .and_then(|f| f.perception.as_mut())
            {
                p.team = crate::ai::perception::Team::from_name(&name);
            }
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "noise",
        lua.create_function(move |_, (x, y, z, kind, volume, maker): (f32, f32, f32, String, f32, Option<usize>)| {
            use crate::ai::perception::Stimulus;
            let stimulus = match kind.as_str() {
                "interesting" => Stimulus::SoundInteresting,
                "danger" => Stimulus::SoundDanger,
                "melee" => Stimulus::SoundMelee,
                "gunshot" => Stimulus::SoundGunshot,
                _ => Stimulus::SoundIgnore,
            };
            h.borrow_mut().world.sound(stimulus, Vec3::new(x, y, z), volume, maker, None);
            Ok(())
        })?,
    )?;

    // The AI's difficulty: "easy", "normal" or "hard".
    let h = host.clone();
    g.set(
        "difficulty",
        lua.create_function(move |_, level: String| {
            let mut host = h.borrow_mut();
            let d = host.classes.difficulty(&level);
            host.world.difficulty = d;
            Ok((d.notice, d.dodge, d.block))
        })?,
    )?;

    // Test hook: make melee AI `id` use technique `name` next (once usable).
    let h = host.clone();
    g.set(
        "technique",
        lua.create_function(move |_, (id, name): (usize, String)| {
            if let Some(crate::ai::Brain::Melee(m)) = h
                .borrow_mut()
                .world
                .fighters
                .get_mut(id)
                .and_then(|f| f.brain.as_mut())
            {
                m.force = Some(name);
            }
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "class_of",
        lua.create_function(move |_, id: usize| Ok(h.borrow().class_names.get(id).cloned()))?,
    )?;

    let h = host.clone();
    g.set(
        "count",
        lua.create_function(move |_, ()| Ok(h.borrow().world.fighters.len() as u32))?,
    )?;

    let h = host.clone();
    g.set(
        "hp",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow().world.fighters.get(id).map(|f| f.hp).unwrap_or(0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "pos_of",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let f = host
                .world
                .fighters
                .get(id)
                .map(|f| f.feet())
                .unwrap_or(Vec3::ZERO);
            Ok((f.x, f.y, f.z))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "anim_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow()
                .world
                .fighters
                .get(id)
                .map(|f| f.motion.name().to_string())
                .unwrap_or_default())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "step",
        lua.create_function(move |_, n: Option<u32>| {
            h.borrow_mut().step(n.unwrap_or(1));
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "cam",
        lua.create_function(
            move |_, (ex, ey, ez, tx, ty, tz): (f32, f32, f32, f32, f32, f32)| {
                let mut hb = h.borrow_mut();
                hb.eye = Some(Vec3::new(ex, ey, ez));
                hb.target = Some(Vec3::new(tx, ty, tz));
                Ok(())
            },
        )?,
    )?;

    let h = host.clone();
    g.set(
        "pos",
        lua.create_function(move |_, ()| {
            let f = h
                .borrow()
                .world
                .player()
                .map(|p| p.feet())
                .unwrap_or(Vec3::ZERO);
            Ok((f.x, f.y, f.z))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "grounded",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.physics.grounded)
                .unwrap_or(false))
        })?,
    )?;

    // The current clip-to-clip stitch blend weight (0 = not blending).
    let h = host.clone();
    g.set(
        "stitch",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .and_then(|p| p.motion.blend_pose())
                .map(|(_, t)| t)
                .unwrap_or(0.0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "anim",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .world
                .player()
                .map(|p| p.motion.name().to_string())
                .unwrap_or_default())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "screenshot",
        lua.create_function(
            move |_, (path, w, h2): (String, Option<u32>, Option<u32>)| {
                h.borrow_mut()
                    .screenshot(Path::new(&path), w.unwrap_or(800), h2.unwrap_or(600));
                Ok(())
            },
        )?,
    )?;

    Ok(())
}
