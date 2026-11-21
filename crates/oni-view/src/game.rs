//! The game around a level (`Oni.c`'s loop, `Oni_GameState.c`; behaviour per
//! `docs/GAME_FLOW.md`): splash screens, and what follows a level's end —
//! a loss restarts it from its save point, a win goes on to the next level.

use std::path::{Path, PathBuf};

/// What happens once a splash screen is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// Back to the level (a script's splash).
    Resume,
    /// The level again, from its save point (a loss).
    Restart,
    /// On to this level (a win).
    Next(u16),
    /// The game is over (the last level won).
    End,
}

/// A splash screen up (`ONrGameState_SplashScreen`): the game stands still
/// while it fades in from black (2 s), holds (up to 10 s), holds again (up
/// to 10 s more), and fades out (2 s). A key moves it on: during the first
/// hold straight to the fade out, during the second to the fade out, and
/// half a second into the fade out to its end.
#[derive(Clone, Debug)]
pub struct Splash {
    pub texture: PathBuf,
    pub after: After,
    phase: Phase,
    /// Ticks into the phase.
    t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    In,
    Hold,
    Out,
    Done,
}

/// Its fade (ticks), each hold's timeout, and how long a key is ignored at
/// the start of the fade out.
const FADE: f32 = 120.0;
const TIMEOUT: f32 = 600.0;
const HASTY: f32 = 30.0;

impl Splash {
    pub fn new(texture: PathBuf, after: After) -> Splash {
        Splash { texture, after, phase: Phase::In, t: 0.0 }
    }

    /// Moves it on `ticks` (real time, in sixtieths), with or without a key
    /// pressed; returns what follows once it is over.
    pub fn tick(&mut self, ticks: f32, key: bool) -> Option<After> {
        self.t += ticks;
        match self.phase {
            Phase::In | Phase::Hold if key => self.go(Phase::Out),
            Phase::In if self.t >= TIMEOUT => self.go(Phase::Hold),
            Phase::Hold if self.t >= TIMEOUT => self.go(Phase::Out),
            Phase::Out if self.t >= FADE || (key && self.t > HASTY) => self.go(Phase::Done),
            _ => {}
        }
        (self.phase == Phase::Done).then_some(self.after)
    }

    fn go(&mut self, phase: Phase) {
        self.phase = phase;
        self.t = 0.0;
    }

    /// How black it is now (0 clear, 1 black): fading in, `(1 − t/2 s)^1.9`;
    /// fading out, `(t/2 s)^1.9` (`get_cosmetic_ramp`).
    pub fn blanket(&self) -> f32 {
        match self.phase {
            Phase::In => (1.0 - self.t / FADE).max(0.0).powf(1.9),
            Phase::Hold => 0.0,
            Phase::Out => (self.t / FADE).min(1.0).powf(1.9),
            Phase::Done => 1.0,
        }
    }
}

/// The level after `level` (`ONrLevel_GetNextLevel`): the lowest numbered
/// one above it that exists (`levels/NN/`), if any.
pub fn next_level(oni_root: &Path, level: u16) -> Option<u16> {
    (level + 1..100).find(|n| oni_root.join(format!("levels/{n:02}/environment.glb")).is_file())
}

/// A level's splash screen texture: the level's own, else the common one.
pub fn splash_texture(oni_root: &Path, level: u16, name: &str) -> Option<PathBuf> {
    [oni_root.join(format!("levels/{level:02}/textures/{name}.png")), oni_root.join(format!("common/textures/{name}.png"))].into_iter().find(|p| p.is_file())
}

/// What a save point keeps of the player (`ONtContinue`), so a restart
/// begins there: the save point, health, place, facing, inventory and guns
/// (by class, with their rounds: in hand, holstered, put away by a
/// cutscene).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Continue {
    pub save_point: i64,
    pub hp: i32,
    pub max_hp: i32,
    pub position: glam::Vec3,
    pub facing: f32,
    pub ammo: u16,
    pub cells: u16,
    pub hypos: u16,
    pub shield: u16,
    pub invisibility: u16,
    pub lsi: bool,
    pub guns: [Option<(String, u16)>; 3],
}

impl Continue {
    fn to_json(&self) -> serde_json::Value {
        let guns: Vec<serde_json::Value> = self.guns.iter().map(|g| g.as_ref().map_or(serde_json::Value::Null, |(c, a)| serde_json::json!([c, a]))).collect();
        serde_json::json!({
            "savePoint": self.save_point, "hp": self.hp, "maxHp": self.max_hp,
            "position": [self.position.x, self.position.y, self.position.z], "facing": self.facing,
            "ammo": self.ammo, "cells": self.cells, "hypos": self.hypos, "shield": self.shield,
            "invisibility": self.invisibility, "lsi": self.lsi, "guns": guns,
        })
    }

    fn from_json(v: &serde_json::Value) -> Option<Continue> {
        let f = |k: &str| v[k].as_f64().map(|x| x as f32);
        let u = |k: &str| v[k].as_u64().unwrap_or(0) as u16;
        let p = &v["position"];
        let gun = |i: usize| {
            let g = &v["guns"][i];
            Some((g[0].as_str()?.to_string(), g[1].as_u64()? as u16))
        };
        Some(Continue {
            save_point: v["savePoint"].as_i64()?,
            hp: v["hp"].as_i64()? as i32,
            max_hp: v["maxHp"].as_i64()? as i32,
            position: glam::Vec3::new(p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32),
            facing: f("facing")?,
            ammo: u("ammo"),
            cells: u("cells"),
            hypos: u("hypos"),
            shield: u("shield"),
            invisibility: u("invisibility"),
            lsi: v["lsi"].as_bool().unwrap_or(false),
            guns: [gun(0), gun(1), gun(2)],
        })
    }
}

/// Where continues are kept between runs (`ONrPersist_SetContinue`'s
/// persistent file): `$ONI_SAVE_DIR/continues.json`, else
/// `~/.oni-rs/continues.json`.
fn continues_file() -> Option<PathBuf> {
    let dir = std::env::var_os("ONI_SAVE_DIR").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".oni-rs")))?;
    Some(dir.join("continues.json"))
}

fn read_continues(path: &std::path::Path) -> serde_json::Value {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}))
}

/// The continue kept for `level`'s save point `n`, if one was made.
pub fn load_continue(level: u16, n: i64) -> Option<Continue> {
    let doc = read_continues(&continues_file()?);
    Continue::from_json(&doc[format!("{level:02}")][n.to_string()])
}

/// Keeps `c` as `level`'s continue for its save point (as the original
/// writes each to its persistent file, for a later load).
pub fn store_continue(level: u16, c: &Continue) {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc[format!("{level:02}")][c.save_point.to_string()] = c.to_json();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap_or_default()) {
        eprintln!("game: can't keep the continue ({e})");
    }
}

/// What the player has met (the pause screen's item kinds and weapon
/// classes; `ONrPersist_ItemUnlocked`, `_WeaponUnlocked`), kept between runs.
pub fn load_unlocks() -> Option<(u32, Vec<String>)> {
    let doc = read_continues(&continues_file()?);
    let u = doc.get("unlocked")?;
    let weapons = u["weapons"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect();
    Some((u["items"].as_u64().unwrap_or(0) as u32, weapons))
}

/// Keeps what the player has met (`ONrPersist_UnlockItem`, `_UnlockWeapon`).
pub fn store_unlocks(items: u32, weapons: &[String]) {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc["unlocked"] = serde_json::json!({ "items": items, "weapons": weapons });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_continue_survives_its_file() {
        let c = Continue {
            save_point: 3,
            hp: 150,
            max_hp: 200,
            position: glam::Vec3::new(1.0, 2.0, 3.0),
            facing: 0.5,
            ammo: 4,
            guns: [Some(("w1_tap".into(), 12)), None, None],
            ..Continue::default()
        };
        assert_eq!(Continue::from_json(&c.to_json()), Some(c));
    }

    #[test]
    fn a_splash_fades_in_holds_and_fades_out() {
        let mut s = Splash::new(PathBuf::new(), After::Resume);
        assert!((s.blanket() - 1.0).abs() < 1e-6, "black at first");
        s.tick(60.0, false);
        assert!(s.blanket() > 0.0 && s.blanket() < 1.0);
        s.tick(60.0, false);
        assert_eq!(s.blanket(), 0.0, "clear after 2 s");
        // Untouched: 10 s, 10 s more, then 2 s of fading out.
        let mut ticks: f32 = 120.0;
        let mut done = None;
        while done.is_none() {
            done = s.tick(1.0, false);
            ticks += 1.0;
        }
        assert_eq!(done, Some(After::Resume));
        assert!((ticks - (600.0 + 600.0 + 120.0)).abs() < 2.0, "{ticks}");
    }

    #[test]
    fn keys_hurry_it() {
        let mut s = Splash::new(PathBuf::new(), After::Restart);
        s.tick(10.0, true);
        assert!(s.tick(10.0, true).is_none(), "keys ignored early in the fade out");
        assert_eq!(s.tick(30.0, true), Some(After::Restart));
    }
}
