//! The world's part of a frame, shared by the play app and the scenario
//! host so both draw the same thing: the renderer's per-frame world state,
//! the characters' poses, the one-bone models (guns, particles' models,
//! turrets, powerups, placed objects, consoles), and the HUD with a
//! cutscene skip's fade.

use glam::{Mat4, Vec3};

use crate::render::{CharDraw, Gpu, ModelDraw, OverlayDraw, Renderer};
use crate::world::World;

/// A frame's poses and models, built once and drawn from.
pub struct WorldFrame {
    bones: Vec<Vec<Mat4>>,
    models: Vec<ModelDraw>,
}

impl WorldFrame {
    /// Hands the renderer the world's state for a frame seen from `eye`
    /// (decals only when `decals`), and poses everything to draw.
    pub fn build(
        world: &World,
        renderer: &mut Renderer,
        gpu: &Gpu,
        eye: Vec3,
        decals: bool,
    ) -> Self {
        renderer.door_motion = world.doors.motions();
        renderer.beams = world.beams.clone();
        renderer.sprites = world.sprites();
        renderer.decals = if decals {
            world.decal_draws(eye)
        } else {
            Vec::new()
        };
        renderer.hidden_refs = world.parts.hidden.clone();
        renderer.env_shades = world.parts.shades.clone();
        renderer.door_shades = world.doors.shades.clone();
        renderer.broken_glass = world.glass.broken.clone();
        renderer.jello = world.see_through.quads.clone();
        renderer.jello_doors = world.see_through.doors.iter().copied().collect();
        renderer.anim_time = world.game_tick();
        renderer.eye = eye;
        renderer.fog = world.fog.fog;
        let bones: Vec<Vec<Mat4>> = world.fighters.iter().map(|f| f.bones()).collect();
        // The magic effects over the bodies (shields, invisibility).
        renderer.magic = world.magic_draws(&bones);
        // One-bone models placed by their matrices.
        let mut models: Vec<ModelDraw> = Vec::new();
        let mut add = |k: Option<usize>, m: Mat4, see_through: bool, alpha: f32, shade| {
            if let Some(k) = k {
                models.push((k, vec![m], see_through, alpha, shade));
            }
        };
        for (path, m) in world
            .gun_draws()
            .into_iter()
            .chain(world.particle_models())
            .chain(world.turret_draws())
            .chain(world.powerup_draws())
        {
            add(renderer.model_from(gpu, &path), m, false, 1.0, None);
        }
        // (Placed objects, some lit flat.)
        for (path, m, shade) in world.object_draws() {
            add(renderer.model_from(gpu, &path), m, false, 1.0, shade);
        }
        // (Objects in the camera's way, see-through.)
        for (path, m) in world.object_jello_draws() {
            add(renderer.model_from(gpu, &path), m, true, 1.0, None);
        }
        // (Guns held by bodies fading out.)
        for (path, m, a) in world.faded_gun_draws() {
            add(renderer.model_from(gpu, &path), m, false, a, None);
        }
        // (Consoles' screens, textured by their state.)
        for (path, tex, m) in world.console_draws() {
            add(
                renderer.model_textured(gpu, &path, &tex),
                m,
                false,
                1.0,
                None,
            );
        }
        WorldFrame { bones, models }
    }

    /// The draws: every fighter, the corpses, the models.
    pub fn draws<'a>(&'a self, world: &'a World) -> Vec<CharDraw<'a>> {
        world
            .fighters
            .iter()
            .zip(&self.bones)
            .map(|(f, b)| f.draw(b))
            .chain(world.corpse_draws())
            .chain(
                self.models
                    .iter()
                    .map(|(m, b, see_through, alpha, shade)| CharDraw {
                        model: *m,
                        bones: b,
                        see_through: *see_through,
                        alpha: *alpha,
                        shade: *shade,
                    }),
            )
            .collect()
    }
}

/// The world's overlay on a `w` × `h` screen: the HUD (hidden while a
/// cutscene is being skipped) and the skip's fade.
pub fn world_overlay(world: &mut World, w: f32, h: f32) -> Vec<OverlayDraw> {
    let mut overlay = if world.skipping() {
        Vec::new()
    } else {
        world.hud_draw(w, h)
    };
    if let Some(a) = world.skip_blanket() {
        overlay.push(black(w, h, a));
    }
    overlay
}

/// Black over the whole `w` × `h` screen, at opacity `a`.
pub fn black(w: f32, h: f32, a: f32) -> OverlayDraw {
    OverlayDraw {
        texture: None,
        tris: quad([0.0, 0.0, w, h]),
        color: [0.0, 0.0, 0.0, a],
        additive: false,
    }
}

/// A screen rectangle's two triangles (x, y, u, v), the whole texture.
pub fn quad(r: [f32; 4]) -> Vec<[f32; 4]> {
    uv_quad(r, [0.0, 0.0, 1.0, 1.0])
}

/// A screen rectangle `r` (left, top, right, bottom) showing the texture's
/// part `uv` (the same order): its two triangles (x, y, u, v).
pub fn uv_quad(r: [f32; 4], uv: [f32; 4]) -> Vec<[f32; 4]> {
    let [l, t, rr, b] = r;
    let [u0, v0, u1, v1] = uv;
    vec![
        [l, t, u0, v0],
        [rr, t, u1, v0],
        [rr, b, u1, v1],
        [l, t, u0, v0],
        [rr, b, u1, v1],
        [l, b, u0, v1],
    ]
}
