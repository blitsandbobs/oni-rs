//! The melee AI's fixed vocabulary: the
//! coarse body states it plans in, and what each kind of move in a profile
//! means — which animation it asks for, from which state, in which direction,
//! with which keys. Profiles (data) name entries of these tables by index, in
//! the original's order (`oni_schema::MELEE_*`).
//!
//! Animation types and states are written by their schema names and resolved
//! once ([`ty`], [`anim_state`]).

use crate::anim::input::key as k;

/// The coarse body states the melee AI plans in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MeleeState {
    None,
    Standing,
    Crouching,
    CrouchStart,
    Prone,
    RunStart,
    RunBackStart,
    RunLeftStart,
    RunRightStart,
    Running,
    RunningBack,
    RunningLeft,
    RunningRight,
    Jumping,
    JumpingForwards,
    JumpingBack,
    JumpingLeft,
    JumpingRight,
}
use MeleeState as S;

/// A move's direction relative to the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    None,
    Forward,
    Back,
    Left,
    Right,
}

impl Dir {
    /// The yaw offset of this direction from the body's facing (+ = left).
    pub fn offset(self) -> Option<f32> {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            Dir::Forward => Some(0.0),
            Dir::Left => Some(FRAC_PI_2),
            Dir::Right => Some(-FRAC_PI_2),
            Dir::Back => Some(PI),
            Dir::None => None,
        }
    }

    /// The keys that move the body this way.
    pub fn keys(self) -> u32 {
        match self {
            Dir::Forward => k::FORWARD,
            Dir::Back => k::BACKWARD,
            Dir::Left => k::STEP_LEFT,
            Dir::Right => k::STEP_RIGHT,
            _ => 0,
        }
    }
}

/// An animation type by schema name.
pub fn ty(name: &str) -> u16 {
    oni_schema::value(&oni_schema::ANIM_TYPES, name)
        .unwrap_or_else(|| panic!("animation type {name}"))
}

/// An animation state by schema name.
pub fn anim_state(name: &str) -> u16 {
    oni_schema::value(&oni_schema::ANIM_STATES, name)
        .unwrap_or_else(|| panic!("animation state {name}"))
}

impl MeleeState {
    /// The melee state for an animation state, if it maps to one.
    pub fn from_anim_state(s: u16) -> MeleeState {
        let name = oni_schema::name(&oni_schema::ANIM_STATES, s).unwrap_or("none");
        match name {
            "standing" | "walkingLeftDown" | "walkingRightDown" => S::Standing,
            "crouch" => S::Crouching,
            "crouchStart" => S::CrouchStart,
            "fallenBack" | "fallenFront" | "lieBack" => S::Prone,
            "runningLeftDown"
            | "runningRightDown"
            | "runningUpstairsRightDown"
            | "runningUpstairsLeftDown" => S::Running,
            "runStart" | "runAccel" | "runJumpLand" => S::RunStart,
            "runBackStart" => S::RunBackStart,
            "runningBackRightDown" | "runningBackLeftDown" => S::RunningBack,
            "runSidestepLeft" | "sidestepLeftLeftDown" | "sidestepLeftRightDown" => S::RunningLeft,
            "sidestepLeftStart" | "sidestepLeftStartLongName" => S::RunLeftStart,
            "runSidestepRight" | "sidestepRightLeftDown" | "sidestepRightRightDown" => {
                S::RunningRight
            }
            "sidestepRightStart" | "sidestepRightStartLongName" => S::RunRightStart,
            "jumpUp" | "flying" | "falling" => S::Jumping,
            "runJump" | "jumpForward" | "fallingForward" | "flyingForward" => S::JumpingForwards,
            "fallingBack" | "flyingBack" => S::JumpingBack,
            "fallingLeft" | "flyingLeft" | "sidestepLeftJump" => S::JumpingLeft,
            "fallingRight" | "flyingRight" | "sidestepRightJump" => S::JumpingRight,
            _ => S::None,
        }
    }

    /// The animation type that "keeps going" in this state (a run-in delay
    /// is spent playing it).
    pub fn delay_anim(self) -> &'static str {
        match self {
            S::None => "none",
            S::Standing => "stand",
            S::Crouching | S::CrouchStart => "crouch",
            S::Prone => "hitFallen",
            S::RunStart | S::Running => "run",
            S::RunBackStart | S::RunningBack => "runBackwards",
            S::RunLeftStart | S::RunningLeft => "runSidestepLeft",
            S::RunRightStart | S::RunningRight => "runSidestepRight",
            _ => "fly",
        }
    }

    /// Blocks crouched in this state.
    pub fn blocks_crouching(self) -> bool {
        matches!(self, S::Crouching | S::CrouchStart)
    }

    /// May turn on the spot to line an attack up.
    pub fn can_turn_for_attack(self) -> bool {
        matches!(
            self,
            S::None
                | S::Standing
                | S::Crouching
                | S::RunStart
                | S::RunBackStart
                | S::RunLeftStart
                | S::RunRightStart
                | S::Running
                | S::RunningBack
                | S::RunningLeft
                | S::RunningRight
        )
    }

    /// Keys that keep the body in this state.
    pub fn maintain_keys(self) -> u32 {
        match self {
            S::Crouching | S::CrouchStart => k::CROUCH,
            S::RunStart | S::Running => k::FORWARD,
            S::RunBackStart | S::RunningBack => k::BACKWARD,
            S::RunLeftStart | S::RunningLeft => k::STEP_LEFT,
            S::RunRightStart | S::RunningRight => k::STEP_RIGHT,
            S::Jumping => k::JUMP,
            S::JumpingForwards => k::JUMP | k::FORWARD,
            S::JumpingBack => k::JUMP | k::BACKWARD,
            S::JumpingLeft => k::JUMP | k::STEP_LEFT,
            S::JumpingRight => k::JUMP | k::STEP_RIGHT,
            _ => 0,
        }
    }

    pub fn is_jumping(self) -> bool {
        matches!(
            self,
            S::Jumping | S::JumpingForwards | S::JumpingBack | S::JumpingLeft | S::JumpingRight
        )
    }

    /// The animation state an attack from this melee state starts in.
    pub fn anim_state(self) -> u16 {
        anim_state(match self {
            S::None => "none",
            S::Standing => "standing",
            S::Crouching => "crouch",
            S::CrouchStart => "crouchStart",
            S::Prone => "fallenBack",
            S::RunStart => "runStart",
            S::RunBackStart => "runBackStart",
            S::RunLeftStart => "sidestepLeftStartLongName",
            S::RunRightStart => "sidestepRightStartLongName",
            S::Running => "runningRightDown",
            S::RunningBack => "runningBackRightDown",
            S::RunningLeft => "sidestepLeftRightDown",
            S::RunningRight => "sidestepRightRightDown",
            S::Jumping => "falling",
            S::JumpingForwards => "fallingForward",
            S::JumpingBack => "fallingBack",
            S::JumpingLeft => "fallingLeft",
            S::JumpingRight => "fallingRight",
        })
    }
}

/// An attack: the animation it asks for, the states it goes from and to, and
/// the direction it strikes. Keys come from the movement rules (whichever
/// first produces this animation), except where given here.
#[allow(dead_code)] // kept whole for fidelity to the original's table
#[derive(Clone, Copy, Debug)]
pub struct AttackMove {
    pub anim: &'static str,
    pub from: MeleeState,
    pub to: MeleeState,
    pub dir: Dir,
    /// Fixed keys (held, pressed) for attacks the rules can't name.
    pub keys: Option<(u32, u32)>,
}

const fn atk(anim: &'static str, from: MeleeState, to: MeleeState, dir: Dir) -> AttackMove {
    AttackMove {
        anim,
        from,
        to,
        dir,
        keys: None,
    }
}

/// The attack moves, in `oni_schema::MELEE_ATTACKS` order.
pub const ATTACKS: [AttackMove; 84] = {
    use Dir::*;
    let st = S::Standing;
    [
        atk("punch", st, st, Forward),
        atk("punch2", st, st, Forward),
        atk("punch3", st, st, Forward),
        atk("punch4", st, st, Forward),
        atk("punchForward", st, st, Forward),
        atk("punchLeft", st, st, Left),
        atk("punchRight", st, st, Right),
        atk("punchBack", st, st, Back),
        atk("punchLow", st, st, Forward),
        atk("pfPf", st, st, Forward),
        atk("pfPfPf", st, st, Forward),
        atk("plPl", st, st, Left),
        atk("plPlPl", st, st, Left),
        atk("prPr", st, st, Right),
        atk("prPrPr", st, st, Right),
        atk("pbPb", st, st, Back),
        atk("pbPbPb", st, st, Back),
        atk("pdPd", st, st, Forward),
        atk("pdPdPd", st, st, Forward),
        atk("kick", st, st, Forward),
        atk("kick2", st, st, Forward),
        atk("kick3", st, st, Forward),
        atk("kick3Forward", st, st, Forward),
        atk("kickForward", st, st, Forward),
        atk("kickLeft", st, st, Left),
        atk("kickRight", st, st, Right),
        atk("kickBack", st, st, Back),
        atk("kickLow", st, st, Forward),
        atk("kfKf", st, st, Forward),
        atk("kfKfKf", st, st, Forward),
        atk("klKl", st, st, Left),
        atk("klKlKl", st, st, Left),
        atk("krKr", st, st, Right),
        atk("krKrKr", st, st, Right),
        atk("kbKb", st, st, Back),
        atk("kbKbKb", st, st, Back),
        atk("kdKd", st, st, Forward),
        atk("kdKdKd", st, st, Forward),
        atk("ppk", st, st, Forward),
        atk("pkk", st, st, Forward),
        atk("pkp", st, st, Forward),
        atk("kpk", st, st, Forward),
        atk("kpp", st, st, Forward),
        atk("kkp", st, st, Forward),
        atk("pk", st, st, Forward),
        atk("kp", st, st, Forward),
        atk("ppkk", st, st, Forward),
        atk("ppkkk", st, st, Forward),
        atk("ppkkkk", st, st, Forward),
        atk("punchHeavy", st, st, Forward),
        atk("punchForwardHeavy", st, st, Forward),
        atk("kickHeavy", st, st, Forward),
        atk("kickForwardHeavy", st, st, Forward),
        atk("punch", S::CrouchStart, st, Forward),
        atk("kick", S::CrouchStart, st, Forward),
        atk("punch", S::Crouching, S::Crouching, Forward),
        atk("punchLow", S::Crouching, S::Crouching, Forward),
        atk("punchForward", S::Crouching, S::Crouching, Forward),
        atk("kick", S::Crouching, S::Crouching, Forward),
        atk("kickLow", S::Crouching, S::Crouching, Forward),
        atk("kickForward", S::Crouching, S::Crouching, Forward),
        atk("kick", S::Prone, st, Forward),
        atk("getupKickBack", S::Prone, st, Back),
        atk("runPunch", S::Running, S::Running, Forward),
        atk("runKick", S::Running, S::Running, Forward),
        atk("runBackPunch", S::RunningBack, S::RunningBack, Back),
        atk("runBackKick", S::RunningBack, S::RunningBack, Back),
        atk("sidestepLeftPunch", S::RunningLeft, S::RunningLeft, Left),
        atk("sidestepLeftKick", S::RunningLeft, S::RunningLeft, Left),
        atk(
            "sidestepRightPunch",
            S::RunningRight,
            S::RunningRight,
            Right,
        ),
        atk("sidestepRightKick", S::RunningRight, S::RunningRight, Right),
        AttackMove {
            anim: "slide",
            from: S::Running,
            to: st,
            dir: Forward,
            keys: Some((k::FORWARD, k::CROUCH)),
        },
        atk("punch", S::Jumping, S::Jumping, Forward),
        atk("kick", S::Jumping, S::Jumping, Forward),
        atk("punch", S::JumpingForwards, S::JumpingForwards, Forward),
        atk("punchBack", S::JumpingForwards, S::JumpingForwards, Back),
        atk("kick", S::JumpingForwards, S::JumpingForwards, Forward),
        atk("kickBack", S::JumpingForwards, S::JumpingForwards, Back),
        atk("punchBack", S::JumpingBack, S::JumpingBack, Back),
        atk("kickBack", S::JumpingBack, S::JumpingBack, Back),
        atk("punch", S::JumpingLeft, S::JumpingLeft, Left),
        atk("kick", S::JumpingLeft, S::JumpingLeft, Left),
        atk("punch", S::JumpingRight, S::JumpingRight, Right),
        atk("kick", S::JumpingRight, S::JumpingRight, Right),
    ]
};

/// How a position move jumps, if it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum JumpKind {
    None,
    Vertical,
    Standing,
    RunForward,
    RunBack,
    RunLeft,
    RunRight,
}

/// A position move: the states to pass through (in order) to be ready for
/// the attack that follows. A delaying move keeps running in at its first
/// state until the attack would connect.
#[allow(dead_code)] // kept whole for fidelity to the original's table
#[derive(Clone, Copy, Debug)]
pub struct PositionMove {
    pub dir: Dir,
    pub can_delay: bool,
    pub can_wait_for_facing: bool,
    pub ignore_end_distance: bool,
    pub jump: JumpKind,
    pub states: &'static [MeleeState],
}

const fn pos(
    dir: Dir,
    flags: (bool, bool, bool),
    jump: JumpKind,
    states: &'static [MeleeState],
) -> PositionMove {
    PositionMove {
        dir,
        can_delay: flags.0,
        can_wait_for_facing: flags.1,
        ignore_end_distance: flags.2,
        jump,
        states,
    }
}

/// The position moves, in `oni_schema::MELEE_POSITIONS` order.
pub const POSITIONS: [PositionMove; 20] = {
    use Dir::*;
    use JumpKind as J;
    const RUN: (bool, bool, bool) = (true, true, false);
    const JUMP: (bool, bool, bool) = (false, false, false);
    const HOLD: (bool, bool, bool) = (false, true, false);
    const CLOSE: (bool, bool, bool) = (true, true, true);
    const RUNJUMP: (bool, bool, bool) = (true, false, false);
    [
        pos(Forward, RUN, J::None, &[S::Running]),
        pos(Left, RUN, J::None, &[S::RunningLeft]),
        pos(Right, RUN, J::None, &[S::RunningRight]),
        pos(Back, RUN, J::None, &[S::RunningBack]),
        pos(None, JUMP, J::Vertical, &[S::Standing, S::Jumping]),
        pos(Forward, JUMP, J::Standing, &[S::JumpingForwards]),
        pos(Left, JUMP, J::Standing, &[S::JumpingLeft]),
        pos(Right, JUMP, J::Standing, &[S::JumpingRight]),
        pos(Back, JUMP, J::Standing, &[S::JumpingBack]),
        pos(None, JUMP, J::None, &[S::CrouchStart]),
        pos(None, HOLD, J::None, &[S::Crouching]),
        pos(None, HOLD, J::None, &[S::Standing]),
        pos(Forward, CLOSE, J::None, &[S::Running, S::Standing]),
        pos(Left, CLOSE, J::None, &[S::RunningLeft, S::Standing]),
        pos(Right, CLOSE, J::None, &[S::RunningRight, S::Standing]),
        pos(Back, CLOSE, J::None, &[S::RunningBack, S::Standing]),
        pos(
            Forward,
            RUNJUMP,
            J::RunForward,
            &[S::Running, S::JumpingForwards],
        ),
        pos(Left, RUNJUMP, J::RunLeft, &[S::RunningLeft, S::JumpingLeft]),
        pos(
            Right,
            RUNJUMP,
            J::RunRight,
            &[S::RunningRight, S::JumpingRight],
        ),
        pos(Back, RUNJUMP, J::RunBack, &[S::RunningBack, S::JumpingBack]),
    ]
};

/// A maneuver: a moving one steers in its direction; the others hold keys.
// Kept whole for fidelity to the original's table; not every field drives
// the modelled behaviour yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Maneuver {
    /// Moving maneuvers (advance / retreat / circle) steer in this direction.
    pub moving: Option<Dir>,
    /// Keys held by the non-moving ones.
    pub keys: u32,
    pub from: MeleeState,
    pub to: MeleeState,
    pub anim: &'static str,
}

const fn man(
    moving: Option<Dir>,
    keys: u32,
    from: MeleeState,
    to: MeleeState,
    anim: &'static str,
) -> Maneuver {
    Maneuver {
        moving,
        keys,
        from,
        to,
        anim,
    }
}

/// Maneuver indices with their own handling.
pub mod maneuver {
    pub const ADVANCE: usize = 0;
    pub const RETREAT: usize = 1;
    pub const CIRCLE_LEFT: usize = 2;
    pub const CIRCLE_RIGHT: usize = 3;
    pub const RANDOM_STOP: usize = 8;
    pub const BARABBAS_WAVE: usize = 13;
}

/// The maneuvers, in `oni_schema::MELEE_MANEUVERS` order.
pub const MANEUVERS: [Maneuver; 14] = {
    use Dir::*;
    [
        man(Some(Forward), 0, S::None, S::Standing, "run"),
        man(Some(Back), 0, S::None, S::Standing, "runBackwards"),
        man(Some(Left), 0, S::None, S::Standing, "runSidestepLeft"),
        man(Some(Right), 0, S::None, S::Standing, "runSidestepRight"),
        man(Option::None, 0, S::None, S::Standing, "stand"),
        man(Option::None, k::CROUCH, S::Standing, S::Crouching, "crouch"),
        man(Option::None, k::JUMP, S::None, S::Jumping, "jump"),
        man(Option::None, ACTION, S::None, S::Standing, "taunt"),
        man(Option::None, 0, S::None, S::Standing, "stand"),
        man(Option::None, k::FORWARD, S::Prone, S::Standing, "run"),
        man(
            Option::None,
            k::BACKWARD,
            S::Prone,
            S::Standing,
            "runBackwards",
        ),
        man(
            Option::None,
            TURN_LEFT,
            S::Prone,
            S::Standing,
            "standingTurnLeft",
        ),
        man(
            Option::None,
            TURN_RIGHT,
            S::Prone,
            S::Standing,
            "standingTurnRight",
        ),
        man(Option::None, k::KICK, S::CrouchStart, S::Standing, "kick"),
    ]
};

const ACTION: u32 = k::ACTION;
const TURN_LEFT: u32 = k::TURN_LEFT;
const TURN_RIGHT: u32 = k::TURN_RIGHT;

/// An evasion: a jump, roll or slide out of the way.
// Kept whole for fidelity to the original's table; not every field drives
// the modelled behaviour yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Evade {
    pub from: MeleeState,
    pub to: MeleeState,
    pub anim: &'static str,
    /// Keys held, keys pressed.
    pub keys: (u32, u32),
}

const fn ev(from: MeleeState, to: MeleeState, anim: &'static str, hold: u32, press: u32) -> Evade {
    Evade {
        from,
        to,
        anim,
        keys: (hold, press),
    }
}

/// The evasions, in `oni_schema::MELEE_EVADES` order.
pub const EVADES: [Evade; 24] = [
    ev(
        S::Standing,
        S::JumpingForwards,
        "jump",
        k::FORWARD | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingForwards,
        "jumpForward",
        k::FORWARD | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingBack,
        "jump",
        k::BACKWARD | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingBack,
        "jumpBackward",
        k::BACKWARD | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingLeft,
        "jump",
        k::STEP_LEFT | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingLeft,
        "jumpLeft",
        k::STEP_LEFT | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingRight,
        "jump",
        k::STEP_RIGHT | k::JUMP,
        0,
    ),
    ev(
        S::Standing,
        S::JumpingRight,
        "jumpRight",
        k::STEP_RIGHT | k::JUMP,
        0,
    ),
    ev(S::Running, S::JumpingForwards, "jump", k::JUMP, k::FORWARD),
    ev(
        S::Running,
        S::JumpingForwards,
        "jumpForward",
        k::JUMP,
        k::FORWARD,
    ),
    ev(S::RunningBack, S::JumpingBack, "jump", k::JUMP, k::BACKWARD),
    ev(
        S::RunningBack,
        S::JumpingBack,
        "jumpBackward",
        k::JUMP,
        k::BACKWARD,
    ),
    ev(
        S::RunningLeft,
        S::JumpingLeft,
        "jump",
        k::JUMP,
        k::STEP_LEFT,
    ),
    ev(
        S::RunningLeft,
        S::JumpingLeft,
        "jumpLeft",
        k::JUMP,
        k::STEP_LEFT,
    ),
    ev(
        S::RunningRight,
        S::JumpingRight,
        "jump",
        k::JUMP,
        k::STEP_RIGHT,
    ),
    ev(
        S::RunningRight,
        S::JumpingRight,
        "jumpRight",
        k::JUMP,
        k::STEP_RIGHT,
    ),
    ev(
        S::Standing,
        S::Standing,
        "crouchForward",
        k::FORWARD,
        k::CROUCH,
    ),
    ev(
        S::Standing,
        S::Standing,
        "crouchBack",
        k::BACKWARD,
        k::CROUCH,
    ),
    ev(
        S::Standing,
        S::Standing,
        "crouchLeft",
        k::STEP_LEFT,
        k::CROUCH,
    ),
    ev(
        S::Standing,
        S::Standing,
        "crouchRight",
        k::STEP_RIGHT,
        k::CROUCH,
    ),
    ev(S::Running, S::Running, "slide", k::FORWARD, k::CROUCH),
    ev(
        S::RunningBack,
        S::RunningBack,
        "slide",
        k::BACKWARD,
        k::CROUCH,
    ),
    ev(
        S::RunningLeft,
        S::RunningLeft,
        "slide",
        k::STEP_LEFT,
        k::CROUCH,
    ),
    ev(
        S::RunningRight,
        S::RunningRight,
        "slide",
        k::STEP_RIGHT,
        k::CROUCH,
    ),
];

/// A throw: the thrower's animation type and the situation it needs.
#[derive(Clone, Copy, Debug)]
pub struct ThrowMove {
    pub anim: &'static str,
    pub from_behind: bool,
    pub running: bool,
    pub disarm: Disarm,
    /// The victim must be running (the tackle).
    pub victim_running: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disarm {
    No,
    Pistol,
    Rifle,
}

const fn thr(anim: &'static str, from_behind: bool, running: bool, disarm: Disarm) -> ThrowMove {
    ThrowMove {
        anim,
        from_behind,
        running,
        disarm,
        victim_running: false,
    }
}

/// The throws, in `oni_schema::MELEE_THROWS` order.
pub const THROWS: [ThrowMove; 25] = {
    const fn set(d: Disarm) -> [ThrowMove; 8] {
        [
            thr("throwForwardPunch", false, false, d),
            thr("throwForwardKick", false, false, d),
            thr("throwBehindPunch", true, false, d),
            thr("throwBehindKick", true, false, d),
            thr("runThrowForwardPunch", false, true, d),
            thr("runThrowForwardKick", false, true, d),
            thr("runThrowBehindPunch", true, true, d),
            thr("runThrowBehindKick", true, true, d),
        ]
    }
    let (a, b, c) = (set(Disarm::No), set(Disarm::Pistol), set(Disarm::Rifle));
    let mut out = [thr("", false, false, Disarm::No); 25];
    let mut i = 0;
    while i < 8 {
        out[i] = a[i];
        out[8 + i] = b[i];
        out[16 + i] = c[i];
        i += 1;
    }
    out[24] = ThrowMove {
        anim: "runThrowBehindPunch",
        from_behind: true,
        running: true,
        disarm: Disarm::No,
        victim_running: true,
    };
    out
};

/// One step between melee states: from `from`, playing `anim` (if the
/// character has it) by holding `keys`, reaching any of `to`; `weight` is the
/// preference cost of taking it (lower = avoided).
// Kept whole for fidelity to the original's table; not every field drives
// the modelled behaviour yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    pub dir: Dir,
    pub from: MeleeState,
    pub essential: bool,
    pub to: &'static [MeleeState],
    /// Alternative animation types (a character uses the first it has).
    pub anims: &'static [&'static str],
    pub keys: u32,
    pub weight: f32,
}

const fn tr(
    dir: Dir,
    from: MeleeState,
    essential: bool,
    to: &'static [MeleeState],
    anims: &'static [&'static str],
    keys: u32,
    weight: f32,
) -> Transition {
    Transition {
        dir,
        from,
        essential,
        to,
        anims,
        keys,
        weight,
    }
}

/// The transition graph, preferred transitions first.
pub const TRANSITIONS: &[Transition] = {
    use Dir::*;
    const STOP_TO: &[MeleeState] = &[S::Standing, S::CrouchStart, S::Crouching, S::Jumping];
    const UP_TO: &[MeleeState] = &[S::CrouchStart, S::Standing, S::Running, S::JumpingForwards];
    const RISE_TO: &[MeleeState] = &[S::Standing, S::Running, S::JumpingForwards];
    &[
        // Standing jumps.
        tr(
            Forward,
            S::Standing,
            true,
            &[S::Jumping],
            &["jump"],
            k::JUMP,
            0.7,
        ),
        tr(
            Forward,
            S::Standing,
            true,
            &[S::JumpingForwards],
            &["jump", "jumpForward"],
            k::FORWARD | k::JUMP,
            0.7,
        ),
        tr(
            Left,
            S::Standing,
            true,
            &[S::JumpingLeft],
            &["jump", "jumpLeft"],
            k::STEP_LEFT | k::JUMP,
            0.7,
        ),
        tr(
            Right,
            S::Standing,
            true,
            &[S::JumpingRight],
            &["jump", "jumpRight"],
            k::STEP_RIGHT | k::JUMP,
            0.7,
        ),
        tr(
            Back,
            S::Standing,
            true,
            &[S::JumpingBack],
            &["jump", "jumpBackward"],
            k::BACKWARD | k::JUMP,
            0.7,
        ),
        // Running jumps.
        tr(
            Forward,
            S::Running,
            true,
            &[S::JumpingForwards],
            &["jump", "jumpForward"],
            k::FORWARD | k::JUMP,
            0.7,
        ),
        tr(
            Left,
            S::RunningLeft,
            true,
            &[S::JumpingLeft],
            &["jump", "jumpLeft"],
            k::STEP_LEFT | k::JUMP,
            0.7,
        ),
        tr(
            Right,
            S::RunningRight,
            true,
            &[S::JumpingRight],
            &["jump", "jumpRight"],
            k::STEP_RIGHT | k::JUMP,
            0.7,
        ),
        tr(
            Back,
            S::RunningBack,
            true,
            &[S::JumpingBack],
            &["jump", "jumpBackward"],
            k::BACKWARD | k::JUMP,
            0.7,
        ),
        // Getting up (heavily avoided: get-up attacks are better).
        tr(
            Forward,
            S::Prone,
            true,
            &[S::Standing, S::Running],
            &["run"],
            k::FORWARD,
            0.2,
        ),
        tr(
            Left,
            S::Prone,
            true,
            &[S::Standing, S::RunningLeft],
            &["standingTurnLeft"],
            TURN_LEFT,
            0.2,
        ),
        tr(
            Right,
            S::Prone,
            true,
            &[S::Standing, S::RunningRight],
            &["standingTurnRight"],
            TURN_RIGHT,
            0.2,
        ),
        tr(
            Back,
            S::Prone,
            true,
            &[S::Standing, S::RunningBack],
            &["runBackwards"],
            k::BACKWARD,
            0.2,
        ),
        // Starting to run.
        tr(
            Forward,
            S::Standing,
            true,
            &[S::Running, S::JumpingForwards],
            &["run", "runStart"],
            k::FORWARD,
            0.7,
        ),
        tr(
            Left,
            S::Standing,
            true,
            &[S::RunningLeft, S::JumpingLeft],
            &["runSidestepLeft", "runSidestepLeftStart"],
            k::STEP_LEFT,
            0.7,
        ),
        tr(
            Right,
            S::Standing,
            true,
            &[S::RunningRight, S::JumpingRight],
            &["runSidestepRight", "runSidestepRightStart"],
            k::STEP_RIGHT,
            0.7,
        ),
        tr(
            Back,
            S::Standing,
            true,
            &[S::RunningBack, S::JumpingBack],
            &["runBackwards", "runBackwardsStart"],
            k::BACKWARD,
            0.7,
        ),
        // Accelerating.
        tr(
            Forward,
            S::RunStart,
            true,
            &[S::Running, S::JumpingForwards],
            &["run", "runStart"],
            k::FORWARD,
            1.0,
        ),
        tr(
            Left,
            S::RunLeftStart,
            false,
            &[S::RunningLeft, S::JumpingLeft],
            &["runSidestepLeft", "runSidestepLeftStart"],
            k::STEP_LEFT,
            1.0,
        ),
        tr(
            Right,
            S::RunRightStart,
            false,
            &[S::RunningRight, S::JumpingRight],
            &["runSidestepRight", "runSidestepRightStart"],
            k::STEP_RIGHT,
            1.0,
        ),
        tr(
            Back,
            S::RunBackStart,
            true,
            &[S::RunningBack, S::JumpingBack],
            &["runBackwards", "runBackwardsStart"],
            k::BACKWARD,
            1.0,
        ),
        // Slowing from a run.
        tr(
            Forward,
            S::Running,
            true,
            STOP_TO,
            &["stand", "runStop"],
            0,
            0.7,
        ),
        tr(
            Left,
            S::RunningLeft,
            true,
            STOP_TO,
            &["stand", "runStop"],
            0,
            0.7,
        ),
        tr(
            Right,
            S::RunningRight,
            true,
            STOP_TO,
            &["stand", "runStop"],
            0,
            0.7,
        ),
        tr(
            Back,
            S::RunningBack,
            true,
            STOP_TO,
            &["stand", "runStop"],
            0,
            0.7,
        ),
        // Stopping from a run start.
        tr(
            Forward,
            S::RunStart,
            true,
            STOP_TO,
            &["stand", "runStop", "oneStepStop"],
            0,
            1.0,
        ),
        tr(
            Left,
            S::RunLeftStart,
            false,
            STOP_TO,
            &["stand", "runStop", "oneStepStop"],
            0,
            1.0,
        ),
        tr(
            Right,
            S::RunRightStart,
            false,
            STOP_TO,
            &["stand", "runStop", "oneStepStop"],
            0,
            1.0,
        ),
        tr(
            Back,
            S::RunBackStart,
            true,
            STOP_TO,
            &["stand", "runStop", "oneStepStop"],
            0,
            1.0,
        ),
        // Crouching.
        tr(
            Forward,
            S::Standing,
            true,
            &[S::CrouchStart, S::Crouching],
            &["crouch"],
            k::CROUCH,
            1.0,
        ),
        tr(
            Forward,
            S::CrouchStart,
            true,
            &[S::Crouching],
            &["crouch"],
            k::CROUCH,
            0.5,
        ),
        // Standing up from a crouch.
        tr(Forward, S::Crouching, true, UP_TO, &["stand"], 0, 0.8),
        tr(Left, S::Crouching, true, UP_TO, &["stand"], 0, 0.8),
        tr(Right, S::Crouching, true, UP_TO, &["stand"], 0, 0.8),
        tr(Back, S::Crouching, true, UP_TO, &["stand"], 0, 0.8),
        tr(
            Forward,
            S::Crouching,
            true,
            &[S::Standing, S::Jumping],
            &["stand"],
            0,
            1.0,
        ),
        tr(Forward, S::CrouchStart, true, RISE_TO, &["stand"], 0, 0.8),
        tr(Left, S::CrouchStart, true, RISE_TO, &["stand"], 0, 0.8),
        tr(Right, S::CrouchStart, true, RISE_TO, &["stand"], 0, 0.8),
        tr(Back, S::CrouchStart, true, RISE_TO, &["stand"], 0, 0.8),
        tr(
            Forward,
            S::CrouchStart,
            true,
            &[S::Standing, S::Jumping],
            &["stand"],
            0,
            1.0,
        ),
    ]
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_animation_exists() {
        for a in &ATTACKS {
            ty(a.anim);
        }
        for m in &MANEUVERS {
            ty(m.anim);
        }
        for e in &EVADES {
            ty(e.anim);
        }
        for t in &THROWS {
            ty(t.anim);
        }
        for t in TRANSITIONS {
            for a in t.anims {
                ty(a);
            }
        }
    }

    #[test]
    fn anim_states_map_to_melee_states() {
        assert_eq!(
            MeleeState::from_anim_state(anim_state("standing")),
            MeleeState::Standing
        );
        assert_eq!(
            MeleeState::from_anim_state(anim_state("fallenFront")),
            MeleeState::Prone
        );
        assert_eq!(
            MeleeState::from_anim_state(anim_state("falling")),
            MeleeState::Jumping
        );
    }
}
