//! The out-of-game menus (`Oni_OutGameUI.c`, `docs/MENU.md`): level 0's
//! dialogs — the main menu, load game, options, the new-game, quit and
//! change-restart prompts, the level-load progress — every item with its
//! window type, style bits, place and font; and the level descriptors
//! (`ONLD`: number, next, display name) the load-game list names levels by;
//! the out-of-game skin (`psui_oniUI`: the part spec of each part); and the
//! popup menus' items (`pm_difficulty`).
//! Written to `common/menus.json`.

use std::path::Path;

use oni_dat::Dat;
use serde_json::{json, Value};

use crate::text_consoles::{text, Records};

/// The dialogs, by their instance names.
const DIALOGS: [&str; 7] = [
    "dialog_mainmenu",
    "dialog_loadgame",
    "dialog_options",
    "dialog_NewGame",
    "dialog_QuitYesNo",
    "dialog_ChangeRestart",
    "dialog_ll_progress",
];

/// One dialog (`WMtDialogData`): its id, title, style, size and items.
fn dialog(r: &Records<'_>, name: &str) -> Option<Value> {
    let (_, d) = r.get(&json!({ "$ref": name }))?;
    let items: Vec<Value> = d["items"]
        .as_array()?
        .iter()
        .map(|it| {
            json!({
                "id": it["id"], "type": it["windowtype"], "title": text(&it["title"]),
                "flags": it["flags"], "style": it["style"],
                "rect": [it["x"], it["y"], it["width"], it["height"]],
                "font": {
                    "family": it["font_info"]["font_family"]["$ref"],
                    "size": it["font_info"]["font_size"],
                    "style": it["font_info"]["font_style"],
                    "shade": format!("#{:08x}", it["font_info"]["font_shade"].as_u64().unwrap_or(0xFFFF_FFFF)),
                },
            })
        })
        .collect();
    Some(json!({
        "id": d["id"], "title": text(&d["title"]), "flags": d["flags"], "style": d["style"],
        "size": [d["width"], d["height"]], "items": items,
    }))
}

/// Level 0's menus and level names.
pub fn common(dat: &Dat<'_>, registry: &oni_template::Registry) -> Option<Value> {
    let r = Records::new(dat, registry);
    let dialogs: serde_json::Map<String, Value> =
        DIALOGS.iter().filter_map(|&n| dialog(&r, n).map(|d| (n.trim_start_matches("dialog_").to_string(), d))).collect();
    if dialogs.is_empty() {
        return None;
    }
    let mut levels: Vec<Value> = dat
        .instances
        .iter()
        .filter(|i| i.tag.to_string() == "ONLD" && dat.instance_data(i).is_some())
        .filter_map(|i| {
            let name = i.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("ONLD#{}", i.index));
            let (_, l) = r.get(&json!({ "$ref": name }))?;
            Some(json!({ "level": l["level_number"], "next": l["next_level_number"], "name": text(&l["level_name"]) }))
        })
        .collect();
    levels.sort_by_key(|l| l["level"].as_i64().unwrap_or(0));
    // The out-of-game skin (`psui_oniUI`): each part's part spec.
    let skin = r.get(&json!({ "$ref": "psui_oniUI" })).map(|(_, ui)| {
        let m: serde_json::Map<String, Value> = ui
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v["$ref"].as_str().map(|n| (k.clone(), json!(n))))
            .collect();
        Value::Object(m)
    });
    // The popup menus' items (`WMtMenuData`; the options' difficulty).
    let popups: serde_json::Map<String, Value> = ["pm_difficulty"]
        .iter()
        .filter_map(|&n| {
            let (_, m) = r.get(&json!({ "$ref": n }))?;
            let items: Vec<Value> = m["items"].as_array()?.iter().map(|i| json!({ "id": i["id"], "flags": i["flags"], "title": text(&i["title"]) })).collect();
            Some((n.to_string(), json!(items)))
        })
        .collect();
    Some(json!({ "dialogs": dialogs, "levels": levels, "skin": skin.unwrap_or(Value::Null), "popups": popups }))
}

pub fn write(doc: &Value, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(doc)?)
}
