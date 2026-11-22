//! The player's own game (#102): the Anniversary Edition's `.7z`, or an
//! extracted AE folder / AE install, made into the files the converter
//! reads — the game data (`GameDataFolder`: the levels' `.dat` / `.raw`,
//! `outro.bik`) and the original level scripts (the AE's vanilla-script
//! package) — staged in the install so later versions can convert again
//! without asking for them.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The levels the game is made of (`levelN_Final`).
pub const LEVELS: [u32; 15] = [0, 1, 2, 3, 4, 6, 8, 9, 10, 11, 12, 13, 14, 18, 19];
/// The game data's folder in an AE tree.
const GAMEDATA: &str = "GameDataFolder";
/// The file only an AE-built game data folder has.
const AE_MARKER: &str = "GameDataFolder/installed_mods.xml";
/// The original level scripts in an AE tree, a folder per level.
const SCRIPTS: &str = "AEInstaller/packages/05100Vanilla_BSL/bsl/common";

/// About what an install takes: the staged source (~350 MB) and the
/// converted tree (~870 MB), with room to spare.
pub const SPACE_NEEDED: u64 = 1_400_000_000;

/// Where the game comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A `.7z` whose AE tree starts at `prefix` (`"Oni - Anniversary
    /// Edition/"`).
    Archive { path: PathBuf, prefix: String },
    /// An AE tree on disk (holding `GameDataFolder` and `AEInstaller`).
    Folder(PathBuf),
}

/// What the converter reads, staged.
#[derive(Clone, Debug, PartialEq)]
pub struct Staged {
    pub gamedata: PathBuf,
    pub scripts: PathBuf,
}

/// Why a source won't do, in words for the player.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    NotFound(PathBuf),
    /// Not a 7z archive we can read.
    NotAnArchive(String),
    /// No Oni game data in it.
    NotOni,
    /// Oni, but not the Anniversary Edition's.
    NotAe,
    /// Files the game needs, missing.
    Missing(Vec<String>),
    NoSpace {
        need: u64,
        free: u64,
    },
    Io(String),
    Cancelled,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::NotFound(p) => write!(f, "{} isn't there.", p.display()),
            Problem::NotAnArchive(e) => write!(f, "That isn't a 7z archive we can read ({e})."),
            Problem::NotOni => write!(f, "There's no Oni game data in it."),
            Problem::NotAe => write!(
                f,
                "That's Oni, but not the Anniversary Edition: choose the Anniversary Edition's archive or folder."
            ),
            Problem::Missing(m) => {
                let shown: Vec<&str> = m.iter().take(4).map(String::as_str).collect();
                let more = if m.len() > 4 { format!(" and {} more", m.len() - 4) } else { String::new() };
                write!(f, "Some of the game's files are missing: {}{more}.", shown.join(", "))
            }
            Problem::NoSpace { need, free } => write!(
                f,
                "Not enough disk space: the game needs {:.1} GB, {:.1} GB is free.",
                *need as f64 / 1e9,
                *free as f64 / 1e9
            ),
            Problem::Io(e) => write!(f, "Couldn't read or write the files ({e})."),
            Problem::Cancelled => write!(f, "Cancelled."),
        }
    }
}

impl From<std::io::Error> for Problem {
    fn from(e: std::io::Error) -> Problem {
        if e.kind() == std::io::ErrorKind::Interrupted {
            Problem::Cancelled
        } else {
            Problem::Io(e.to_string())
        }
    }
}

/// The game data files the game needs, as named in `GameDataFolder`.
fn required_data() -> Vec<String> {
    LEVELS
        .iter()
        .flat_map(|l| ["dat", "raw"].map(|x| format!("level{l}_Final.{x}")))
        .collect()
}

/// Checks a tree by its file names (relative to its root, `/`-separated):
/// Oni, the Anniversary Edition's, everything there.
fn check_names<'a>(names: impl Iterator<Item = &'a str> + Clone) -> Result<(), Problem> {
    let has = |n: &str| names.clone().any(|m| m == n);
    if !has(&format!("{GAMEDATA}/level0_Final.dat")) {
        return Err(Problem::NotOni);
    }
    if !has(AE_MARKER) {
        return Err(Problem::NotAe);
    }
    let mut missing: Vec<String> = required_data()
        .into_iter()
        .filter(|f| !has(&format!("{GAMEDATA}/{f}")))
        .collect();
    let scripts = format!("{SCRIPTS}/");
    if !names
        .clone()
        .any(|n| n.starts_with(&scripts) && n.to_ascii_lowercase().ends_with(".bsl"))
    {
        missing.push("the original level scripts".into());
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Problem::Missing(missing))
    }
}

/// What `path` is: an AE archive or an AE folder (the AE tree itself, a
/// folder holding it, or its `GameDataFolder`), checked.
pub fn identify(path: &Path) -> Result<Source, Problem> {
    if !path.exists() {
        return Err(Problem::NotFound(path.to_path_buf()));
    }
    if path.is_file() {
        return identify_archive(path);
    }
    let candidates = [
        Some(path.to_path_buf()),
        Some(path.join("Oni - Anniversary Edition")),
        path.parent().map(Path::to_path_buf),
    ];
    let root = candidates
        .into_iter()
        .flatten()
        .find(|r| r.join(GAMEDATA).join("level0_Final.dat").is_file())
        .ok_or(Problem::NotOni)?;
    let names = tree_names(&root)?;
    check_names(names.iter().map(String::as_str))?;
    Ok(Source::Folder(root))
}

fn identify_archive(path: &Path) -> Result<Source, Problem> {
    let reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
        .map_err(|e| Problem::NotAnArchive(e.to_string()))?;
    let names: Vec<&str> = reader.archive().files.iter().map(|e| e.name()).collect();
    let anchor = format!("{GAMEDATA}/level0_Final.dat");
    let prefix = names
        .iter()
        .find_map(|n| n.strip_suffix(&anchor))
        .ok_or(Problem::NotOni)?
        .to_string();
    check_names(names.iter().filter_map(|n| n.strip_prefix(&prefix)))?;
    Ok(Source::Archive {
        path: path.to_path_buf(),
        prefix,
    })
}

/// The files under `root`, relative, `/`-separated (the game data and the
/// scripts' folders only: an install holds much else).
fn tree_names(root: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    for sub in [GAMEDATA, SCRIPTS] {
        let mut stack = vec![root.join(sub)];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    Ok(out)
}

/// Where a tree's file goes in the staged source, if it is one the
/// converter reads: the game data folder's own files, and the scripts.
fn staged_path(rel: &str) -> Option<PathBuf> {
    if let Some(f) = rel.strip_prefix(&format!("{GAMEDATA}/")) {
        return (!f.contains('/')).then(|| Path::new("gamedata").join(f));
    }
    let f = rel.strip_prefix(&format!("{SCRIPTS}/"))?;
    let (level, file) = f.split_once('/')?;
    (!file.contains('/') && file.to_ascii_lowercase().ends_with(".bsl"))
        .then(|| Path::new("bsl").join(level).join(file))
}

/// Whether `dir`'s file system has room for an install.
pub fn check_space(dir: &Path) -> Result<(), Problem> {
    let probe = dir.ancestors().find(|a| a.exists()).unwrap_or(dir);
    match fs4::available_space(probe) {
        Ok(free) if free < SPACE_NEEDED => Err(Problem::NoSpace {
            need: SPACE_NEEDED,
            free,
        }),
        _ => Ok(()),
    }
}

/// Copies what the converter reads from `source` into `dest` (as a whole:
/// into `<dest>.partial` first), telling `progress` the fraction done; a
/// false from it cancels.
pub fn stage(
    source: &Source,
    dest: &Path,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<Staged, Problem> {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    let partial = dest.with_file_name(name);
    let _ = std::fs::remove_dir_all(&partial);
    let result = match source {
        Source::Archive { path, prefix } => stage_archive(path, prefix, &partial, progress),
        Source::Folder(root) => stage_folder(root, &partial, progress),
    };
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&partial, dest)?;
    progress(1.0);
    Ok(Staged {
        gamedata: dest.join("gamedata"),
        scripts: dest.join("bsl"),
    })
}

fn write_file(to: &Path, from: &mut dyn Read) -> std::io::Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(to)?);
    std::io::copy(from, &mut f)?;
    f.flush()
}

fn stage_archive(
    path: &Path,
    prefix: &str,
    dest: &Path,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<(), Problem> {
    let mut reader = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())
        .map_err(|e| Problem::NotAnArchive(e.to_string()))?;
    // (Solid: everything before a wanted file is decoded anyway, so the
    // whole archive's size is the measure.)
    let total: u64 = reader
        .archive()
        .files
        .iter()
        .map(|e| e.size())
        .sum::<u64>()
        .max(1);
    let mut done = 0u64;
    let mut cancelled = false;
    let mut failed: Option<std::io::Error> = None;
    let result = reader.for_each_entries(|entry, data| {
        let wanted = entry
            .name()
            .strip_prefix(prefix)
            .filter(|_| !entry.is_directory())
            .and_then(staged_path);
        if let Some(rel) = wanted {
            if let Err(e) = write_file(&dest.join(rel), data) {
                failed = Some(e);
                return Ok(false);
            }
        } else {
            std::io::copy(data, &mut std::io::sink())?;
        }
        done += entry.size();
        if !progress(done as f32 / total as f32 * 0.99) {
            cancelled = true;
            return Ok(false);
        }
        Ok(true)
    });
    if cancelled {
        return Err(Problem::Cancelled);
    }
    if let Some(e) = failed {
        return Err(e.into());
    }
    result.map_err(|e| Problem::Io(e.to_string()))?;
    // (Cancelling skips only the rest of a block: make sure it all came.)
    let names = tree_names_staged(dest)?;
    check_staged(&names)
}

fn stage_folder(
    root: &Path,
    dest: &Path,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<(), Problem> {
    let files: Vec<(PathBuf, PathBuf, u64)> = tree_names(root)?
        .into_iter()
        .filter_map(|rel| {
            let to = staged_path(&rel)?;
            let from = root.join(&rel);
            let size = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
            Some((from, dest.join(to), size))
        })
        .collect();
    let total = files.iter().map(|f| f.2).sum::<u64>().max(1);
    let mut done = 0;
    for (from, to, size) in files {
        write_file(&to, &mut std::fs::File::open(&from)?)?;
        done += size;
        if !progress(done as f32 / total as f32 * 0.99) {
            return Err(Problem::Cancelled);
        }
    }
    Ok(())
}

/// The staged files' names, as an AE tree would have them.
fn tree_names_staged(dest: &Path) -> Result<Vec<String>, Problem> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dest.join("gamedata")) {
        for e in entries.flatten() {
            out.push(format!("{GAMEDATA}/{}", e.file_name().to_string_lossy()));
        }
    }
    if let Ok(levels) = std::fs::read_dir(dest.join("bsl")) {
        for l in levels.flatten() {
            if let Ok(files) = std::fs::read_dir(l.path()) {
                for f in files.flatten() {
                    out.push(format!(
                        "{SCRIPTS}/{}/{}",
                        l.file_name().to_string_lossy(),
                        f.file_name().to_string_lossy()
                    ));
                }
            }
        }
    }
    Ok(out)
}

fn check_staged(names: &[String]) -> Result<(), Problem> {
    check_names(names.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny AE tree (the files' contents don't matter here) in `root`,
    /// less the files named in `without`.
    fn fake_ae(root: &Path, without: &[&str]) {
        let mut files: Vec<String> = required_data()
            .into_iter()
            .map(|f| format!("{GAMEDATA}/{f}"))
            .collect();
        files.push(AE_MARKER.into());
        files.push(format!("{GAMEDATA}/outro.bik"));
        files.push(format!("{GAMEDATA}/IGMD/x/y.bsl"));
        files.push(format!("{SCRIPTS}/EnvWarehouse/warehouse_main.bsl"));
        files.push("AEInstaller/vanilla/level0_Final.dat".into());
        for f in files
            .iter()
            .filter(|f| !without.iter().any(|w| f.ends_with(w)))
        {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f.as_bytes()).unwrap();
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oni-source-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn staged_ok(staged: &Staged) {
        assert!(staged.gamedata.join("level19_Final.raw").is_file());
        assert!(staged.gamedata.join("outro.bik").is_file());
        assert!(
            !staged.gamedata.join("IGMD").exists(),
            "only the folder's own files"
        );
        assert!(staged
            .scripts
            .join("EnvWarehouse/warehouse_main.bsl")
            .is_file());
    }

    #[test]
    fn an_ae_folder_is_found_from_above_or_below_and_staged() {
        let d = scratch("folder");
        let ae = d.join("Oni - Anniversary Edition");
        fake_ae(&ae, &[]);
        for pick in [d.clone(), ae.clone(), ae.join(GAMEDATA)] {
            assert_eq!(identify(&pick), Ok(Source::Folder(ae.clone())));
        }
        let mut last = 0.0;
        let staged = stage(&Source::Folder(ae), &d.join("source"), &mut |p| {
            assert!(p >= last);
            last = p;
            true
        })
        .unwrap();
        assert_eq!(last, 1.0);
        staged_ok(&staged);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_ae_archive_is_read_and_staged() {
        let d = scratch("archive");
        let tree = d.join("tree");
        fake_ae(&tree.join("Oni - Anniversary Edition"), &[]);
        let archive = d.join("ae.7z");
        sevenz_rust2::compress_to_path(&tree, &archive).unwrap();
        let source = identify(&archive).unwrap();
        assert_eq!(
            source,
            Source::Archive {
                path: archive.clone(),
                prefix: "Oni - Anniversary Edition/".into()
            }
        );
        let staged = stage(&source, &d.join("source"), &mut |_| true).unwrap();
        staged_ok(&staged);
        // Cancelled: nothing left behind.
        let r = stage(&source, &d.join("again"), &mut |_| false);
        assert_eq!(r, Err(Problem::Cancelled));
        assert!(!d.join("again").exists() && !d.join("again.partial").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn what_isnt_the_ae_is_told_apart() {
        let d = scratch("wrong");
        fake_ae(&d.join("a"), &["installed_mods.xml"]);
        assert_eq!(identify(&d.join("a")), Err(Problem::NotAe), "vanilla data");
        fake_ae(&d.join("b"), &["level4_Final.raw", "warehouse_main.bsl"]);
        assert_eq!(
            identify(&d.join("b")),
            Err(Problem::Missing(vec![
                "level4_Final.raw".into(),
                "the original level scripts".into()
            ]))
        );
        std::fs::create_dir_all(d.join("c")).unwrap();
        assert_eq!(identify(&d.join("c")), Err(Problem::NotOni));
        std::fs::write(d.join("x.7z"), b"not an archive").unwrap();
        assert!(matches!(
            identify(&d.join("x.7z")),
            Err(Problem::NotAnArchive(_))
        ));
        assert!(matches!(
            identify(&d.join("nope")),
            Err(Problem::NotFound(_))
        ));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The real archive (slow: it decodes all of it): `cargo test -p
    /// oni-convert real_ae_archive -- --ignored`.
    #[test]
    #[ignore]
    fn real_ae_archive_stages_what_the_converter_reads() {
        let archive = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../original/Oni - Anniversary Edition.7z");
        if !archive.is_file() {
            eprintln!("SKIP: no AE archive");
            return;
        }
        let d = scratch("real");
        let staged = stage(&identify(&archive).unwrap(), &d.join("source"), &mut |_| {
            true
        })
        .unwrap();
        let n = std::fs::read_dir(&staged.gamedata).unwrap().count();
        let levels = std::fs::read_dir(&staged.scripts).unwrap().count();
        println!("staged {n} game data files, {levels} level script folders");
        assert!(n >= 31 && levels >= 14);
        if let Ok(keep) = std::env::var("ONI_KEEP_STAGED") {
            let _ = std::fs::rename(d.join("source"), keep);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
