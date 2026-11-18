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
- **Note:** no `.bsl` files ship in `original/` here (`find … *.bsl` → 0). Real
  authored examples must come from the shipped `GameDataFolder` or community dumps —
  the snippets below are reconstructed from param-specs, not copied from real scripts.

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
    chr_teleport ai_name 14        # shell-style
    ai2_active(ai_name)            # C-style
    chr_giveweapon ai_name "w2_tap"
}

func void check_mission(string self)
{
    if trigvolume_count 3 eq 0     # return-value command in a condition
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
- **`console_*`** (consoles/terminals): `console_activate`/`_deactivate`/`_locked`/
  `_already_used`/`killed_griffen`.
- **Mission flow / game-state** (`Oni_GameState.c`): `win`/`lose`, `save_game`/
  `restore_game`/`save_point`, `timer_start`/`_stop`, `slowmo`/`fast_mode`, `input`/
  `lock_keys`, player cheats (`invincible`/`omnipotent`), `gs_*` / `gs_show_*`.
- **Rendering / sky / debug:** `sky_show_*`, `m3_*`, `am_*`, `anim_flags`/`anim_states`/
  `anim_types`; `dprint`/`dmsg`/`dump_docs`; the large `*_debug*`/`*_show*` set.

> `func_` is **not** a prefix — `func` is the keyword for user subroutines, which are
> what the engine invokes as event handlers.

## Open questions (verify before relying on these)
1. No `.bsl` ships here — confirm authored style from the real `GameDataFolder/IGMD`.
2. `iterate`/`using`/`over` + iterator registration: mechanism exists in the core,
   no registrations found in OniGameSource — confirm Oni actually uses it.
3. Exact grammar of `for`/`repeat`/`schedule … at … repeat` not fully traced
   (`Parse.c:1200-1500`).
4. `fork` vs `schedule` concurrency/return semantics not fully traced.
5. Full command count is approximate — ground truth is runtime `dump_docs`.
6. Save-game: exactly which script-visible state serializes.
7. Scheduler limits (32 tasks, 6-deep call nesting) — preserve or lift in a port.
