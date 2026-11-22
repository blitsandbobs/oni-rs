//! The engine commands level scripts call (`docs/lua.md`, *Command
//! surface*; semantics in `docs/lua.md`, *Runtime*), and the world
//! events that start scripts — character events and trigger volumes.
//! Commands the engine can't carry out yet are no-ops, counted in
//! [`State::stubbed`].

mod ai;
mod characters;
mod effects;
mod flow;
mod objects;
mod output;
mod screen;
mod weapons;

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
                feet + Vec3::Y * f.pelvis_height().max(12.0),
                1u32 << (f.team() as u32),
            )
        })
        .collect();
    let names: Vec<String> = (0..n).map(|i| host.world().name_of(i)).collect();
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

/// Carries out one script command (or a prelude request, `@…`): each area
/// module matches its own commands and passes the rest to the next
/// (`output` → `screen` → … → `flow`), the last to [`stubbed`].
pub fn dispatch(
    state: &mut State,
    host: &mut dyn Host,
    lua: &Lua,
    name: &str,
    args: Variadic<Value>,
) -> mlua::Result<Reply> {
    // (`ONI_TRACE_COMMANDS=1`: each engine command a script calls, with the
    // tick — a stalled one shows again every tick it waits.)
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let trace = *TRACE.get_or_init(|| std::env::var_os("ONI_TRACE_COMMANDS").is_some());
    if trace && !name.starts_with('@') {
        let shown: Vec<String> = args
            .iter()
            .map(|v| match v {
                Value::String(s) => format!("{:?}", s.to_string_lossy()),
                other => format!("{other:?}"),
            })
            .collect();
        eprintln!("cmd {} {name}({})", state.now, shown.join(", "));
    }
    output::dispatch(state, host, lua, name, args)
}

/// A command no area carries out: logged the first time, answered 0.
fn stubbed(state: &mut State, name: &str) -> mlua::Result<Reply> {
    let n = state.stubbed.entry(name.to_string()).or_insert(0);
    if *n == 0 {
        eprintln!("script: {name} (not carried out yet)");
    }
    *n += 1;
    Ok((Value::Integer(0), Value::Boolean(false), None, None))
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
    let id = match spawn_placed(host, &placed) {
        Ok(id) => id,
        Err(e) => {
            state.warn(format!("ai2_spawn {name}: {e}"));
            return None;
        }
    };
    state.scripts.insert(id, placed.scripts.clone());
    state.fired.remove(&id);
    eprintln!("script: spawned {} (fighter {id})", placed.name);
    placed
        .scripts
        .get("spawn")
        .map(|s| (s.clone(), placed.name.clone()))
}

/// Spawns placement `placed` as the game does (`ONrGameState_NewCharacter`):
/// its class a random costume, or upgraded on hard; its melee profile the
/// new class's default if it had its old class's; its goals; passive if
/// every AI is. Returns its fighter id.
pub fn spawn_placed(
    host: &mut dyn Host,
    placed: &crate::ai::level::Placed,
) -> Result<usize, String> {
    let world = host.world();
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
        eprintln!(
            "script: {} wears {class} (placed as {})",
            placed.name, placed.class
        );
    }
    let kind = host.kind(&class)?;
    let melee = if default_melee {
        kind.melee.clone()
    } else {
        placed.melee_id.and_then(|m| host.melee(m))
    };
    let world = host.world();
    // (A placement is the feet; spawning takes the pelvis: start a little
    // above and let it settle.)
    let id = world.spawn(&kind, placed.at + Vec3::Y * 10.0, placed.facing, false);
    world.give_goals(id, Some(placed), melee);
    if world.all_passive {
        world.set_passive(id, true);
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Script arguments read as BSL reads them: numbers from strings, a
    /// flag true only for a non-zero number (or `true`).
    #[test]
    fn arguments_read_as_bsl_reads_them() {
        let lua = mlua::Lua::new();
        let s = |t: &str| Value::String(lua.create_string(t).unwrap());
        assert_eq!(int(Some(&s(" 12 "))), Some(12));
        assert_eq!(int(Some(&Value::Number(3.9))), Some(3));
        assert_eq!(int(Some(&Value::Boolean(true))), Some(1));
        assert_eq!(int(Some(&s("twelve"))), None);
        assert_eq!(float(Some(&s("2.5"))), Some(2.5));
        assert!(truthy(Some(&s("1"))) && truthy(Some(&Value::Integer(-1))));
        assert!(!truthy(Some(&s("0"))) && !truthy(Some(&s("yes"))) && !truthy(None));
        assert_eq!(text(&Value::Integer(7)).as_deref(), Some("7"));
    }
}
