//! Engine commands: AI orders.

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
        // --- AI orders -------------------------------------------------------
        // `ai2_dopath who path`: the AI walks the level's patrol path of that name
        // (not while passive).
        "ai2_dopath" => {
            let (Some(id), Some(path)) = (ai(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if host.world().fighters[id].passive {
                eprintln!(
                    "script: ai2_dopath: {} is passive",
                    host.world().name_of(id)
                );
                return nil();
            }
            host.world().order_path(id, &path);
            nil()
        }
        // A character's neutral behaviour, by name ("none": none;
        // `AI2iScript_SetNeutralBehavior`).
        "ai2_neutralbehavior" => {
            if let Some(id) = ai(host, a(0)) {
                let name = a(1).map(|v| match v {
                    Value::String(s) => s.to_string_lossy().to_string(),
                    v => format!("{v:?}"),
                });
                let world = host.world();
                match name.as_deref() {
                    None | Some("none") => {
                        world.neutral_behavior(id, None, false);
                    }
                    Some(n) => match world.neutrals.values().find(|b| b.name == n).map(|b| b.id) {
                        Some(b) => {
                            world.neutral_behavior(id, Some(b), false);
                        }
                        None => eprintln!("script: ai2_neutralbehavior: no behaviour '{n}'"),
                    },
                }
            }
            nil()
        }
        // `ai2_setjobstate who`: what the AI is doing now becomes its job.
        "ai2_setjobstate" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().make_job(id);
            }
            nil()
        }
        // `ai2_passive` / `chr_freeze` / `chr_neutral who on`: the AI made passive
        // (doing nothing, noticing nothing) or not. (`chr_freeze` and `chr_neutral`
        // are the original's own aliases of it: one handler, `AI2iScript_Passive`.)
        "ai2_passive" | "chr_freeze" | "chr_neutral" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().set_passive(id, truthy(a(1)));
            }
            nil()
        }
        // `ai2_allpassive on`: every AI passive or not: those present now and, while
        // on, those spawned later.
        "ai2_allpassive" => {
            host.world().set_all_passive(truthy(a(0)));
            nil()
        }
        // `ai2_attack who target`: the AI attacks `target` (a forced target);
        // `ai2_makeaware`: it gets only a definite sighting of `target`.
        "ai2_attack" | "ai2_makeaware" => {
            if let (Some(id), Some(target)) = (ai(host, a(0)), fighter(host, a(1))) {
                host.world().order_attack(id, target, name == "ai2_attack");
            }
            nil()
        }
        // `ai2_makeignoreplayer who on`: whether the AI pays the player no attention.
        "ai2_makeignoreplayer" => {
            if let Some(p) =
                ai(host, a(0)).and_then(|id| host.world().fighters[id].perception.as_mut())
            {
                p.ignore_player = truthy(a(1));
            }
            nil()
        }
        "ai2_setmovementmode" => {
            // (The player too, while the AI moves her: `ai2_takecontrol`.)
            if let (Some(id), Some(mode)) = (fighter(host, a(0)), a(1).and_then(text)) {
                let mode = crate::ai::goals::MoveMode::from_script(&mode);
                match host.world().fighters[id].brain.as_mut() {
                    Some(crate::ai::Brain::Agent(ag)) => ag.mode = mode,
                    Some(crate::ai::Brain::Steer(s)) => s.mode = mode,
                    _ if id == 0 => state.warn(
                        "ai2_setmovementmode: cannot move the player without ai2_takecontrol 1"
                            .into(),
                    ),
                    _ => {}
                }
            }
            nil()
        }
        // --- scripted movement -----------

        // (Not this area's: the next's.)
        _ => super::flow::dispatch(state, host, lua, name, args),
    }
}
