//! Scenario functions: any fighter: spawning, inspecting, test hooks.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // Whether fighter `id` is still in the level (not removed).
    r.def("present", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).is_some_and(|f| !f.gone))
    })?;

    // Deals damage to fighter `id` (no reaction unless it dies), with a
    // small shove along (dx, dz) if given — the blow's way, attacker to it.
    r.def(
        "damage",
        |_, host, (id, n, dx, dz): (usize, i32, Option<f32>, Option<f32>)| {
            if let Some(f) = host.world.fighters.get_mut(id) {
                let shove =
                    Vec3::new(dx.unwrap_or(0.0), 0.0, dz.unwrap_or(0.0)).normalize_or_zero() * 0.01;
                f.take_damage(n, shove);
            }
            Ok(())
        },
    )?;

    // lift(id, by, ticks): fighter `id` is lifted toward `by` as a weapon's
    // "picked up" damage does, for `ticks` (at most 60).
    r.def("lift", |_, host, (id, by, ticks): (usize, usize, u16)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.lift.timer = (f.lift.timer + ticks).min(60);
            f.lift.owner = Some(by);
        }
        Ok(())
    })?;

    // Test hook: sets fighter `id`'s hit points outright (no reaction).
    r.def("set_hp", |_, host, (id, v): (usize, i32)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.hp = v;
        }
        Ok(())
    })?;

    // Test hook: turn any fighter to an absolute yaw (body and target).
    r.def("set_facing", |_, host, (id, deg): (usize, f32)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.facing = crate::control::Facing::new(deg.to_radians());
        }
        Ok(())
    })?;

    // Test hook: force a reaction (e.g. a knockdown) on any fighter.
    r.def("knock", |_, host, (id, kind): (usize, u16)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.pending_reaction = kind;
            f.apply_reaction();
        }
        Ok(())
    })?;

    // Fighter `id`'s current animation type number (0: no such fighter).
    r.def("type_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .map(|f| f.motion.kind)
            .unwrap_or(0))
    })?;

    // Fighter `id`'s body state number (the current clip's end state; 0: no such
    // fighter).
    r.def("state_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .map(|f| f.motion.state)
            .unwrap_or(0))
    })?;

    // effects(id) → {shield, shield_shown, shield_active, shield_parts,
    // daodan, invisibility, invisible_for, invis_active, invis_alpha,
    // seen}: a fighter's shield and invisibility (`seen`: the AI can see
    // it).
    r.def("effects", |lua, host, id: usize| {
        let f = host
            .world
            .fighters
            .get(id)
            .ok_or_else(|| mlua::Error::runtime("no such fighter"))?;
        let t = lua.create_table()?;
        t.set("shield", f.inventory.shield)?;
        t.set("shield_shown", f.inventory.shield_display)?;
        t.set("shield_active", f.effects.shield_active)?;
        t.set("shield_parts", f.effects.shield_parts.to_vec())?;
        t.set("daodan", f.effects.daodan)?;
        t.set("invisibility", f.inventory.invisibility)?;
        t.set("invisible_for", f.inventory.invisible_frames)?;
        t.set("invis_active", f.effects.invis_active)?;
        t.set("invis_alpha", f.effects.invis_alpha)?;
        t.set("seen", host.world.can_be_seen(id))?;
        t.set("super", f.super_amount)?;
        t.set("super_shield", f.has_super_shield())?;
        t.set(
            "super_particles",
            f.super_handles.as_ref().map_or(0, Vec::len),
        )?;
        t.set("boss_shield", f.boss_shield)?;
        Ok(t)
    })?;

    // boss_of(id) → a boss's mode now ("ShootBeam", "Regenerate", …) or nil.
    r.def("boss_of", |_, host, id: usize| {
        Ok(
            match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::Agent(a)) => a.boss_mode().map(|m| format!("{m:?}")),
                _ => None,
            },
        )
    })?;

    // lock_of(id) → the frames fighter `id`'s scripted animation still holds
    // it (`animationLockFrames`; 0: free).
    r.def("lock_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .map_or(0, |f| f.motion.lock_frames))
    })?;

    // Fighter `id`'s body yaw (degrees; 0: no such fighter).
    r.def("facing_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .map(|f| f.facing.facing.to_degrees())
            .unwrap_or(0.0))
    })?;

    // Whether fighter `id` is dying or dead (false: no such fighter).
    r.def("dead", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).is_some_and(|f| f.motion.dying))
    })?;

    // Spawn a training dummy; returns its fighter id.
    r.def(
        "spawn",
        |_, host, (x, y, z, facing_deg, class): (f32, f32, f32, Option<f32>, Option<String>)| {
            let class = class.unwrap_or_else(|| host.opponent.clone());
            let kind = host.kind(&class).map_err(mlua::Error::runtime)?;
            let yaw = facing_deg.unwrap_or(180.0).to_radians();
            let id = host.world.spawn(&kind, Vec3::new(x, y, z), yaw, true);
            Ok(id as u32)
        },
    )?;

    // Goals: places one of the level's characters by name, as the level's
    // scripts spawn it (`ai2_spawn`: its costume, melee profile, goals and
    // alerts) → its fighter id; errors if the level places no such
    // character. (Its spawn script isn't run.)
    r.def("place", |_, host, name: String| {
        let placed = host
            .world
            .level_ai
            .characters
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&name))
            .cloned()
            .ok_or_else(|| mlua::Error::runtime(format!("no placed character {name:?}")))?;
        let id = crate::mission::spawn_placed(&mut *host, &placed).map_err(mlua::Error::runtime)?;
        Ok(id as u32)
    })?;

    // collisions(id) → walls, objects, characters, records, serious: what
    // fighter id's movement met last tick, and its AI's collision records.
    r.def("collisions", |_, host, id: usize| {
        let Some(f) = host.world.fighters.get(id) else {
            return Ok((0, 0, 0, 0, 0));
        };
        let (r, s) = f.avoid.count();
        Ok((f.collisions.0, f.collisions.1, f.collisions.2, r, s))
    })?;

    // Adds `clips` ammo clips and `cells` energy cells to fighter `id`'s
    // inventory (no carry limit).
    r.def(
        "give_ammo",
        |_, host, (id, clips, cells): (usize, u16, Option<u16>)| {
            if let Some(f) = host.world.fighters.get_mut(id) {
                f.inventory.ammo += clips;
                f.inventory.cells += cells.unwrap_or(0);
            }
            Ok(())
        },
    )?;

    // corpses() → {{class, x, y, z, drawn, dynamic}, …}: the level's
    // corpses (its own, then the dead of play), where each one's pelvis is,
    // and whether it is drawn (has a model, not cleared).
    r.def("corpses", |lua, host, ()| {
        let out = lua.create_table()?;
        let fixed = host.world.corpses.fixed;
        for (k, c) in host.world.corpses.list.iter().enumerate() {
            let at = c.bones.first().map_or(Vec3::ZERO, |m| m.w_axis.truncate());
            let t = lua.create_table()?;
            t.set("class", c.class.clone())?;
            t.set("x", at.x)?;
            t.set("y", at.y)?;
            t.set("z", at.z)?;
            t.set("drawn", c.model.is_some())?;
            t.set("dynamic", k >= fixed)?;
            out.set(out.raw_len() + 1, t)?;
        }
        Ok(out)
    })?;

    // powerup(id, kind [, amount]) gives a powerup the game's way (`ammo`,
    // `cell`, `hypo`, `shield`, `invis`, `lsi`) → how much it took.
    r.def(
        "powerup",
        |_, host, (id, kind, amount): (usize, String, Option<u16>)| {
            Ok(host.world.give_powerup(id, &kind, amount, false))
        },
    )?;

    // health_shade(id) → the colour of a fighter's health (melee flashes'
    // tint).
    r.def("health_shade", |_, host, id: usize| {
        let f = host
            .world
            .fighters
            .get(id)
            .ok_or_else(|| mlua::Error::runtime("no such fighter"))?;
        Ok(host.world.health_shade(f.hp, f.max_hp))
    })?;

    // shadow(id) → {alpha, quad, vertices, positions} or nil: fighter
    // `id`'s shadow this frame (and the quad it was placed by).
    r.def("shadow", |lua, host, id: usize| {
        let Some((mesh, alpha, _)) = host.world.shadow(id) else {
            return Ok(None);
        };
        let t = lua.create_table()?;
        t.set("alpha", alpha)?;
        t.set("quad", mesh.quad)?;
        t.set("vertices", mesh.positions.len())?;
        let ps = lua.create_table()?;
        for (i, v) in mesh.positions.iter().enumerate() {
            ps.set(i + 1, vec![v.x, v.y, v.z])?;
        }
        t.set("positions", ps)?;
        Ok(Some(t))
    })?;

    // Fighter `id`'s posed bone k in world space → {x={…}, y={…}, z={…},
    // at={…}} (axes and position); nil: no such fighter or bone.
    r.def("bone", |lua, host, (id, k): (usize, usize)| {
        let Some(m) = host
            .world
            .fighters
            .get(id)
            .and_then(|f| f.bones().get(k).copied())
        else {
            return Ok(None);
        };
        mat(lua, m).map(Some)
    })?;

    // Fighter `id`'s inventory → ammo clips, energy cells, hypos (0s: no such
    // fighter).
    r.def("inventory", |_, host, id: usize| {
        let i = host
            .world
            .fighters
            .get(id)
            .map(|f| f.inventory)
            .unwrap_or_default();
        Ok((i.ammo, i.cells, i.hypos))
    })?;

    // Moves fighter `id` (feet) to a point.
    r.def(
        "teleport",
        |_, host, (id, x, y, z): (usize, f32, f32, f32)| {
            host.world.teleport(id, Vec3::new(x, y, z), None);
            Ok(())
        },
    )?;

    // overlays_of(id) → the partial-body clips playing on fighter id: a list
    // of {slot = "recoil" | "inventory" | "hit", clip, type, frame}.
    r.def("overlays_of", |lua, host, id: usize| {
        let t = lua.create_table()?;
        if let Some(f) = host.world.fighters.get(id) {
            for (slot, o) in f.overlays.iter().enumerate() {
                let Some(o) = o else { continue };
                let c = f.motion.clip_of(o.clip);
                let e = lua.create_table()?;
                e.set("slot", ["recoil", "inventory", "hit"][slot])?;
                e.set("clip", f.motion.coll.entries[o.clip].name.clone())?;
                e.set("type", c.anim_type)?;
                e.set("frame", o.frame)?;
                t.push(e)?;
            }
        }
        Ok(t)
    })?;

    // passive_of(id) → whether fighter id's AI is passive (`ai2_passive`, a
    // cutscene); nil: no such fighter.
    r.def("passive_of", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).map(|f| f.passive))
    })?;

    // The id the level scripts know fighter `id` by: the player 0, a
    // `chr_create`d character its setup's; nil for the rest.
    r.def("script_id_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .and_then(|f| if id == 0 { Some(0) } else { f.script_id }))
    })?;

    // The AI's difficulty: "easy", "normal" or "hard".
    r.def("difficulty", |_, host, level: String| {
        let d = host.classes.difficulty(&level);
        host.world.difficulty = d;
        Ok((d.notice, d.dodge, d.block))
    })?;

    // Fighter `id`'s size (`ONtCharacter.scale`, drawn from its class's
    // range when it came), or nil.
    r.def("scale_of", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).map(|f| f.scale))
    })?;

    // Test hook: sizes fighter `id` (as the minime / behemoth cheats do).
    r.def("set_scale", |_, host, (id, scale): (usize, f32)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.set_scale(scale);
        }
        Ok(())
    })?;

    // Fighter `id`'s character class name, or nil.
    r.def("class_of", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).map(|f| f.class.to_string()))
    })?;

    // How many fighters there are (the player included, removed ones too).
    r.def("count", |_, host, ()| Ok(host.world.fighters.len() as u32))?;

    // Fighter `id`'s hit points (0: no such fighter).
    r.def("hp", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).map(|f| f.hp).unwrap_or(0))
    })?;

    // Fighter `id`'s feet → x, y, z (0, 0, 0: no such fighter).
    r.def("pos_of", |_, host, id: usize| {
        let f = host
            .world
            .fighters
            .get(id)
            .map(|f| f.feet())
            .unwrap_or(Vec3::ZERO);
        Ok((f.x, f.y, f.z))
    })?;

    // Fighter `id`'s current clip name ("": no such fighter).
    r.def("anim_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .map(|f| f.motion.name().to_string())
            .unwrap_or_default())
    })?;
    Ok(())
}
