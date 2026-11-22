# Architecture

oni-rs is two programs: a one-time **converter** that turns the original
game data into an open asset tree, and an **engine** that plays that tree.
Behaviour is ported from the original C source (`../original/source`), which
stays the reference for *how* anything works; this page is the map of
*where* it lives here. The crates are listed in `CLAUDE.md`.

## The engine (`crates/oni-view/src`)

- **Simulation** — `world.rs` and `world/` hold `World`: the fighters, the
  level's objects and its subsystems (`doors.rs`, `glass.rs`, `lasers.rs`,
  `world/turrets.rs`, `world/corpses.rs`, `world/cutscene.rs` …).
  `World::step` is one 60 Hz tick. `fighter.rs` is a character;
  `anim/` its animation state machine and moves; `chrphys.rs` its collision
  against the level (`quads.rs`, the level's collision quads);
  `objphys.rs` loose objects. `ai/` is the AI (goals, perception,
  pathfinding in `nav/`, melee and ranged combat, bosses). `p3/` is the
  particle system (classes, emitters, actions). The simulation never names
  the renderer or wgpu.
- **Presentation** — `draw.rs` is the plain data a frame is drawn from;
  `world/draws.rs` builds it from `World` (read-only); `frame.rs` hands it
  to `render.rs` (wgpu). `hud/` is the HUD, text consoles, pause screen and
  menus; `audio/` the sound engine (mixer, ambients, impulses, occlusion).
- **Front-ends** — `app.rs` (the window: play, the main menu, movies) and
  `script.rs` (the headless scenario host). Both open a level with
  `opening.rs`, build frames with `frame.rs` and tick with
  `mission::game_tick`, so a scenario runs what the game runs.
- **Level scripts** — `mission/`: the original BSL scripts, translated to
  Lua by the converter, run as coroutines against the engine commands in
  `mission/commands.rs` (`docs/lua.md`).
- **Fidelity switch** — `quirks.rs`: the original's bugs, fixed by default
  and kept under `--faithful` (`docs/deviations.md`).

## Why it is built this way

- **A thin engine on wgpu**, not a general game engine: the original's
  look and feel come from its own rules (its tick, its animation-driven
  movement, its draw order), which are easiest to keep exact without a
  framework's in the way.
- **Ported collision, no physics engine**: characters move by their
  animations' root motion and collide by the original's sphere-against-quad
  rules (`chrphys.rs`); a general physics engine gave different contacts
  and was removed.
- **A fixed 60 Hz tick**, as the original: the game clock advances in whole
  ticks (at most a few a frame), and every timer in the data counts ticks.
- **Converted once, typed on both sides**: the engine never reads the
  original's binary formats; the converter's output is the only interface,
  and `oni_schema::files` pins it down.

## Checks

`scripts/check.sh` is the gate: clippy, the unit tests, and every scenario
in `scenarios/` in both the fixed and the `--faithful` mode.
`scripts/compare_trees.py` checks that a converter change leaves the
converted data unchanged.
