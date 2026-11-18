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
}
