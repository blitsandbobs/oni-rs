//! Numeric identifiers used by Oni's animation data: animation types
//! (`TRtAnimType`), body states (`TRtAnimState`), variant bits and per-clip
//! flag bits. Only the members the movement port references are listed; the
//! values are what the converted clips carry in their `animType` /
//! `fromState` / `toState` / `varient` / `flags` fields.

/// Animation types (what a clip *does*).
pub mod ty {
    pub const NONE: u16 = 0;
    pub const WALK: u16 = 2;
    pub const RUN: u16 = 3;
    pub const SLIDE: u16 = 4;
    pub const JUMP: u16 = 5;
    pub const STAND: u16 = 6;
    pub const STANDING_TURN_LEFT: u16 = 7;
    pub const STANDING_TURN_RIGHT: u16 = 8;
    pub const RUN_BACKWARDS: u16 = 9;
    pub const RUN_SIDESTEP_LEFT: u16 = 10;
    pub const RUN_SIDESTEP_RIGHT: u16 = 11;
    pub const KICK: u16 = 12;
    pub const WALK_SIDESTEP_LEFT: u16 = 13;
    pub const WALK_SIDESTEP_RIGHT: u16 = 14;
    pub const WALK_BACKWARDS: u16 = 15;
    pub const CROUCH: u16 = 17;
    pub const JUMP_FORWARD: u16 = 18;
    pub const JUMP_BACKWARD: u16 = 19;
    pub const JUMP_LEFT: u16 = 20;
    pub const JUMP_RIGHT: u16 = 21;
    pub const PUNCH: u16 = 22;
    pub const BLOCK: u16 = 23;
    pub const LAND: u16 = 24;
    pub const FLY: u16 = 25;
    pub const KICK_FORWARD: u16 = 26;
    pub const KICK_LEFT: u16 = 27;
    pub const KICK_RIGHT: u16 = 28;
    pub const KICK_BACK: u16 = 29;
    pub const KICK_LOW: u16 = 30;
    pub const PUNCH_FORWARD: u16 = 31;
    pub const PUNCH_LEFT: u16 = 32;
    pub const PUNCH_RIGHT: u16 = 33;
    pub const PUNCH_BACK: u16 = 34;
    pub const PUNCH_LOW: u16 = 35;
    pub const KICK2: u16 = 36;
    pub const KICK3: u16 = 37;
    pub const PUNCH2: u16 = 38;
    pub const PUNCH3: u16 = 39;
    pub const LAND_FORWARD: u16 = 40;
    pub const LAND_RIGHT: u16 = 41;
    pub const LAND_LEFT: u16 = 42;
    pub const LAND_BACK: u16 = 43;
    pub const PPK: u16 = 44;
    pub const PKK: u16 = 45;
    pub const PKP: u16 = 46;
    pub const KPK: u16 = 47;
    pub const KPP: u16 = 48;
    pub const KKP: u16 = 49;
    pub const PK: u16 = 50;
    pub const KP: u16 = 51;
    pub const CROUCH_RUN: u16 = 58;
    pub const CROUCH_WALK: u16 = 59;
    pub const CROUCH_RUN_BACKWARDS: u16 = 60;
    pub const CROUCH_WALK_BACKWARDS: u16 = 61;
    pub const CROUCH_RUN_SIDESTEP_LEFT: u16 = 62;
    pub const CROUCH_RUN_SIDESTEP_RIGHT: u16 = 63;
    pub const CROUCH_WALK_SIDESTEP_LEFT: u16 = 64;
    pub const CROUCH_WALK_SIDESTEP_RIGHT: u16 = 65;
    pub const RUN_KICK: u16 = 66;
    pub const RUN_PUNCH: u16 = 67;
    pub const RUN_BACK_PUNCH: u16 = 68;
    pub const RUN_BACK_KICK: u16 = 69;
    pub const SIDESTEP_LEFT_KICK: u16 = 70;
    pub const SIDESTEP_LEFT_PUNCH: u16 = 71;
    pub const SIDESTEP_RIGHT_KICK: u16 = 72;
    pub const SIDESTEP_RIGHT_PUNCH: u16 = 73;
    pub const PRONE: u16 = 74;
    pub const FLIP: u16 = 75;
    pub const IDLE: u16 = 93;
    pub const TAUNT: u16 = 94;
    pub const KICK3_FORWARD: u16 = 125;
    pub const PPKK: u16 = 133;
    pub const PPKKK: u16 = 134;
    pub const PPKKKK: u16 = 135;
    pub const LAND_HARD: u16 = 136;
    pub const LAND_FORWARD_HARD: u16 = 137;
    pub const LAND_RIGHT_HARD: u16 = 138;
    pub const LAND_LEFT_HARD: u16 = 139;
    pub const LAND_BACK_HARD: u16 = 140;
    pub const LAND_DEAD: u16 = 141;
    pub const CROUCH_TURN_LEFT: u16 = 142;
    pub const CROUCH_TURN_RIGHT: u16 = 143;
    pub const CROUCH_FORWARD: u16 = 144;
    pub const CROUCH_BACK: u16 = 145;
    pub const CROUCH_LEFT: u16 = 146;
    pub const CROUCH_RIGHT: u16 = 147;
    pub const GETUP_KICK_BACK: u16 = 148;
    pub const PUNCH4: u16 = 163;
    pub const STAGGER: u16 = 198;
    pub const RUN_STOP: u16 = 206;
    pub const WALK_STOP: u16 = 207;
    pub const RUN_START: u16 = 208;
    pub const WALK_START: u16 = 209;
    pub const RUN_BACKWARDS_START: u16 = 210;
    pub const WALK_BACKWARDS_START: u16 = 211;
    pub const STUN: u16 = 212;
    pub const STAGGER_BEHIND: u16 = 213;
    pub const ONE_STEP_STOP: u16 = 216;
    pub const RUN_SIDESTEP_LEFT_START: u16 = 217;
    pub const RUN_SIDESTEP_RIGHT_START: u16 = 218;
    pub const FALLING_FLAIL: u16 = 220;
    pub const PUNCH_RIFLE: u16 = 226;
}

/// Body states (the pose a clip starts from / ends in).
pub mod st {
    pub const ANYTHING: u16 = 1;
    pub const SLIDING: u16 = 4;
    pub const STANDING: u16 = 7;
    pub const RUN_SLIDE: u16 = 12;
    pub const FALLEN_BACK: u16 = 18;
    pub const CROUCH: u16 = 19;
    pub const RUN_BACK_SLIDE: u16 = 30;
    pub const LIE_BACK: u16 = 31;
    pub const FLYING: u16 = 37;
    pub const FALLING: u16 = 38;
    pub const FALLING_RIGHT: u16 = 46;
    pub const CROUCH_START: u16 = 47;
    pub const FALLEN_FRONT: u16 = 50;

    /// The airborne states (a clip entered from one of these launches the
    /// character into the air).
    pub fn is_flying(s: u16) -> bool {
        (FLYING..=FALLING_RIGHT).contains(&s)
    }
}

/// Variant bits a clip may require (`ONcAnimVarientMask_*`).
pub mod var {
    pub const SPRINT: u16 = 0x0100;
    pub const FIGHT: u16 = 0x0200;
}

/// Per-clip flag bit indices (`ONcAnimFlag_*`; mask = `1 << index`).
pub mod flag {
    pub const NO_INTERP_VELOCITY: u32 = 14;
    pub const THROW_SOURCE: u32 = 15;
    pub const THROW_TARGET: u32 = 16;
}

/// Shortcut flag masks (`TRtShortcut.flags`).
pub mod shortcut {
    /// May force in over an atomic clip (`ONcShortcutFlag_Nuke`, bit 0).
    pub const NUKE: u32 = 1 << 0;
}
