//! The world's side of the game flow (`docs/GAME_FLOW.md`): a level's
//! outcome, splash screens the scripts ask for, and save points
//! (`save_game` keeps the player; `restore_game` puts them back).

use super::World;
use crate::game::Continue;

/// An autosave's message stays this long (ticks; `ONcAutosaveFadeTime`).
const AUTOSAVE_MESSAGE: u32 = 120;

impl World {
    /// `save_game n [autosave]` (`ONrGameState_MakeContinue`): what the
    /// player is now, kept for a restart at save point `n`. An autosave says
    /// so (its sound and message).
    pub fn save_continue(&mut self, n: i64, autosave: bool) {
        if autosave {
            self.event_sound("autosave", None);
            if let Some(t) = self.hud.message_text("autosave").map(str::to_string) {
                self.hud.messages.print(&t, None, AUTOSAVE_MESSAGE);
            }
        }
        let Some(p) = self.fighters.first() else { return };
        let gun = |w: Option<usize>| w.map(|w| (self.armory.weapons[w].class.name.clone(), self.armory.weapons[w].ammo));
        let i = &p.inventory;
        self.saved = Some(Continue {
            save_point: n,
            hp: p.hp + i32::from(p.inventory.hypo_remaining),
            max_hp: p.max_hp,
            position: p.feet(),
            facing: p.facing.facing,
            ammo: i.ammo,
            cells: i.cells,
            hypos: i.hypos,
            shield: i.shield,
            invisibility: i.invisibility,
            lsi: i.lsi,
            guns: [gun(p.hand), gun(p.holster), gun(p.magic)],
        });
        eprintln!("game: {} {n}", if autosave { "autosave" } else { "save" });
    }

    /// `restore_game` (`ONrGameState_UseContinue`): the player as saved —
    /// health (as the same share of full, for another class), place,
    /// facing, inventory and guns; the HUD stops prompting old objectives.
    pub fn restore_continue(&mut self) {
        let Some(c) = self.saved.clone() else {
            eprintln!("game: no save to restore");
            return;
        };
        if self.fighters.is_empty() {
            return;
        }
        {
            let p = &mut self.fighters[0];
            p.hp = if p.max_hp == c.max_hp { c.hp } else { c.hp * p.max_hp / c.max_hp.max(1) };
        }
        self.teleport(0, c.position, Some(c.facing));
        self.reset_inventory(0);
        let i = &mut self.fighters[0].inventory;
        (i.ammo, i.cells, i.hypos, i.shield, i.invisibility, i.lsi) = (c.ammo, c.cells, c.hypos, c.shield, c.invisibility, c.lsi);
        // The guns: the one put away, then the holstered, then the one in
        // hand.
        for (slot, magic) in [(2, Some(true)), (1, Some(false)), (0, None)] {
            let Some((class, ammo)) = &c.guns[slot] else { continue };
            if self.give_weapon(0, class) {
                if let Some(w) = self.fighters[0].hand {
                    self.armory.weapons[w].ammo = *ammo;
                }
                if let Some(m) = magic {
                    self.put_away(0, m);
                }
            }
        }
        self.set_weapon_varient(0);
        self.hud.restored();
    }
}
