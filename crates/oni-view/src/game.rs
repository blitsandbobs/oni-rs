//! The game around a level (`Oni.c`'s loop, `Oni_GameState.c`; behaviour per
//! `docs/GAME_FLOW.md`): splash screens, and what follows a level's end —
//! a loss restarts it from its save point, a win goes on to the next level.

use std::path::{Path, PathBuf};

/// What happens once a splash screen is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// Back to the level (a script's splash).
    Resume,
    /// The level again, from its save point (a loss).
    Restart,
    /// On to this level (a win).
    Next(u16),
    /// The game is over (the last level won).
    End,
}

/// A splash screen up (`ONrGameState_SplashScreen`): the game stands still
/// while it fades in from black (2 s), holds (up to 10 s), holds again (up
/// to 10 s more), and fades out (2 s). A key moves it on: during the first
/// hold straight to the fade out, during the second to the fade out, and
/// half a second into the fade out to its end.
#[derive(Clone, Debug)]
pub struct Splash {
    pub texture: PathBuf,
    pub after: After,
    phase: Phase,
    /// Ticks into the phase.
    t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    In,
    Hold,
    Out,
    Done,
}

/// Its fade (ticks), each hold's timeout, and how long a key is ignored at
/// the start of the fade out.
const FADE: f32 = 120.0;
const TIMEOUT: f32 = 600.0;
const HASTY: f32 = 30.0;

impl Splash {
    pub fn new(texture: PathBuf, after: After) -> Splash {
        Splash { texture, after, phase: Phase::In, t: 0.0 }
    }

    /// Moves it on `ticks` (real time, in sixtieths), with or without a key
    /// pressed; returns what follows once it is over.
    pub fn tick(&mut self, ticks: f32, key: bool) -> Option<After> {
        self.t += ticks;
        match self.phase {
            Phase::In | Phase::Hold if key => self.go(Phase::Out),
            Phase::In if self.t >= TIMEOUT => self.go(Phase::Hold),
            Phase::Hold if self.t >= TIMEOUT => self.go(Phase::Out),
            Phase::Out if self.t >= FADE || (key && self.t > HASTY) => self.go(Phase::Done),
            _ => {}
        }
        (self.phase == Phase::Done).then_some(self.after)
    }

    fn go(&mut self, phase: Phase) {
        self.phase = phase;
        self.t = 0.0;
    }

    /// How black it is now (0 clear, 1 black): fading in, `(1 − t/2 s)^1.9`;
    /// fading out, `(t/2 s)^1.9` (`get_cosmetic_ramp`).
    pub fn blanket(&self) -> f32 {
        match self.phase {
            Phase::In => (1.0 - self.t / FADE).max(0.0).powf(1.9),
            Phase::Hold => 0.0,
            Phase::Out => (self.t / FADE).min(1.0).powf(1.9),
            Phase::Done => 1.0,
        }
    }
}

/// The level after `level` (`ONrLevel_GetNextLevel`): the lowest numbered
/// one above it that exists (`levels/NN/`), if any.
pub fn next_level(oni_root: &Path, level: u16) -> Option<u16> {
    (level + 1..100).find(|n| oni_root.join(format!("levels/{n:02}/environment.glb")).is_file())
}

/// A level's splash screen texture: the level's own, else the common one.
pub fn splash_texture(oni_root: &Path, level: u16, name: &str) -> Option<PathBuf> {
    [oni_root.join(format!("levels/{level:02}/textures/{name}.png")), oni_root.join(format!("common/textures/{name}.png"))].into_iter().find(|p| p.is_file())
}

/// What a save point keeps of the player (`ONtContinue`), so a restart
/// begins there: the save point, health, place, facing, inventory and guns
/// (by class, with their rounds: in hand, holstered, put away by a
/// cutscene).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Continue {
    pub save_point: i64,
    pub hp: i32,
    pub max_hp: i32,
    pub position: glam::Vec3,
    pub facing: f32,
    pub ammo: u16,
    pub cells: u16,
    pub hypos: u16,
    pub shield: u16,
    pub invisibility: u16,
    pub lsi: bool,
    pub guns: [Option<(String, u16)>; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_splash_fades_in_holds_and_fades_out() {
        let mut s = Splash::new(PathBuf::new(), After::Resume);
        assert!((s.blanket() - 1.0).abs() < 1e-6, "black at first");
        s.tick(60.0, false);
        assert!(s.blanket() > 0.0 && s.blanket() < 1.0);
        s.tick(60.0, false);
        assert_eq!(s.blanket(), 0.0, "clear after 2 s");
        // Untouched: 10 s, 10 s more, then 2 s of fading out.
        let mut ticks: f32 = 120.0;
        let mut done = None;
        while done.is_none() {
            done = s.tick(1.0, false);
            ticks += 1.0;
        }
        assert_eq!(done, Some(After::Resume));
        assert!((ticks - (600.0 + 600.0 + 120.0)).abs() < 2.0, "{ticks}");
    }

    #[test]
    fn keys_hurry_it() {
        let mut s = Splash::new(PathBuf::new(), After::Restart);
        s.tick(10.0, true);
        assert!(s.tick(10.0, true).is_none(), "keys ignored early in the fade out");
        assert_eq!(s.tick(30.0, true), Some(After::Restart));
    }
}
