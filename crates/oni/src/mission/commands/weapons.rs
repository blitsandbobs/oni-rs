//! Engine commands: weapons.

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
        // --- weapons ---------------------------------------
        // `chr_giveweapon who class`: a new gun of that class in its hand (the one it
        // held is deleted); warns if no such weapon.
        "chr_giveweapon" => {
            let (Some(id), Some(class)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if !host.world().give_weapon(id, &class) {
                state.warn(format!("chr_giveweapon: no weapon {class:?}"));
            }
            nil()
        }
        // `weapon_spawn class flag` / `powerup_spawn kind flag [amount]`: a gun or
        // powerup placed at the flag; warns if no such flag or kind.
        "weapon_spawn" | "powerup_spawn" => {
            let (Some(what), Some(flag)) = (a(0).and_then(text), int(a(1))) else {
                return nil();
            };
            let world = host.world();
            let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() else {
                state.warn(format!("{name}: no flag {flag}"));
                return nil();
            };
            let made = if name == "weapon_spawn" {
                world
                    .spawn_weapon(
                        &what,
                        f.at + Vec3::Y * crate::weapons::PLACED_LIFT,
                        f.facing,
                        false,
                    )
                    .is_some()
            } else {
                world.spawn_powerup(&what, f.at, int(a(2)).map(|n| n as u16))
            };
            if !made {
                state.warn(format!("{name}: no such {what:?}"));
            }
            nil()
        }
        // give_powerup name [amount] [character]: limits ignored; a negative
        // character gives to everyone alive.
        "give_powerup" => {
            let Some(what) = a(0).and_then(text) else {
                return nil();
            };
            let amount = int(a(1)).map(|n| n as u16);
            let who: Vec<usize> = match int(a(2)) {
                Some(n) if n < 0 => (0..host.world().fighters.len())
                    .filter(|&i| host.world().fighters[i].alive())
                    .collect(),
                Some(_) | None if a(2).is_some() => fighter(host, a(2)).into_iter().collect(),
                _ => vec![0],
            };
            for id in who {
                host.world().give_powerup(id, &what, amount, true);
            }
            nil()
        }
        // `chr_forceholster who on`: puts the gun in hand away into the hidden slot
        // (or onto the floor if that is taken), or gives it back into an empty hand.
        "chr_forceholster" => {
            let Some(id) = fighter(host, a(0)) else {
                return nil();
            };
            host.world().force_holster(id, int(a(1)).unwrap_or(0) > 0);
            nil()
        }
        // `chr_inv_reset who`: takes every gun from the character and empties its
        // pockets.
        "chr_inv_reset" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().reset_inventory(id);
            }
            nil()
        }
        // `chr_weapon_immune who`: shots no longer hurt the character (for good).
        "chr_weapon_immune" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.weapon_immune = true;
            }
            nil()
        }
        // `chr_unstoppable who on`: blows don't stun it, knock it about or make it
        // react, and it blocks without block stun.
        "chr_unstoppable" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.unstoppable = int(a(1)).unwrap_or(0) > 0;
            }
            nil()
        }
        // `chr_poison who amount every [initial]` (called each tick): poisons the
        // character: at `initial` ticks of poisoning it takes `amount`, then every
        // `every` ticks; 30 ticks without a call and the count restarts.
        "chr_poison" => {
            let (Some(id), Some(amount), Some(every)) = (fighter(host, a(0)), int(a(1)), int(a(2)))
            else {
                return nil();
            };
            let initial = int(a(3)).unwrap_or(every);
            host.world()
                .poison(id, amount as i32, every as i32, initial as i32);
            nil()
        }
        // Kills every AI, every AI "except" one, or one.
        "ai2_kill" => {
            let spare = a(0).and_then(text).is_some_and(|t| t == "except");
            let chosen = if spare {
                fighter(host, a(1))
            } else {
                a(0).and_then(|_| fighter(host, a(0)))
            };
            if a(0).is_some() && !spare && chosen.is_none() {
                return nil(); // (already gone)
            }
            for (i, f) in host.world().fighters.iter_mut().enumerate() {
                let picked = match (chosen, spare) {
                    (Some(c), false) => i == c,
                    (Some(c), true) => i != c,
                    (None, _) => true,
                };
                if picked && !f.player && f.alive() && !f.gone {
                    // (An act of god: kills even the invincible and the
                    // unkillable.)
                    let (was, unkillable, all) = (f.invincible, f.unkillable, f.hp);
                    (f.invincible, f.unkillable) = (false, false);
                    f.take_damage(all, Vec3::ZERO);
                    (f.invincible, f.unkillable) = (was, unkillable);
                }
            }
            nil()
        }
        // (Every character here is always active.)
        "chr_lock_active" | "chr_unlock_active" => nil(),

        "env_shade" => {
            // (`env_shade ref ref r g b`: those level parts lit flat.)
            if let (Some(lo), Some(hi)) = (int(a(0)), int(a(1))) {
                let c = |k: usize| float(a(k)).unwrap_or(1.0);
                host.world()
                    .env_shade(lo as u32, hi as u32, [c(2), c(3), c(4)]);
            }
            nil()
        }
        // `obj_shade a b r g b`: placed objects `a`–`b` lit flat in this colour (each
        // 0–1).
        "obj_shade" => {
            if let (Some(lo), Some(hi)) = (int(a(0)), int(a(1))) {
                let c = |k: usize| float(a(k)).unwrap_or(1.0);
                host.world()
                    .obj_shade(lo as u32, hi as u32, [c(2), c(3), c(4)]);
            }
            nil()
        }
        // `obj_force_draw a [b]` (`OBcFlags_ForceDraw`): only kept; every object is
        // drawn already (the port doesn't cull them).
        "obj_force_draw" => {
            if let Some(lo) = int(a(0)) {
                let hi = int(a(1)).unwrap_or(lo);
                host.world().obj_force_draw(lo as u32, hi as u32);
            }
            nil()
        }
        // The frame clock (`iComputeDeltaTicks`): a mark in a cutscene clears
        // the ticks owed; "on" / "off" whether it keeps up at all.
        "cutscene_sync" => {
            let world = host.world();
            match a(0)
                .and_then(text)
                .map(|s| s.trim().to_ascii_lowercase())
                .as_deref()
            {
                None | Some("mark") => {}
                Some("on") => world.clock.sync = true,
                Some("off") => world.clock.sync = false,
                Some(other) => state.warn(format!("cutscene_sync: invalid parameter {other}")),
            }
            world.clock.behind = 0;
            nil()
        }
        // Slow motion for this many ticks (0: off at once; else at least).
        "slowmo" => {
            let n = int(a(0)).unwrap_or(0).max(0) as u32;
            let world = host.world();
            world.clock.slowmo = if n == 0 { 0 } else { world.clock.slowmo.max(n) };
            nil()
        }
        // `particle_temp_start` / `particle_temp_stop`: whether new particles are
        // temporary (they, and what they emit, go at `particle_temp_kill`).
        "particle_temp_start" | "particle_temp_stop" => {
            host.world().armory.particles.temporary = name == "particle_temp_start";
            nil()
        }
        // Kills the temporary particles (`particle_temp_start`).
        "particle_temp_kill" => {
            host.world().kill_temporary_particles();
            nil()
        }
        // `chr_boss_shield who` (`ONcCharacterFlag_BossShield`): a boss shield on the
        // character for good: shots flare it and do only the class's share; drawn
        // red.
        "chr_boss_shield" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().fighters[id].boss_shield = true;
            }
            nil()
        }
        // `chr_super who amount`: sets the character's Daodan power
        // (`external_super_amount`; its power eases toward it).
        "chr_super" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().fighters[id].external_super = float(a(1)).unwrap_or(0.0);
            }
            nil()
        }
        // `ai2_barabbas_retrievegun who` (`WPrFindBarabbasWeapon`): Barabbas's gun
        // back in his hand at once, unless he holds a gun, someone else holds it, or
        // it lies on the building's top floor.
        "ai2_barabbas_retrievegun" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().barabbas_retrieve_gun(id);
            }
            nil()
        }
        "ai2_followme" => {
            // (`AI2rPath_FollowCharacter`: one AI, or every AI, walks to where
            // the player is — stopping within 12 of her; each new path heads
            // for where she is then.)
            let world = host.world();
            let Some(player) = world.fighters.first().map(|p| p.feet()) else {
                return nil();
            };
            let who: Vec<usize> = match a(0) {
                Some(v) => ai(host, Some(v)).into_iter().collect(),
                None => (1..host.world().fighters.len()).collect(),
            };
            for id in who {
                let world = host.world();
                let alert = world.fighters[id]
                    .perception
                    .as_ref()
                    .map_or(crate::ai::perception::Alert::Lull, |p| p.alert);
                if let Some(crate::ai::Brain::Agent(ag)) = world.fighters[id].brain.as_mut() {
                    ag.script_move(player, FOLLOW_DISTANCE, None, None, alert, Some(0));
                    if let Some(n) = ag.nav.as_mut() {
                        n.follow = Some(0);
                    }
                }
            }
            nil()
        }
        // `env_show ref on`: shows or hides a level part: drawn, solid and blocking
        // sight only while shown (hiding glass breaks it; showing mends it).
        "env_show" => {
            if let Some(r) = int(a(0)) {
                host.world().env_show(r as u32, truthy(a(1)));
            }
            nil()
        }

        // (Not this area's: the next's.)
        _ => super::ai::dispatch(state, host, lua, name, args),
    }
}
