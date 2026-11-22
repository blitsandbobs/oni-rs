//! The engine commands level scripts call (`docs/lua.md`, *Command
//! surface*; semantics in `docs/lua.md`, *Runtime*), and the world
//! events that start scripts — character events and trigger volumes.
//! Commands the engine can't carry out yet are no-ops, counted in
//! [`State::stubbed`].

use glam::Vec3;
use mlua::{Lua, Value, Variadic};

use super::{Host, State};

// --- arguments ------------------------------------------------------------------

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.to_string_lossy()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// How near an AI following the player stops (`AI2cFollowCharacter_DestinationDist`).
const FOLLOW_DISTANCE: f32 = 12.0;

/// Whether the player killed Griffin: as a script said this session, else
/// as kept with the saved game (in play).
fn did_kill_griffin(host: &mut dyn Host) -> bool {
    let world = host.world();
    world
        .progress
        .killed_griffin
        .unwrap_or_else(|| world.progress.persist_continues && crate::game::killed_griffin())
}

fn int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Integer(i) => Some(*i),
        Value::Number(n) => Some(*n as i64),
        Value::Boolean(b) => Some(*b as i64),
        Value::String(s) => s.to_str().ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn float(v: Option<&Value>) -> Option<f32> {
    match v? {
        Value::Integer(i) => Some(*i as f32),
        Value::Number(n) => Some(*n as f32),
        Value::String(s) => s.to_str().ok()?.trim().parse().ok(),
        _ => None,
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Nil) => false,
        Some(Value::Boolean(b)) => *b,
        Some(v) => int(Some(v)).is_some_and(|i| i != 0),
    }
}

/// A character argument: a script id (a number — only the player has one,
/// 0) or a name (exactly, among the characters present).
fn fighter(host: &mut dyn Host, v: Option<&Value>) -> Option<usize> {
    let v = v?;
    if let Some(i) = int(Some(v)) {
        // (By script id: the player 0, a `chr_create`d character its setup's.)
        let world = host.world();
        if i == 0 {
            return (!world.fighters.is_empty()).then_some(0);
        }
        return world
            .fighters
            .iter()
            .position(|f| !f.gone && f.script_id.is_some_and(|s| i64::from(s) == i));
    }
    // (By name: every living character, the player by its placement's name.)
    let name = text(v)?;
    let world = host.world();
    let player = world.player_name.clone();
    world.fighters.iter().enumerate().position(|(i, f)| {
        !f.gone
            && match &f.brain {
                Some(crate::ai::Brain::Agent(a)) => a.name == name,
                _ => i == 0 && !player.is_empty() && player == name,
            }
    })
}

/// An AI character argument (the `ai2_*` commands don't take the player).
fn ai(host: &mut dyn Host, v: Option<&Value>) -> Option<usize> {
    fighter(host, v).filter(|&i| i != 0)
}

fn name_of(host: &mut dyn Host, i: usize) -> String {
    match host.world().fighters.get(i).and_then(|f| f.brain.as_ref()) {
        Some(crate::ai::Brain::Agent(a)) => a.name.clone(),
        _ if i == 0 => host.world().player_name.clone(),
        _ => format!("fighter{i}"),
    }
}

// --- trigger volumes -------------------------------------------------------------

pub mod tv {
    pub const ENTRY_ONCE: u32 = 0x01;
    pub const ENTRY_OFF: u32 = 0x08;
    pub const DISABLED: u32 = 0x40;
    pub const PLAYER_ONLY: u32 = 0x80;
}

/// A trigger volume (`levels/NN/triggers.json`): a box from `corner` along
/// `axes` by `size`.
#[derive(Clone, Debug, Default)]
pub struct Volume {
    pub id: i64,
    pub name: String,
    /// Entry / inside / exit scripts (now, and as authored).
    pub scripts: [String; 3],
    authored_scripts: [String; 3],
    pub teams: u32,
    pub corner: Vec3,
    pub axes: [Vec3; 3],
    pub size: Vec3,
    pub parent: Option<i64>,
    pub flags: u32,
    authored_flags: u32,
}

impl Volume {
    /// Whether the vertical segment `from`..`to` crosses the box.
    pub fn crossed(&self, from: Vec3, to: Vec3) -> bool {
        crate::geom::segment_box(
            from,
            to - from,
            self.corner,
            self.axes,
            Vec3::ZERO,
            self.size,
        )
        .is_some()
    }
}

/// The level's trigger volumes, and which each character is inside.
#[derive(Clone, Debug, Default)]
pub struct Triggers {
    pub volumes: Vec<Volume>,
    /// Per fighter: the ids of the volumes it is inside (at most 4).
    pub inside: std::collections::HashMap<usize, Vec<i64>>,
}

const TRACKED: usize = 4;

impl Triggers {
    pub fn load(path: &std::path::Path) -> Triggers {
        let doc: oni_schema::files::TriggersFile = crate::json::read_file(path);
        let bits = |names: &[String], table: &[&str]| -> u32 {
            names
                .iter()
                .filter_map(|n| table.iter().position(|x| x == n))
                .fold(0, |m, b| m | 1 << b)
        };
        let volumes = doc
            .triggers
            .iter()
            .map(|t| {
                let s = |v: &Option<String>| v.clone().unwrap_or_default();
                let scripts = [s(&t.entry), s(&t.inside), s(&t.exit)];
                let flags = bits(&t.flags, &oni_schema::TRIGGER_FLAGS);
                Volume {
                    id: i64::from(t.id),
                    name: t.name.clone(),
                    authored_scripts: scripts.clone(),
                    scripts,
                    teams: bits(&t.teams, &oni_schema::TEAMS),
                    corner: Vec3::from(t.position),
                    axes: t.axes.map(Vec3::from),
                    size: Vec3::from(t.size),
                    parent: t.parent.map(i64::from),
                    flags,
                    authored_flags: flags,
                }
            })
            .collect();
        Triggers {
            volumes,
            inside: Default::default(),
        }
    }

    fn by_name(&self, v: Option<&Value>) -> Vec<usize> {
        let Some(n) = v.and_then(text) else {
            return Vec::new();
        };
        self.volumes
            .iter()
            .enumerate()
            .filter(|(_, t)| t.name == n)
            .map(|(i, _)| i)
            .collect()
    }

    fn by_id(&self, v: Option<&Value>) -> Vec<usize> {
        let Some(id) = int(v) else { return Vec::new() };
        self.volumes
            .iter()
            .enumerate()
            .filter(|(_, t)| t.id == id)
            .map(|(i, _)| i)
            .collect()
    }

    /// Fires volume `i`'s script of `kind` (0 entry, 1 inside, 2 exit) for
    /// `who` — and its parent's — honouring the disabled / once flags.
    fn fire(&mut self, i: usize, kind: usize, who: &str, out: &mut Vec<(String, Vec<String>)>) {
        let parent = self.volumes[i]
            .parent
            .and_then(|p| self.volumes.iter().position(|v| v.id == p));
        for k in std::iter::once(i).chain(parent) {
            let v = &mut self.volumes[k];
            let (once, off) = (tv::ENTRY_ONCE << kind, tv::ENTRY_OFF << kind);
            if v.scripts[kind].is_empty() || v.flags & off != 0 {
                continue;
            }
            out.push((v.scripts[kind].clone(), vec![who.to_string()]));
            if v.flags & once != 0 {
                v.flags |= off;
            }
        }
    }
}

// --- events -------------------------------------------------------------------

/// The script functions this tick's events call, each with the character's
/// name: character events (once per life) and trigger volumes.
pub fn events(state: &mut State, host: &mut dyn Host) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();

    // A countdown run out: its script, once.
    if let Some(script) = host.world().timer_expired.take() {
        out.push((script, Vec::new()));
    }

    // Character events.
    let n = host.world().fighters.len();
    for id in 0..n {
        let world = host.world();
        let f = &world.fighters[id];
        // (An AI's, by its name; the player's — its die script loses the
        // level — by its placement's.)
        let (combat, name) = match &f.brain {
            Some(crate::ai::Brain::Agent(a)) => (
                matches!(a.goal, crate::ai::goals::Goal::Combat(_)),
                a.name.clone(),
            ),
            _ if id == 0 && f.player => (false, world.player_name.clone()),
            _ => continue,
        };
        let Some(scripts) = state.scripts.get(&id) else {
            continue;
        };
        let (dying, hurt) = (
            f.motion.dying || f.hp <= 0,
            f.hp < f.max_hp && !f.invincible,
        );
        let defeated = dying || f.defeated;
        let alarmed = std::mem::take(&mut host.world().fighters[id].alarmed);
        // (Its gun empty with nothing to reload it.)
        let dry = host.world().has_empty_weapon(id);
        let fired = state.fired.entry(id).or_default();
        for (event, now) in [
            ("hurt", hurt),
            ("combat", combat),
            ("alarm", alarmed),
            ("outOfAmmo", dry),
            ("defeated", defeated),
            ("die", dying),
        ] {
            if now && fired.insert(event) {
                if let Some(s) = scripts.get(event) {
                    out.push((s.clone(), vec![name.clone()]));
                }
            }
        }
    }

    // Trigger volumes: each active character's vertical extent against
    // each enabled volume (`docs`: `docs/lua.md`, *Trigger volumes*).
    let bodies: Vec<(usize, Vec3, Vec3, u32)> = host
        .world()
        .fighters
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.gone)
        .map(|(i, f)| {
            let feet = f.feet();
            (
                i,
                feet,
                feet + Vec3::Y * f.motion.height().max(12.0),
                1u32 << (f.team() as u32),
            )
        })
        .collect();
    let names: Vec<String> = (0..n).map(|i| name_of(host, i)).collect();
    let tr = &mut state.triggers;
    for vi in 0..tr.volumes.len() {
        let v = &tr.volumes[vi];
        if v.flags & tv::DISABLED != 0 {
            continue;
        }
        let teams = v
            .parent
            .and_then(|p| tr.volumes.iter().find(|x| x.id == p))
            .map_or(v.teams, |p| p.teams);
        let (id, player_only) = (v.id, v.flags & tv::PLAYER_ONLY != 0);
        for &(i, feet, top, team) in &bodies {
            if player_only && i != 0 {
                continue;
            }
            let hit = teams & team != 0 && tr.volumes[vi].crossed(feet, top);
            let list = tr.inside.entry(i).or_default();
            let was = list.contains(&id);
            if hit {
                if !was && list.len() < TRACKED {
                    list.push(id);
                    tr.fire(vi, 0, &names[i], &mut out);
                }
                tr.fire(vi, 1, &names[i], &mut out);
            } else if was {
                list.retain(|&x| x != id);
                tr.fire(vi, 2, &names[i], &mut out);
            }
        }
    }
    out
}

// --- commands -----------------------------------------------------------------

/// What a command gives back to the calling script: its value, whether the
/// script must wait, and a script function to run right now (as part of the
/// command, with a character's name).
/// (`wait`: `true` — call me again next tick; a number — sleep that long.)
pub type Reply = (Value, Value, Option<String>, Option<String>);

/// Carries out one script command (or a prelude request, `@…`).
pub fn dispatch(
    state: &mut State,
    host: &mut dyn Host,
    lua: &Lua,
    name: &str,
    args: Variadic<Value>,
) -> mlua::Result<Reply> {
    let a = |k: usize| args.get(k);
    let done = |v: Value| Ok((v, Value::Boolean(false), None, None));
    let nil = || done(Value::Nil);
    let _ = lua;
    // (`ONI_TRACE_COMMANDS=1`: each engine command a script calls, with the
    // tick — a stalled one shows again every tick it waits.)
    if !name.starts_with('@') && std::env::var_os("ONI_TRACE_COMMANDS").is_some() {
        let shown: Vec<String> = args
            .iter()
            .map(|v| match v {
                Value::String(s) => format!("{:?}", s.to_string_lossy()),
                other => format!("{other:?}"),
            })
            .collect();
        eprintln!("cmd {} {name}({})", state.now, shown.join(", "));
    }
    match name {
        // --- the prelude's requests --------------------------------------
        "@word" => {
            // An engine variable's value, a value command's result, or nil
            // (the word stands for itself).
            let w = a(0).and_then(text).unwrap_or_default();
            if let Some(v) = state.vars.get(&w) {
                return done(v.clone());
            }
            match w.as_str() {
                "difficulty" => done(Value::Integer(host.world().difficulty.level)),
                "did_kill_griffen" => done(Value::Integer(did_kill_griffin(host) as i64)),
                "chr_has_lsi" | "chr_has_empty_weapon" => done(Value::Integer(0)),
                _ => nil(),
            }
        }
        "@chr_at" => {
            // (For our fixes, `fixes/`: where a character stands, feet —
            // the player by default; nil if it isn't present.)
            let who = match a(0) {
                None | Some(Value::Nil) => Some(0),
                v => fighter(host, v),
            };
            match who.and_then(|i| host.world().fighters.get(i).map(|f| f.feet())) {
                Some(at) => done(Value::Table(lua.create_sequence_from([at.x, at.y, at.z])?)),
                None => nil(),
            }
        }
        // A `sleep` argument (`SLEEP`) as ticks: a number as is, a word by its value
        // (0 if none); never negative.
        "@ticks" => {
            let n = match a(0) {
                Some(Value::String(s)) => state.ticks_of_word(&s.to_string_lossy()),
                v => int(v).unwrap_or(0),
            };
            done(Value::Integer(n.max(0)))
        }
        // Runs a coroutine as a new task (`@adopt`: one the prelude already started
        // and that yielded) after `delay` ticks, labelled for `mission_info()`.waiting.
        // (No prelude code sends `@fork` now.)
        "@fork" | "@adopt" => {
            if let Some(Value::Thread(t)) = a(0) {
                let delay = int(a(1)).unwrap_or(0).max(0) as u64;
                let label = a(2).and_then(text).unwrap_or_else(|| "fork".into());
                state.started.push((t.clone(), label, None, delay));
            }
            nil()
        }
        // A script error (`ERR`, a missing script function): printed and kept in
        // `mission_info()`.errors, once each.
        "@error" => {
            let msg = a(0).and_then(text).unwrap_or_default();
            if !state.errors.contains(&msg) {
                eprintln!("script error: {msg}");
                state.errors.push(msg);
            }
            nil()
        }
        // Reads an engine variable (`G.name`): the fog's `gl_fog_*` from the draw
        // engine, else what a script set (0 if unset).
        "@get" => {
            let k = a(0).and_then(text).unwrap_or_default();
            // (The fog's are the draw engine's.)
            let fog = host.world().fog.fog;
            let v = match k.as_str() {
                "gl_fog_red" => Some(fog.color[0]),
                "gl_fog_green" => Some(fog.color[1]),
                "gl_fog_blue" => Some(fog.color[2]),
                "gl_fog_start" => Some(fog.start),
                "gl_fog_end" => Some(fog.end),
                _ => None,
            };
            match v {
                Some(v) => done(Value::Number(f64::from(v))),
                None => done(state.vars.get(&k).cloned().unwrap_or(Value::Integer(0))),
            }
        }
        // Sets an engine variable (`G.name = v`); `gl_fog_*`, `ai2_boss_battle` and
        // the camera's `cm_distance` / `cm_height` / `cm_canter_*` also change the
        // engine.
        "@set" => {
            let k = a(0).and_then(text).unwrap_or_default();
            // (The fog's and the camera's tunables are engine variables.)
            if let Some(v) = float(a(1)) {
                let fog = &mut host.world().fog.fog;
                match k.as_str() {
                    "gl_fog_red" => fog.color[0] = v,
                    "gl_fog_green" => fog.color[1] = v,
                    "gl_fog_blue" => fog.color[2] = v,
                    "gl_fog_start" => fog.start = v,
                    "gl_fog_end" => fog.end = v,
                    _ => {}
                }
                if k == "ai2_boss_battle" {
                    host.world().boss_battle = v != 0.0;
                }
                let cam = &mut host.world().camera;
                match k.as_str() {
                    "cm_distance" => cam.distance = v,
                    "cm_height" => cam.height = v,
                    "cm_canter_weapon" => cam.canter_weapon = v,
                    "cm_canter_unarmed" => cam.canter_unarmed = v,
                    _ => {}
                }
            }
            state.vars.insert(k, a(1).cloned().unwrap_or(Value::Nil));
            nil()
        }
        // Does nothing (no prelude code sends it now).
        "@still" => nil(),

        // --- output ----------------------------------------------------------
        // Prints its words to the terminal; `dmsg` also shows them in the HUD's
        // message area for 10 s (`dprint` went to the developer console, unseen in
        // the shipped game).
        "dprint" | "dmsg" => {
            let words: Vec<String> = args.iter().filter_map(text).collect();
            eprintln!("script: {}", words.join(" "));
            // (`dmsg` shows in the message area for 10 s; `dprint` goes to
            // the developer console, not shown in the shipped game.)
            if name == "dmsg" {
                host.world().hud.messages.print(&words.join(" "), None, 600);
            }
            nil()
        }
        // `message name [ticks]` (`AIiScript_Message`): shows a message from the
        // messages table (else a subtitle in the console strip), for `ticks` if
        // given; warns if it is in neither.
        "message" => {
            if let Some(m) = a(0).and_then(text) {
                match state.message_text(&m) {
                    Some(t) => eprintln!("MESSAGE ({m}): {t}"),
                    None => eprintln!("MESSAGE {m}"),
                }
                state.messages.retain(|x| *x != m);
                if let Some(t) = int(a(1)).filter(|&t| t > 0) {
                    state.message_until.insert(m.clone(), state.now + t as u64);
                } else {
                    state.message_until.remove(&m);
                }
                if !host
                    .world()
                    .hud
                    .message(&m, int(a(1)).unwrap_or(0).max(0) as u32)
                {
                    state.warn(format!("message: no message {m}"));
                }
                state.messages.push(m);
            }
            nil()
        }
        // `message_remove [name]`: that message (every one, without a name) fades
        // from the HUD.
        "message_remove" => {
            let m = a(0).and_then(text);
            host.world().hud.message_remove(m.as_deref());
            match m {
                Some(m) => state.messages.retain(|x| *x != m),
                None => state.messages.clear(),
            }
            nil()
        }
        "target_set" => {
            // The HUD's target locator (no HUD yet: said in the terminal).
            let (Some(flag), d) = (
                int(a(0)),
                a(1).and_then(|v| match v {
                    Value::Number(n) => Some(*n),
                    Value::Integer(i) => Some(*i as f64),
                    _ => None,
                }),
            ) else {
                return nil();
            };
            let world = host.world();
            let d = d.unwrap_or(0.0) as f32;
            let at = world.level_ai.flags.get(&(flag as i16)).map(|f| f.at);
            if d != 0.0 && at.is_none() {
                state.warn(format!("target_set: no flag {flag}"));
            }
            let from = world.player().map_or(Vec3::ZERO, |p| p.feet());
            if d != 0.0 && world.hud.target_set(at, d, from) {
                world.event_sound_ui("compass");
            } else if d == 0.0 {
                world.hud.target_set(None, 0.0, from);
            }
            nil()
        }
        // `objective_set n [silent]`: the objective becomes text `n`; the HUD prompts
        // the new one with its sound unless `silent`.
        "objective_set" => {
            state.objective = int(a(0));
            eprintln!("script: objective {:?}", state.objective);
            // (The HUD prompts a new one, with its sound, unless `silent`.)
            let silent = a(1)
                .and_then(text)
                .is_some_and(|t| t.eq_ignore_ascii_case("silent"));
            host.world()
                .hud
                .objective_set(int(a(0)).unwrap_or(0), silent);
            nil()
        }
        // Plays the objective-complete sound (no other effect).
        "objective_complete" => {
            host.world().event_sound("objectiveComplete", None);
            nil()
        }
        // `save_game n [autosave]` (`ONrGameState_MakeContinue`): keeps the player as
        // she is now for a restart at save point `n`; an autosave shows its message
        // and sound.
        "save_game" => {
            let autosave = a(1).and_then(text).is_some_and(|t| t == "autosave");
            host.world().save_continue(int(a(0)).unwrap_or(0), autosave);
            nil()
        }
        // Puts the player back as last saved (`ONrGameState_UseContinue`):
        // health, place, facing, inventory and guns.
        "restore_game" => {
            host.world().restore_continue();
            nil()
        }
        // `splash_screen name`: asks for that splash screen (kept for the game flow
        // to show, `game()`.splash).
        "splash_screen" => {
            if let Some(name) = a(0).and_then(text) {
                host.world().progress.pending_splash = Some(name);
            }
            nil()
        }
        "begin_cutscene" => {
            // (`World::begin_cutscene`.)
            let names: Vec<String> = args.iter().filter_map(text).collect();
            let (keep, unknown) =
                crate::world::cutscene::CutsceneKeeps::parse(names.iter().map(String::as_str));
            for k in unknown {
                state.warn(format!("begin_cutscene: unknown flag {k}"));
            }
            host.world().begin_cutscene(keep);
            nil()
        }
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

        // --- environmental particles ----------------------
        // `particle tag command [event]`: sends the environmental particles tagged
        // `tag` a command ("create", "kill", "reset", "do event", or an event name);
        // warns on an unknown command.
        "particle" => {
            let (Some(tag), Some(command)) = (a(0).and_then(text), a(1).and_then(text)) else {
                return nil();
            };
            let event = a(2).and_then(text);
            if !host
                .world()
                .env_particle_command(&tag, &command, event.as_deref())
            {
                state.warn(format!("particle {tag}: unknown command {command}"));
            }
            nil()
        }
        // --- sound ---------------------------------
        // One dialogue voice: a line still on drops a plain play, holds a
        // `_block` play until it is over, is cut off by `_interrupt`. Its
        // subtitle shows for its length and a second.
        "sound_dialog_play" | "sound_dialog_play_block" | "sound_dialog_play_interrupt" => {
            let Some(line) = a(0).and_then(text) else {
                return nil();
            };
            match host
                .world()
                .audio
                .dialog_start(&line, name.ends_with("_interrupt"))
            {
                Err(_) if name.ends_with("_block") => {
                    Ok((Value::Nil, Value::Boolean(true), None, None))
                }
                Err(_) => nil(),
                // (No such line: nothing, quietly, no subtitle — as shipped.)
                Ok(None) => nil(),
                Ok(Some(duration)) => {
                    if let Some(t) = state.message_text(&line) {
                        eprintln!("SAY ({line}): {t}");
                    }
                    // Its subtitle, with the option on, for its length and a
                    // second more (`OSrInGameDialog_Play`,
                    // `OScSubtitleStayTime`).
                    let world = host.world();
                    let letterboxed = world.cutscene.letterbox;
                    world.hud.subtitle(&line, duration + 60, letterboxed);
                    nil()
                }
            }
        }
        // (Called by a few scripts, but never an engine command: nothing.)
        "sound_dialog_play_pause" => nil(),
        // Music and script ambients: one pool of 16 unplaced slots.
        "sound_music_start" | "sound_ambient_start" => {
            let Some(sound) = a(0).and_then(text) else {
                return nil();
            };
            let volume = float(a(1)).unwrap_or(1.0);
            if !host.world().audio.script_start(&sound, volume) {
                state.warn(format!("{name}: can't play {sound}"));
            }
            nil()
        }
        // `sound_music_stop` / `sound_ambient_stop name`: the first slot playing that
        // sound plays out its end.
        "sound_music_stop" | "sound_ambient_stop" => {
            if let Some(sound) = a(0).and_then(text) {
                host.world().audio.script_stop(&sound);
            }
            nil()
        }
        // `sound_music_volume` / `sound_ambient_volume name volume [seconds]`: the
        // first slot of that name fades to `volume` (0–1) over `seconds` (even one
        // that has ended, as shipped).
        "sound_music_volume" | "sound_ambient_volume" => {
            let (Some(sound), Some(volume)) = (a(0).and_then(text), float(a(1))) else {
                return nil();
            };
            let seconds = float(a(2)).unwrap_or(0.0);
            host.world().audio.script_volume(&sound, volume, seconds);
            nil()
        }
        // `sound_impulse_play name [volume]`: plays an impulse sound, unplaced, at
        // `volume` (0–1).
        "sound_impulse_play" => {
            let Some(sound) = a(0).and_then(text) else {
                return nil();
            };
            let volume = float(a(1)).map(|v| v.clamp(0.0, 1.0));
            let world = host.world();
            world
                .audio
                .impulse_play(&sound, None, volume, Some(&world.quads));
            nil()
        }
        // A character says a line: `chr_talk who sound pre post [type]`
        // (no type: override). (The original never stores a given type —
        // uninitialised; here it is used.)
        "chr_talk" => {
            let (Some(who), Some(sound)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let kind = match a(4).and_then(text) {
                None => crate::world::speech::OVERRIDE,
                Some(t) => match [
                    "none", "idle", "chatter", "say", "call", "pain", "yell", "override",
                ]
                .iter()
                .position(|n| *n == t)
                {
                    Some(k) => k as u8,
                    None => return nil(),
                },
            };
            let line = crate::world::Line {
                sound,
                kind,
                pre_pause: int(a(2)).unwrap_or(0).max(0) as u32,
                post_pause: int(a(3)).unwrap_or(0).max(0) as u32,
            };
            host.world().say(who, line, false);
            nil()
        }
        // `chr_vocalize who kind`: its AI's line of that kind, forced.
        "chr_vocalize" => {
            let (Some(who), Some(kind)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if let Some(k) = oni_schema::VOCALIZATIONS
                .iter()
                .position(|v| v.eq_ignore_ascii_case(&kind))
            {
                host.world().vocalize(who, k, true);
            }
            nil()
        }

        // --- characters ------------------------------------------------------
        // `ai2_spawn name [force]`: spawns the level's placed character `name` (the
        // first placement not yet spawned, unless `force` or it may spawn many times)
        // with its goals; its spawn script runs at once.
        "ai2_spawn" => {
            let Some(who) = a(0).and_then(text) else {
                return nil();
            };
            let force = a(1).and_then(text).is_some_and(|f| f == "force");
            match spawn(state, host, &who, force) {
                Some((script, arg)) => {
                    Ok((Value::Nil, Value::Boolean(false), Some(script), Some(arg)))
                }
                None => nil(),
            }
        }
        "chr_create" => {
            // (The second argument, start / create, the original ignores.)
            let Some(sid) = int(a(0)) else { return nil() };
            match create(state, host, sid) {
                Some((script, arg)) => {
                    Ok((Value::Nil, Value::Boolean(false), Some(script), Some(arg)))
                }
                None => nil(),
            }
        }
        // A data console's text, modal: the game stands still until it is
        // closed (`ONiTextConsole_Display`; `hud/console.rs`).
        "text_console" => {
            let Some(name) = a(0).and_then(text) else {
                return nil();
            };
            if !host.world().hud.open_text_console(&name) {
                state.warn(format!("text_console: no console {name}"));
            }
            nil()
        }
        // `chr_delete who`: removes the character from the level.
        "chr_delete" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().remove(id);
            }
            nil()
        }
        // `chr_teleport who flag`: puts the character at the flag's position (keeping
        // its facing); warns if there is no such flag.
        "chr_teleport" => {
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let world = host.world();
            match world.level_ai.flags.get(&(flag as i16)).copied() {
                Some(f) => world.teleport(id, f.at, None),
                None => state.warn(format!("chr_teleport: no flag {flag}")),
            }
            nil()
        }
        // `chr_facetoflag who flag`: turns the character to the flag's facing where
        // it stands.
        "chr_facetoflag" => {
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let world = host.world();
            if let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() {
                let at = world.fighters[id].feet();
                world.teleport(id, at, Some(f.facing));
            }
            nil()
        }
        // `chr_set_health who hp`: sets its hit points; 0 or less kills it outright,
        // even if invincible or unkillable.
        "chr_set_health" => {
            let (Some(id), Some(hp)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            if let Some(f) = host.world().fighters.get_mut(id) {
                if hp <= 0 && f.alive() {
                    // (An act of god: kills even the invincible and the
                    // unkillable.)
                    let (was, unkillable, all) = (f.invincible, f.unkillable, f.hp);
                    (f.invincible, f.unkillable) = (false, false);
                    f.take_damage(all, Vec3::ZERO);
                    (f.invincible, f.unkillable) = (was, unkillable);
                } else {
                    f.hp = hp as i32;
                }
            }
            nil()
        }
        // `chr_full_health who`: restores its hit points to its full health.
        "chr_full_health" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.hp = f.max_hp;
            }
            nil()
        }
        // `chr_invincible who on`: whether it takes no damage.
        "chr_invincible" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.invincible = truthy(a(1));
            }
            nil()
        }
        // --- placed objects (`world/objects.rs`) -----------------------------
        // `obj_create` / `obj_kill` / `obj_show` / `obj_hide a [b]`: placed objects
        // with script ids `a`–`b` made anew, removed, or shown / hidden (drawn and
        // solid, or neither; `OBrShow`).
        "obj_create" | "obj_kill" | "obj_show" | "obj_hide" => {
            let Some(lo) = int(a(0)) else { return nil() };
            let hi = int(a(1)).unwrap_or(lo);
            let (lo, hi) = (lo.max(0) as u32, hi.max(0) as u32);
            let world = host.world();
            match name {
                "obj_create" => world.obj_create(lo, hi),
                "obj_kill" => world.obj_kill(lo, hi),
                // (`obj_show` only ever shows, whatever its help says.)
                "obj_show" => world.obj_show(lo, hi, true),
                _ => world.obj_show(lo, hi, false),
            }
            nil()
        }
        // `env_anim a [b]`: the placed objects `a`–`b` start their animation from the
        // beginning; `_block` waits while one plays on (as shipped, only when given
        // `b`).
        "env_anim" | "env_anim_block" => {
            let Some(lo) = int(a(0)) else { return nil() };
            let hi = int(a(1)).map(|h| h.max(0) as u32);
            if host
                .world()
                .env_anim(lo.max(0) as u32, hi, name == "env_anim_block")
            {
                return Ok((Value::Nil, Value::Boolean(true), None, None));
            }
            nil()
        }
        // `env_setanim id clip` (`OBrSetAnimationByName`): those objects take the
        // named clip and start it; `_block` with one still playing does nothing, as
        // shipped. Warns if no such object or clip.
        "env_setanim" | "env_setanim_block" => {
            let (Some(id), Some(clip)) = (int(a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if !host
                .world()
                .env_setanim(id.max(0) as u32, &clip, name == "env_setanim_block")
            {
                state.warn(format!("{name}: no object {id} or animation {clip}"));
            }
            nil()
        }
        // --- collision and environment animations --------------------------
        // `chr_nocollision who on`: whether the character passes through walls and
        // others.
        "chr_nocollision" => {
            let Some(id) = fighter(host, a(0)) else {
                return nil();
            };
            host.world().fighters[id].no_collision = truthy(a(1));
            nil()
        }
        // An object animation moves the character (its pelvis on the clip's
        // path, no collision) from frame 1 — the script sets 1, but
        // `OBrAnim_Start` resets it to 0 before the `+= 1`, so it keeps step
        // with an object started the same tick (a frame ahead and a fast
        // motorcycle leaves its rider over the front wheel); `_block` waits
        // for one already playing to end, then starts and goes on at once.
        "chr_envanim" | "chr_envanim_block" => {
            let (Some(id), Some(clip)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let world = host.world();
            if name == "chr_envanim_block" {
                if let Some((anim, frame)) = &world.fighters[id].env_anim {
                    if frame + 1 < i32::from(anim.num_frames) {
                        return Ok((Value::Nil, Value::Boolean(true), None, None));
                    }
                }
            }
            match world.object_anim(&clip) {
                Some(anim) => {
                    let f = &mut world.fighters[id];
                    f.env_anim = Some((anim, 1));
                    f.env_anim_no_rotation = a(2).and_then(text).is_some_and(|t| t == "norotation");
                }
                None => state.warn(format!("{name}: no animation {clip}")),
            }
            nil()
        }
        // `chr_envanim_stop who`: ends the object animation moving the character.
        "chr_envanim_stop" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().fighters[id].env_anim = None;
            }
            nil()
        }
        // --- scripted animation ------------------------------------------------
        // `chr_animate who clip [frames] [interp]`: plays the clip on the character
        // now, blending over `interp` frames, then holds other clips off for `frames`
        // ticks (−1: its length); `_block` waits for an earlier one to end. Warns if
        // the clip isn't found.
        "chr_animate" | "chr_animate_block" => {
            let (Some(id), Some(clip)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let f = &mut host.world().fighters[id];
            if name == "chr_animate_block" && f.motion.lock_frames > 1 {
                return Ok((Value::Nil, Value::Boolean(true), None, None)); // wait for the earlier one
            }
            let frames = int(a(2)).unwrap_or(0);
            let interp = int(a(3)).filter(|&n| n >= 0).map(|n| n as u16);
            let ch = f.character.clone();
            let own = f
                .motion
                .coll
                .entries
                .iter()
                .any(|e| e.name.eq_ignore_ascii_case(&clip));
            let elsewhere = if own { None } else { host.clip(&clip) };
            let f = &mut host.world().fighters[id];
            if !f.motion.animate(&ch, &clip, frames, interp, elsewhere) {
                state.warn(format!("chr_animate: no animation {clip}"));
            }
            nil()
        }

        // A team by its name in the original's list (exactly as written).
        "chr_changeteam" => {
            const NAMES: [(&str, &str); 8] = [
                ("Konoko", "konoko"),
                ("TCTF", "tctf"),
                ("Syndicate", "syndicate"),
                ("Neutral", "neutral"),
                ("SecurityGuard", "securityGuard"),
                ("RogueKonoko", "rogueKonoko"),
                ("Switzerland", "switzerland"),
                ("SyndicateAccessory", "syndicateAccessory"),
            ];
            let wanted = a(1).and_then(text).unwrap_or_default();
            let Some(&(_, ours)) = NAMES.iter().find(|(n, _)| *n == wanted) else {
                state.warn(format!("chr_changeteam: no team {wanted:?}"));
                return nil();
            };
            if let Some(p) = fighter(host, a(0))
                .and_then(|id| host.world().fighters.get_mut(id))
                .and_then(|f| f.perception.as_mut())
            {
                p.team = crate::ai::perception::Team::from_name(ours);
            }
            nil()
        }
        // `chr_shadow who on`: shows or takes away the character's shadow.
        "chr_shadow" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.no_shadow = int(a(1)).unwrap_or(0) <= 0;
            }
            nil()
        }
        // `chr_death_lock who on`: the character can die but its body is never
        // removed.
        "chr_death_lock" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.death.lock = int(a(1)).unwrap_or(0) != 0;
            }
            nil()
        }
        // --- turrets ----------------------------------------
        // `turret_activate` / `_deactivate` / `_reset id`: switches the turrets with
        // that script id (−1: every turret, except `_deactivate`, which then switches
        // none, as shipped).
        "turret_activate" | "turret_deactivate" | "turret_reset" => {
            let id = int(a(0)).unwrap_or(-1);
            let what = name.trim_start_matches("turret_");
            host.world().turret_set(id, what);
            nil()
        }
        // Resets every turret.
        "reset_mechanics" => {
            host.world().turret_set(-1, "reset");
            nil()
        }
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

        // --- laser triggers --------------------------------------------------
        // Laser trigger `id` (none or -1: every one, `OBJrTrigger_*`):
        // switched on or off, reset to its start, hidden or shown; `trig_speed
        // id factor` scales its sweep speed.
        "trig_activate" | "trig_deactivate" | "trig_reset" | "trig_hide" | "trig_show"
        | "trig_speed" => {
            let id = int(a(0)).unwrap_or(crate::lasers::ALL);
            let factor = a(1).and_then(|v| match v {
                Value::Number(n) => Some(*n as f32),
                Value::Integer(n) => Some(*n as f32),
                Value::String(s) => s.to_str().ok()?.trim().parse().ok(),
                _ => None,
            });
            host.world()
                .laser_set(id, &name[5..], factor.unwrap_or(1.0));
            nil()
        }

        // --- doors ------------------------------------------------------------
        // `door_lock` / `door_unlock` / `door_jam` / `door_unjam [id]`: locks or jams
        // door `id` (none or −1: every door), or undoes it.
        "door_lock" | "door_unlock" | "door_jam" | "door_unjam" => {
            let id = int(a(0)).unwrap_or(-1);
            let world = host.world();
            match name {
                "door_lock" => world.door_set(id, Some(true), None),
                "door_unlock" => world.door_set(id, Some(false), None),
                "door_jam" => world.door_set(id, None, Some(true)),
                _ => world.door_set(id, None, Some(false)),
            }
            nil()
        }
        // `door_open` / `door_close [id]`: opens or closes door `id` (none or −1:
        // every door).
        "door_open" | "door_close" => {
            let id = int(a(0)).unwrap_or(-1);
            for d in host
                .world()
                .doors
                .list
                .iter_mut()
                .filter(|d| id < 0 || i64::from(d.id) == id)
            {
                if name == "door_open" {
                    d.open();
                } else {
                    d.close();
                }
            }
            nil()
        }

        // --- consoles, input, alarms ----------------------------------------
        // `console_activate` / `_deactivate` / `_reset [id]`: console `id` (none:
        // every console) made usable or not, or reset to as authored and unused.
        "console_activate" | "console_deactivate" | "console_reset" => {
            let id = int(a(0));
            let world = host.world();
            for k in world
                .consoles
                .iter_mut()
                .filter(|k| id.is_none_or(|i| i64::from(k.id) == i))
            {
                match name {
                    "console_activate" => k.active = true,
                    "console_deactivate" => k.active = false,
                    _ => {
                        k.triggered = false;
                        k.active = k.initial_active;
                    }
                }
            }
            nil()
        }
        // `lock_keys [group…]`: alone, every gameplay key locked; with a group
        // (`keys_movement`, `keys_jump`, `keys_crouch`, `keys_attack`, `keys_action`,
        // `keys_walk`, `keys_inventory`, `keys_reload`, `keys_all`), those keys work
        // again. Pause and hypo keys aren't modelled.
        "lock_keys" => {
            use crate::anim::input::key as k;
            let world = host.world();
            match a(0).and_then(text) {
                // Alone: every gameplay key locked.
                None => world.input.key_mask = 0,
                Some(group) => {
                    world.input.key_mask |= match group.as_str() {
                        "keys_movement" => {
                            k::FORWARD
                                | k::BACKWARD
                                | k::STEP_LEFT
                                | k::STEP_RIGHT
                                | k::TURN_LEFT
                                | k::TURN_RIGHT
                        }
                        "keys_jump" => k::JUMP,
                        "keys_crouch" => k::CROUCH | k::FIRE3,
                        "keys_attack" => k::PUNCH | k::KICK | k::FIRE1 | k::FIRE2,
                        "keys_action" => k::ACTION,
                        "keys_walk" => k::WALK,
                        "keys_inventory" => k::SWAP | k::DROP,
                        "keys_reload" => k::RELOAD,
                        "keys_all" => u32::MAX,
                        _ => 0, // pause, hypo: not modelled
                    }
                }
            }
            nil()
        }
        // `input on`: whether the player's input works at all.
        "input" => {
            host.world().input.enabled = truthy(a(0));
            nil()
        }
        // `ai2_doalarm who [console]`: sends the AI to raise the alarm at that
        // console (none or 0: the nearest alarm console).
        "ai2_doalarm" => {
            if let Some(id) = ai(host, a(0)) {
                let console = int(a(1)).map(|c| c as u16);
                host.world().order_alarm(id, console);
            }
            nil()
        }
        // `ai2_tripalarm group [cause]`: an alarm of that group goes off, caused by
        // `cause` (default the player): AIs answering it learn where the cause is;
        // its alarm script is due. Nothing while every AI is passive.
        "ai2_tripalarm" => {
            let Some(group) = int(a(0)) else { return nil() };
            let cause = match a(1) {
                Some(v) => fighter(host, Some(v)).unwrap_or(0),
                None => 0,
            };
            if !host.world().all_passive {
                host.world().trip_alarm(group as u32, cause);
            }
            nil()
        }

        // --- films -------------------------------------------------------------
        // `playback who film [mode] [interp]`: plays a recorded film on the character
        // (`mode` "fromhere" where it stands, "interp" easing there over `interp`
        // ticks, default 30); `_block` waits the film's length. Warns if it can't
        // play.
        "playback" | "chr_playback" | "playback_block" | "chr_playback_block" => {
            let (Some(id), Some(film)) = (fighter(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            let mode = a(2).and_then(text).unwrap_or_default();
            let interp = int(a(3)).unwrap_or(30).max(0) as u32;
            match host.world().play_film(id, &film, &mode, interp) {
                Some(len) if name.ends_with("_block") => {
                    Ok((Value::Nil, Value::Integer(len as i64), None, None))
                }
                Some(_) => nil(),
                None => {
                    state.warn(format!("{name}: can't play film {film}"));
                    nil()
                }
            }
        }

        // --- waiting on a character ---------------------------------------
        // `chr_wait_animtype` / `_animation` / `_animstate who name…`: waits until
        // the character plays one of those animation types (body or overlay), clips,
        // or states (names matched ignoring case and `_`).
        "chr_wait_animtype" | "chr_wait_animation" | "chr_wait_animstate" => {
            let Some(id) = fighter(host, a(0)) else {
                return nil();
            };
            let f = &host.world().fighters[id];
            // (A type counts if the body or an overlay plays it.)
            let now: Vec<String> = match name {
                "chr_wait_animtype" => f
                    .anim_types()
                    .into_iter()
                    .map(|t| {
                        oni_schema::name(&oni_schema::ANIM_TYPES, t)
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect(),
                "chr_wait_animstate" => {
                    vec![
                        oni_schema::name(&oni_schema::ANIM_STATES, f.motion.current().to_state)
                            .unwrap_or("")
                            .to_string(),
                    ]
                }
                _ => vec![f.motion.name().to_string()],
            };
            let key = |s: &str| {
                s.chars()
                    .filter(|c| *c != '_')
                    .collect::<String>()
                    .to_ascii_lowercase()
            };
            let now: Vec<String> = now.iter().map(|n| key(n)).collect();
            let hit = args
                .iter()
                .skip(1)
                .filter_map(text)
                .any(|w| now.contains(&key(&w)));
            Ok((Value::Nil, Value::Boolean(!hit), None, None))
        }
        // `chr_wait_health who hp`: waits until the character's hit points are `hp`
        // or less.
        "chr_wait_health" => {
            let (Some(id), Some(hp)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let above = host.world().fighters[id].hp as i64 > hp;
            Ok((Value::Nil, Value::Boolean(above), None, None))
        }
        "chr_is_player" => done(Value::Integer((fighter(host, a(0)) == Some(0)) as i64)),
        // How many of the references in a range have all their glass broken.
        "env_broken" => {
            let Some(from) = int(a(0)) else { return nil() };
            let to = int(a(1)).unwrap_or(from);
            done(Value::Integer(i64::from(
                host.world().env_broken(from as u32, to as u32),
            )))
        }
        // `chr_has_empty_weapon who` → 1 if it holds an empty gun with nothing to
        // reload it and no reload under way, else 0.
        "chr_has_empty_weapon" => {
            let empty = fighter(host, a(0)).is_some_and(|id| host.world().has_empty_weapon(id));
            done(Value::Integer(empty as i64))
        }
        // `chr_has_lsi who` → 1 if the character carries the LSI, else 0.
        "chr_has_lsi" => {
            let lsi = fighter(host, a(0))
                .and_then(|id| host.world().fighters.get(id))
                .is_some_and(|f| f.inventory.lsi);
            done(Value::Integer(lsi as i64))
        }
        // → 1 if the player killed Griffin (as a script said this session, else as
        // kept with the saved game), else 0.
        "did_kill_griffen" => done(Value::Integer(did_kill_griffin(host) as i64)),
        "killed_griffen" => {
            // (`ONrWeKilledGriffen`: kept with the saved game.)
            let murder = truthy(a(0));
            let world = host.world();
            world.progress.killed_griffin = Some(murder);
            if world.progress.persist_continues {
                crate::game::set_killed_griffin(murder);
            }
            nil()
        }
        // → the difficulty level (0 easy, 1 normal, 2 hard).
        "difficulty" => done(Value::Integer(host.world().difficulty.level)),

        // --- AI orders -------------------------------------------------------
        // `ai2_dopath who path`: the AI walks the level's patrol path of that name
        // (not while passive).
        "ai2_dopath" => {
            let (Some(id), Some(path)) = (ai(host, a(0)), a(1).and_then(text)) else {
                return nil();
            };
            if host.world().fighters[id].passive {
                eprintln!("script: ai2_dopath: {} is passive", name_of(host, id));
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
        // are treated the same.)
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
        // --- the countdown timer (`world/timer.rs`) ----------------------------
        // `timer_start seconds script` (`AIiScript_Timer_Start`): the HUD countdown
        // from `seconds` (held to an hour), then script function `script` runs.
        "timer_start" => {
            let (Some(seconds), Some(script)) = (float(a(0)), a(1).and_then(text)) else {
                state.warn("timer_start: expected <seconds> <script>".into());
                return nil();
            };
            host.world().timer_start(seconds, &script);
            nil()
        }
        // Turns the countdown off (a success, with its sound, if it was still
        // running).
        "timer_stop" => {
            host.world().timer_stop();
            nil()
        }
        "ai2_takecontrol" => {
            // (`AI2iScript_TakeControl`: the AI movement system takes the
            // player — no sprint, no look offset — or lets go.)
            let on = truthy(a(0));
            let world = host.world();
            world.input.ai_control = on;
            if let Some(p) = world.fighters.first_mut() {
                if on {
                    p.facing.facing_modifier = 0.0;
                    p.brain = Some(crate::ai::Brain::Steer(Box::default()));
                } else if matches!(p.brain, Some(crate::ai::Brain::Steer(_))) {
                    p.brain = None;
                }
            }
            nil()
        }
        "ai2_movetoflag" => {
            // (`AI2rPath_GoToPoint` within 3 — `AI2cMoveToPoint_DefaultDistance`
            // — then the flag's facing with "setfacing".)
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else {
                return nil();
            };
            let set_facing = a(2).and_then(text).is_some_and(|s| s == "setfacing");
            let world = host.world();
            let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() else {
                state.warn(format!("ai2_movetoflag: no flag {flag}"));
                return nil();
            };
            let face = set_facing.then_some(f.facing);
            script_move(state, host, id, f.at, face, None, "ai2_movetoflag");
            nil()
        }
        "ai2_lookatchar" => {
            // (Stays where it is, turned toward the character, looking at it.)
            let (Some(id), Some(target)) = (fighter(host, a(0)), fighter(host, a(1))) else {
                return nil();
            };
            let world = host.world();
            let (from, to) = (world.fighters[id].feet(), world.fighters[target].feet());
            let face = (to.x - from.x)
                .atan2(to.z - from.z)
                .rem_euclid(std::f32::consts::TAU);
            script_move(
                state,
                host,
                id,
                from,
                Some(face),
                Some(to),
                "ai2_lookatchar",
            );
            nil()
        }
        "ai2_lookatme" => {
            // (`AI2rDebug_LookHere`: one AI, or every AI, looks at the
            // player — and keeps looking as she moves.)
            let world = host.world();
            let Some(player) = world
                .fighters
                .first()
                .map(|p| p.feet() + Vec3::Y * p.motion.height())
            else {
                return nil();
            };
            let who: Vec<usize> = match a(0) {
                Some(v) => ai(host, Some(v)).into_iter().collect(),
                None => (1..host.world().fighters.len()).collect(),
            };
            for id in who {
                if let Some(crate::ai::Brain::Agent(ag)) = host.world().fighters[id].brain.as_mut()
                {
                    ag.look_at_char(0, player);
                }
            }
            nil()
        }
        "ai2_forget" => {
            // (`AI2rForget`: one AI, or every AI, forgets one character, or
            // everyone.)
            let who: Vec<usize> = match a(0) {
                Some(v) => ai(host, Some(v)).into_iter().collect(),
                None => (1..host.world().fighters.len()).collect(),
            };
            let forget = a(1).and_then(|v| fighter(host, Some(v)));
            for id in who {
                if let Some(p) = host.world().fighters[id].perception.as_mut() {
                    p.forget(forget);
                }
            }
            nil()
        }
        // `ai2_makeblind` / `ai2_makedeaf who on`: the AI sees nothing / hears
        // nothing, or again does.
        "ai2_makeblind" | "ai2_makedeaf" => {
            if let Some(p) =
                ai(host, a(0)).and_then(|id| host.world().fighters[id].perception.as_mut())
            {
                if name == "ai2_makeblind" {
                    p.blind = truthy(a(1));
                } else {
                    p.deaf = truthy(a(1));
                }
            }
            nil()
        }
        // `chr_peace who` (`ONrCharacter_ExitFightMode`): the character leaves its
        // fight stance at once.
        "chr_peace" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().fighters[id].motion.exit_fight_mode();
            }
            nil()
        }
        // `ai2_idle who`: the AI stands idle now.
        "ai2_idle" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().order_idle(id);
            }
            nil()
        }
        // `ai2_setalert who level`: sets the AI's alert level (`lull`, `low`,
        // `medium`, `high`, `combat`; anything else lull).
        "ai2_setalert" => {
            if let (Some(id), Some(level)) = (ai(host, a(0)), a(1).and_then(text)) {
                if let Some(p) = host.world().fighters[id].perception.as_mut() {
                    p.alert = crate::ai::level::alert_named(&level);
                }
            }
            nil()
        }

        // --- trigger volumes -------------------------------------------------
        // `trigvolume_enable name on [entry|inside|exit]`: the named trigger volumes
        // switched on or off, wholly or just one of their events; disabled wholly,
        // they forget who is inside (re-enabled, entry fires again).
        "trigvolume_enable" => {
            let on = truthy(a(1));
            let kind = a(2).and_then(text);
            let found = state.triggers.by_name(a(0));
            if found.is_empty() {
                state.warn(format!(
                    "trigvolume_enable: no volume {:?}",
                    a(0).and_then(text)
                ));
            }
            for i in found {
                let bit = match kind.as_deref() {
                    Some("entry") => tv::ENTRY_OFF,
                    Some("inside") => tv::ENTRY_OFF << 1,
                    Some("exit") => tv::ENTRY_OFF << 2,
                    _ => tv::DISABLED,
                };
                let v = &mut state.triggers.volumes[i];
                if on {
                    v.flags &= !bit;
                } else {
                    v.flags |= bit;
                    if bit == tv::DISABLED {
                        // (Forgotten without an exit: re-enabling fires entry again.)
                        let id = v.id;
                        for list in state.triggers.inside.values_mut() {
                            list.retain(|&x| x != id);
                        }
                    }
                }
            }
            nil()
        }
        // `trigvolume_reset name`: the named trigger volumes back to their authored
        // flags and scripts.
        "trigvolume_reset" => {
            for i in state.triggers.by_name(a(0)) {
                let v = &mut state.triggers.volumes[i];
                v.flags = v.authored_flags;
                v.scripts = v.authored_scripts.clone();
            }
            nil()
        }
        // `trigvolume_setscript name script [entry|inside|exit]`: the script function
        // the named volumes call on that event (default entry).
        "trigvolume_setscript" => {
            let kind = match a(2).and_then(text).as_deref() {
                Some("inside") => 1,
                Some("exit") => 2,
                _ => 0,
            };
            let script = a(1).and_then(text).unwrap_or_default();
            for i in state.triggers.by_name(a(0)) {
                state.triggers.volumes[i].scripts[kind] = script.clone();
            }
            nil()
        }
        // `trigvolume_trigger name [entry|inside|exit] [who]`: fires that event of
        // the named volumes as if `who` (default the player) did it.
        "trigvolume_trigger" => {
            let kind = match a(1).and_then(text).as_deref() {
                Some("inside") => 1,
                Some("exit") => 2,
                _ => 0,
            };
            let who = match a(2) {
                Some(v) => fighter(host, Some(v)).unwrap_or(0),
                None => 0,
            };
            let who = name_of(host, who);
            let mut out = Vec::new();
            for i in state.triggers.by_name(a(0)) {
                state.triggers.fire(i, kind, &who, &mut out);
            }
            state.pending_events.extend(out);
            nil()
        }
        // `trigvolume_count id` → how many living characters of the volume's teams
        // are inside trigger volume `id`.
        "trigvolume_count" => {
            let found = state.triggers.by_id(a(0));
            let world = host.world();
            let mut n = 0;
            for i in found {
                let v = &state.triggers.volumes[i];
                n += world
                    .fighters
                    .iter()
                    .filter(|f| {
                        f.alive() && v.teams & (1 << f.team() as u32) != 0 && {
                            let feet = f.feet();
                            v.crossed(feet, feet + Vec3::Y * f.motion.height().max(12.0))
                        }
                    })
                    .count();
            }
            done(Value::Integer(n as i64))
        }
        // `trigvolume_kill id`: removes every non-player character inside trigger
        // volume `id`; `trigvolume_corpse`: only the dead ones, and the corpses there
        // (`ONrGameState_KillCorpsesInsideTrigger`).
        "trigvolume_kill" | "trigvolume_corpse" => {
            let found = state.triggers.by_id(a(0));
            let world = host.world();
            let doomed: Vec<usize> = (1..world.fighters.len())
                .filter(|&j| {
                    let f = &world.fighters[j];
                    let wanted = if name == "trigvolume_kill" {
                        !f.gone
                    } else {
                        !f.gone && !f.alive()
                    };
                    let feet = f.feet();
                    wanted
                        && found.iter().any(|&i| {
                            state.triggers.volumes[i].crossed(feet, feet + Vec3::Y * 12.0)
                        })
                })
                .collect();
            for j in doomed {
                world.remove(j);
            }
            // (And the corpses of the dead of play there:
            // `ONrGameState_KillCorpsesInsideTrigger`.)
            if name == "trigvolume_corpse" {
                world.corpses.clear_where(|lo, hi| {
                    found
                        .iter()
                        .any(|&i| state.triggers.volumes[i].crossed(lo, hi))
                });
            }
            nil()
        }
        // Clears the dead of play, leaving the level's own corpses.
        "corpse_reset" => {
            host.world().corpses.reset();
            nil()
        }

        // --- not carried out yet ------------------------------------------
        _ => {
            let n = state.stubbed.entry(name.to_string()).or_insert(0);
            if *n == 0 {
                eprintln!("script: {name} (not carried out yet)");
            }
            *n += 1;
            done(Value::Integer(0))
        }
    }
}

/// A movement order for fighter `id` (`AI2rPath_GoToPoint`): go to `at`,
/// then face `face`, looking at `look`. The player only while the AI holds
/// her (`ai2_takecontrol 1`), as in the original.
fn script_move(
    state: &mut State,
    host: &mut dyn Host,
    id: usize,
    at: Vec3,
    face: Option<f32>,
    look: Option<Vec3>,
    what: &str,
) {
    const WITHIN: f32 = 0.75 * 4.0; // AI2cMoveToPoint_DefaultDistance
    let world = host.world();
    let alert = world.fighters[id]
        .perception
        .as_ref()
        .map_or(crate::ai::perception::Alert::Lull, |p| p.alert);
    let here = (at - world.fighters[id].feet()).with_y(0.0).length() <= WITHIN;
    match world.fighters[id].brain.as_mut() {
        Some(crate::ai::Brain::Steer(s)) => {
            s.nav = (!here).then(|| {
                crate::ai::navigate::Navigator::new(at, WITHIN, s.mode.pace(alert, false))
            });
            s.face = face;
        }
        Some(crate::ai::Brain::Agent(ag)) => ag.script_move(at, WITHIN, face, look, alert, None),
        _ if id == 0 => state.warn(format!(
            "{what}: cannot move the player without ai2_takecontrol 1"
        )),
        _ => {}
    }
}

/// `chr_create` (`AI2iScript_ChrCreate`, `ONrGameState_NewCharacter` from a
/// setup): the level's character setup with script id `sid`, at its default
/// flag — its class, team, gun and ammo, an AI going about no job — named by
/// the setup or `ai_<n>`; its spawn script runs. Only the cutscenes use it.
pub fn create(state: &mut State, host: &mut dyn Host, sid: i64) -> Option<(String, String)> {
    let world = host.world();
    let Some(setup) = u16::try_from(sid)
        .ok()
        .and_then(|s| world.level_ai.setups.get(&s))
        .cloned()
    else {
        state.warn(format!("chr_create: no character setup {sid}"));
        return None;
    };
    let Some(flag) = world.level_ai.flags.get(&setup.flag).copied() else {
        state.warn(format!("chr_create {sid}: no flag {}", setup.flag));
        return None;
    };
    let class = setup
        .class
        .clone()
        .unwrap_or_else(|| "konoko_generic".into());
    let kind = match host.kind(&class) {
        Ok(k) => k,
        Err(e) => {
            state.warn(format!("chr_create {sid}: {e}"));
            return None;
        }
    };
    let world = host.world();
    let id = world.spawn(&kind, flag.at + Vec3::Y * 10.0, flag.facing, false);
    world.give_goals(id, None, None);
    let f = &mut world.fighters[id];
    f.script_id = Some(setup.script_id);
    let name = setup.name.clone().unwrap_or_else(|| format!("ai_{id}"));
    if let Some(crate::ai::Brain::Agent(a)) = f.brain.as_mut() {
        a.name = name.clone();
    }
    if let Some(p) = f.perception.as_mut() {
        p.team = setup.team;
    }
    if let Some(w) = setup.weapon.as_deref() {
        if world.give_weapon(id, w) {
            if let Some(g) = world.fighters[id].hand {
                world.armory.weapons[g].ammo = setup.ammo;
            }
        }
    }
    if world.all_passive {
        world.set_passive(id, true);
    }
    state.scripts.insert(id, setup.scripts.clone());
    state.fired.remove(&id);
    eprintln!("script: created {name} (setup {sid}, fighter {id})");
    setup.scripts.get("spawn").map(|s| (s.clone(), name))
}

/// Spawns the level's character `name` (the first placement of that name
/// not already spawned, unless `force` or it may spawn many times) with its
/// goals. Returns its spawn script (to run at once) with its name.
pub fn spawn(
    state: &mut State,
    host: &mut dyn Host,
    name: &str,
    force: bool,
) -> Option<(String, String)> {
    let world = host.world();
    let index = world
        .level_ai
        .characters
        .iter()
        .enumerate()
        .position(|(i, c)| {
            c.name == name && (force || c.has("spawnMultiple") || !state.spawned.contains(&i))
        })?;
    let placed = world.level_ai.characters[index].clone();
    state.spawned.insert(index);
    // Its class: a random costume, or upgraded on hard (`ONrGameState_
    // NewCharacter`); its melee profile the new class's default if it had
    // its old class's.
    let roll = world.roll();
    let hard = world.difficulty.level >= 2;
    let (class, default_melee) = match host.classes() {
        Some(c) => (
            c.costume(
                &placed.class,
                placed.has("randomCostume"),
                placed.has("upgradeDifficulty"),
                hard,
                roll,
            ),
            placed.melee_id.is_some() && placed.melee_id == c.default_melee_id(&placed.class),
        ),
        None => (placed.class.clone(), false),
    };
    if class != placed.class {
        eprintln!("script: {name} wears {class} (placed as {})", placed.class);
    }
    let kind = match host.kind(&class) {
        Ok(k) => k,
        Err(e) => {
            state.warn(format!("ai2_spawn {name}: {e}"));
            return None;
        }
    };
    let melee = if default_melee {
        kind.melee.clone()
    } else {
        placed.melee_id.and_then(|m| host.melee(m))
    };
    let world = host.world();
    // (A placement is the feet; spawning takes the pelvis: start a little
    // above and let it settle.)
    let id = world.spawn(&kind, placed.at + Vec3::Y * 10.0, placed.facing, false);
    world.give_goals(id, Some(&placed), melee);
    if world.all_passive {
        world.set_passive(id, true);
    }
    state.scripts.insert(id, placed.scripts.clone());
    state.fired.remove(&id);
    eprintln!("script: spawned {} (fighter {id})", placed.name);
    placed
        .scripts
        .get("spawn")
        .map(|s| (s.clone(), placed.name.clone()))
}
