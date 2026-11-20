//! Per-tick control state for the movement model: which controls are held,
//! and which went down or up since the previous tick.

/// Control bits (an engine-internal encoding; key bindings map onto these).
pub mod key {
    pub const FORWARD: u32 = 1 << 0;
    pub const BACKWARD: u32 = 1 << 1;
    pub const STEP_LEFT: u32 = 1 << 2;
    pub const STEP_RIGHT: u32 = 1 << 3;
    pub const WALK: u32 = 1 << 4;
    pub const CROUCH: u32 = 1 << 5;
    pub const JUMP: u32 = 1 << 6;
    pub const PUNCH: u32 = 1 << 7;
    pub const KICK: u32 = 1 << 8;
    /// The fire buttons (mouse left / right / middle by default).
    pub const FIRE1: u32 = 1 << 9;
    pub const FIRE2: u32 = 1 << 10;
    pub const FIRE3: u32 = 1 << 11;
    /// The action key (doors, consoles; in a fight, a taunt).
    pub const ACTION: u32 = 1 << 12;
    /// Turning by key (unbound for the player, who turns with the mouse; the
    /// AI uses them, e.g. to roll up from the ground).
    pub const TURN_LEFT: u32 = 1 << 13;
    pub const TURN_RIGHT: u32 = 1 << 14;
    /// Pick up / holster / draw (Q), reload (R), drop the gun.
    pub const SWAP: u32 = 1 << 15;
    pub const RELOAD: u32 = 1 << 16;
    pub const DROP: u32 = 1 << 17;
    /// Look mode (V): the mouse turns the aim off the facing instead of
    /// the body.
    pub const LOOK: u32 = 1 << 18;
}

/// The original's button bits (`LIc_Bit_*`, as films record them) as ours.
pub fn from_original_buttons(b: u64) -> u32 {
    const MAP: [(u32, u32); 15] = [
        (21, key::FORWARD),
        (22, key::BACKWARD),
        (23, key::TURN_LEFT),
        (24, key::TURN_RIGHT),
        (25, key::STEP_LEFT),
        (26, key::STEP_RIGHT),
        (27, key::JUMP),
        (28, key::CROUCH),
        (29, key::PUNCH),
        (30, key::KICK),
        (32, key::WALK),
        (33, key::ACTION),
        (38, key::FIRE1),
        (39, key::FIRE2),
        (40, key::FIRE3),
    ];
    MAP.iter().filter(|(bit, _)| b & (1u64 << bit) != 0).fold(0, |k, (_, ours)| k | ours)
}

/// What the fire buttons do without a usable weapon, as in the original:
/// fire 1 also counts as punch when the character holds no weapon (or it is
/// empty); fire 2 also counts as kick unless the weapon has a working
/// alternate fire; fire 3 always also counts as crouch. Weapons aren't
/// modelled yet, so everyone is unarmed (`armed = false`).
pub fn fire_as_melee(held: u32, armed: bool, alternate_fire: bool) -> u32 {
    let mut out = held;
    if held & key::FIRE1 != 0 && !armed {
        out |= key::PUNCH;
    }
    if held & key::FIRE2 != 0 && (!armed || !alternate_fire) {
        out |= key::KICK;
    }
    if held & key::FIRE3 != 0 {
        out |= key::CROUCH;
    }
    out
}

/// Held / pressed / released control bits for the current tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Buttons {
    pub held: u32,
    pub pressed: u32,
    pub released: u32,
}

impl Buttons {
    /// Advances to a new tick where `held` are the controls now down.
    pub fn advance(&mut self, held: u32) {
        self.pressed = held & !self.held;
        self.released = self.held & !held;
        self.held = held;
    }

    /// Whether every control in `mask` is held.
    pub fn holding(&self, mask: u32) -> bool {
        self.held & mask == mask
    }

    /// Whether every control in `mask` went down this tick.
    pub fn just_pressed(&self, mask: u32) -> bool {
        self.pressed & mask == mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_are_reported_once() {
        let mut b = Buttons::default();
        b.advance(key::PUNCH);
        assert!(b.just_pressed(key::PUNCH) && b.holding(key::PUNCH));
        b.advance(key::PUNCH);
        assert!(!b.just_pressed(key::PUNCH), "held, not newly pressed");
        b.advance(0);
        assert_eq!(b.released, key::PUNCH);
        // An empty mask is trivially satisfied (rules with no press/hold).
        assert!(b.just_pressed(0) && b.holding(0));
    }

    #[test]
    fn unarmed_fire_buttons_are_melee() {
        assert_eq!(fire_as_melee(key::FIRE1, false, false), key::FIRE1 | key::PUNCH);
        assert_eq!(fire_as_melee(key::FIRE2, false, false), key::FIRE2 | key::KICK);
        assert_eq!(fire_as_melee(key::FIRE3, false, false), key::FIRE3 | key::CROUCH);
        // Armed: fire 1 shoots; fire 2 kicks only without an alternate fire.
        assert_eq!(fire_as_melee(key::FIRE1, true, true), key::FIRE1);
        assert_eq!(fire_as_melee(key::FIRE2, true, false), key::FIRE2 | key::KICK);
        assert_eq!(fire_as_melee(key::FIRE2, true, true), key::FIRE2);
    }
}
