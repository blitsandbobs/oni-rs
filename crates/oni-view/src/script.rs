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
    /// Controls the player holds across steps (`anim::input::key` bits).
    held: u32,
    /// Buttons tapped for the next step's first tick only.
    taps: u32,
    eye: Option<Vec3>,
    target: Option<Vec3>,
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
        if faithful {
            world.set_quirks(Quirks::FAITHFUL);
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
            held: 0,
            taps: 0,
            eye: None,
            target: None,
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
        let bones: Vec<Vec<Mat4>> = self.world.fighters.iter().map(|f| f.bones()).collect();
        let draws: Vec<CharDraw> = self
            .world
            .fighters
            .iter()
            .zip(&bones)
            .map(|(f, b)| f.draw(b))
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
