//! The converter as the launcher drives it (#101): progress reported and
//! rising to the end, a cancel that leaves nothing behind, and an install
//! that replaces the old tree only once the new one is whole. On level 1's
//! data alone (quick); skipped without the game data.

use oni_convert::{install, Progress, Stage};
use std::path::{Path, PathBuf};

fn gamedata_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONI_GAMEDATA_DIR") {
        let p = PathBuf::from(p);
        return p.is_dir().then_some(p);
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../original/gamedata");
    p.is_dir().then_some(p)
}

/// A scratch folder with level 1's files only, and a place to install to.
fn level1_only(name: &str) -> Option<(PathBuf, PathBuf)> {
    let data = gamedata_dir()?;
    let root = std::env::temp_dir().join(format!("oni-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let input = root.join("in");
    std::fs::create_dir_all(&input).ok()?;
    for f in ["level1_Final.dat", "level1_Final.raw"] {
        std::fs::copy(data.join(f), input.join(f)).ok()?;
    }
    Some((input, root.join("oni")))
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(dir));
}

#[test]
fn an_install_reports_its_progress_and_replaces_the_old_tree_whole() {
    let Some((input, out)) = level1_only("whole") else {
        eprintln!("SKIP: gamedata dir not found (set ONI_GAMEDATA_DIR)");
        return;
    };
    // An old tree, to be replaced.
    std::fs::create_dir_all(out.join("stale")).unwrap();
    let mut seen: Vec<Progress> = Vec::new();
    let report = install(&input, &out, None, &mut |p| {
        seen.push(p);
        true
    })
    .expect("installed");
    assert!(report.levels.contains_key("levels/01"));
    assert!(
        seen.windows(2).all(|w| w[0].done <= w[1].done),
        "only rising"
    );
    assert_eq!(seen.first().map(|p| &p.stage), Some(&Stage::Reading));
    assert_eq!(
        seen.last(),
        Some(&Progress {
            stage: Stage::Done,
            done: 1.0
        })
    );
    assert!(seen
        .iter()
        .any(|p| p.stage == Stage::Converting("levels/01".into())));
    assert!(
        out.join("levels/01/environment.glb").is_file(),
        "the new tree"
    );
    assert!(!out.join("stale").exists(), "the old tree gone");
    assert!(!out.with_file_name("oni.partial").exists() && !out.with_file_name("oni.old").exists());
    cleanup(&out);
}

#[test]
fn a_cancelled_install_leaves_nothing_behind() {
    let Some((input, out)) = level1_only("cancel") else {
        eprintln!("SKIP: gamedata dir not found (set ONI_GAMEDATA_DIR)");
        return;
    };
    std::fs::create_dir_all(out.join("kept")).unwrap();
    let Err(err) = install(&input, &out, None, &mut |p| {
        !matches!(p.stage, Stage::Converting(_))
    }) else {
        panic!("not cancelled");
    };
    assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
    assert!(out.join("kept").is_dir(), "the old tree as it was");
    assert!(
        !out.with_file_name("oni.partial").exists(),
        "no partial tree"
    );
    cleanup(&out);
}
