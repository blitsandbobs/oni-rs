//! The engine commands level scripts call (`scripting/BSL.md`, *Command
//! surface*; semantics in `scripting/BSL.md`, *Runtime*), and the world
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
        return (i == 0 && !host.world().fighters.is_empty()).then_some(0);
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
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let (p, d) = (from - self.corner, to - from);
        for k in 0..3 {
            let (o, v) = (p.dot(self.axes[k]), d.dot(self.axes[k]));
            let (lo, hi) = (0.0, self.size[k]);
            if v.abs() < 1e-6 {
                if o < lo || o > hi {
                    return false;
                }
            } else {
                let (a, b) = ((lo - o) / v, (hi - o) / v);
                let (a, b) = if a < b { (a, b) } else { (b, a) };
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
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
        let doc: serde_json::Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let v3 = |v: &serde_json::Value| {
            let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
            Vec3::new(f(0), f(1), f(2))
        };
        let volumes = doc["triggers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                let s = |k: &str| t[k].as_str().unwrap_or("").to_string();
                let scripts = [s("entry"), s("inside"), s("exit")];
                let names = |k: &str, table: &[&str]| -> u32 {
                    t[k].as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| table.iter().position(|x| Some(*x) == n.as_str()))
                        .fold(0, |m, b| m | 1 << b)
                };
                let flags = names("flags", &oni_schema::TRIGGER_FLAGS);
                Volume {
                    id: t["id"].as_i64().unwrap_or(0),
                    name: s("name"),
                    authored_scripts: scripts.clone(),
                    scripts,
                    teams: names("teams", &oni_schema::TEAMS),
                    corner: v3(&t["position"]),
                    axes: [v3(&t["axes"][0]), v3(&t["axes"][1]), v3(&t["axes"][2])],
                    size: v3(&t["size"]),
                    parent: t["parent"].as_i64(),
                    flags,
                    authored_flags: flags,
                }
            })
            .collect();
        Triggers { volumes, inside: Default::default() }
    }

    fn by_name(&self, v: Option<&Value>) -> Vec<usize> {
        let Some(n) = v.and_then(text) else { return Vec::new() };
        self.volumes.iter().enumerate().filter(|(_, t)| t.name == n).map(|(i, _)| i).collect()
    }

    fn by_id(&self, v: Option<&Value>) -> Vec<usize> {
        let Some(id) = int(v) else { return Vec::new() };
        self.volumes.iter().enumerate().filter(|(_, t)| t.id == id).map(|(i, _)| i).collect()
    }

    /// Fires volume `i`'s script of `kind` (0 entry, 1 inside, 2 exit) for
    /// `who` — and its parent's — honouring the disabled / once flags.
    fn fire(&mut self, i: usize, kind: usize, who: &str, out: &mut Vec<(String, Vec<String>)>) {
        let parent = self.volumes[i].parent.and_then(|p| self.volumes.iter().position(|v| v.id == p));
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

    // Character events.
    let n = host.world().fighters.len();
    for id in 0..n {
        let world = host.world();
        let f = &world.fighters[id];
        let Some(crate::ai::Brain::Agent(a)) = &f.brain else { continue };
        let Some(scripts) = state.scripts.get(&id) else { continue };
        let (dying, hurt) = (f.motion.dying || f.hp <= 0, f.hp < f.max_hp && !f.invincible);
        let combat = matches!(a.goal, crate::ai::goals::Goal::Combat(_));
        let name = a.name.clone();
        let alarmed = std::mem::take(&mut host.world().fighters[id].alarmed);
        // (Its gun empty with nothing to reload it.)
        let dry = host.world().has_empty_weapon(id);
        let fired = state.fired.entry(id).or_default();
        for (event, now) in [("hurt", hurt), ("combat", combat), ("alarm", alarmed), ("outOfAmmo", dry), ("defeated", dying), ("die", dying)] {
            if now && fired.insert(event) {
                if let Some(s) = scripts.get(event) {
                    out.push((s.clone(), vec![name.clone()]));
                }
            }
        }
    }

    // Trigger volumes: each active character's vertical extent against
    // each enabled volume (`docs`: `scripting/BSL.md`, *Trigger volumes*).
    let bodies: Vec<(usize, Vec3, Vec3, u32)> = host
        .world()
        .fighters
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.gone)
        .map(|(i, f)| {
            let feet = f.feet();
            (i, feet, feet + Vec3::Y * f.motion.height().max(12.0), 1u32 << (f.team() as u32))
        })
        .collect();
    let names: Vec<String> = (0..n).map(|i| name_of(host, i)).collect();
    let tr = &mut state.triggers;
    for vi in 0..tr.volumes.len() {
        let v = &tr.volumes[vi];
        if v.flags & tv::DISABLED != 0 {
            continue;
        }
        let teams = v.parent.and_then(|p| tr.volumes.iter().find(|x| x.id == p)).map_or(v.teams, |p| p.teams);
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
pub fn dispatch(state: &mut State, host: &mut dyn Host, lua: &Lua, name: &str, args: Variadic<Value>) -> mlua::Result<Reply> {
    let a = |k: usize| args.get(k);
    let done = |v: Value| Ok((v, Value::Boolean(false), None, None));
    let nil = || done(Value::Nil);
    let _ = lua;
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
                "difficulty" => done(Value::Integer(state.difficulty)),
                "did_kill_griffen" | "chr_has_lsi" | "chr_has_empty_weapon" => done(Value::Integer(0)),
                _ => nil(),
            }
        }
        "@ticks" => {
            let n = match a(0) {
                Some(Value::String(s)) => state.ticks_of_word(&s.to_string_lossy()),
                v => int(v).unwrap_or(0),
            };
            done(Value::Integer(n.max(0)))
        }
        "@fork" | "@adopt" => {
            if let Some(Value::Thread(t)) = a(0) {
                let delay = int(a(1)).unwrap_or(0).max(0) as u64;
                let label = a(2).and_then(text).unwrap_or_else(|| "fork".into());
                state.started.push((t.clone(), label, None, delay));
            }
            nil()
        }
        "@error" => {
            let msg = a(0).and_then(text).unwrap_or_default();
            if !state.errors.contains(&msg) {
                eprintln!("script error: {msg}");
                state.errors.push(msg);
            }
            nil()
        }
        "@get" => {
            let k = a(0).and_then(text).unwrap_or_default();
            done(state.vars.get(&k).cloned().unwrap_or(Value::Integer(0)))
        }
        "@set" => {
            let k = a(0).and_then(text).unwrap_or_default();
            state.vars.insert(k, a(1).cloned().unwrap_or(Value::Nil));
            nil()
        }
        "@still" => nil(),

        // --- output ----------------------------------------------------------
        "dprint" | "dmsg" => {
            let words: Vec<String> = args.iter().filter_map(text).collect();
            eprintln!("script: {}", words.join(" "));
            nil()
        }
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
                state.messages.push(m);
            }
            nil()
        }
        "message_remove" => {
            match a(0).and_then(text) {
                Some(m) => state.messages.retain(|x| *x != m),
                None => state.messages.clear(),
            }
            nil()
        }
        "target_set" => {
            // The HUD's target locator (no HUD yet: said in the terminal).
            let (Some(flag), d) = (int(a(0)), a(1).and_then(|v| match v {
                Value::Number(n) => Some(*n),
                Value::Integer(i) => Some(*i as f64),
                _ => None,
            })) else {
                return nil();
            };
            if d.is_some_and(|d| d == 0.0) {
                eprintln!("TARGET off");
            } else if let Some(f) = host.world().level_ai.flags.get(&(flag as i16)) {
                eprintln!("TARGET flag {flag} at ({:.0}, {:.0}, {:.0})", f.at.x, f.at.y, f.at.z);
            }
            host.world().event_sound_ui("compass");
            nil()
        }
        "objective_set" => {
            state.objective = int(a(0));
            eprintln!("script: objective {:?}", state.objective);
            // (The HUD's new-objective prompt and its sound.)
            host.world().event_sound_ui("objectiveNew");
            nil()
        }
        "objective_complete" => {
            host.world().event_sound("objectiveComplete", None);
            nil()
        }
        "save_game" => {
            // (Saving isn't modelled; an autosave's sound and message are.)
            if a(1).and_then(text).is_some_and(|t| t == "autosave") {
                host.world().event_sound("autosave", None);
                eprintln!("script: autosave");
            }
            nil()
        }
        "begin_cutscene" | "end_cutscene" => {
            host.world().in_cutscene = name == "begin_cutscene";
            nil()
        }
        "letterbox" => {
            host.world().letterbox = truthy(a(0)) && int(a(0)) != Some(0);
            nil()
        }
        "win" | "lose" => {
            state.outcome = Some(name == "win");
            eprintln!("script: {name}");
            nil()
        }

        // --- environmental particles (`docs/PARTICLES.md`) ----------------------
        "particle" => {
            let (Some(tag), Some(command)) = (a(0).and_then(text), a(1).and_then(text)) else { return nil() };
            let event = a(2).and_then(text);
            if !host.world().env_particle_command(&tag, &command, event.as_deref()) {
                state.warn(format!("particle {tag}: unknown command {command}"));
            }
            nil()
        }
        // --- sound (`docs/AUDIO.md`, *Scripts*) ---------------------------------
        // One dialogue voice: a line still on drops a plain play, holds a
        // `_block` play until it is over, is cut off by `_interrupt`. Its
        // subtitle (no HUD yet: in the terminal).
        "sound_dialog_play" | "sound_dialog_play_block" | "sound_dialog_play_interrupt" => {
            let Some(line) = a(0).and_then(text) else { return nil() };
            match host.world().audio.dialog_start(&line, name.ends_with("_interrupt")) {
                Err(_) if name.ends_with("_block") => Ok((Value::Nil, Value::Boolean(true), None, None)),
                Err(_) => nil(),
                // (No such line: nothing, quietly — as shipped.)
                Ok(_) => {
                    if let Some(t) = state.message_text(&line) {
                        eprintln!("SAY ({line}): {t}");
                    }
                    nil()
                }
            }
        }
        // (Called by a few scripts, but never an engine command: nothing.)
        "sound_dialog_play_pause" => nil(),
        // Music and script ambients: one pool of 16 unplaced slots.
        "sound_music_start" | "sound_ambient_start" => {
            let Some(sound) = a(0).and_then(text) else { return nil() };
            let volume = float(a(1)).unwrap_or(1.0);
            if !host.world().audio.script_start(&sound, volume) {
                state.warn(format!("{name}: can't play {sound}"));
            }
            nil()
        }
        "sound_music_stop" | "sound_ambient_stop" => {
            if let Some(sound) = a(0).and_then(text) {
                host.world().audio.script_stop(&sound);
            }
            nil()
        }
        "sound_music_volume" | "sound_ambient_volume" => {
            let (Some(sound), Some(volume)) = (a(0).and_then(text), float(a(1))) else { return nil() };
            let seconds = float(a(2)).unwrap_or(0.0);
            host.world().audio.script_volume(&sound, volume, seconds);
            nil()
        }
        "sound_impulse_play" => {
            let Some(sound) = a(0).and_then(text) else { return nil() };
            let volume = float(a(1)).map(|v| v.clamp(0.0, 1.0));
            let world = host.world();
            world.audio.impulse_play(&sound, None, volume, Some(&world.quads));
            nil()
        }
        // A character says a line: `chr_talk who sound pre post [type]`
        // (no type: override). (The original never stores a given type —
        // uninitialised; here it is used.)
        "chr_talk" => {
            let (Some(who), Some(sound)) = (fighter(host, a(0)), a(1).and_then(text)) else { return nil() };
            let kind = match a(4).and_then(text) {
                None => crate::world::speech::OVERRIDE,
                Some(t) => match ["none", "idle", "chatter", "say", "call", "pain", "yell", "override"].iter().position(|n| *n == t) {
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
            let (Some(who), Some(kind)) = (fighter(host, a(0)), a(1).and_then(text)) else { return nil() };
            if let Some(k) = oni_schema::VOCALIZATIONS.iter().position(|v| v.eq_ignore_ascii_case(&kind)) {
                host.world().vocalize(who, k, true);
            }
            nil()
        }

        // --- characters ------------------------------------------------------
        "ai2_spawn" => {
            let Some(who) = a(0).and_then(text) else { return nil() };
            let force = a(1).and_then(text).is_some_and(|f| f == "force");
            match spawn(state, host, &who, force) {
                Some((script, arg)) => Ok((Value::Nil, Value::Boolean(false), Some(script), Some(arg))),
                None => nil(),
            }
        }
        "chr_delete" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().remove(id);
            }
            nil()
        }
        "chr_teleport" => {
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else { return nil() };
            let world = host.world();
            match world.level_ai.flags.get(&(flag as i16)).copied() {
                Some(f) => world.teleport(id, f.at, None),
                None => state.warn(format!("chr_teleport: no flag {flag}")),
            }
            nil()
        }
        "chr_facetoflag" => {
            let (Some(id), Some(flag)) = (fighter(host, a(0)), int(a(1))) else { return nil() };
            let world = host.world();
            if let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() {
                let at = world.fighters[id].feet();
                world.teleport(id, at, Some(f.facing));
            }
            nil()
        }
        "chr_set_health" => {
            let (Some(id), Some(hp)) = (fighter(host, a(0)), int(a(1))) else { return nil() };
            if let Some(f) = host.world().fighters.get_mut(id) {
                if hp <= 0 && f.alive() {
                    let (was, all) = (f.invincible, f.hp);
                    f.invincible = false;
                    f.take_damage(all, Vec3::ZERO);
                    f.invincible = was;
                } else {
                    f.hp = hp as i32;
                }
            }
            nil()
        }
        "chr_full_health" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.hp = f.max_hp;
            }
            nil()
        }
        "chr_invincible" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.invincible = truthy(a(1));
            }
            nil()
        }
        // --- scripted animation ------------------------------------------------
        "chr_animate" | "chr_animate_block" => {
            let (Some(id), Some(clip)) = (fighter(host, a(0)), a(1).and_then(text)) else { return nil() };
            let f = &mut host.world().fighters[id];
            if name == "chr_animate_block" && f.motion.lock_frames > 1 {
                return Ok((Value::Nil, Value::Boolean(true), None, None)); // wait for the earlier one
            }
            let frames = int(a(2)).unwrap_or(0);
            let interp = int(a(3)).filter(|&n| n >= 0).map(|n| n as u16);
            let ch = f.character.clone();
            let own = f.motion.coll.entries.iter().any(|e| e.name.eq_ignore_ascii_case(&clip));
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
            if let Some(p) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)).and_then(|f| f.perception.as_mut()) {
                p.team = crate::ai::perception::Team::from_name(ours);
            }
            nil()
        }
        "chr_shadow" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.no_shadow = int(a(1)).unwrap_or(0) <= 0;
            }
            nil()
        }
        "chr_death_lock" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.death_lock = int(a(1)).unwrap_or(0) != 0;
            }
            nil()
        }
        // --- turrets (`docs/TURRETS.md`) ----------------------------------------
        "turret_activate" | "turret_deactivate" | "turret_reset" => {
            let id = int(a(0)).unwrap_or(-1);
            let what = name.trim_start_matches("turret_");
            host.world().turret_set(id, what);
            nil()
        }
        "reset_mechanics" => {
            host.world().turret_set(-1, "reset");
            nil()
        }
        // --- weapons (`docs/WEAPONS.md`) ---------------------------------------
        "chr_giveweapon" => {
            let (Some(id), Some(class)) = (fighter(host, a(0)), a(1).and_then(text)) else { return nil() };
            if !host.world().give_weapon(id, &class) {
                state.warn(format!("chr_giveweapon: no weapon {class:?}"));
            }
            nil()
        }
        "weapon_spawn" | "powerup_spawn" => {
            let (Some(what), Some(flag)) = (a(0).and_then(text), int(a(1))) else { return nil() };
            let world = host.world();
            let Some(f) = world.level_ai.flags.get(&(flag as i16)).copied() else {
                state.warn(format!("{name}: no flag {flag}"));
                return nil();
            };
            let made = if name == "weapon_spawn" {
                world.spawn_weapon(&what, f.at + Vec3::Y * crate::weapons::PLACED_LIFT, f.facing, false).is_some()
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
            let Some(what) = a(0).and_then(text) else { return nil() };
            let amount = int(a(1)).map(|n| n as u16);
            let who: Vec<usize> = match int(a(2)) {
                Some(n) if n < 0 => (0..host.world().fighters.len()).filter(|&i| host.world().fighters[i].alive()).collect(),
                Some(_) | None if a(2).is_some() => fighter(host, a(2)).into_iter().collect(),
                _ => vec![0],
            };
            for id in who {
                host.world().give_powerup(id, &what, amount, true);
            }
            nil()
        }
        "chr_forceholster" => {
            let Some(id) = fighter(host, a(0)) else { return nil() };
            host.world().force_holster(id, int(a(1)).unwrap_or(0) > 0);
            nil()
        }
        "chr_inv_reset" => {
            if let Some(id) = fighter(host, a(0)) {
                host.world().reset_inventory(id);
            }
            nil()
        }
        "chr_weapon_immune" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.weapon_immune = true;
            }
            nil()
        }
        "chr_unstoppable" => {
            if let Some(f) = fighter(host, a(0)).and_then(|id| host.world().fighters.get_mut(id)) {
                f.unstoppable = int(a(1)).unwrap_or(0) > 0;
            }
            nil()
        }
        "chr_poison" => {
            let (Some(id), Some(amount), Some(every)) = (fighter(host, a(0)), int(a(1)), int(a(2))) else { return nil() };
            let initial = int(a(3)).unwrap_or(every);
            host.world().poison(id, amount as i32, every as i32, initial as i32);
            nil()
        }
        // Kills every AI, every AI "except" one, or one.
        "ai2_kill" => {
            let spare = a(0).and_then(text).is_some_and(|t| t == "except");
            let chosen = if spare { fighter(host, a(1)) } else { a(0).and_then(|_| fighter(host, a(0))) };
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
                    let (was, all) = (f.invincible, f.hp);
                    f.invincible = false;
                    f.take_damage(all, Vec3::ZERO);
                    f.invincible = was;
                }
            }
            nil()
        }
        // (Every character here is always active.)
        "chr_lock_active" | "chr_unlock_active" => nil(),

        "env_show" => {
            if let Some(r) = int(a(0)) {
                host.world().env_show(r as u32, truthy(a(1)));
            }
            nil()
        }

        // --- laser triggers --------------------------------------------------
        "trig_activate" | "trig_deactivate" | "trig_reset" | "trig_hide" | "trig_show" | "trig_speed" => {
            let id = int(a(0)).unwrap_or(crate::lasers::ALL);
            let factor = a(1).and_then(|v| match v {
                Value::Number(n) => Some(*n as f32),
                Value::Integer(n) => Some(*n as f32),
                Value::String(s) => s.to_str().ok()?.trim().parse().ok(),
                _ => None,
            });
            host.world().laser_set(id, &name[5..], factor.unwrap_or(1.0));
            nil()
        }

        // --- doors ------------------------------------------------------------
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
        "door_open" | "door_close" => {
            let id = int(a(0)).unwrap_or(-1);
            for d in host.world().doors.iter_mut().filter(|d| id < 0 || i64::from(d.id) == id) {
                if name == "door_open" {
                    d.open();
                } else {
                    d.close();
                }
            }
            nil()
        }

        // --- consoles, input, alarms ----------------------------------------
        "console_activate" | "console_deactivate" | "console_reset" => {
            let id = int(a(0));
            let world = host.world();
            for k in world.consoles.iter_mut().filter(|k| id.is_none_or(|i| i64::from(k.id) == i)) {
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
        "lock_keys" => {
            use crate::anim::input::key as k;
            let world = host.world();
            match a(0).and_then(text) {
                // Alone: every gameplay key locked.
                None => world.key_mask = 0,
                Some(group) => {
                    world.key_mask |= match group.as_str() {
                        "keys_movement" => k::FORWARD | k::BACKWARD | k::STEP_LEFT | k::STEP_RIGHT | k::TURN_LEFT | k::TURN_RIGHT,
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
        "input" => {
            host.world().input_enabled = truthy(a(0));
            nil()
        }
        "ai2_doalarm" => {
            if let Some(id) = ai(host, a(0)) {
                let console = int(a(1)).map(|c| c as u16);
                host.world().order_alarm(id, console);
            }
            nil()
        }
        "ai2_tripalarm" => {
            let Some(group) = int(a(0)) else { return nil() };
            let cause = match a(1) {
                Some(v) => fighter(host, Some(v)).unwrap_or(0),
                None => 0,
            };
            if !state.all_passive {
                host.world().trip_alarm(group as u32, cause);
            }
            nil()
        }

        // --- films -------------------------------------------------------------
        "playback" | "chr_playback" | "playback_block" | "chr_playback_block" => {
            let (Some(id), Some(film)) = (fighter(host, a(0)), a(1).and_then(text)) else { return nil() };
            let mode = a(2).and_then(text).unwrap_or_default();
            let interp = int(a(3)).unwrap_or(30).max(0) as u32;
            match host.world().play_film(id, &film, &mode, interp) {
                Some(len) if name.ends_with("_block") => Ok((Value::Nil, Value::Integer(len as i64), None, None)),
                Some(_) => nil(),
                None => {
                    state.warn(format!("{name}: can't play film {film}"));
                    nil()
                }
            }
        }

        // --- waiting on a character ---------------------------------------
        "chr_wait_animtype" | "chr_wait_animation" | "chr_wait_animstate" => {
            let Some(id) = fighter(host, a(0)) else { return nil() };
            let f = &host.world().fighters[id];
            // (A type counts if the body or an overlay plays it.)
            let now: Vec<String> = match name {
                "chr_wait_animtype" => {
                    f.anim_types().into_iter().map(|t| oni_schema::name(&oni_schema::ANIM_TYPES, t).unwrap_or("").to_string()).collect()
                }
                "chr_wait_animstate" => {
                    vec![oni_schema::name(&oni_schema::ANIM_STATES, f.motion.current().to_state).unwrap_or("").to_string()]
                }
                _ => vec![f.motion.name().to_string()],
            };
            let key = |s: &str| s.chars().filter(|c| *c != '_').collect::<String>().to_ascii_lowercase();
            let now: Vec<String> = now.iter().map(|n| key(n)).collect();
            let hit = args.iter().skip(1).filter_map(text).any(|w| now.contains(&key(&w)));
            Ok((Value::Nil, Value::Boolean(!hit), None, None))
        }
        "chr_wait_health" => {
            let (Some(id), Some(hp)) = (fighter(host, a(0)), int(a(1))) else { return nil() };
            let above = host.world().fighters[id].hp as i64 > hp;
            Ok((Value::Nil, Value::Boolean(above), None, None))
        }
        "chr_is_player" => done(Value::Integer((fighter(host, a(0)) == Some(0)) as i64)),
        // How many of the references in a range have all their glass broken.
        "env_broken" => {
            let Some(from) = int(a(0)) else { return nil() };
            let to = int(a(1)).unwrap_or(from);
            done(Value::Integer(i64::from(host.world().env_broken(from as u32, to as u32))))
        }
        "chr_has_empty_weapon" => {
            let empty = fighter(host, a(0)).is_some_and(|id| host.world().has_empty_weapon(id));
            done(Value::Integer(empty as i64))
        }
        "chr_has_lsi" => {
            let lsi = fighter(host, a(0)).and_then(|id| host.world().fighters.get(id)).is_some_and(|f| f.inventory.lsi);
            done(Value::Integer(lsi as i64))
        }
        "did_kill_griffen" => done(Value::Integer(0)),
        "difficulty" => done(Value::Integer(state.difficulty)),

        // --- AI orders -------------------------------------------------------
        "ai2_dopath" => {
            let (Some(id), Some(path)) = (ai(host, a(0)), a(1).and_then(text)) else { return nil() };
            if host.world().fighters[id].passive {
                eprintln!("script: ai2_dopath: {} is passive", name_of(host, id));
                return nil();
            }
            host.world().order_path(id, &path);
            nil()
        }
        "ai2_setjobstate" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().make_job(id);
            }
            nil()
        }
        "ai2_passive" | "chr_freeze" | "chr_neutral" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().set_passive(id, truthy(a(1)));
            }
            nil()
        }
        "ai2_allpassive" => {
            let on = truthy(a(0));
            state.all_passive = on;
            let world = host.world();
            world.all_passive = on;
            for id in 1..world.fighters.len() {
                world.set_passive(id, on);
            }
            nil()
        }
        "ai2_attack" | "ai2_makeaware" => {
            if let (Some(id), Some(target)) = (ai(host, a(0)), fighter(host, a(1))) {
                host.world().order_attack(id, target, name == "ai2_attack");
            }
            nil()
        }
        "ai2_makeignoreplayer" => {
            if let Some(p) = ai(host, a(0)).and_then(|id| host.world().fighters[id].perception.as_mut()) {
                p.ignore_player = truthy(a(1));
            }
            nil()
        }
        "ai2_setmovementmode" => {
            if let (Some(id), Some(mode)) = (ai(host, a(0)), a(1).and_then(text)) {
                if let Some(crate::ai::Brain::Agent(ag)) = host.world().fighters[id].brain.as_mut() {
                    ag.mode = crate::ai::goals::MoveMode::from_script(&mode);
                }
            }
            nil()
        }
        "ai2_idle" => {
            if let Some(id) = ai(host, a(0)) {
                host.world().order_idle(id);
            }
            nil()
        }
        "ai2_setalert" => {
            if let (Some(id), Some(level)) = (ai(host, a(0)), a(1).and_then(text)) {
                if let Some(p) = host.world().fighters[id].perception.as_mut() {
                    p.alert = crate::ai::level::alert_named(&level);
                }
            }
            nil()
        }

        // --- trigger volumes -------------------------------------------------
        "trigvolume_enable" => {
            let on = truthy(a(1));
            let kind = a(2).and_then(text);
            let found = state.triggers.by_name(a(0));
            if found.is_empty() {
                state.warn(format!("trigvolume_enable: no volume {:?}", a(0).and_then(text)));
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
        "trigvolume_reset" => {
            for i in state.triggers.by_name(a(0)) {
                let v = &mut state.triggers.volumes[i];
                v.flags = v.authored_flags;
                v.scripts = v.authored_scripts.clone();
            }
            nil()
        }
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
        "trigvolume_kill" | "trigvolume_corpse" => {
            let found = state.triggers.by_id(a(0));
            let world = host.world();
            let doomed: Vec<usize> = (1..world.fighters.len())
                .filter(|&j| {
                    let f = &world.fighters[j];
                    let wanted = if name == "trigvolume_kill" { !f.gone } else { !f.gone && !f.alive() };
                    let feet = f.feet();
                    wanted && found.iter().any(|&i| state.triggers.volumes[i].crossed(feet, feet + Vec3::Y * 12.0))
                })
                .collect();
            for j in doomed {
                world.remove(j);
            }
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

/// Spawns the level's character `name` (the first placement of that name
/// not already spawned, unless `force` or it may spawn many times) with its
/// goals. Returns its spawn script (to run at once) with its name.
pub fn spawn(state: &mut State, host: &mut dyn Host, name: &str, force: bool) -> Option<(String, String)> {
    let world = host.world();
    let index = world.level_ai.characters.iter().enumerate().position(|(i, c)| {
        c.name == name && (force || c.has("spawnMultiple") || !state.spawned.contains(&i))
    })?;
    let placed = world.level_ai.characters[index].clone();
    state.spawned.insert(index);
    let kind = match host.kind(&placed.class) {
        Ok(k) => k,
        Err(e) => {
            state.warn(format!("ai2_spawn {name}: {e}"));
            return None;
        }
    };
    let melee = placed.melee_id.and_then(|m| host.melee(m));
    let world = host.world();
    // (A placement is the feet; spawning takes the pelvis: start a little
    // above and let it settle.)
    let id = world.spawn(&kind, placed.at + Vec3::Y * 10.0, placed.facing, false);
    world.give_goals(id, Some(&placed), melee);
    if state.all_passive {
        world.set_passive(id, true);
    }
    state.scripts.insert(id, placed.scripts.clone());
    state.fired.remove(&id);
    eprintln!("script: spawned {} (fighter {id})", placed.name);
    placed.scripts.get("spawn").map(|s| (s.clone(), placed.name.clone()))
}
