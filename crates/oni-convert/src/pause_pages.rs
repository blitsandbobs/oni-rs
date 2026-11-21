//! The pause screen's pages (`ONrPauseScreen_Display`, `docs/HUD.md`,
//! *Pause screen*): level 0 holds the dialog (`pause_screen`), the help
//! (`HPge`), item (`IPge`), weapon (`WPge`) and diary (`DPge`) pages and the
//! diary's key icons (`KeyI`); each level its objectives (`OPge`) and its own
//! item pages. Pages are written as the text consoles' are
//! (`text_consoles::page`).

use std::path::Path;

use oni_dat::Dat;
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
fn items(r: &Records<'_>, dat: &Dat<'_>, base: Font) -> Vec<Value> {
    of_tag(dat, "IPge")
        .filter_map(|name| {
            let (_, p) = r.get(&json!({ "$ref": name }))?;
            Some(json!({ "type": p["powerup_type"], "name": name, "page": page(r, &p["page"], base)? }))
        })
        .collect()
}

/// Level 0's: the dialog and the pages every level shares. Its base font:
/// the dialog's main area's.
pub fn common(dat: &Dat<'_>, registry: &oni_template::Registry) -> Option<(Value, Font)> {
    let (dialog, base) = named_dialog(dat, registry, "pause_screen")?;
    let r = Records::new(dat, registry);
    let get = |name: &str| r.get(&json!({ "$ref": name })).map(|(_, v)| v);
    let mut help: Vec<(i64, Value)> = of_tag(dat, "HPge")
        .filter_map(|n| {
            let p = get(&n)?;
            Some((
                p["page_number"].as_i64().unwrap_or(0),
                page(&r, &p["page"], base)?,
            ))
        })
        .collect();
    help.sort_by_key(|(n, _)| *n);
    let weapons: Vec<Value> = of_tag(dat, "WPge")
        .filter_map(|n| {
            let p = get(&n)?;
            Some(json!({ "class": p["weaponClass"]["$ref"], "page": page(&r, &p["page"], base)? }))
        })
        .collect();
    let diary: Vec<Value> = of_tag(dat, "DPge")
        .filter_map(|n| {
            let p = get(&n)?;
            let keys: Vec<i64> = p["keys"]
                .as_array()?
                .iter()
                .filter_map(Value::as_i64)
                .take_while(|&k| k != 0)
                .collect();
            Some(json!({
                "level": p["level_number"], "page": p["page_number"],
                "newMove": p["has_new_move"].as_i64().unwrap_or(0) != 0,
                "keys": keys, "content": page(&r, &p["page"], base)?,
            }))
        })
        .collect();
    let icons = of_tag(dat, "KeyI").next().and_then(|n| get(&n)).map(|k| {
        let names = [
            "punch", "kick", "forward", "backward", "left", "right", "crouch", "jump", "hold",
            "plus",
        ];
        let m: serde_json::Map<String, Value> = names
            .iter()
            .filter_map(|&f| k[f]["$ref"].as_str().map(|t| (f.to_string(), json!(t))))
            .collect();
        Value::Object(m)
    });
    let doc = json!({
        "dialog": dialog,
        "help": help.into_iter().map(|(_, p)| p).collect::<Vec<_>>(),
        "items": items(&r, dat, base),
        "weapons": weapons,
        "diary": diary,
        "keyIcons": icons.unwrap_or(Value::Null),
    });
    Some((doc, base))
}

/// A level's: its objectives (drawn green or red over the dialog's font,
/// so their shade is left to the drawing) and its own item pages.
pub fn level(dat: &Dat<'_>, registry: &oni_template::Registry, base: Font) -> Option<Value> {
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
    (objectives.is_some() || !items.is_empty())
        .then(|| json!({ "objectives": objectives.unwrap_or_default(), "items": items }))
}

pub fn write(doc: &Value, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(doc)?)
}
