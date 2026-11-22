//! Engine commands: characters, their scripted animation, waiting on one.

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
        // --- characters ------------------------------------------------------
        // `ai2_spawn name [force]`: spawns the level's placed character `name` (the
        // first placement not yet spawned, unless `force` or it may spawn many times)
        // with its goals; its spawn script runs at once.
        "ai2_spawn" => {
            let Some(who) = a(0).and_then(text) else {
                return nil();
            };
            let force = a(1).and_then(text).is_some_and(|f| f == "force");
            match spawn(state, host, &who, force) {
                Some((script, arg)) => {
                    Ok((Value::Nil, Value::Boolean(false), Some(script), Some(arg)))
                }
                None => nil(),
            }
        }
        "chr_create" => {
            // (The second argument, start / create, the original ignores.)
            let Some(sid) = int(a(0)) else { return nil() };
            match create(state, host, sid) {
                Some((script, arg)) => {
                    Ok((Value::Nil, Value::Boolean(false), Some(script), Some(arg)))
                }
                None => nil(),
            }
        }
        // A data console's text, modal: the game stands still until it is
        // closed (`ONiTextConsole_Display`; `hud/console.rs`).
        "text_console" => {
            let Some(name) = a(0).and_then(text) else {
                return nil();
            };
            if !host.world().hud.open_text_console(&name) {
                state.warn(format!("text_console: no console {name}"));
            }
            nil()
        }
        // `chr_delete who`: removes the character from the level.
        "chr_delete" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().remove(id);
            }
            nil()
        }
        // `chr_teleport who flag`: puts the character at the flag's position (keeping
        // its facing); warns if there is no such flag.
        "chr_teleport" => {
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let world = host.world();
            match world.level_ai.flags.get(&(flag as i16)).copied() {
                Some(f) => world.teleport(id, f.at, None),
                None => state.warn(format!("chr_teleport: no flag {flag}")),
            }
            nil()
        }
        // `chr_facetoflag who flag`: turns the character to the flag's facing where
        // it stands.
        "chr_facetoflag" => {
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let world = host.world();
            if let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() {
                let at = world.fighters[id].feet();
                world.teleport(id, at, Some(f.facing));
            }
            nil()
        }
        // `chr_set_health who hp`: sets its hit points; 0 or less kills it outright,
        // even if invincible or unkillable.
        "chr_set_health" => {
            let (Some(id), Some(hp)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            if let Some(f) = host.world().fighters.get_mut(id) {
                if hp <= 0 && f.alive() {
                    // (An act of god: kills even the invincible and the
                    // unkillable.)
                    let (was, unkillable, all) = (f.invincible, f.unkillable, f.hp);
                    (f.invincible, f.unkillable) = (false, false);
                    f.take_damage(all, Vec3::ZERO);
                    (f.invincible, f.unkillable) = (was, unkillable);
                } else {
                    f.hp = hp as i32;
                }
            }
            nil()
        }
        // `chr_full_health who`: restores its hit points to its full health.
        "chr_full_health" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.hp = f.max_hp;
            }
            nil()
        }
        // `chr_invincible who on`: whether it takes no damage.
        "chr_invincible" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.invincible = truthy(a(1));
            }
            nil()
        }
        // --- scripted animation ------------------------------------------------
        // `chr_animate who clip [frames] [interp]`: plays the clip on the character
        // now, blending over `interp` frames, then holds other clips off for `frames`
        // ticks (−1: its length); `_block` waits for an earlier one to end. Warns if
        // the clip isn't found.
        "chr_animate" | "chr_animate_block" => {
            let (Some(id), Some(clip)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let f = &mut host.world().fighters[id];
            if name == "chr_animate_block" && f.motion.lock_frames > 1 {
                return Ok((Value::Nil, Value::Boolean(true), None, None)); // wait for the earlier one
            }
            let frames = int(a(2)).unwrap_or(0);
            let interp = int(a(3)).filter(|&n| n >= 0).map(|n| n as u16);
            let ch = f.character.clone();
            let own = f
                .motion
                .coll
                .entries
                .iter()
                .any(|e| e.name.eq_ignore_ascii_case(&clip));
            let elsewhere = if own { None } else { host.clip(&clip) };
            let f = &mut host.world().fighters[id];
            if !f.motion.animate(&ch, &clip, frames, interp, elsewhere) {
                state.warn(format!("chr_animate: no animation {clip}"));
            }
            nil()
        }

        // A team by its name in the original's list (exactly as written).
        "chr_changeteam" => {
            const NAMES: [(&str, &str); 8] = [
                ("Konoko", "konoko"),
                ("TCTF", "tctf"),
                ("Syndicate", "syndicate"),
                ("Neutral", "neutral"),
                ("SecurityGuard", "securityGuard"),
                ("RogueKonoko", "rogueKonoko"),
                ("Switzerland", "switzerland"),
                ("SyndicateAccessory", "syndicateAccessory"),
            ];
            let wanted = a(1).and_then(text).unwrap_or_default();
            let Some(&(_, ours)) = NAMES.iter().find(|(n, _)| *n == wanted) else {
                state.warn(format!("chr_changeteam: no team {wanted:?}"));
                return nil();
            };
            if let Some(p) = fighter(host, a(0))
                .and_then(|id| host.world().fighters.get_mut(id))
                .and_then(|f| f.perception.as_mut())
            {
                p.team = crate::ai::perception::Team::from_name(ours);
            }
            nil()
        }
        // `chr_shadow who on`: shows or takes away the character's shadow.
        "chr_shadow" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.no_shadow = int(a(1)).unwrap_or(0) <= 0;
            }
            nil()
        }
        // `chr_death_lock who on`: the character can die but its body is never
        // removed.
        "chr_death_lock" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.death.lock = int(a(1)).unwrap_or(0) != 0;
            }
            nil()
        }
        // --- waiting on a character ---------------------------------------
        // `chr_wait_animtype` / `_animation` / `_animstate who name…`: waits until
        // the character plays one of those animation types (body or overlay), clips,
        // or states (names matched ignoring case and `_`).
        "chr_wait_animtype" | "chr_wait_animation" | "chr_wait_animstate" => {
            let Some(id) = fighter(host, a(0)) else {
                return nil();
            };
            let f = &host.world().fighters[id];
            // (A type counts if the body or an overlay plays it.)
            let now: Vec<String> = match name {
                "chr_wait_animtype" => f
                    .anim_types()
                    .into_iter()
                    .map(|t| {
                        oni_schema::name(&oni_schema::ANIM_TYPES, t)
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect(),
                "chr_wait_animstate" => {
                    vec![
                        oni_schema::name(&oni_schema::ANIM_STATES, f.motion.current().to_state)
                            .unwrap_or("")
                            .to_string(),
                    ]
                }
                _ => vec![f.motion.name().to_string()],
            };
            let key = |s: &str| {
                s.chars()
                    .filter(|c| *c != '_')
                    .collect::<String>()
                    .to_ascii_lowercase()
            };
            let now: Vec<String> = now.iter().map(|n| key(n)).collect();
            let hit = args
                .iter()
                .skip(1)
                .filter_map(text)
                .any(|w| now.contains(&key(&w)));
            Ok((Value::Nil, Value::Boolean(!hit), None, None))
        }
        // `chr_wait_health who hp`: waits until the character's hit points are `hp`
        // or less.
        "chr_wait_health" => {
            let (Some(id), Some(hp)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let above = host.world().fighters[id].hp as i64 > hp;
            Ok((Value::Nil, Value::Boolean(above), None, None))
        }
        "chr_is_player" => done(Value::Integer((fighter(host, a(0)) == Some(0)) as i64)),
        // How many of the references in a range have all their glass broken.
        "env_broken" => {
            let Some(from) = int(a(0)) else { return nil() };
            let to = int(a(1)).unwrap_or(from);
            done(Value::Integer(i64::from(
                host.world().env_broken(from as u32, to as u32),
            )))
        }
        // `chr_has_empty_weapon who` → 1 if it holds an empty gun with nothing to
        // reload it and no reload under way, else 0.
        "chr_has_empty_weapon" => {
            let empty = fighter(host, a(0)).is_some_and(|id| host.world().has_empty_weapon(id));
            done(Value::Integer(empty as i64))
        }
        // `chr_has_lsi who` → 1 if the character carries the LSI, else 0.
        "chr_has_lsi" => {
            let lsi = fighter(host, a(0))
                .and_then(|id| host.world().fighters.get(id))
                .is_some_and(|f| f.inventory.lsi);
            done(Value::Integer(lsi as i64))
        }
        // → 1 if the player killed Griffin (as a script said this session, else as
        // kept with the saved game), else 0.
        "did_kill_griffen" => done(Value::Integer(did_kill_griffin(host) as i64)),
        "killed_griffen" => {
            // (`ONrWeKilledGriffen`: kept with the saved game.)
            let murder = truthy(a(0));
            let world = host.world();
            world.progress.killed_griffin = Some(murder);
            if world.progress.persist_continues {
                world
                    .persist
                    .push(crate::game::Persist::KilledGriffin(murder));
            }
            nil()
        }
        // → the difficulty level (0 easy, 1 normal, 2 hard).
        "difficulty" => done(Value::Integer(host.world().difficulty.level)),

        // (Not this area's: the next's.)
        _ => super::objects::dispatch(state, host, lua, name, args),
    }
}
