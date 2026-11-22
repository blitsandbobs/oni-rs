//! Engine commands: environmental particles and sound.

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
        // --- environmental particles ----------------------
        // `particle tag command [event]`: sends the environmental particles tagged
        // `tag` a command ("create", "kill", "reset", "do event", or an event name);
        // warns on an unknown command.
        "particle" => {
            let (Some(tag), Some(command)) = (a(0).and_then(text), a(1).and_then(text)) else {
                return nil();
            };
            let event = a(2).and_then(text);
            if !host
                .world()
                .env_particle_command(&tag, &command, event.as_deref())
            {
                state.warn(format!("particle {tag}: unknown command {command}"));
            }
            nil()
        }
        // --- sound ---------------------------------
        // One dialogue voice: a line still on drops a plain play, holds a
        // `_block` play until it is over, is cut off by `_interrupt`. Its
        // subtitle shows for its length and a second.
        "sound_dialog_play" | "sound_dialog_play_block" | "sound_dialog_play_interrupt" => {
            let Some(line) = a(0).and_then(text) else {
                return nil();
            };
            match host
                .world()
                .audio
                .dialog_start(&line, name.ends_with("_interrupt"))
            {
                Err(_) if name.ends_with("_block") => {
                    Ok((Value::Nil, Value::Boolean(true), None, None))
                }
                Err(_) => nil(),
                // (No such line: nothing, quietly, no subtitle — as shipped.)
                Ok(None) => nil(),
                Ok(Some(duration)) => {
                    if let Some(t) = state.message_text(&line) {
                        eprintln!("SAY ({line}): {t}");
                    }
                    // Its subtitle, with the option on, for its length and a
                    // second more (`OSrInGameDialog_Play`,
                    // `OScSubtitleStayTime`).
                    let world = host.world();
                    let letterboxed = world.cutscene.letterbox;
                    world.hud.subtitle(&line, duration + 60, letterboxed);
                    nil()
                }
            }
        }
        // (Called by a few scripts, but never an engine command: nothing.)
        "sound_dialog_play_pause" => nil(),
        // Music and script ambients: one pool of 16 unplaced slots.
        "sound_music_start" | "sound_ambient_start" => {
            let Some(sound) = a(0).and_then(text) else {
                return nil();
            };
            let volume = float(a(1)).unwrap_or(1.0);
            if !host.world().audio.script_start(&sound, volume) {
                state.warn(format!("{name}: can't play {sound}"));
            }
            nil()
        }
        // `sound_music_stop` / `sound_ambient_stop name`: the first slot playing that
        // sound plays out its end.
        "sound_music_stop" | "sound_ambient_stop" => {
            if let Some(sound) = a(0).and_then(text) {
                host.world().audio.script_stop(&sound);
            }
            nil()
        }
        // `sound_music_volume` / `sound_ambient_volume name volume [seconds]`: the
        // first slot of that name fades to `volume` (0–1) over `seconds` (even one
        // that has ended, as shipped).
        "sound_music_volume" | "sound_ambient_volume" => {
            let (Some(sound), Some(volume)) = (a(0).and_then(text), float(a(1))) else {
                return nil();
            };
            let seconds = float(a(2)).unwrap_or(0.0);
            host.world().audio.script_volume(&sound, volume, seconds);
            nil()
        }
        // `sound_impulse_play name [volume]`: plays an impulse sound, unplaced, at
        // `volume` (0–1).
        "sound_impulse_play" => {
            let Some(sound) = a(0).and_then(text) else {
                return nil();
            };
            let volume = float(a(1)).map(|v| v.clamp(0.0, 1.0));
            let world = host.world();
            world
                .audio
                .impulse_play(&sound, None, volume, Some(&world.quads));
            nil()
        }
        // A character says a line: `chr_talk who sound pre post [type]`
        // (no type: override). (The original never stores a given type —
        // uninitialised; here it is used.)
        "chr_talk" => {
            let (Some(who), Some(sound)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let kind = match a(4).and_then(text) {
                None => crate::world::speech::OVERRIDE,
                Some(t) => match [
                    "none", "idle", "chatter", "say", "call", "pain", "yell", "override",
                ]
                .iter()
                .position(|n| *n == t)
                {
                    Some(k) => k as u8,
                    None => return nil(),
                },
            };
            let line = crate::world::Line {
                sound,
                kind,
                pre_pause: int(a(2)).unwrap_or(0).max(0) as u32,
                post_pause: int(a(3)).unwrap_or(0).max(0) as u32,
            };
            host.world().say(who, line, false);
            nil()
        }
        // `chr_vocalize who kind`: its AI's line of that kind, forced.
        "chr_vocalize" => {
            let (Some(who), Some(kind)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if let Some(k) = oni_schema::VOCALIZATIONS
                .iter()
                .position(|v| v.eq_ignore_ascii_case(&kind))
            {
                host.world().vocalize(who, k, true);
            }
            nil()
        }

        // (Not this area's: the next's.)
        _ => super::characters::dispatch(state, host, lua, name, args),
    }
}
