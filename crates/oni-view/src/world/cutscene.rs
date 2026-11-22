//! Cutscenes begun and ended (`ONrGameState_BeginCutscene` /
//! `ONrGameState_EndCutscene`), as the level scripts ask.

use super::World;

/// A cutscene's state.
#[derive(Clone, Debug, Default)]
pub struct Cutscene {
    /// A cutscene is on (no compass / objective sounds; the condition loops
    /// hushed).
    pub on: bool,
    /// Ticks into it (or since a splash over it went).
    pub ticks: u32,
    /// A skip under way (`world/skip.rs`).
    pub skip: super::skip::Skip,
    /// The sound turned off (a skip): the overall volume to restore.
    pub(super) sound_off_volume: Option<f32>,
    pub letterbox: bool,
}

/// The frame clock.
#[derive(Clone, Debug, Default)]
pub struct Clock {
    /// A cutscene keeps up with the clock: whether, and the ticks owed.
    pub sync: bool,
    pub behind: u32,
    /// Slow motion's ticks left.
    pub slowmo: u32,
}

/// What a cutscene's start leaves alone: `begin_cutscene`'s flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutsceneKeeps {
    /// The player's gun stays out (`weapon`).
    pub weapon: bool,
    /// Walls between camera and player still fade (`jello`).
    pub jello: bool,
    /// The AIs stay active (`ai`).
    pub ai: bool,
    /// The player stays invisible (`invis`).
    pub invis: bool,
    /// The clock isn't kept up with (`nosync`).
    pub nosync: bool,
    /// Dangerous projectiles stay (`keepparticles`).
    pub particles: bool,
    /// The player's animation runs on (`animation`).
    pub animation: bool,
}

impl CutsceneKeeps {
    /// From the script's flag names (any case); the names it doesn't know
    /// come back too. (`nojump` is known; no shipped script uses it.)
    pub fn parse<'a>(names: impl IntoIterator<Item = &'a str>) -> (Self, Vec<String>) {
        let mut k = CutsceneKeeps::default();
        let mut unknown = Vec::new();
        for name in names {
            match name.to_ascii_lowercase().as_str() {
                "weapon" => k.weapon = true,
                "jello" => k.jello = true,
                "ai" => k.ai = true,
                "invis" => k.invis = true,
                "nosync" => k.nosync = true,
                "keepparticles" => k.particles = true,
                "animation" => k.animation = true,
                "nojump" => {}
                other => unknown.push(other.to_string()),
            }
        }
        (k, unknown)
    }
}

impl World {
    /// A cutscene begins (`ONrGameState_BeginCutscene`): letterboxed, input
    /// off; the player out of fight mode, standing, the gun put away,
    /// visible; every AI passive; no jello; the dangerous projectiles
    /// cleared — each but the first two unless `keep` keeps it.
    pub fn begin_cutscene(&mut self, keep: CutsceneKeeps) {
        self.cutscene.on = true;
        self.cutscene.ticks = 0;
        // (It keeps up with the clock, unless told not to.)
        self.clock.sync = !keep.nosync;
        self.clock.behind = 0;
        self.cutscene.skip = super::skip::Skip::Idle;
        self.sound_enabled(true);
        self.cutscene.letterbox = true;
        self.input.enabled = false;
        if let Some(f) = self.fighters.first_mut() {
            f.motion.exit_fight_mode();
            if !keep.animation {
                let ch = f.character.clone();
                f.motion.force_stand(&ch);
            }
            if !keep.invis {
                f.inventory.invisibility = 0;
            }
            if !keep.weapon {
                self.cinematic_holster(0);
            }
        }
        if !keep.ai {
            self.set_all_passive(true);
        }
        if !keep.jello {
            self.camera.jello = false;
        }
        if !keep.particles {
            self.remove_dangerous_projectiles();
        }
    }

    /// A cutscene ends (`ONrGameState_EndCutscene`): input and jello back,
    /// every AI out of passivity (those a script made passive too), the
    /// letterbox gone, the gun put away for it drawn again.
    pub fn end_cutscene(&mut self) {
        self.cutscene.on = false;
        self.clock.sync = false;
        self.cancel_skipping();
        self.sound_enabled(true);
        self.cutscene.letterbox = false;
        self.input.enabled = true;
        self.camera.jello = true;
        self.hud.cinematics.clear();
        self.set_all_passive(false);
        if self.fighters.first().is_some_and(|f| f.hand.is_none()) {
            self.force_holster(0, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_parse_in_any_case() {
        let (k, unknown) = CutsceneKeeps::parse(["AI", "weapon", "nojump", "bogus"]);
        assert!(k.ai && k.weapon && !k.jello);
        assert_eq!(unknown, ["bogus"]);
    }

    #[test]
    fn a_cutscene_letterboxes_and_takes_input_until_it_ends() {
        let mut w = World::new();
        w.begin_cutscene(CutsceneKeeps::default());
        assert!(w.cutscene.on && w.cutscene.letterbox && !w.input.enabled && w.all_passive);
        assert!(!w.camera.jello);
        w.end_cutscene();
        assert!(!w.cutscene.on && !w.cutscene.letterbox && w.input.enabled && !w.all_passive);
        assert!(w.camera.jello);
    }
}
