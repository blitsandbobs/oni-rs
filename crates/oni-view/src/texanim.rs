//! Animated textures (`docs/PARTICLES.md`, *Animated textures*): a texture
//! with frames, and which one shows at a time (`M3iResolveTexture`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// An animated texture (`common/texture_animations.json`).
#[derive(Clone, Debug, Default)]
pub struct TexAnim {
    /// The frames' files, the texture itself first.
    pub frames: Vec<PathBuf>,
    /// Ticks each frame shows.
    pub time_per_frame: u32,
    /// Back and forth through the frames.
    pub ping_pong: bool,
    /// A random frame each step.
    pub random_frame: bool,
    /// Its time starts at the drawer's random index.
    pub random_start: bool,
    /// Its time held at 0 (with a random start: one random frame for good).
    pub dont_animate: bool,
}

impl TexAnim {
    /// The frame showing at `time` (ticks) for a drawer whose random index
    /// is `instance` (`M3iResolveTexture`).
    pub fn frame(&self, time: u32, instance: u32) -> usize {
        let n = self.frames.len() as u32;
        if n <= 1 {
            return 0;
        }
        let mut time = if self.dont_animate { 0 } else { time };
        if self.random_start {
            time = time.wrapping_add(instance);
        }
        let step = time / self.time_per_frame.max(1);
        let frame = if self.random_frame {
            // The local generator, seeded from the step (`UUrLocalRandom`).
            let seed = 0xfded_fded_u32 ^ step.swap_bytes();
            let r = (seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) >> 16) & 0xFFFF;
            (n * r) >> 16
        } else if self.ping_pong {
            let span = 2 * (n - 1);
            let f = step % span;
            if f >= n {
                span - f
            } else {
                f
            }
        } else {
            step % n
        };
        frame as usize
    }

    /// The file showing at `time` for a drawer with random index `instance`.
    pub fn file(&self, time: u32, instance: u32) -> &Path {
        &self.frames[self.frame(time, instance)]
    }
}

/// The animated textures by file stem, their frames under `oni` (the
/// converted tree's root).
pub fn load(oni: &Path) -> HashMap<String, TexAnim> {
    let doc: serde_json::Value = std::fs::read_to_string(oni.join("common/texture_animations.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let Some(map) = doc.as_object() else { return HashMap::new() };
    map.iter()
        .filter_map(|(name, a)| {
            let frames: Vec<PathBuf> = a["frames"].as_array()?.iter().filter_map(|f| Some(oni.join(f.as_str()?))).collect();
            let has = |flag: &str| a["flags"].as_array().is_some_and(|f| f.iter().any(|x| x == flag));
            let anim = TexAnim {
                frames,
                time_per_frame: a["timePerFrame"].as_u64().unwrap_or(1) as u32,
                ping_pong: has("pingPong"),
                random_frame: has("randomFrame"),
                random_start: has("randomStart"),
                dont_animate: has("dontAnimate"),
            };
            (!anim.frames.is_empty()).then(|| (name.clone(), anim))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(n: usize, tpf: u32) -> TexAnim {
        TexAnim { frames: (0..n).map(|k| PathBuf::from(format!("f{k}"))).collect(), time_per_frame: tpf, ..TexAnim::default() }
    }

    #[test]
    fn frames_follow_the_original() {
        // Looping: a frame every 3 ticks, round again after the last.
        let a = anim(4, 3);
        let seen: Vec<usize> = (0..15).map(|t| a.frame(t, 0)).collect();
        assert_eq!(seen, [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 0, 0, 0]);
        // Ping-pong: back and forth without repeating the ends.
        let p = TexAnim { ping_pong: true, ..anim(3, 1) };
        assert_eq!((0..8).map(|t| p.frame(t, 0)).collect::<Vec<_>>(), [0, 1, 2, 1, 0, 1, 2, 1]);
        // A random start shifts the time; held still, it picks one frame.
        let r = TexAnim { random_start: true, dont_animate: true, ..anim(5, 1) };
        assert_eq!(r.frame(0, 7), 2);
        assert_eq!(r.frame(1000, 7), 2);
        // Random frames: the same step, the same frame; always in range.
        let q = TexAnim { random_frame: true, ..anim(5, 2) };
        assert_eq!(q.frame(10, 0), q.frame(11, 0));
        assert!((0..500).all(|t| q.frame(t, 0) < 5));
        assert!((0..500).map(|t| q.frame(t, 0)).collect::<std::collections::HashSet<_>>().len() > 1);
        // One frame: always it.
        assert_eq!(anim(1, 1).frame(99, 3), 0);
    }
}
