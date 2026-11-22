//! Engine commands: the prelude's requests and output.

use super::*;

pub(super) fn dispatch(
    state: &mut State,
    host: &mut dyn Host,
    lua: &Lua,
    name: &str,
    args: Variadic<Value>,
) -> mlua::Result<Reply> {
    let a = |k: usize| args.get(k);
    let done = |v: Value| Ok((v, Value::Boolean(false), None, None));
    let nil = || done(Value::Nil);
    match name {
        // --- the prelude's requests --------------------------------------
        "@word" => {
            // An engine variable's value, a value command's result, or nil
            // (the word stands for itself).
            let w = a(0).and_then(text).unwrap_or_default();
            if let Some(v) = state.vars.get(&w) {
                return done(v.clone());
            }
            match w.as_str() {
                "difficulty" => done(Value::Integer(host.world().difficulty.level)),
                "did_kill_griffen" => done(Value::Integer(did_kill_griffin(host) as i64)),
                "chr_has_lsi" | "chr_has_empty_weapon" => done(Value::Integer(0)),
                _ => nil(),
            }
        }
        "@chr_at" => {
            // (For our fixes, `fixes/`: where a character stands, feet —
            // the player by default; nil if it isn't present.)
            let who = match a(0) {
                None | Some(Value::Nil) => Some(0),
                v => fighter(host, v),
            };
            match who.and_then(|i| host.world().fighters.get(i).map(|f| f.feet())) {
                Some(at) => done(Value::Table(lua.create_sequence_from([at.x, at.y, at.z])?)),
                None => nil(),
            }
        }
        // A `sleep` argument (`SLEEP`) as ticks: a number as is, a word by its value
        // (0 if none); never negative.
        "@ticks" => {
            let n = match a(0) {
                Some(Value::String(s)) => state.ticks_of_word(&s.to_string_lossy()),
                v => int(v).unwrap_or(0),
            };
            done(Value::Integer(n.max(0)))
        }
        // Adopts a coroutine the prelude started and that yielded as a new task,
        // run after `delay` ticks, labelled for `mission_info()`.waiting.
        "@adopt" => {
            if let Some(Value::Thread(t)) = a(0) {
                let delay = int(a(1)).unwrap_or(0).max(0) as u64;
                let label = a(2).and_then(text).unwrap_or_else(|| "fork".into());
                state.started.push((t.clone(), label, None, delay));
            }
            nil()
        }
        // A script error (`ERR`, a missing script function): printed and kept in
        // `mission_info()`.errors, once each.
        "@error" => {
            let msg = a(0).and_then(text).unwrap_or_default();
            if !state.errors.contains(&msg) {
                eprintln!("script error: {msg}");
                state.errors.push(msg);
            }
            nil()
        }
        // Reads an engine variable (`G.name`): the fog's `gl_fog_*` from the draw
        // engine, else what a script set (0 if unset).
        "@get" => {
            let k = a(0).and_then(text).unwrap_or_default();
            // (The fog's are the draw engine's.)
            let fog = host.world().fog.fog;
            let v = match k.as_str() {
                "gl_fog_red" => Some(fog.color[0]),
                "gl_fog_green" => Some(fog.color[1]),
                "gl_fog_blue" => Some(fog.color[2]),
                "gl_fog_start" => Some(fog.start),
                "gl_fog_end" => Some(fog.end),
                _ => None,
            };
            match v {
                Some(v) => done(Value::Number(f64::from(v))),
                None => done(state.vars.get(&k).cloned().unwrap_or(Value::Integer(0))),
            }
        }
        // Sets an engine variable (`G.name = v`); `gl_fog_*`, `ai2_boss_battle` and
        // the camera's `cm_distance` / `cm_height` / `cm_canter_*` also change the
        // engine.
        "@set" => {
            let k = a(0).and_then(text).unwrap_or_default();
            // (The fog's and the camera's tunables are engine variables.)
            if let Some(v) = float(a(1)) {
                let fog = &mut host.world().fog.fog;
                match k.as_str() {
                    "gl_fog_red" => fog.color[0] = v,
                    "gl_fog_green" => fog.color[1] = v,
                    "gl_fog_blue" => fog.color[2] = v,
                    "gl_fog_start" => fog.start = v,
                    "gl_fog_end" => fog.end = v,
                    _ => {}
                }
                if k == "ai2_boss_battle" {
                    host.world().boss_battle = v != 0.0;
                }
                let cam = &mut host.world().camera;
                match k.as_str() {
                    "cm_distance" => cam.distance = v,
                    "cm_height" => cam.height = v,
                    "cm_canter_weapon" => cam.canter_weapon = v,
                    "cm_canter_unarmed" => cam.canter_unarmed = v,
                    _ => {}
                }
            }
            state.vars.insert(k, a(1).cloned().unwrap_or(Value::Nil));
            nil()
        }
        // --- output ----------------------------------------------------------
        // Prints its words to the terminal; `dmsg` also shows them in the HUD's
        // message area for 10 s (`dprint` went to the developer console, unseen in
        // the shipped game).
        "dprint" | "dmsg" => {
            let words: Vec<String> = args.iter().filter_map(text).collect();
            eprintln!("script: {}", words.join(" "));
            // (`dmsg` shows in the message area for 10 s; `dprint` goes to
            // the developer console, not shown in the shipped game.)
            if name == "dmsg" {
                host.world().hud.messages.print(&words.join(" "), None, 600);
            }
            nil()
        }
        // `message name [ticks]` (`AIiScript_Message`): shows a message from the
        // messages table (else a subtitle in the console strip), for `ticks` if
        // given; warns if it is in neither.
        "message" => {
            if let Some(m) = a(0).and_then(text) {
                match state.message_text(&m) {
                    Some(t) => eprintln!("MESSAGE ({m}): {t}"),
                    None => eprintln!("MESSAGE {m}"),
                }
                state.messages.retain(|x| *x != m);
                if let Some(t) = int(a(1)).filter(|&t| t > 0) {
                    state.message_until.insert(m.clone(), state.now + t as u64);
                } else {
                    state.message_until.remove(&m);
                }
                if !host
                    .world()
                    .hud
                    .message(&m, int(a(1)).unwrap_or(0).max(0) as u32)
                {
                    state.warn(format!("message: no message {m}"));
                }
                state.messages.push(m);
            }
            nil()
        }
        // `message_remove [name]`: that message (every one, without a name) fades
        // from the HUD.
        "message_remove" => {
            let m = a(0).and_then(text);
            host.world().hud.message_remove(m.as_deref());
            match m {
                Some(m) => state.messages.retain(|x| *x != m),
                None => state.messages.clear(),
            }
            nil()
        }
        "target_set" => {
            // The HUD's target locator (no HUD yet: said in the terminal).
            let (Some(flag), d) = (
                int(a(0)),
                a(1).and_then(|v| match v {
                    Value::Number(n) => Some(*n),
                    Value::Integer(i) => Some(*i as f64),
                    _ => None,
                }),
            ) else {
                return nil();
            };
            let world = host.world();
            let d = d.unwrap_or(0.0) as f32;
            let at = world.level_ai.flags.get(&(flag as i16)).map(|f| f.at);
            if d != 0.0 && at.is_none() {
                state.warn(format!("target_set: no flag {flag}"));
            }
            let from = world.player().map_or(Vec3::ZERO, |p| p.feet());
            if d != 0.0 && world.hud.target_set(at, d, from) {
                world.event_sound_ui("compass");
            } else if d == 0.0 {
                world.hud.target_set(None, 0.0, from);
            }
            nil()
        }
        // `objective_set n [silent]`: the objective becomes text `n`; the HUD prompts
        // the new one with its sound unless `silent`.
        "objective_set" => {
            state.objective = int(a(0));
            eprintln!("script: objective {:?}", state.objective);
            // (The HUD prompts a new one, with its sound, unless `silent`.)
            let silent = a(1)
                .and_then(text)
                .is_some_and(|t| t.eq_ignore_ascii_case("silent"));
            host.world()
                .hud
                .objective_set(int(a(0)).unwrap_or(0), silent);
            nil()
        }
        // Plays the objective-complete sound (no other effect).
        "objective_complete" => {
            host.world().event_sound("objectiveComplete", None);
            nil()
        }
        // `save_game n [autosave]` (`ONrGameState_MakeContinue`): keeps the player as
        // she is now for a restart at save point `n`; an autosave shows its message
        // and sound.
        "save_game" => {
            let autosave = a(1).and_then(text).is_some_and(|t| t == "autosave");
            host.world().save_continue(int(a(0)).unwrap_or(0), autosave);
            nil()
        }
        // Puts the player back as last saved (`ONrGameState_UseContinue`):
        // health, place, facing, inventory and guns.
        "restore_game" => {
            host.world().restore_continue();
            nil()
        }
        // `splash_screen name`: asks for that splash screen (kept for the game flow
        // to show, `game()`.splash).
        "splash_screen" => {
            if let Some(name) = a(0).and_then(text) {
                host.world().progress.pending_splash = Some(name);
            }
            nil()
        }
        "begin_cutscene" => {
            // (`World::begin_cutscene`.)
            let names: Vec<String> = args.iter().filter_map(text).collect();
            let (keep, unknown) =
                crate::world::cutscene::CutsceneKeeps::parse(names.iter().map(String::as_str));
            for k in unknown {
                state.warn(format!("begin_cutscene: unknown flag {k}"));
            }
            host.world().begin_cutscene(keep);
            nil()
        }

        // (Not this area's: the next's.)
        _ => super::screen::dispatch(state, host, lua, name, args),
    }
}
