//! The pause screen (F1; `ONrPauseScreen_Display`, `docs/HUD.md`, *Pause
//! screen*): a modal page viewer with five sections — the level's
//! objectives (the current one green, earlier ones red), the items and the
//! weapons the player has met (unlocked for good), the diary (earlier
//! levels' pages and this level's) and the help — each paged with Previous
//! and Next, a section with something new flashing yellow. The game stands
//! still while it is up; F1 again (after a second) or Close shuts it.

use std::path::Path;
use std::time::Instant;

use super::console::{self, button_draws, lines_draws, page_draws, Page};
use super::text::Fonts;
use super::{rect, PartSpec};
use crate::render::OverlayDraw;

/// The sections, in their buttons' order (101–105).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Objective,
    Items,
    Weapons,
    Diary,
    Help,
}

const TABS: [(Tab, i64); 5] = [
    (Tab::Objective, 101),
    (Tab::Items, 102),
    (Tab::Weapons, 103),
    (Tab::Diary, 104),
    (Tab::Help, 105),
];
/// `WPcPowerup_LSI`: read about only while carried.
const LSI: u32 = 5;
const GREEN: u32 = 0xFF00_FF00;
const RED: u32 = 0xFFFF_0000;
const WHITE: u32 = 0xFFFF_FFFF;
const ORANGE: u32 = 0xFFFF_7F00;
const YELLOW: u32 = 0xFFFF_FF00;
const GREY: u32 = 0xFF80_8080;

#[derive(Clone, Debug)]
struct Item {
    id: i64,
    title: String,
    rect: [f32; 4],
    font: (u16, u8),
    /// Its font's colour (ARGB).
    shade: u32,
}

#[derive(Clone, Debug)]
struct Diary {
    level: i64,
    page: i64,
    new_move: bool,
    content: Page,
}

/// What the pause screen can show (`common/pause_screen.json`, the level's
/// `pause_pages.json`).
#[derive(Clone, Debug, Default)]
pub struct Data {
    size: [f32; 2],
    items: Vec<Item>,
    help: Vec<Page>,
    item_pages: Vec<(u32, Page)>,
    weapons: Vec<(String, Page)>,
    diary: Vec<Diary>,
    objectives: Vec<Page>,
}

fn read(path: &Path) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

impl Data {
    pub fn load(common: &Path, level_dir: &Path) -> Data {
        let c = read(&common.join("pause_screen.json"));
        let l = read(&level_dir.join("pause_pages.json"));
        let pages = |v: &serde_json::Value| {
            v.as_array()
                .into_iter()
                .flatten()
                .map(console::page)
                .collect::<Vec<_>>()
        };
        let items = c["dialog"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| Item {
                id: i["id"].as_i64().unwrap_or(-1),
                title: i["title"].as_str().unwrap_or("").to_string(),
                rect: std::array::from_fn(|k| i["rect"][k].as_f64().unwrap_or(0.0) as f32),
                font: (
                    i["font"]["size"].as_u64().unwrap_or(10) as u16,
                    i["font"]["style"].as_u64().unwrap_or(1) as u8,
                ),
                shade: i["font"]["shade"]
                    .as_str()
                    .and_then(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
                    .unwrap_or(0xFFFF_7F00),
            })
            .collect();
        let item_pages = c["items"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(l["items"].as_array().into_iter().flatten())
            .map(|i| {
                (
                    i["type"].as_u64().unwrap_or(0) as u32,
                    console::page(&i["page"]),
                )
            })
            .collect();
        let mut diary: Vec<Diary> = c["diary"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| Diary {
                level: d["level"].as_i64().unwrap_or(0),
                page: d["page"].as_i64().unwrap_or(0),
                new_move: d["newMove"].as_bool().unwrap_or(false),
                content: console::page(&d["content"]),
            })
            .collect();
        diary.sort_by_key(|d| (d.level, d.page));
        Data {
            size: [
                c["dialog"]["size"][0].as_f64().unwrap_or(640.0) as f32,
                c["dialog"]["size"][1].as_f64().unwrap_or(480.0) as f32,
            ],
            items,
            help: pages(&c["help"]),
            item_pages,
            weapons: c["weapons"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|w| {
                    (
                        w["class"].as_str().unwrap_or("").to_string(),
                        console::page(&w["page"]),
                    )
                })
                .collect(),
            diary,
            objectives: pages(&l["objectives"]),
        }
    }

    fn item(&self, id: i64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Whether this level has diary pages teaching a new move (its diary
    /// flashes on arriving, `ONiPS_DiaryPage_Init`).
    pub fn new_moves(&self, level: i64) -> bool {
        self.diary.iter().any(|d| d.level == level && d.new_move)
    }
}

/// What the player has, for the pause screen.
pub struct Player<'a> {
    pub level: i64,
    /// The objective number (`objective_set`; 0: none yet).
    pub objective: i64,
    pub has_lsi: bool,
    /// Its current gun's class (the weapons page starts on it).
    pub weapon: Option<&'a str>,
    /// The item kinds and weapon classes met (unlocked).
    pub items_seen: u32,
    pub weapons_seen: &'a [String],
    /// What is new (their sections flash; the pages start on them).
    pub new_objective: bool,
    pub new_item: Option<u32>,
    pub new_weapon: Option<&'a str>,
    pub new_move: bool,
}

/// The pause screen's state (for tests): its section, the page showing
/// and how many, the page's strings and hint, Previous / Next enabled.
pub struct State {
    pub tab: Tab,
    pub page: usize,
    pub pages: usize,
    pub text: Vec<String>,
    pub hint: Vec<String>,
    pub previous: bool,
    pub next: bool,
}

/// The pause screen up.
#[derive(Clone, Debug)]
pub struct Pause {
    pub tab: Tab,
    objective_number: i64,
    objective_page: i64,
    items: Vec<usize>,
    item_page: usize,
    weapons: Vec<usize>,
    weapon_page: usize,
    diary: Vec<usize>,
    diary_page: usize,
    help_page: usize,
    /// The sections flashing (bits by `Tab as u32`).
    highlighted: u32,
    opened: Instant,
    /// A cheat's message, shown in the hint area instead of the hint while
    /// the screen is up (`ONrPauseScreen_OverrideMessage`).
    pub message: Option<&'static str>,
}

impl Pause {
    /// Opened (`ONiPauseScreen_InitDialog`): on the objectives, the other
    /// sections' pages chosen as the original chooses them.
    pub fn open(data: &Data, p: &Player<'_>) -> Pause {
        // Items: one page per kind met (the LSI only while carried).
        let mut kinds = 0u32;
        let mut items = Vec::new();
        let mut item_page = 0;
        for (k, (kind, _)) in data.item_pages.iter().enumerate() {
            let bit = 1 << kind;
            if kinds & bit != 0 {
                continue;
            }
            let enabled = if *kind == LSI {
                p.has_lsi
            } else {
                p.items_seen & bit != 0
            };
            if enabled {
                if p.new_item == Some(*kind) {
                    item_page = items.len();
                }
                items.push(k);
                kinds |= bit;
            }
        }
        // Weapons: those met; on the new one, else the one in hand.
        let weapons: Vec<usize> = (0..data.weapons.len())
            .filter(|&k| {
                p.weapons_seen
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(&data.weapons[k].0))
            })
            .collect();
        let want = p.new_weapon.or(p.weapon);
        let weapon_page = want
            .and_then(|w| {
                weapons
                    .iter()
                    .position(|&k| data.weapons[k].0.eq_ignore_ascii_case(w))
            })
            .unwrap_or(0);
        // Diary: earlier levels' pages and this level's; on its first page.
        let diary: Vec<usize> = (0..data.diary.len())
            .filter(|&k| data.diary[k].level <= p.level)
            .collect();
        let diary_page = diary
            .iter()
            .position(|&k| data.diary[k].level == p.level && data.diary[k].page == 1)
            .or_else(|| diary.iter().position(|&k| data.diary[k].level >= p.level))
            .unwrap_or(0);
        let mut highlighted = 0;
        for (on, tab) in [
            (p.new_objective, Tab::Objective),
            (p.new_weapon.is_some(), Tab::Weapons),
            (p.new_item.is_some(), Tab::Items),
            (p.new_move, Tab::Diary),
        ] {
            if on {
                highlighted |= 1 << tab as u32;
            }
        }
        Pause {
            tab: Tab::Objective,
            objective_number: p.objective,
            objective_page: 0,
            items,
            item_page,
            weapons,
            weapon_page,
            diary,
            diary_page,
            help_page: 0,
            highlighted,
            opened: Instant::now(),
            message: None,
        }
    }

    /// F1 again closes it — once it has been up a second
    /// (`ONcPauseScreen_KeyAcceptDelay`).
    pub fn may_close(&self) -> bool {
        self.opened.elapsed().as_secs_f32() >= 1.0
    }

    fn max_objective(&self, data: &Data) -> i64 {
        (data.objectives.len() as i64)
            .min(self.objective_number)
            .max(0)
    }

    /// The section's pages and the one showing.
    pub fn pages(&self, data: &Data) -> (usize, usize) {
        match self.tab {
            Tab::Objective => (
                self.max_objective(data) as usize,
                self.objective_page as usize,
            ),
            Tab::Items => (self.items.len(), self.item_page),
            Tab::Weapons => (self.weapons.len(), self.weapon_page),
            Tab::Diary => (self.diary.len(), self.diary_page),
            Tab::Help => (data.help.len(), self.help_page),
        }
    }

    /// Previous / Next enabled (`ONiPS_EnableButtons`): the objectives run
    /// newest first, Previous going back to older ones.
    pub fn buttons(&self, data: &Data) -> (bool, bool) {
        let (n, k) = self.pages(data);
        match self.tab {
            Tab::Objective if data.objectives.is_empty() || self.objective_number == 0 => {
                (false, false)
            }
            Tab::Objective => (k + 1 < n, k > 0),
            Tab::Help => (false, false),
            _ if n == 0 => (false, false),
            _ => (k > 0, k + 1 < n),
        }
    }

    /// The page showing, and the shade for strings without their own.
    pub fn page<'d>(&self, data: &'d Data) -> Option<(&'d Page, u32)> {
        match self.tab {
            Tab::Objective => {
                if self.objective_number == 0 {
                    return None;
                }
                let k = (self.objective_number - self.objective_page - 1).max(0) as usize;
                let shade = if self.objective_page == 0 { GREEN } else { RED };
                data.objectives.get(k).map(|p| (p, shade))
            }
            Tab::Items => self
                .items
                .get(self.item_page)
                .map(|&k| (&data.item_pages[k].1, ORANGE)),
            Tab::Weapons => self
                .weapons
                .get(self.weapon_page)
                .map(|&k| (&data.weapons[k].1, ORANGE)),
            Tab::Diary => self
                .diary
                .get(self.diary_page)
                .map(|&k| (&data.diary[k].content, ORANGE)),
            Tab::Help => data.help.get(self.help_page).map(|p| (p, ORANGE)),
        }
    }

    /// A button (by its title or id): a section, Previous, Next or Close.
    /// Returns false if it closes the screen.
    pub fn press(&mut self, data: &Data, id: i64) -> bool {
        if let Some(&(tab, _)) = TABS.iter().find(|(_, i)| *i == id) {
            self.tab = tab;
            return true;
        }
        let (prev, next) = self.buttons(data);
        match id {
            106 if prev => self.step(data, -1),
            107 if next => self.step(data, 1),
            2 => return false,
            _ => {}
        }
        true
    }

    fn step(&mut self, data: &Data, d: i64) {
        let clamp =
            |k: usize, n: usize| (k as i64 + d).clamp(0, n.saturating_sub(1) as i64) as usize;
        match self.tab {
            // (Objectives: Previous is one older.)
            Tab::Objective => {
                self.objective_page =
                    (self.objective_page - d).clamp(0, (self.max_objective(data) - 1).max(0))
            }
            Tab::Items => self.item_page = clamp(self.item_page, self.items.len()),
            Tab::Weapons => self.weapon_page = clamp(self.weapon_page, self.weapons.len()),
            Tab::Diary => self.diary_page = clamp(self.diary_page, self.diary.len()),
            Tab::Help => self.help_page = clamp(self.help_page, data.help.len()),
        }
    }

    /// The section showing is read: it stops flashing.
    pub fn mark_read(&mut self) {
        self.highlighted &= !(1 << self.tab as u32);
    }

    /// Where the dialog sits on a `w` × `h` screen.
    fn at(data: &Data, w: f32, h: f32) -> [f32; 2] {
        [
            ((w - data.size[0]) * 0.5).floor().max(0.0),
            ((h - data.size[1]) * 0.5).floor().max(0.0),
        ]
    }

    /// The button under screen point `x`, `y`, if one is showing there.
    pub fn hit(&self, data: &Data, w: f32, h: f32, x: f32, y: f32) -> Option<i64> {
        let at = Pause::at(data, w, h);
        let help = self.tab == Tab::Help;
        data.items
            .iter()
            .filter(|i| matches!(i.id, 101..=105 | 2) || (matches!(i.id, 106 | 107) && !help))
            .find_map(|i| {
                let [rx, ry, rw, rh] = i.rect;
                let (l, t) = (at[0] + rx, at[1] + ry);
                (x >= l && x < l + rw && y >= t && y < t + rh).then_some(i.id)
            })
    }

    /// The screen (`ONiPauseScreen_Paint`, `_DrawItem`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        data: &Data,
        fonts: &Fonts,
        part: &dyn Fn(&str) -> Option<PartSpec>,
        textures: &Path,
        texture_size: &dyn Fn(&Path) -> Option<[f32; 2]>,
        w: f32,
        h: f32,
    ) -> Vec<OverlayDraw> {
        let mut out = Vec::new();
        let at = Pause::at(data, w, h);
        let place = |r: [f32; 4]| [at[0] + r[0], at[1] + r[1], r[2], r[3]];
        match part("ps_oniUI_background") {
            Some(p) => out.extend(p.draw(at, data.size[0], data.size[1])),
            None => out.push(rect(
                None,
                [at[0], at[1], at[0] + data.size[0], at[1] + data.size[1]],
                [0.0, 0.0, 0.0, 0.85],
            )),
        }
        let help = self.tab == Tab::Help;
        // The outlines (the hint area's hidden on the help page).
        for i in data
            .items
            .iter()
            .filter(|i| (i.id == 0) || (i.id == 120 && !help))
        {
            if let Some(p) = part("ps_oniUI_outline") {
                let [x, y, rw, rh] = place(i.rect);
                out.extend(p.draw([x, y], rw, rh));
            }
        }
        // The buttons: the section's white, a new one flashing yellow, the
        // rest orange; Previous / Next grey when they can't go.
        let t = (self.opened.elapsed().as_secs_f32() * 60.0 + 180.0) % 90.0;
        let flash = if t < 60.0 {
            mix(ORANGE, YELLOW, (std::f32::consts::PI * t / 60.0).sin())
        } else {
            ORANGE
        };
        let (prev, next) = self.buttons(data);
        for i in &data.items {
            let shade = match i.id {
                101..=105 => {
                    let tab = TABS.iter().find(|(_, id)| *id == i.id).map(|(t, _)| *t);
                    if tab == Some(self.tab) {
                        WHITE
                    } else if tab.is_some_and(|t| self.highlighted & (1 << t as u32) != 0) {
                        flash
                    } else {
                        ORANGE
                    }
                }
                106 if !help => {
                    if prev {
                        ORANGE
                    } else {
                        GREY
                    }
                }
                107 if !help => {
                    if next {
                        ORANGE
                    } else {
                        GREY
                    }
                }
                2 => ORANGE,
                _ => continue,
            };
            button_draws(
                &mut out,
                part,
                place(i.rect),
                &i.title,
                fonts,
                i.font,
                shade,
                matches!(i.id, 101..=105)
                    && TABS.iter().any(|&(t, id)| id == i.id && t == self.tab),
            );
        }
        // The page in the main area, its hint below.
        if let (Some((page, shade)), Some(main)) = (self.page(data), data.item(110)) {
            out.extend(page_draws(
                page,
                place(main.rect),
                fonts,
                part,
                textures,
                texture_size,
                shade,
                self.tab == Tab::Diary,
            ));
            if let (false, Some(sub), None) = (help, data.item(111), self.message) {
                lines_draws(&mut out, &page.hint, place(sub.rect), 0.0, fonts, shade);
            }
        }
        // A cheat's message there instead, in the area's own font (on any
        // page).
        if let (Some(m), Some(sub)) = (self.message, data.item(111)) {
            let line = console::Line {
                text: m.to_string(),
                size: sub.font.0,
                style: sub.font.1,
                shade: Some(sub.shade),
            };
            lines_draws(&mut out, &[line], place(sub.rect), 0.0, fonts, sub.shade);
        }
        out
    }
}

/// Shade `a` toward `b` by `t` (0–1), ARGB.
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch =
        |s: u32| -> f32 { ((a >> s) & 0xFF) as f32 * (1.0 - t) + ((b >> s) & 0xFF) as f32 * t };
    0xFF00_0000 | ((ch(16) as u32) << 16) | ((ch(8) as u32) << 8) | ch(0) as u32
}
