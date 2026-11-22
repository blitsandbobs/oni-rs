//! Engine commands: placed objects, doors, consoles, lasers, turrets.

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
        // --- placed objects (`world/objects.rs`) -----------------------------
        // `obj_create` / `obj_kill` / `obj_show` / `obj_hide a [b]`: placed objects
        // with script ids `a`–`b` made anew, removed, or shown / hidden (drawn and
        // solid, or neither; `OBrShow`).
        "obj_create" | "obj_kill" | "obj_show" | "obj_hide" => {
            let Some(lo) = int(a(0)) else { return nil() };
            let hi = int(a(1)).unwrap_or(lo);
            let (lo, hi) = (lo.max(0) as u32, hi.max(0) as u32);
            let world = host.world();
            match name {
                "obj_create" => world.obj_create(lo, hi),
                "obj_kill" => world.obj_kill(lo, hi),
                // (`obj_show` only ever shows, whatever its help says.)
                "obj_show" => world.obj_show(lo, hi, true),
                _ => world.obj_show(lo, hi, false),
            }
            nil()
        }
        // `env_anim a [b]`: the placed objects `a`–`b` start their animation from the
        // beginning; `_block` waits while one plays on (as shipped, only when given
        // `b`).
        "env_anim" | "env_anim_block" => {
            let Some(lo) = int(a(0)) else { return nil() };
            let hi = int(a(1)).map(|h| h.max(0) as u32);
            if host
                .world()
                .env_anim(lo.max(0) as u32, hi, name == "env_anim_block")
            {
                return Ok((Value::Nil, Value::Boolean(true), None, None));
            }
            nil()
        }
        // `env_setanim id clip` (`OBrSetAnimationByName`): those objects take the
        // named clip and start it; `_block` with one still playing does nothing, as
        // shipped. Warns if no such object or clip.
        "env_setanim" | "env_setanim_block" => {
            let (Some(id), Some(clip)) = (int(a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if !host
                .world()
                .env_setanim(id.max(0) as u32, &clip, name == "env_setanim_block")
            {
                state.warn(format!("{name}: no object {id} or animation {clip}"));
            }
            nil()
        }
        // --- collision and environment animations --------------------------
        // `chr_nocollision who on`: whether the character passes through walls and
        // others.
        "chr_nocollision" => {
            let Some(id) = fighter(host, a(0)) else {
                return nil();
            };
            host.world().fighters[id].no_collision = truthy(a(1));
            nil()
        }
        // An object animation moves the character (its pelvis on the clip's
        // path, no collision) from frame 1 — the script sets 1, but
        // `OBrAnim_Start` resets it to 0 before the `+= 1`, so it keeps step
        // with an object started the same tick (a frame ahead and a fast
        // motorcycle leaves its rider over the front wheel); `_block` waits
        // for one already playing to end, then starts and goes on at once.
        "chr_envanim" | "chr_envanim_block" => {
            let (Some(id), Some(clip)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let world = host.world();
            if name == "chr_envanim_block" {
                if let Some((anim, frame)) = &world.fighters[id].env_anim {
                    if frame + 1 < i32::from(anim.num_frames) {
                        return Ok((Value::Nil, Value::Boolean(true), None, None));
                    }
                }
            }
            match world.object_anim(&clip) {
                Some(anim) => {
                    let f = &mut world.fighters[id];
                    f.env_anim = Some((anim, 1));
                    f.env_anim_no_rotation = a(2).and_then(text).is_some_and(|t| t == "norotation");
                }
                None => state.warn(format!("{name}: no animation {clip}")),
            }
            nil()
        }
        // `chr_envanim_stop who`: ends the object animation moving the character.
        "chr_envanim_stop" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().fighters[id].env_anim = None;
            }
            nil()
        }
        // --- turrets ----------------------------------------
        // `turret_activate` / `_deactivate` / `_reset id`: switches the turrets with
        // that script id (−1: every turret, except `_deactivate`, which then switches
        // none, as shipped).
        "turret_activate" | "turret_deactivate" | "turret_reset" => {
            let id = int(a(0)).unwrap_or(-1);
            let what = name.trim_start_matches("turret_");
            host.world().turret_set(id, what);
            nil()
        }
        // Resets every turret.
        "reset_mechanics" => {
            host.world().turret_set(-1, "reset");
            nil()
        }
        // --- laser triggers --------------------------------------------------
        // Laser trigger `id` (none or -1: every one, `OBJrTrigger_*`):
        // switched on or off, reset to its start, hidden or shown; `trig_speed
        // id factor` scales its sweep speed.
        "trig_activate" | "trig_deactivate" | "trig_reset" | "trig_hide" | "trig_show"
        | "trig_speed" => {
            let id = int(a(0)).unwrap_or(crate::lasers::ALL);
            let factor = a(1).and_then(|v| match v {
                Value::Number(n) => Some(*n as f32),
                Value::Integer(n) => Some(*n as f32),
                Value::String(s) => s.to_str().ok()?.trim().parse().ok(),
                _ => None,
            });
            host.world()
                .laser_set(id, &name[5..], factor.unwrap_or(1.0));
            nil()
        }

        // --- doors ------------------------------------------------------------
        // `door_lock` / `door_unlock` / `door_jam` / `door_unjam [id]`: locks or jams
        // door `id` (none or −1: every door), or undoes it.
        "door_lock" | "door_unlock" | "door_jam" | "door_unjam" => {
            let id = int(a(0)).unwrap_or(-1);
            let world = host.world();
            match name {
                "door_lock" => world.door_set(id, Some(true), None),
                "door_unlock" => world.door_set(id, Some(false), None),
                "door_jam" => world.door_set(id, None, Some(true)),
                _ => world.door_set(id, None, Some(false)),
            }
            nil()
        }
        // `door_open` / `door_close [id]`: opens or closes door `id` (none or −1:
        // every door).
        "door_open" | "door_close" => {
            let id = int(a(0)).unwrap_or(-1);
            for d in host
                .world()
                .doors
                .list
                .iter_mut()
                .filter(|d| id < 0 || i64::from(d.id) == id)
            {
                if name == "door_open" {
                    d.open();
                } else {
                    d.close();
                }
            }
            nil()
        }

        // --- consoles, input, alarms ----------------------------------------
        // `console_activate` / `_deactivate` / `_reset [id]`: console `id` (none:
        // every console) made usable or not, or reset to as authored and unused.
        "console_activate" | "console_deactivate" | "console_reset" => {
            let id = int(a(0));
            let world = host.world();
            for k in world
                .consoles
                .iter_mut()
                .filter(|k| id.is_none_or(|i| i64::from(k.id) == i))
            {
                match name {
                    "console_activate" => k.active = true,
                    "console_deactivate" => k.active = false,
                    _ => {
                        k.triggered = false;
                        k.active = k.initial_active;
                    }
                }
            }
            nil()
        }
        // `lock_keys [group…]`: alone, every gameplay key locked; with a group
        // (`keys_movement`, `keys_jump`, `keys_crouch`, `keys_attack`, `keys_action`,
        // `keys_walk`, `keys_inventory`, `keys_reload`, `keys_all`), those keys work
        // again. Pause and hypo keys aren't modelled.
        "lock_keys" => {
            use crate::anim::input::key as k;
            let world = host.world();
            match a(0).and_then(text) {
                // Alone: every gameplay key locked.
                None => world.input.key_mask = 0,
                Some(group) => {
                    world.input.key_mask |= match group.as_str() {
                        "keys_movement" => {
                            k::FORWARD
                                | k::BACKWARD
                                | k::STEP_LEFT
                                | k::STEP_RIGHT
                                | k::TURN_LEFT
                                | k::TURN_RIGHT
                        }
                        "keys_jump" => k::JUMP,
                        "keys_crouch" => k::CROUCH | k::FIRE3,
                        "keys_attack" => k::PUNCH | k::KICK | k::FIRE1 | k::FIRE2,
                        "keys_action" => k::ACTION,
                        "keys_walk" => k::WALK,
                        "keys_inventory" => k::SWAP | k::DROP,
                        "keys_reload" => k::RELOAD,
                        "keys_all" => u32::MAX,
                        _ => 0, // pause, hypo: not modelled
                    }
                }
            }
            nil()
        }
        // `input on`: whether the player's input works at all.
        "input" => {
            host.world().input.enabled = truthy(a(0));
            nil()
        }
        // `ai2_doalarm who [console]`: sends the AI to raise the alarm at that
        // console (none or 0: the nearest alarm console).
        "ai2_doalarm" => {
            if let Some(id) = ai(host, a(0)) {
                let console = int(a(1)).map(|c| c as u16);
                host.world().order_alarm(id, console);
            }
            nil()
        }
        // `ai2_tripalarm group [cause]`: an alarm of that group goes off, caused by
        // `cause` (default the player): AIs answering it learn where the cause is;
        // its alarm script is due. Nothing while every AI is passive.
        "ai2_tripalarm" => {
            let Some(group) = int(a(0)) else { return nil() };
            let cause = match a(1) {
                Some(v) => fighter(host, Some(v)).unwrap_or(0),
                None => 0,
            };
            if !host.world().all_passive {
                host.world().trip_alarm(group as u32, cause);
            }
            nil()
        }

        // (Not this area's: the next's.)
        _ => super::weapons::dispatch(state, host, lua, name, args),
    }
}
