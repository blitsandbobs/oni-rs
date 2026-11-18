//! A tiny headless Lua driver for the engine (dev/test harness).
//!
//! This exists so scenario validation — "spawn Konoko, run forward 1s, put the
//! camera here, screenshot" — lives in `.lua` files instead of accreting bespoke
//! CLI flags and throwaway Rust test binaries. It is deliberately scoped to the
//! systems that already exist (level + character + physics + locomotion state
//! machine + offscreen render); the binding surface grows as the engine does,
//! and later becomes the substrate for the gameplay-script layer (E4).
//!
//! Lua is confined to `oni-view` (the app/harness crate); the portable
//! preservation crates (`oni-geom`, `oni-convert`) stay dependency-light.
//!
//! ## Lua API (all headless)
//! - `input(forward, backward)` — set the move keys (W/S)
//! - `strafe(left, right)` — set the strafe keys (A/D sidestep)
//! - `walk(on)` — the hold-to-walk modifier
//! - `facing(deg)` — snap the body + target to an absolute yaw
//! - `turn(delta_deg)` — nudge the facing *target* (body slews toward it)
//! - `facing_deg()` / `desired_deg()` — read the body yaw / target yaw
//! - `step(n)` — advance `n` 60 Hz ticks (state machine + physics)
//! - `cam(ex,ey,ez, tx,ty,tz)` — place the camera (eye, look-at target)
//! - `pos()` — returns the character's feet `x, y, z`
//! - `grounded()` — whether the character is currently on the ground
//! - `stitch()` — the current clip-blend weight (0 = not blending)
//! - `anim()` — returns the current clip name
//! - `screenshot(path [, w, h])` — render one frame to a PNG

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use glam::{Mat4, Quat, Vec3};
use mlua::Lua;

use crate::anim_state::{Input, Locomotion};
use crate::physics::Physics;
use crate::render::{Gpu, Renderer};
use crate::CharView;

/// Everything a script drives: one level, one character, its physics + state
/// machine, a camera, and a reusable offscreen renderer.
struct ScriptHost {
    gpu: Gpu,
    renderer: Renderer,
    format: wgpu::TextureFormat,
    scene: crate::gltf::Scene,
    charview: CharView,
    physics: Physics,
    loco: Option<Locomotion>,
    foot_offset: f32,
    facing: crate::control::Facing,
    input: Input,
    eye: Option<Vec3>,
    target: Option<Vec3>,
}

impl ScriptHost {
    fn new(
        scene: crate::gltf::Scene,
        charview: CharView,
        particles: &[crate::particles::Particle],
        loco: Option<Locomotion>,
    ) -> Self {
        let gpu = pollster::block_on(Gpu::new(None));
        eprintln!("  GPU: {}", gpu.adapter.get_info().name);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let renderer = Renderer::new(&gpu, &scene, Some(&charview.character), particles, format);

        // Capsule + physics from the level mesh and the character's bounds
        // (same setup as play mode).
        let (verts, idx) = crate::level_collision_mesh(&scene);
        let (mn, mx) = charview.aabb(0.0);
        let origin = charview.placement.transform_point3(Vec3::ZERO);
        let foot_offset = mn.y - origin.y;
        let height = (mx.y - mn.y).max(2.0);
        let radius = ((mx.x - mn.x).max(mx.z - mn.z) * 0.5).clamp(0.5, 3.0);
        let feet = Vec3::new(origin.x, origin.y + foot_offset, origin.z);
        let physics = Physics::new(&verts, &idx, feet, height, radius);

        ScriptHost {
            gpu,
            renderer,
            format,
            scene,
            charview,
            physics,
            loco,
            foot_offset,
            facing: crate::control::Facing::new(0.0),
            input: Input::default(),
            eye: None,
            target: None,
        }
    }

    /// Advances `n` ticks: locomotion state machine (if any) → root motion →
    /// physics, then re-places the character at its settled feet + facing.
    fn step(&mut self, n: u32) {
        let moving = self.input.forward
            || self.input.backward
            || self.input.step_left
            || self.input.step_right;
        let turn_cap = if self.input.walk {
            crate::control::SLOW_TURN
        } else {
            crate::control::MED_TURN
        };
        for _ in 0..n {
            // Turn-in-place intent, then tick the state machine, then slew the
            // body toward the desired facing (Oni's heartbeat order).
            let offset = self.facing.desired_offset();
            self.input.turn_left = !moving && offset > crate::control::TURN_DEADZONE;
            self.input.turn_right = !moving && offset < -crate::control::TURN_DEADZONE;
            let local = match &mut self.loco {
                Some(l) => {
                    let d = l.tick(self.input, Some(&self.charview.character));
                    Vec3::new(d.x, 0.0, d.y)
                }
                None => Vec3::ZERO,
            };
            let world = Quat::from_rotation_y(self.facing.facing) * local;
            self.physics.step(world);
            self.facing.turn_toward(turn_cap);
        }
        let feet = self.physics.feet;
        // Pelvis elevation: the locomotion's per-frame `heightThisFrame` (Oni's
        // `location + height`), falling back to the fixed bind offset when no
        // state machine is driving (a static clip).
        let pelvis_y = match &self.loco {
            Some(l) => feet.y + l.height(),
            None => feet.y - self.foot_offset,
        };
        let origin = Vec3::new(feet.x, pelvis_y, feet.z);
        self.charview.placement =
            Mat4::from_rotation_translation(Quat::from_rotation_y(self.facing.facing), origin);
    }

    fn screenshot(&mut self, path: &Path, w: u32, h: u32) {
        let aspect = w as f32 / h as f32;
        let t = self.loco.as_ref().map(|l| l.frame_time()).unwrap_or(0.0);
        let mut cam = crate::frame_character(&self.charview, aspect, t);
        match (self.eye, self.target) {
            (Some(e), Some(tg)) => cam.look_at(e, tg),
            (Some(e), None) => cam.look_at(e, self.scene.center()),
            _ => {}
        }
        let bones = match &self.loco {
            Some(l) => self
                .charview
                .bones_with(l.clip(), l.frame_time(), l.blend()),
            None => self.charview.bones(t),
        };
        let basis = cam.billboard_basis();
        let rgba = crate::render_to_rgba(
            &self.gpu,
            &mut self.renderer,
            self.format,
            (w, h),
            cam.view_proj(),
            &bones,
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
    loco: Option<Locomotion>,
) {
    let host = Rc::new(RefCell::new(ScriptHost::new(
        scene, charview, &particles, loco,
    )));
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
            host.input.forward = f;
            host.input.backward = b;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "strafe",
        lua.create_function(move |_, (l, r): (bool, bool)| {
            let mut host = h.borrow_mut();
            host.input.step_left = l;
            host.input.step_right = r;
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "walk",
        lua.create_function(move |_, on: bool| {
            h.borrow_mut().input.walk = on;
            Ok(())
        })?,
    )?;

    // `facing(deg)` snaps the body (and its target); `turn(delta)` nudges the
    // target so the body slews toward it over subsequent ticks (mouse turn).
    let h = host.clone();
    g.set(
        "facing",
        lua.create_function(move |_, deg: f32| {
            h.borrow_mut().facing = crate::control::Facing::new(deg.to_radians());
            Ok(())
        })?,
    )?;

    let h = host.clone();
    g.set(
        "turn",
        lua.create_function(move |_, delta: f32| {
            h.borrow_mut().facing.adjust_desired(delta.to_radians());
            Ok(())
        })?,
    )?;

    // Read-back of the body yaw and the (mouse) target, in degrees.
    let h = host.clone();
    g.set(
        "facing_deg",
        lua.create_function(move |_, ()| Ok(h.borrow().facing.facing.to_degrees()))?,
    )?;

    let h = host.clone();
    g.set(
        "desired_deg",
        lua.create_function(move |_, ()| Ok(h.borrow().facing.desired_facing.to_degrees()))?,
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
            let f = h.borrow().physics.feet;
            Ok((f.x, f.y, f.z))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "grounded",
        lua.create_function(move |_, ()| Ok(h.borrow().physics.grounded))?,
    )?;

    // The current clip-to-clip stitch blend weight (0 = not blending).
    let h = host.clone();
    g.set(
        "stitch",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .loco
                .as_ref()
                .and_then(|l| l.blend())
                .map(|(_, t)| t)
                .unwrap_or(0.0))
        })?,
    )?;

    let h = host.clone();
    g.set(
        "anim",
        lua.create_function(move |_, ()| {
            Ok(h.borrow()
                .loco
                .as_ref()
                .map(|l| l.clip_name().to_string())
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
