//! Scenario functions: sound, particles, decals, lens flares, fog and magic effects.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // fog() → r, g, b, start, end: the fog now (screen depth: −1 near, 1 far).
    r.def("fog", |_, host, ()| {
        let f = host.world.fog.fog;
        Ok((f.color[0], f.color[1], f.color[2], f.start, f.end))
    })?;

    // overall_volume() → the sound's overall volume now (0–1).
    r.def("overall_volume", |_, host, ()| {
        Ok(host.world.audio.volume())
    })?;

    // magic_drawn() → how many magic effects the last screenshot drew
    // (shields, invisibility's shimmer).
    r.def("magic_drawn", |_, host, ()| Ok(host.magic_drawn))?;

    // Lens flares: how many there are, and how many are shown now
    //.
    r.def("lens_flares", |_, host, ()| {
        let l = &host.world.lens;
        // (And how many are drawn — after the fog's fading too.)
        let drawn = host.world.sprites().iter().filter(|s| s.no_depth).count();
        Ok((l.len(), l.values().filter(|v| v.1 > 0.0).count(), drawn))
    })?;

    // flares_behind_characters(on): the option (#58) for this run's frames —
    // characters hide the lens flares behind them.
    r.def("flares_behind_characters", |_, host, on: bool| {
        host.renderer.flares_behind_characters = on;
        Ok(())
    })?;

    // lens_flare_near(x, y, z) → the opacity share now of the lens flare
    // nearest that point (0 hidden, 1 fully seen).
    r.def("lens_flare_near", |_, host, (x, y, z): (f32, f32, f32)| {
        let w = &host.world;
        let sys = &w.armory.particles;
        let at = Vec3::new(x, y, z);
        Ok(w.lens
            .iter()
            .filter_map(|(h, v)| {
                sys.get(*h)
                    .map(|p| (sys.world_position(p).distance(at), v.1))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|v| v.1))
    })?;

    // Sound: sounds() → the playing ambients, each {name, stage ("in",
    // "body", "stopping", "out"), clip (the sound playing now), left,
    // right (gains)}.
    r.def("sounds", |lua, host, ()| {
        let list = lua.create_table()?;
        for (i, s) in host.world.audio.heard().into_iter().enumerate() {
            let t = lua.create_table()?;
            t.set("name", s.name)?;
            t.set("stage", format!("{:?}", s.stage).to_lowercase())?;
            t.set("clip", s.clip)?;
            t.set("left", s.gain[0])?;
            t.set("right", s.gain[1])?;
            list.set(i + 1, t)?;
        }
        Ok(list)
    })?;

    // impulse(name [, x, y, z [, volume]]) plays an impulse sound (placed,
    // or not) → its channel (0–15) or nil.
    r.def("impulse", |_, host, (name, x, y, z, volume): (
                String,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
            )| {
                let at = match (x, y, z) {
                    (Some(x), Some(y), Some(z)) => Some(Vec3::new(x, y, z)),
                    _ => None,
                };
                let w = &mut host.world;
                Ok(w.audio.impulse_play(&name, at, volume, Some(&w.quads)))
            },)?;

    // The 16 sound channels → each {clip, locked, volume (linear), left, right
    // (output gains)}.
    r.def("channels", |lua, host, ()| {
        let list = lua.create_table()?;
        for (i, c) in host.world.audio.channels().into_iter().enumerate() {
            let t = lua.create_table()?;
            t.set("clip", c.clip)?;
            t.set("locked", c.locked)?;
            t.set("volume", c.volume)?;
            t.set("left", c.gain[0])?;
            t.set("right", c.gain[1])?;
            list.set(i + 1, t)?;
        }
        Ok(list)
    })?;

    // sound_objects() → the level's sound objects, each {ambient, x, y, z,
    // box (else a sphere), state ("Off", "Starting", "Playing",
    // "Stopping"), volume, playing}.
    r.def("sound_objects", |lua, host, ()| {
        let list = lua.create_table()?;
        for (i, (ambient, at, is_box, state, volume, playing)) in
            host.world.audio.sound_objects().into_iter().enumerate()
        {
            let t = lua.create_table()?;
            t.set("ambient", ambient)?;
            t.set("x", at.x)?;
            t.set("y", at.y)?;
            t.set("z", at.z)?;
            t.set("box", is_box)?;
            t.set("state", format!("{state:?}"))?;
            t.set("volume", volume)?;
            t.set("playing", playing)?;
            list.set(i + 1, t)?;
        }
        Ok(list)
    })?;

    // Sets the overall sound volume to head for (0–1).
    r.def("sound_volume", |_, host, v: f32| {
        host.world.audio.set_volume(v);
        Ok(())
    })?;

    // env_particles(tag) → how many of the level's environmental particles
    // tagged `tag` are alive.
    r.def("env_particles", |_, host, tag: String| {
        Ok(host.world.env_particles_alive(&tag))
    })?;

    // decals([class]) → { {class, static, alpha, vertices, triangles,
    // positions = {{x,y,z}…}, quad}, … }: the live decals (of a class),
    // static first, oldest first.
    r.def("decals", |lua, host, class: Option<String>| {
        let particles = &host.world.armory.particles;
        let out = lua.create_table()?;
        let fixed = particles.decals.fixed.len();
        for (k, d) in particles.decals.iter().enumerate() {
            let Some(p) = particles.get(d.particle) else {
                continue;
            };
            let name = &particles.classes.list[p.class].name;
            if class.as_ref().is_some_and(|c| c != name) {
                continue;
            }
            let t = lua.create_table()?;
            t.set("class", name.as_str())?;
            t.set("static", k < fixed)?;
            t.set("alpha", d.alpha)?;
            t.set("vertices", d.mesh.positions.len())?;
            t.set("triangles", d.mesh.indices.len() / 3)?;
            t.set("quad", d.mesh.quad)?;
            let ps = lua.create_table()?;
            for (i, v) in d.mesh.positions.iter().enumerate() {
                ps.set(i + 1, vec![v.x, v.y, v.z])?;
            }
            t.set("positions", ps)?;
            out.set(out.raw_len() + 1, t)?;
        }
        Ok(out)
    })?;

    // particles_of(class) → { {x, y, z, dx, dy, dz [, tint]}, … }: where
    // each live particle of a class is, its forward (Y) axis, and its colour
    // when that is a variable.
    r.def("particles_of", |lua, host, class: String| {
        let sys = &host.world.armory.particles;
        let out = lua.create_table()?;
        for (_, p) in sys.iter() {
            if sys.classes.list[p.class].name != class {
                continue;
            }
            let (at, o) = (sys.world_position(p), sys.world_orientation(p).y_axis);
            let mut v = vec![
                f64::from(at.x),
                f64::from(at.y),
                f64::from(at.z),
                f64::from(o.x),
                f64::from(o.y),
                f64::from(o.z),
            ];
            if let crate::p3::class::Value::Var(k) = sys.classes.list[p.class].tint {
                if let Some(crate::p3::Val::U(t)) = p.vars.get(k) {
                    v.push(f64::from(*t));
                }
            }
            out.set(out.raw_len() + 1, v)?;
        }
        Ok(out)
    })?;

    // particle_textures(class) → {file stem, …}: the texture each live
    // particle of a class shows now (an animated one's current frame).
    r.def("particle_textures", |_, host, class: String| {
        let sys = &host.world.armory.particles;
        Ok(sys
            .iter()
            .filter(|(_, p)| sys.classes.list[p.class].name == class)
            .map(|(_, p)| {
                host.world
                    .particle_texture(p)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect::<Vec<String>>())
    })?;

    // show_decals(on): whether screenshots draw the decals (to compare).
    r.def("show_decals", |_, host, on: bool| {
        host.show_decals = on;
        Ok(())
    })?;

    // decals_drawn(x, y, z) → {{quad, alpha}, …}: the decals drawn from
    // that eye this frame (alpha 0..1, as drawn).
    r.def("decals_drawn", |lua, host, (x, y, z): (f32, f32, f32)| {
        let out = lua.create_table()?;
        for (k, d) in host
            .world
            .decal_draws(Vec3::new(x, y, z))
            .iter()
            .enumerate()
        {
            let t = lua.create_table()?;
            t.set("quad", d.positions.quad)?;
            t.set("alpha", d.alpha)?;
            out.set(k + 1, t)?;
        }
        Ok(out)
    })?;

    // Sends the environmental particles tagged `tag` a command, as the BSL
    // `particle` does ("create", "kill", "reset", "do" with `event`, or an event
    // name) → false for an unknown command.
    r.def(
        "particle",
        |_, host, (tag, command, event): (String, String, Option<String>)| {
            Ok(host
                .world
                .env_particle_command(&tag, &command, event.as_deref()))
        },
    )?;

    // sounds_playing() → {name, …}: the sounds playing now, in start order.
    r.def("sounds_playing", |_, host, ()| {
        Ok(host.world.audio.playing_names())
    })?;

    // saying(id) → the line a character is saying (an ambient's name) or nil.
    r.def("saying", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .and_then(|f| f.speech.saying().map(str::to_string)))
    })?;
    Ok(())
}
