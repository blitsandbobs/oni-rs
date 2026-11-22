//! Engine commands: films, the countdown timer and trigger volumes.

use super::*;

pub(super) fn dispatch(
    state: &mut State,
    host: &mut dyn Host,
    _lua: &Lua,
    name: &str,
    args: Variadic<Value>,
) -> mlua::Result<Reply> {
    let a = |k: usize| args.get(k);
    let done = |v: Value| Ok((v, Value::Boolean(false), None, None));
    let nil = || done(Value::Nil);
    match name {
        // --- films -------------------------------------------------------------
        // `playback who film [mode] [interp]`: plays a recorded film on the character
        // (`mode` "fromhere" where it stands, "interp" easing there over `interp`
        // ticks, default 30); `_block` waits the film's length. Warns if it can't
        // play.
        "playback" | "chr_playback" | "playback_block" | "chr_playback_block" => {
            let (Some(id), Some(film)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let mode = a(2).and_then(text).unwrap_or_default();
            let interp = int(a(3)).unwrap_or(30).max(0) as u32;
            match host.world().play_film(id, &film, &mode, interp) {
                Some(len) if name.ends_with("_block") => {
                    Ok((Value::Nil, Value::Integer(len as i64), None, None))
                }
                Some(_) => nil(),
                None => {
                    state.warn(format!("{name}: can't play film {film}"));
                    nil()
                }
            }
        }

        // --- the countdown timer (`world/timer.rs`) ----------------------------
        // `timer_start seconds script` (`AIiScript_Timer_Start`): the HUD countdown
        // from `seconds` (held to an hour), then script function `script` runs.
        "timer_start" => {
            let (Some(seconds), Some(script)) = (float(a(0)), a(1).and_then(text)) else {
                state.warn("timer_start: expected <seconds> <script>".into());
                return nil();
            };
            host.world().timer_start(seconds, &script);
            nil()
        }
        // Turns the countdown off (a success, with its sound, if it was still
        // running).
        "timer_stop" => {
            host.world().timer_stop();
            nil()
        }
        "ai2_takecontrol" => {
            // (`AI2iScript_TakeControl`: the AI movement system takes the
            // player — no sprint, no look offset — or lets go.)
            let on = truthy(a(0));
            let world = host.world();
            world.input.ai_control = on;
            if let Some(p) = world.fighters.first_mut() {
                if on {
                    p.facing.facing_modifier = 0.0;
                    p.brain = Some(crate::ai::Brain::Steer(Box::default()));
                } else if matches!(p.brain, Some(crate::ai::Brain::Steer(_))) {
                    p.brain = None;
                }
            }
            nil()
        }
        "ai2_movetoflag" => {
            // (`AI2rPath_GoToPoint` within 3 — `AI2cMoveToPoint_DefaultDistance`
            // — then the flag's facing with "setfacing".)
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let set_facing = a(2).and_then(text).is_some_and(|s| s == "setfacing");
            let world = host.world();
            let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() else {
                state.warn(format!("ai2_movetoflag: no flag {flag}"));
                return nil();
            };
            let face = set_facing.then_some(f.facing);
            script_move(state, host, id, f.at, face, None, "ai2_movetoflag");
            nil()
        }
        "ai2_lookatchar" => {
            // (Stays where it is, turned toward the character, looking at it.)
            let (Some(id), Some(target)) = (fighter(host, a(0)), fighter(host, a(1))) else {
                return nil();
            };
            let world = host.world();
            let (from, to) = (world.fighters[id].feet(), world.fighters[target].feet());
            let face = (to.x - from.x)
                .atan2(to.z - from.z)
                .rem_euclid(std::f32::consts::TAU);
            script_move(
                state,
                host,
                id,
                from,
                Some(face),
                Some(to),
                "ai2_lookatchar",
            );
            nil()
        }
        "ai2_lookatme" => {
            // (`AI2rDebug_LookHere`: one AI, or every AI, looks at the
            // player — and keeps looking as she moves.)
            let world = host.world();
            let Some(player) = world
                .fighters
                .first()
                .map(|p| p.feet() + Vec3::Y * p.motion.height())
            else {
                return nil();
            };
            let who: Vec<usize> = match a(0) {
                Some(v) => ai(host, Some(v)).into_iter().collect(),
                None => (1..host.world().fighters.len()).collect(),
            };
            for id in who {
                if let Some(crate::ai::Brain::Agent(ag)) = host.world().fighters[id].brain.as_mut()
                {
                    ag.look_at_char(0, player);
                }
            }
            nil()
        }
        "ai2_forget" => {
            // (`AI2rForget`: one AI, or every AI, forgets one character, or
            // everyone.)
            let who: Vec<usize> = match a(0) {
                Some(v) => ai(host, Some(v)).into_iter().collect(),
                None => (1..host.world().fighters.len()).collect(),
            };
            let forget = a(1).and_then(|v| fighter(host, Some(v)));
            for id in who {
                if let Some(p) = host.world().fighters[id].perception.as_mut() {
                    p.forget(forget);
                }
            }
            nil()
        }
        // `ai2_makeblind` / `ai2_makedeaf who on`: the AI sees nothing / hears
        // nothing, or again does.
        "ai2_makeblind" | "ai2_makedeaf" => {
            if let Some(p) =
                ai(host, a(0)).and_then(|id| host.world().fighters[id].perception.as_mut())
            {
                if name == "ai2_makeblind" {
                    p.blind = truthy(a(1));
                } else {
                    p.deaf = truthy(a(1));
                }
            }
            nil()
        }
        // `chr_peace who` (`ONrCharacter_ExitFightMode`): the character leaves its
        // fight stance at once.
        "chr_peace" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().fighters[id].motion.exit_fight_mode();
            }
            nil()
        }
        // `ai2_idle who`: the AI stands idle now.
        "ai2_idle" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().order_idle(id);
            }
            nil()
        }
        // `ai2_setalert who level`: sets the AI's alert level (`lull`, `low`,
        // `medium`, `high`, `combat`; anything else lull).
        "ai2_setalert" => {
            if let (Some(id), Some(level)) = (ai(host, a(0)), a(1).and_then(text)) {
                if let Some(p) = host.world().fighters[id].perception.as_mut() {
                    p.alert = crate::ai::level::alert_named(&level);
                }
            }
            nil()
        }

        // --- trigger volumes -------------------------------------------------
        // `trigvolume_enable name on [entry|inside|exit]`: the named trigger volumes
        // switched on or off, wholly or just one of their events; disabled wholly,
        // they forget who is inside (re-enabled, entry fires again).
        "trigvolume_enable" => {
            let on = truthy(a(1));
            let kind = a(2).and_then(text);
            let found = state.triggers.by_name(a(0));
            if found.is_empty() {
                state.warn(format!(
                    "trigvolume_enable: no volume {:?}",
                    a(0).and_then(text)
                ));
            }
            for i in found {
                let bit = match kind.as_deref() {
                    Some("entry") => tv::ENTRY_OFF,
                    Some("inside") => tv::ENTRY_OFF << 1,
                    Some("exit") => tv::ENTRY_OFF << 2,
                    _ => tv::DISABLED,
                };
                let v = &mut state.triggers.volumes[i];
                if on {
                    v.flags &= !bit;
                } else {
                    v.flags |= bit;
                    if bit == tv::DISABLED {
                        // (Forgotten without an exit: re-enabling fires entry again.)
                        let id = v.id;
                        for list in state.triggers.inside.values_mut() {
                            list.retain(|&x| x != id);
                        }
                    }
                }
            }
            nil()
        }
        // `trigvolume_reset name`: the named trigger volumes back to their authored
        // flags and scripts.
        "trigvolume_reset" => {
            for i in state.triggers.by_name(a(0)) {
                let v = &mut state.triggers.volumes[i];
                v.flags = v.authored_flags;
                v.scripts = v.authored_scripts.clone();
            }
            nil()
        }
        // `trigvolume_setscript name script [entry|inside|exit]`: the script function
        // the named volumes call on that event (default entry).
        "trigvolume_setscript" => {
            let kind = match a(2).and_then(text).as_deref() {
                Some("inside") => 1,
                Some("exit") => 2,
                _ => 0,
            };
            let script = a(1).and_then(text).unwrap_or_default();
            for i in state.triggers.by_name(a(0)) {
                state.triggers.volumes[i].scripts[kind] = script.clone();
            }
            nil()
        }
        // `trigvolume_trigger name [entry|inside|exit] [who]`: fires that event of
        // the named volumes as if `who` (default the player) did it.
        "trigvolume_trigger" => {
            let kind = match a(1).and_then(text).as_deref() {
                Some("inside") => 1,
                Some("exit") => 2,
                _ => 0,
            };
            let who = match a(2) {
                Some(v) => fighter(host, Some(v)).unwrap_or(0),
                None => 0,
            };
            let who = host.world().name_of(who);
            let mut out = Vec::new();
            for i in state.triggers.by_name(a(0)) {
                state.triggers.fire(i, kind, &who, &mut out);
            }
            state.pending_events.extend(out);
            nil()
        }
        // `trigvolume_count id` → how many living characters of the volume's teams
        // are inside trigger volume `id`.
        "trigvolume_count" => {
            let found = state.triggers.by_id(a(0));
            let world = host.world();
            let mut n = 0;
            for i in found {
                let v = &state.triggers.volumes[i];
                n += world
                    .fighters
                    .iter()
                    .filter(|f| {
                        f.alive() && v.teams & (1 << f.team() as u32) != 0 && {
                            let feet = f.feet();
                            v.crossed(feet, feet + Vec3::Y * f.motion.height().max(12.0))
                        }
                    })
                    .count();
            }
            done(Value::Integer(n as i64))
        }
        // `trigvolume_kill id`: removes every non-player character inside trigger
        // volume `id`; `trigvolume_corpse`: only the dead ones, and the corpses there
        // (`ONrGameState_KillCorpsesInsideTrigger`).
        "trigvolume_kill" | "trigvolume_corpse" => {
            let found = state.triggers.by_id(a(0));
            let world = host.world();
            let doomed: Vec<usize> = (1..world.fighters.len())
                .filter(|&j| {
                    let f = &world.fighters[j];
                    let wanted = if name == "trigvolume_kill" {
                        !f.gone
                    } else {
                        !f.gone && !f.alive()
                    };
                    let feet = f.feet();
                    wanted
                        && found.iter().any(|&i| {
                            state.triggers.volumes[i].crossed(feet, feet + Vec3::Y * 12.0)
                        })
                })
                .collect();
            for j in doomed {
                world.remove(j);
            }
            // (And the corpses of the dead of play there:
            // `ONrGameState_KillCorpsesInsideTrigger`.)
            if name == "trigvolume_corpse" {
                world.corpses.clear_where(|lo, hi| {
                    found
                        .iter()
                        .any(|&i| state.triggers.volumes[i].crossed(lo, hi))
                });
            }
            nil()
        }
        // Clears the dead of play, leaving the level's own corpses.
        "corpse_reset" => {
            host.world().corpses.reset();
            nil()
        }

        // (Not this area's: the next's.)
        _ => super::stubbed(state, name),
    }
}
