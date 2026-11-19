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
use crate::camera::Camera;
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
    /// Controls the player holds across steps (`anim::input::key` bits).
    held: u32,
    /// Buttons tapped for the next step's first tick only.
    taps: u32,
    eye: Option<Vec3>,
    target: Option<Vec3>,
    /// The level's own scripts, once begun (`mission_begin`).
    mission: Option<crate::mission::Mission>,
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
    }    fn clip(&mut self, name: &str) -> Option<std::rc::Rc<crate::anim::collection::AnimCollection>> {
        self.classes.clip(name)
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

        let (verts, idx) = crate::level_collision_mesh(&scene);
        let mut world = World::new(verts, idx);
        world.floor_textures = crate::floor_textures(&scene);
        if faithful {
            world.set_quirks(Quirks::FAITHFUL);
        }
        world.difficulty = classes.difficulty(&cast.difficulty);
        world.nav = crate::nav::Navigation::load(&cast.level_dir).ok().map(Rc::new);
        world.load_level(&cast.level_dir);
        let (ov, oi) = crate::opaque_mesh(&scene);
        world.sight = Some(crate::physics::Sight::new(&ov, &oi));
        world.attach_doors(crate::door_meshes(&scene));
        let (solid, opaque) = crate::env_parts(&scene);
        world.attach_env(solid, opaque);
        world.attach_glass(crate::glass_parts(&scene));
        world.attach_weapons(&cast.level_dir.join("../../common"));
        if world.nav.is_none() {
            eprintln!("  (no navigation data for this level)");
        }
        // Player = fighter 0, facing +Z.
        world.spawn(&kind, spawn_pos, 0.0, false);

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
            held: 0,
            taps: 0,
            eye: None,
            target: None,
            mission: None,
        })
    }

    /// The spawnable kind for class `name` (loaded on first use).
    fn kind(&mut self, name: &str) -> Result<CharKind, String> {
        let (renderer, gpu) = (&mut self.renderer, &self.gpu);
        self.classes.kind(name, |ch| renderer.add_character(gpu, ch))
    }

    /// Advances `n` ticks: drive the player from the held controls (plus any
    /// tapped buttons on the first tick), the dummies idle, then resolve melee
    /// hits — exactly as play mode does.
    fn step(&mut self, n: u32) {
        let mut taps = std::mem::take(&mut self.taps);
        for _ in 0..n {
            let controls = self.held | taps;
            taps = 0;
            self.world.step(controls);
            if let Some(mut m) = self.mission.take() {
                m.tick(self);
                self.mission = Some(m);
            }
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
        let mut cam = Camera::new(aspect);
        match self.world.player() {
            Some(p) => {
                let c = p.feet() + Vec3::new(0.0, 6.0, 0.0);
                cam.look_at(c + Vec3::new(14.0, 8.0, -22.0), c);
            }
            None => cam = crate::default_camera(&self.scene, aspect),
        }
        match (self.eye, self.target) {
            (Some(e), Some(tg)) => cam.look_at(e, tg),
            (Some(e), None) => cam.look_at(e, self.scene.center()),
            _ => {}
        }
        self.renderer.open_doors = self.world.doors.iter().filter(|d| !d.solid()).map(|d| d.object_id).collect();
        self.renderer.beams = self.world.beams.clone();
        self.renderer.sprites = self.world.sprites();
        self.renderer.decals = if self.show_decals { self.world.decal_draws() } else { Vec::new() };
        self.renderer.hidden_refs = self.world.env_hidden.clone();
        self.renderer.broken_glass = self.world.broken_glass.clone();
        let bones: Vec<Vec<Mat4>> = self.world.fighters.iter().map(|f| f.bones()).collect();
        let mut guns: Vec<(usize, Vec<Mat4>)> = self
            .world
            .gun_draws()
            .into_iter()
            .chain(self.world.particle_models())
            .chain(self.world.turret_draws())
            .filter_map(|(path, m)| Some((self.renderer.model_from(&self.gpu, &path)?, vec![m])))
            .collect();
        // (Consoles' screens, textured by their state.)
        for (path, tex, m) in self.world.console_draws() {
            if let Some(k) = self.renderer.model_textured(&self.gpu, &path, &tex) {
                guns.push((k, vec![m]));
            }
        }
        let draws: Vec<CharDraw> = self
            .world
            .fighters
            .iter()
            .zip(&bones)
            .map(|(f, b)| f.draw(b))
            .chain(guns.iter().map(|(m, b)| CharDraw { model: *m, bones: b }))
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
        lua.create_function(move |_, on: bool| {
            let q = if on { Quirks::FAITHFUL } else { Quirks::FIXED };
            h.borrow_mut().world.set_quirks(q);
            Ok(())
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
        lua.create_function(move |_, id: usize| Ok(h.borrow().world.fighters.get(id).is_some_and(|f| !f.gone)))?,
    )?;

    // Deals damage to fighter `id` (no reaction unless it dies).
    let h = host.clone();
    g.set(
        "damage",
        lua.create_function(move |_, (id, n): (usize, i32)| {
            if let Some(f) = h.borrow_mut().world.fighters.get_mut(id) {
                f.take_damage(n, Vec3::ZERO);
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
            move |_, (id, points, pace, looping): (usize, mlua::Table, Option<String>, Option<bool>)| {
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
                    f.brain = Some(Brain::Hold(Orders { direction, pace, face }));
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
                    f.brain = Some(crate::ai::Brain::Spar(crate::ai::Spar::new(target, id as u32 + 1)));
                }
            }
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "route_done",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow().world.fighters.get(id).is_some_and(|f| match &f.brain {
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
            Ok(h.borrow().world.fighters.get(id).map(|f| f.motion.kind).unwrap_or(0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "state_of",
        lua.create_function(move |_, id: usize| {
            Ok(h.borrow().world.fighters.get(id).map(|f| f.motion.state).unwrap_or(0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "facing_of",
        lua.create_function(move |_, id: usize| {
            Ok(h
                .borrow()
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
            Ok(h.borrow().world.fighters.get(id).is_some_and(|f| f.motion.dying))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "state",
        lua.create_function(move |_, ()| {
            Ok(h.borrow().world.player().map(|p| p.motion.state).unwrap_or(0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "anim_type",
        lua.create_function(move |_, ()| {
            Ok(h.borrow().world.player().map(|p| p.motion.kind).unwrap_or(0))
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
            Ok(h.borrow().world.player().map(|p| p.airborne()).unwrap_or(false))
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
            Ok(h
                .borrow()
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
            Ok(h
                .borrow()
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
            Ok(h
                .borrow()
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
            let id = host.world.spawn(&kind, placed.at + Vec3::Y * 10.0, placed.facing, false);
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
        lua.create_function(move |_, ()| {
            let mut host = h.borrow_mut();
            let mut m = crate::mission::Mission::load(&host.world.level_dir).map_err(mlua::Error::runtime)?;
            m.begin(&mut *host);
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
        lua.create_function(move |_, (id, name, mode): (usize, String, Option<String>)| {
            Ok(h.borrow_mut().world.play_film(id, &name, mode.as_deref().unwrap_or(""), 30))
        })?,
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
            for k in h.borrow_mut().world.consoles.iter_mut().filter(|k| k.id == id) {
                k.triggered = false;
                k.active = k.initial_active;
            }
            Ok(())
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
        lua.create_function(move |_, (id, class): (usize, String)| Ok(h.borrow_mut().world.give_weapon(id, &class)))?,
    )?;
    let h = host.clone();
    g.set(
        "weapon",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let w = &host.world;
            let gun = w.fighters.get(id).and_then(|f| f.hand).map(|k| &w.weapons()[k]);
            Ok((gun.map(|g| g.class.name.clone()), gun.map(|g| g.ammo), gun.map(|g| g.firing)))
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
    let h = host.clone();
    g.set(
        "aim_of",
        lua.create_function(move |_, id: usize| Ok(h.borrow().world.fighters.get(id).map_or(0.0, |f| f.aiming_ud)))?,
    )?;
    let h = host.clone();
    g.set("reload", lua.create_function(move |_, id: usize| Ok(h.borrow_mut().world.reload(id)))?)?;
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
    let h = host.clone();
    g.set("shots", lua.create_function(move |_, ()| Ok(h.borrow().world.particles().iter().count()))?)?;
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
        lua.create_function(move |_, (class, x, y, z): (String, f32, f32, f32)| {
            Ok(h.borrow_mut().world.spawn_weapon(&class, Vec3::new(x, y, z), 0.0, true).is_some())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "holster_of",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let w = &host.world;
            Ok(w.fighters.get(id).and_then(|f| f.holster).map(|k| w.weapons()[k].class.name.clone()))
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
        lua.create_function(move |_, (name, x, y, z, volume): (String, Option<f32>, Option<f32>, Option<f32>, Option<f32>)| {
            let mut host = h.borrow_mut();
            let at = match (x, y, z) {
                (Some(x), Some(y), Some(z)) => Some(Vec3::new(x, y, z)),
                _ => None,
            };
            let w = &mut host.world;
            Ok(w.audio.impulse_play(&name, at, volume, w.sight.as_ref()))
        })?,
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
            for (i, (ambient, at, is_box, state, volume, playing)) in host.world.audio.sound_objects().into_iter().enumerate() {
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
        lua.create_function(move |_, (id, kind, amount): (usize, String, Option<u16>)| Ok(h.borrow_mut().world.give_powerup(id, &kind, amount, false)))?,
    )?;
    // env_particles(tag) → how many of the level's environmental particles
    // tagged `tag` are alive; particle(tag, command [, event]) sends them a
    // command as scripts do.
    let h = host.clone();
    g.set("env_particles", lua.create_function(move |_, tag: String| Ok(h.borrow().world.env_particles_alive(&tag)))?)?;
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
                let Some(p) = particles.get(d.particle) else { continue };
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
            let f = host.world.fighters.get(id).ok_or_else(|| mlua::Error::runtime("no such fighter"))?;
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
                let mut v = vec![f64::from(at.x), f64::from(at.y), f64::from(at.z), f64::from(o.x), f64::from(o.y), f64::from(o.z)];
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
    // show_decals(on): whether screenshots draw the decals (to compare).
    let h = host.clone();
    g.set("show_decals", lua.create_function(move |_, on: bool| {
        h.borrow_mut().show_decals = on;
        Ok(())
    })?)?;
    // quad_plane(q) → a, b, c, d: a level quad's plane.
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
        lua.create_function(move |_, (tag, command, event): (String, String, Option<String>)| {
            Ok(h.borrow_mut().world.env_particle_command(&tag, &command, event.as_deref()))
        })?,
    )?;
    // console_screen(id) → the texture console `id`'s screen shows now (by
    // its state), or nil.
    let h = host.clone();
    g.set(
        "console_screen",
        lua.create_function(move |_, id: u16| {
            let host = h.borrow();
            Ok(host.world.consoles.iter().find(|c| c.id == id).map(crate::world::World::screen_of))
        })?,
    )?;
    // dialog() → whether a scripted dialogue line is on.
    let h = host.clone();
    g.set("dialog", lua.create_function(move |_, ()| Ok(h.borrow().world.audio.dialog_playing()))?)?;
    // saying(id) → the line a character is saying (an ambient's name) or nil.
    let h = host.clone();
    g.set(
        "saying",
        lua.create_function(move |_, id: usize| Ok(h.borrow().world.fighters.get(id).and_then(|f| f.speech.saying().map(str::to_string))))?,
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
            let Some(t) = host.world.turrets.iter().find(|t| t.id == id) else { return Ok(mlua::Value::Nil) };
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
            if let Some(crate::ai::Brain::Agent(a)) = h.borrow_mut().world.fighters.get_mut(id).and_then(|f| f.brain.as_mut()) {
                a.class.gun_pickup_percent = percent;
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
        lua.create_function(move |_, (from, to): (u32, Option<u32>)| Ok(h.borrow().world.env_broken(from, to.unwrap_or(from))))?,
    )?;
    let h = host.clone();
    g.set(
        "inventory",
        lua.create_function(move |_, id: usize| {
            let host = h.borrow();
            let i = host.world.fighters.get(id).map(|f| f.inventory).unwrap_or_default();
            Ok((i.ammo, i.cells, i.hypos))
        })?,
    )?;
    let h = host.clone();
    g.set("powerups_lying", lua.create_function(move |_, ()| Ok(h.borrow().world.powerups().len()))?)?;
    let h = host.clone();
    g.set(
        "weapons_lying",
        lua.create_function(move |_, ()| Ok(h.borrow().world.weapons().iter().filter(|g| g.owner.is_none()).count()))?,
    )?;

    // Orders laser trigger `id` ("activate", "deactivate", "reset", "hide",
    // "show", "speed" with a factor); laser_state(id): active, on,
    // triggered, and how many beams are out (nil: no such laser).
    let h = host.clone();
    g.set(
        "laser",
        lua.create_function(move |_, (id, what, factor): (i64, String, Option<f32>)| {
            h.borrow_mut().world.laser_set(id, &what, factor.unwrap_or(1.0));
            Ok(())
        })?,
    )?;
    let h = host.clone();
    g.set(
        "laser_state",
        lua.create_function(move |_, id: u16| {
            let host = h.borrow();
            let l = host.world.lasers.iter().find(|l| l.id == id);
            Ok((l.map(|l| l.active), l.map(|l| l.laser_on), l.map(|l| l.triggered), l.map(|l| if l.beaming() { l.emitters.len() } else { 0 })))
        })?,
    )?;
    // Laser beams out now: a list of {x1, y1, z1, x2, y2, z2}.
    let h = host.clone();
    g.set(
        "beams",
        lua.create_function(move |_, ()| {
            Ok(h.borrow().world.beams.iter().map(|(a, b, _)| vec![a.x, a.y, a.z, b.x, b.y, b.z]).collect::<Vec<_>>())
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
            Ok(h.borrow().world.consoles.iter().find(|k| k.id == id).map(|k| k.screen()))
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
                None => Err(mlua::Error::external("no mission (call mission_begin first)")),
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
    // Where an agent's eyes look (degrees; nil: along its body).
    let h = host.clone();
    g.set(
        "eye_of",
        lua.create_function(move |_, id: usize| {
            Ok(match h.borrow().world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::Agent(a)) => a.eye.map(|e| e.to_degrees().rem_euclid(360.0)),
                _ => None,
            })
        })?,
    )?;
    let h = host.clone();
    g.set(
        "goal_of",
        lua.create_function(move |_, id: usize| {
            Ok(match h.borrow().world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::Agent(a)) => a.describe(),
                Some(_) => "other".to_string(),
                None => "none".to_string(),
            })
        })?,
    )?;

    // Looks along the floor from fighter `id` toward yaw `deg` up to `dist`:
    // the free distance before a wall, and the distance to a ledge (or nil).
    let h = host.clone();
    g.set(
        "probe",
        lua.create_function(move |_, (id, deg, dist): (usize, f32, f32)| {
            let host = h.borrow();
            let Some(f) = host.world.fighters.get(id) else { return Ok((0.0, None)) };
            let a = deg.to_radians();
            let p = f.physics.probe(Vec3::new(a.sin(), 0.0, a.cos()), dist, 10.0);
            Ok((p.free, p.ledge))
        })?,
    )?;

    // Navigation: fighter `id` walks (pace "run" / "walk") to a point
    // through the level; nav_state(id) reads "planning" / "following" /
    // "arrived" / "failed: …".
    let h = host.clone();
    g.set(
        "go_to",
        lua.create_function(move |_, (id, x, y, z, pace): (usize, f32, f32, f32, Option<String>)| {
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
        })?,
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
            Ok(h.borrow().world.fighters.get(id).and_then(|f| f.perception.as_ref()).map(|p| p.alert.name()))
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
            if let Some(p) = h.borrow_mut().world.fighters.get_mut(id).and_then(|f| f.perception.as_mut()) {
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
            if let Some(crate::ai::Brain::Melee(m)) = h.borrow_mut().world.fighters.get_mut(id).and_then(|f| f.brain.as_mut()) {
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
            Ok(h
                .borrow()
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
            let f = h.borrow().world.player().map(|p| p.feet()).unwrap_or(Vec3::ZERO);
            Ok((f.x, f.y, f.z))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "grounded",
        lua.create_function(move |_, ()| {
            Ok(h
                .borrow()
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
            Ok(h
                .borrow()
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
            Ok(h
                .borrow()
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
