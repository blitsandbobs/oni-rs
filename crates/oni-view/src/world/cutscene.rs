//! Cutscenes begun and ended (`ONrGameState_BeginCutscene` /
//! `ONrGameState_EndCutscene`), as the level scripts ask.

use super::World;

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
        self.in_cutscene = true;
        self.cutscene_ticks = 0;
        // (It keeps up with the clock, unless told not to.)
        self.sync_enabled = !keep.nosync;
        self.sync_behind = 0;
        self.skip = super::skip::Skip::Idle;
        self.sound_enabled(true);
        self.letterbox = true;
        self.input_enabled = false;
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
        self.in_cutscene = false;
        self.sync_enabled = false;
        self.cancel_skipping();
        self.sound_enabled(true);
        self.letterbox = false;
        self.input_enabled = true;
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
        assert!(w.in_cutscene && w.letterbox && !w.input_enabled && w.all_passive);
        assert!(!w.camera.jello);
        w.end_cutscene();
        assert!(!w.in_cutscene && !w.letterbox && w.input_enabled && !w.all_passive);
        assert!(w.camera.jello);
    }
}
