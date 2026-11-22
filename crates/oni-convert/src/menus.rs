//! The out-of-game menus (`Oni_OutGameUI.c`): level 0's
//! dialogs — the main menu, load game, options, the new-game, quit and
//! change-restart prompts, the level-load progress — every item with its
//! window type, style bits, place and font; and the level descriptors
//! (`ONLD`: number, next, display name) the load-game list names levels by;
//! the out-of-game skin (`psui_oniUI`: the part spec of each part); and the
//! popup menus' items (`pm_difficulty`).
//! Written to `common/menus.json`.

use std::path::Path;

use oni_dat::Dat;
use std::collections::BTreeMap;

use oni_schema::files;
use serde_json::{json, Value};

use crate::text_consoles::{text, Records};

/// The dialogs, by their instance names.
const DIALOGS: [&str; 8] = [
    "dialog_mainmenu",
    "dialog_loadgame",
    "dialog_options",
    "dialog_NewGame",
    "dialog_QuitYesNo",
    "dialog_ChangeRestart",
    "dialog_ll_progress",
    "dialog_Progress",
];

/// One dialog (`WMtDialogData`): its id, title, style, size and items.
fn dialog(r: &Records<'_>, name: &str) -> Option<files::MenuDialog> {
    let (_, d) = r.get(&json!({ "$ref": name }))?;
    let int = |v: &Value| v.as_i64().unwrap_or(0);
    let items = d["items"]
        .as_array()?
        .iter()
        .map(|it| files::MenuItem {
            id: int(&it["id"]),
            kind: int(&it["windowtype"]),
            title: text(&it["title"]),
            flags: int(&it["flags"]),
            style: int(&it["style"]),
            rect: [
                int(&it["x"]),
                int(&it["y"]),
                int(&it["width"]),
                int(&it["height"]),
            ],
            font: files::MenuFont {
                family: it["font_info"]["font_family"]["$ref"]
                    .as_str()
                    .map(str::to_string),
                size: int(&it["font_info"]["font_size"]),
                style: int(&it["font_info"]["font_style"]),
                shade: format!(
                    "#{:08x}",
                    it["font_info"]["font_shade"]
                        .as_u64()
                        .unwrap_or(0xFFFF_FFFF)
                ),
            },
        })
        .collect();
    Some(files::MenuDialog {
        id: int(&d["id"]),
        title: text(&d["title"]),
        flags: int(&d["flags"]),
        style: int(&d["style"]),
        size: [int(&d["width"]), int(&d["height"])],
        items,
    })
}

/// Level 0's menus and level names.
pub fn common(dat: &Dat<'_>, registry: &oni_template::Registry) -> Option<files::MenusFile> {
    let r = Records::new(dat, registry);
    let int = |v: &Value| v.as_i64().unwrap_or(0);
    let dialogs: BTreeMap<String, files::MenuDialog> = DIALOGS
        .iter()
        .filter_map(|&n| dialog(&r, n).map(|d| (n.trim_start_matches("dialog_").to_string(), d)))
        .collect();
    if dialogs.is_empty() {
        return None;
    }
    let mut levels: Vec<files::MenuLevel> = dat
        .instances
        .iter()
        .filter(|i| i.tag.to_string() == "ONLD" && dat.instance_data(i).is_some())
        .filter_map(|i| {
            let name = i
                .name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("ONLD#{}", i.index));
            let (_, l) = r.get(&json!({ "$ref": name }))?;
            Some(files::MenuLevel {
                level: int(&l["level_number"]),
                next: int(&l["next_level_number"]),
                name: text(&l["level_name"]),
            })
        })
        .collect();
    levels.sort_by_key(|l| l.level);
    // The out-of-game skin (`psui_oniUI`): each part's part spec.
    let skin = r.get(&json!({ "$ref": "psui_oniUI" })).map(|(_, ui)| {
        ui.as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v["$ref"].as_str().map(|n| (k.clone(), n.to_string())))
            .collect()
    });
    // The popup menus' items (`WMtMenuData`; the options' difficulty).
    let popups = ["pm_difficulty"]
        .iter()
        .filter_map(|&n| {
            let (_, m) = r.get(&json!({ "$ref": n }))?;
            let items = m["items"]
                .as_array()?
                .iter()
                .map(|i| files::PopupItem {
                    id: int(&i["id"]),
                    flags: int(&i["flags"]),
                    title: text(&i["title"]),
                })
                .collect();
            Some((n.to_string(), items))
        })
        .collect();
    Some(files::MenusFile {
        dialogs,
        levels,
        skin,
        popups,
    })
}

pub fn write(doc: &files::MenusFile, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(doc)?)
}
