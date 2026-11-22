//! The out-of-game menus (`Oni_OutGameUI.c`; `docs/MENU.md`): the main menu
//! and its dialogs — new game, load game, options, quit — built from level
//! 0's dialog data (`common/menus.json`) and drawn in its skin
//! (`psui_oniUI`), with the window manager's controls as the original draws
//! and works them (`BFW_WindowManager`: buttons, text, pictures, boxes,
//! checkboxes, sliders, popup menus, list boxes with their scroll bars).
//!
//! A [`Window`] is one dialog up: its controls, what has the focus, what the
//! mouse is doing; it turns mouse and keys into [`Command`]s as the window
//! manager does (`WMcMessage_Command`). A [`MainMenu`] is the stack of
//! dialogs, and does what the original's callbacks do with the commands —
//! the [`Action`]s the game carries out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::console::{glyphs, rgba};
use super::text::{self, Align, Fonts};
use super::{rect, PartSpec};
use crate::draw::OverlayDraw;
use crate::settings::Settings;

// --- window types and styles (`BFW_WindowManager_Public.h`, `WM_*.h`) -----

const BOX: u16 = 3;
const BUTTON: u16 = 4;
const CHECKBOX: u16 = 5;
const LISTBOX: u16 = 8;
const PICTURE: u16 = 11;
const POPUP: u16 = 12;
const PROGRESSBAR: u16 = 13;
const SLIDER: u16 = 17;
const TEXT: u16 = 20;

const FLAG_VISIBLE: u32 = 0x1;
const FLAG_DISABLED: u32 = 0x2;
const FLAG_MOUSE_TRANSPARENT: u32 = 0x4;

const BUTTON_DEFAULT: u32 = 0x10 << 16;
const BOX_TITLE: u32 = 0x4 << 16;
const CHECKBOX_TITLE: u32 = 0x1 << 16;
const LIST_SCROLLBAR: u32 = 0x1 << 16;
const PICTURE_SCALE: u32 = 0x1 << 16;
const POPUP_RUNTIME: u32 = 0x2 << 16;
const TEXT_HCENTER: u32 = 0x2 << 16;
const TEXT_HRIGHT: u32 = 0x4 << 16;
const TEXT_VCENTER: u32 = 0x10 << 16;

/// The dialogs' OK and Cancel items (`WMcDialogItem_OK`, `_Cancel`).
pub const OK: i64 = 1;
pub const CANCEL: i64 = 2;
/// `IMcShade_Gray50`: a disabled control's text.
const GRAY50: u32 = 0xFF80_8080;
/// `WMcCheckBox_Buffer`; `LBcDefaultTextOffset_X`; `WMcMenu_Buffer_*`.
const CHECKBOX_GAP: f32 = 2.0;
const LIST_TEXT_X: f32 = 3.0;
const MENU_LEFT: f32 = 12.0;
const MENU_RIGHT: f32 = 10.0;
/// Two clicks this close make a double click (the platform's).
const DOUBLE_CLICK: f32 = 0.5;
/// A slider's timer (`WMcSlider_TimerFrequency`: 10 sixtieths): held down
/// on its track, it pages again each time it fires.
const SLIDER_REPEAT: f32 = 10.0 / 60.0;

// --- the data ------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontDef {
    pub size: u16,
    /// 0 plain, 1 bold, 2 italic.
    pub style: u8,
    /// ARGB.
    pub shade: u32,
}

/// A dialog item (`WMtDialogItemData`).
#[derive(Clone, Debug, Default)]
pub struct ItemDef {
    pub id: i64,
    pub kind: u16,
    pub title: String,
    pub flags: u32,
    pub style: u32,
    /// x, y, width, height, within the dialog.
    pub rect: [f32; 4],
    pub font: FontDef,
}

/// A dialog (`WMtDialogData`): its size and items, front to back.
#[derive(Clone, Debug, Default)]
pub struct DialogDef {
    pub size: [f32; 2],
    pub items: Vec<ItemDef>,
}

/// What the dialogs are drawn with: the fonts, the part specifications and
/// the skin naming which part draws what, the textures.
#[derive(Clone, Debug, Default)]
pub struct Look {
    fonts: Fonts,
    specs: oni_schema::files::PartSpecsFile,
    skin: HashMap<String, String>,
    textures: PathBuf,
    sizes: std::cell::RefCell<HashMap<PathBuf, Option<[f32; 2]>>>,
}

impl Look {
    /// A skin part (`button_off`, `outline`, …) by its name in the skin.
    fn part(&self, key: &str) -> Option<PartSpec> {
        let name = self.skin.get(key)?;
        PartSpec::from_file(self.specs.part_specs.get(name), &self.textures)
    }

    fn font(&self, f: FontDef) -> Option<&text::Font> {
        self.fonts.best_styled(
            f.size,
            ["plain", "bold", "italic"][usize::from(f.style.min(2))],
        )
    }

    /// A line's height in a font (14 without fonts).
    fn line_height(&self, f: FontDef) -> f32 {
        self.font(f).map_or(14.0, |f| f.line_height().max(1) as f32)
    }

    fn texture(&self, name: &str) -> (PathBuf, Option<[f32; 2]>) {
        let path = self.textures.join(format!("{name}.png"));
        let size = *self
            .sizes
            .borrow_mut()
            .entry(path.clone())
            .or_insert_with(|| {
                let f = std::fs::File::open(&path).ok()?;
                let r = png::Decoder::new(std::io::BufReader::new(f))
                    .read_info()
                    .ok()?;
                Some([r.info().width as f32, r.info().height as f32])
            });
        (path, size)
    }

    /// How wide a string is in a font.
    fn width(&self, f: FontDef, s: &str) -> f32 {
        self.font(f).map_or(0.0, |font| {
            s.chars()
                .filter_map(|c| font.glyph(c))
                .map(|g| g[3])
                .sum::<i32>() as f32
        })
    }
}

/// Everything the menus are made of (`common/menus.json`).
#[derive(Clone, Debug, Default)]
pub struct Data {
    dialogs: HashMap<String, DialogDef>,
    /// The level descriptors: number and name, in their order.
    pub levels: Vec<(u16, String)>,
    popups: HashMap<String, Vec<(i64, String)>>,
    pub look: Look,
}

fn shade(v: &str) -> u32 {
    u32::from_str_radix(v.trim_start_matches('#'), 16).unwrap_or(0xFFFF_FFFF)
}

impl Data {
    pub fn load(common: &Path) -> Data {
        let doc: oni_schema::files::MenusFile = crate::json::read_file(common.join("menus.json"));
        let dialogs = doc
            .dialogs
            .iter()
            .map(|(name, d)| {
                let items = d
                    .items
                    .iter()
                    .map(|i| ItemDef {
                        id: i.id,
                        kind: i.kind as u16,
                        title: i.title.clone(),
                        flags: i.flags as u32,
                        style: i.style as u32,
                        rect: i.rect.map(|v| v as f32),
                        font: FontDef {
                            size: i.font.size as u16,
                            style: i.font.style as u8,
                            shade: shade(&i.font.shade),
                        },
                    })
                    .collect();
                let size = d.size.map(|v| v as f32);
                (name.clone(), DialogDef { size, items })
            })
            .collect();
        let levels = doc
            .levels
            .iter()
            .map(|l| (l.level as u16, l.name.clone()))
            .collect();
        let popups = doc
            .popups
            .iter()
            .map(|(name, items)| {
                (
                    name.clone(),
                    items.iter().map(|i| (i.id, i.title.clone())).collect(),
                )
            })
            .collect();
        Data {
            dialogs,
            levels,
            popups,
            look: Look {
                fonts: Fonts::load(common),
                specs: crate::json::read_file(common.join("part_specs.json")),
                skin: doc.skin.unwrap_or_default().into_iter().collect(),
                textures: common.join("textures"),
                sizes: Default::default(),
            },
        }
    }

    pub fn has(&self, dialog: &str) -> bool {
        self.dialogs.contains_key(dialog)
    }
}

// --- the controls --------------------------------------------------------

/// A control's own state.
#[derive(Clone, Debug)]
pub enum Ctl {
    Static,
    /// Pressed (the mouse went down on it and is over it).
    Button {
        hilite: bool,
        down: bool,
    },
    Check(bool),
    /// `drag`: the thumb held, the mouse's offset from it; `held`: the
    /// left button down on it (its timer presses again).
    Slider {
        min: i32,
        max: i32,
        value: i32,
        drag: Option<f32>,
        held: bool,
    },
    /// A progress bar's percent (`WMtProgressBar`).
    Progress(u32),
    /// Rows (string, data), the one selected, the first showing.
    List {
        rows: Vec<(String, u32)>,
        selected: Option<usize>,
        start: usize,
    },
    /// Items (id, string), the one chosen; its menu open (and just opened:
    /// released over the popup, it stays open), the item under the mouse.
    Popup {
        items: Vec<(i64, String)>,
        selected: usize,
        open: bool,
        fresh: bool,
        hover: Option<usize>,
    },
}

#[derive(Clone, Debug)]
pub struct Control {
    pub def: ItemDef,
    pub visible: bool,
    pub enabled: bool,
    pub ctl: Ctl,
}

impl Control {
    fn contains(&self, at: [f32; 2], x: f32, y: f32) -> bool {
        let [rx, ry, rw, rh] = self.def.rect;
        let (l, t) = (at[0] + rx, at[1] + ry);
        x >= l && x < l + rw && y >= t && y < t + rh
    }
}

/// How a control reports (`WMcNotify_*`, `SLcNotify_NewPosition`,
/// `WMcMessage_MenuCommand`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notify {
    Click,
    DoubleClick,
    NewPosition,
    Menu(i64),
}

/// A control's report: its id and what happened; `button` when a button
/// was clicked (its click sounds, `WMcButton_ClickSound`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub id: i64,
    pub notify: Notify,
    pub button: bool,
}

/// The keys the dialogs answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// One dialog up.
#[derive(Clone, Debug)]
pub struct Window {
    pub name: String,
    size: [f32; 2],
    pub controls: Vec<Control>,
    focus: Option<usize>,
    /// The list row last clicked, and when (double clicks).
    last_click: Option<(usize, usize, Instant)>,
    /// The line height of each list (by control index).
    line: HashMap<usize, f32>,
    /// When the sliders' timer fires next (`WMrTimer_Start` at their
    /// creation: it runs from the dialog's making).
    timer: Instant,
}

/// The options dialog with the port's "Hide Lens Flares" row (#58): the
/// Graphics panel a row taller, all below it 25 lower (the Close button
/// ends at 470 of 480), and in the new row a label like Gamma's and a box
/// like Subtitles'.
fn add_hide_flares(def: &mut DialogDef) {
    const ROW: f32 = 25.0;
    let label = def
        .items
        .iter()
        .find(|i| i.id == 0 && i.title == "Gamma:")
        .cloned();
    let check = def.items.iter().find(|i| i.id == SUBTITLES).cloned();
    let (Some(mut label), Some(mut check)) = (label, check) else {
        return;
    };
    for i in &mut def.items {
        let [_, y, _, h] = i.rect;
        if i.title == "Graphics" && h > 50.0 {
            i.rect[3] += ROW;
        } else if y > 150.0 && h < 400.0 {
            i.rect[1] += ROW;
        }
    }
    label.title = "Hide Lens Flares:".into();
    label.rect = [30.0, 140.0, 130.0, 20.0];
    check.id = HIDE_FLARES;
    check.title = "On".into();
    check.rect = [165.0, 140.0, 50.0, 20.0];
    def.items.insert(0, check);
    def.items.insert(0, label);
}

impl Window {
    /// The dialog `name` made up (`WMrDialog_Create`): each item a control,
    /// visible as its flags say.
    pub fn new(data: &Data, name: &str) -> Window {
        let mut def = data.dialogs.get(name).cloned().unwrap_or_default();
        if name == "options" {
            add_hide_flares(&mut def);
        }
        let mut line = HashMap::new();
        let controls = def
            .items
            .iter()
            .enumerate()
            .map(|(k, i)| {
                let ctl = match i.kind {
                    BUTTON => Ctl::Button {
                        hilite: false,
                        down: false,
                    },
                    CHECKBOX => Ctl::Check(false),
                    SLIDER => Ctl::Slider {
                        min: 0,
                        max: 100,
                        value: 0,
                        drag: None,
                        held: false,
                    },
                    PROGRESSBAR => Ctl::Progress(0),
                    LISTBOX => {
                        line.insert(k, data.look.line_height(i.font));
                        Ctl::List {
                            rows: Vec::new(),
                            selected: None,
                            start: 0,
                        }
                    }
                    POPUP => {
                        let items = if i.style & POPUP_RUNTIME != 0 {
                            Vec::new()
                        } else {
                            data.popups.get(&i.title).cloned().unwrap_or_default()
                        };
                        Ctl::Popup {
                            items,
                            selected: 0,
                            open: false,
                            fresh: false,
                            hover: None,
                        }
                    }
                    _ => Ctl::Static,
                };
                Control {
                    def: i.clone(),
                    visible: i.flags & FLAG_VISIBLE != 0,
                    enabled: i.flags & FLAG_DISABLED == 0,
                    ctl,
                }
            })
            .collect();
        Window {
            name: name.to_string(),
            size: def.size,
            controls,
            focus: None,
            last_click: None,
            line,
            timer: Instant::now() + std::time::Duration::from_secs_f32(SLIDER_REPEAT),
        }
    }

    fn index(&self, id: i64) -> Option<usize> {
        self.controls.iter().position(|c| c.def.id == id)
    }

    pub fn control(&self, id: i64) -> Option<&Control> {
        self.index(id).map(|k| &self.controls[k])
    }

    pub fn control_mut(&mut self, id: i64) -> Option<&mut Control> {
        self.index(id).map(|k| &mut self.controls[k])
    }

    pub fn set_visible(&mut self, id: i64, on: bool) {
        if let Some(c) = self.control_mut(id) {
            c.visible = on;
        }
    }

    pub fn set_check(&mut self, id: i64, on: bool) {
        if let Some(Ctl::Check(c)) = self.control_mut(id).map(|c| &mut c.ctl) {
            *c = on;
        }
    }

    pub fn check(&self, id: i64) -> bool {
        matches!(self.control(id).map(|c| &c.ctl), Some(Ctl::Check(true)))
    }

    /// A progress bar's percent (`WMcMessage_SetValue`: at most 100).
    pub fn set_progress(&mut self, id: i64, percent: u32) {
        if let Some(Ctl::Progress(p)) = self.control_mut(id).map(|c| &mut c.ctl) {
            *p = percent.min(100);
        }
    }

    pub fn set_slider(&mut self, id: i64, min: i32, max: i32, value: i32) {
        if let Some(Ctl::Slider {
            min: a,
            max: b,
            value: v,
            ..
        }) = self.control_mut(id).map(|c| &mut c.ctl)
        {
            (*a, *b, *v) = (min, max, value.clamp(min, max));
        }
    }

    /// Control `id` takes input, or not (greyed).
    pub fn set_enabled(&mut self, id: i64, on: bool) {
        if let Some(c) = self.control_mut(id) {
            c.enabled = on;
        }
    }

    pub fn slider(&self, id: i64) -> Option<i32> {
        match self.control(id)?.ctl {
            Ctl::Slider { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn set_popup(&mut self, id: i64, items: Option<Vec<(i64, String)>>, selected_id: i64) {
        if let Some(Ctl::Popup {
            items: its,
            selected,
            ..
        }) = self.control_mut(id).map(|c| &mut c.ctl)
        {
            if let Some(i) = items {
                *its = i;
            }
            *selected = its.iter().position(|(k, _)| *k == selected_id).unwrap_or(0);
        }
    }

    pub fn popup(&self, id: i64) -> Option<i64> {
        match &self.control(id)?.ctl {
            Ctl::Popup {
                items, selected, ..
            } => items.get(*selected).map(|(k, _)| *k),
            _ => None,
        }
    }

    pub fn set_rows(&mut self, id: i64, rows: Vec<(String, u32)>, selected: Option<usize>) {
        let k = self.index(id);
        let visible = k.map_or(1, |k| self.visible_rows(k));
        if let Some(Ctl::List {
            rows: r,
            selected: s,
            start,
        }) = self.control_mut(id).map(|c| &mut c.ctl)
        {
            *r = rows;
            *s = selected;
            *start = selected.map_or(0, |s| s.saturating_sub(visible.saturating_sub(1)));
        }
    }

    /// The selected row's data (`WMrListBox_GetItemData(…, -1)`).
    pub fn selected_data(&self, id: i64) -> Option<u32> {
        match &self.control(id)?.ctl {
            Ctl::List { rows, selected, .. } => selected.and_then(|s| rows.get(s)).map(|r| r.1),
            _ => None,
        }
    }

    pub fn rows(&self, id: i64) -> Vec<String> {
        match self.control(id).map(|c| &c.ctl) {
            Some(Ctl::List { rows, .. }) => rows.iter().map(|r| r.0.clone()).collect(),
            _ => Vec::new(),
        }
    }

    pub fn focus(&mut self, id: i64) {
        self.focus = self.index(id);
    }

    /// Where it sits on a `w` × `h` screen: centred.
    pub fn at(&self, w: f32, h: f32) -> [f32; 2] {
        [
            ((w - self.size[0]) * 0.5).floor().max(0.0),
            ((h - self.size[1]) * 0.5).floor().max(0.0),
        ]
    }

    fn visible_rows(&self, k: usize) -> usize {
        let h = self.controls[k].def.rect[3];
        ((h / self.line.get(&k).copied().unwrap_or(14.0)).floor() as usize).max(1)
    }

    /// The control the mouse is over, front to back
    /// (`WMiWindow_GetWindowUnderPoint`).
    fn under(&self, at: [f32; 2], x: f32, y: f32) -> Option<usize> {
        self.controls.iter().position(|c| {
            c.visible
                && c.def.flags & FLAG_MOUSE_TRANSPARENT == 0
                && c.def.kind != PICTURE
                && c.def.kind != TEXT
                && c.def.kind != BOX
                && c.contains(at, x, y)
        })
    }

    /// An open popup's menu: its index, and its items' rows (screen x, y,
    /// width, height each).
    fn open_menu(&self, look: &Look, at: [f32; 2]) -> Option<(usize, Vec<[f32; 4]>)> {
        let k = self
            .controls
            .iter()
            .position(|c| matches!(c.ctl, Ctl::Popup { open: true, .. }))?;
        let c = &self.controls[k];
        let Ctl::Popup { items, .. } = &c.ctl else {
            return None;
        };
        let [bw, bh] = look.part("background").map_or([0.0; 2], |p| p.corner());
        let lh = look.line_height(c.def.font);
        let width = items
            .iter()
            .map(|(_, s)| look.width(c.def.font, s))
            .fold(0.0, f32::max)
            + 2.0 * bw
            + MENU_LEFT
            + MENU_RIGHT;
        let (x, y) = (at[0] + c.def.rect[0], at[1] + c.def.rect[1]);
        Some((
            k,
            (0..items.len())
                .map(|i| [x, y + bh + i as f32 * lh, width.max(c.def.rect[2]), lh])
                .collect(),
        ))
    }

    /// The left button down at screen `x`, `y`.
    pub fn mouse_down(&mut self, look: &Look, w: f32, h: f32, x: f32, y: f32) -> Vec<Command> {
        let at = self.at(w, h);
        // An open popup menu takes the click: an item chosen, else closed.
        if let Some((k, rows)) = self.open_menu(look, at) {
            let hit = rows
                .iter()
                .position(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]);
            return self.close_menu(k, hit);
        }
        let Some(k) = self.under(at, x, y) else {
            return Vec::new();
        };
        if !self.controls[k].enabled {
            return Vec::new();
        }
        self.focus = Some(k);
        let lh = self.line.get(&k).copied().unwrap_or(14.0);
        let visible = self.visible_rows(k);
        let c = &mut self.controls[k];
        let [rx, ry, rw, _] = c.def.rect;
        let (lx, ly) = (x - at[0] - rx, y - at[1] - ry);
        if let Ctl::Slider { held, .. } = &mut c.ctl {
            *held = true;
            return slider_press(look, &c.def, &mut c.ctl, lx, ly)
                .into_iter()
                .collect();
        }
        let mut out = Vec::new();
        match &mut c.ctl {
            Ctl::Button { hilite, down } => {
                *hilite = true;
                *down = true;
            }
            Ctl::Popup {
                open, fresh, hover, ..
            } => {
                *open = true;
                *fresh = true;
                *hover = None;
            }
            Ctl::List {
                rows,
                selected,
                start,
            } => {
                let scroll_w = if c.def.style & LIST_SCROLLBAR != 0 {
                    look.part("up_arrow").map_or(0.0, |p| p.corner()[0])
                } else {
                    0.0
                };
                if scroll_w > 0.0 && lx >= rw - scroll_w {
                    // The scroll bar: its arrows a line, its track a page.
                    let arrow_h = look.part("up_arrow").map_or(0.0, |p| p.corner()[1]);
                    let most = rows.len().saturating_sub(visible);
                    *start = if ly < arrow_h {
                        start.saturating_sub(1)
                    } else if ly >= c.def.rect[3] - arrow_h {
                        (*start + 1).min(most)
                    } else if ly < c.def.rect[3] * 0.5 {
                        start.saturating_sub(visible)
                    } else {
                        (*start + visible).min(most)
                    };
                    return out;
                }
                let row = *start + (ly / lh).floor().max(0.0) as usize;
                if row < rows.len() && row < *start + visible {
                    *selected = Some(row);
                    let double = self.last_click.is_some_and(|(kk, r, t)| {
                        kk == k && r == row && t.elapsed().as_secs_f32() < DOUBLE_CLICK
                    });
                    self.last_click = Some((k, row, Instant::now()));
                    out.push(Command {
                        id: c.def.id,
                        notify: if double {
                            Notify::DoubleClick
                        } else {
                            Notify::Click
                        },
                        button: false,
                    });
                }
            }
            _ => {}
        }
        out
    }

    /// The window manager's timers (`WMiTimer_Update`), at `now` with the
    /// mouse at screen `x`, `y`: when the sliders' fires, each slider the
    /// left button is down on is pressed again where the mouse is
    /// (`WMiSlider_HandleTimer`) — paging on until the thumb comes under
    /// the mouse, which then takes it.
    pub fn tick(
        &mut self,
        look: &Look,
        w: f32,
        h: f32,
        x: f32,
        y: f32,
        now: Instant,
    ) -> Vec<Command> {
        if now <= self.timer {
            return Vec::new();
        }
        self.timer = now + std::time::Duration::from_secs_f32(SLIDER_REPEAT);
        let at = self.at(w, h);
        let mut out = Vec::new();
        for c in &mut self.controls {
            if matches!(c.ctl, Ctl::Slider { held: true, .. }) && c.enabled {
                let (lx, ly) = (x - at[0] - c.def.rect[0], y - at[1] - c.def.rect[1]);
                out.extend(slider_press(look, &c.def, &mut c.ctl, lx, ly));
            }
        }
        out
    }

    /// The mouse moved (the left button `held` or not).
    pub fn mouse_move(
        &mut self,
        look: &Look,
        w: f32,
        h: f32,
        x: f32,
        y: f32,
        held: bool,
    ) -> Vec<Command> {
        let at = self.at(w, h);
        if let Some((k, rows)) = self.open_menu(look, at) {
            let hit = rows
                .iter()
                .position(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]);
            if let Ctl::Popup { hover, .. } = &mut self.controls[k].ctl {
                *hover = hit;
            }
            return Vec::new();
        }
        let mut out = Vec::new();
        for c in &mut self.controls {
            let inside = c.contains(at, x, y);
            match &mut c.ctl {
                Ctl::Button { hilite, down } if *down && held => *hilite = inside,
                Ctl::Slider {
                    min,
                    max,
                    value,
                    drag: Some(off),
                    ..
                } if held => {
                    let (track, thumb_w) = slider_track(look, c.def.rect[2]);
                    let span = (track[1] - track[0] - thumb_w).max(1.0);
                    let lx = x - at[0] - c.def.rect[0];
                    let v = (*min + ((lx - track[0] + *off) * (*max - *min) as f32 / span) as i32)
                        .clamp(*min, *max);
                    if v != *value {
                        *value = v;
                        out.push(Command {
                            id: c.def.id,
                            notify: Notify::NewPosition,
                            button: false,
                        });
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The left button up at screen `x`, `y`.
    pub fn mouse_up(&mut self, look: &Look, w: f32, h: f32, x: f32, y: f32) -> Vec<Command> {
        let at = self.at(w, h);
        if let Some((k, rows)) = self.open_menu(look, at) {
            let hit = rows
                .iter()
                .position(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]);
            // (Released over the popup just after opening it: the menu
            // stays open for a click.)
            let over_popup = self.controls[k].contains(at, x, y);
            if let Ctl::Popup { fresh, .. } = &mut self.controls[k].ctl {
                if std::mem::take(fresh) && over_popup {
                    return Vec::new();
                }
            }
            return self.close_menu(k, hit);
        }
        let mut out = Vec::new();
        let focus = self.focus;
        for (k, c) in self.controls.iter_mut().enumerate() {
            let inside = c.contains(at, x, y) && c.visible && c.enabled;
            match &mut c.ctl {
                Ctl::Button { hilite, down } => {
                    if *down && inside {
                        out.push(Command {
                            id: c.def.id,
                            notify: Notify::Click,
                            button: true,
                        });
                    }
                    (*hilite, *down) = (false, false);
                }
                Ctl::Slider { drag, held, .. } => (*drag, *held) = (None, false),
                Ctl::Check(on) if inside && focus == Some(k) => {
                    *on = !*on;
                    out.push(Command {
                        id: c.def.id,
                        notify: Notify::Click,
                        button: false,
                    });
                }
                _ => {}
            }
        }
        out
    }

    fn close_menu(&mut self, k: usize, hit: Option<usize>) -> Vec<Command> {
        let id = self.controls[k].def.id;
        let Ctl::Popup {
            items,
            selected,
            open,
            fresh,
            hover,
        } = &mut self.controls[k].ctl
        else {
            return Vec::new();
        };
        *open = false;
        *fresh = false;
        *hover = None;
        match hit {
            Some(i) if i < items.len() => {
                *selected = i;
                vec![Command {
                    id,
                    notify: Notify::Menu(items[i].0),
                    button: false,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// A key (`WMiDialog_HandleKeyEvent` and the focused control's): Enter
    /// clicks the default button, Escape the dialog's Cancel; the arrows and
    /// friends move the focused list's selection or slider.
    pub fn key(&mut self, key: Key) -> Vec<Command> {
        match key {
            Key::Enter => {
                let k = self.controls.iter().position(|c| {
                    c.def.kind == BUTTON
                        && c.def.style & BUTTON_DEFAULT != 0
                        && c.visible
                        && c.enabled
                });
                return k
                    .map(|k| {
                        vec![Command {
                            id: self.controls[k].def.id,
                            notify: Notify::Click,
                            button: false,
                        }]
                    })
                    .unwrap_or_default();
            }
            Key::Escape => {
                return vec![Command {
                    id: CANCEL,
                    notify: Notify::Click,
                    button: false,
                }]
            }
            _ => {}
        }
        let Some(k) = self.focus else {
            return Vec::new();
        };
        let visible = self.visible_rows(k);
        let c = &mut self.controls[k];
        match &mut c.ctl {
            Ctl::List {
                rows,
                selected,
                start,
            } if !rows.is_empty() => {
                let n = rows.len();
                let s = selected.unwrap_or(0);
                // (Nothing selected: -1, so Down lands on the first row.)
                let to = match key {
                    Key::Up => s.saturating_sub(1),
                    Key::Down => selected.map_or(0, |s| (s + 1).min(n - 1)),
                    Key::Home => 0,
                    Key::End => n - 1,
                    Key::PageUp => s.saturating_sub(visible),
                    Key::PageDown => (s + visible).min(n - 1),
                    _ => return Vec::new(),
                };
                *selected = Some(to);
                if to < *start {
                    *start = to;
                } else if to >= *start + visible {
                    *start = to + 1 - visible;
                }
                vec![Command {
                    id: c.def.id,
                    notify: Notify::Click,
                    button: false,
                }]
            }
            Ctl::Slider {
                min, max, value, ..
            } => {
                let page = ((*max - *min) / 10).max(1);
                let to = match key {
                    Key::Left => *value - 1,
                    Key::Right => *value + 1,
                    Key::PageUp => *value - page,
                    Key::PageDown => *value + page,
                    Key::Home => *min,
                    Key::End => *max,
                    _ => return Vec::new(),
                };
                *value = to.clamp(*min, *max);
                vec![Command {
                    id: c.def.id,
                    notify: Notify::NewPosition,
                    button: false,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// The wheel over a list: its rows scroll.
    pub fn wheel(&mut self, w: f32, h: f32, x: f32, y: f32, lines: i32) {
        let at = self.at(w, h);
        let Some(k) = self.under(at, x, y) else {
            return;
        };
        let visible = self.visible_rows(k);
        if let Ctl::List { rows, start, .. } = &mut self.controls[k].ctl {
            let most = rows.len().saturating_sub(visible) as i32;
            *start = (*start as i32 - lines).clamp(0, most.max(0)) as usize;
        }
    }

    /// The dialog over a `w` × `h` screen: its controls back to front
    /// (`WMiWindow_Draw`), an open popup menu over all.
    pub fn draw(&self, look: &Look, w: f32, h: f32) -> Vec<OverlayDraw> {
        let at = self.at(w, h);
        let mut out = Vec::new();
        for (k, c) in self.controls.iter().enumerate().rev() {
            if !c.visible {
                continue;
            }
            let [rx, ry, rw, rh] = c.def.rect;
            let r = [at[0] + rx, at[1] + ry, rw, rh];
            let shade = if c.enabled { c.def.font.shade } else { GRAY50 };
            match (&c.ctl, c.def.kind) {
                (_, PICTURE) => {
                    let (path, size) = look.texture(&c.def.title);
                    if let Some([tw, th]) = size {
                        let (dw, dh) = if c.def.style & PICTURE_SCALE != 0 {
                            (rw, rh)
                        } else {
                            (rw.min(tw), rh.min(th))
                        };
                        // (Its texels from the top left, as many as show.)
                        let (u, v) = (dw / tw, dh / th);
                        let mut d = rect(Some(path), [r[0], r[1], r[0] + dw, r[1] + dh], [1.0; 4]);
                        for t in &mut d.tris {
                            t[2] *= u;
                            t[3] *= v;
                        }
                        out.push(d);
                    }
                }
                (_, BOX) => {
                    let lh = look.line_height(c.def.font);
                    let titled = c.def.style & BOX_TITLE != 0;
                    let dy = if titled { (lh / 2.0).floor() } else { 0.0 };
                    if let Some(p) = look.part("outline") {
                        out.extend(p.draw([r[0], r[1] + dy], rw, rh - dy));
                    }
                    if titled {
                        let tw = look.width(c.def.font, &c.def.title);
                        if let Some(p) = look.part("background") {
                            out.extend(p.draw_middle([r[0] + 5.0, r[1]], tw, lh));
                        }
                        string(
                            &mut out,
                            look,
                            c.def.font,
                            shade,
                            &c.def.title,
                            [r[0] + 5.0, r[1], tw + 1.0, lh],
                            Halign::Left,
                            true,
                        );
                    }
                }
                (_, TEXT) => {
                    let h = if c.def.style & TEXT_HCENTER != 0 {
                        Halign::Center
                    } else if c.def.style & TEXT_HRIGHT != 0 {
                        Halign::Right
                    } else {
                        Halign::Left
                    };
                    string(
                        &mut out,
                        look,
                        c.def.font,
                        shade,
                        &c.def.title,
                        r,
                        h,
                        c.def.style & TEXT_VCENTER != 0,
                    );
                }
                (Ctl::Button { hilite, .. }, _) => {
                    let key = if *hilite {
                        "button_on"
                    } else if c.def.style & BUTTON_DEFAULT != 0 {
                        "default_button"
                    } else {
                        "button_off"
                    };
                    if let Some(p) = look.part(key) {
                        out.extend(p.draw([r[0], r[1]], rw, rh));
                    }
                    string(
                        &mut out,
                        look,
                        c.def.font,
                        shade,
                        &c.def.title,
                        r,
                        Halign::Center,
                        true,
                    );
                }
                (Ctl::Check(on), _) => {
                    let p = look.part(if *on { "checkbox_on" } else { "checkbox_off" });
                    let [pw, ph] = p.as_ref().map_or([0.0; 2], |p| p.corner());
                    if let Some(p) = p {
                        out.extend(p.draw_corner([r[0], r[1] + ((rh - ph) / 2.0).floor()], pw, ph));
                    }
                    if c.def.style & CHECKBOX_TITLE != 0 {
                        let x = r[0] + pw + CHECKBOX_GAP;
                        string(
                            &mut out,
                            look,
                            c.def.font,
                            shade,
                            &c.def.title,
                            [x, r[1], (rw - pw - CHECKBOX_GAP).max(0.0), rh],
                            Halign::Left,
                            true,
                        );
                    }
                }
                (
                    Ctl::Slider {
                        min, max, value, ..
                    },
                    _,
                ) => {
                    let (track, thumb_w) = slider_track(look, rw);
                    if let Some(p) = look.part("sl_track") {
                        let th = p.corner()[1];
                        out.extend(p.draw_top_row(
                            [r[0], r[1] + ((rh - th) / 2.0).floor()],
                            rw,
                            th,
                        ));
                    }
                    if let Some(p) = look.part("sl_thumb") {
                        let [tw, th] = p.corner();
                        let span = (track[1] - track[0] - thumb_w).max(1.0);
                        let x = r[0]
                            + track[0]
                            + ((*value - *min) as f32 * span / (*max - *min).max(1) as f32).floor();
                        out.extend(p.draw_corner([x, r[1] + ((rh - th) / 2.0).floor()], tw, th));
                    }
                }
                // (`WMiProgressBar_Paint`: the track over the whole; the
                // fill inside its edges, as wide as the percent, at least
                // its two ends' width.)
                (Ctl::Progress(percent), _) => {
                    if let Some(p) = look.part("progressbar_track") {
                        out.extend(p.draw([r[0], r[1]], rw, rh));
                        if c.enabled && *percent > 0 {
                            let ([lw, lh], [bw, bh]) = (p.corner(), p.bottom_right());
                            let (iw, ih) = (rw - (lw + bw) + 2.0, rh - (lh + bh) + 2.0);
                            let fw = (*percent as f32 * iw / 100.0).floor().max(lw + bw);
                            if let Some(f) = look.part("progressbar_fill") {
                                out.extend(f.draw([r[0] + lw - 1.0, r[1] + lh - 1.0], fw, ih));
                            }
                        }
                    }
                }
                (
                    Ctl::Popup {
                        items, selected, ..
                    },
                    _,
                ) => {
                    let p = look.part("popup_menu");
                    let lw = p.as_ref().map_or(0.0, |p| p.corner()[0]);
                    if let Some(p) = p {
                        out.extend(p.draw([r[0], r[1]], rw, rh));
                    }
                    let title = items.get(*selected).map_or("", |(_, s)| s.as_str());
                    // (On one line, cut before the arrow: the original
                    // wraps a title too long for it.)
                    let room =
                        (rw - 2.0 - lw - look.part("popup_menu").map_or(0.0, |p| p.right_width()))
                            .max(0.0);
                    let title = fit(look, c.def.font, title, room);
                    string(
                        &mut out,
                        look,
                        c.def.font,
                        shade,
                        &title,
                        [r[0] + 2.0 + lw, r[1], room + 1000.0, rh],
                        Halign::Left,
                        true,
                    );
                }
                (
                    Ctl::List {
                        rows,
                        selected,
                        start,
                    },
                    _,
                ) => {
                    let lh = self.line.get(&k).copied().unwrap_or(14.0);
                    let scroll = c.def.style & LIST_SCROLLBAR != 0;
                    let sw = if scroll {
                        look.part("up_arrow").map_or(0.0, |p| p.corner()[0])
                    } else {
                        0.0
                    };
                    let item_w = if scroll { rw - (sw - 1.0) } else { rw };
                    let visible = self.visible_rows(k);
                    let focused = self.focus == Some(k);
                    for (n, (text, _)) in rows.iter().enumerate().skip(*start).take(visible) {
                        let y = r[1] + (n - start) as f32 * lh;
                        if *selected == Some(n) && c.enabled {
                            if let Some(p) =
                                look.part(if focused { "hilite" } else { "background" })
                            {
                                out.extend(p.draw([r[0], y], item_w, lh));
                            }
                        }
                        string(
                            &mut out,
                            look,
                            c.def.font,
                            shade,
                            text,
                            [r[0] + LIST_TEXT_X, y, item_w - LIST_TEXT_X, lh],
                            Halign::Left,
                            false,
                        );
                    }
                    if scroll {
                        scrollbar(
                            &mut out,
                            look,
                            [r[0] + rw - sw + 1.0, r[1] - 1.0, sw, rh + 2.0],
                            *start,
                            rows.len(),
                            visible,
                        );
                    }
                }
                _ => {}
            }
        }
        // An open popup menu (`WMiMenu_Paint`): the background, the item
        // under the mouse lit, the items' strings.
        if let Some((k, rows)) = self.open_menu(look, at) {
            let c = &self.controls[k];
            if let (Ctl::Popup { items, hover, .. }, Some(first)) = (&c.ctl, rows.first()) {
                let [bw, bh] = look.part("background").map_or([0.0; 2], |p| p.corner());
                let total = rows.len() as f32 * first[3] + 2.0 * bh;
                if let Some(p) = look.part("background") {
                    out.extend(p.draw([first[0], first[1] - bh], first[2], total));
                }
                for (i, row) in rows.iter().enumerate() {
                    if *hover == Some(i) {
                        if let Some(p) = look.part("hilite") {
                            out.extend(p.draw([row[0] + bw, row[1]], row[2] - 2.0 * bw, row[3]));
                        }
                    }
                    string(
                        &mut out,
                        look,
                        c.def.font,
                        c.def.font.shade,
                        &items[i].1,
                        [row[0] + bw + MENU_LEFT, row[1], row[2], row[3]],
                        Halign::Left,
                        false,
                    );
                }
            }
        }
        out
    }
}

/// A slider pressed at `lx`, `ly` in it (`WMiSlider_HandleMouseEvent`,
/// `WMcMessage_LMouseDown`): on the thumb (not yet held), the thumb taken;
/// else left of it (to its first pixel) a page less, right of it (from its
/// last) a page more — a tenth of the range, at least one — reported.
fn slider_press(look: &Look, def: &ItemDef, ctl: &mut Ctl, lx: f32, ly: f32) -> Option<Command> {
    let Ctl::Slider {
        min,
        max,
        value,
        drag,
        ..
    } = ctl
    else {
        return None;
    };
    let [_, _, rw, rh] = def.rect;
    let (track, thumb_w) = slider_track(look, rw);
    let thumb_h = look.part("sl_thumb").map_or(0.0, |p| p.corner()[1]);
    let span = (track[1] - track[0] - thumb_w).max(1.0);
    let thumb = track[0] + ((*value - *min) as f32 * span / (*max - *min).max(1) as f32).floor();
    let thumb_top = ((rh - thumb_h) / 2.0).floor();
    let in_thumb =
        lx >= thumb && lx < thumb + thumb_w && ly >= thumb_top && ly < thumb_top + thumb_h;
    let in_height = ly >= 0.0 && ly < rh;
    let page = ((*max - *min) / 10).max(1);
    let step = if in_thumb && drag.is_none() {
        *drag = Some(thumb - lx);
        0
    } else if in_height && lx >= 0.0 && lx < thumb + 1.0 {
        -page
    } else if in_height && lx >= thumb + thumb_w - 1.0 && lx < rw {
        page
    } else {
        0
    };
    if step == 0 {
        return None;
    }
    *value = (*value + step).clamp(*min, *max);
    Some(Command {
        id: def.id,
        notify: Notify::NewPosition,
        button: false,
    })
}

/// A slider's track (its run between the track's end caps, within the
/// slider) and its thumb's width (`WMiSlider_GetTrackBounds`).
fn slider_track(look: &Look, width: f32) -> ([f32; 2], f32) {
    let (l, r) = look
        .part("sl_track")
        .map_or((0.0, 0.0), |p| (p.corner()[0], p.right_width()));
    let thumb = look.part("sl_thumb").map_or(0.0, |p| p.corner()[0]);
    ([l, width - r], thumb)
}

/// A vertical scroll bar (`WMiScrollbar_Paint`): its arrows at the ends,
/// the track between, the thumb where the first row showing puts it.
fn scrollbar(
    out: &mut Vec<OverlayDraw>,
    look: &Look,
    r: [f32; 4],
    start: usize,
    rows: usize,
    visible: usize,
) {
    let [x, y, w, h] = r;
    let (up, dn) = (look.part("up_arrow"), look.part("dn_arrow"));
    let ah = up.as_ref().map_or(0.0, |p| p.corner()[1]);
    if let Some(p) = look.part("sb_v_track") {
        out.extend(p.draw([x, y + ah], w, (h - 2.0 * ah).max(0.0)));
    }
    if let Some(p) = up {
        out.extend(p.draw_corner([x, y], w, ah));
    }
    if let Some(p) = dn {
        out.extend(p.draw_corner([x, y + h - ah], w, ah));
    }
    if rows > visible {
        if let Some(p) = look.part("sb_thumb") {
            let th = p.corner()[1].max(1.0);
            let span = (h - 2.0 * ah - th).max(0.0);
            let t = y + ah + (start as f32 / (rows - visible) as f32 * span).floor();
            out.extend(p.draw([x, t], w, th));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Halign {
    Left,
    Center,
    Right,
}

/// A string in an area (`DCrDraw_String`): wrapped to its width, placed as
/// asked, centred top to bottom if `vcenter`.
#[allow(clippy::too_many_arguments)]
fn string(
    out: &mut Vec<OverlayDraw>,
    look: &Look,
    f: FontDef,
    shade: u32,
    s: &str,
    area: [f32; 4],
    h: Halign,
    vcenter: bool,
) {
    let Some(font) = look.font(f) else { return };
    let [x, y, w, ah] = area;
    let bounds = [
        x as i32,
        y as i32,
        (x + w) as i32,
        (y + ah.max(1000.0)) as i32,
    ];
    let align = if h == Halign::Center {
        Align::Center
    } else {
        Align::Left
    };
    let segments = text::format(
        font,
        &look.fonts.breaking,
        s,
        bounds,
        x as i32,
        y as i32,
        align,
        false,
        shade & 0xFF_FFFF,
    );
    let lines = segments
        .iter()
        .map(|s| s.top)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        .max(1) as f32;
    let used = lines * font.line_height().max(1) as f32 - font.leading as f32;
    let dy = if vcenter {
        ((ah - used) / 2.0).floor()
    } else {
        0.0
    };
    for seg in &segments {
        let dx = if h == Halign::Right {
            (w - seg.width as f32).floor()
        } else {
            0.0
        };
        out.push(OverlayDraw {
            texture: Some(font.atlas.clone()),
            tris: glyphs(font, &seg.text, seg.x as f32 + dx, seg.top as f32 + dy),
            color: rgba(shade),
            additive: false,
        });
    }
}

/// As much of `s` as fits `width` in a font (whole characters).
fn fit(look: &Look, f: FontDef, s: &str, width: f32) -> String {
    let mut out = s.to_string();
    while !out.is_empty() && look.width(f, &out) > width {
        out.pop();
    }
    out.trim_end().to_string()
}

// --- the out-of-game menus -----------------------------------------------

/// The main menu's buttons (`ONcMainMenu_Btn_*`).
const NEW_GAME: i64 = 100;
const LOAD_GAME: i64 = 101;
const OPTIONS: i64 = 102;
const QUIT: i64 = 103;
const RESUME: i64 = 104;
/// The load game list (`ONcLoadGame_LB_Levels`).
const LEVELS: i64 = 100;
/// The options' controls (`ONcOptions_*`).
const QUALITY: i64 = 100;
const RESOLUTION: i64 = 101;
const VOLUME: i64 = 102;
const SUBTITLES: i64 = 107;
const DIFFICULTY: i64 = 108;
const INVERT_MOUSE: i64 = 109;
const GAMMA: i64 = 110;
/// The port's own box on the options' Graphics panel (#58): characters
/// hide the lens flares behind them.
const HIDE_FLARES: i64 = 112;
/// The new game prompt's faithful checkbox (the port's own; not in the
/// data).
pub const FAITHFUL: i64 = 3;
/// The resolutions the options offer (shown and kept; the window is sized
/// freely), named without the original's bit depth (`%d x %d x %d bit`:
/// the port has none to choose, and the longest wouldn't fit the popup).
const RESOLUTIONS: [(u32, u32); 5] = [
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 1024),
    (1600, 1200),
];
const DIFFICULTIES: [&str; 3] = ["easy", "normal", "hard"];

/// What the menus ask the game to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A new game: level 1 from its start, with the original's bugs or not.
    NewGame {
        faithful: bool,
    },
    /// A level from a save point (0: its start).
    Load {
        level: u16,
        save_point: i64,
    },
    /// Back to the game.
    Resume,
    Quit,
    /// An option changed (kept at once, `ONrPersist_Set*`).
    Options(Settings),
}

/// A row of the load game list: the level's, or one of its continues.
pub struct LoadRow {
    pub text: String,
    pub level: u16,
    pub save_point: i64,
}

/// The main menu up (`ONrOutGameUI_MainMenu_Display`) and the dialogs over
/// it, innermost last.
pub struct MainMenu {
    pub stack: Vec<Window>,
    /// Over a level being played (Resume instead of Options; Escape
    /// resumes).
    pub in_game: bool,
    pub settings: Settings,
    /// The original's slips kept (Q19).
    pub quirks: crate::quirks::Quirks,
}

impl MainMenu {
    /// Opened (`ONiOGU_MainMenu_InitDialog`): in a level, Resume shows in
    /// Options' place; at the start, Options.
    pub fn open(data: &Data, in_game: bool, settings: Settings) -> MainMenu {
        let mut w = Window::new(data, "mainmenu");
        w.set_visible(OPTIONS, !in_game);
        w.set_visible(RESUME, in_game);
        MainMenu {
            stack: vec![w],
            in_game,
            settings,
            quirks: crate::quirks::Quirks::FIXED,
        }
    }

    pub fn top(&self) -> Option<&Window> {
        self.stack.last()
    }

    pub fn top_mut(&mut self) -> Option<&mut Window> {
        self.stack.last_mut()
    }

    /// The load game dialog over it (`ONiOGU_LoadGame_InitDialog`,
    /// `OWrLevelList_Initialize`): `rows` — each level the player has
    /// reached, then its continues — the place last played selected.
    pub fn open_load(&mut self, data: &Data, rows: Vec<LoadRow>, place: Option<(u16, i64)>) {
        let mut w = Window::new(data, "loadgame");
        let selected = place.and_then(|(l, s)| {
            rows.iter()
                .position(|r| r.level == l && r.save_point == s && s > 0)
        });
        let list = rows
            .into_iter()
            .map(|r| (r.text, ((r.save_point as u32) << 8) | u32::from(r.level)))
            .collect();
        w.set_rows(LEVELS, list, selected);
        w.focus(LEVELS);
        self.stack.push(w);
    }

    fn open_options(&mut self, data: &Data) {
        let s = &self.settings;
        let mut w = Window::new(data, "options");
        // (Frozen at high: the port always draws every effect; the
        // original's lower settings only dropped effects for speed.)
        w.set_slider(QUALITY, 0, 2, 2);
        w.set_enabled(QUALITY, false);
        w.set_slider(GAMMA, 0, 100, (s.gamma * 100.0).round() as i32);
        w.set_slider(VOLUME, 0, 100, (s.overall_volume * 100.0).round() as i32);
        w.set_check(SUBTITLES, s.subtitles);
        w.set_check(HIDE_FLARES, s.flares_behind_characters);
        w.set_check(INVERT_MOUSE, s.invert_mouse);
        // (Q19: shipped, the box ticked for inverted is titled "Off".)
        if !self.quirks.invert_mouse_titled_off() {
            if let Some(c) = w.control_mut(INVERT_MOUSE) {
                c.def.title = "On".into();
            }
        }
        w.set_popup(
            DIFFICULTY,
            None,
            DIFFICULTIES
                .iter()
                .position(|d| *d == s.difficulty)
                .unwrap_or(1) as i64,
        );
        let modes: Vec<(i64, String)> = RESOLUTIONS
            .iter()
            .enumerate()
            .map(|(i, (w, h))| (i as i64, format!("{w} x {h}")))
            .collect();
        let current = RESOLUTIONS
            .iter()
            .position(|r| *r == s.resolution)
            .unwrap_or(0) as i64;
        w.set_popup(RESOLUTION, Some(modes), current);
        self.stack.push(w);
    }

    fn open_new_game(&mut self, data: &Data) {
        let mut w = Window::new(data, "NewGame");
        // (The port's own: play with the original's bugs. Under the
        // buttons, off.)
        let font = w.control(OK).map(|c| c.def.font).unwrap_or_default();
        let def = ItemDef {
            id: FAITHFUL,
            kind: CHECKBOX,
            title: "Faithful (the original's bugs)".into(),
            flags: FLAG_VISIBLE,
            style: CHECKBOX_TITLE,
            rect: [35.0, 115.0, 280.0, 20.0],
            font,
        };
        w.controls.insert(
            0,
            Control {
                def,
                visible: true,
                enabled: true,
                ctl: Ctl::Check(false),
            },
        );
        w.line.clear();
        self.stack.push(w);
    }

    /// A dialog's commands, as its callback takes them (`ONiOGU_*_Handle*`).
    /// `rows`: the load game list, when it opens.
    pub fn command(
        &mut self,
        data: &Data,
        cmd: Command,
        rows: &dyn Fn() -> (Vec<LoadRow>, Option<(u16, i64)>),
    ) -> Option<Action> {
        let name = self.top()?.name.clone();
        match (name.as_str(), cmd.id, cmd.notify) {
            ("mainmenu", NEW_GAME, Notify::Click) => self.open_new_game(data),
            ("mainmenu", LOAD_GAME, Notify::Click) => {
                let (r, place) = rows();
                self.open_load(data, r, place);
            }
            ("mainmenu", OPTIONS, Notify::Click) => self.open_options(data),
            ("mainmenu", QUIT, Notify::Click) => self.stack.push(Window::new(data, "QuitYesNo")),
            ("mainmenu", RESUME, Notify::Click) => return Some(Action::Resume),
            // (Escape resumes a game; at the start it does nothing.)
            ("mainmenu", CANCEL, Notify::Click) if self.in_game => return Some(Action::Resume),
            ("NewGame", OK, Notify::Click) => {
                let faithful = self.top().is_some_and(|w| w.check(FAITHFUL));
                return Some(Action::NewGame { faithful });
            }
            ("loadgame", LEVELS, Notify::DoubleClick) | ("loadgame", OK, Notify::Click) => {
                let data = self.top()?.selected_data(LEVELS)?;
                return Some(Action::Load {
                    level: (data & 0xFF) as u16,
                    save_point: i64::from(data >> 8),
                });
            }
            ("QuitYesNo", OK, Notify::Click) => return Some(Action::Quit),
            ("options", id, n) if id != CANCEL => return self.option(id, n),
            (_, CANCEL, Notify::Click) if self.stack.len() > 1 => {
                self.stack.pop();
            }
            _ => {}
        }
        None
    }

    /// An option changed (`ONiOGU_Options_HandleCommand`, `_MenuCommand`).
    fn option(&mut self, id: i64, n: Notify) -> Option<Action> {
        let w = self.stack.last()?;
        let s = &mut self.settings;
        match (id, n) {
            (QUALITY, Notify::NewPosition) => s.quality = 2,
            (GAMMA, Notify::NewPosition) => s.gamma = w.slider(GAMMA)? as f32 / 100.0,
            (VOLUME, Notify::NewPosition) => s.overall_volume = w.slider(VOLUME)? as f32 / 100.0,
            (SUBTITLES, Notify::Click) => s.subtitles = w.check(SUBTITLES),
            (HIDE_FLARES, Notify::Click) => s.flares_behind_characters = w.check(HIDE_FLARES),
            (INVERT_MOUSE, Notify::Click) => s.invert_mouse = w.check(INVERT_MOUSE),
            (DIFFICULTY, Notify::Menu(k)) => {
                s.difficulty = DIFFICULTIES.get(k as usize)?.to_string()
            }
            (RESOLUTION, Notify::Menu(k)) => s.resolution = *RESOLUTIONS.get(k as usize)?,
            _ => return None,
        }
        Some(Action::Options(s.clone()))
    }

    /// The menus over a `w` × `h` screen: black behind (the desktop's
    /// background), each dialog up from the main menu out.
    pub fn draw(&self, data: &Data, w: f32, h: f32) -> Vec<OverlayDraw> {
        let mut out = vec![rect(None, [0.0, 0.0, w, h], [0.0, 0.0, 0.0, 1.0])];
        for win in &self.stack {
            out.extend(win.draw(&data.look, w, h));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, kind: u16, rect: [f32; 4], style: u32) -> ItemDef {
        ItemDef {
            id,
            kind,
            title: String::new(),
            flags: FLAG_VISIBLE,
            style,
            rect,
            font: FontDef::default(),
        }
    }

    fn data() -> Data {
        let mut d = Data::default();
        let dialog = |items: Vec<ItemDef>| DialogDef {
            size: [640.0, 480.0],
            items,
        };
        d.dialogs.insert(
            "mainmenu".into(),
            dialog(vec![
                item(NEW_GAME, BUTTON, [350.0, 230.0, 180.0, 40.0], 0),
                item(LOAD_GAME, BUTTON, [350.0, 290.0, 180.0, 40.0], 0),
                item(OPTIONS, BUTTON, [350.0, 350.0, 180.0, 40.0], 0),
                item(RESUME, BUTTON, [350.0, 350.0, 180.0, 40.0], 0),
                item(QUIT, BUTTON, [350.0, 410.0, 180.0, 40.0], 0),
            ]),
        );
        d.dialogs.insert(
            "NewGame".into(),
            DialogDef {
                size: [350.0, 150.0],
                items: vec![
                    item(OK, BUTTON, [35.0, 70.0, 130.0, 40.0], 0),
                    item(CANCEL, BUTTON, [185.0, 70.0, 130.0, 40.0], 0),
                ],
            },
        );
        d.dialogs.insert(
            "loadgame".into(),
            dialog(vec![
                item(LEVELS, LISTBOX, [140.0, 30.0, 375.0, 360.0], 0),
                item(OK, BUTTON, [120.0, 425.0, 130.0, 40.0], BUTTON_DEFAULT),
                item(CANCEL, BUTTON, [410.0, 425.0, 130.0, 40.0], 0),
            ]),
        );
        d.dialogs.insert(
            "options".into(),
            dialog(vec![
                item(VOLUME, SLIDER, [160.0, 215.0, 120.0, 20.0], 0),
                item(SUBTITLES, CHECKBOX, [115.0, 330.0, 50.0, 20.0], 0),
                item(DIFFICULTY, POPUP, [115.0, 305.0, 150.0, 20.0], 0),
                item(CANCEL, BUTTON, [175.0, 405.0, 130.0, 40.0], 0),
            ]),
        );
        d.popups.insert(
            String::new(),
            vec![(0, "Easy".into()), (1, "Normal".into()), (2, "Hard".into())],
        );
        d
    }

    fn click(m: &mut MainMenu, d: &Data, x: f32, y: f32) -> Option<Action> {
        let w = m.top_mut().unwrap();
        let mut cmds = w.mouse_down(&d.look, 640.0, 480.0, x, y);
        cmds.extend(m.top_mut().unwrap().mouse_up(&d.look, 640.0, 480.0, x, y));
        let rows = || {
            (
                vec![
                    LoadRow {
                        text: "TCTF Training".into(),
                        level: 1,
                        save_point: 0,
                    },
                    LoadRow {
                        text: "Syndicate Warehouse".into(),
                        level: 1,
                        save_point: 1,
                    },
                ],
                Some((1, 1)),
            )
        };
        cmds.into_iter()
            .filter_map(|c| m.command(d, c, &rows))
            .last()
    }

    #[test]
    fn the_main_menu_starts_games_and_resumes_as_the_original() {
        let d = data();
        // At the start: Options shows, Resume doesn't; Escape does nothing.
        let mut m = MainMenu::open(&d, false, Settings::default());
        assert!(
            m.top().unwrap().control(OPTIONS).unwrap().visible
                && !m.top().unwrap().control(RESUME).unwrap().visible
        );
        let esc = m.top_mut().unwrap().key(Key::Escape);
        assert_eq!(m.command(&d, esc[0], &|| (Vec::new(), None)), None);
        // New Game → the prompt; its faithful box off → a game as fixed.
        assert_eq!(click(&mut m, &d, 400.0, 250.0), None);
        assert_eq!(m.top().unwrap().name, "NewGame");
        // (The prompt is centred: 350 × 150 on 640 × 480 at 145, 165.)
        let (ox, oy) = (145.0, 165.0);
        assert_eq!(
            click(&mut m, &d, ox + 50.0, oy + 120.0),
            None,
            "the faithful box toggles"
        );
        assert!(m.top().unwrap().check(FAITHFUL));
        assert_eq!(
            click(&mut m, &d, ox + 60.0, oy + 90.0),
            Some(Action::NewGame { faithful: true })
        );
        // Cancel goes back to the main menu.
        click(&mut m, &d, ox + 200.0, oy + 90.0);
        assert_eq!(m.top().unwrap().name, "mainmenu");
        // Load Game: the warehouse selected (the place last played); Load.
        click(&mut m, &d, 400.0, 310.0);
        assert_eq!(
            m.top().unwrap().rows(LEVELS),
            ["TCTF Training", "Syndicate Warehouse"]
        );
        assert_eq!(
            click(&mut m, &d, 150.0, 445.0),
            Some(Action::Load {
                level: 1,
                save_point: 1
            })
        );
        // The level's row (the list focused again), then Down and Up the
        // arrows, and Enter (the default button): the level's start.
        click(&mut m, &d, 200.0, 35.0);
        m.top_mut().unwrap().key(Key::Down);
        assert_eq!(m.top().unwrap().selected_data(LEVELS), Some((1 << 8) | 1));
        m.top_mut().unwrap().key(Key::Up);
        let enter = m.top_mut().unwrap().key(Key::Enter);
        assert_eq!(
            m.command(&d, enter[0], &|| (Vec::new(), None)),
            Some(Action::Load {
                level: 1,
                save_point: 0
            })
        );
        // In a game: Resume, and Escape resumes.
        let mut m = MainMenu::open(&d, true, Settings::default());
        assert!(m.top().unwrap().control(RESUME).unwrap().visible);
        assert_eq!(click(&mut m, &d, 400.0, 370.0), Some(Action::Resume));
        let esc = m.top_mut().unwrap().key(Key::Escape);
        assert_eq!(
            m.command(&d, esc[0], &|| (Vec::new(), None)),
            Some(Action::Resume)
        );
    }

    #[test]
    fn options_change_the_settings() {
        let d = data();
        let mut m = MainMenu::open(&d, false, Settings::default());
        click(&mut m, &d, 400.0, 370.0);
        assert_eq!(m.top().unwrap().name, "options");
        assert_eq!(m.top().unwrap().slider(VOLUME), Some(100));
        // The track left of the thumb: a page down (a tenth).
        let a = click(&mut m, &d, 165.0, 225.0);
        assert!(
            matches!(a, Some(Action::Options(ref s)) if (s.overall_volume - 0.9).abs() < 1e-6),
            "{a:?}"
        );
        // The subtitles box.
        let a = click(&mut m, &d, 120.0, 340.0);
        assert!(matches!(a, Some(Action::Options(ref s)) if s.subtitles));
        // The difficulty popup: open, then Hard (the third row; 14 a row
        // without fonts).
        assert_eq!(click(&mut m, &d, 120.0, 310.0), None);
        let a = click(&mut m, &d, 120.0, 305.0 + 2.0 * 14.0 + 5.0);
        assert!(
            matches!(a, Some(Action::Options(ref s)) if s.difficulty == "hard"),
            "{a:?}"
        );
        // Cancel (Close) back to the main menu.
        click(&mut m, &d, 180.0, 410.0);
        assert_eq!(m.top().unwrap().name, "mainmenu");
    }

    #[test]
    fn a_slider_held_on_its_track_pages_on_its_timer() {
        let d = data();
        let mut w = Window::new(&d, "options");
        w.set_slider(VOLUME, 0, 100, 0);
        let value = |w: &Window| w.slider(VOLUME).unwrap();
        // (The options dialog fills the screen: its slider at 160, 215.)
        let (x, y) = (160.0 + 110.0, 225.0);
        let cmds = w.mouse_down(&d.look, 640.0, 480.0, x, y);
        assert_eq!(
            cmds,
            vec![Command {
                id: VOLUME,
                notify: Notify::NewPosition,
                button: false
            }]
        );
        assert_eq!(value(&w), 10, "a page: a tenth of the range");
        // Its timer: a page each firing, not more often than every 10/60 s.
        let t0 = w.timer + std::time::Duration::from_millis(1);
        assert_eq!(w.tick(&d.look, 640.0, 480.0, x, y, t0).len(), 1);
        assert!(
            w.tick(
                &d.look,
                640.0,
                480.0,
                x,
                y,
                t0 + std::time::Duration::from_millis(100)
            )
            .is_empty(),
            "not yet"
        );
        assert_eq!(
            w.tick(
                &d.look,
                640.0,
                480.0,
                x,
                y,
                t0 + std::time::Duration::from_millis(200)
            )
            .len(),
            1
        );
        assert_eq!(value(&w), 30);
        // Let go: no more.
        w.mouse_up(&d.look, 640.0, 480.0, x, y);
        assert!(w
            .tick(
                &d.look,
                640.0,
                480.0,
                x,
                y,
                t0 + std::time::Duration::from_secs(1)
            )
            .is_empty());
        assert_eq!(value(&w), 30);
    }
}
