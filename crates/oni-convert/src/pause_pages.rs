//! The pause screen's pages (`ONrPauseScreen_Display`,,
//! *Pause screen*): level 0 holds the dialog (`pause_screen`), the help
//! (`HPge`), item (`IPge`), weapon (`WPge`) and diary (`DPge`) pages and the
//! diary's key icons (`KeyI`); each level its objectives (`OPge`) and its own
//! item pages. Pages are written as the text consoles' are
//! (`text_consoles::page`).

use std::path::Path;

use oni_dat::Dat;
use oni_schema::files;
use serde_json::{json, Value};

use crate::text_consoles::{named_dialog, page, page_array, Font, Records};

fn of_tag<'a>(dat: &'a Dat<'a>, tag: &'a str) -> impl Iterator<Item = String> + 'a {
    dat.instances
        .iter()
        .filter(move |i| i.tag.to_string() == tag && dat.instance_data(i).is_some())
        .map(|i| match &i.name {
            Some(n) if !n.is_empty() => n.clone(),
            _ => format!("{}#{}", i.tag, i.index),
        })
}

/// A file's item pages (`IPge`): powerup type, name, page.
fn items(r: &Records<'_>, dat: &Dat<'_>, base: Font) -> Vec<files::ItemPage> {
    of_tag(dat, "IPge")
        .filter_map(|name| {
            let (_, p) = r.get(&json!({ "$ref": name }))?;
            Some(files::ItemPage {
                kind: p["powerup_type"].as_i64().unwrap_or(0),
                page: page(r, &p["page"], base)?,
                name,
            })
        })
        .collect()
}

/// Level 0's: the dialog and the pages every level shares. Its base font:
/// the dialog's main area's.
pub fn common(
    dat: &Dat<'_>,
    registry: &oni_template::Registry,
) -> Option<(files::PauseScreenFile, Font)> {
    let (dialog, base) = named_dialog(dat, registry, "pause_screen")?;
    let r = Records::new(dat, registry);
    let get = |name: &str| r.get(&json!({ "$ref": name })).map(|(_, v)| v);
    let mut help: Vec<(i64, files::Page)> = of_tag(dat, "HPge")
        .filter_map(|n| {
            let p = get(&n)?;
            Some((
                p["page_number"].as_i64().unwrap_or(0),
                page(&r, &p["page"], base)?,
            ))
        })
        .collect();
    help.sort_by_key(|(n, _)| *n);
    let weapons: Vec<files::WeaponPage> = of_tag(dat, "WPge")
        .filter_map(|n| {
            let p = get(&n)?;
            Some(files::WeaponPage {
                class: p["weaponClass"]["$ref"].as_str().map(str::to_string),
                page: page(&r, &p["page"], base)?,
            })
        })
        .collect();
    let diary: Vec<files::DiaryPage> = of_tag(dat, "DPge")
        .filter_map(|n| {
            let p = get(&n)?;
            let keys: Vec<i64> = p["keys"]
                .as_array()?
                .iter()
                .filter_map(Value::as_i64)
                .take_while(|&k| k != 0)
                .collect();
            Some(files::DiaryPage {
                level: p["level_number"].as_i64().unwrap_or(0),
                page: p["page_number"].as_i64().unwrap_or(0),
                new_move: p["has_new_move"].as_i64().unwrap_or(0) != 0,
                keys,
                content: page(&r, &p["page"], base)?,
            })
        })
        .collect();
    let icons = of_tag(dat, "KeyI").next().and_then(|n| get(&n)).map(|k| {
        let names = [
            "punch", "kick", "forward", "backward", "left", "right", "crouch", "jump", "hold",
            "plus",
        ];
        names
            .iter()
            .filter_map(|&f| {
                k[f]["$ref"]
                    .as_str()
                    .map(|t| (f.to_string(), t.to_string()))
            })
            .collect()
    });
    let doc = files::PauseScreenFile {
        dialog,
        help: help.into_iter().map(|(_, p)| p).collect(),
        items: items(&r, dat, base),
        weapons,
        diary,
        key_icons: icons,
    };
    Some((doc, base))
}

/// A level's: its objectives (drawn green or red over the dialog's font,
/// so their shade is left to the drawing) and its own item pages.
pub fn level(
    dat: &Dat<'_>,
    registry: &oni_template::Registry,
    base: Font,
) -> Option<files::PausePagesFile> {
    let r = Records::new(dat, registry);
    let objectives = of_tag(dat, "OPge")
        .next()
        .and_then(|n| r.get(&json!({ "$ref": n })))
        .map(|(_, o)| {
            page_array(
                &r,
                &o["objectives"],
                Font {
                    shade: None,
                    ..base
                },
            )
        });
    let items = items(&r, dat, base);
    (objectives.is_some() || !items.is_empty()).then(|| files::PausePagesFile {
        objectives: objectives.unwrap_or_default(),
        items,
    })
}

pub fn write(doc: &impl serde::Serialize, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(doc)?)
}

/// The HUD help overlay (`ONtIGUI_HUDHelp`, `IGHH` `hud_help_info`, level 0;
/// `Oni_InGameUI.h`): `left_texture` @28, `right_texture` @32,
/// `left_offset` / `right_offset` (two `i16` each) @36 / @40,
/// `num_left_items` @44, `num_right_items` @48, then the var-array's count
/// @52 and its items @56, each 68 bytes: `text[64]`, `offset` (two `i16`).
/// The first `num_left_items` label the left ring, the rest the right.
pub fn hud_help(dat: &Dat<'_>) -> Option<files::HudHelpFile> {
    let inst = dat
        .instances
        .iter()
        .find(|i| i.tag.to_string() == "IGHH" && dat.instance_data(i).is_some())?;
    let d = dat.instance_data(inst)?;
    let u = |o: usize| Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?));
    let s = |o: usize| Some(i16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?));
    let name = |r: u32| -> Option<String> {
        (r & 1 != 0)
            .then(|| dat.instances.get((r >> 8) as usize))
            .flatten()
            .and_then(|i| i.name.clone())
    };
    let left_items = u(44)? as usize;
    let items = (0..u(52)? as usize)
        .map(|k| {
            let o = 56 + k * 68;
            let text = d.get(o..o + 64)?;
            let end = text.iter().position(|&b| b == 0).unwrap_or(64);
            Some(files::HelpItem {
                text: String::from_utf8_lossy(&text[..end]).into_owned(),
                offset: [s(o + 64)?, s(o + 66)?],
                right: k >= left_items,
            })
        })
        .collect::<Option<_>>()?;
    Some(files::HudHelpFile {
        left: files::HelpRing {
            texture: name(u(28)?),
            offset: [s(36)?, s(38)?],
        },
        right: files::HelpRing {
            texture: name(u(32)?),
            offset: [s(40)?, s(42)?],
        },
        items,
    })
}
