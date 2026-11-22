# Lua

The engine runs Lua in two places: **scenarios**, the headless test scripts
that drive the engine and assert, and **level scripts**, the original BSL
translated to Lua. Every function and command is listed in
`docs/lua-api.md`, generated from the source on each build.

## Scenarios

A scenario is a `.lua` file in `scenarios/` run against a level without a
window:

```sh
cargo run --release -p oni-view -- \
  ../generated/oni/levels/01/environment.glb \
  --character ../generated/oni/common/characters/konoko_body_high/model.glb \
  --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
  --char-pos 200,8,300 --script scenarios/<name>.lua [--faithful]
```

`scenarios/run_all.sh` runs them all (8 at a time), taking each one's level
and start from the command line in its header comment; `scripts/check.sh`
runs every scenario in both modes as part of the gate. A scenario passes
when it finishes and fails on any error — use `assert`. Each starts with a
header comment saying what it checks and the command line to run it.

Conventions:
- **Time** is in ticks (60 a second); `step(n)` advances the game `n`
  ticks. Nothing happens between steps.
- **Fighters** are ids: the player is 0, the rest in spawn order.
- **Places** are world units (x, y, z; y up); angles in the API are in
  degrees unless a function says otherwise.
- **Input** is held (`input`, `strafe`, `walk`, `hold_fire`…) until changed,
  or tapped for one tick (`punch`, `kick`, `jump`, `fire`…; leave a tick
  between taps).
- **Modes:** `faithful(true)` switches to the shipped bugs
  (`docs/deviations.md` §1), `faithful()` tells which is on; `difficulty()`
  is normal unless set.
- **Missions:** `mission_begin()` starts the level's own scripts; their
  dialogue and waits run on game ticks, so a scenario waits them out with
  `step`. Scenarios run with an empty save folder (`ONI_SAVE_DIR`).
- **Files:** a scenario that writes (screenshots) uses `temp_path(name)`, so
  the fixed and faithful passes don't share a file.

## Level scripts

The converter translates each level's BSL scripts (the Anniversary Edition's
vanilla package) once, into `levels/NN/scripts.lua`. The engine runs them as
coroutines, a tick at a time (`crates/oni-view/src/mission/`).

**The translated chunk** receives `V, F, C, G, W, T, EQ, NE, LT, GT, LE, GE,
ADD, SUB, SLEEP, FORK, ERR` (from `mission/prelude.lua`):
- `V` — the scripts' variables (declared up front); `F` — their functions;
  `C` — engine commands (`C.ai2_spawn("bob")`); `G` — engine variables
  (read and written through the engine).
- `W(name)` — a bare word: an engine variable, a value-returning command's
  result, else the word itself. A shell-style argument is always its literal
  spelling (`chr_delete ai_name` passes `"ai_name"`); C-style arguments and
  expressions read variables. Names are case-sensitive.
- `T(x)` — BSL truth (int or bool; a string is an error). `EQ`… `GE`, `ADD`,
  `SUB` coerce as BSL does: bools to ints, numeric strings to numbers, else
  string comparison. `sleep f60` is `sleep 60`; `5-3` reads only `5`.

**The scheduler:**
- `SLEEP(n)` yields `n` ticks. Calling a script function is synchronous:
  a sleep in it suspends the caller.
- `FORK(f, …)` runs the new task at once until its first sleep, then the
  caller carries on; an error in the fork aborts its parent.
- At most 32 tasks wait at once (the newest beyond that die). Tasks resume
  in creation order; one started this tick runs this tick. A task that
  errors is dropped.
- A command returns `(result, wait, now, arg)`: a number `wait` sleeps that
  many ticks; a stalled command is called again each tick until it stops
  stalling. Most `_block` commands wait only for an earlier action to end
  before starting theirs; `chr_wait_*`, `cm_wait` and `playback_block` wait
  for completion; `sound_dialog_play_block` waits for the line before, not
  its own. `now, arg` runs a script at once (a spawn script inside
  `ai2_spawn`).
- Commands the engine doesn't handle are logged once and do nothing (the
  shipped scripts' own typos included, as in the original).

**Level start:** all the level's files share one namespace. The player and
every character not flagged "not initially present" are created (their
spawn scripts run on the first tick), then `main` runs, with `save_point` 0
on a fresh start (else the save point begun from). `difficulty` is
0, 1 or 2.

**Characters:** placed characters are found by name (their placement name,
the player's too); `chr_create`d ones by script id from `setups.json`; the
player is also 0. Each event script runs once per life, given the
character's name: spawn, hurt (while not invincible), defeated, die, combat,
alarm, out of ammo ("no path" never fires). Patrol `PlayScript` waypoints
run their script and the patroller waits; a timer runs its script when it
runs out.

**Trigger volumes:** a volume is a box from its corner along its rotated
axes. A character counts when the segment from its feet to its top crosses
it and its team is in the volume's mask (a child uses its parent's).
*Entry* fires on getting inside (a character tracks at most 4 volumes),
*inside* every tick, *exit* on leaving; each can be disabled or set to fire
once. Disabling forgets who was inside (no exit), so re-enabling fires entry
again.

**Our fixes** to the shipped scripts (`docs/deviations.md` §2) live in
`mission/fixes/NN.lua`, run after the level's chunk unless `--faithful`.
`FIX(id, name, make)` replaces script function `name` with `make(original)`
(an error if the scripts no longer have it). `CHR_AT(who)` / `PLAYER_AT()`
give feet positions, for fixes only.

**Tools:**
- `ONI_TRACE_COMMANDS=1` prints each command with its tick (a stalled one
  repeats each tick).
- `scripting/shakedown.sh [ticks]` runs every level from its start and its
  save points; `--tour` teleports Konoko to every trigger volume and console
  (stops from `scripting/tour_stops.py`).
- `scripting/script_commands.py` lists the commands the scripts use that the
  engine doesn't handle.
