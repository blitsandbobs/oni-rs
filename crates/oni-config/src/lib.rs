//! Input configuration for Oni: the player **actions**, the **key/mouse
//! bindings** that map physical inputs to them, and **tuning** (mouse
//! sensitivity, invert-Y).
//!
//! This crate is portable and GUI-agnostic — it defines its own [`Key`] /
//! [`MouseButton`] identifiers rather than depending on a windowing library, so
//! the engine (`oni-view`) maps `winit` key codes onto these at the input
//! boundary. The defaults ([`Bindings::oni_defaults`]) reproduce Oni's stock
//! keyboard/mouse layout (`LIrInitialize`, `BFW_LocalInput.c:1007-1035`); a
//! later settings screen can rebind freely.

use std::collections::HashMap;

/// A player input action — mirrors Oni's `LIc_Bit_*` button bits. Mouse-look
/// (aim) is a continuous axis handled by the engine, not an `Action`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Forward,
    Backward,
    StepLeft,
    StepRight,
    TurnLeft,
    TurnRight,
    Jump,
    Crouch,
    /// Hold-to-walk modifier (Oni runs by default).
    Walk,
    Punch,
    Kick,
    Block,
    /// "Use"/interact (consoles, doors, pickups) — Oni's `action` bit.
    Use,
    /// Free-look: mouse turns the aim without turning the body.
    LookMode,
    Hypo,
    Reload,
    Swap,
    Drop,
    Fire1,
    Fire2,
    Fire3,
    Pause,
    Screenshot,
    Escape,
    /// Debug profiling toggle (bound in the shipped config; no effect here).
    ProfileToggle,
}

/// A portable keyboard key identifier. Variant names follow `winit`'s
/// `KeyCode` so the engine's mapping is mechanical; only the keys the stock
/// bindings (and common rebinds) need are listed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Space,
    Enter,
    Escape,
    Tab,
    CapsLock,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    Semicolon,
    Quote,
    BracketLeft,
    Comma,
    Backslash,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
}

/// A portable mouse-button identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Mouse / aim tuning. `mouse_sensitivity` scales over Oni's base `/64`
/// per-axis divisor (`HandleTurnInput`); `invert_y` flips aim-up/down.
#[derive(Clone, Copy, Debug)]
pub struct InputTuning {
    pub mouse_sensitivity: f32,
    pub invert_y: bool,
}

impl Default for InputTuning {
    fn default() -> Self {
        InputTuning {
            mouse_sensitivity: 1.0,
            invert_y: false,
        }
    }
}

/// Physical input → action map.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    keys: HashMap<Key, Action>,
    mouse: HashMap<MouseButton, Action>,
}

impl Bindings {
    /// Oni's stock bindings as the game shipped them: the `key_config.txt`
    /// installed with it, which clears the engine's built-in table and binds
    /// WASD (forward/back + strafe), mouse buttons = fire 1/2/3 (unarmed: punch
    /// / kick / crouch, see `oni-view`'s input), F/C = punch/kick, Shift =
    /// crouch, Caps Lock / Enter = walk, Ctrl = action, plus a second movement
    /// cluster (P / L / ; / ' with K, comma, O, [). `turnleft`/`turnright`/
    /// `block` are unbound, as in the original (turning is the mouse). Escape is
    /// kept for releasing the mouse.
    pub fn oni_defaults() -> Self {
        use Action::*;
        use Key::*;
        let keys = [
            (KeyW, Forward),
            (KeyA, StepLeft),
            (KeyS, Backward),
            (KeyD, StepRight),
            (KeyQ, Swap),
            (KeyE, Drop),
            (KeyF, Punch),
            (KeyC, Kick),
            (Space, Jump),
            (Key::F1, Action::Pause),
            (KeyV, LookMode),
            (ShiftLeft, Crouch),
            (ShiftRight, Crouch),
            (CapsLock, Walk),
            (ControlLeft, Use),
            (ControlRight, Use),
            (Tab, Hypo),
            (KeyR, Reload),
            (Backslash, ProfileToggle),
            (Key::F13, Screenshot),
            // The alternate cluster.
            (KeyP, Forward),
            (KeyL, StepLeft),
            (Quote, StepRight),
            (Semicolon, Backward),
            (KeyO, Swap),
            (BracketLeft, Drop),
            (KeyK, Punch),
            (Comma, Kick),
            (Enter, Walk),
            (Key::Escape, Action::Escape),
        ]
        .into_iter()
        .collect();
        let mouse = [
            (MouseButton::Left, Fire1),
            (MouseButton::Right, Fire2),
            (MouseButton::Middle, Fire3),
        ]
        .into_iter()
        .collect();
        Bindings { keys, mouse }
    }

    /// The action bound to `key`, if any.
    pub fn key_action(&self, key: Key) -> Option<Action> {
        self.keys.get(&key).copied()
    }

    /// The action bound to `button`, if any.
    pub fn mouse_action(&self, button: MouseButton) -> Option<Action> {
        self.mouse.get(&button).copied()
    }

    /// Binds `key` to `action` (replacing any existing binding on that key).
    pub fn bind_key(&mut self, key: Key, action: Action) {
        self.keys.insert(key, action);
    }

    /// Binds `button` to `action`.
    pub fn bind_mouse(&mut self, button: MouseButton, action: Action) {
        self.mouse.insert(button, action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oni_defaults_match_stock_layout() {
        let b = Bindings::oni_defaults();
        assert_eq!(b.key_action(Key::KeyW), Some(Action::Forward));
        assert_eq!(b.key_action(Key::KeyS), Some(Action::Backward));
        // A/D are STRAFE in Oni, not turn.
        assert_eq!(b.key_action(Key::KeyA), Some(Action::StepLeft));
        assert_eq!(b.key_action(Key::KeyD), Some(Action::StepRight));
        assert_eq!(b.key_action(Key::CapsLock), Some(Action::Walk));
        assert_eq!(b.mouse_action(MouseButton::Left), Some(Action::Fire1));
        assert_eq!(b.mouse_action(MouseButton::Right), Some(Action::Fire2));
        assert_eq!(b.mouse_action(MouseButton::Middle), Some(Action::Fire3));
        // The shipped config's alternates.
        assert_eq!(b.key_action(Key::ShiftRight), Some(Action::Crouch));
        assert_eq!(b.key_action(Key::KeyK), Some(Action::Punch));
        assert_eq!(b.key_action(Key::Comma), Some(Action::Kick));
        assert_eq!(b.key_action(Key::KeyP), Some(Action::Forward));
        // Turn keys are unbound by default (mouse turns).
        assert_eq!(b.key_action(Key::Digit0), None);
    }

    #[test]
    fn rebinding_overrides() {
        let mut b = Bindings::oni_defaults();
        b.bind_key(Key::KeyT, Action::Forward);
        assert_eq!(b.key_action(Key::KeyT), Some(Action::Forward));
    }
}
