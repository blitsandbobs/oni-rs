//! Engine commands: cutscene inserts, the HUD and the camera.

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
        // --- cutscene inserts (`hud/cinematic.rs`) ---------------------------
        "cinematic_start" => {
            // (name, width, height, start, end, speed, mirror)
            let Some(name) = a(0).and_then(text) else {
                return nil();
            };
            let n = |k: usize| int(a(k)).unwrap_or(0) as i32;
            let (speed, mirror) = (float(a(5)).unwrap_or(0.0), truthy(a(6)));
            let hud = &mut host.world().hud;
            let texture = hud.cinematic_texture(&name);
            if texture.is_none() {
                state.warn(format!("cinematic_start: no texture {name}"));
            }
            hud.cinematics
                .start(&name, texture, n(1), n(2), n(3), n(4), speed, mirror);
            nil()
        }
        // `cinematic_stop name end speed` (`OCrCinematic_Stop`): the first insert of
        // that texture slides to position `end` and goes.
        "cinematic_stop" => {
            let Some(name) = a(0).and_then(text) else {
                return nil();
            };
            host.world().hud.cinematics.stop(
                &name,
                int(a(1)).unwrap_or(0) as i32,
                float(a(2)).unwrap_or(0.0),
            );
            nil()
        }
        // Ends a cutscene (`ONrGameState_EndCutscene`): input and jello back, every
        // AI out of passivity, the letterbox gone, the holstered gun drawn again.
        "end_cutscene" => {
            host.world().end_cutscene();
            nil()
        }
        // Clears dangerous projectiles for a cutscene
        // (`P3iRemoveDangerousProjectiles`): particles of classes marked to die on a
        // cutscene die, those marked to expire expire next tick.
        "p3_removedangerous" => {
            host.world().remove_dangerous_projectiles();
            nil()
        }
        // --- the HUD ------------------------------------
        // `ui_show_element left|right on`: shows or hides the HUD's left or right
        // ring; warns on any other element.
        "ui_show_element" => {
            let (Some(which), on) = (a(0).and_then(text), int(a(1)).unwrap_or(0) > 0) else {
                return nil();
            };
            match which.as_str() {
                "left" => host.world().hud.show[0] = on,
                "right" => host.world().hud.show[1] = on,
                _ => state.warn(format!("ui_show_element: no element {which}")),
            }
            nil()
        }
        // (A debugging command: no level script uses it.)
        "ui_show_help" => {
            host.world().hud.help_on = int(a(0)).unwrap_or(0) > 0;
            nil()
        }
        // `ui_fill_element` / `ui_flash_element element on`: fills or flashes a
        // HUD element (`health`, `damage`, `invis`, …, or `all`), or stops.
        "ui_fill_element" | "ui_flash_element" => {
            let (Some(which), on) = (a(0).and_then(text), int(a(1)).unwrap_or(0) > 0) else {
                return nil();
            };
            let mask = crate::hud::Hud::element_mask(&which);
            if mask == 0 {
                state.warn(format!("{name}: no element {which}"));
            }
            let hud = &mut host.world().hud;
            let bits = if name == "ui_fill_element" {
                &mut hud.filled
            } else {
                &mut hud.flashing
            };
            if on {
                *bits |= mask;
            } else {
                *bits &= !mask;
            }
            nil()
        }
        // `fade_out r g b ticks`: fades the screen to that colour (each 0–1) over
        // `ticks`.
        "fade_out" => {
            let c = |k: usize| float(a(k)).unwrap_or(0.0).clamp(0.0, 1.0);
            let ticks = int(a(3)).unwrap_or(0).max(0) as u32;
            host.world().hud.fade_out([c(0), c(1), c(2)], ticks);
            nil()
        }
        // `fade_in ticks`: fades back from wherever a fade got to (no fade: nothing).
        "fade_in" => {
            let ticks = int(a(0)).unwrap_or(0).max(0) as u32;
            host.world().hud.fade_in(ticks);
            nil()
        }
        // --- the camera ---------------------------
        // A `_block` form waits while the camera is busy (easing or riding a
        // clip), then does as the plain one.
        "cm_interpolate"
        | "cm_interpolate_block"
        | "cm_anim"
        | "cm_anim_block"
        | "cm_orbit"
        | "cm_orbit_block"
            if name.ends_with("_block") && host.world().camera.busy =>
        {
            Ok((Value::Nil, Value::Boolean(true), None, None))
        }
        // `cm_interpolate clip ticks`: the camera eases to the camera clip's first
        // frame over `ticks` (`_block`: once the camera is free); warns if there is
        // no such clip.
        "cm_interpolate" | "cm_interpolate_block" => {
            let Some(clip) = a(0).and_then(text) else {
                return nil();
            };
            if !host
                .world()
                .camera_interpolate_to(&clip, int(a(1)).unwrap_or(1))
            {
                state.warn(format!("{name}: no camera clip {clip}"));
            }
            nil()
        }
        "cm_anim" | "cm_anim_block" => {
            // (The first word — move, look, both — changes nothing.)
            let Some(clip) = a(1).and_then(text) else {
                return nil();
            };
            if !host.world().camera_animate(&clip) {
                state.warn(format!("{name}: no camera clip {clip}"));
            }
            nil()
        }
        // `cm_orbit speed [stop]`: the camera circles the player at `speed` degrees a
        // tick, stopping at angle `stop` if given (`_block`: once free).
        "cm_orbit" | "cm_orbit_block" => {
            let speed = float(a(0)).unwrap_or(0.0);
            host.world().camera_orbit(speed, float(a(1)));
            nil()
        }
        "cm_wait" if host.world().camera.busy => Ok((Value::Nil, Value::Boolean(true), None, None)),
        // Waits while the camera is busy (easing or riding a clip; the guarded
        // arm just above does the waiting).
        "cm_wait" => nil(),
        // The camera follows the player again, snapped into place
        // (`CArFollow_Enter`).
        "cm_reset" => {
            host.world().camera_follow();
            nil()
        }
        // The camera holds still where it is.
        "cm_detach" => {
            host.world().camera_detach();
            nil()
        }
        // `cm_jello on`: whether walls between camera and player turn see-through.
        "cm_jello" => {
            host.world().camera.jello = int(a(0)).is_some_and(|i| i > 0);
            nil()
        }
        // `cm_barabus who away up ticks`: the camera eases over `ticks` to a view of
        // `who` from `away` back along the line from the camera, `up` higher (world
        // units).
        "cm_barabus" => {
            let (Some(who), Some(away), Some(up), Some(ticks)) =
                (fighter(host, a(0)), float(a(1)), float(a(2)), int(a(3)))
            else {
                state.warn("cm_barabus: expected <id> <away> <up> <time>".into());
                return nil();
            };
            host.world().camera_barabus(who, away, up, ticks);
            nil()
        }
        // `gs_fov_set deg`: the camera's vertical field of view (degrees).
        "gs_fov_set" => {
            if let Some(deg) = float(a(0)) {
                host.world().camera.fov = deg.to_radians();
            }
            nil()
        }
        // The fog's start / end eased over frames (`gl_fog_start_changeto`).
        "gl_fog_start_changeto" | "gl_fog_end_changeto" => {
            if let Some(v) = float(a(0)) {
                let frames = int(a(1)).unwrap_or(0) as i32;
                let fog = &mut host.world().fog;
                if name == "gl_fog_start_changeto" {
                    fog.start_to(v, frames);
                } else {
                    fog.end_to(v, frames);
                }
            }
            nil()
        }
        // `gs_farclipplane_set d`: the camera's far clip plane (world units).
        "gs_farclipplane_set" => {
            if let Some(far) = float(a(0)) {
                host.world().camera.far = far;
            }
            nil()
        }
        // `letterbox on`: slides the cutscene letterbox bars in (1) or out (0).
        "letterbox" => {
            host.world().cutscene.letterbox = truthy(a(0)) && int(a(0)) != Some(0);
            nil()
        }
        // `win` / `lose`: ends the level as won or lost (`game()`.victory).
        "win" | "lose" => {
            state.outcome = Some(name == "win");
            host.world().progress.victory = Some(name == "win");
            eprintln!("script: {name}");
            nil()
        }

        // (Not this area's: the next's.)
        _ => super::effects::dispatch(state, host, lua, name, args),
    }
}
