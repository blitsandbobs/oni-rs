//! The player's options (`Oni_Persistance.c`'s game options; `docs/MENU.md`,
//! *Options*), kept apart from the saved games: `$ONI_SAVE_DIR/settings.json`,
//! else `~/.oni-rs/settings.json`. A missing or unreadable file gives the
//! original's defaults (`ONrPersistance_Initialize`).

use std::path::PathBuf;

/// The options, as the original keeps them.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The overall volume, 0–1 (default 1).
    pub overall_volume: f32,
    /// Dialogue subtitles (the original's default: on only without sound).
    pub subtitles: bool,
    /// "Invert mouse" (default on: the mouse moved down aims down).
    pub invert_mouse: bool,
    /// "easy", "normal" (default) or "hard".
    pub difficulty: String,
    /// The gamma slider, 0–1 (default 0.5: unchanged).
    pub gamma: f32,
    /// Graphics quality 0–2: kept for the settings file, but always high —
    /// the options' slider is frozen there (`docs/MENU.md`).
    pub quality: u8,
    /// The resolution chosen (default 640 × 480; shown, not used: the window
    /// is sized freely).
    pub resolution: (u32, u32),
    /// Not the original's (#58, `docs/PARTICLES.md`, *Lens flares*):
    /// characters hide the lens flares behind them (default off, as
    /// shipped). Only in the file (`"flaresBehindCharacters": true`).
    pub flares_behind_characters: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            overall_volume: 1.0,
            subtitles: false,
            invert_mouse: true,
            difficulty: "normal".into(),
            gamma: 0.5,
            quality: 2,
            resolution: (640, 480),
            flares_behind_characters: false,
        }
    }
}

/// Where the player's files live: `$ONI_SAVE_DIR`, else `~/.oni-rs`.
pub fn save_dir() -> Option<PathBuf> {
    std::env::var_os("ONI_SAVE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".oni-rs")))
}

impl Settings {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "overallVolume": self.overall_volume, "subtitles": self.subtitles,
            "invertMouse": self.invert_mouse, "difficulty": self.difficulty,
            "gamma": self.gamma, "quality": self.quality,
            "resolution": [self.resolution.0, self.resolution.1],
            "flaresBehindCharacters": self.flares_behind_characters,
        })
    }

    fn from_json(v: &serde_json::Value) -> Settings {
        let d = Settings::default();
        let f = |k: &str, x: f32| v[k].as_f64().map_or(x, |y| (y as f32).clamp(0.0, 1.0));
        let b = |k: &str, x: bool| v[k].as_bool().unwrap_or(x);
        let difficulty = v["difficulty"]
            .as_str()
            .filter(|s| ["easy", "normal", "hard"].contains(s))
            .map_or(d.difficulty.clone(), str::to_string);
        let r = &v["resolution"];
        Settings {
            overall_volume: f("overallVolume", d.overall_volume),
            subtitles: b("subtitles", d.subtitles),
            invert_mouse: b("invertMouse", d.invert_mouse),
            difficulty,
            gamma: f("gamma", d.gamma),
            quality: v["quality"].as_u64().map_or(d.quality, |q| q.min(2) as u8),
            resolution: match (r[0].as_u64(), r[1].as_u64()) {
                (Some(w), Some(h)) => (w as u32, h as u32),
                _ => d.resolution,
            },
            flares_behind_characters: b("flaresBehindCharacters", d.flares_behind_characters),
        }
    }

    /// The options kept (the defaults if none).
    pub fn load() -> Settings {
        let doc = save_dir()
            .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        doc.map(|v| Settings::from_json(&v)).unwrap_or_default()
    }

    /// Keeps the options (`ONrPersist`).
    pub fn store(&self) {
        let Some(dir) = save_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = std::fs::write(
            dir.join("settings.json"),
            serde_json::to_string_pretty(&self.to_json()).unwrap_or_default(),
        ) {
            eprintln!("settings: can't keep them ({e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_survive_their_file_and_bad_values_fall_back() {
        let s = Settings {
            overall_volume: 0.25,
            subtitles: true,
            invert_mouse: false,
            difficulty: "hard".into(),
            ..Settings::default()
        };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        let bad =
            serde_json::json!({ "overallVolume": 7.0, "difficulty": "nightmare", "quality": 9 });
        let b = Settings::from_json(&bad);
        assert_eq!(
            (b.overall_volume, b.difficulty.as_str(), b.quality),
            (1.0, "normal", 2)
        );
        assert!(b.invert_mouse, "inverted by default, as shipped");
    }
}
