//! The data consoles' text (`text_console name`, `ONrInGameUI_TextConsole_
//! Display`): each `TxtC` is a page array (`IGPA`) of pages (`IGPG`: a
//! picture — a texture or a part specification — then text and hint string
//! arrays, `IGSA` of `IGSt`), each with font info that stacks over the
//! dialog's text font. Also the dialog itself (`WMDD` `dialog_text_console`).

use std::collections::HashMap;
use std::path::Path;

use oni_dat::{Dat, FourCc};
use serde_json::{json, Value};

/// `IGUIcFontInfoFlag_*`: which parts of a font info apply.
const FAMILY: i64 = 1;
const STYLE: i64 = 2;
const SHADE: i64 = 4;
const SIZE: i64 = 8;

/// A resolved font: size, style (0 plain, 1 bold, 2 italic), shade (ARGB;
/// none: whatever the drawing pushes under it — the objectives' green / red).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    pub size: i64,
    pub style: i64,
    pub shade: Option<u32>,
}

impl Font {
    /// This font with `info` (a decoded `ONtIGUI_FontInfo`) pushed over it
    /// (`ONiFIStack_Push`): only the parts its flags name change. (The family
    /// is always Tahoma in the shipped data.)
    fn push(self, info: &Value) -> Font {
        let flags = info["flags"].as_i64().unwrap_or(0);
        let mut f = self;
        if flags & SIZE != 0 {
            f.size = info["font_size"].as_i64().unwrap_or(f.size);
        }
        if flags & STYLE != 0 {
            f.style = info["font_style"].as_i64().unwrap_or(f.style);
        }
        if flags & SHADE != 0 {
            f.shade = info["font_shade"].as_u64().map(|v| v as u32).or(f.shade);
        }
        let _ = FAMILY;
        f
    }

    fn json(self) -> Value {
        let mut v = json!({ "size": self.size, "style": self.style });
        if let Some(s) = self.shade {
            v["shade"] = json!(format!("#{s:08x}"));
        }
        v
    }
}

/// A file's instances decoded on demand, found by their `$ref` names.
pub(crate) struct Records<'a> {
    dat: &'a Dat<'a>,
    registry: &'a oni_template::Registry,
    by_ref: HashMap<String, usize>,
}

impl<'a> Records<'a> {
    pub(crate) fn new(dat: &'a Dat<'a>, registry: &'a oni_template::Registry) -> Records<'a> {
        let by_ref = dat
            .instances
            .iter()
            .enumerate()
            .map(|(k, i)| {
                let name = match &i.name {
                    Some(n) if !n.is_empty() => n.clone(),
                    _ => format!("{}#{}", i.tag, i.index),
                };
                (name, k)
            })
            .collect();
        Records { dat, registry, by_ref }
    }

    /// The instance a `{"$ref": …}` names: its tag and decoded fields.
    pub(crate) fn get(&self, link: &Value) -> Option<(FourCc, Value)> {
        let inst = &self.dat.instances[*self.by_ref.get(link["$ref"].as_str()?)?];
        let layout = self.registry.get(inst.tag)?;
        let rec = oni_template::decode(layout, self.dat.instance_data(inst)?, self.dat.header.endian).ok()?;
        Some((inst.tag, crate::level_json::record_to_json(&rec, self.dat)))
    }
}

/// A C string from its bytes.
pub(crate) fn text(v: &Value) -> String {
    let bytes: Vec<u8> = v.as_array().into_iter().flatten().map(|b| b.as_i64().unwrap_or(0) as u8).take_while(|&b| b != 0).collect();
    // (Mac Roman in the data; the shipped consoles' text is ASCII.)
    bytes.iter().map(|&b| b as char).collect()
}

/// A string array's strings, each with its font pushed over `font`.
fn strings(r: &Records<'_>, link: &Value, font: Font) -> Vec<Value> {
    let Some((_, sa)) = r.get(link) else { return Vec::new() };
    sa["string"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| r.get(s))
        .map(|(_, s)| json!({ "text": text(&s["string"]), "font": font.push(&s["font_info"]).json() }))
        .collect()
}

/// A page's picture: a texture (drawn at its size, centred) or a part
/// specification (its top row across the area).
fn picture(r: &Records<'_>, link: &Value) -> Option<Value> {
    let (tag, _) = r.get(link)?;
    match &tag.to_string()[..] {
        "TXMP" => Some(json!({ "texture": link["$ref"] })),
        // (By name: `common/part_specs.json` has it.)
        "PSpc" => Some(json!({ "partSpec": link["$ref"] })),
        _ => None,
    }
}

/// A page (`IGPG`): its picture, then its text and hint strings, fonts
/// pushed over `base`.
pub(crate) fn page(r: &Records<'_>, link: &Value, base: Font) -> Option<Value> {
    let (_, page) = r.get(link)?;
    let font = base.push(&page["font_info"]);
    let mut v = json!({
        "text": strings(r, &page["text"], font),
        "hint": strings(r, &page["hint"], font),
    });
    if let Some(pic) = picture(r, &page["pict"]) {
        v["picture"] = pic;
    }
    Some(v)
}

/// A page array's (`IGPA`) pages.
pub(crate) fn page_array(r: &Records<'_>, link: &Value, base: Font) -> Vec<Value> {
    let Some((_, pa)) = r.get(link) else { return Vec::new() };
    pa["pages"].as_array().into_iter().flatten().filter_map(|p| page(r, p, base)).collect()
}

/// The text consoles in `dat` (by name), their fonts resolved over `base`
/// (the dialog's text font).
pub fn consoles(dat: &Dat<'_>, registry: &oni_template::Registry, base: Font) -> serde_json::Map<String, Value> {
    let r = Records::new(dat, registry);
    let mut out = serde_json::Map::new();
    for inst in dat.instances.iter().filter(|i| i.tag.to_string() == "TxtC") {
        let Some(name) = inst.name.as_deref().filter(|n| !n.is_empty()) else { continue };
        let Some((_, txtc)) = r.get(&json!({ "$ref": name })) else { continue };
        out.insert(name.to_string(), json!({ "pages": page_array(&r, &txtc["console_data"], base) }));
    }
    out
}

/// The text console dialog (`dialog_text_console`): its size and controls,
/// and its text area's font (the pages' base), if `dat` has it.
pub fn dialog(dat: &Dat<'_>, registry: &oni_template::Registry) -> Option<(Value, Font)> {
    named_dialog(dat, registry, "dialog_text_console")
}

/// A dialog (`WMDD`) by name: its size and controls, and its main text
/// area's (110) font.
pub(crate) fn named_dialog(dat: &Dat<'_>, registry: &oni_template::Registry, name: &str) -> Option<(Value, Font)> {
    let r = Records::new(dat, registry);
    let (_, d) = r.get(&json!({ "$ref": name }))?;
    let mut base = None;
    let items: Vec<Value> = d["items"]
        .as_array()?
        .iter()
        .map(|it| {
            let f = &it["font_info"];
            let font = Font {
                size: f["font_size"].as_i64().unwrap_or(7),
                style: f["font_style"].as_i64().unwrap_or(0),
                shade: Some(f["font_shade"].as_u64().unwrap_or(0xFFFF_FFFF) as u32),
            };
            if it["id"] == 110 {
                base = Some(font);
            }
            json!({
                "id": it["id"], "type": it["windowtype"], "title": text(&it["title"]),
                "rect": [it["x"], it["y"], it["width"], it["height"]], "font": font.json(),
            })
        })
        .collect();
    Some((json!({ "size": [d["width"], d["height"]], "items": items }), base?))
}

/// Writes `common/text_console.json` (the dialog).
pub fn write_dialog(dialog: &Value, common: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(common)?;
    std::fs::write(common.join("text_console.json"), serde_json::to_string_pretty(dialog)?)
}

/// Writes a level's `text_consoles.json`.
pub fn write(consoles: serde_json::Map<String, Value>, root: &Path) -> std::io::Result<usize> {
    let n = consoles.len();
    if n > 0 {
        std::fs::write(root.join("text_consoles.json"), serde_json::to_string_pretty(&json!({ "consoles": consoles }))?)?;
    }
    Ok(n)
}
