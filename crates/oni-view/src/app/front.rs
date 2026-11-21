//! The out-of-game menus in the window (`docs/MENU.md`): the main menu at
//! start-up (no level loaded) and over a level (Esc), its dialogs taking
//! the mouse and keys, its music, and what it starts — a new game, a level
//! from a continue — or ends.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use winit::keyboard::KeyCode;

use super::{apply_capture, mission_level, State};
use crate::hud::menu::{self, Action, Data, Key, MainMenu};

/// The main menu's music (`OScMusicScore_Win`).
const MUSIC: &str = "main_menu_win";
/// Its fades (`OScMainMusic_FadeInTime`, `_FadeOutTime`).
const FADE: f32 = 0.5;
const TICK: f32 = 1.0 / 60.0;

/// The menus: their data, the menu up, its own sound when no level plays.
pub(super) struct Front {
    pub data: Data,
    pub menu: Option<MainMenu>,
    pub oni_root: PathBuf,
    /// The player's options (`settings.json`).
    pub user: crate::settings::Settings,
    /// The menus' sound with no level: its engine, output, real time banked.
    audio: Option<(crate::audio::Audio, Option<crate::audio::output::Output>, f32)>,
    /// What to do once the music has faded (`ONiOGU_MainMenu_FadeMusic`).
    pending: Option<(Action, Instant)>,
    /// The left button down (for drags).
    left: bool,
    /// The game began here (its end comes back to the menu).
    pub started_here: bool,
}

impl Front {
    pub fn new(oni_root: PathBuf, user: crate::settings::Settings, started_here: bool) -> Front {
        Front { data: Data::load(&oni_root.join("common")), menu: None, oni_root, user, audio: None, pending: None, left: false, started_here }
    }
}

/// A key for the dialogs.
fn dialog_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    })
}

impl State {
    pub(super) fn menu_open(&self) -> bool {
        self.front.as_ref().is_some_and(|f| f.menu.is_some())
    }

    /// The sound the menu plays through: the level's, else its own.
    fn menu_audio(&mut self) -> Option<&mut crate::audio::Audio> {
        if let Some(pw) = self.play.as_mut() {
            return Some(&mut pw.world.audio);
        }
        self.front.as_mut()?.audio.as_mut().map(|(a, _, _)| a)
    }

    /// The main menu up (`ONrOutGameUI_MainMenu_Display`,
    /// `ONiOGU_MainMenu_InitDialog`): over a level, with Resume; the
    /// sounds playing paused, music only, the menu's music fading in.
    pub(super) fn open_menu(&mut self) {
        let in_game = self.play.is_some();
        let Some(front) = self.front.as_mut() else { return };
        if !front.data.has("mainmenu") {
            eprintln!("menu: no menus (common/menus.json); reconvert the assets");
            return;
        }
        front.menu = Some(MainMenu::open(&front.data, in_game, front.user.clone()));
        if !in_game && front.audio.is_none() {
            let common = front.oni_root.join("common");
            let mut audio = crate::audio::Audio::load(&common, &common);
            let output = crate::audio::output::Output::open(audio.mixer.clone());
            audio.realtime = output.is_some();
            audio.set_volume(front.user.overall_volume);
            front.audio = Some((audio, output, 0.0));
        }
        if let Some(a) = self.menu_audio() {
            a.script_only = true;
            a.pause_channels(true);
            if a.script_start(MUSIC, 0.0) {
                a.script_volume(MUSIC, 1.0, FADE);
            }
        }
        self.keys.clear();
        self.mouse_held.clear();
        apply_capture(&self.window, false);
    }

    /// The menu gone back to the game (`ONiOGU_MainMenu_Destroy`): the music
    /// fading out as it stops (as shipped), the game's sounds on again.
    fn close_menu(&mut self) {
        if let Some(f) = self.front.as_mut() {
            f.menu = None;
        }
        if let Some(a) = self.menu_audio() {
            a.script_volume(MUSIC, 0.0, FADE);
            a.script_stop(MUSIC);
            a.script_only = false;
            a.pause_channels(false);
        }
        if self.play.is_some() {
            self.captured = true;
            apply_capture(&self.window, true);
        }
    }

    /// The menu's controls' commands, as its callbacks take them.
    fn menu_commands(&mut self, cmds: Vec<menu::Command>) {
        for cmd in cmds {
            if cmd.button {
                if let Some(a) = self.menu_audio() {
                    a.impulse_play("button_click", None, None, None);
                }
            }
            let Some(front) = self.front.as_mut() else { return };
            let rows = || crate::game::load_rows(&front.data.levels, &front.oni_root);
            let Some(menu) = front.menu.as_mut() else { return };
            let action = menu.command(&front.data, cmd, &rows);
            if let Some(a) = action {
                self.menu_action(a);
            }
        }
    }

    fn menu_action(&mut self, action: Action) {
        match action {
            Action::Options(s) => {
                s.store();
                self.tuning.invert_y = s.invert_mouse;
                self.settings.subtitles = s.subtitles;
                self.settings.difficulty = s.difficulty.clone();
                self.settings.volume = s.overall_volume;
                self.gamma = s.gamma;
                if let Some(pw) = self.play.as_mut() {
                    pw.world.hud.subtitles_on = s.subtitles;
                }
                if let Some(a) = self.menu_audio() {
                    a.set_volume(s.overall_volume);
                }
                if let Some(f) = self.front.as_mut() {
                    f.user = s;
                }
            }
            Action::Resume => self.close_menu(),
            // New game, load, quit: the music fades out first.
            a => {
                if let Some(au) = self.menu_audio() {
                    au.script_volume(MUSIC, 0.0, FADE);
                    au.script_stop(MUSIC);
                }
                if let Some(f) = self.front.as_mut() {
                    f.pending = Some((a, Instant::now() + Duration::from_secs_f32(FADE)));
                }
            }
        }
    }

    /// A key while the menu is up. True if the menu took it.
    pub(super) fn menu_key(&mut self, code: KeyCode, pressed: bool) -> bool {
        if !self.menu_open() {
            return false;
        }
        if pressed {
            if let Some(k) = dialog_key(code) {
                let cmds = self.front.as_mut().and_then(|f| f.menu.as_mut()).and_then(|m| m.top_mut()).map(|w| w.key(k)).unwrap_or_default();
                self.menu_commands(cmds);
            }
        }
        self.keys.clear();
        true
    }

    fn screen(&self) -> (f32, f32) {
        let scale = self.window.scale_factor() as f32;
        (self.config.width as f32 / scale, self.config.height as f32 / scale)
    }

    /// The left button while the menu is up. True if the menu took it.
    pub(super) fn menu_mouse(&mut self, pressed: bool) -> bool {
        if !self.menu_open() {
            return false;
        }
        let ((w, h), [x, y]) = (self.screen(), self.cursor);
        let cmds = {
            let Some(front) = self.front.as_mut() else { return true };
            front.left = pressed;
            let look = &front.data.look;
            match front.menu.as_mut().and_then(|m| m.stack.last_mut()) {
                Some(win) if pressed => win.mouse_down(look, w, h, x, y),
                Some(win) => win.mouse_up(look, w, h, x, y),
                None => Vec::new(),
            }
        };
        self.menu_commands(cmds);
        true
    }

    pub(super) fn menu_cursor(&mut self) {
        if !self.menu_open() {
            return;
        }
        let ((w, h), [x, y]) = (self.screen(), self.cursor);
        let cmds = {
            let Some(front) = self.front.as_mut() else { return };
            let (look, held) = (&front.data.look, front.left);
            front.menu.as_mut().and_then(|m| m.stack.last_mut()).map(|win| win.mouse_move(look, w, h, x, y, held)).unwrap_or_default()
        };
        self.menu_commands(cmds);
    }

    pub(super) fn menu_wheel(&mut self, lines: i32) {
        let ((w, h), [x, y]) = (self.screen(), self.cursor);
        if let Some(win) = self.front.as_mut().and_then(|f| f.menu.as_mut()).and_then(|m| m.stack.last_mut()) {
            win.wheel(w, h, x, y, lines);
        }
    }

    /// Each frame: the menus' own sound ticks by real time; once the music
    /// has faded, what was chosen is done.
    pub(super) fn front_tick(&mut self, dt: f32) {
        let no_level = self.play.is_none();
        let Some(front) = self.front.as_mut() else { return };
        if let Some((audio, _, banked)) = front.audio.as_mut().filter(|_| no_level) {
            *banked += dt;
            while *banked >= TICK {
                *banked -= TICK;
                audio.tick(None);
            }
        }
        let due = front.pending.as_ref().is_some_and(|(_, at)| Instant::now() >= *at);
        if !due {
            return;
        }
        let Some((action, _)) = front.pending.take() else { return };
        match action {
            Action::Quit => self.quit = true,
            Action::NewGame { faithful } => {
                crate::game::set_game_faithful(faithful);
                self.start_level(1, 0, None, faithful);
            }
            Action::Load { level, save_point } => {
                let saved = (save_point > 0).then(|| crate::game::load_continue(level, save_point)).flatten();
                self.start_level(level, save_point, saved, crate::game::game_faithful());
            }
            _ => {}
        }
    }

    /// A level started from the menu (`OWrLevelLoad_StartLevel`): the menu
    /// and its sound gone, any level playing unloaded.
    fn start_level(&mut self, level: u16, save_point: i64, saved: Option<crate::game::Continue>, faithful: bool) {
        let Some(front) = self.front.as_mut() else { return };
        front.menu = None;
        front.audio = None;
        let root = front.oni_root.clone();
        self.play = None;
        self.settings.faithful = faithful;
        let format = self.config.format;
        match mission_level(&self.gpu, format, &root, level, None, &self.settings, save_point, saved) {
            Ok((r, pw)) => {
                self.renderer = r;
                self.play = Some(pw);
                self.captured = true;
                apply_capture(&self.window, true);
                eprintln!("game: level {level}, save point {save_point}{}", if faithful { " (faithful)" } else { "" });
            }
            Err(e) => {
                eprintln!("game: can't load level {level}: {e}");
                self.open_menu();
            }
        }
    }

    /// The menu drawn over everything.
    pub(super) fn menu_overlay(&self) -> Option<Vec<crate::render::OverlayDraw>> {
        let front = self.front.as_ref()?;
        let menu = front.menu.as_ref()?;
        let (w, h) = self.screen();
        Some(menu.draw(&front.data, w, h))
    }
}
