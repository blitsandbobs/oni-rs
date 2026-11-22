//! The sounds of the level's objects and of the game itself
//! (`docs/AUDIO.md`, *Objects and the game*): doors opening and closing,
//! consoles, lasers; the game's event sounds (pickups, objectives, saving)
//! and its condition loops (low health, overcharged, invisible).

use std::collections::HashMap;
use std::path::Path;

use glam::Vec3;
use serde_json::Value;

use super::World;

/// The condition loops: their names in `conditionSounds`.
const CONDITIONS: [&str; 5] = [
    "healthLow",
    "healthOver",
    "shield",
    "invis",
    "timerCritical",
];
/// How quiet a condition loop gets.
const CONDITION_MIN_VOLUME: f32 = 0.4;
/// Invisibility's full length (ticks).
const MAX_INVISIBILITY: f32 = 60.0 * 30.0;

/// A door class's sounds: opening, closing, its noise for the AI.
type DoorSounds = (
    String,
    String,
    Option<(crate::ai::perception::Stimulus, f32)>,
);

#[derive(Default)]
pub(crate) struct GameSounds {
    events: HashMap<String, String>,
    conditions: HashMap<String, String>,
    pub(super) boosted_health: f32,
    /// Door class → (open, close, AI noise).
    doors: HashMap<String, DoorSounds>,
    /// Laser class → (active ambient, trip impulse).
    lasers: HashMap<String, (String, String)>,
    /// The condition loops playing (their playing ambients).
    playing: [Option<Option<u32>>; 5],
    /// Each door's last direction (true: opening or open).
    door_open: Vec<bool>,
    /// Each laser's active ambient (an audio handle).
    laser_hum: Vec<Option<Option<usize>>>,
}

fn names(j: &Value) -> HashMap<String, String> {
    j.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect()
}

fn read(path: &Path) -> Value {
    crate::json::load(path)
}

impl GameSounds {
    pub fn load(common: &Path) -> GameSounds {
        let gs = read(&common.join("game_settings.json"));
        let doors = read(&common.join("door_classes.json"));
        let lasers = read(&common.join("laser_classes.json"));
        let s = |v: &Value| v.as_str().unwrap_or("").to_string();
        GameSounds {
            events: names(&gs["eventSounds"]),
            conditions: names(&gs["conditionSounds"]),
            boosted_health: gs["boostedHealth"].as_f64().unwrap_or(1.5) as f32,
            doors: doors["classes"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, c)| {
                    let ai = c["ai"].is_object().then(|| {
                        let kind = match c["ai"]["type"].as_str() {
                            Some("interesting") => {
                                crate::ai::perception::Stimulus::SoundInteresting
                            }
                            Some("danger") => crate::ai::perception::Stimulus::SoundDanger,
                            Some("melee") => crate::ai::perception::Stimulus::SoundMelee,
                            Some("gunshot") => crate::ai::perception::Stimulus::SoundGunshot,
                            _ => crate::ai::perception::Stimulus::SoundIgnore,
                        };
                        (kind, c["ai"]["radius"].as_f64().unwrap_or(0.0) as f32)
                    });
                    (k.clone(), (s(&c["open"]), s(&c["close"]), ai))
                })
                .collect(),
            lasers: lasers["classes"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, c)| (k.clone(), (s(&c["active"]), s(&c["trigger"]))))
                .collect(),
            ..Default::default()
        }
    }
}

impl World {
    /// Plays game event sound `event` (`eventSounds`; empty: silent), at
    /// `at` or unplaced.
    pub fn event_sound(&mut self, event: &str, at: Option<Vec3>) {
        let Some(name) = self
            .game_sounds
            .events
            .get(event)
            .filter(|n| !n.is_empty())
            .cloned()
        else {
            return;
        };
        self.audio.impulse_play(&name, at, None, Some(&self.quads));
    }

    /// Like `event_sound`, but not during a cutscene or the level's first
    /// half second (the compass, objectives).
    pub fn event_sound_ui(&mut self, event: &str) {
        if !self.cutscene.on && self.now >= 30 {
            self.event_sound(event, None);
        }
    }

    /// Door `k` started opening or closing: its class's sound at the door,
    /// and its noise for the AI.
    pub(super) fn door_sounds(&mut self) {
        let n = self.doors.list.len();
        self.game_sounds.door_open.resize(n, false);
        for k in 0..n {
            use crate::doors::State;
            let open = matches!(self.doors.list[k].state, State::Opening(_) | State::Open(_));
            if open == self.game_sounds.door_open[k] {
                continue;
            }
            self.game_sounds.door_open[k] = open;
            let (class, at) = (
                self.doors.list[k].class.clone(),
                self.doors.list[k].position,
            );
            let Some((opens, closes, ai)) = self.game_sounds.doors.get(&class).cloned() else {
                continue;
            };
            let sound = if open { opens } else { closes };
            if !sound.is_empty() {
                self.audio
                    .impulse_play(&sound, Some(at), None, Some(&self.quads));
            }
            if let Some((stimulus, radius)) = ai {
                self.sound(stimulus, at, radius, None, None);
            }
        }
    }

    /// The texture a console's screen shows: by its state (inactive,
    /// active, used); none set, `NONE`.
    pub fn screen_of(c: &crate::consoles::Console) -> String {
        let k = if !c.active {
            0
        } else if c.triggered {
            2
        } else {
            1
        };
        if c.screens[k].is_empty() {
            "NONE".to_string()
        } else {
            c.screens[k].to_uppercase()
        }
    }

    /// A console's sound for fighter `user` using it (`what`: locked,
    /// already used, activate), between its marker and the user.
    pub(super) fn console_sound(&mut self, c: usize, user: Option<usize>, what: &str) {
        let marker = self.consoles[c].marker;
        let at = user
            .and_then(|u| self.fighters.get(u))
            .map_or(marker, |f| (marker + f.feet()) * 0.5);
        self.audio.impulse_simple(what, at);
    }

    /// Laser `k`'s hum while active and showing; its trip sound when it
    /// trips.
    pub(super) fn laser_sounds(&mut self, k: usize, tripped: bool) {
        let n = self.lasers.len();
        self.game_sounds.laser_hum.resize(n, None);
        let l = &self.lasers[k];
        let (on, at, class) = (l.active && !l.hidden, l.position, l.class.clone());
        let Some((hum, trip)) = self.game_sounds.lasers.get(&class).cloned() else {
            return;
        };
        if tripped && !trip.is_empty() {
            self.audio
                .impulse_play(&trip, Some(at), None, Some(&self.quads));
        }
        match (on, self.game_sounds.laser_hum[k]) {
            (true, None) => {
                let h = (!hum.is_empty())
                    .then(|| self.audio.ambient_start(&hum, at, Some(&self.quads)))
                    .flatten();
                self.game_sounds.laser_hum[k] = Some(h);
            }
            (false, Some(h)) => {
                if let Some(h) = h {
                    self.audio.ambient_stop(h);
                }
                self.game_sounds.laser_hum[k] = None;
            }
            _ => {}
        }
    }

    /// The player's condition loops: low health (louder the lower), health
    /// over its maximum (by how far), invisibility (by how much is left) —
    /// hushed during cutscenes and letterbox.
    pub(super) fn conditions_tick(&mut self) {
        if self.cutscene.on || self.cutscene.letterbox {
            for k in 0..CONDITIONS.len() {
                if let Some(Some(id)) = self.game_sounds.playing[k] {
                    self.audio.simple_volume(id, 0.0, 1.0);
                }
            }
            return;
        }
        let Some(p) = self.fighters.first().filter(|f| f.player) else {
            return;
        };
        let (hp, max, invis) = (
            p.hp as f32,
            p.max_hp.max(1) as f32,
            f32::from(p.inventory.invisibility),
        );
        let fade = |x: f32| 1.0 - (1.0 - CONDITION_MIN_VOLUME) * x * x;
        let boost = self.game_sounds.boosted_health;
        let wanted = [
            (hp > 0.0 && hp < max / 5.0).then(|| fade(hp * 5.0 / max)),
            (hp > max).then(|| {
                fade(1.0 - ((hp - max) / (max * (boost - 1.0)).max(1e-3)).clamp(0.0, 1.0))
            }),
            // (The shield's loop was taken out of the game.)
            None,
            // (Endless invisibility, the cheat's, comes out below silent.)
            (invis > 0.0).then(|| fade(1.0 - invis / MAX_INVISIBILITY).max(0.0)),
            self.timer_critical().then_some(1.0),
        ];
        for (k, want) in wanted.into_iter().enumerate() {
            match (want, self.game_sounds.playing[k]) {
                (Some(v), None) => {
                    let name = self
                        .game_sounds
                        .conditions
                        .get(CONDITIONS[k])
                        .cloned()
                        .unwrap_or_default();
                    let id = (!name.is_empty())
                        .then(|| self.audio.simple_start(&name, v))
                        .flatten();
                    self.game_sounds.playing[k] = Some(id);
                }
                (Some(v), Some(Some(id))) => self.audio.simple_volume(id, v, 1.0),
                (None, Some(id)) => {
                    if let Some(id) = id {
                        self.audio.simple_stop(id);
                    }
                    self.game_sounds.playing[k] = None;
                }
                _ => {}
            }
        }
    }
}
