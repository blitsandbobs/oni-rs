//! The fog (`gl_utility.c`, `docs/RENDERING.md`): its colour and its start
//! and end in screen depth, the engine variables scripts set (`gl_fog_red`,
//! `_green`, `_blue`, `_start`, `_end`) and the ramps they start
//! (`gl_fog_start_changeto`, `gl_fog_end_changeto`), back to the draw
//! engine's defaults on each level.

use crate::render::Fog;

/// The fog and its ramps: where each end heads and by how much a tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct FogState {
    pub fog: Fog,
    start_ramp: Option<(f32, f32)>,
    end_ramp: Option<(f32, f32)>,
    /// The start ramp's step, kept: the end's ramp moves by it (as shipped).
    start_delta: f32,
}

impl FogState {
    /// `gl_fog_start_changeto value [frames]`: at once, or eased over the
    /// frames (a direct set leaves a ramp running, as shipped).
    pub fn start_to(&mut self, value: f32, frames: i32) {
        if frames <= 0 {
            self.fog.start = value;
        } else {
            self.start_delta = (value - self.fog.start) / frames as f32;
            self.start_ramp = Some((value, self.start_delta));
        }
    }

    /// `gl_fog_end_changeto value [frames]`.
    pub fn end_to(&mut self, value: f32, frames: i32) {
        if frames <= 0 {
            self.fog.end = value;
        } else {
            self.end_ramp = Some((value, (value - self.fog.end) / frames as f32));
        }
    }

    /// One tick of the ramps (`gl_fog_update`): each end steps toward where
    /// it heads, landing on it once a step would reach it. (As shipped the
    /// end's ramp steps by the *start's* step — none, if the start never
    /// ramped, and then it never arrives. No shipped script ramps the end.)
    pub fn tick(&mut self) {
        if let Some((to, delta)) = self.start_ramp {
            self.fog.start = step(self.fog.start, to, delta, &mut self.start_ramp);
        }
        if let Some((to, _)) = self.end_ramp {
            let delta = self.start_delta;
            self.fog.end = step(self.fog.end, to, delta, &mut self.end_ramp);
        }
    }
}

/// One step of `value` toward `to` by `delta`; the ramp ends on arriving.
fn step<T>(value: f32, to: f32, delta: f32, ramp: &mut Option<T>) -> f32 {
    if delta > 0.0 {
        if value < to - delta {
            return value + delta;
        }
    } else if delta < 0.0 {
        if value > to - delta {
            return value + delta;
        }
    } else {
        return value;
    }
    *ramp = None;
    to
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fog_eases_as_the_original_does() {
        let mut f = FogState::default();
        assert_eq!((f.fog.start, f.fog.end), (0.975, 1.0));
        // Eased over 4 ticks to 0.995: 0.98, 0.985, 0.99, then there.
        f.start_to(0.995, 4);
        let seen: Vec<f32> = (0..5)
            .map(|_| {
                f.tick();
                f.fog.start
            })
            .collect();
        assert!(
            (seen[0] - 0.98).abs() < 1e-6 && (seen[2] - 0.99).abs() < 1e-6,
            "{seen:?}"
        );
        assert_eq!(seen[3], 0.995);
        assert_eq!(seen[4], 0.995);
        // At once with no frames.
        f.start_to(0.9, 0);
        assert_eq!(f.fog.start, 0.9);
        // The end ramps by the start's step (as shipped): rising 0.005 a
        // tick toward a higher end...
        f.fog.end = 0.98;
        f.end_to(0.995, 30);
        f.tick();
        assert!((f.fog.end - 0.985).abs() < 1e-6, "{}", f.fog.end);
        // ...but toward a lower one that step already "reaches": there at
        // once.
        f.end_to(0.97, 30);
        f.tick();
        assert_eq!(f.fog.end, 0.97);
    }
}
