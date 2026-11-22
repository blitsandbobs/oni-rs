//! Scenario functions: the run itself: ticks, mode, seed, camera, screenshots, images.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // Switches reproducing the original's known bugs (`docs/deviations.md`) on
    // or off (omitted: left as is) → whether it is on now.
    r.def("faithful", |_, host, on: Option<bool>| {
        if let Some(on) = on {
            host.world
                .set_quirks(if on { Quirks::FAITHFUL } else { Quirks::FIXED });
        }
        Ok(host.world.quirks.faithful)
    })?;

    // Reseeds every fighter's weighted animation pick, for repeatable runs.
    r.def("seed", |_, host, n: u32| {
        for f in &host.world.fighters {
            f.motion.coll.seed(n);
        }
        Ok(())
    })?;

    // temp_path(name) → a path in the temp folder of this process's own
    // (`oni_<pid>_<name>`): for a screenshot a scenario reads back, which a
    // copy running at once (the fixed and faithful passes) mustn't share.
    r.def("temp_path", |_, _, name: String| {
        Ok(std::env::temp_dir()
            .join(format!("oni_{}_{name}", std::process::id()))
            .to_string_lossy()
            .into_owned())
    })?;

    // image_mean(path, x0, y0, x1, y1) → r, g, b (0–1): a saved screenshot's
    // mean colour over a region (fractions of its width and height).
    r.def(
        "image_mean",
        |_, _, (path, x0, y0, x1, y1): (String, f32, f32, f32, f32)| {
            let (w, h, rgba) = crate::render::load_png(std::path::Path::new(&path))
                .map_err(mlua::Error::external)?;
            let (ax, bx) = ((x0 * w as f32) as u32, ((x1 * w as f32) as u32).min(w));
            let (ay, by) = ((y0 * h as f32) as u32, ((y1 * h as f32) as u32).min(h));
            let mut sum = [0.0f64; 3];
            let mut n = 0.0;
            for y in ay..by {
                for x in ax..bx {
                    let k = ((y * w + x) * 4) as usize;
                    for c in 0..3 {
                        sum[c] += f64::from(rgba[k + c]) / 255.0;
                    }
                    n += 1.0;
                }
            }
            let n = f64::max(n, 1.0);
            Ok((sum[0] / n, sum[1] / n, sum[2] / n))
        },
    )?;

    // frame_ticks(n) → the ticks a frame runs with n passed on the clock
    // (at most 6, plus any the clock is behind).
    r.def("frame_ticks", |_, host, n: u32| {
        Ok(host.world.frame_ticks(n))
    })?;

    // How much slower than the clock the game runs: 2 in slow motion, else 1.
    r.def("time_scale", |_, host, ()| Ok(host.world.time_scale()))?;

    // world_stats() → what the world holds, for finding a runaway: live
    // particles (and the busiest classes), weapons, fighters, corpses.
    r.def("world_stats", |lua, host, ()| {
        let w = &host.world;
        let t = lua.create_table()?;
        let sys = w.particles();
        let (n, top) = sys.census();
        t.set("particles", n)?;
        let top: Vec<String> = top
            .iter()
            .map(|(c, k)| format!("{} {k}", sys.class(*c).name))
            .collect();
        t.set("top", top.join(", "))?;
        t.set("weapons", w.armory.weapons.len())?;
        t.set("fighters", w.fighters.iter().filter(|f| !f.gone).count())?;
        t.set("corpses", w.corpses.list.len())?;
        t.set("contrails", w.contrail_count())?;
        Ok(t)
    })?;

    // The game camera → x, y, z (where it is), vx, vy, vz (which way it
    // looks), busy (whether a script waits on it).
    r.def("camera", |_, host, ()| {
        let c = &host.world.camera;
        let (p, v) = (c.view.location, c.view.view);
        Ok((p.x, p.y, p.z, v.x, v.y, v.z, c.busy))
    })?;

    // Advances `n` 60 Hz game ticks (default 1): held keys apply each tick,
    // tapped buttons on the first; the level scripts run along.
    r.def("step", |_, host, n: Option<u32>| {
        host.step(n.unwrap_or(1));
        Ok(())
    })?;

    // Places the screenshot camera: eye ex, ey, ez looking at tx, ty, tz (world
    // units), overriding the game camera from then on.
    r.def(
        "cam",
        |_, host, (ex, ey, ez, tx, ty, tz): (f32, f32, f32, f32, f32, f32)| {
            host.eye = Some(Vec3::new(ex, ey, ez));
            host.target = Some(Vec3::new(tx, ty, tz));
            Ok(())
        },
    )?;

    // Renders one frame (default 800 × 600) to a PNG at `path`: the game camera,
    // or `cam()`'s, with the HUD or menu on top.
    r.def(
        "screenshot",
        |_, host, (path, w, h2): (String, Option<u32>, Option<u32>)| {
            host.screenshot(Path::new(&path), w.unwrap_or(800), h2.unwrap_or(600));
            Ok(())
        },
    )?;

    // render_frames(n, w, h[, step]) → the mean wall time of a frame in ms,
    // drawn offscreen and read back n times (a game tick between frames if
    // `step()`): for profiling the renderer.
    r.def(
        "render_frames",
        |_, host, (n, w, h2, step): (u32, u32, u32, Option<bool>)| {
            let start = std::time::Instant::now();
            for _ in 0..n {
                if step == Some(true) {
                    host.step(1);
                }
                let _ = host.frame_rgba(w, h2);
            }
            Ok(start.elapsed().as_secs_f64() * 1000.0 / f64::from(n.max(1)))
        },
    )?;
    Ok(())
}
