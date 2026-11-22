//! The game around a level (`Oni.c`'s loop, `Oni_GameState.c`):
//! splash screens, and what follows a level's end —
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
    /// Its music (the win and fail splashes': `main_menu_win`,
    /// `loss_screen_music`), faded in over a second and out over a second as
    /// the splash fades out, then halted.
    pub music: Option<&'static str>,
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
        Splash {
            texture,
            after,
            music: None,
            phase: Phase::In,
            t: 0.0,
        }
    }

    /// Whether it is fading out.
    pub fn fading_out(&self) -> bool {
        self.phase == Phase::Out
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
    (level + 1..100).find(|n| {
        oni_root
            .join(format!("levels/{n:02}/environment.glb"))
            .is_file()
    })
}

/// A level's splash screen texture: the level's own, else the common one.
pub fn splash_texture(oni_root: &Path, level: u16, name: &str) -> Option<PathBuf> {
    [
        oni_root.join(format!("levels/{level:02}/textures/{name}.png")),
        oni_root.join(format!("common/textures/{name}.png")),
    ]
    .into_iter()
    .find(|p| p.is_file())
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
        let guns: Vec<serde_json::Value> = self
            .guns
            .iter()
            .map(|g| {
                g.as_ref()
                    .map_or(serde_json::Value::Null, |(c, a)| serde_json::json!([c, a]))
            })
            .collect();
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
            position: glam::Vec3::new(
                p[0].as_f64()? as f32,
                p[1].as_f64()? as f32,
                p[2].as_f64()? as f32,
            ),
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

/// Where the saved game is kept between runs (`ONrPersist`'s persistent
/// file, less the options — `settings.rs`): `$ONI_SAVE_DIR/continues.json`,
/// else `~/.oni-rs/continues.json`. Its continues by level and save point,
/// what the player has met and the levels unlocked (`unlocked`), the place
/// last played (`place`) and the game's mode (`faithful`).
fn continues_file() -> Option<PathBuf> {
    Some(crate::settings::save_dir()?.join("continues.json"))
}

fn write_continues(path: &std::path::Path, doc: &serde_json::Value) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, serde_json::to_string_pretty(doc).unwrap_or_default()) {
        eprintln!("game: can't keep the saved game ({e})");
    }
}

/// A continue's name in the load-game list (`ONrGameState_SaveContinue`):
/// level 1's first is the warehouse, its others count from the training's
/// end; elsewhere "Save Point N" — each indented under its level (four
/// spaces on level 1, five elsewhere, as shipped).
pub fn continue_name(level: u16, save_point: i64) -> String {
    match (level, save_point) {
        (1, 1) => "Syndicate Warehouse".into(),
        (1, n) => format!("    Save Point {}", n - 1),
        (_, n) => format!("     Save Point {n}"),
    }
}

/// `level`'s continues to offer, by save point, with their names: those
/// kept, and level 1's warehouse always (`ONrPersistance_Initialize` makes
/// it, flagged to start there without restoring anything).
pub fn continues_of(level: u16) -> Vec<(i64, String)> {
    let doc = continues_file()
        .map(|p| read_continues(&p))
        .unwrap_or_else(|| serde_json::json!({}));
    let mut out: Vec<(i64, String)> = doc[format!("{level:02}")]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let n: i64 = k.parse().ok()?;
            Some((
                n,
                v["name"]
                    .as_str()
                    .map_or_else(|| continue_name(level, n), str::to_string),
            ))
        })
        .filter(|(n, _)| (1..10).contains(n))
        .collect();
    if level == 1 && !out.iter().any(|(n, _)| *n == 1) {
        out.push((1, continue_name(1, 1)));
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

/// A level begun (`ONrGameState_LevelBegin`): the place last played set to
/// its start, and the level unlocked (`ONrUnlockLevel`; level 1 always is).
pub fn level_begun(level: u16) {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc["place"] = serde_json::json!({ "level": level, "savePoint": 0 });
    let mut levels: Vec<u64> = doc["unlocked"]["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l.as_u64())
        .collect();
    if level > 1 && !levels.contains(&u64::from(level)) {
        levels.push(u64::from(level));
        levels.sort_unstable();
    }
    doc["unlocked"]["levels"] = serde_json::json!(levels);
    write_continues(&path, &doc);
}

/// Whether `level` is unlocked (`ONrLevelIsUnlocked`): level 1 always.
pub fn level_unlocked(level: u16) -> bool {
    level == 1
        || continues_file()
            .map(|p| read_continues(&p))
            .is_some_and(|d| {
                d["unlocked"]["levels"]
                    .as_array()
                    .is_some_and(|ls| ls.iter().any(|l| l.as_u64() == Some(u64::from(level))))
            })
}

/// The load game list (`OWrLevelList_Initialize`): each level (of the
/// descriptors `levels`, in their order) shipped and reached, then its
/// continues; and the place last played.
pub fn load_rows(
    levels: &[(u16, String)],
    oni_root: &Path,
) -> (Vec<crate::hud::menu::LoadRow>, Option<(u16, i64)>) {
    use crate::hud::menu::LoadRow;
    let mut rows = Vec::new();
    for (level, name) in levels {
        let exists = oni_root
            .join(format!("levels/{level:02}/environment.glb"))
            .is_file();
        if !exists || !level_unlocked(*level) {
            continue;
        }
        rows.push(LoadRow {
            text: name.clone(),
            level: *level,
            save_point: 0,
        });
        for (sp, text) in continues_of(*level) {
            rows.push(LoadRow {
                text,
                level: *level,
                save_point: sp,
            });
        }
    }
    (rows, place())
}

/// The place last played: level and save point.
pub fn place() -> Option<(u16, i64)> {
    let doc = read_continues(&continues_file()?);
    Some((
        doc["place"]["level"].as_u64()? as u16,
        doc["place"]["savePoint"].as_i64().unwrap_or(0),
    ))
}

/// The game's mode: shipped bugs (`--faithful`, chosen at a new game) or not.
pub fn game_faithful() -> bool {
    continues_file()
        .map(|p| read_continues(&p))
        .is_some_and(|d| d["faithful"].as_bool() == Some(true))
}

pub fn set_game_faithful(on: bool) {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc["faithful"] = serde_json::json!(on);
    write_continues(&path, &doc);
}

fn read_continues(path: &std::path::Path) -> serde_json::Value {
    crate::json::read(path).unwrap_or_else(|| serde_json::json!({}))
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
    let mut v = c.to_json();
    v["name"] = serde_json::json!(continue_name(level, c.save_point));
    doc[format!("{level:02}")][c.save_point.to_string()] = v;
    write_continues(&path, &doc);
}

/// What the player has met (the pause screen's item kinds and weapon
/// classes; `ONrPersist_ItemUnlocked`, `_WeaponUnlocked`), kept between runs.
pub fn load_unlocks() -> Option<(u32, Vec<String>)> {
    let doc = read_continues(&continues_file()?);
    let u = doc.get("unlocked")?;
    let weapons = u["weapons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| w.as_str().map(str::to_string))
        .collect();
    Some((u["items"].as_u64().unwrap_or(0) as u32, weapons))
}

/// Keeps what the player has met (`ONrPersist_UnlockItem`, `_UnlockWeapon`).
pub fn store_unlocks(items: u32, weapons: &[String]) {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc["unlocked"]["items"] = serde_json::json!(items);
    doc["unlocked"]["weapons"] = serde_json::json!(weapons);
    write_continues(&path, &doc);
}

/// Whether the game has been won (`ONrPersist_GetWonGame`): the cheat
/// codes work.
pub fn won_game() -> bool {
    continues_file().is_some_and(|p| read_continues(&p)["wonGame"].as_bool() == Some(true))
}

/// The game won (`ONrPersist_MarkWonGame`, after the last level).
pub fn mark_won_game() {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc["wonGame"] = serde_json::json!(true);
    write_continues(&path, &doc);
}

/// The save points a level's scripts keep (`save_game N`), with level 1's
/// warehouse (1, always offered).
pub fn save_points(level: u16, level_dir: &Path) -> Vec<i64> {
    let text = std::fs::read_to_string(level_dir.join("scripts.lua")).unwrap_or_default();
    let mut out: Vec<i64> = text
        .split("C[\"save_game\"](")
        .skip(1)
        .filter_map(|rest| {
            rest.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .filter(|n| (1..10).contains(n))
        .collect();
    if level == 1 {
        out.push(1);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// For testing (`oni --unlock-all`): every level unlocked, and each
/// of its save points offered in the load game list — those not kept yet as
/// bare continues (begun there with nothing restored: the player as the
/// scripts set her up). Continues already kept stay. Returns what it
/// offered, by level.
pub fn unlock_all(oni_root: &Path) -> Vec<(u16, Vec<i64>)> {
    let Some(path) = continues_file() else {
        return Vec::new();
    };
    let mut doc = read_continues(&path);
    let mut levels: Vec<u16> = std::fs::read_dir(oni_root.join("levels"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .filter(|l: &u16| {
            oni_root
                .join(format!("levels/{l:02}/environment.glb"))
                .is_file()
        })
        .collect();
    levels.sort_unstable();
    let mut out = Vec::new();
    for &level in &levels {
        let points = save_points(level, &oni_root.join(format!("levels/{level:02}")));
        for &n in &points {
            let key = format!("{level:02}");
            if doc[&key][n.to_string()].is_null() {
                doc[&key][n.to_string()] = serde_json::json!({ "name": continue_name(level, n) });
            }
        }
        out.push((level, points));
    }
    doc["unlocked"]["levels"] =
        serde_json::json!(levels.iter().filter(|&&l| l > 1).collect::<Vec<_>>());
    write_continues(&path, &doc);
    out
}

/// Whether the player killed Griffin (`ONrDidWeKillGriffen`; level 18
/// decides, kept with the saved game).
pub fn killed_griffin() -> bool {
    continues_file().is_some_and(|p| read_continues(&p)["killedGriffin"].as_bool() == Some(true))
}

/// `killed_griffen` (`ONrWeKilledGriffen`): kept.
pub fn set_killed_griffin(murder: bool) {
    let Some(path) = continues_file() else { return };
    let mut doc = read_continues(&path);
    doc["killedGriffin"] = serde_json::json!(murder);
    write_continues(&path, &doc);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_points_are_read_from_the_scripts() {
        let dir = std::env::temp_dir().join(format!("oni-save-points-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("scripts.lua"),
            "C[\"save_game\"](3, \"autosave\")\nC[\"save_game\"](2)\nC[\"save_game\"](3)",
        )
        .unwrap();
        assert_eq!(save_points(4, &dir), vec![2, 3]);
        assert_eq!(
            save_points(1, &dir),
            vec![1, 2, 3],
            "level 1's warehouse too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn continues_are_named_as_shipped() {
        assert_eq!(continue_name(1, 1), "Syndicate Warehouse");
        assert_eq!(continue_name(1, 3), "    Save Point 2");
        assert_eq!(continue_name(4, 2), "     Save Point 2");
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
        assert!(
            s.tick(10.0, true).is_none(),
            "keys ignored early in the fade out"
        );
        assert_eq!(s.tick(30.0, true), Some(After::Restart));
    }
}
