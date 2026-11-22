//! The **mission**: a level's own scripts, running (`docs/lua.md`,
//! `docs/lua.md`). The converter translated the level's BSL to Lua
//! (`levels/NN/scripts.lua`); here a Lua state runs it — `main` at the
//! start, event functions as the level calls for them — on a cooperative
//! scheduler (`sleep` suspends a task for N ticks, `fork` starts another),
//! with each engine command carried out against the [`World`].

mod commands;
mod fixes;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use mlua::{Function, Lua, MultiValue, Thread, ThreadStatus, Value, Variadic};

use crate::world::{CharKind, World};

/// One game tick, as the play app and the scenario host both run it: the
/// world steps with the player's `controls`, then the level's scripts run.
pub fn game_tick(host: &mut dyn Host, mission: Option<&mut Mission>, controls: u32) {
    host.world().step(controls);
    if let Some(m) = mission {
        m.tick(host);
    }
    // (What the tick asked to keep between runs, written.)
    crate::game::persist(std::mem::take(&mut host.world().persist));
}

/// What the mission needs from whoever runs it.
pub trait Host {
    fn world(&mut self) -> &mut World;
    /// The spawnable kind for a character class (loading it if need be).
    fn kind(&mut self, class: &str) -> Result<CharKind, String>;
    /// A melee profile by id.
    fn melee(&mut self, id: u16) -> Option<std::rc::Rc<crate::ai::melee::profile::Profile>>;
    /// A clip by name from any character's animations (as a one-clip
    /// collection), for a script playing it on someone else.
    fn clip(
        &mut self,
        _name: &str,
    ) -> Option<std::rc::Rc<crate::anim::collection::AnimCollection>> {
        None
    }
    /// The character classes (a placement's costume, its default melee).
    fn classes(&self) -> Option<&crate::classes::Classes> {
        None
    }
}

/// A running script task.
struct Task {
    co: Thread,
    /// Resume at this tick.
    wake: u64,
    label: String,
    /// A patrolling fighter waiting for this script to end.
    waiter: Option<usize>,
    /// The blocking command it waits on, and since which tick (none: asleep).
    on: Option<(String, u64)>,
}

/// What the scripts have changed or asked for, besides the world.
#[derive(Default)]
pub struct State {
    pub now: u64,
    /// Engine variables scripts read / write (`save_point`, fog, …).
    pub vars: HashMap<String, Value>,
    /// Trigger volumes.
    pub triggers: commands::Triggers,
    pub objective: Option<i64>,
    pub messages: Vec<String>,
    /// `win` (true) / `lose` (false).
    pub outcome: Option<bool>,
    /// When shown messages with a time limit go away (tick).
    pub message_until: HashMap<String, u64>,
    /// The game's message texts (`message`), and dialogue subtitles.
    pub texts: HashMap<String, String>,
    pub subtitles: crate::hud::Subtitles,
    /// Errors that stopped a script (logged once each).
    pub errors: Vec<String>,
    /// Commands that reported a problem and carried on (the original logs
    /// these to its console: a volume or flag that doesn't exist, …).
    pub warnings: Vec<String>,
    /// Commands not carried out (stubs), with how often they were called.
    pub stubbed: HashMap<String, u32>,
    /// Tasks started this tick (forks, events): label, who waits for it,
    /// and in how many ticks it next runs.
    started: Vec<(Thread, String, Option<usize>, u64)>,
    /// Placements (`LevelAi::characters` indices) already spawned.
    pub spawned: HashSet<usize>,
    /// Each spawned fighter's event scripts.
    pub scripts: HashMap<usize, HashMap<String, String>>,
    /// Event scripts to start at the next tick (a spawn's script, …).
    pub(super) pending_events: Vec<(String, Vec<String>)>,
    /// Character event scripts fired, per fighter (once per life).
    fired: HashMap<usize, HashSet<&'static str>>,
}

impl State {
    /// Records a script error (logged once).
    pub fn error(&mut self, msg: String) {
        if !self.errors.contains(&msg) {
            eprintln!("script error: {msg}");
            self.errors.push(msg);
        }
    }

    /// A message's text (the messages table, else the subtitles without the
    /// name's first letter), its markup stripped.
    pub fn message_text(&self, name: &str) -> Option<String> {
        let key = name.to_ascii_lowercase();
        // (Else the subtitles, by the name without its first letter, as the
        // `message` command looks — `crate::hud::Hud::message`.)
        let raw = self
            .texts
            .get(&key)
            .map(String::as_str)
            .or_else(|| self.subtitles.find(name.get(1..).unwrap_or("")))?;
        // `[o.text]` / `[c.text]`: styled spans.
        let mut out = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '[' => {
                    // skip the style letter and its dot
                    if chars.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                        chars.next();
                        if chars.peek() == Some(&'.') {
                            chars.next();
                        }
                    }
                }
                ']' => {}
                c => out.push(c),
            }
        }
        Some(out)
    }

    /// Records a command's complaint (logged once; the script carries on).
    pub fn warn(&mut self, msg: String) {
        if !self.warnings.contains(&msg) {
            eprintln!("script warning: {msg}");
            self.warnings.push(msg);
        }
    }

    /// A tick count given as a word (`sleep` of a string): its number, or 0.
    pub fn ticks_of_word(&self, w: &str) -> i64 {
        w.trim().parse().unwrap_or(0)
    }
}

/// At most this many tasks can be waiting (asleep or blocked) at once; a
/// task that would wait beyond that dies (as in the original).
const MAX_TASKS: usize = 32;

pub struct Mission {
    lua: Lua,
    tasks: Vec<Task>,
    pub state: State,
}

impl Mission {
    /// Loads a level's translated scripts (`level_dir/scripts.lua`).
    pub fn load(level_dir: &Path) -> Result<Mission, String> {
        let chunk = std::fs::read_to_string(level_dir.join("scripts.lua"))
            .map_err(|e| format!("{}: {e}", level_dir.join("scripts.lua").display()))?;
        let lua = Lua::new();
        lua.load(include_str!("prelude.lua"))
            .set_name("prelude")
            .exec()
            .map_err(|e| e.to_string())?;
        let load: Function = lua.globals().get("__load").map_err(|e| e.to_string())?;
        load.call::<()>((chunk, "scripts"))
            .map_err(|e| e.to_string())?;
        let mut state = State::default();
        state.vars.insert("save_point".into(), Value::Integer(0));
        let doc: oni_schema::files::MessagesFile =
            crate::json::read_file(level_dir.join("../../common/messages.json"));
        state.texts = crate::hud::messages_by_name(&doc, "messages");
        state.subtitles = crate::hud::Subtitles::from_table(doc.tables.get("subtitles"));
        Ok(Mission {
            lua,
            tasks: Vec::new(),
            state,
        })
    }

    /// Starts the level (`docs/lua.md`, *Level start*): the player is
    /// put at its placement, every character present from the start is
    /// spawned (their spawn scripts run on the first tick), then `main`
    /// runs.
    pub fn begin(&mut self, host: &mut dyn Host) {
        self.state.triggers =
            commands::Triggers::load(&host.world().level_dir.join("triggers.json"));
        if !host.world().quirks.scripts_as_shipped() {
            let level = host.world().level_dir.clone();
            self.apply_fixes(&level);
        }
        let placed = host.world().level_ai.characters.clone();
        if let Some(k) = placed.iter().position(|c| c.has("player")) {
            let p = &placed[k];
            self.state.spawned.insert(k);
            let world = host.world();
            world.player_name = p.name.clone();
            world.teleport(0, p.at + glam::Vec3::Y * 2.0, Some(p.facing));
            let scale = world.difficulty.player_hp;
            if let Some(f) = world.fighters.first_mut() {
                f.max_hp = ((f.max_hp + p.hp_offset) as f32 * scale) as i32;
                f.hp = f.max_hp;
            }
            world.arm(0, p);
            // (Its event scripts: the player's die script loses the level.)
            self.state.scripts.insert(0, p.scripts.clone());
        }
        for c in placed
            .iter()
            .filter(|c| !c.has("player") && !c.has("notInitiallyPresent"))
        {
            if let Some((script, arg)) = commands::spawn(&mut self.state, host, &c.name, false) {
                self.state.pending_events.push((script, vec![arg]));
            }
        }
        if !self.start("main", &[]) {
            self.state.error("no main function".into());
        }
    }

    /// Our fixes to the level's scripts (`docs/deviations.md`), by the
    /// level folder's name (`NN`).
    pub fn apply_fixes(&mut self, level_dir: &Path) {
        let level = level_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(src) = fixes::source(level) else {
            return;
        };
        if let Err(e) = self.lua.load(src).set_name(format!("fixes/{level}")).exec() {
            self.state.error(format!("fixes/{level}: {e}"));
        }
        // (`FIX` reports a function the scripts lack as an error.)
    }

    /// Runs a chunk of Lua in the scripts' state (in the translator's form:
    /// `F.name = function() C.command(…) SLEEP(n) end`) — for tests that
    /// define their own script functions.
    pub fn exec(&self, code: &str) -> Result<(), String> {
        self.lua
            .load(code)
            .set_name("exec")
            .exec()
            .map_err(|e| e.to_string())
    }

    fn function(&self, name: &str) -> Option<Function> {
        let f: mlua::Table = self.lua.globals().get("F").ok()?;
        f.raw_get::<Option<Function>>(name).ok().flatten()
    }

    /// Starts script function `name` (with string arguments) as a new task
    /// (it first runs at the next [`tick`](Self::tick)). False if there is
    /// no such function.
    pub fn start(&mut self, name: &str, args: &[&str]) -> bool {
        self.start_for(name, args, None)
    }

    fn start_for(&mut self, name: &str, args: &[&str], waiter: Option<usize>) -> bool {
        let Some(f) = self.function(name) else {
            return false;
        };
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let lua = &self.lua;
        let made = lua
            .create_function(move |_, ()| Ok(args.clone()))
            .and_then(|a| {
                // A wrapper so the task's first resume passes the arguments.
                let w: Function = lua
                    .load("local f, a = ... return function() f(table.unpack(a())) end")
                    .call((f, a))?;
                lua.create_thread(w)
            });
        match made {
            Ok(co) => {
                self.state.started.push((co, name.to_string(), waiter, 0));
                true
            }
            Err(e) => {
                self.state.errors.push(format!("{name}: {e}"));
                false
            }
        }
    }

    /// One tick: the world's events start their scripts, then every task
    /// that is due runs until it sleeps or ends.
    pub fn tick(&mut self, host: &mut dyn Host) {
        self.state.now += 1;
        // Messages shown for a while go away.
        let now = self.state.now;
        let st = &mut self.state;
        let expired: Vec<String> = st
            .message_until
            .iter()
            .filter(|(_, &t)| t <= now)
            .map(|(m, _)| m.clone())
            .collect();
        for m in expired {
            st.message_until.remove(&m);
            st.messages.retain(|x| *x != m);
        }
        let mut events = std::mem::take(&mut self.state.pending_events);
        events.append(&mut host.world().script_events);
        events.extend(commands::events(&mut self.state, host));
        for (name, arg) in events {
            let args: Vec<&str> = arg.iter().map(String::as_str).collect();
            if !self.start(&name, &args) {
                eprintln!("script: no function {name} to run");
            }
        }
        // Patrol scripts (`patrolscriptNNNN`, with the character's name).
        for id in 0..host.world().fighters.len() {
            let Some(crate::ai::Brain::Agent(a)) = host.world().fighters[id].brain.as_mut() else {
                continue;
            };
            let (asked, who) = (std::mem::take(&mut a.scripts), a.name.clone());
            for name in asked {
                if !self.start_for(&name, &[&who], Some(id)) {
                    eprintln!("script: no patrol script {name}");
                    if let Some(crate::ai::Brain::Agent(a)) =
                        host.world().fighters[id].brain.as_mut()
                    {
                        a.script_done();
                    }
                }
            }
        }
        let mut finished_waits = Vec::new();
        let Mission { lua, tasks, state } = self;
        // (The dispatcher and the loop below share the state.)
        let cell = std::cell::RefCell::new(std::mem::take(state));
        let result = lua.scope(|scope| {
            let dispatch =
                scope.create_function_mut(|lua, (name, args): (String, Variadic<Value>)| {
                    commands::dispatch(&mut cell.borrow_mut(), host, lua, &name, args)
                })?;
            lua.globals().set("__cmd", dispatch)?;
            let now = cell.borrow().now;
            // Tasks started now run this tick; so do those they start.
            let mut i = 0;
            loop {
                for (co, label, waiter, delay) in cell.borrow_mut().started.drain(..) {
                    tasks.push(Task {
                        co,
                        wake: now + delay,
                        label,
                        waiter,
                        on: None,
                    });
                }
                if i >= tasks.len() {
                    break;
                }
                if tasks[i].wake <= now {
                    let t = &mut tasks[i];
                    match t.co.resume::<MultiValue>(()) {
                        Ok(v) if t.co.status() == ThreadStatus::Resumable => {
                            let n = v
                                .front()
                                .and_then(|x| x.as_number().or(x.as_integer().map(|i| i as f64)));
                            t.wake = now + n.unwrap_or(1.0).max(1.0) as u64;
                            // (A blocking command names itself as it waits.)
                            let cmd = v
                                .get(1)
                                .and_then(|x| x.as_string())
                                .map(|s| s.to_string_lossy());
                            t.on = match (cmd, t.on.take()) {
                                (Some(c), Some((p, since))) if c == p => Some((p, since)),
                                (Some(c), _) => Some((c, now)),
                                (None, _) => None,
                            };
                        }
                        Ok(_) => t.wake = u64::MAX, // done
                        Err(e) => {
                            let msg = format!("{}: {}", t.label, first_line(&e.to_string()));
                            t.wake = u64::MAX;
                            cell.borrow_mut().error(msg);
                            // (A failing task skips the rest of the tick.)
                            break;
                        }
                    }
                }
                i += 1;
            }
            // Beyond the task limit, the newest waiting tasks die.
            let waiting: Vec<usize> = (0..tasks.len())
                .filter(|&k| tasks[k].wake != u64::MAX)
                .collect();
            if waiting.len() > MAX_TASKS {
                for &k in &waiting[MAX_TASKS..] {
                    cell.borrow_mut()
                        .error(format!("{}: no task slot left (32)", tasks[k].label));
                    tasks[k].wake = u64::MAX;
                }
            }
            for t in tasks.iter().filter(|t| t.wake == u64::MAX) {
                finished_waits.extend(t.waiter);
            }
            tasks.retain(|t| t.wake != u64::MAX);
            Ok(())
        });
        for id in finished_waits {
            if let Some(crate::ai::Brain::Agent(a)) = host
                .world()
                .fighters
                .get_mut(id)
                .and_then(|f| f.brain.as_mut())
            {
                a.script_done();
            }
        }
        *state = cell.into_inner();
        if let Err(e) = result {
            state.errors.push(format!("scheduler: {e}"));
        }
    }

    /// How many tasks are alive (running or sleeping).
    pub fn tasks(&self) -> usize {
        self.tasks.len()
    }

    /// What each live task is waiting for: a blocking command (and for how
    /// long) or the end of a sleep — for finding a script stuck for good.
    pub fn waiting(&self) -> Vec<String> {
        let now = self.state.now;
        self.tasks
            .iter()
            .map(|t| match &t.on {
                Some((cmd, since)) => {
                    format!(
                        "{}: {cmd} for {} ticks",
                        t.label,
                        now.saturating_sub(*since)
                    )
                }
                None if t.wake == u64::MAX => format!("{}: done", t.label),
                None => format!(
                    "{}: asleep {} more ticks",
                    t.label,
                    t.wake.saturating_sub(now)
                ),
            })
            .collect()
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Headless {
        world: World,
        classes: crate::classes::Classes,
    }

    impl Host for Headless {
        fn world(&mut self) -> &mut World {
            &mut self.world
        }
        fn kind(&mut self, class: &str) -> Result<CharKind, String> {
            self.classes.kind(class, |_| 0)
        }
        fn melee(&mut self, id: u16) -> Option<std::rc::Rc<crate::ai::melee::profile::Profile>> {
            self.classes.melee_by_id(id)
        }
        fn clip(
            &mut self,
            name: &str,
        ) -> Option<std::rc::Rc<crate::anim::collection::AnimCollection>> {
            self.classes.clip(name)
        }
        fn classes(&self) -> Option<&crate::classes::Classes> {
            Some(&self.classes)
        }
    }

    /// Runs `check` on every converted level with translated scripts, each
    /// on its own thread (a level's world is its own), and returns their
    /// failures in level order. None without the converted assets.
    fn each_level(check: impl Fn(&Path, u16, &Path) -> Vec<String> + Sync) -> Vec<String> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../generated/oni");
        let Ok(levels) = std::fs::read_dir(root.join("levels")) else {
            return Vec::new();
        };
        let mut dirs: Vec<PathBuf> = levels
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("scripts.lua").exists())
            .collect();
        dirs.sort();
        std::thread::scope(|s| {
            let runs: Vec<_> = dirs
                .iter()
                .map(|dir| {
                    let level: u16 = dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(1);
                    let (root, check) = (&root, &check);
                    std::thread::Builder::new()
                        .name(format!("level {level}"))
                        .stack_size(64 << 20)
                        .spawn_scoped(s, move || check(root, level, dir))
                        .expect("a level's thread")
                })
                .collect();
            runs.into_iter()
                .flat_map(|r| {
                    r.join()
                        .unwrap_or_else(|_| vec!["a level's check panicked".into()])
                })
                .collect()
        })
    }

    /// Every level's translated scripts load and begin — characters
    /// spawned, `main` running for 10 s of game time — without a script
    /// error. (Skipped when the converted assets aren't there.)
    #[test]
    fn every_level_begins_without_script_errors() {
        let failures = each_level(|root, level, dir| {
            let Ok(classes) = crate::classes::Classes::open(root, level) else {
                return Vec::new();
            };
            // (No level geometry: nothing to stand on.)
            let mut host = Headless {
                world: World::new(),
                classes,
            };
            host.world.load_level(dir);
            let Ok(player) = host.kind("konoko_generic") else {
                return Vec::new();
            };
            host.world.spawn(&player, glam::Vec3::ZERO, 0.0, false);
            let mut m = match Mission::load(dir) {
                Ok(m) => m,
                Err(e) => return vec![format!("{}: {e}", dir.display())],
            };
            m.begin(&mut host);
            for _ in 0..600 {
                host.world.step(0);
                m.tick(&mut host);
            }
            eprintln!(
                "level {level}: {} spawned, {} tasks, {} errors",
                m.state.spawned.len(),
                m.tasks(),
                m.state.errors.len()
            );
            m.state
                .errors
                .iter()
                .map(|e| format!("level {level}: {e}"))
                .collect()
        });
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The player walks into every trigger volume of every level in turn
    /// (firing whatever scripts they run), then the scripts run on for 20 s:
    /// a script error anywhere is reported. (Skipped without assets.)
    #[test]
    fn every_trigger_volume_runs_without_script_errors() {
        let report = each_level(|root, level, dir| {
            let Ok(classes) = crate::classes::Classes::open(root, level) else {
                return Vec::new();
            };
            let mut host = Headless {
                world: World::new(),
                classes,
            };
            host.world.load_level(dir);
            let Ok(player) = host.kind("konoko_generic") else {
                return Vec::new();
            };
            host.world.spawn(&player, glam::Vec3::ZERO, 0.0, false);
            let Ok(mut m) = Mission::load(dir) else {
                return Vec::new();
            };
            m.begin(&mut host);
            let centres: Vec<glam::Vec3> = m
                .state
                .triggers
                .volumes
                .iter()
                .map(|v| {
                    v.corner
                        + v.axes[0] * v.size.x * 0.5
                        + v.axes[2] * v.size.z * 0.5
                        + v.axes[1] * 1.0
                })
                .collect();
            for c in centres {
                host.world.teleport(0, c, None);
                for _ in 0..20 {
                    host.world.step(0);
                    m.tick(&mut host);
                }
            }
            for _ in 0..1200 {
                host.world.step(0);
                m.tick(&mut host);
            }
            eprintln!(
                "level {level}: {} spawned, {} tasks, {} errors, {} stubbed kinds",
                m.state.spawned.len(),
                m.tasks(),
                m.state.errors.len(),
                m.state.stubbed.len()
            );
            for w in &m.state.warnings {
                eprintln!("  level {level} warning: {w}");
            }
            m.state
                .errors
                .iter()
                .map(|e| format!("level {level}: {e}"))
                .collect()
        });
        assert!(report.is_empty(), "{report:#?}");
    }
}
