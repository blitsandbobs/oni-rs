//! Scenario functions: guns: held, aimed, fired, dropped and lying about.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // Weapons: puts a new gun of `class` in fighter
    // `id`'s hand (the one it held is deleted, as `chr_giveweapon`) →
    // whether it did (false: no such fighter or class).
    r.def("give_weapon", |_, host, (id, class): (usize, String)| {
        Ok(host.world.give_weapon(id, &class))
    })?;

    // The gun in fighter `id`'s hand → class, ammo, firing, chamber (the ticks
    // till it can fire again); all nil unarmed.
    r.def("weapon", |_, host, id: usize| {
        let w = &host.world;
        let gun = w
            .fighters
            .get(id)
            .and_then(|f| f.hand)
            .map(|k| &w.weapons()[k]);
        Ok((
            gun.map(|g| g.class.name.clone()),
            gun.map(|g| g.ammo),
            gun.map(|g| g.firing),
            gun.map(|g| g.chamber),
        ))
    })?;

    // Sets fighter `id`'s aim pitch (radians, up positive).
    r.def("aim", |_, host, (id, pitch): (usize, f32)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.aim.ud = pitch;
        }
        Ok(())
    })?;

    // Fighter `id`'s aim pitch (radians, up positive; 0: no such fighter).
    r.def("aim_of", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).map_or(0.0, |f| f.aim.ud))
    })?;

    // Fighter `id` lets go of the gun in its hand (`knock`: thrown clear, as a
    // knockdown does).
    r.def(
        "drop_weapon",
        |_, host, (id, knock): (usize, Option<bool>)| {
            host.world.drop_weapon(id, knock.unwrap_or(false));
            Ok(())
        },
    )?;

    // How many particles are alive (shots and every other effect).
    r.def("shots", |_, host, ()| {
        Ok(host.world.particles().iter().count())
    })?;

    // Puts a new gun of `class` at a point (falling: with physics, else resting
    // as placed) → whether the class exists.
    r.def(
        "spawn_weapon",
        |_, host, (class, x, y, z, falling): (String, f32, f32, f32, Option<bool>)| {
            Ok(host
                .world
                .spawn_weapon(&class, Vec3::new(x, y, z), 0.0, !falling.unwrap_or(false))
                .is_some())
        },
    )?;

    // loose_guns() → {{class, at={x,y,z}, moving}, …}: the guns no one holds
    // (moving: still sliding or falling).
    r.def("loose_guns", |lua, host, ()| {
        let t = lua.create_table()?;
        for (k, g) in host
            .world
            .weapons()
            .iter()
            .filter(|g| g.owner.is_none() && !g.gone)
            .enumerate()
        {
            let o = lua.create_table()?;
            o.set("class", g.class.name.clone())?;
            o.set("at", vec![g.position.x, g.position.y, g.position.z])?;
            o.set("moving", g.velocity.length() > 1e-3)?;
            t.set(k + 1, o)?;
        }
        Ok(t)
    })?;

    // The class of the gun fighter `id` has holstered, or nil.
    r.def("holster_of", |_, host, id: usize| {
        let w = &host.world;
        Ok(w.fighters
            .get(id)
            .and_then(|f| f.holster)
            .map(|k| w.weapons()[k].class.name.clone()))
    })?;

    // The gun in fighter `id`'s hand, as `bone()` → {x, y, z, at}; nil unarmed.
    r.def("held_gun", |lua, host, id: usize| {
        let w = &host.world;
        let Some(k) = w.fighters.get(id).and_then(|f| f.hand) else {
            return Ok(None);
        };
        mat(lua, w.armory.weapons[k].matrix.get()).map(Some)
    })?;

    // sight(id) → {from, to, target, laser, tunnel}: fighter id's gun sight
    // this tick (nil without one); target "none" | "environment" | "object" |
    // "character", laser its {r, g, b} (nil without one), tunnel how many
    // sprites.
    r.def("sight", |lua, host, id: usize| {
        let Some(s) = host.world.gun_sight(id) else {
            return Ok(None);
        };
        let t = lua.create_table()?;
        t.set("from", vec![s.from.x, s.from.y, s.from.z])?;
        t.set("to", vec![s.to.x, s.to.y, s.to.z])?;
        t.set(
            "target",
            match s.target {
                crate::world::SightTarget::None => "none",
                crate::world::SightTarget::Object => "object",
                crate::world::SightTarget::Environment => "environment",
                crate::world::SightTarget::Character => "character",
            },
        )?;
        t.set("laser", s.laser.map(|c| c.to_vec()))?;
        t.set("tunnel", s.tunnel.len())?;
        Ok(Some(t))
    })?;

    // Whether fighter `id`'s gun pokes through a wall
    // (`ONrCharacter_IsGunThroughWall`): such a gun neither fires nor shows its
    // sight.
    r.def("gun_through_wall", |_, host, id: usize| {
        Ok(host.world.gun_through_wall(id))
    })?;

    // How many guns no one holds (as `loose_guns()`: not those gone).
    r.def("weapons_lying", |_, host, ()| {
        Ok(host
            .world
            .weapons()
            .iter()
            .filter(|g| g.owner.is_none() && !g.gone)
            .count())
    })?;
    Ok(())
}
