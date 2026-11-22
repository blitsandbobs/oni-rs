//! Group melee: everyone
//! fighting the same target is in that target's *fight*, and only a couple
//! of them hold an **attack slot** at a time. The rest keep their distance
//! with spacing techniques. A slot lasts a while — longer for whoever deals
//! damage, shorter for whoever takes it — and is handed on only when someone
//! is waiting for one.

use std::collections::HashMap;

/// Attack slots per target.
pub const MAX_SLOTS: usize = 2;
/// A slot's lifetime from when it was taken (ticks).
pub const SLOT_TICKS: i64 = 180;
/// Ticks added per point of damage dealt / taken away per point received.
pub const PER_DAMAGE_DEALT: i64 = 10;
pub const PER_DAMAGE_TAKEN: i64 = 6;

#[derive(Default, Debug)]
struct Fight {
    attackers: Vec<usize>,
    /// Slot holders and when their slot runs out.
    slots: Vec<(usize, i64)>,
}

/// Every fight in the world, keyed by target.
#[derive(Default, Debug)]
pub struct Fights {
    fights: HashMap<usize, Fight>,
}

impl Fights {
    /// `me` fights `target` (leaving any other fight).
    pub fn join(&mut self, me: usize, target: usize) {
        if self
            .fights
            .get(&target)
            .is_some_and(|f| f.attackers.contains(&me))
        {
            return;
        }
        self.leave(me);
        self.fights.entry(target).or_default().attackers.push(me);
    }

    /// `me` stops fighting (and gives up any slot).
    pub fn leave(&mut self, me: usize) {
        for f in self.fights.values_mut() {
            f.attackers.retain(|&a| a != me);
            f.slots.retain(|&(h, _)| h != me);
        }
        self.fights.retain(|_, f| !f.attackers.is_empty());
    }

    fn fight_of(&mut self, me: usize) -> Option<&mut Fight> {
        self.fights.values_mut().find(|f| f.attackers.contains(&me))
    }

    /// Whether `me` may attack now, taking a free slot if there is one. An
    /// expired slot is kept (renewed) unless someone is waiting for it.
    pub fn slot(&mut self, me: usize, now: i64) -> bool {
        let Some(f) = self.fight_of(me) else {
            return true; // not in a fight: nothing to share
        };
        let waiting = f.attackers.len() > f.slots.len();
        if let Some(i) = f.slots.iter().position(|&(h, _)| h == me) {
            if f.slots[i].1 > now {
                return true;
            }
            if waiting {
                f.slots.remove(i);
                return false;
            }
            f.slots[i].1 = now + SLOT_TICKS;
            return true;
        }
        if f.slots.len() < MAX_SLOTS {
            f.slots.push((me, now + SLOT_TICKS));
            return true;
        }
        false
    }

    /// Gives `me`'s slot up.
    pub fn release(&mut self, me: usize) {
        if let Some(f) = self.fight_of(me) {
            f.slots.retain(|&(h, _)| h != me);
        }
    }

    /// `me` dealt (`dealt`) or took (`!dealt`) `damage`: its slot lasts
    /// longer or shorter.
    pub fn damage(&mut self, me: usize, damage: i32, dealt: bool) {
        let delta = i64::from(damage)
            * if dealt {
                PER_DAMAGE_DEALT
            } else {
                -PER_DAMAGE_TAKEN
            };
        if let Some(f) = self.fight_of(me) {
            if let Some(s) = f.slots.iter_mut().find(|(h, _)| *h == me) {
                s.1 += delta;
            }
        }
    }

    /// Whether `me` holds an attack slot.
    pub fn holds_slot(&self, me: usize) -> bool {
        self.fights
            .values()
            .any(|f| f.slots.iter().any(|&(h, _)| h == me))
    }

    /// Everyone fighting `target`.
    pub fn attackers(&self, target: usize) -> &[usize] {
        self.fights
            .get(&target)
            .map_or(&[], |f| f.attackers.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_slots_then_hand_over_on_expiry() {
        let mut f = Fights::default();
        for me in 1..=3 {
            f.join(me, 0);
        }
        assert!(f.slot(1, 0));
        assert!(f.slot(2, 0));
        assert!(!f.slot(3, 0), "both slots taken");
        // Expired with someone waiting: handed on.
        assert!(!f.slot(1, SLOT_TICKS));
        assert!(f.slot(3, SLOT_TICKS));
        // Alone: an expired slot is simply renewed.
        let mut g = Fights::default();
        g.join(1, 0);
        assert!(g.slot(1, 0));
        assert!(g.slot(1, SLOT_TICKS + 5));
    }

    #[test]
    fn damage_stretches_or_shortens_a_slot() {
        let mut f = Fights::default();
        f.join(1, 0);
        f.join(2, 0);
        f.join(3, 0);
        assert!(f.slot(1, 0));
        f.damage(1, 5, true); // +50
        assert!(
            f.slot(1, SLOT_TICKS + 40),
            "still within the stretched slot"
        );
        f.damage(1, 20, false); // −120: now expired, and others are waiting
        assert!(!f.slot(1, SLOT_TICKS + 40), "handed on");
    }
}
