//! Scenario functions: the HUD, pause screen, text consoles and main menu.

use super::*;

fn act(a: Option<crate::hud::menu::Action>) -> Option<String> {
    use crate::hud::menu::Action;
    a.map(|a| match a {
        Action::NewGame { faithful } => {
            if faithful {
                "new_game faithful".into()
            } else {
                "new_game".into()
            }
        }
        Action::Load { level, save_point } => format!("load {level} {save_point}"),
        Action::Resume => "resume".into(),
        Action::Quit => "quit".into(),
        Action::Options(_) => "options".into(),
        Action::Display(_) => "display".into(),
    })
}
fn run(host: &mut ScriptHost, cmds: Vec<crate::hud::menu::Command>) -> Option<String> {
    let root = host.oni_root.clone();
    let (data, m) = host.menu.as_mut()?;
    let rows = || crate::game::load_rows(&data.levels, &root);
    let mut out = None;
    for c in cmds {
        out = act(m.command(data, c, &rows)).or(out);
    }
    out
}

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // pause_toggle(): F1 (the pause screen opened, or closed once up a
    // second).
    r.def("pause_toggle", |_, host, ()| {
        host.world.pause_toggle();
        Ok(())
    })?;

    // The pause screen → {tab, page (from 1), pages, lines, hint, previous,
    // next}, or nil when it is not up.
    r.def("pause_screen", |lua, host, ()| {
        let Some(s) = host.world.hud.pause_state() else {
            return Ok(None);
        };
        let t = lua.create_table()?;
        t.set("tab", format!("{:?}", s.tab).to_ascii_lowercase())?;
        t.set("page", s.page + 1)?;
        t.set("pages", s.pages)?;
        t.set("lines", s.text)?;
        t.set("hint", s.hint)?;
        t.set("previous", s.previous)?;
        t.set("next", s.next)?;
        Ok(Some(t))
    })?;

    // cheats(on): typing the cheat codes allowed (as once the game is won).
    r.def("cheats", |_, host, on: bool| {
        host.world.cheats.allowed = on;
        Ok(())
    })?;

    // Clicks a pause-screen button: "objective", "items", "weapons", "diary",
    // "help", "previous", "next" or "close" (errors on any other).
    r.def("pause_press", |_, host, button: String| {
        let id = match button.as_str() {
            "objective" => 101,
            "items" => 102,
            "weapons" => 103,
            "diary" => 104,
            "help" => 105,
            "previous" => 106,
            "next" => 107,
            "close" => 2,
            _ => {
                return Err(mlua::Error::runtime(format!(
                    "no pause screen button {button}"
                )))
            }
        };
        host.world.pause_press(id, true);
        Ok(())
    })?;

    // text_console() → {name, page, pages, last, lines, button} or nil: the
    // data console showing its text (page from 1; lines: the page's strings;
    // button: its Next / Close button's {x, y, w, h} on a 640 × 480 screen).
    r.def("text_console", |lua, host, ()| {
        let Some(c) = host.world.hud.text_console.as_ref() else {
            return Ok(None);
        };
        let t = lua.create_table()?;
        t.set("name", c.name.clone())?;
        t.set("page", c.page + 1)?;
        t.set("pages", c.pages.len())?;
        t.set("last", c.last())?;
        t.set(
            "lines",
            c.pages
                .get(c.page)
                .map(|p| p.text.iter().map(|l| l.text.clone()).collect::<Vec<_>>())
                .unwrap_or_default(),
        )?;
        if let Some(b) = host.world.hud.text_console_button(640.0, 480.0) {
            t.set("button", b.to_vec())?;
        }
        Ok(Some(t))
    })?;

    // Presses a key on the data console's text: "next" (Space) turns the page;
    // anything else (Esc) closes it.
    r.def("text_console_key", |_, host, key: String| {
        let hud = &mut host.world.hud;
        match key.as_str() {
            "next" => hud.text_console_next(),
            _ => hud.text_console_close(),
        }
        Ok(())
    })?;

    // Clicks the left button at screen x, y (640 × 480) on the data console's
    // text → whether it was on its Next / Close button.
    r.def("text_console_click", |_, host, (x, y): (f32, f32)| {
        let world = &mut host.world;
        let hit = world.hud.text_console_hit(640.0, 480.0, x, y);
        if hit {
            world.text_console_click();
        }
        Ok(hit)
    })?;

    // The HUD: the lines in its message area, subtitles and
    // console strip; the objective (and whether it is unread); the compass
    // target; the letterbox's depth; the fade's opacity; the elements filled
    // and flashing; the rings shown; the player's hit ring.
    r.def("hud", |lua, host, ()| {
        let hud = &host.world.hud;
        let t = lua.create_table()?;
        let lines = |a: &crate::hud::text::TextArea| {
            a.entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>()
        };
        t.set("messages", lines(&hud.messages))?;
        t.set("subtitles", lines(&hud.subtitles))?;
        t.set("console", lines(&hud.console))?;
        t.set("objective", hud.objective)?;
        t.set("new_objective", hud.new_objective)?;
        if let Some(tg) = hud.target {
            t.set("target", vec![tg.at.x, tg.at.y, tg.at.z])?;
        }
        t.set("letterbox", hud.letterbox)?;
        t.set("fade", hud.fade_alpha())?;
        t.set("filled", hud.filled)?;
        t.set("flashing", hud.flashing)?;
        t.set("show", vec![hud.show[0], hud.show[1]])?;
        t.set(
            "hits",
            host.world
                .fighters
                .first()
                .map(|p| p.hits.to_vec())
                .unwrap_or_default(),
        )?;
        Ok(t)
    })?;

    // Whether the HUD's help overlay shows (the pause screen's Help page,
    // ui_show_help).
    r.def("hud_help", |_, host, ()| Ok(host.world.hud.help_on))?;

    // How many overlay triangles the HUD draws now (on a 640 × 480 screen).
    r.def("hud_triangles", |_, host, ()| {
        let draws = host.world.hud_draw(640.0, 480.0);
        Ok(draws.iter().map(|d| d.tris.len() / 3).sum::<usize>())
    })?;

    // console_screen(id) → the texture console `id`'s screen shows now (by
    // its state), or nil.
    r.def("console_screen", |_, host, id: u16| {
        Ok(host
            .world
            .consoles
            .iter()
            .find(|c| c.id == id)
            .map(crate::world::World::screen_of))
    })?;

    // set_gamma(g): the gamma option for frames drawn (0–1; 0.5 as drawn).
    r.def("set_gamma", |_, host, v: f32| {
        host.renderer.gamma = v;
        Ok(())
    })?;

    // The main menu: brings it up (`in_game`: the in-game
    // menu, with Resume); screenshots then draw it.
    r.def("menu_open", |_, host, in_game: Option<bool>| {
        let data = crate::hud::menu::Data::load(&host.oni_root.join("common"));
        if !data.has("mainmenu") {
            return Err(mlua::Error::external("no menus (common/menus.json)"));
        }
        let mut m = crate::hud::menu::MainMenu::open(
            &data,
            in_game.unwrap_or(false),
            crate::settings::Settings::default(),
        );
        m.quirks = host.world.quirks;
        host.menu = Some((data, m));
        Ok(())
    })?;

    // Takes the main menu down.
    r.def("menu_close", |_, host, ()| {
        host.menu = None;
        Ok(())
    })?;

    // The menu dialog on top → {dialog, depth, rows, selected, visible = {id =
    // title}, checks = {id = on}, sliders = {id = value}, popups = {id = item}},
    // or nil when none is up.
    r.def("menu_state", |lua, host, ()| {
        let Some((_, m)) = &host.menu else {
            return Ok(None);
        };
        let Some(w) = m.top() else { return Ok(None) };
        let t = lua.create_table()?;
        t.set("dialog", w.name.clone())?;
        t.set("depth", m.stack.len())?;
        let (checks, sliders, popups, visible) = (
            lua.create_table()?,
            lua.create_table()?,
            lua.create_table()?,
            lua.create_table()?,
        );
        for c in &w.controls {
            if c.visible {
                visible.set(c.def.id, c.def.title.clone())?;
            }
            match &c.ctl {
                crate::hud::menu::Ctl::Check(on) => checks.set(c.def.id, *on)?,
                crate::hud::menu::Ctl::Slider { value, .. } => sliders.set(c.def.id, *value)?,
                crate::hud::menu::Ctl::Popup { .. } => popups.set(c.def.id, w.popup(c.def.id))?,
                crate::hud::menu::Ctl::List { .. } => {
                    t.set("rows", w.rows(c.def.id))?;
                    t.set(
                        "selected",
                        w.selected_data(c.def.id)
                            .map(|d| (d & 0xFF, d >> 8))
                            .map(|(l, s)| format!("{l} {s}")),
                    )?;
                }
                _ => {}
            }
        }
        t.set("visible", visible)?;
        t.set("checks", checks)?;
        t.set("sliders", sliders)?;
        t.set("popups", popups)?;
        Ok(Some(t))
    })?;

    // Clicks control `id` of the menu dialog on top (near its middle) → what the
    // menu asks for ("new_game", "new_game faithful", "load L S", "resume",
    // "quit", "options"), or nil; errors if the control isn't showing.
    r.def("menu_click", |_, host, id: i64| {
        let (w, hh) = (640.0, 480.0);
        let cmds = {
            let Some((data, m)) = host.menu.as_mut() else {
                return Ok(None);
            };
            let Some(win) = m.top_mut() else {
                return Ok(None);
            };
            let Some(c) = win.control(id).filter(|c| c.visible) else {
                return Err(mlua::Error::external(format!("no control {id} showing")));
            };
            let at = win.at(w, hh);
            let [x, y, cw, ch] = c.def.rect;
            let (px, py) = (at[0] + x + (cw * 0.5).min(20.0), at[1] + y + ch * 0.5);
            let mut cmds = win.mouse_down(&data.look, w, hh, px, py);
            cmds.extend(win.mouse_up(&data.look, w, hh, px, py));
            cmds
        };
        Ok(run(host, cmds))
    })?;

    // menu_press(id, f): the left button down on control `id`, a fraction
    // `f` of the way across it (held, for menu_timer / menu_release) → what
    // the menu asks for, as menu_click.
    r.def("menu_press", |_, host, (id, f): (i64, f32)| {
        let (w, hh) = (640.0, 480.0);
        let (cmds, at) = {
            let Some((data, m)) = host.menu.as_mut() else {
                return Ok(None);
            };
            let Some(win) = m.top_mut() else {
                return Ok(None);
            };
            let Some(c) = win.control(id).filter(|c| c.visible) else {
                return Err(mlua::Error::external(format!("no control {id} showing")));
            };
            let o = win.at(w, hh);
            let [x, y, cw, ch] = c.def.rect;
            let at = [o[0] + x + cw * f, o[1] + y + ch * 0.5];
            (win.mouse_down(&data.look, w, hh, at[0], at[1]), at)
        };
        host.menu_held = Some((at, std::time::Instant::now()));
        Ok(run(host, cmds))
    })?;

    // Fires the menu's timer `n` times (200 ms each) while `menu_press()` holds the
    // button, so a held slider steps on → what the menu asks for, as menu_click.
    r.def("menu_timer", |_, host, n: u32| {
        let Some((at, mut clock)) = host.menu_held else {
            return Ok(None);
        };
        let mut cmds = Vec::new();
        if let Some((data, m)) = host.menu.as_mut() {
            if let Some(win) = m.top_mut() {
                for _ in 0..n {
                    clock += std::time::Duration::from_millis(200);
                    cmds.extend(win.tick(&data.look, 640.0, 480.0, at[0], at[1], clock));
                }
            }
        }
        host.menu_held = Some((at, clock));
        Ok(run(host, cmds))
    })?;

    // Lets go of the button `menu_press()` held → what the menu asks for, as
    // menu_click (nil: nothing held).
    r.def("menu_release", |_, host, ()| {
        let Some((at, _)) = host.menu_held.take() else {
            return Ok(None);
        };
        let cmds = host
            .menu
            .as_mut()
            .and_then(|(data, m)| {
                Some(
                    m.top_mut()?
                        .mouse_up(&data.look, 640.0, 480.0, at[0], at[1]),
                )
            })
            .unwrap_or_default();
        Ok(run(host, cmds))
    })?;

    // menu_loading(percent): the level-load dialog alone over black, its
    // bar at `percent` (screenshots draw it); menu_loading() hides it.
    r.def("menu_loading", |_, host, percent: Option<u32>| {
        let Some(p) = percent else {
            host.loading = None;
            return Ok(());
        };
        if host.loading.is_none() {
            let data = crate::hud::menu::Data::load(&host.oni_root.join("common"));
            if !data.has("Progress") {
                return Err(mlua::Error::external(
                    "no level-load dialog (common/menus.json); reconvert the assets",
                ));
            }
            let w = crate::hud::menu::Window::new(&data, "Progress");
            host.loading = Some((data, w));
        }
        if let Some((_, w)) = host.loading.as_mut() {
            w.set_progress(100, p);
        }
        Ok(())
    })?;

    // Presses a key on the menu: "enter", "escape", "up", "down", "left",
    // "right", "home", "end", "pageup", "pagedown" → what the menu asks for, as
    // menu_click.
    r.def("menu_key", |_, host, name: String| {
        use crate::hud::menu::Key;
        let key = match name.as_str() {
            "enter" => Key::Enter,
            "escape" => Key::Escape,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" => Key::PageUp,
            "pagedown" => Key::PageDown,
            _ => return Err(mlua::Error::external(format!("no key {name}"))),
        };
        let cmds = host
            .menu
            .as_mut()
            .and_then(|(_, m)| m.top_mut())
            .map(|w| w.key(key))
            .unwrap_or_default();
        Ok(run(host, cmds))
    })?;
    Ok(())
}
