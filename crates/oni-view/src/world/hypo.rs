//! Hypos (`docs/WEAPONS.md`, *Hypos*): a hypo taken heals over time —
//! a point every class's `hypoTime` ticks — a quarter of full health, or
//! past full a boost up to double; above full, health drains back a point
//! every 10 ticks; a hurt eats into what is still to come; and a super-hypo
//! class hits harder while above full.

use super::World;

/// Above full, health drains a point every this many ticks
/// (`WPrInventory_GetHypoDownTime`).
const DOWN_TIME: u16 = 10;

/// The game settings' hypo values (`game_settings.json` `hypo`).
#[derive(Clone, Copy, Debug)]
pub struct HypoSettings {
    pub amount: f32,
    pub boost_amount: f32,
    pub overhypo_base: f32,
    pub overhypo_max: f32,
}

impl Default for HypoSettings {
    fn default() -> Self {
        HypoSettings {
            amount: 0.25,
            boost_amount: 1.0,
            overhypo_base: 1.5,
            overhypo_max: 2.3,
        }
    }
}

impl HypoSettings {
    /// From the game settings (none: the defaults).
    pub fn load(gs: Option<&oni_schema::files::GameSettingsFile>) -> HypoSettings {
        gs.map_or_else(HypoSettings::default, |gs| HypoSettings {
            amount: gs.hypo.amount,
            boost_amount: gs.hypo.boost_amount,
            overhypo_base: gs.hypo.overhypo_base_damage,
            overhypo_max: gs.hypo.overhypo_max_damage,
        })
    }
}

impl World {
    /// Fighter `who` uses a hypo (`WPrPowerup_Use`): none to use, or
    /// already headed past full, nothing; else the health it will give —
    /// up to full by a quarter of full, a hypo that crosses full giving its
    /// rest as boost, at full a whole boost — capped at the boosted most.
    /// Plays the hypo sound. Returns whether it was used.
    pub fn use_hypo(&mut self, who: usize) -> bool {
        let h = self.hypo;
        let boosted_most = self.game_sounds.boosted_health;
        let Some(f) = self.fighters.get_mut(who) else {
            return false;
        };
        let max = f.max_hp.max(1) as u32;
        let inv = &mut f.inventory;
        let mut target = f.hp.max(0) as u32 + u32::from(inv.hypo_remaining);
        if inv.hypos == 0 || target > max {
            return false;
        }
        if target >= max {
            target += (max as f32 * h.boost_amount).round() as u32;
        } else {
            let amount = (max as f32 * h.amount).round() as u32;
            if target + amount > max {
                let rest = 1.0 - (max - target) as f32 / amount as f32;
                target = max + (rest * max as f32 * h.boost_amount).round() as u32;
            } else {
                target += amount;
            }
        }
        target = target.min((max as f32 * boosted_most) as u32);
        inv.hypo_timer = 0;
        inv.hypo_remaining = target.saturating_sub(f.hp.max(0) as u32) as u16;
        inv.hypos -= 1;
        if f.player {
            self.event_sound("useHypo", None);
        }
        true
    }

    /// One tick of fighter `who`'s hypo (`WPrInventory_Update`): with health
    /// still to come, a point every `hypoTime` ticks (no wait once past
    /// full), stopping at the boosted most; else above full, a point back
    /// every 10 ticks.
    pub(super) fn inventory_tick(&mut self, who: usize) {
        let boosted_most = self.game_sounds.boosted_health;
        let f = &mut self.fighters[who];
        if !f.alive() {
            return;
        }
        let up_time = f.motion.tuning.hypo_time;
        let inv = &mut f.inventory;
        if inv.hypo_timer > 0 {
            inv.hypo_timer -= 1;
            return;
        }
        let boosted = (f.max_hp as f32 * boosted_most).round() as i32;
        if inv.hypo_remaining > 0 {
            if f.hp >= boosted {
                f.hp = boosted;
                inv.hypo_remaining = 0;
                inv.hypo_timer = DOWN_TIME;
            } else {
                inv.hypo_remaining -= 1;
                f.hp += 1;
                if f.hp < f.max_hp {
                    inv.hypo_timer = up_time;
                }
            }
        } else if f.hp > f.max_hp {
            f.hp -= 1;
            inv.hypo_timer = DOWN_TIME;
        }
    }

    /// How hard fighter `who` hits (`ONiCharacter_GetAttackPotency`): a
    /// super-hypo class above full health, from the base to the most at
    /// double health; else 1.
    pub(super) fn potency(&self, who: usize) -> f32 {
        let Some(f) = self.fighters.get(who) else {
            return 1.0;
        };
        if !f.motion.tuning.super_hypo || f.hp <= f.max_hp {
            return 1.0;
        }
        let over = ((f.hp - f.max_hp) as f32 / f.max_hp.max(1) as f32).min(1.0);
        self.hypo.overhypo_base + (self.hypo.overhypo_max - self.hypo.overhypo_base) * over
    }
}
