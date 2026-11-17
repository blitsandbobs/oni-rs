//! Loads a level's `particles.json` (written by `oni-convert`) into placed
//! billboard particles. These are Oni's environment particles (`ENVP`) — the
//! additive glowing sprites (lens flares, console/door screen glow, scanners)
//! that the particle system drew in-engine.

use std::path::{Path, PathBuf};

use glam::Vec3;
use serde_json::Value;

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
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let base = path.parent().unwrap_or(Path::new("."));
    let Some(arr) = doc["particles"].as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|p| {
            let pos = &p["position"];
            let position = Vec3::new(
                pos["x"].as_f64()? as f32,
                pos["y"].as_f64()? as f32,
                pos["z"].as_f64()? as f32,
            );
            let size = p["size"].as_f64().unwrap_or(1.0) as f32;
            let texture = p["texturePath"].as_str().map(|rel| normalize(base, rel));
            Some(Particle {
                position,
                size,
                texture,
                additive: p["additive"].as_bool().unwrap_or(true),
            })
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
