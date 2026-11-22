//! Loads a level's `particles.json` (written by `oni-convert`) into placed
//! billboard particles. These are Oni's environment particles (`ENVP`) — the
//! additive glowing sprites (lens flares, console/door screen glow, scanners)
//! that the particle system drew in-engine.

use std::path::{Path, PathBuf};

use glam::Vec3;

/// One placed particle to draw as a camera-facing billboard.
pub struct Particle {
    /// World position of the sprite centre.
    pub position: Vec3,
    /// Half-extent of the billboard quad in world units (`appearance.scale`).
    pub size: f32,
    /// Absolute path to the sprite's PNG, or `None` for a non-sprite class.
    pub texture: Option<PathBuf>,
    /// Drawn with additive blending (glow). Non-additive sprites are still
    /// drawn additively here — every shipped env particle is additive.
    pub additive: bool,
}

/// Parses `particles.json` at `path`; texture URIs are resolved relative to the
/// JSON file's directory. Returns an empty vec on any error (viewer is lenient).
pub fn load(path: &Path) -> Vec<Particle> {
    let doc: oni_schema::files::ParticlesFile = crate::json::read_file(path);
    let base = path.parent().unwrap_or(Path::new("."));
    doc.particles
        .iter()
        .map(|p| Particle {
            position: Vec3::new(p.position.x, p.position.y, p.position.z),
            size: p.size,
            texture: p.texture_path.as_deref().map(|rel| normalize(base, rel)),
            additive: p.additive,
        })
        .collect()
}

/// Joins `base` with a `/`-separated relative URI, folding `..` segments so the
/// result points at the real file on disk.
fn normalize(base: &Path, rel: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out
}
