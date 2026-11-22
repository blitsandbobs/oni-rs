//! Scenario functions: the AI: orders, goals, perception, melee, pathfinding.

use super::*;

pub(super) fn register(r: &Reg) -> mlua::Result<()> {
    // AI: walk fighter `id` along `points` ({{x, z}, …}) at `pace` ("run",
    // "walk" or "creep"), optionally looping; `route_done(id)` reads progress.
    r.def(
        "patrol",
        |_,
         host,
         (id, points, pace, looping): (usize, Vec<[f32; 2]>, Option<String>, Option<bool>)| {
            let pts = points.iter().map(|p| Vec3::new(p[0], 0.0, p[1])).collect();
            let pace = crate::ai::Pace::from_name(pace.as_deref());
            if let Some(f) = host.world.fighters.get_mut(id) {
                let route = crate::ai::Route::new(pts, pace, looping.unwrap_or(false));
                f.brain = Some(crate::ai::Brain::Route(route));
            }
            Ok(())
        },
    )?;

    // AI: hold fixed orders — direction "forward" | "backpedal" | "left" |
    // "right" | "stop", pace "run" | "walk" | "creep", optional facing (deg).
    r.def(
        "ai_order",
        |_, host, (id, dir, pace, face): (usize, String, Option<String>, Option<f32>)| {
            use crate::ai::{Brain, Direction, Orders, Pace};
            let direction = match dir.as_str() {
                "forward" => Direction::Forward,
                "backpedal" => Direction::Backpedal,
                "left" => Direction::SidestepLeft,
                "right" => Direction::SidestepRight,
                _ => Direction::Stopped,
            };
            let pace = Pace::from_name(pace.as_deref());
            let face = face.map(f32::to_radians);
            if let Some(f) = host.world.fighters.get_mut(id) {
                f.brain = Some(Brain::Hold(Orders {
                    direction,
                    pace,
                    face,
                }));
            }
            Ok(())
        },
    )?;

    // AI: make fighter `id` fight fighter `target` (a sparring partner).
    r.def("spar", |_, host, (id, target): (usize, usize)| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.passive = false;
        }
        // The class's melee AI; the stand-in sparring partner if it has none.
        if !host.world.fight(id, target) {
            if let Some(f) = host.world.fighters.get_mut(id) {
                f.brain = Some(crate::ai::Brain::Spar(crate::ai::Spar::new(
                    target,
                    id as u32 + 1,
                )));
            }
        }
        Ok(())
    })?;

    // Whether fighter `id` has finished its `patrol()` route (true too when it has
    // none).
    r.def("route_done", |_, host, id: usize| {
        Ok(host.world.fighters.get(id).is_some_and(|f| match &f.brain {
            Some(crate::ai::Brain::Route(r)) => r.finished(),
            _ => true,
        }))
    })?;

    // Agent `id`'s dodge → dx, dz (the way it dodges; nil: not dodging), weight
    // (how strongly), projectiles (how many of the dangers are projectiles).
    r.def("dodge_of", |_, host, id: usize| {
        let (d, projectiles) = match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
            Some(crate::ai::Brain::Agent(a)) => (a.dodging(), a.projectile_dodges()),
            _ => (None, 0),
        };
        Ok((
            d.map(|(v, _)| v.x),
            d.map(|(v, _)| v.z),
            d.map(|(_, w)| w),
            projectiles,
        ))
    })?;

    // Sets agent `id`'s chance (percent) of going for a gun when it has none.
    r.def("gun_chance", |_, host, (id, percent): (usize, u32)| {
        if let Some(crate::ai::Brain::Agent(a)) = host
            .world
            .fighters
            .get_mut(id)
            .and_then(|f| f.brain.as_mut())
        {
            a.class.gun_pickup_percent = percent;
        }
        Ok(())
    })?;

    // running_pickup_chance(id, percent): its chance of picking a gun up on
    // the run (`gun_runningpickup_percentage`).
    r.def(
        "running_pickup_chance",
        |_, host, (id, percent): (usize, u32)| {
            if let Some(crate::ai::Brain::Agent(a)) = host
                .world
                .fighters
                .get_mut(id)
                .and_then(|f| f.brain.as_mut())
            {
                a.class.gun_running_pickup_percent = percent;
            }
            Ok(())
        },
    )?;

    // Gives fighter `id` the default goals (idle, its class's combat profile).
    r.def("goals", |_, host, id: usize| {
        host.world.give_goals(id, None, None);
        Ok(())
    })?;

    // look_at(id, x, y, z): an agent looks at a point (as `ai2_lookatme`);
    // look_at(id): it stops looking.
    r.def(
        "look_at",
        |_, host, (id, x, y, z): (usize, Option<f32>, Option<f32>, Option<f32>)| {
            if let Some(crate::ai::Brain::Agent(a)) = host
                .world
                .fighters
                .get_mut(id)
                .and_then(|f| f.brain.as_mut())
            {
                match (x, y, z) {
                    (Some(x), Some(y), Some(z)) => a.look_at(glam::Vec3::new(x, y, z)),
                    _ => a.stop_looking(),
                }
            }
            Ok(())
        },
    )?;

    // aim_off(id) → its aim off the body, left-right and up-down (degrees).
    r.def("aim_off", |_, host, id: usize| {
        let f = host.world.fighters.get(id);
        Ok((
            f.map(|f| f.aim.lr.to_degrees()),
            f.map(|f| f.aim.ud.to_degrees()),
        ))
    })?;

    // Where an agent's eyes look (degrees; nil: along its body).
    r.def("eye_of", |_, host, id: usize| {
        Ok(
            match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::Agent(a)) => a.eye.map(|e| e.to_degrees().rem_euclid(360.0)),
                _ => None,
            },
        )
    })?;

    // What fighter `id`'s AI is doing, as one line ("job idle", "pursuit …", …);
    // "other" for a test brain, "none" without one.
    r.def("goal_of", |_, host, id: usize| {
        Ok(
            match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
                Some(crate::ai::Brain::Agent(a)) => a.describe(),
                Some(_) => "other".to_string(),
                None => "none".to_string(),
            },
        )
    })?;

    // neutral_of(id) → fighter id's neutral interaction:
    // {behavior (id), with (whom it talks with), line (0-based, nil before
    // the conversation), accepted, begun, items_given, completed, resume_at}
    // — nil without a behaviour or an interaction.
    r.def("neutral_of", |lua, host, id: usize| {
        let w = &host.world;
        let Some(f) = w.fighters.get(id) else {
            return Ok(None);
        };
        if f.talk.settings.behavior.is_none() && f.talk.with.is_none() {
            return Ok(None);
        }
        let t = lua.create_table()?;
        t.set("behavior", f.talk.settings.behavior)?;
        t.set("with", f.talk.with)?;
        t.set("begun", f.talk.settings.begun)?;
        t.set("items_given", f.talk.settings.items_given)?;
        t.set("completed", f.talk.settings.completed)?;
        t.set("resume_at", f.talk.settings.resume_at)?;
        if let Some(r) = w.neutral_runs.get(&id) {
            t.set("line", r.line)?;
            t.set("accepted", r.accepted)?;
        }
        Ok(Some(t))
    })?;

    // Looks along the floor from fighter `id` toward yaw `deg` up to `dist`:
    // the free distance before a wall, and the distance to a ledge (or nil).
    r.def("probe", |_, host, (id, deg, dist): (usize, f32, f32)| {
        let Some(f) = host.world.fighters.get(id) else {
            return Ok((0.0, None));
        };
        let a = deg.to_radians();
        let env = crate::chrphys::Env::new(&host.world.quads, &[]);
        let radius = if f.player {
            crate::chrphys::PLAYER_LEAF
        } else {
            crate::chrphys::AI_LEAF
        };
        let p = crate::chrphys::probe(
            &env,
            f.feet(),
            radius,
            Vec3::new(a.sin(), 0.0, a.cos()),
            dist,
            10.0,
        );
        Ok((p.free, p.ledge))
    })?;

    // Fighter `id`'s AI local probe along the floor toward yaw `deg` up to
    // `dist` (the level's grids, obstructions counted, as its local movement
    // sees them) → how far it is free, and the square it stopped at (nil: it
    // got all the way).
    r.def(
        "nav_probe",
        |_, host, (id, deg, dist): (usize, f32, f32)| {
            let a = deg.to_radians();
            let p = host
                .world
                .local_probe(id, glam::Vec2::new(a.sin(), a.cos()), dist);
            Ok(p.map_or((0.0, None), |p| {
                (p.free, p.stopped.map(|s| format!("{s:?}").to_lowercase()))
            }))
        },
    )?;

    // Navigation: fighter `id` walks (pace "run" / "walk") to a point
    // through the level, pathfinding; nav_state(id) reads its progress. A new
    // point while walking may keep the path (within 90 ticks, 20 of the last).
    r.def(
        "go_to",
        |_, host, (id, x, y, z, pace): (usize, f32, f32, f32, Option<String>)| {
            let pace = match pace.as_deref() {
                Some("walk") => crate::ai::Pace::Walk,
                _ => crate::ai::Pace::Run,
            };
            if let Some(f) = host.world.fighters.get_mut(id) {
                f.passive = false;
                let at = Vec3::new(x, y, z);
                // (Already walking somewhere: a new order, which may keep
                // the path, as `AI2rPath_GoToPoint`.)
                match f.brain.as_mut() {
                    Some(crate::ai::Brain::GoTo(n)) => n.new_goal(at, 3.0, pace),
                    _ => {
                        let n = crate::ai::navigate::Navigator::new(at, 3.0, pace);
                        f.brain = Some(crate::ai::Brain::GoTo(Box::new(n)));
                    }
                }
            }
            Ok(())
        },
    )?;

    // The waypoints fighter `id` is following (`go_to()` or an agent's) → {{x, z,
    // radius, next}, …} (next: the one it heads for); empty without a path.
    r.def("nav_path", |lua, host, id: usize| {
        let t = lua.create_table()?;
        let nav = match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
            Some(crate::ai::Brain::GoTo(n)) => Some(&**n),
            Some(crate::ai::Brain::Agent(a)) => a.nav.as_ref(),
            _ => None,
        };
        if let Some(n) = nav {
            let (points, next) = n.waypoints();
            for (i, (p, r)) in points.iter().enumerate() {
                let w = lua.create_table()?;
                w.set(1, p.x)?;
                w.set(2, p.z)?;
                w.set(3, *r)?;
                w.set(4, i == next)?;
                t.set(i + 1, w)?;
            }
        }
        Ok(t)
    })?;

    // Fighter `id`'s path through the rooms → the room it is in along it (0:
    // the first), how many rooms it passes, and when it was planned; nil
    // without one.
    r.def("nav_legs", |_, host, id: usize| {
        let n = host
            .world
            .fighters
            .get(id)
            .and_then(|f| f.brain.as_ref())
            .and_then(|b| b.navigator());
        Ok(match n.and_then(|n| n.global.as_ref().map(|g| (n, g))) {
            Some((n, g)) => (Some(n.leg), Some(g.legs()), Some(n.planned.0)),
            None => (None, None, None),
        })
    })?;

    // Fighter `id`'s navigation → "planning", "following", "arrived", "failed:
    // …", or "none".
    r.def("nav_state", |_, host, id: usize| {
        use crate::ai::navigate::State;
        let nav = match host.world.fighters.get(id).and_then(|f| f.brain.as_ref()) {
            Some(crate::ai::Brain::GoTo(n)) => Some(&**n),
            Some(crate::ai::Brain::Agent(a)) => a.nav.as_ref(),
            _ => None,
        };
        Ok(match nav {
            Some(n) => match &n.state {
                State::Planning => "planning".to_string(),
                State::Following => "following".to_string(),
                State::Arrived => "arrived".to_string(),
                State::Failed(why) => format!("failed: {why}"),
            },
            _ => "none".to_string(),
        })
    })?;

    // What the pathfinding grid has stamped at (`x`, `y`, `z`) → "character",
    // "object" or "door", its index (fighter, object, door), and whether it
    // blocks fighter `asker` (default 1) planning a path; nil if nothing.
    r.def(
        "nav_obstruction",
        |_, host, (x, y, z, asker): (f32, f32, f32, Option<usize>)| {
            use crate::nav::dynamic::Obstruction as O;
            let at = host
                .world
                .obstruction_at(Vec3::new(x, y, z), asker.unwrap_or(1));
            Ok(match at {
                Some((O::Character(i), b)) => (Some("character"), Some(i), Some(b)),
                Some((O::Object(i), b)) => (Some("object"), Some(i), Some(b)),
                Some((O::Door(i), b)) => (Some("door"), Some(i), Some(b)),
                None => (None, None, None),
            })
        },
    )?;

    // The class fighter `id`'s melee techniques are planned with (its
    // profile's `char_classname`), or nil: none (its own class, or no
    // profile).
    r.def("melee_plan_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .and_then(|f| f.melee_profile.as_ref())
            .filter(|p| p.plan.get().is_some())
            .map(|p| p.class.clone()))
    })?;

    // Perception: makes fighter `id` an AI that watches and listens (not a
    // passive dummy).
    r.def("watch", |_, host, id: usize| {
        if let Some(f) = host.world.fighters.get_mut(id) {
            f.passive = false;
        }
        Ok(())
    })?;

    // Fighter `id`'s alert level name ("lull", "low", "medium", "high",
    // "combat"), or nil without perception.
    r.def("alert_of", |_, host, id: usize| {
        Ok(host
            .world
            .fighters
            .get(id)
            .and_then(|f| f.perception.as_ref())
            .map(|p| p.alert.name()))
    })?;

    // What fighter `id` knows of fighter `other` → "dead", "forgotten", "weak",
    // "strong" or "definite", or nil: nothing.
    r.def("contact_of", |_, host, (id, other): (usize, usize)| {
        Ok(host
            .world
            .fighters
            .get(id)
            .and_then(|f| f.perception.as_ref())
            .and_then(|p| p.about(other))
            .map(|c| c.strength.name()))
    })?;

    // Sets fighter `id`'s team: "konoko", "tctf", "syndicate", "securityGuard",
    // "rogueKonoko", "switzerland", "syndicateAccessory" (else neutral).
    r.def("team", |_, host, (id, name): (usize, String)| {
        if let Some(p) = host
            .world
            .fighters
            .get_mut(id)
            .and_then(|f| f.perception.as_mut())
        {
            p.team = crate::ai::perception::Team::from_name(&name);
        }
        Ok(())
    })?;

    // Makes a noise at a point for the AI to hear: kind "interesting", "danger",
    // "melee" or "gunshot" (else ignored), volume its reach (world units, times
    // each listener's hearing), maker the fighter who made it.
    r.def(
        "noise",
        |_, host, (x, y, z, kind, volume, maker): (f32, f32, f32, String, f32, Option<usize>)| {
            use crate::ai::perception::Stimulus;
            let stimulus = match kind.as_str() {
                "interesting" => Stimulus::SoundInteresting,
                "danger" => Stimulus::SoundDanger,
                "melee" => Stimulus::SoundMelee,
                "gunshot" => Stimulus::SoundGunshot,
                _ => Stimulus::SoundIgnore,
            };
            host.world
                .sound(stimulus, Vec3::new(x, y, z), volume, maker, None);
            Ok(())
        },
    )?;

    // Test hook: make melee AI `id` use technique `name` next (once usable).
    r.def("technique", |_, host, (id, name): (usize, String)| {
        if let Some(crate::ai::Brain::Melee(m)) = host
            .world
            .fighters
            .get_mut(id)
            .and_then(|f| f.brain.as_mut())
        {
            m.force = Some(name);
        }
        Ok(())
    })?;
    Ok(())
}
