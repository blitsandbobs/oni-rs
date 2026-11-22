//! What gates the player's input: script locks (`lock_keys`, `input`), the
//! AI driving her (`ai2_takecontrol`), and last tick's keys.

/// The player's input gates.
#[derive(Clone, Debug)]
pub struct InputGate {
    /// Which of the player's buttons work (`lock_keys`).
    pub key_mask: u32,
    /// Whether the player's input works at all (`input`).
    pub enabled: bool,
    /// The AI movement system drives the player (`ai2_takecontrol`): her
    /// keys come from her [`crate::ai::Brain::Steer`], not the input.
    pub ai_control: bool,
    /// Last tick's keys.
    pub(super) last: u32,
    /// Last tick's hypo key (held, it acts as pressed).
    pub(super) last_hypo_key: u32,
}

impl Default for InputGate {
    fn default() -> Self {
        InputGate {
            key_mask: u32::MAX,
            enabled: true,
            ai_control: false,
            last: 0,
            last_hypo_key: 0,
        }
    }
}
