//! Where the game is installed (#103): the platform's own folders, finding
//! a converted tree, and installing one from the player's Anniversary
//! Edition (`oni_convert::source`) — or bringing it up to date from the
//! kept source when the converter has changed (`oni_convert::BUILD`).
//!
//! In the platform's data folder (`data_dir`): `oni/` the converted tree,
//! `source/` the game data and scripts it was made from, `continues.json`
//! the saved game; the options (`settings.json`) in its config folder.

use oni_convert::source::{self, Problem};
use std::path::{Path, PathBuf};

/// The folders by platform, from the environment `var` gives.
fn folders(os: &str, var: &dyn Fn(&str) -> Option<PathBuf>) -> Option<(PathBuf, PathBuf)> {
    let home = var("HOME");
    match os {
        "macos" => {
            let d = home?.join("Library/Application Support/Oni");
            Some((d.clone(), d))
        }
        "windows" => {
            let local = var("LOCALAPPDATA")?;
            let roaming = var("APPDATA").unwrap_or_else(|| local.clone());
            Some((local.join("Oni"), roaming.join("Oni")))
        }
        _ => {
            let data = var("XDG_DATA_HOME")
                .filter(|p| p.is_absolute())
                .or_else(|| home.as_ref().map(|h| h.join(".local/share")))?;
            let config = var("XDG_CONFIG_HOME")
                .filter(|p| p.is_absolute())
                .or_else(|| home.as_ref().map(|h| h.join(".config")))?;
            Some((data.join("oni"), config.join("oni")))
        }
    }
}

fn env_var(k: &str) -> Option<PathBuf> {
    std::env::var_os(k)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The platform's data and config folders for the game.
fn platform() -> Option<(PathBuf, PathBuf)> {
    folders(std::env::consts::OS, &env_var)
}

/// Where the game's data lives (the install, the saved game):
/// `$ONI_SAVE_DIR` if set, else the platform's.
pub fn data_dir() -> Option<PathBuf> {
    env_var("ONI_SAVE_DIR").or_else(|| platform().map(|p| p.0))
}

/// Where the options live: `$ONI_SAVE_DIR` if set, else the platform's
/// config folder.
pub fn config_dir() -> Option<PathBuf> {
    env_var("ONI_SAVE_DIR").or_else(|| platform().map(|p| p.1))
}

/// The installed tree's place.
pub fn installed_root() -> Option<PathBuf> {
    Some(data_dir()?.join("oni"))
}

fn source_dir() -> Option<PathBuf> {
    Some(data_dir()?.join("source"))
}

/// The player's files from before (`~/.oni-rs`), brought over once: each
/// kept where it now belongs unless something is there already.
pub fn migrate_old_files() {
    if env_var("ONI_SAVE_DIR").is_some() {
        return;
    }
    let Some(old) = env_var("HOME").map(|h| h.join(".oni-rs")) else {
        return;
    };
    for (name, dir) in [
        ("settings.json", config_dir()),
        ("continues.json", data_dir()),
    ] {
        let (from, Some(dir)) = (old.join(name), dir) else {
            continue;
        };
        let to = dir.join(name);
        if from.is_file() && !to.exists() {
            let _ = std::fs::create_dir_all(&dir);
            if std::fs::copy(&from, &to).is_ok() {
                eprintln!("oni: {} brought over to {}", from.display(), to.display());
            }
        }
    }
}

/// How a converted tree was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `$ONI_ROOT`.
    Env,
    /// The platform's install.
    Installed,
    /// Beside the program (a portable copy) or a developer's
    /// `generated/oni`.
    Beside,
}

/// A converted tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub root: PathBuf,
    pub kind: Kind,
    /// Made by this converter (else to be brought up to date).
    pub current: bool,
}

fn is_tree(p: &Path) -> bool {
    p.join("common/menus.json").is_file()
}

/// The converted tree to play: `$ONI_ROOT`, the install, one beside the
/// program, or a `generated/oni` above the current folder or the program
/// (a developer's).
pub fn find() -> Option<Found> {
    let found = |root: PathBuf, kind| {
        let current = oni_convert::tree_build(&root).as_deref() == Some(oni_convert::BUILD);
        Found {
            root,
            kind,
            current,
        }
    };
    if let Some(r) = env_var("ONI_ROOT") {
        return is_tree(&r).then(|| found(r, Kind::Env));
    }
    if let Some(r) = installed_root().filter(|r| is_tree(r)) {
        return Some(found(r, Kind::Installed));
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    if let Some(r) = exe.as_ref().map(|e| e.join("oni")).filter(|r| is_tree(r)) {
        return Some(found(r, Kind::Beside));
    }
    let starts = std::env::current_dir().ok().into_iter().chain(exe);
    starts
        .flat_map(|s| {
            s.ancestors()
                .map(|a| a.join("generated/oni"))
                .collect::<Vec<_>>()
        })
        .find(|p| is_tree(p))
        .map(|r| found(r, Kind::Beside))
}

/// Whether the install can be brought up to date by itself (its source
/// kept, its tree from another converter).
pub fn can_update(f: &Found) -> bool {
    f.kind == Kind::Installed
        && !f.current
        && source_dir().is_some_and(|s| s.join("gamedata/level0_Final.dat").is_file())
}

/// A step of installing, and how far the whole has come (0–1).
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub what: String,
    pub done: f32,
}

/// The share of an install spent unpacking the source.
const UNPACK: f32 = 0.3;

/// Installs the game from the player's AE (`path`: its `.7z` or folder):
/// checked, unpacked into the install's source, converted into its tree.
/// `progress` hears each step; a false from it cancels.
pub fn install_from(
    path: &Path,
    progress: &mut dyn FnMut(Step) -> bool,
) -> Result<PathBuf, Problem> {
    let data = data_dir().ok_or_else(|| Problem::Io("no folder to install into".into()))?;
    let src = source::identify(path)?;
    source::check_space(&data)?;
    std::fs::create_dir_all(&data)?;
    let staged = source::stage(&src, &data.join("source"), &mut |f| {
        progress(Step {
            what: "Unpacking the game".into(),
            done: f * UNPACK,
        })
    })?;
    convert(&staged.gamedata, &staged.scripts, UNPACK, progress)
}

/// Converts the kept source again (the converter has changed).
pub fn update(progress: &mut dyn FnMut(Step) -> bool) -> Result<PathBuf, Problem> {
    let src = source_dir().ok_or_else(|| Problem::Io("no install".into()))?;
    convert(&src.join("gamedata"), &src.join("bsl"), 0.0, progress)
}

fn convert(
    gamedata: &Path,
    scripts: &Path,
    from: f32,
    progress: &mut dyn FnMut(Step) -> bool,
) -> Result<PathBuf, Problem> {
    let root = installed_root().ok_or_else(|| Problem::Io("no folder to install into".into()))?;
    oni_convert::install(gamedata, &root, Some(scripts), &mut |p| {
        let what = match &p.stage {
            oni_convert::Stage::Reading | oni_convert::Stage::Indexing => {
                "Reading the game data".to_string()
            }
            oni_convert::Stage::Converting(r) if r == "common" => {
                "Converting the shared assets".into()
            }
            oni_convert::Stage::Converting(r) => format!(
                "Converting level {}",
                r.trim_start_matches("levels/").trim_start_matches('0')
            ),
            oni_convert::Stage::Finishing | oni_convert::Stage::Done => "Finishing".into(),
        };
        progress(Step {
            what,
            done: from + (1.0 - from) * p.done,
        })
    })?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| PathBuf::from(v))
        }
    }

    #[test]
    fn each_platform_has_its_folders() {
        let home = [("HOME", "/Users/k")];
        assert_eq!(
            folders("macos", &env(&home)),
            Some((
                "/Users/k/Library/Application Support/Oni".into(),
                "/Users/k/Library/Application Support/Oni".into()
            ))
        );
        assert_eq!(
            folders("linux", &env(&[("HOME", "/home/k")])),
            Some((
                "/home/k/.local/share/oni".into(),
                "/home/k/.config/oni".into()
            ))
        );
        // (A Flatpak sets these to its own folders.)
        let flatpak = [
            ("HOME", "/home/k"),
            ("XDG_DATA_HOME", "/home/k/.var/app/x/data"),
            ("XDG_CONFIG_HOME", "/home/k/.var/app/x/config"),
        ];
        assert_eq!(
            folders("linux", &env(&flatpak)),
            Some((
                "/home/k/.var/app/x/data/oni".into(),
                "/home/k/.var/app/x/config/oni".into()
            ))
        );
        let win = [
            ("LOCALAPPDATA", r"C:\Users\k\AppData\Local"),
            ("APPDATA", r"C:\Users\k\AppData\Roaming"),
        ];
        let (data, config) = folders("windows", &env(&win)).unwrap();
        assert!(data.starts_with(r"C:\Users\k\AppData\Local") && data.ends_with("Oni"));
        assert!(config.starts_with(r"C:\Users\k\AppData\Roaming"));
        assert_eq!(folders("linux", &env(&[])), None);
    }

    #[test]
    fn a_tree_from_another_converter_is_not_current() {
        let d = std::env::temp_dir().join(format!("oni-install-tree-{}", std::process::id()));
        std::fs::create_dir_all(d.join("common")).unwrap();
        std::fs::write(d.join("common/menus.json"), "{}").unwrap();
        assert!(is_tree(&d));
        std::fs::write(d.join("oni.json"), r#"{"build": "0000"}"#).unwrap();
        assert_ne!(
            oni_convert::tree_build(&d).as_deref(),
            Some(oni_convert::BUILD)
        );
        let now = format!(r#"{{"build": "{}"}}"#, oni_convert::BUILD);
        std::fs::write(d.join("oni.json"), now).unwrap();
        assert_eq!(
            oni_convert::tree_build(&d).as_deref(),
            Some(oni_convert::BUILD)
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
