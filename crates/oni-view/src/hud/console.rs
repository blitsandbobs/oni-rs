//! The data consoles' text (`docs/HUD.md`, *Text consoles*): `text_console
//! name` opens a modal dialog (`ONrInGameUI_TextConsole_Display`,
//! `dialog_text_console`) showing the console's pages one at a time — a
//! page's picture across the top of the text area, then its strings, each in
//! its own font — with Next (Space) until the last, then Close (Esc closes
//! at any time). The game stands still while it is up.

use oni_schema::files;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::text::{self, Align, Fonts};
use super::{rect, PartSpec};
use crate::draw::OverlayDraw;

/// One string of a page, in its font.
#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub size: u16,
    /// 0 plain, 1 bold, 2 italic.
    pub style: u8,
    /// ARGB; none: the colour the drawing gives (the objectives' green or
    /// red).
    pub shade: Option<u32>,
}

/// A page's picture: a texture drawn at its size, centred, or a part
/// specification's top row across the text area.
#[derive(Clone, Debug)]
pub enum Picture {
    Texture(String),
    PartSpec(String),
}

#[derive(Clone, Debug)]
pub struct Page {
    pub picture: Option<Picture>,
    pub text: Vec<Line>,
    pub hint: Vec<Line>,
}

/// The dialog's layout (`common/text_console.json`), 640 × 480 units.
#[derive(Clone, Debug)]
pub struct Dialog {
    size: [f32; 2],
    /// x, y, width, height.
    text: [f32; 4],
    button: [f32; 4],
    button_font: (u16, u8, u32),
    next: String,
    close: String,
}

/// An open console: its name, pages and the page showing.
#[derive(Clone, Debug)]
pub struct Open {
    pub name: String,
    pub pages: Vec<Page>,
    pub page: usize,
}

impl Open {
    /// Space / Next (`ONiTC_HandleCommand`): the next page; past the last,
    /// closed (false).
    pub fn next(&mut self) -> bool {
        self.page += 1;
        self.page < self.pages.len()
    }

    /// Its button: Next while pages follow, else Close (`ONiTC_SetButtons`).
    pub fn last(&self) -> bool {
        self.page + 1 >= self.pages.len()
    }
}

fn shade(v: Option<&str>) -> Option<u32> {
    v.and_then(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
}

fn line(v: &files::TextRun) -> Line {
    Line {
        text: v.text.clone(),
        size: v.font.size as u16,
        style: v.font.style as u8,
        shade: shade(v.font.shade.as_deref()),
    }
}

/// A page from its file form (`text_consoles.json`, `pause_screen.json`).
pub fn page(p: &files::Page) -> Page {
    Page {
        picture: p
            .picture
            .as_ref()
            .and_then(|pic| match (&pic.texture, &pic.part_spec) {
                (Some(t), _) => Some(Picture::Texture(t.clone())),
                (_, Some(s)) => Some(Picture::PartSpec(s.clone())),
                _ => None,
            }),
        text: p.text.iter().map(line).collect(),
        hint: p.hint.iter().map(line).collect(),
    }
}

/// A level's consoles (`levels/NN/text_consoles.json`), by lowercase name.
pub fn load(level_dir: &Path) -> HashMap<String, Vec<Page>> {
    let doc: files::TextConsolesFile = crate::json::read_file(level_dir.join("text_consoles.json"));
    doc.consoles
        .iter()
        .map(|(name, c)| {
            (
                name.to_ascii_lowercase(),
                c.pages.iter().map(page).collect(),
            )
        })
        .collect()
}

/// The dialog's layout (`common/text_console.json`).
pub fn load_dialog(common: &Path) -> Option<Dialog> {
    let doc: files::Dialog = crate::json::read_some(common.join("text_console.json"))?;
    let item = |id: i64| doc.items.iter().find(|i| i.id == id);
    let r = |i: &files::DialogItem| -> [f32; 4] { i.rect.map(|v| v as f32) };
    let (text, next) = (item(110)?, item(102)?);
    // (The Close button: the dialog's cancel item, 2.)
    let close = item(2);
    let f = &next.font;
    Some(Dialog {
        size: doc.size.map(|v| v as f32),
        text: r(text),
        button: r(next),
        button_font: (
            f.size as u16,
            f.style as u8,
            shade(f.shade.as_deref()).unwrap_or(0xFFFF_FFFF),
        ),
        next: next.title.clone(),
        close: close.map_or("Close", |c| c.title.as_str()).to_string(),
    })
}

/// A string's glyph cells from `x`, its line's top at `top`.
pub(super) fn glyphs(font: &text::Font, s: &str, mut x: f32, top: f32) -> Vec<[f32; 4]> {
    let (aw, ah) = (font.atlas_size[0], font.atlas_size[1]);
    let chh = font.cell_height() as f32;
    let mut tris = Vec::new();
    for ch in s.chars() {
        let Some([gx, gy, gw, adv]) = font.glyph(ch) else {
            continue;
        };
        let cw = gw as f32;
        let (u0, v0, u1, v1) = (
            gx as f32 / aw,
            gy as f32 / ah,
            (gx as f32 + cw) / aw,
            (gy as f32 + chh) / ah,
        );
        tris.extend_from_slice(&[
            [x, top, u0, v0],
            [x + cw, top, u1, v0],
            [x + cw, top + chh, u1, v1],
            [x, top, u0, v0],
            [x + cw, top + chh, u1, v1],
            [x, top + chh, u0, v1],
        ]);
        x += adv as f32;
    }
    tris
}

pub(super) fn rgba(shade: u32) -> [f32; 4] {
    [
        ((shade >> 16) & 0xFF) as f32 / 255.0,
        ((shade >> 8) & 0xFF) as f32 / 255.0,
        (shade & 0xFF) as f32 / 255.0,
        1.0,
    ]
}

/// Glyph runs of `lines` laid down an area (x, y, width, height) from `y`
/// within it, each in its font, wrapped to the width; what runs past its
/// foot isn't shown. Returns the height used.
#[allow(clippy::too_many_arguments)]
pub fn lines_draws(
    out: &mut Vec<OverlayDraw>,
    lines: &[Line],
    area: [f32; 4],
    mut y: f32,
    fonts: &Fonts,
    fallback: u32,
) -> f32 {
    let [left, top, tw, th] = area;
    let bottom = top + th;
    for l in lines {
        let style = ["plain", "bold", "italic"][usize::from(l.style.min(2))];
        let Some(font) = fonts.best_styled(l.size, style) else {
            continue;
        };
        let shade = l.shade.unwrap_or(fallback);
        let line_h = font.line_height().max(1);
        let bounds = [
            left as i32,
            (top + y) as i32,
            (left + tw) as i32,
            bottom as i32,
        ];
        let segments = text::format(
            font,
            &fonts.breaking,
            &l.text,
            bounds,
            left as i32,
            (top + y) as i32,
            Align::Left,
            false,
            shade & 0xFF_FFFF,
        );
        let n = segments
            .iter()
            .map(|s| s.top)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            .max(1);
        for s in &segments {
            if s.top as f32 + font.cell_height() as f32 > bottom {
                continue;
            }
            out.push(OverlayDraw {
                texture: Some(font.atlas.clone()),
                tris: glyphs(font, &s.text, s.x as f32, s.top as f32),
                color: rgba(shade),
                additive: false,
            });
        }
        y += (n as i32 * line_h) as f32;
    }
    y
}

/// A page in an area (x, y, width, height; `ONiIGUI_Page_Paint`): its
/// picture first — a part specification's top row across, or a texture at
/// its size, centred — ten above its strings. As an overlay
/// (`_PaintOverlay`, the diary): the strings from the top, then a texture
/// centred in the area over them. `fallback`: the shade of strings with
/// none of their own.
#[allow(clippy::too_many_arguments)]
pub fn page_draws(
    page: &Page,
    area: [f32; 4],
    fonts: &Fonts,
    part: &dyn Fn(&str) -> Option<PartSpec>,
    textures: &Path,
    texture_size: &dyn Fn(&Path) -> Option<[f32; 2]>,
    fallback: u32,
    overlay: bool,
) -> Vec<OverlayDraw> {
    let mut out = Vec::new();
    let [left, top, tw, th] = area;
    let texture = |name: &str| -> Option<(PathBuf, [f32; 2])> {
        let path: PathBuf = textures.join(format!("{name}.png"));
        let size = texture_size(&path)?;
        Some((path, size))
    };
    if overlay {
        lines_draws(&mut out, &page.text, area, 0.0, fonts, fallback);
        if let Some(Picture::Texture(name)) = &page.picture {
            if let Some((path, [pw, ph])) = texture(name) {
                let (x, y) = (
                    left + ((tw - pw) / 2.0).floor(),
                    top + ((th - ph) / 2.0).floor(),
                );
                out.push(rect(Some(path), [x, y, x + pw, y + ph], [1.0; 4]));
            }
        }
        return out;
    }
    let mut y = 0.0;
    match &page.picture {
        Some(Picture::PartSpec(name)) => {
            if let Some(p) = part(name) {
                let ph = p.top_height();
                out.extend(p.draw([left, top], tw, ph));
                y += ph + 10.0;
            }
        }
        Some(Picture::Texture(name)) => {
            if let Some((path, [pw, ph])) = texture(name) {
                let x = left + ((tw - pw) / 2.0).floor();
                out.push(rect(Some(path), [x, top, x + pw, top + ph], [1.0; 4]));
                y += ph + 10.0;
            }
        }
        None => {}
    }
    lines_draws(&mut out, &page.text, area, y, fonts, fallback);
    out
}

/// A button: the in-game UI's button (`ps_oniUI_button_on` while pressed,
/// else `_off`) over `r` (x, y, width, height), its title centred in a font
/// and shade.
#[allow(clippy::too_many_arguments)]
pub fn button_draws(
    out: &mut Vec<OverlayDraw>,
    part: &dyn Fn(&str) -> Option<PartSpec>,
    r: [f32; 4],
    title: &str,
    fonts: &Fonts,
    font: (u16, u8),
    shade: u32,
    on: bool,
) {
    let [bx, by, bw, bh] = r;
    if let Some(p) = part(if on {
        "ps_oniUI_button_on"
    } else {
        "ps_oniUI_button_off"
    }) {
        out.extend(p.draw([bx, by], bw, bh));
    }
    if let Some(f) = fonts.best_styled(
        font.0,
        ["plain", "bold", "italic"][usize::from(font.1.min(2))],
    ) {
        let width: i32 = title.chars().filter_map(|c| f.glyph(c)).map(|g| g[3]).sum();
        let x = (bx + (bw - width as f32) / 2.0).floor();
        let t = (by + (bh - f.cell_height() as f32) / 2.0).floor();
        out.push(OverlayDraw {
            texture: Some(f.atlas.clone()),
            tris: glyphs(f, title, x, t),
            color: rgba(shade),
            additive: false,
        });
    }
}

/// The dialog's button on a `w` × `h` screen: x, y, width, height.
pub fn button_rect(d: &Dialog, w: f32, h: f32) -> [f32; 4] {
    let at = [
        ((w - d.size[0]) * 0.5).floor().max(0.0),
        ((h - d.size[1]) * 0.5).floor().max(0.0),
    ];
    let [bx, by, bw, bh] = d.button;
    [at[0] + bx, at[1] + by, bw, bh]
}

/// The dialog over a `w` × `h` screen, centred (at a whole multiple of its
/// size that fits): the in-game UI's background and outline, the page in
/// the text area, the button.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    open: &Open,
    d: &Dialog,
    fonts: &Fonts,
    part: &dyn Fn(&str) -> Option<PartSpec>,
    textures: &Path,
    texture_size: &dyn Fn(&Path) -> Option<[f32; 2]>,
    w: f32,
    h: f32,
) -> Vec<OverlayDraw> {
    let mut out = Vec::new();
    let at = [
        ((w - d.size[0]) * 0.5).floor().max(0.0),
        ((h - d.size[1]) * 0.5).floor().max(0.0),
    ];
    match part("ps_oniUI_background") {
        Some(p) => out.extend(p.draw(at, d.size[0], d.size[1])),
        None => out.push(rect(
            None,
            [at[0], at[1], at[0] + d.size[0], at[1] + d.size[1]],
            [0.0, 0.0, 0.0, 0.85],
        )),
    }
    if let Some(p) = part("ps_oniUI_outline") {
        out.extend(p.draw(at, d.size[0], d.size[1]));
    }
    let Some(page) = open.pages.get(open.page) else {
        return out;
    };
    let [tx, ty, tw, th] = d.text;
    out.extend(page_draws(
        page,
        [at[0] + tx, at[1] + ty, tw, th],
        fonts,
        part,
        textures,
        texture_size,
        0xFFFF_FFFF,
        false,
    ));
    // The button: Next, or Close on the last page; its title centred.
    let [bx, by, bw, bh] = d.button;
    let (size, style, shade) = d.button_font;
    let title = if open.last() { &d.close } else { &d.next };
    button_draws(
        &mut out,
        part,
        [at[0] + bx, at[1] + by, bw, bh],
        title,
        fonts,
        (size, style),
        shade,
        false,
    );
    out
}
