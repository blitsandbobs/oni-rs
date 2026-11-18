# BSL → Lua port plan (groundwork)

**Status (2026-11): translation and runtime working.** The converter translates every
level's scripts (`crates/oni-convert/src/bsl.rs` → `levels/NN/scripts.lua`, all 14
levels, no untranslated lines); the engine runs them (`crates/oni-view/src/mission/`):
the level start, `main`, `sleep` / `fork`, character and trigger-volume events,
patrol scripts, and the gameplay commands (spawning, paths, passivity, attack orders,
health, trigger volumes, objectives / messages, character waits, consoles and alarms,
doors, films). Dialogue prints its subtitle (no audio yet). Camera, sound, particles,
cutscenes, turrets and the like are logged no-ops so far (laser triggers are real). This
document originally laid out *what to build*
for the gameplay scripting layer (E4+), derived from [`BSL.md`](BSL.md). The dev/test
Lua driver that exists today (`crates/oni-view/src/script.rs`,
[[oni-config-input-facing]]) is a *separate, movement-only* harness; the gameplay port
below is the thing that replaces BSL mission logic.

## Decision recap

Lua, not a BSL interpreter ([[oni-scripting-lua]], `ENGINE.md`). Oni is frozen, so
translating the shipped `.bsl` to Lua **once** is a migration, not a maintained fork,
and Lua is universally readable. We embed Lua (`mlua`, already a dep) and expose an
Oni host API; the `.bsl` files are translated to `.lua` with the same semantics.

## The two mechanisms any port MUST replicate

1. **Entity references by name or id.** Every command takes `[ai_name | script_id]`
   or a typed `flag_id`/`door_id`/`obj_id`/`trig_id`/`turret_id`. The host keeps
   name→entity and id→entity registries; Lua passes strings/ints, never handles.
2. **Blocking / cooperative scheduling.** BSL commands return
   `ticksTillCompletion` + `stall`; `_block`/`wait_*`/`cm_wait`/`sleep`/`schedule`
   suspend the script until an engine action finishes. In Lua this maps cleanly to
   **coroutines**: a blocking host call `coroutine.yield`s a "resume-when" condition;
   the host's per-tick scheduler resumes the coroutine when the engine signals done.
   `schedule`/`fork` become scheduled/independent coroutines; `sleep(n)` yields for n
   ticks. Preserve (or deliberately lift) the 32-task / 6-deep limits.

## Language mapping (BSL → Lua)

| BSL | Lua | Note |
|---|---|---|
| `func void f(string x) { … }` | `function f(x) … end` | Lua is dynamically typed — drop the type annotations (or keep as asserts). |
| `name a b c` / `name(a,b,c)` | `ns.name(a, b, c)` | Namespace by family (`chr.`, `ai2.`, …) — see `oni_api.lua`. |
| `if c eq 0` | `if c == 0 then` | `eq`/`ne`→`==`/`~=`; `and`/`or`/`!`→`and`/`or`/`not`. |
| `+`/`-` only | full Lua arithmetic | BSL had no `*`/`/`; Lua is a superset — fine. |
| `sleep 30` | `wait(30)` | yields the coroutine 30 ticks. |
| `schedule f every 60 repeat 0` | `schedule(f, 60, 0)` | host registers a periodic coroutine. |
| `fork f` | `fork(f)` | host starts an independent coroutine. |
| `chr_animate_block …` | `chr.animate(…)` (blocking) | the host call yields until the anim ends. |
| engine global var | a field on a host table | read/write proxied to engine state. |
| event handler `func void ondeath(string self)` | same func, registered as a handler | host calls it with the entity name. |

A small **translator** can convert most `.bsl` lines mechanically (comment `#`→`--`,
operators, call-style normalisation, `func`→`function`). The event-handler wiring and
blocking semantics are the parts needing care, not the syntax.

## Build order (cheapest → deepest)

Each layer needs its engine subsystem to exist first. Stub cosmetic/debug commands as
no-ops from day one so real mission scripts load without erroring.

1. **Mission/UI orchestration** — `objective_*`, `message`, `win`/`lose`, `timer_*`,
   `save_*`, cutscene markers, `fade_*`, `letterbox`. *Pure orchestration*: set a flag /
   print text / change a mode. Build first; gives playable mission flow.
2. **Objects / triggers** — `door_*`, `turret_*`, `obj_*`, `console_*`, `flag_*`,
   `env_show/shade/texswap`, `powerup_spawn`/`weapon_spawn`. Needs the object-instance
   system (id→record) + a few toggles. Light.
3. **Camera + audio** — `cm_*`/`cinematic_*` (camera state machine + blocking waits),
   `sound_*` (backend + name resolution; `sound_dialog_play_block` uses the stall path).
   Medium; needed for cutscenes.
4. **Characters** — `chr_*` (create/teleport/animate/playback/health/weapon/vocalize/
   wait_*). Deep: depends on the character SM, animation graph, inventory, and the stall
   protocol. Port after those subsystems exist.
5. **AI** — `ai2_*`, patrol scripts. Deepest: needs the whole AI2 system (goals, jobs,
   pathfinding, perception, combat). Highest effort; mission scripts lean on it heavily.

## What the engine must expose (host API surface)

The Lua side is thin; the work is host functions backed by engine subsystems:

- **Registries:** name/id → character, flag, door, object, trigger, turret, console.
- **Scheduler:** per-tick coroutine pump (resume on tick/condition), `wait`, `schedule`,
  `fork`, and the blocking-call convention (host call yields a "done when" predicate).
- **Event dispatch:** level `main`; per-character `spawn/combat/die/alarm/hurt/defeated/
  outofammo/nopath`; trigger `entry/inside/exit`; patrol `PlayScript`; timer expiry.
- **Per family:** the command set in [`BSL.md`](BSL.md), each backed by its subsystem.

`oni_api.lua` in this directory is a **skeleton** of that surface — namespaced tables
with stubbed functions and doc comments, so the shape is pinned down before any
subsystem is wired. It is not loaded by the engine yet.

## Not in scope yet
- The translator tool (`.bsl` → `.lua`).
- Validating translated scripts against original BSL execution order / semantics
  (will need the real `GameDataFolder/IGMD/*.bsl`, absent from `original/` here).
- Save-game serialization of script-visible state.
