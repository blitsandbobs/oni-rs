//! The HUD (`Oni_InGameUI.c`; behaviour per `docs/HUD.md`): the two rings at
//! the bottom corners — weapon, ammo, energy cells and the compass on the
//! left; health, shield, phase cloak, hypos and the hit ring on the right —
//! the message area between them, subtitles, the console strip, the
//! letterbox and screen fades. The state lives in the world (scripts and
//! play change it each tick); [`Hud::draw`] lays a frame out as overlay
//! triangles.

pub mod cinematic;
pub mod text;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use glam::Vec3;

use crate::render::OverlayDraw;
use text::{Align, Fonts, TextArea};

/// The HUD's elements (`ONcIGUIElement_*`), as the scripts name them.
pub const ELEMENTS: [&str; 12] = ["health", "damage", "invis", "shield", "hypo", "weapon", "ammo", "compass", "arrow", "ballistic", "energy", "lsi"];
mod el {
    pub const HEALTH: u32 = 1 << 0;
    pub const DAMAGE: u32 = 1 << 1;
    pub const INVIS: u32 = 1 << 2;
    pub const SHIELD: u32 = 1 << 3;
    pub const HYPO: u32 = 1 << 4;
    pub const WEAPON: u32 = 1 << 5;
    pub const AMMO: u32 = 1 << 6;
    pub const COMPASS: u32 = 1 << 7;
    pub const ARROW: u32 = 1 << 8;
    pub const BALLISTIC: u32 = 1 << 9;
    pub const ENERGY: u32 = 1 << 10;
    pub const LSI: u32 = 1 << 11;
}

/// The most clips, cells and hypos the rings show (`WPcMax*`), the shield's
/// full charge and the cloak's full time (ticks).
const MAX_AMMO: u16 = 6;
const MAX_CELLS: u16 = 6;
const MAX_HYPOS: u16 = 6;
pub const MAX_SHIELD: u16 = 100;
pub const MAX_INVISIBILITY: u16 = 60 * 30;
/// Below this share of health (%), the health ring throbs and the hypo
/// prompt shows (`ONcIGUI_HealthWarnLevel`).
const HEALTH_WARN: i32 = 30;
/// The hit ring fades this much a tick (of 255).
pub const HITS_FADE: u8 = 3;
/// How long a new objective's and a new weapon's messages stay (ticks), and
/// how often an unread objective is prompted again (ticks).
const NEW_OBJECTIVE_FADE: u32 = 240;
const NEW_WEAPON_FADE: u32 = 240;
const PROMPT_EVERY: u64 = 30 * 60;
/// Autoprompts are looked at this often (ticks).
const AUTOPROMPT_EVERY: u64 = 12;
/// The letterbox's depth (at 480 lines) and its slide a tick.
const LETTERBOX_DEPTH: f32 = 75.0;
const LETTERBOX_STEP: f32 = 1.25;
/// Two compass targets this close are the same one.
const SAME_TARGET: f32 = 10.0;

const DEG: f32 = std::f32::consts::PI / 180.0;

/// A meter that shows where it was a moment ago (`ONtInGame_DamageMeter`):
/// a fall leaves a fading after-shadow, a coming rise a fore-shadow.
#[derive(Clone, Copy, Debug, Default)]
struct Meter {
    value: u32,
    target: u32,
    fore: bool,
    after: bool,
    shadow: f32,
}

impl Meter {
    /// One tick (`ONiInGameUI_UpdateDamageMeter`).
    fn update(&mut self, value: u32, target: u32, decay: f32) {
        if self.after {
            self.shadow -= decay;
            if self.shadow <= self.value as f32 {
                self.after = false;
            }
        }
        if target > value {
            self.after = false;
            self.fore = true;
            if self.target != target {
                self.target = target;
                self.shadow = target as f32;
            }
        } else {
            self.target = value;
            self.fore = false;
        }
        if value > self.value {
            self.after = false;
        } else if value < self.value && !self.after && !self.fore {
            self.after = true;
            self.shadow = self.value as f32;
        }
        self.value = value;
    }
}

/// The compass's target (`target_set`): where, and the distances its arc
/// spans (squared).
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub at: Vec3,
    max_d2: f32,
    min_d2: f32,
}

/// A screen fade (`fade_out` / `fade_in`): from one colour and opacity to
/// another over a span of ticks.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    start: u64,
    end: u64,
    from: [f32; 4],
    to: [f32; 4],
}

/// What the HUD shows of the player this tick (gathered by the world).
#[derive(Clone, Debug, Default)]
pub struct Player {
    pub hp: i32,
    /// Health a hypo still has to give (the meter's fore-shadow).
    pub hypo_remaining: u16,
    pub max_hp: i32,
    pub boosted: f32,
    pub health_shade: u32,
    pub shadow_shade: u32,
    pub hits: [u8; 4],
    pub ammo: u16,
    pub cells: u16,
    pub hypos: u16,
    pub shield: u16,
    pub invisibility: u16,
    pub lsi: bool,
    /// The countdown's display this frame (None: none, or blinked off), and
    /// whether one runs at all (the LSI icon dims meanwhile).
    pub timer: Option<String>,
    pub timer_on: bool,
    pub feet: Vec3,
    /// Its aim (radians; what the compass turns with).
    pub facing: f32,
    /// The gun in hand (else the one holstered, for the in-gun ring):
    /// rounds, the most it holds, its HUD textures; whether it is in hand.
    pub gun: Option<Gun>,
}

#[derive(Clone, Debug, Default)]
pub struct Gun {
    pub rounds: u16,
    pub max: u16,
    pub in_hand: bool,
    pub icon: Option<String>,
    pub empty: Option<String>,
    pub fill: Option<String>,
}

/// The HUD's state (`Oni_InGameUI.c`'s globals, the console's text areas,
/// the letterbox and fade).
#[derive(Clone, Debug)]
pub struct Hud {
    textures: PathBuf,
    /// The cutscene inserts showing (`hud/cinematic.rs`), and their frame
    /// (`cinematic_border` in `part_specs.json`).
    pub cinematics: cinematic::Cinematics,
    border: Option<PartSpec>,
    /// The textures flagged additive (`texture_flags.json`).
    additive: std::collections::HashSet<String>,
    fonts: Fonts,
    texture_sizes: HashMap<String, [f32; 2]>,
    /// The message tables (`messages`, `subtitles`), by lowercase name.
    pub texts: HashMap<String, String>,
    pub subtitle_texts: Subtitles,
    /// Which level (the autoprompts' ranges).
    pub level: u16,
    /// The rings shown (`ui_show_element left/right`).
    pub show: [bool; 2],
    /// Elements drawn full (`ui_fill_element`) and flashing (`ui_flash_element`).
    pub filled: u32,
    pub flashing: u32,
    flash_start: u64,
    health: Meter,
    shield: Meter,
    pub target: Option<Target>,
    pub objective: i64,
    /// An objective not yet read: its message comes back every 30 s (F1
    /// reads it).
    pub new_objective: bool,
    immediate_prompt: bool,
    last_prompt: u64,
    /// The message area, the in-game subtitles and the console strip.
    pub messages: TextArea,
    pub subtitles: TextArea,
    pub console: TextArea,
    /// Show dialogue subtitles (the game's option; off by default where
    /// there is sound).
    pub subtitles_on: bool,
    /// How far the letterbox has slid in (at 480 lines high).
    pub letterbox: f32,
    pub fade: Option<Fade>,
    /// The autoprompt up now (a message name).
    autoprompt: Option<String>,
    /// The autoprompts: event, levels, message.
    autoprompts: Vec<(String, [u16; 2], String)>,
    /// Weapon classes the player has had (their message shows once).
    pub weapons_seen: Vec<String>,
    /// The tick (for flashing, prompts and fades).
    pub now: u64,
    /// When a save was last restored.
    restored_at: Option<u64>,
}

impl Default for Hud {
    fn default() -> Self {
        Hud {
            textures: PathBuf::new(),
            cinematics: Default::default(),
            border: None,
            additive: Default::default(),
            fonts: Fonts::default(),
            texture_sizes: HashMap::new(),
            texts: HashMap::new(),
            subtitle_texts: Subtitles::default(),
            level: 0,
            show: [true, true],
            filled: 0,
            flashing: 0,
            flash_start: 0,
            health: Meter::default(),
            shield: Meter::default(),
            target: None,
            objective: 0,
            new_objective: false,
            immediate_prompt: false,
            last_prompt: 0,
            // (Message area: centred, 12 at 480 lines; subtitles: left, 7;
            // the console strip: left, 7, fading what overflows.)
            messages: TextArea::new(13, Align::Center, true, 12, false),
            subtitles: TextArea::new(13, Align::Left, true, 7, false),
            console: TextArea::new(4, Align::Left, false, 7, true),
            subtitles_on: false,
            letterbox: 0.0,
            fade: None,
            autoprompt: None,
            autoprompts: Vec::new(),
            weapons_seen: Vec::new(),
            now: 0,
            restored_at: None,
        }
    }
}

/// The dialogue subtitles (`SUBT subtitles`), in the table's order.
#[derive(Clone, Debug, Default)]
pub struct Subtitles(Vec<(String, String)>);

impl Subtitles {
    /// From `messages.json`'s `subtitles`: `[key, text]` pairs.
    pub fn from_json(v: &serde_json::Value) -> Subtitles {
        Subtitles(
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string())))
                .collect(),
        )
    }

    /// A line's subtitle (`SSiSubtitleArray_FindByNumber`): the first entry,
    /// in the table's order, whose key the name begins with (ignoring case)
    /// — a numbered key (`01_02_50`) against the name after its first letter
    /// (`c01_02_50griffin`), any other (`brain07final`) against the whole
    /// name. (First, not best: a key that is the start of another wins if it
    /// comes first, as the shipped table has it once.)
    pub fn find(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| {
                let n = if k.starts_with(|c: char| c.is_ascii_digit()) { name.get(1..).unwrap_or("") } else { name };
                n.get(..k.len()).is_some_and(|p| p.eq_ignore_ascii_case(k))
            })
            .map(|(_, t)| t.as_str())
    }
}

impl Hud {
    /// The HUD for a level: its textures, fonts and message tables from
    /// `common`.
    pub fn load(common: &Path, level: u16) -> Hud {
        let doc: serde_json::Value = std::fs::read_to_string(common.join("messages.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let table = |k: &str| -> HashMap<String, String> {
            doc[k].as_object().map(|m| m.iter().map(|(n, v)| (n.to_ascii_lowercase(), v.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default()
        };
        let settings: serde_json::Value = std::fs::read_to_string(common.join("game_settings.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let autoprompts = settings["autoprompts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                let l = |k: usize| a["levels"][k].as_u64().unwrap_or(0) as u16;
                (a["event"].as_str().unwrap_or("").to_string(), [l(0), l(1)], a["message"].as_str().unwrap_or("").to_string())
            })
            .collect();
        let flags: serde_json::Value = std::fs::read_to_string(common.join("texture_flags.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let additive = flags["additive"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect();
        let specs: serde_json::Value = std::fs::read_to_string(common.join("part_specs.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let border = PartSpec::from_json(&specs["partSpecs"]["cinematic_border"], &common.join("textures"));
        Hud {
            textures: common.join("textures"),
            border,
            additive,
            fonts: Fonts::load(common),
            texts: table("messages"),
            subtitle_texts: Subtitles::from_json(&doc["subtitles"]),
            level,
            autoprompts,
            ..Hud::default()
        }
    }

    /// The health meter's shadow (what its colour follows while it fades).
    pub fn health_shadow(&self) -> f32 {
        if self.health.after { self.health.shadow } else { self.health.value as f32 }
    }

    /// A message's text by name, as the game looks it up (`SSrMessage_Find`).
    pub fn message_text(&self, name: &str) -> Option<&str> {
        self.texts.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    /// A dialogue line's subtitle, by the line's name (`SSrSubtitle_Find`;
    /// see [`Subtitles::find`]).
    pub fn subtitle_text(&self, name: &str) -> Option<&str> {
        self.subtitle_texts.find(name)
    }

    /// `message name [ticks]` (`AIiScript_Message`): from the messages table
    /// into the message area; else from the subtitles into the console
    /// strip. False if it is in neither.
    pub fn message(&mut self, name: &str, ticks: u32) -> bool {
        if let Some(t) = self.message_text(name).map(str::to_string) {
            self.messages.print(&t, Some(name), ticks);
            true
        } else if let Some(t) = self.subtitle_texts.find(name.get(1..).unwrap_or("")).map(str::to_string) {
            // (The subtitles, by the name without its first letter — which
            // the lookup then strips again for a numbered entry, so a
            // numbered line is never found this way; as shipped.)
            self.console.print(&t, Some(name), ticks);
            true
        } else {
            false
        }
    }

    /// `message_remove [name]`: that message (all, without one) fades.
    pub fn message_remove(&mut self, name: Option<&str>) {
        self.messages.fade(name);
    }

    /// A dialogue line's subtitle (`OSrSubtitle_Draw`), with the option on:
    /// in the console strip while letterboxed, else the subtitle area.
    pub fn subtitle(&mut self, line: &str, ticks: u32, letterboxed: bool) {
        if !self.subtitles_on {
            return;
        }
        let Some(t) = self.subtitle_text(line).map(str::to_string) else { return };
        let tag = (ticks == 0).then_some("dialog");
        if letterboxed {
            self.console.print(&t, tag, ticks);
        } else {
            self.subtitles.print(&t, tag, ticks);
        }
    }

    /// The dialogue is over: its subtitles (printed for as long as it
    /// plays) fade.
    pub fn subtitles_done(&mut self) {
        for area in [&mut self.subtitles, &mut self.console] {
            for e in &mut area.entries {
                if e.timer == 0 && e.tag.as_deref() == Some("dialog") {
                    e.timer = text::IMMEDIATE_FADE;
                }
            }
        }
    }

    /// `objective_set n [silent]` (`ONiInGameUI_ObjectiveSet`): a new one,
    /// unless silent, is prompted. Returns whether it is new.
    pub fn objective_set(&mut self, n: i64, silent: bool) -> bool {
        if n == self.objective {
            return false;
        }
        self.objective = n;
        // (Just after a restore: silently.)
        let silent = silent || self.restored_at.is_some_and(|t| self.now < t + 300);
        if !silent {
            self.new_objective = true;
            self.immediate_prompt = true;
        }
        true
    }

    /// A save was restored (`ONrInGameUI_NotifyRestoreGame`): no prompting
    /// for the objective it had, and objectives set in the next 5 s are set
    /// silently.
    pub fn restored(&mut self) {
        self.new_objective = false;
        self.messages.fade(Some("new_objective"));
        self.restored_at = Some(self.now);
    }

    /// The player read the objective (the pause screen's objective page —
    /// F1 here): no more prompting.
    pub fn read_objective(&mut self) {
        self.new_objective = false;
        self.messages.fade(Some("new_objective"));
        self.messages.fade(Some("new_weapon"));
    }

    /// `target_set flag distance` (`ONiInGameUI_TargetSet`): the compass
    /// points at `at`; its arc is widest within `distance` and narrowest at
    /// the distance it is set from. A distance of 0 turns it off. Returns
    /// whether it changed (the compass sound plays).
    pub fn target_set(&mut self, at: Option<Vec3>, distance: f32, from: Vec3) -> bool {
        if distance == 0.0 {
            self.target = None;
            return false;
        }
        let Some(at) = at else {
            self.target = None;
            return true;
        };
        if self.target.is_some_and(|t| t.at.distance_squared(at) < SAME_TARGET * SAME_TARGET) {
            return false;
        }
        self.target = Some(Target { at, max_d2: (at - from).length_squared(), min_d2: distance * distance });
        true
    }

    /// `ui_fill_element` / `ui_flash_element name on`: bits by name (`all`).
    pub fn element_mask(name: &str) -> u32 {
        if name == "all" {
            return (1 << ELEMENTS.len()) - 1;
        }
        ELEMENTS.iter().position(|e| *e == name).map_or(0, |k| 1 << k)
    }

    /// `fade_out r g b ticks`.
    pub fn fade_out(&mut self, rgb: [f32; 3], ticks: u32) {
        self.fade = Some(Fade { start: self.now, end: self.now + u64::from(ticks), from: [rgb[0], rgb[1], rgb[2], 0.0], to: [rgb[0], rgb[1], rgb[2], 1.0] });
    }

    /// `fade_in ticks`: back from where a fade got to (no fade: nothing).
    pub fn fade_in(&mut self, ticks: u32) {
        if let Some(f) = &mut self.fade {
            f.from = f.to;
            f.to[3] = 0.0;
            f.start = self.now;
            f.end = self.now + u64::from(ticks);
        }
    }

    /// The fade's opacity now (0 without one).
    pub fn fade_alpha(&self) -> f32 {
        self.fade_now().map_or(0.0, |c| c[3])
    }

    /// The fade's colour now (RGBA), if one is on (`ONi_GameState_Display_FadeInOut`):
    /// eased by the 1.9th power of the time gone.
    fn fade_now(&self) -> Option<[f32; 4]> {
        let f = self.fade?;
        let span = f.end.saturating_sub(f.start) as f32;
        let t = if self.now < f.end && span > 0.0 { ((self.now.min(f.end) - f.start) as f32 / span).powf(1.9) } else { 1.0 };
        Some(std::array::from_fn(|k| f.from[k] * (1.0 - t) + f.to[k] * t))
    }

    /// A new weapon class in the player's hand: its message, the first time
    /// (`ONrInGameUI_NewWeapon`).
    pub fn new_weapon(&mut self, instance: &str) {
        if self.weapons_seen.iter().any(|w| w == instance) {
            return;
        }
        self.weapons_seen.push(instance.to_string());
        self.messages.fade(Some("new_weapon"));
        if let Some(t) = self.message_text(instance).map(str::to_string) {
            self.messages.print(&t, Some("new_weapon"), NEW_WEAPON_FADE);
        }
    }

    /// One tick (`ONrInGameUI_Update`, the text areas' update, the
    /// letterbox): the meters follow the player, an unread objective is
    /// prompted (returns the sound to play), printed lines run down, the
    /// letterbox slides.
    pub fn tick(&mut self, p: &Player, letterbox: bool, in_cutscene: bool) -> Option<&'static str> {
        self.now += 1;
        if p.max_hp > 0 {
            let hp = p.hp.max(0) as u32;
            self.health.update(hp, hp + u32::from(p.hypo_remaining), p.max_hp as f32 / 180.0);
        }
        self.shield.update(u32::from(p.shield), u32::from(p.shield), f32::from(MAX_SHIELD) / 180.0);
        let mut sound = None;
        if self.new_objective && (self.immediate_prompt || self.now.saturating_sub(self.last_prompt) > PROMPT_EVERY) {
            if !in_cutscene && self.now >= 30 {
                sound = Some(if self.immediate_prompt { "objectiveNew" } else { "objectivePrompt" });
            }
            if !self.messages.visible("new_objective") {
                if let Some(t) = self.message_text("new_objective").map(str::to_string) {
                    self.messages.print(&t, Some("new_objective"), NEW_OBJECTIVE_FADE);
                }
            }
            self.immediate_prompt = false;
            self.last_prompt = self.now;
        }
        self.messages.tick();
        self.subtitles.tick();
        self.console.tick();
        self.letterbox = if letterbox { (self.letterbox + LETTERBOX_STEP).min(LETTERBOX_DEPTH) } else { (self.letterbox - LETTERBOX_STEP).max(0.0) };
        if let Some(c) = self.fade_now() {
            if c[3] <= 0.0 && self.now >= self.fade.map_or(0, |f| f.end) {
                self.fade = None;
            }
        }
        sound
    }

    /// Whether it is time to look at the autoprompt again.
    pub fn autoprompt_due(&self) -> bool {
        self.now.is_multiple_of(AUTOPROMPT_EVERY)
    }

    /// The autoprompt for what the player could do now — events in rising
    /// priority, the last that has a message on this level wins
    /// (`ONrGameState_UpdateAutoPrompt`) — put up in place of the last.
    pub fn autoprompt(&mut self, events: &[&str]) {
        let mut message = None;
        for e in events {
            if let Some((_, _, m)) = self.autoprompts.iter().find(|(ev, l, _)| ev.eq_ignore_ascii_case(e) && (l[0]..=l[1]).contains(&self.level)) {
                message = Some(m.clone());
            }
        }
        if message == self.autoprompt {
            return;
        }
        self.messages.fade(Some("autoprompt"));
        self.autoprompt = message.clone();
        if let Some(t) = message.and_then(|m| self.message_text(&m).map(str::to_string)) {
            self.messages.print(&t, Some("autoprompt"), 0);
        }
    }

    fn texture(&self, name: &str) -> PathBuf {
        self.textures.join(format!("{name}.png"))
    }

    /// A texture's size (loaded once).
    fn size_of(&mut self, name: &str) -> [f32; 2] {
        if let Some(s) = self.texture_sizes.get(name) {
            return *s;
        }
        let s = std::fs::File::open(self.texture(name))
            .ok()
            .and_then(|f| png::Decoder::new(std::io::BufReader::new(f)).read_info().ok().map(|r| [r.info().width as f32, r.info().height as f32]))
            .unwrap_or([128.0, 128.0]);
        self.texture_sizes.insert(name.to_string(), s);
        s
    }

    /// The flashing elements' opacity now (`gFlashAlpha`): a pulse each
    /// second that fades fast.
    fn flash_alpha(&mut self) -> f32 {
        if self.new_objective || self.flashing != 0 {
            (-0.03 * ((self.now - self.flash_start) % 60) as f32).exp()
        } else {
            self.flash_start = self.now;
            1.0
        }
    }

    /// The frame's overlay, for a screen `w` × `h` (in the HUD's pixels).
    pub fn draw(&mut self, p: Option<&Player>, w: f32, h: f32, letterboxed: bool) -> Vec<OverlayDraw> {
        let mut out = Vec::new();
        // The letterbox's bars.
        if self.letterbox > 0.0 {
            let d = self.letterbox * h / 480.0;
            out.push(rect(None, [0.0, 0.0, w, d], [0.0, 0.0, 0.0, 1.0]));
            out.push(rect(None, [0.0, h - d, w, h], [0.0, 0.0, 0.0, 1.0]));
        }
        if let (Some(p), false) = (p, letterboxed) {
            let from = out.len();
            self.rings(&mut out, p, w, h);
            // (Textures flagged additive add to the frame.)
            for d in &mut out[from..] {
                let stem = d.texture.as_ref().and_then(|t| t.file_stem()).and_then(|s| s.to_str()).unwrap_or("");
                d.additive = self.additive.contains(stem);
            }
        }
        let fade = self.fade_now();
        if let Some(c) = fade.filter(|c| c[3] > 0.0) {
            out.push(rect(None, [0.0, 0.0, w, h], c));
        }
        // The messages (subtitles in their place while any are up), between
        // the rings; not while letterboxed or fading.
        if !letterboxed && self.fade.is_none() {
            let bounds = [148, 0, (w as i32 - 148).max(178), h as i32 - 2];
            let line = self.fonts.best(7).map_or(13, |f| f.line_height());
            let bounds = [bounds[0], (bounds[3] - line * 10).max(2), bounds[2], bounds[3]];
            let fonts = self.fonts.clone();
            let area = if self.subtitles.entries.is_empty() { &mut self.messages } else { &mut self.subtitles };
            let laid = area.layout(&fonts, bounds, h);
            out.extend(text_draws(&fonts, &laid));
        }
        // The console strip, in the bottom letterbox band.
        let bounds = [2, h as i32 - (h as i32 * 60) / 480, w as i32 - 2, h as i32];
        let fonts = self.fonts.clone();
        let laid = self.console.layout(&fonts, bounds, h);
        out.extend(text_draws(&fonts, &laid));
        // The cutscene inserts, over everything (`OCrCinematic_Draw`): each
        // picture, then its frame 3 pixels out all round.
        for (c, r) in self.cinematics.rects(w, h) {
            let mut d = rect(Some(c.texture.clone()), r, [1.0, 1.0, 1.0, 1.0]);
            if c.mirror {
                for v in &mut d.tris {
                    v[2] = 1.0 - v[2];
                }
            }
            out.push(d);
            if let Some(b) = &self.border {
                out.extend(b.draw([r[0] - 3.0, r[1] - 3.0], (r[2] - r[0]).round() + 6.0, (r[3] - r[1]).round() + 6.0));
            }
        }
        out
    }

    /// The texture named `name` for a cutscene insert, and its size (None:
    /// no such texture — the original then shows nothing).
    pub fn cinematic_texture(&mut self, name: &str) -> Option<(PathBuf, [f32; 2])> {
        let path = self.texture(name);
        let path = if path.exists() {
            path
        } else {
            // (Names are looked up regardless of case.)
            let want = format!("{}.png", name.to_ascii_lowercase());
            std::fs::read_dir(&self.textures).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase() == want))?
        };
        let stem = path.file_stem()?.to_str()?.to_string();
        let size = self.size_of(&stem);
        Some((path, size))
    }

    /// The two rings (`ONrInGameUI_Display`).
    fn rings(&mut self, out: &mut Vec<OverlayDraw>, p: &Player, w: f32, h: f32) {
        let flash = self.flash_alpha();
        let fl = |m: u32, s: &Self| if s.flashing & m != 0 { flash } else { 1.0 };
        let filled = self.filled;
        let left = self.size_of("left");
        let right = self.size_of("right");
        let lc = [10.0 + (left[0] as i32 / 2) as f32, h - (left[1] as i32 / 2) as f32];
        let rc = [w - 10.0 - (right[0] as i32 / 2) as f32, h - (right[1] as i32 / 2) as f32];
        let white = [1.0, 1.0, 1.0, 1.0];
        let lring = Some(self.texture("leftRing"));
        let rring = Some(self.texture("rightRing"));
        let ring_w = self.size_of("leftRing")[0];
        let rring_w = self.size_of("rightRing")[0];
        if self.show[0] {
            out.push(sprite(Some(self.texture("left")), lc, left, false, white));
            // Ballistic clips and energy cells.
            let clips = if filled & el::BALLISTIC != 0 { MAX_AMMO } else { p.ammo.min(MAX_AMMO) };
            if clips > 0 {
                out.push(arc(lring.clone(), lc, 64.0, 51.5, ring_w, 180.0 * DEG, 270.0 * DEG, u32::from(MAX_AMMO), u32::from(clips), [1.0, 1.0, 1.0, fl(el::BALLISTIC, self)]));
            }
            let cells = if filled & el::ENERGY != 0 { MAX_CELLS } else { p.cells.min(MAX_CELLS) };
            if cells > 0 {
                out.push(arc(lring.clone(), lc, 64.0, 51.5, ring_w, 180.0 * DEG, 90.0 * DEG, u32::from(MAX_CELLS), u32::from(cells), [1.0, 1.0, 1.0, fl(el::ENERGY, self)]));
            }
            self.compass(out, p, lc, lring.clone(), ring_w, flash);
            // The gun's rounds: its empty ring, the full one by the share left.
            if let Some(g) = &p.gun {
                if let Some(empty) = &g.empty {
                    out.push(sprite(Some(self.texture(empty)), lc, left, false, white));
                    let rounds = if filled & el::AMMO != 0 { g.max } else { g.rounds };
                    if let (true, Some(fill)) = (rounds > 0 && g.max > 0, &g.fill) {
                        let share = f32::from(rounds) / f32::from(g.max);
                        let (s, e) = (15.0 * DEG, 345.0 * DEG);
                        let fw = self.size_of(fill)[0];
                        out.push(arc(Some(self.texture(fill)), lc, 44.0, 30.0, fw, s, s + (e - s) * share, 24, 24, [1.0, 1.0, 1.0, fl(el::AMMO, self)]));
                    }
                }
                if let (true, Some(icon)) = (g.in_hand, &g.icon) {
                    out.push(sprite(Some(self.texture(icon)), lc, [54.0, 54.0], false, [1.0, 1.0, 1.0, fl(el::WEAPON, self)]));
                }
            }
        }
        if self.show[1] {
            out.push(sprite(Some(self.texture("right")), rc, right, false, white));
            self.health_rings(out, p, rc, rring.clone(), rring_w, flash);
            // The phase cloak's time.
            let invis = if filled & el::INVIS != 0 { MAX_INVISIBILITY } else { p.invisibility };
            if invis > 0 {
                let (s, e) = (-2.0 * DEG, -89.0 * DEG);
                let share = (f32::from(invis) / f32::from(MAX_INVISIBILITY)).min(1.0);
                out.push(arc(rring.clone(), rc, 63.0, 51.0, rring_w, s, s + (e - s) * share, 8, 8, [1.0, 1.0, 1.0, fl(el::INVIS, self)]));
            }
            // The shield, with its shadow.
            let (s, e) = (2.0 * DEG, 89.0 * DEG);
            let m = self.shield;
            if filled & el::SHIELD != 0 {
                out.push(arc(rring.clone(), rc, 63.0, 51.0, rring_w, s, e, 8, 8, [1.0, 1.0, 1.0, fl(el::SHIELD, self)]));
            } else {
                let share = m.value as f32 / f32::from(MAX_SHIELD);
                if m.value > 0 {
                    out.push(arc(rring.clone(), rc, 63.0, 51.0, rring_w, s, s + (e - s) * share.min(1.0), 8, 8, [1.0, 1.0, 1.0, fl(el::SHIELD, self)]));
                }
                if m.fore || m.after {
                    let to = (m.shadow / f32::from(MAX_SHIELD)).min(1.0);
                    // (As shipped, a flashing shield's shadow takes a
                    // garbled opacity: the flash over its own alpha.)
                    let a = if self.flashing & el::SHIELD != 0 { (((flash * 255.0).powi(2) / 127.0) as u32 & 0xFF) as f32 / 255.0 } else { 127.0 / 255.0 };
                    out.push(arc(rring.clone(), rc, 63.0, 51.0, rring_w, s + (e - s) * share, s + (e - s) * to, 8, 8, [1.0, 1.0, 1.0, a]));
                }
            }
            // Hypos, one segment each.
            let hypos = if filled & el::HYPO != 0 { MAX_HYPOS } else { p.hypos.min(MAX_HYPOS) };
            const HYPO_ARCS: [(f32, f32); 6] = [(92.0, 103.0), (107.0, 118.0), (123.0, 133.0), (137.0, 148.0), (152.0, 163.0), (167.0, 179.0)];
            for &(s, e) in HYPO_ARCS.iter().take(usize::from(hypos)) {
                out.push(arc(rring.clone(), rc, 63.0, 51.0, rring_w, s * DEG, e * DEG, 1, 1, [1.0, 1.0, 1.0, fl(el::HYPO, self)]));
            }
            // The hit ring: where the last hits came from.
            const HIT_ARCS: [(f32, f32); 4] = [(-45.0, 45.0), (45.0, 135.0), (135.0, 225.0), (225.0, 315.0)];
            for (k, &(s, e)) in HIT_ARCS.iter().enumerate() {
                let a = if filled & el::DAMAGE != 0 { 255 } else { p.hits[k] };
                if a > 0 {
                    out.push(arc(rring.clone(), rc, 37.0, 30.0, rring_w, s * DEG, e * DEG, 4, 4, [1.0, 1.0, 1.0, f32::from(a) / 255.0 * fl(el::DAMAGE, self)]));
                }
            }
            if p.lsi {
                // (A third as bright while a countdown runs.)
                let dim = if p.timer_on { 1.0 / 3.0 } else { 1.0 };
                out.push(sprite(Some(self.texture("lsi_icon")), rc, [54.0, 54.0], false, [1.0, 1.0, 1.0, fl(el::LSI, self) * dim]));
            }
            // The countdown (`ONrGameState_Timer_Display`): the default
            // font, bold, white, centred on the ring's middle.
            if let Some(t) = p.timer.as_deref() {
                if let Some(font) = self.fonts.fonts.iter().find(|f| f.size == 7 && f.style == "bold").cloned() {
                    out.push(timer_draw(&font, t, rc));
                }
            }
        }
    }

    /// The health ring (`ONiInGameUI_DisplayHealth`): the health in its
    /// colour along 270°, past full (a hypo's boost) on an inner ring, the
    /// shadow of where it was, and a throb below 30 %.
    fn health_rings(&mut self, out: &mut Vec<OverlayDraw>, p: &Player, rc: [f32; 2], tex: Option<PathBuf>, tw: f32, flash: f32) {
        let (hs, he) = (180.0 * DEG, -90.0 * DEG);
        let (ss, se) = (-90.0 * DEG, -170.0 * DEG);
        let m = self.health;
        let max = p.max_hp.max(1) as f32;
        let mut arcs: Vec<(f32, f32, f32, f32, bool)> = Vec::new(); // start, end, outer, inner, main
        if self.filled & el::HEALTH != 0 {
            arcs.push((hs, he, 50.0, 40.0, true));
            arcs.push((ss, se, 55.0, 40.0, true));
        } else {
            let share = m.value as f32 / max;
            let mut boost = 0.0;
            if m.value > 0 {
                arcs.push((hs, hs + (he - hs) * share.min(1.0), 50.0, 40.0, true));
                if share > 1.0 {
                    boost = share.min(p.boosted) - 1.0;
                    arcs.push((ss, ss + (se - ss) * boost, 55.0, 40.0, true));
                }
            }
            if m.fore || m.after {
                let shadow = m.shadow / max;
                if share <= 1.0 {
                    arcs.push((hs + share * (he - hs), hs + shadow.min(1.0) * (he - hs), 50.0, 40.0, false));
                }
                if shadow > 1.0 {
                    let start = if boost > 0.0 { ss + boost * (se - ss) } else { ss };
                    arcs.push((start, ss + (se - ss) * (shadow.min(p.boosted) - 1.0), 55.0, 40.0, false));
                }
            }
        }
        let shade = if m.after { p.shadow_shade } else { p.health_shade };
        let rgb = [((shade >> 16) & 0xFF) as f32 / 255.0, ((shade >> 8) & 0xFF) as f32 / 255.0, (shade & 0xFF) as f32 / 255.0];
        let mut main = 1.0;
        let mut shadow = if m.after { 127.0 / 255.0 } else { 80.0 / 255.0 };
        if self.flashing & el::HEALTH != 0 {
            main *= flash;
            shadow *= flash;
        }
        if !m.fore {
            let low = (p.hp as f32 / max) / (HEALTH_WARN as f32 / 100.0);
            if low < 1.0 {
                let period = 40.0 + low * (140.0 - 40.0);
                let flash_period = 40.0 + low * (75.0 - 40.0);
                let t = (self.now as f32) % period;
                if t < flash_period {
                    let amount = (0.5 + low * (0.3 - 0.5)) * (std::f32::consts::PI * t / flash_period).sin();
                    main *= 1.0 - amount;
                }
            }
        }
        for (s, e, o, i, is_main) in arcs {
            out.push(arc(tex.clone(), rc, o, i, tw, s, e, 24, 24, [rgb[0], rgb[1], rgb[2], if is_main { main } else { shadow }]));
        }
    }

    /// The compass (`ONiInGameUI_DisplayRadar`): an arc toward the target,
    /// wide near it and narrow far off, and an arrow if it is well above or
    /// below; the whole ring flashing for an unread objective with none.
    fn compass(&mut self, out: &mut Vec<OverlayDraw>, p: &Player, lc: [f32; 2], tex: Option<PathBuf>, tw: f32, flash: f32) {
        let alpha = if self.new_objective || self.flashing & el::COMPASS != 0 { flash } else { 1.0 };
        let mut to = Vec3::ZERO;
        let ring = if self.filled & el::COMPASS != 0 {
            Some((0.0, std::f32::consts::TAU))
        } else if let Some(t) = self.target {
            to = t.at - p.feet;
            let d2 = to.length_squared();
            let len = if d2 < t.min_d2 {
                std::f32::consts::PI
            } else if d2 > t.max_d2 {
                20.0 * DEG
            } else {
                std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_2 * (d2 / t.max_d2.max(1e-6))
            }
            .clamp(20.0 * DEG, std::f32::consts::PI);
            let toward = to.x.atan2(to.z);
            let mut a = p.facing - toward;
            a = (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            Some((a - len + std::f32::consts::FRAC_PI_2 * 3.0, a + len + std::f32::consts::FRAC_PI_2 * 3.0))
        } else if self.new_objective {
            Some((0.0, std::f32::consts::TAU))
        } else {
            None
        };
        if let Some((s, e)) = ring {
            out.push(arc(tex, lc, 50.0, 46.0, tw, s, e, 32, 32, [1.0, 1.0, 1.0, alpha]));
        }
        let alpha = if self.flashing & el::ARROW != 0 { flash } else { 1.0 };
        let (mut up, down) = match self.target {
            Some(_) => (to.y > 10.0, to.y < -10.0),
            None => (false, false),
        };
        if !down && self.filled & el::ARROW != 0 {
            up = true;
        }
        if up || down {
            let c = [lc[0] + 36.5, lc[1]];
            out.push(sprite(Some(self.texture("updn_arrow")), c, [12.0, 12.0], down, [1.0, 1.0, 1.0, alpha]));
        }
    }
}

/// A screen rectangle (left, top, right, bottom) of a texture's whole.
/// A part specification (`PSpc`, `PSrPartSpec_Draw`): a frame cut from a
/// texture into nine — corners kept their size, edges and middle stretched.
#[derive(Clone, Debug)]
pub struct PartSpec {
    texture: PathBuf,
    /// Each slice's texel corners, `[column][row]`: left, top, right, bottom.
    slices: [[[f32; 4]; 3]; 3],
    size: [f32; 2],
}

impl PartSpec {
    fn from_json(v: &serde_json::Value, textures: &std::path::Path) -> Option<PartSpec> {
        let name = v["texture"].as_str()?;
        let texture = textures.join(format!("{name}.png"));
        let f = std::fs::File::open(&texture).ok()?;
        let r = png::Decoder::new(std::io::BufReader::new(f)).read_info().ok()?;
        let size = [r.info().width as f32, r.info().height as f32];
        let p = |k: &str, i: usize| -> [f32; 2] { [v[k][i][0].as_f64().unwrap_or(0.0) as f32, v[k][i][1].as_f64().unwrap_or(0.0) as f32] };
        let mut slices = [[[0.0; 4]; 3]; 3];
        for (c, column) in slices.iter_mut().enumerate() {
            for (r, s) in column.iter_mut().enumerate() {
                let (tl, br) = (p("tl", c * 3 + r), p("br", c * 3 + r));
                *s = [tl[0], tl[1], br[0], br[1]];
            }
        }
        Some(PartSpec { texture, slices, size })
    }

    /// The frame drawn `width` × `height` from `at`: the corners at their
    /// own size, the middles filling between (the corners' sizes cut down
    /// when the two don't fit, as the original reckons the middles — the
    /// corners themselves still drawn whole).
    pub fn draw(&self, at: [f32; 2], width: f32, height: f32) -> Vec<OverlayDraw> {
        let s = &self.slices;
        let (lw, rw) = ((s[0][0][2] - s[0][0][0]).abs(), (s[2][0][2] - s[2][0][0]).abs());
        let (th, bh) = ((s[0][0][3] - s[0][0][1]).abs(), (s[0][2][3] - s[0][2][1]).abs());
        let fit = |a: f32, b: f32, total: f32| if a + b > total { ((a * a / total).trunc(), (b * b / total).trunc()) } else { (a, b) };
        let (l2, r2) = fit(lw, rw, width);
        let (t2, b2) = fit(th, bh, height);
        let (mw, mh) = ((width - (l2 + r2)).max(0.0), (height - (t2 + b2)).max(0.0));
        let xs = [(at[0], lw), (at[0] + lw, mw), (at[0] + lw + mw, rw)];
        let ys = [(at[1], th), (at[1] + th, mh), (at[1] + th + mh, bh)];
        let mut out = Vec::new();
        for (c, &(x, w)) in xs.iter().enumerate() {
            for (r, &(y, h)) in ys.iter().enumerate() {
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let [u0, v0, u1, v1] = s[c][r];
                let (u0, v0, u1, v1) = (u0 / self.size[0], v0 / self.size[1], u1 / self.size[0], v1 / self.size[1]);
                let (l, t, rr, b) = (x, y, x + w, y + h);
                out.push(OverlayDraw {
                    texture: Some(self.texture.clone()),
                    tris: vec![[l, t, u0, v0], [rr, t, u1, v0], [rr, b, u1, v1], [l, t, u0, v0], [rr, b, u1, v1], [l, b, u0, v1]],
                    color: [1.0, 1.0, 1.0, 1.0],
                    additive: false,
                });
            }
        }
        out
    }
}

fn rect(texture: Option<PathBuf>, r: [f32; 4], color: [f32; 4]) -> OverlayDraw {
    let [l, t, rr, b] = r;
    OverlayDraw { texture, tris: vec![[l, t, 0.0, 0.0], [rr, t, 1.0, 0.0], [rr, b, 1.0, 1.0], [l, t, 0.0, 0.0], [rr, b, 1.0, 1.0], [l, b, 0.0, 1.0]], color, additive: false }
}

/// A sprite `size` big centred on `c` (its top-left on whole pixels, as the
/// original's halving does), upside down if `flip`.
fn sprite(texture: Option<PathBuf>, c: [f32; 2], size: [f32; 2], flip: bool, color: [f32; 4]) -> OverlayDraw {
    let (l, t) = (c[0] - (size[0] * 0.5).floor(), c[1] - (size[1] * 0.5).floor());
    let mut d = rect(texture, [l, t, l + size[0], t + size[1]], color);
    if flip {
        for v in &mut d.tris {
            v[3] = 1.0 - v[3];
        }
    }
    d
}

/// The first `draw` of an arc's `segments` (`ONiInGameUI_MakeArc`,
/// `_DrawArc`): a band between two radii from one angle to another (screen
/// angles, y down), textured by where it lies on a ring texture `tex_w`
/// wide.
#[allow(clippy::too_many_arguments)]
fn arc(texture: Option<PathBuf>, c: [f32; 2], outer: f32, inner: f32, tex_w: f32, start: f32, end: f32, segments: u32, draw: u32, color: [f32; 4]) -> OverlayDraw {
    let half = (tex_w as i32 >> 1).max(1) as f32;
    let (uo, ui) = (outer / half, inner / half);
    let step = (end - start) / segments as f32;
    let point = |k: u32, r: f32, ur: f32| {
        let th = start + k as f32 * step;
        let (s, co) = th.sin_cos();
        [co * r + c[0], s * r + c[1], co * 0.5 * ur + 0.5, s * 0.5 * ur + 0.5]
    };
    let mut tris = Vec::new();
    for k in 0..draw.min(segments) {
        let (i0, o0, o1, i1) = (point(k, inner, ui), point(k, outer, uo), point(k + 1, outer, uo), point(k + 1, inner, ui));
        tris.extend_from_slice(&[i0, o0, o1, i0, o1, i1]);
    }
    OverlayDraw { texture, tris, color, additive: false }
}

/// Text segments as glyph cells: a drop shadow a pixel down and right
/// (25 % grey), then the text in its colour.
/// The countdown's text, centred across `center`, its baseline half the
/// plain font's ascent below it (as `ONrGameState_Timer_Display` places it).
fn timer_draw(font: &text::Font, s: &str, center: [f32; 2]) -> OverlayDraw {
    let width: i32 = s.chars().filter_map(|c| font.glyph(c)).map(|g| g[3]).sum();
    let mut x = (center[0] - (width / 2) as f32).floor();
    let top = (center[1] + (font.ascending / 2) as f32 - font.ascending as f32).floor();
    let (aw, ah) = (font.atlas_size[0], font.atlas_size[1]);
    let mut tris = Vec::new();
    for ch in s.chars() {
        let Some([gx, gy, gw, adv]) = font.glyph(ch) else { continue };
        let (cw, chh) = (gw as f32, font.cell_height() as f32);
        let (u0, v0, u1, v1) = (gx as f32 / aw, gy as f32 / ah, (gx as f32 + cw) / aw, (gy as f32 + chh) / ah);
        tris.extend_from_slice(&[[x, top, u0, v0], [x + cw, top, u1, v0], [x + cw, top + chh, u1, v1], [x, top, u0, v0], [x + cw, top + chh, u1, v1], [x, top + chh, u0, v1]]);
        x += adv as f32;
    }
    OverlayDraw { texture: Some(font.atlas.clone()), tris, color: [1.0, 1.0, 1.0, 1.0], additive: false }
}

fn text_draws(fonts: &Fonts, laid: &[(text::Segment, f32, String)]) -> Vec<OverlayDraw> {
    let mut out = Vec::new();
    for (shadow, (dx, rgb)) in [(true, (1.0, 0x404040)), (false, (0.0, 0))] {
        for (s, alpha, atlas) in laid {
            let Some(font) = fonts.fonts.iter().find(|f| f.atlas.display().to_string() == *atlas) else { continue };
            let color = if shadow { rgb } else { s.color };
            let mut tris = Vec::new();
            let mut x = s.x as f32 + dx;
            let y = s.top as f32 + dx;
            let (aw, ah) = (font.atlas_size[0], font.atlas_size[1]);
            for ch in s.text.chars() {
                let Some([gx, gy, gw, adv]) = font.glyph(ch) else { continue };
                let (cw, chh) = (gw as f32, font.cell_height() as f32);
                let (u0, v0, u1, v1) = (gx as f32 / aw, gy as f32 / ah, (gx as f32 + cw) / aw, (gy as f32 + chh) / ah);
                tris.extend_from_slice(&[[x, y, u0, v0], [x + cw, y, u1, v0], [x + cw, y + chh, u1, v1], [x, y, u0, v0], [x + cw, y + chh, u1, v1], [x, y + chh, u0, v1]]);
                x += adv as f32;
            }
            let rgbf = [((color >> 16) & 0xFF) as f32 / 255.0, ((color >> 8) & 0xFF) as f32 / 255.0, (color & 0xFF) as f32 / 255.0];
            out.push(OverlayDraw { texture: Some(font.atlas.clone()), tris, color: [rgbf[0], rgbf[1], rgbf[2], *alpha], additive: false });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitles_match_as_the_original_finds_them() {
        let t = Subtitles::from_json(&serde_json::json!([
            ["00_01_113", "line 113"],
            ["brain07final", "Deadly Brain: hello"],
            ["00_01_11", "line 11"],
            ["gift_field", "field"],
            ["gift_fields", "fields"],
        ]));
        // A numbered key against the name after its first letter.
        assert_eq!(t.find("c00_01_113shinatama"), Some("line 113"));
        assert_eq!(t.find("C00_01_11Shinatama"), Some("line 11"));
        // Any other key against the whole name.
        assert_eq!(t.find("brain07final"), Some("Deadly Brain: hello"));
        // The first in the table's order, not the best.
        assert_eq!(t.find("gift_fields"), Some("field"));
        assert_eq!(t.find("nothing"), None);
    }

    #[test]
    fn a_fall_leaves_a_shadow_that_fades() {
        let mut m = Meter::default();
        m.update(100, 100, 1.0);
        m.update(60, 60, 1.0);
        assert!(m.after && m.shadow == 100.0, "{m:?}");
        for _ in 0..40 {
            m.update(60, 60, 1.0);
        }
        assert!(!m.after, "gone once it reaches the value: {m:?}");
    }

    #[test]
    fn an_arc_draws_its_first_segments() {
        let a = arc(None, [0.0, 0.0], 64.0, 51.5, 128.0, 0.0, std::f32::consts::FRAC_PI_2, 6, 2, [1.0; 4]);
        assert_eq!(a.tris.len(), 2 * 6);
        // Its first corner: the inner radius at angle 0, the texture at the
        // matching place on the ring.
        assert!((a.tris[0][0] - 51.5).abs() < 1e-4 && (a.tris[0][2] - (0.5 + 0.5 * 51.5 / 64.0)).abs() < 1e-4);
    }

    #[test]
    fn objectives_prompt_until_read() {
        let mut h = Hud::default();
        h.texts.insert("new_objective".into(), "New objective received.".into());
        assert!(h.objective_set(1, false));
        let p = Player { hp: 100, max_hp: 100, ..Player::default() };
        for _ in 0..40 {
            h.tick(&p, false, false);
        }
        assert!(h.messages.visible("new_objective"));
        h.read_objective();
        assert!(!h.new_objective);
        assert!(!h.objective_set(1, false), "the same one again: nothing");
    }
}
