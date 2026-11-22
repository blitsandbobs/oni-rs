//! Scenario functions: the level's own scripts, cutscenes, dialogue and the game flow.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // The level's own scripts (`docs/lua.md`): begins the level (the
    // player at its placement, the characters present from the start,
    // `main`). `run` false: the scripts loaded but the level's start not
    // run; `save_point` n: begun from the level's save point n.
    r.def(
        "mission_begin",
        |_, host, (run, save_point): (Option<bool>, Option<i64>)| {
            let mut m = crate::mission::Mission::load(&host.world.level_dir)
                .map_err(mlua::Error::runtime)?;
            // (`mission_begin(true, n)`: from the level's save point n.)
            if let Some(n) = save_point {
                m.state
                    .vars
                    .insert("save_point".into(), mlua::Value::Integer(n));
            }
            // (A level begun afresh flags its new moves.)
            if save_point.unwrap_or(0) == 0 && run != Some(false) {
                host.world.hud.level_started();
            }
            // (`mission_begin(false)`: the scripts loaded, the level's own
            // start not run.)
            if run != Some(false) {
                m.begin(&mut *host);
            } else if !host.world.quirks.scripts_as_shipped() {
                let level = host.world.level_dir.clone();
                m.apply_fixes(&level);
            }
            host.mission = Some(m);
            Ok(())
        },
    )?;

    // What the level scripts are doing → {tasks, waiting = {…}, errors = {…},
    // objective, outcome, spawned, messages = {…}, stubbed = {command, …}}; an
    // empty table before `mission_begin()`.
    r.def("mission_info", |lua, host, ()| {
        let t = lua.create_table()?;
        if let Some(m) = &host.mission {
            t.set("tasks", m.tasks())?;
            t.set("waiting", m.waiting())?;
            t.set("errors", m.state.errors.clone())?;
            t.set("objective", m.state.objective)?;
            t.set("outcome", m.state.outcome)?;
            t.set("spawned", m.state.spawned.len())?;
            t.set("messages", m.state.messages.clone())?;
            let mut stubs: Vec<_> = m.state.stubbed.keys().cloned().collect();
            stubs.sort();
            t.set("stubbed", stubs)?;
        }
        Ok(t)
    })?;

    // The fighter id of the live scripted character called `name` (its
    // `chr_create` name; case matters), or nil.
    r.def("named", |_, host, name: String| {
        Ok(host.world.fighters.iter().position(|f| {
            !f.gone && matches!(&f.brain, Some(crate::ai::Brain::Agent(a)) if a.name == name)
        }))
    })?;

    // Plays a level film on fighter `id` (mode "", "fromhere", "interp");
    // returns its length in ticks, or nil.
    r.def(
        "play_film",
        |_, host, (id, name, mode): (usize, String, Option<String>)| {
            Ok(host
                .world
                .play_film(id, &name, mode.as_deref().unwrap_or(""), 30))
        },
    )?;

    // The game flow: the level's outcome (true won,
    // false lost), a splash screen asked for, the save point kept.
    r.def("game", |lua, host, ()| {
        let w = &host.world;
        let t = lua.create_table()?;
        t.set("victory", w.progress.victory)?;
        t.set("splash", w.progress.pending_splash.clone())?;
        t.set(
            "save_point",
            w.progress.saved.as_ref().map(|c| c.save_point),
        )?;
        Ok(t)
    })?;

    // The player reads the objective (F1).
    r.def("read_objective", |_, host, ()| {
        host.world.hud.read_objective();
        Ok(())
    })?;

    // Dialogue subtitles on or off (the game's option).
    r.def("subtitles", |_, host, on: bool| {
        host.world.hud.subtitles_on = on;
        Ok(())
    })?;

    // dialog() → whether a scripted dialogue line is on.
    r.def("dialog", |_, host, ()| {
        Ok(host.world.audio.dialog_playing())
    })?;

    // Defines script functions of the scenario's own (translated-Lua form)
    // in the running mission, to call with mission_call.
    r.def("mission_exec", |_, host, code: String| {
        match host.mission.as_ref() {
            Some(m) => m.exec(&code).map_err(mlua::Error::external),
            None => Err(mlua::Error::external(
                "no mission (call mission_begin first)",
            )),
        }
    })?;

    // Starts level-script function `name` as a new task (with one string
    // argument, if given) → whether it exists (false without a mission).
    r.def(
        "mission_call",
        |_, host, (name, arg): (String, Option<String>)| {
            let args: Vec<&str> = arg.as_deref().into_iter().collect();
            Ok(host.mission.as_mut().is_some_and(|m| m.start(&name, &args)))
        },
    )?;

    // Asks to skip the cutscene (as Space does).
    r.def("skip_cutscene", |_, host, ()| {
        host.world.skip_cutscene();
        Ok(())
    })?;

    // The cutscene skip → mode ("idle" / "starting" / "black" / "skipping" /
    // "fading in") and the screen's black (0–1; nil: none).
    r.def("cutscene_skip", |_, host, ()| {
        use crate::world::skip::Skip;
        let w = &host.world;
        let mode = match w.cutscene.skip {
            Skip::Idle => "idle",
            Skip::Starting(_) => "starting",
            Skip::Black => "black",
            Skip::Skipping => "skipping",
            Skip::FadingIn(_) => "fading in",
        };
        Ok((mode, w.skip_blanket()))
    })?;

    // player_ready() → whether the player is in play: no cutscene, input
    // on, no environment animation or film moving her, alive.
    r.def("player_ready", |_, host, ()| {
        let w = &host.world;
        Ok(!w.cutscene.on
            && w.input.enabled
            && w.fighters
                .first()
                .is_some_and(|p| p.env_anim.is_none() && !p.motion.dying && p.hp > 0))
    })?;

    // cinematics() → the cutscene inserts showing: a list of {name, x, y,
    // w, h} (top left and size on the 1024 × 768 virtual screen).
    r.def("cinematics", |lua, host, ()| {
        let t = lua.create_table()?;
        for c in &host.world.hud.cinematics.list {
            let e = lua.create_table()?;
            e.set("name", c.name.clone())?;
            e.set("x", c.at[0])?;
            e.set("y", c.at[1])?;
            e.set("w", c.size[0])?;
            e.set("h", c.size[1])?;
            t.push(e)?;
        }
        Ok(t)
    })?;

    // timer() → (what it shows now or nil, ticks left, mode "on" | "fail"),
    // or nil with no countdown running (`world/timer.rs`).
    r.def("timer", |_, host, ()| {
        let w = &host.world;
        Ok(match w.timer.as_ref() {
            Some(t) => (
                w.timer_text(),
                Some(t.ticks),
                Some(if t.mode == crate::world::timer::Mode::On {
                    "on"
                } else {
                    "fail"
                }),
            ),
            None => (None, None, None),
        })
    })?;
    Ok(())
}
