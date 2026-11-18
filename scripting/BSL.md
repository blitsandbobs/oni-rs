# BSL — Bungie Scripting Language (reference)

Oni's mission logic is written in **BSL**, a small interpreted language built into
the Bungie framework. This document maps the language and its command surface from
the original C source so a faithful Lua port ([`PORT.md`](PORT.md)) knows *what BSL
has* and *what the engine must provide*. All paths are under
`original/source/`; line anchors are for verification (per CLAUDE.md §1).

> BSL is **mission/level orchestration layered on the engine**, not the simulation
> itself (confirmed in [[oni-scripting-lua]] / `ENGINE.md`). The language core is
> small; the hard part is the several-hundred-command engine surface it drives.

## Two layers

- **Language core / VM** — `BungieFrameWork/BFW_Source/BFW_ScriptLang/` (lexer,
  recursive-state-machine parser/interpreter, symbol database, cooperative
  scheduler). Public API: `BFW_Headers/BFW_ScriptLang.h`.
- **Oni command surface** — commands + variables registered from ~28 files under
  `OniProj/OniGameSource/`. Loader/glue: `Oni_Script.c`.

Author: Brent H. Pease; a "simple interpreted language to describe game logic"
that subsystems extend by registering commands/variables (`BFW_ScriptLang.h:9-55`).

## The language

### Lexical (`BFW_ScriptLang_Token.c`)
- **Comments:** `#` to end of line (`:534-556`). No block comments.
- **Lines are significant.** Tokens carry `SLcTokenFlag_PrecedesNewline`; a newline
  terminates a statement / old-style arg list and makes the trailing `;` optional.
  Separators: `;` and `{ }` blocks.
- **Strings:** double-quoted, single-line, max length 64 (`SLcScript_String_MaxLength`).
  Identifiers max 32 (`SLcScript_MaxNameLength`).
- **Numbers:** integer, or float if it contains `.`; leading `-` allowed.
- Limits: 8192 tokens/file (`SLcMaxTokens`).

### Types (`SLtType`, `BFW_ScriptLang.h:76-85`)
`int32`, `float`, `string`, `bool`, `void`. Value is a tagged union
`SLtValue { i; f; str; b; }`. Implicit promotions (`Database.c:1003-1048`):
bool→int, int→float, int→bool. **No automatic promotion to string.** `true`/`false`
are bool literals.

### Variables
- `var` keyword or typed `int`/`float`/`bool`/`string`. **No `$`-prefix** — plain
  identifiers. Lexically scoped, up to 8 nested levels. Lookup walks the current
  function's scope → globals → permanent (engine) symbols (`Database.c:40-70`).
- **Engine-exposed globals:** C variables registered read-only or read/write via
  `SLrGlobalVariable_Register_*`; a write can fire a notification callback.

### Control flow (`BFW_ScriptLang_Parse.c`, parse states `:28-69`)
- `func` — define a subroutine (typed params + return type); `return` returns a value.
- `if` / `else`.
- `for`, `repeat`, `every`, `iterate … using/over` — loops / iteration.
- `sleep <ticks>` — suspend the running script N game ticks (cooperative).
- `schedule <func> every <delta> [repeat N] [at <time>]` — periodic/deferred.
- `fork` — launch a function as an independent task.
- `{ … }` block.

### Expressions & operators (`Private.h:31-44`)
`=` (assign), `+`, `-`, `and`, `or`, `!`, and comparisons `eq` `ne` `<` `>` `<=` `>=`.
**No `*`, `/`, `%`** — arithmetic is add/subtract only. Tiny operator/paren stack
(depth 4).

### Call syntax — TWO forms (`Parse.c:432-464`, `1651-1742`)
- **C-style:** `name(a, b, c)` — identifier immediately followed by `(`.
- **Shell-style:** `name a b c` — space-separated args, newline-terminated. This is
  what the console and most `.bsl` lines use: `chr_teleport my_guy 12`.

Return-value commands can appear in expressions: `if did_kill_griffen …`.

### Storage & loading (`Oni_Script.c`)
- **Plain `.bsl` text files on disk — NOT compiled into a `.dat` tag.**
  `ONiScript_LoadDirectory` (`:40-82`) reads every `*.bsl` and calls
  `SLrScript_Database_Add(name, text)`.
- Per level: `GameDataFolder/IGMD/<LevelName>/*.bsl` (`ONiScript_Reload :99-117`).
- `Database_Add` parses in *add-to-database* mode (registers `func` defs + globals,
  no execution). Live reload via the `script_reload` command.
- The shipped scripts come from the Anniversary Edition archive, extracted to
  `../original/bsl/…/AEInstaller/packages/05100Vanilla_BSL/bsl/common/<Level>/`
  (the vanilla set; AE's own `GameDataFolder/IGMD` patches compound and
  manplant). 121 files, 14 levels; the folder is named by the level's `ONLV`.
  The example below is illustrative, not copied from them.

### Example (reconstructed)
```
# engine calls func "main" once at level begin
func void main(void)
{
    cm_reset
    objective_new 1
    sound_music_start "neutral"
    schedule check_mission every 60 repeat 0   # every 60 ticks, forever
}

func void spawn_guard(string ai_name)
{
    chr_teleport(ai_name, 14)      # C-style: ai_name is the variable
    ai2_active(ai_name)
    chr_giveweapon ai_name "w2_tap"
}

func void check_mission(string self)
{
    if (trigvolume_count(3) eq 0)  # parentheses required
        win
}
```

## Execution / trigger model

### Entry points
- **Level load:** engine runs `func main` once (`SLrScript_ExecuteOnce("main", …)`,
  `Oni_Level.c:690`), after loading the level's `.bsl` (`ONrScript_LevelBegin`).
- **Console:** the debug console *is* the BSL interpreter — every console line runs
  as a one-line script (`COrConsole_AddHook("", …, SLiScript_ConsoleHook)`,
  `BFW_ScriptLang.c:496`).
- **Character event handlers** (`AItScriptTable`, `Oni_AI_Script.h:51-62`) — named
  funcs fired once per character: `spawn_*`, `combat_*`, `die_*`, `alarm_*`, `hurt_*`,
  `defeated_*`, `outofammo_*`, `nopath_*`. Each is called with the character's
  `player_name` string (so a handler is `func void ondeath(string self)`).
- **Trigger volumes** (`Oni_Object.h:1181-1199`) — `entry_script`, `inside_script`,
  `exit_script` fields fired on enter/inside/exit.
- **Patrol waypoints** — type `PlayScript` runs `patrolscript%04d` funcs.
- **Timers** — a countdown holds a `script` name run on expiry (`timer_start`/`_stop`).
- **Deferred:** handlers are queued (`ONrCharacter_Defer_Script`) and flushed outside
  the physics update.

### Entity references
Always **by name (string) or numeric id**, never a pointer. Characters by
`[ai_name:string | script_id:int]`; flags/doors/objects/turrets/trigger-volumes/
consoles by integer id. This `[name|id]` / `flag_id` convention is consistent across
the whole command surface.

### Scheduler (cooperative, per-tick) — `BFW_ScriptLang_Scheduler.c`
- 32 task slots (`SLcScheduler_MaxTasks`). `SLrScheduler_Update(gameTime)` runs each
  heartbeat (Oni = 60 ticks/s), resuming sleeping scripts whose `wakeTime` arrived.
- `schedule … every <delta> [repeat N]`: `delta=0,N=1` = run now; else periodic. A
  scheduled (non-immediate) func **must return void**. `N=0` = every tick forever.
- `sleep <ticks>`: marks the running context sleeping until `now + ticks`.
- **Blocking commands:** every command returns `*outTicksTillCompletion` + `*outStall`
  (`SLtEngineCommand`, `BFW_ScriptLang.h:129-136`). A `_block`/`wait_*`/`cm_wait`
  command can **stall its calling script** until the in-engine action finishes; the
  scheduler re-invokes it each tick until `!stall`. This is the single most important
  cross-cutting mechanism for a port (see `PORT.md`).
- `fork` launches an independent task; the engine can track/cancel it via a stored
  `SLtContext**`.

### State persistence
- Engine-registered globals are live C variables — the *engine* owns and saves that
  state (not the script). Pure script `var`s are wiped on level change / database
  reset.
- Mission progress is encoded as engine state + "flag"-style commands (`killed_griffen`,
  `console_already_used`, objective state) and the one-shot `*Called` handler bits
  (saved as character state). There is no generic script-variable serialization.

## Command surface (catalogue by family)

Registered via `SLrScript_Command_Register_{Void,ReturnType}` or bulk tables
(`SLtRegisterVoidFunctionTable`). AI2 uses a 6-field table with `name2`/`name3`
**aliases** and a parallel `_block` variant per command. The authoritative
in-engine dump is the `dump_docs` command → `script_commands.txt`
(`Database.c:1348`). Parameter spec mini-language: `name:type`, `[...]` groups,
`{a|b}` enums, `|` either/or, `+` repeating (`BFW_ScriptLang.h:150-167`).

- **`chr_*` — characters** (largest family; `ONI_Character.c`, `Oni_AI2_Script.c`).
  Lifecycle (`chr_create`/`chr_delete`/`chr_teleport`/`chr_who`/`chr_active`),
  animation (`chr_animate`(+`_block`)/`chr_envanim`/`chr_playback`/`chr_wait_animation`/
  `_animstate`/`_animtype`), combat+state (`chr_health`/`chr_set_health`/`chr_invincible`/
  `chr_super`/`chr_pain`/`chr_poison`/`chr_freeze`/`chr_giveweapon`/`chr_disarm`/
  `chr_boss_shield`), behaviour (`chr_facetoflag`/`chr_focus`/`chr_dontaim`/`chr_holdkey`/
  `chr_talk`/`chr_vocalize`/`chr_neutral`/`chr_changeteam`/`chr_is_player`→bool/
  `chr_teleport`/`chr_location`), cosmetic/debug (`chr_big_head`/`chr_mini_me`/`chr_shadow`/
  `chr_lod`/`chr_draw_*`/`chr_debug_*`/`chr_nocollision`).
- **`ai2_*` — AI** (`Oni_AI2_Script.c`). State (`ai2_active`/`_inactive`/`_passive`/
  `_reset`/`_idle`/`_panic`/`_neutralbehavior`/`_takecontrol`), perception
  (`ai2_blind`/`_deaf`/`_makeaware`/`_ignore_player`/`_lookatchar`/`_setalert`), orders
  (`ai2_attack`/`_comehere`/`_followme`/`_movetoflag`/`_dopath`/`_doalarm`/`_forget`/
  `_kill`/`_spawn`/`_boss_battle`/`_barabbas_*`), skill tuning (`ai2_skill_*`), debug
  (`ai2_show*`/`ai2_report*`). Return-value: `did_kill_griffen`, `trigvolume_count`,
  `difficulty` (int) (`:4503-4508`).
- **`ai_*`** (older; `Oni_AI_Script.c`): `dprint`/`dmsg`; implements the `AITR`
  script-trigger template.
- **`cm_*` / `camera*` / `cinematic_*` — camera & cutscene** (`Oni_Camera.c`,
  `Oni_Cinematics.c`). `cm_anim`/`cm_orbit`/`cm_interpolate`(+`_block`)/`cm_detach`/
  `cm_reset`/`cm_wait`/`cm_distance`/`cm_height`; `cinematic_start`/`_stop`/`begin_cutscene`/
  `end_cutscene`/`cutscene_sync`/`letterbox`/`fade_in`/`fade_out`.
- **`door_*`** (`OT_Door.c`): `door_open`/`_close`/`_lock`/`_unlock`/`_jam`/`_unjam`/
  `_fail`/`_success`/`_ignore_locks`.
- **`env_*` / `obj_*` / flags / gunk** (`Oni_Object/*`, `OT_Flags.c`): `env_anim`(+`_block`)/
  `env_setanim`/`env_show`/`env_shade`/`env_texswap`/`env_broken`; `obj_create`/`_kill`/
  `_show`/`_hide`/`_shade`; `flag_new_id`/`flag_name_distance`; glass controls.
- **`objective_*` / UI** (`Oni_InGameUI.c`): `objective_new`/`_set`/`_complete`/`_prompt`;
  `message`/`message_remove`/`splash_screen`.
- **`sound_*`** (`Oni_Sound2.c`): `sound_music_start`/`_stop`/`_volume`, `sound_ambient_*`,
  `sound_dialog_play`(+`_block`/`_interrupt`), `sound_impulse_play`.
- **`particle` / `fx_*` / `p3_*`** (`Oni_Particle3.c`): `particle`/`particle_temp_*`/
  `fx_laser_width`/`p3_*_breakable`.
- **`weapon_*` / `wp_*` / powerups** (`Oni_Weapon.c`): `weapon_spawn`/`weapon_reset`/
  `powerup_spawn`/`powerup_reset`; tuning `wp_*`.
- **`turret_*`** (`OT_Turret.c`): `turret_activate`/`_deactivate`/`_reset`.
- **`console_*`** (consoles/terminals): `console_activate`/`_deactivate`/`_reset`
  (`console_locked` / `console_already_used` are sound names, not commands).
- **Mission flow / game-state** (`Oni_GameState.c`): `win`/`lose`, `save_game`/
  `restore_game`/`save_point`, `timer_start`/`_stop`, `slowmo`/`fast_mode`, `input`/
  `lock_keys`, player cheats (`invincible`/`omnipotent`), `gs_*` / `gs_show_*`.
- **Rendering / sky / debug:** `sky_show_*`, `m3_*`, `am_*`, `anim_flags`/`anim_states`/
  `anim_types`; `dprint`/`dmsg`/`dump_docs`; the large `*_debug*`/`*_show*` set.

> `func_` is **not** a prefix — `func` is the keyword for user subroutines, which are
> what the engine invokes as event handlers.

## Runtime semantics (traced from the interpreter and the game)

What the port reproduces, in our words (anchors in the original's
`BFW_ScriptLang_*.c`, `Oni_Script.c`, `Oni_AI2_Script.c`, `ONI_Character.c`,
`OT_TriggerVolume.c`).

**Language actually used.** The shipped scripts use `var` / `func`, `if`
(always with parentheses, never `else`), `sleep`, `fork`, assignments and
calls — no `return`, `schedule`, `iterate` or loops (`for` / `repeat` /
`every` have no loop grammar at all).

**Words.** A shell-style argument (`cmd a b`) is always the word's own
spelling, even if it names a variable (so `chr_delete ai_name` passes the text
"ai_name"); C-style arguments and expressions read variables, and an
undeclared word is a string of its own name. A function's name ending its line
is a call; alone mid-expression it is an error. Names are case-sensitive.

**Numbers.** A number may start with `-` (no space) or `.`; `f` right before a
digit is dropped (`sleep f60` is `sleep 60`); only the valid prefix of
`5-3` counts (5). `x - 1` needs the spaces.

**Types.** Values are int / float / bool / string; `if` needs an int or bool
(a string aborts the script); comparisons and `+ -` take the left operand's
type without promoting; strings compare by identity (literal against literal
works, an engine-given name never equals a literal). Missing command
arguments get defaults, extra ones are dropped, a wrong type aborts.

**Calls and tasks.** Calling a script function is synchronous (a sleep in the
callee suspends the caller too; 6 frames deep at most). `fork f` starts an
independent task that runs at once until it first sleeps, then the parent
carries on. At most 32 tasks can be waiting; one that would wait beyond that
dies. A task that errors is dropped and the rest of that tick's tasks wait.
A blocking ("stalled") command is called again every tick until it stops
stalling; most `_block` commands only wait for an earlier action to finish
before starting theirs (`chr_wait_*`, `cm_wait`, `playback_block` wait for
completion).

**Level start.** The level's files are loaded (one global namespace; a
duplicate function aborts the rest of the load), the player (the `CHAR` with
the player flag) and every character not flagged "not initially present" are
created — their spawn scripts run on the first tick — then `main` runs.
`save_point` is 0 on a fresh start.

**Characters and events.** A number names a character by script id (only the
player has one, 0); a name, the living character of that name. `ai2_spawn`
creates the first `CHAR` of that name not yet spawned (unless `force`, or it
may spawn many times), with its job and alerts, and runs its spawn script at
once. Event scripts, each once per life and with the character's name: spawn,
hurt (damage while not invincible), defeated and die (on death), combat (on
entering combat), alarm, out of ammo; "no path" never fires.

**Trigger volumes.** A box from its position (a corner) along its rotated axes
by its size; a character counts when the vertical segment from its feet to its
top crosses it, if its team is in the mask (the parent's, for a child volume)
and — for player-only volumes — it is the player. Entry fires when one starts
being inside (each character tracks at most 4 volumes), inside every tick,
exit on leaving; each type can be disabled, or fire once and then disable
itself; a child also fires its parent's scripts. Disabling a volume forgets
who was inside (no exit), so enabling it again fires entry anew.

**Consoles.** A console (class template in level 0: where its action marker is)
has an id, flags (initially active, punch animation, alarm console) and an event
list. Action, by the player, picks the marker nearest the pelvis; a user must be
within 8 of its height, from 2 ahead to 10 behind along its axis and 8 aside
(twice that for an AI), facing it within 70° (×1.5 for an AI). It then stops,
walks onto the marker (0.25 a tick, up to 3 s), plays the console (or console
punch) animation and on its action frame uses the console: inactive → locked,
used → already used, else it is marked used and runs its events — script (the
user's name), console / turret / trigger on or off, alarm (every AI answering
that alarm group, not ignoring the user, learns where the user is and runs its
alarm script), door lock / unlock. `console_activate` / `_deactivate` /
`_reset`; `lock_keys` with no argument locks every key, `lock_keys keys_X`
*enables* a group again; `input 0` drops all the player's input.

**Not reproduced** (no shipped script relies on them): operator precedence
slips (each operator reduces at most one pending one), no short-circuit,
assignments taking effect in a skipped branch after a local `var`, `return`
in a skipped branch, a trailing `#` comment joining the next line, string
`+`, the 6-frame call limit, the 32 KB handler-name buffer.

## Open questions
1. The per-command parameter specs (types, enums) are applied loosely: our
   commands read what they need and ignore the rest.
2. Save games (`save_game` / `restore_game`) aren't modelled; `save_point`
   stays 0.
