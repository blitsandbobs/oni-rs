//! Scenario functions: the level: its geometry, doors, consoles, lasers, turrets, powerups and jello.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // jello_objects() → {name, …}: the placed objects see-through this tick
    // (in the camera's way).
    r.def("jello_objects", |_, host, ()| {
        let w = &host.world;
        let names = w.object_states();
        Ok(w.see_through
            .objects
            .iter()
            .filter_map(|&k| names.get(k).map(|n| n.0.clone()))
            .collect::<Vec<_>>())
    })?;

    // jello_doors() → {quads, leaves}: how many of the see-through quads this
    // tick are closed doors' own (their "gunk"), and the leaves of open or
    // moving doors see-through as objects ("door id:leaf").
    r.def("jello_doors", |lua, host, ()| {
        let w = &host.world;
        let t = lua.create_table()?;
        t.set(
            "quads",
            w.see_through
                .quads
                .iter()
                .filter(|&&q| {
                    w.quads
                        .quads
                        .get(q as usize)
                        .is_some_and(|q| q.object_tag >> 24 == 3)
                })
                .count(),
        )?;
        let leaves: Vec<String> = w
            .see_through
            .doors
            .iter()
            .filter_map(|&(o, l)| {
                w.doors
                    .list
                    .iter()
                    .find(|d| d.object_id == o)
                    .map(|d| format!("{}:{l}", d.id))
            })
            .collect();
        t.set("leaves", leaves)?;
        Ok(t)
    })?;

    // object_in_way(ax, ay, az, bx, by, bz) → whether the segment between
    // the points meets an animating object (a placed object, an open or
    // moving door's leaf: `AMrRayToObject`, as a lens flare's test asks).
    r.def(
        "object_in_way",
        |_, host, (ax, ay, az, bx, by, bz): (f32, f32, f32, f32, f32, f32)| {
            Ok(host
                .world
                .segment_meets_object(Vec3::new(ax, ay, az), Vec3::new(bx, by, bz)))
        },
    )?;

    // The flat shade `env_shade` put on level part `r` (its script
    // reference) → {r, g, b} (0–1), or nil.
    r.def("env_shade_of", |_, host, r: u32| {
        Ok(host.world.parts.shades.get(&r).map(|c| c.to_vec()))
    })?;

    // door_shade_of(object id) → {r, g, b}: the door's flat shade (0–1).
    r.def("door_shade_of", |_, host, id: u32| {
        Ok(host.world.doors.shades.get(&id).map(|c| c.to_vec()))
    })?;

    // door_ids() → {object id, …}: the level's doors.
    r.def("door_ids", |_, host, ()| {
        Ok(host
            .world
            .doors
            .list
            .iter()
            .map(|d| d.object_id)
            .collect::<Vec<_>>())
    })?;

    // Each live placed object's flat shade, in order: {r, g, b} (0–1), or false
    // for none.
    r.def("obj_shades", |lua, host, ()| {
        let out = lua.create_table()?;
        for o in &host.world.placed.list {
            match o.shade {
                Some(c) => out.set(out.raw_len() + 1, c.to_vec())?,
                None => out.set(out.raw_len() + 1, false)?,
            }
        }
        Ok(out)
    })?;

    // laser_eyes() → {{x, y, z}, …}: where the laser triggers' emitter eyes
    // are drawn (#78).
    r.def("laser_eyes", |_, host, ()| {
        Ok(host
            .world
            .lasers
            .iter()
            .flat_map(|l| l.eye_draws())
            .map(|(_, m)| {
                let t = m.w_axis;
                vec![t.x, t.y, t.z]
            })
            .collect::<Vec<_>>())
    })?;

    // Sends an AI to raise the alarm at console `console` (nil: the nearest).
    r.def("alarm", |_, host, (id, console): (usize, Option<u16>)| {
        host.world.order_alarm(id, console);
        Ok(())
    })?;

    // Resets console `id` as if never used: not triggered, and active or not as
    // the level authored it.
    r.def("console_reset", |_, host, id: u16| {
        for k in host.world.consoles.iter_mut().filter(|k| k.id == id) {
            k.triggered = false;
            k.active = k.initial_active;
        }
        Ok(())
    })?;

    // objects() → {{name, at={x,y,z}, frame, animating}, …}: the live placed
    // objects.
    r.def("objects", |lua, host, ()| {
        let t = lua.create_table()?;
        for (k, (name, at, frame, animating)) in host.world.object_states().into_iter().enumerate()
        {
            let o = lua.create_table()?;
            o.set("name", name)?;
            o.set("at", vec![at.x, at.y, at.z])?;
            o.set("frame", frame)?;
            o.set("animating", animating)?;
            t.set(k + 1, o)?;
        }
        Ok(t)
    })?;

    // door_obstruction(id) → {{x1, y1, z1, x2, y2, z2}, …}: where door id's
    // leaves stand in the way of a path (each one's base edge at the end of
    // its move; none for a leaf opening upward).
    r.def("door_obstruction", |_, host, id: u16| {
        Ok(host
            .world
            .doors
            .list
            .iter()
            .find(|d| d.id == id)
            .map(|d| {
                d.obstruction()
                    .iter()
                    .map(|(a, b)| vec![a.x, a.y, a.z, b.x, b.y, b.z])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default())
    })?;

    // door_leaves(id) → {{at={x,y,z}, half={x,y,z}, solid, paused}, …}: door
    // id's leaf boxes this tick.
    r.def("door_leaves", |lua, host, id: u16| {
        let t = lua.create_table()?;
        for (k, (at, half, solid, paused)) in host.world.doors.leaves(id).into_iter().enumerate() {
            let l = lua.create_table()?;
            l.set("at", vec![at.x, at.y, at.z])?;
            l.set("half", vec![half.x, half.y, half.z])?;
            l.set("solid", solid)?;
            l.set("paused", paused)?;
            t.set(k + 1, l)?;
        }
        Ok(t)
    })?;

    // Orders door `id`: "open", "close", "lock", "unlock", "jam", "unjam".
    r.def("door", |_, host, (id, what): (u16, String)| {
        let w = &mut host.world;
        match what.as_str() {
            "lock" => w.door_set(i64::from(id), Some(true), None),
            "unlock" => w.door_set(i64::from(id), Some(false), None),
            "jam" => w.door_set(i64::from(id), None, Some(true)),
            "unjam" => w.door_set(i64::from(id), None, Some(false)),
            _ => {
                for d in w.doors.list.iter_mut().filter(|d| d.id == id) {
                    if what == "open" {
                        d.open();
                    } else {
                        d.close();
                    }
                }
            }
        }
        Ok(())
    })?;

    // How many level quads are see-through (jello) for the camera this tick.
    r.def("jello", |_, host, ()| {
        Ok(host.world.see_through.quads.len())
    })?;

    // animated_textures() → {{name, frame}, …}: the animated textures the
    // level and its models draw, and the frame each shows at the game time
    // (as of the last screenshot).
    r.def("animated_textures", |lua, host, ()| {
        let out = lua.create_table()?;
        for (k, (name, frame)) in host.renderer.animated_textures().into_iter().enumerate() {
            let t = lua.create_table()?;
            t.set("name", name)?;
            t.set("frame", frame)?;
            out.set(k + 1, t)?;
        }
        Ok(out)
    })?;

    // jello_drawn() → how many see-through (jello) triangles the last
    // screenshot drew, and how many of them were doors'.
    r.def("jello_drawn", |_, host, ()| {
        Ok((
            host.renderer.jello_triangles(),
            host.renderer.jello_door_triangles(),
        ))
    })?;

    // set_jello(quad, on): marks a level quad see-through or not, as the
    // camera does (until the camera's next update clears its own).
    r.def("set_jello", |_, host, (q, on): (u32, bool)| {
        if let Some(quad) = host.world.quads.quads.get_mut(q as usize) {
            if on {
                quad.flags |= crate::quads::flag::JELLO;
            } else {
                quad.flags &= !crate::quads::flag::JELLO;
            }
        }
        Ok(())
    })?;

    // ray(x1, y1, z1, x2, y2, z2) → the first level quad between two points
    // (any flags): {quad, at={x,y,z}, flags, object_tag, jello} — jello: it is
    // see-through for the camera now; nil if nothing is in the way.
    r.def(
        "ray",
        |lua, host, (x1, y1, z1, x2, y2, z2): (f32, f32, f32, f32, f32, f32)| {
            let w = &host.world;
            let (a, b) = (Vec3::new(x1, y1, z1), Vec3::new(x2, y2, z2));
            let Some(hit) = w.quads.ray(a, b - a, 0) else {
                return Ok(None);
            };
            let q = &w.quads.quads[hit.quad as usize];
            let t = lua.create_table()?;
            t.set("quad", hit.quad)?;
            t.set("at", vec![hit.point.x, hit.point.y, hit.point.z])?;
            t.set("flags", q.flags)?;
            t.set("object_tag", q.object_tag)?;
            t.set("jello", w.see_through.quads.contains(&hit.quad))?;
            Ok(Some(t))
        },
    )?;

    // Level quad `q`'s plane → a, b, c (its normal), d.
    r.def("quad_plane", |_, host, q: u32| {
        let quads = &host.world.quads;
        let (n, d) = quads.plane(quads.quads.get(q as usize).map_or(0, |x| x.plane));
        Ok((n.x, n.y, n.z, d))
    })?;

    // Turrets: orders the turrets with script id `id`: "activate",
    // "deactivate" or "reset".
    r.def("turret", |_, host, (id, what): (i64, String)| {
        host.world.turret_set(id, &what);
        Ok(())
    })?;

    // The first turret with script id `id` → {active, yaw, pitch (degrees),
    // target, firing, shots, idle_effect}, or nil: none.
    r.def("turret_state", |lua, host, id: u16| {
        let Some(t) = host.world.turrets.iter().find(|t| t.id == id) else {
            return Ok(mlua::Value::Nil);
        };
        let r = lua.create_table()?;
        r.set("active", t.active)?;
        r.set("yaw", t.yaw.to_degrees())?;
        r.set("pitch", t.pitch.to_degrees())?;
        r.set("target", t.target)?;
        r.set("firing", t.firing)?;
        r.set("shots", t.shots)?;
        r.set("idle_effect", t.background)?;
        Ok(mlua::Value::Table(r))
    })?;

    // Shows or hides level part `r` (`env_show`): drawn, solid and blocking sight
    // only while shown; hiding glass breaks it.
    r.def("env_show", |_, host, (r, on): (u32, bool)| {
        host.world.env_show(r, on);
        Ok(())
    })?;

    // How many of the script references `from..=to` (`to` defaults to `from`)
    // have no unbroken glass pane left (`env_broken`).
    r.def("env_broken", |_, host, (from, to): (u32, Option<u32>)| {
        Ok(host.world.env_broken(from, to.unwrap_or(from)))
    })?;

    // How many powerups lie about the level.
    r.def("powerups_lying", |_, host, ()| {
        Ok(host.world.powerups().len())
    })?;

    // powerups() → {{kind, at={x,y,z}, falling}, …}: the powerups lying
    // about (falling: still with physics).
    r.def("powerups", |lua, host, ()| {
        let t = lua.create_table()?;
        for (k, p) in host.world.powerups().iter().enumerate() {
            let o = lua.create_table()?;
            o.set("kind", crate::world::POWERUPS[p.kind])?;
            o.set("at", vec![p.position.x, p.position.y, p.position.z])?;
            o.set("falling", p.body.is_some())?;
            t.set(k + 1, o)?;
        }
        Ok(t)
    })?;

    // Orders laser trigger `id` ("activate", "deactivate", "reset", "hide",
    // "show", "speed" with a factor).
    r.def(
        "laser",
        |_, host, (id, what, factor): (i64, String, Option<f32>)| {
            host.world.laser_set(id, &what, factor.unwrap_or(1.0));
            Ok(())
        },
    )?;

    // Laser trigger `id` → active, on, triggered, and how many beams are out (all
    // nil: no such laser).
    r.def("laser_state", |_, host, id: u16| {
        let l = host.world.lasers.iter().find(|l| l.id == id);
        Ok((
            l.map(|l| l.active),
            l.map(|l| l.laser_on),
            l.map(|l| l.triggered),
            l.map(|l| if l.beaming() { l.emitters.len() } else { 0 }),
        ))
    })?;

    // Laser beams out now: a list of {x1, y1, z1, x2, y2, z2}.
    r.def("beams", |_, host, ()| {
        Ok(host
            .world
            .beams
            .iter()
            .map(|(a, b, _)| vec![a.x, a.y, a.z, b.x, b.y, b.z])
            .collect::<Vec<_>>())
    })?;

    // A door's state ("closed" / "opening" / "open" / "closing") and
    // whether it is locked (nil: no such door).
    r.def("door_state", |_, host, id: u16| {
        let d = host.world.doors.list.iter().find(|d| d.id == id);
        Ok((d.map(|d| d.state_name()), d.map(|d| d.locked)))
    })?;

    // A console's state: "inactive", "active" or "triggered" (nil: none).
    r.def("console_state", |_, host, id: u16| {
        Ok(host
            .world
            .consoles
            .iter()
            .find(|k| k.id == id)
            .map(|k| k.screen()))
    })?;
    Ok(())
}
