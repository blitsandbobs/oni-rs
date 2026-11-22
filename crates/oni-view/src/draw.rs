//! What the simulation hands the renderer each frame — plain data, no GPU:
//! fog, decals, sprites, overlays, character and model draws, magic
//! effects. (`render.rs` draws them.)

use glam::Mat4;

/// The fog (`gl_fog_*`, `docs/RENDERING.md`): linear in screen depth between
/// its start and end (−1 near, 1 far), toward its colour.
#[derive(Clone, Copy, Debug)]
pub struct Fog {
    pub color: [f32; 3],
    pub start: f32,
    pub end: f32,
}

impl Default for Fog {
    /// The draw engine's (`FOG_COLOR_GL`, `ONI_FOG_START`, `ONI_FOG_END`).
    fn default() -> Fog {
        Fog {
            color: [0.25; 3],
            start: 0.975,
            end: 1.0,
        }
    }
}

/// A decal to draw this frame (`docs/PARTICLES.md`, *Decals*): its
/// triangles over the level, lit corners (RGB, 0–1), its opacity, texture,
/// and whether the texture glows (additive).
pub struct DecalDraw {
    pub positions: std::rc::Rc<crate::decal::Mesh>,
    pub alpha: f32,
    pub texture: std::path::PathBuf,
    pub additive: bool,
}

/// A particle sprite: where, its half-size (x, y), and — for a sprite
/// stretched along its flight — the direction its x runs; else it faces the
/// camera.
pub struct Sprite {
    pub at: glam::Vec3,
    pub size: [f32; 2],
    /// How it lies (`MSrGeomContext_Method_Sprite_Draw`).
    pub mode: SpriteMode,
    /// Its turn in its own plane (degrees).
    pub rotation: f32,
    /// Its colour (tint × alpha: additive).
    pub color: [f32; 3],
    pub texture: std::path::PathBuf,
    /// A contrail (with an axis): the texture's u across it, v along it
    /// (`M3rContrail_Draw`), where a sprite has u along its axis.
    pub contrail: bool,
    /// Drawn over everything, without the depth test (a lens flare).
    pub no_depth: bool,
}

/// How a sprite lies: its X (along its width) and Y (its height).
#[derive(Clone, Copy, Debug)]
pub enum SpriteMode {
    /// Given outright: four corners in order round the quad, each with its
    /// texture coordinate and colour (a contrail's link, `M3rContrail_Draw`).
    Quad([(glam::Vec3, [f32; 2], [f32; 3]); 4]),
    /// Facing the camera: the screen's X and Y.
    Facing,
    /// Facing the camera, turned on screen toward this direction.
    Rotated(glam::Vec3),
    /// Along this direction, turned about it to face the camera.
    Billboard(glam::Vec3),
    /// Lying along these two axes (arrow, flat, discus).
    Fixed([glam::Vec3; 2]),
}

/// The overlay texture name that draws the movie's picture.
pub const MOVIE_TEXTURE: &str = "<movie>";

/// Something drawn over the frame (the HUD): triangles in screen pixels
/// (x, y, u, v per corner, three a triangle, y down) of a texture (none:
/// plain), tinted by `color` (RGBA, 0–1).
#[derive(Clone, Debug)]
pub struct OverlayDraw {
    pub texture: Option<std::path::PathBuf>,
    pub tris: Vec<[f32; 4]>,
    pub color: [f32; 4],
    /// Added to what is behind (a texture flagged additive) rather than
    /// laid over it.
    pub additive: bool,
}

/// One character instance to draw this frame: which uploaded model and
/// the posed bone world-matrices for it (from the instance's own pose).
pub struct CharDraw<'a> {
    pub model: usize,
    pub bones: &'a [Mat4],
    /// Drawn see-through (an object in the camera's way: 31 of 255).
    pub see_through: bool,
    /// How visible (1: as usual; below, blended at that alpha without
    /// writing depth — an invisible body fading; 0: not drawn).
    pub alpha: f32,
    /// Lit flat in this colour (`OBcFlags_FlatLighting`: `obj_shade`).
    pub shade: Option<[f32; 3]>,
}

/// A one-model draw the app lists before the frame: model, bones, drawn
/// see-through, alpha, flat shade.
pub type ModelDraw = (usize, Vec<Mat4>, bool, f32, Option<[f32; 3]>);

/// Which magic effect (`M3cTextureFlags_MagicShield_*`): its colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagicKind {
    Shield,
    Daodan,
    Invis,
}

/// A magic effect over a body (`TRrBody_DrawMagic`): a model drawn part
/// by part with `bones`, in `texture`, each part at its `alpha`, shimmering
/// by `kind` at game time `time`.
#[derive(Clone, Debug)]
pub struct MagicDraw {
    pub model: usize,
    pub bones: Vec<Mat4>,
    pub texture: std::path::PathBuf,
    pub kind: MagicKind,
    pub alpha: [f32; 19],
    pub time: u32,
}
