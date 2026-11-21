//! Cheat codes (`ONrGameState_HandleCheats`, `ONrCheater`;
//! `docs/CHEATS.md`): typed on the pause screen once the game has been won
//! (here also with `ONI_CHEATS=1`), each toggling or giving something, its
//! message shown in the pause screen's hint area until it closes.

use super::World;

/// The longest code the typing is kept for (`MAX_CHEAT_STRING`).
const BUFFER: usize = 20;

/// The codes the port carries out: the word, its message on (or given)
/// and off. The original's others aren't here yet (#60's docs list them).
const CODES: [(&str, &str, Option<&str>, Cheat); 12] = [
    ("liveforever", "Invincibility Enabled", Some("Invincibility Disabled"), Cheat::Invincible),
    ("touchofdeath", "Omnipotence Enabled", Some("Omnipotence Disabled"), Cheat::Omnipotent),
    ("canttouchthis", "Unstoppable Enabled", Some("Unstoppable Disabled"), Cheat::Unstoppable),
    ("fatloot", "Fat Loot Received", None, Cheat::FatLoot),
    ("winlevel", "Instantly Win Level", None, Cheat::WinLevel),
    ("loselevel", "Instantly Lose Level", None, Cheat::LoseLevel),
    ("superammo", "Super Ammo Mode Enabled", Some("Super Ammo Mode Disabled"), Cheat::SuperAmmo),
    ("elderrune", "Regeneration Enabled", Some("Regeneration Disabled"), Cheat::Regeneration),
    ("moonshadow", "Phase Cloak Enabled", Some("Phase Cloak Disabled"), Cheat::Invisibility),
    ("chenille", "Daodan Power Enabled", Some("Daodan Power Disabled"), Cheat::DaodanPower),
    ("carousel", "Slow Motion Enabled", Some("Slow Motion Disabled"), Cheat::SlowMotion),
    // (Not the original's: a full shield belt, for trying shields out.)
    ("forcefield", "Force Field Received", None, Cheat::ForceField),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cheat {
    Invincible,
    Omnipotent,
    Unstoppable,
    FatLoot,
    WinLevel,
    LoseLevel,
    SuperAmmo,
    Regeneration,
    Invisibility,
    DaodanPower,
    SlowMotion,
    ForceField,
}

/// The cheats' state: what has been typed, and the player-only toggles.
#[derive(Clone, Debug, Default)]
pub struct Cheats {
    /// Typing them works (the game won, or the developers' switch).
    pub allowed: bool,
    typed: String,
    pub invincible: bool,
    pub omnipotent: bool,
    pub unstoppable: bool,
    pub regeneration: bool,
    pub slow_motion: bool,
}

impl World {
    /// A letter typed on the pause screen (`ONrGameState_HandleCheats`):
    /// kept with the last 19; a code it completes is carried out, its
    /// message shown, the typing forgotten. Returns the message.
    pub fn cheat_key(&mut self, key: char) -> Option<&'static str> {
        if !self.cheats.allowed || !key.is_ascii_lowercase() || self.hud.pause.is_none() {
            return None;
        }
        self.cheats.typed.push(key);
        if self.cheats.typed.len() > BUFFER {
            self.cheats.typed.remove(0);
        }
        let (code, on, off, cheat) = *CODES.iter().find(|(w, ..)| self.cheats.typed.ends_with(w))?;
        let enabled = self.cheat(cheat);
        eprintln!("cheat: {code} ({})", if enabled { "on" } else { "off" });
        self.cheats.typed.clear();
        let message = if enabled { Some(on) } else { off };
        if let (Some(m), Some(p)) = (message, self.hud.pause.as_mut()) {
            p.message = Some(m);
        }
        message
    }

    /// `ONrCheater`: carries one out; whether it is now on.
    fn cheat(&mut self, cheat: Cheat) -> bool {
        let Some(p) = self.fighters.first_mut().filter(|f| f.player) else { return false };
        let c = &mut self.cheats;
        let toggle = |b: &mut bool| {
            *b = !*b;
            *b
        };
        match cheat {
            Cheat::Invincible => toggle(&mut c.invincible),
            Cheat::Omnipotent => toggle(&mut c.omnipotent),
            Cheat::Unstoppable => toggle(&mut c.unstoppable),
            Cheat::Regeneration => toggle(&mut c.regeneration),
            Cheat::FatLoot => {
                p.inventory.ammo = 6;
                p.inventory.cells = 6;
                p.inventory.hypos = 6;
                true
            }
            Cheat::WinLevel => {
                self.victory = Some(true);
                true
            }
            Cheat::LoseLevel => {
                self.victory = Some(false);
                true
            }
            Cheat::SuperAmmo => toggle(&mut p.infinite_ammo),
            Cheat::Invisibility => {
                if p.inventory.invisibility == super::magic::ENDLESS {
                    p.inventory.invisibility = 0;
                    false
                } else {
                    p.inventory.invisibility = super::magic::ENDLESS;
                    true
                }
            }
            // Its super amount at 10 and a boss shield; off, neither.
            Cheat::DaodanPower => {
                if p.external_super > 1.0 {
                    p.external_super = 0.0;
                    p.boss_shield = false;
                    false
                } else {
                    p.external_super = 10.0;
                    p.boss_shield = true;
                    true
                }
            }
            Cheat::SlowMotion => toggle(&mut c.slow_motion),
            Cheat::ForceField => {
                p.inventory.shield = super::magic::MAX_SHIELD;
                true
            }
        }
    }

    /// The player-only cheats onto the player this tick (`ONgPlayer*`:
    /// apart from the flags scripts set).
    pub(super) fn cheats_tick(&mut self) {
        let c = &self.cheats;
        let (invincible, unstoppable, regeneration) = (c.invincible, c.unstoppable, c.regeneration);
        let now = self.now as u64;
        if let Some(p) = self.fighters.first_mut().filter(|f| f.player) {
            p.cheat_invincible = invincible;
            p.cheat_unstoppable = unstoppable;
            // Regeneration (`WPrInventory_Update`): a point a second, up
            // to full, while no hypo is at work.
            if regeneration && p.alive() && p.inventory.hypo_remaining == 0 && p.inventory.hypo_timer == 0 && p.hp < p.max_hp && now.is_multiple_of(60) {
                p.hp += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_found_at_the_end_of_what_was_typed() {
        let typed = "xxmoonshadow";
        assert_eq!(CODES.iter().find(|(w, ..)| typed.ends_with(w)).map(|c| c.3), Some(Cheat::Invisibility));
        assert!(CODES.iter().all(|(w, ..)| w.len() <= BUFFER));
    }
}
