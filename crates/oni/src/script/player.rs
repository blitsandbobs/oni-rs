//! Scenario functions: the player's controls and state (fighter 0).

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // Holds (or lets go of) the forward / backward move keys (W / S) for the
    // player.
    r.def("input", |_, host, (f, b): (bool, bool)| {
        host.hold(key::FORWARD, f);
        host.hold(key::BACKWARD, b);
        Ok(())
    })?;

    // feet_offset() → how far the player's posed body's lowest point (its
    // parts' bounding spheres, arms left out) is above its location.
    r.def("feet_offset", |_, host, ()| {
        let Some(f) = host.world.player() else {
            return Ok(0.0);
        };
        Ok(f.feet_offset())
    })?;

    // Holds (or lets go of) jump (a press, then held: the higher jump).
    r.def("hold_jump", |_, host, on: bool| {
        host.hold(key::JUMP, on);
        Ok(())
    })?;

    // Holds (or lets go of) fire 1.
    r.def("hold_fire", |_, host, on: bool| {
        host.hold(key::FIRE1, on);
        Ok(())
    })?;

    // Holds (or lets go of) the turn-left / turn-right keys (lying down: rolls
    // up).
    r.def("turn_keys", |_, host, (l, r): (bool, bool)| {
        host.hold(key::TURN_LEFT, l);
        host.hold(key::TURN_RIGHT, r);
        Ok(())
    })?;

    // Taps the action key for one tick (in a fight: taunt).
    r.def("action", |_, host, ()| {
        host.taps |= key::ACTION;
        Ok(())
    })?;

    // Holds (or lets go of) the strafe keys (A / D sidestep).
    r.def("strafe", |_, host, (l, r): (bool, bool)| {
        host.hold(key::STEP_LEFT, l);
        host.hold(key::STEP_RIGHT, r);
        Ok(())
    })?;

    // Holds (or lets go of) the walk modifier (CapsLock).
    r.def("walk", |_, host, on: bool| {
        host.hold(key::WALK, on);
        Ok(())
    })?;

    // Holds (or lets go of) the crouch key.
    r.def("crouch", |_, host, on: bool| {
        host.hold(key::CROUCH, on);
        Ok(())
    })?;

    // Tap a fire button (1 = left mouse, 2 = right, 3 = middle by default):
    // unarmed, these are punch / kick / crouch.
    r.def("fire", |_, host, n: u8| {
        let bit = match n {
            1 => key::FIRE1,
            2 => key::FIRE2,
            3 => key::FIRE3,
            _ => 0,
        };
        host.taps |= bit;
        Ok(())
    })?;

    // Taps crouch for one tick (unarmed: the crouch attack button).
    r.def("crouch_tap", |_, host, ()| {
        host.taps |= key::CROUCH;
        Ok(())
    })?;

    // Types the letters of `s` on the pause screen (cheat codes) → the message
    // the last finished code shows, or nil.
    r.def("type_keys", |_, host, s: String| {
        let mut last = None;
        for c in s.chars() {
            if let Some(m) = host.world.cheat_key(c) {
                last = Some(m.to_string());
            }
        }
        Ok(last)
    })?;

    // The player's body state number (`state_of(0)`).
    r.def("state", |_, host, ()| {
        Ok(host.world.player().map(|p| p.motion.state).unwrap_or(0))
    })?;

    // The player's current animation type number (`type_of(0)`).
    r.def("anim_type", |_, host, ()| {
        Ok(host.world.player().map(|p| p.motion.kind).unwrap_or(0))
    })?;

    // Queue a jump (fires on the next grounded tick); `airborne()` reads state.
    r.def("jump", |_, host, ()| {
        host.taps |= key::JUMP;
        Ok(())
    })?;

    // Whether the player is in the air.
    r.def("airborne", |_, host, ()| {
        Ok(host.world.player().map(|p| p.airborne()).unwrap_or(false))
    })?;

    // Melee: queue a punch / kick (fires on the next step's first tick).
    r.def("punch", |_, host, ()| {
        host.taps |= key::PUNCH;
        Ok(())
    })?;

    // Queues a kick (fires on the next step's first tick).
    r.def("kick", |_, host, ()| {
        host.taps |= key::KICK;
        Ok(())
    })?;

    // Snaps the player's body (and its facing target) to an absolute yaw
    // (degrees).
    r.def("facing", |_, host, deg: f32| {
        if let Some(p) = host.world.fighters.first_mut() {
            p.facing = crate::control::Facing::new(deg.to_radians());
        }
        Ok(())
    })?;

    // Nudges the player's facing target by `delta` degrees; the body slews toward
    // it over the next ticks (as the mouse turns).
    r.def("turn", |_, host, delta: f32| {
        if let Some(p) = host.world.fighters.first_mut() {
            p.facing.adjust_desired(delta.to_radians());
        }
        Ok(())
    })?;

    // The player's body yaw (degrees).
    r.def("facing_deg", |_, host, ()| {
        Ok(host
            .world
            .player()
            .map(|p| p.facing.facing.to_degrees())
            .unwrap_or(0.0))
    })?;

    // The aim direction: where the player points, without the body lean a
    // diagonal run or turned sidestep adds.
    r.def("aim_deg", |_, host, ()| {
        Ok(host
            .world
            .player()
            .map(|p| p.facing.aim().to_degrees())
            .unwrap_or(0.0))
    })?;

    // The player's facing target yaw (degrees), which the body slews toward.
    r.def("desired_deg", |_, host, ()| {
        Ok(host
            .world
            .player()
            .map(|p| p.facing.desired_facing.to_degrees())
            .unwrap_or(0.0))
    })?;

    // use_hypo(id) → whether fighter `id` used a hypo (as Tab does).
    r.def("use_hypo", |_, host, id: usize| Ok(host.world.use_hypo(id)))?;

    // The health fighter `id`'s hypo still has to give (0: none, or no such
    // fighter).
    r.def("hypo_left", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .map_or(0, |f| f.inventory.hypo_remaining))
    })?;

    // Holds (or lets go of) the look-mode key.
    r.def("look_mode", |_, host, on: bool| {
        host.hold(key::LOOK, on);
        Ok(())
    })?;

    // The player's look-mode turn (radians).
    r.def("aim_lr", |_, host, v: Option<f32>| {
        let Some(p) = host.world.fighters.first_mut() else {
            return Ok(0.0);
        };
        if let Some(v) = v {
            p.aim.lr = v;
        }
        Ok(p.aim.lr)
    })?;

    // Fighter `id` starts reloading its gun (`WPrTryReload`) → whether it did
    // (refused when full, busy, just fired, or out of clips).
    r.def("reload", |_, host, id: usize| Ok(host.world.reload(id)))?;

    // Taps the swap-weapon key for one tick (holster / draw).
    r.def("swap", |_, host, ()| {
        host.taps |= key::SWAP;
        Ok(())
    })?;

    // The player's feet → x, y, z.
    r.def("pos", |_, host, ()| {
        let f = host.world.player().map(|p| p.feet()).unwrap_or(Vec3::ZERO);
        Ok((f.x, f.y, f.z))
    })?;

    // Whether the player is on the ground.
    r.def("grounded", |_, host, ()| {
        Ok(host
            .world
            .player()
            .map(|p| p.physics.grounded)
            .unwrap_or(false))
    })?;

    // The current clip-to-clip stitch blend weight (0 = not blending).
    r.def("stitch", |_, host, ()| {
        Ok(host
            .world
            .player()
            .and_then(|p| p.motion.blend_pose())
            .map(|(_, t)| t)
            .unwrap_or(0.0))
    })?;

    // The player's current clip name.
    r.def("anim", |_, host, ()| {
        Ok(host
            .world
            .player()
            .map(|p| p.motion.name().to_string())
            .unwrap_or_default())
    })?;
    Ok(())
}
