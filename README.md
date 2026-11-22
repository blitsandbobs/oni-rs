# oni-rs

A ground-up, portable **Rust** reimplementation of [Oni](https://en.wikipedia.org/wiki/Oni_%28video_game%29),
built for long-term preservation.

The original 2000/2001 engine (`../original/source`) only builds with dead
toolchains (Visual C++ 6 + DirectX/Glide, CodeWarrior + RAVE). This project
reimplements it in clean Rust, subsystem by subsystem, using the original C
as the authoritative spec and the real game data as the test oracle. A
converter turns the game data into an open asset tree once; the engine plays
that tree.

Reference data lives **outside** the workspace in `../original/` and
`../generated/` (copyrighted assets — not committed).

## Docs

- `docs/architecture.md` — the crates and the engine's map.
- `docs/lua.md` — the Lua scenario host and the level-script runtime;
  `docs/lua-api.md` — every function and command (generated on build).
- `docs/deviations.md` — where and why we differ from the original: the
  shipped bugs we fix (`--faithful` keeps them), the level-script fixes, the
  port's own choices.
- The converted files' formats: `oni_schema::files`
  (`crates/oni-schema/src/files.rs`).
- How anything *works* — the original source is the reference.

## Playing the game

Convert the original game data once (a few seconds), then run the game; with
no arguments it opens Oni's main menu:

```sh
cargo run --release -p oni-convert -- ../original/gamedata ../generated/oni
cargo run --release -p oni-view
```

Controls are the shipped defaults (W/S/A/D, mouse, Space, Shift, Ctrl, Q,
Tab, R, F1 for the pause screen). `--faithful` plays with the original's
known bugs (`docs/deviations.md`).

### Starting any level

```sh
cargo run --release -p oni-view -- --unlock-all
```

unlocks every level and puts each one's save points in the main menu's Load
Game list (from the save points its scripts keep; one begun this way starts
there with nothing carried over, the player as the level's scripts set her
up). Saved games already kept stay as they are. The saved game lives in
`~/.oni-rs/continues.json` (or `$ONI_SAVE_DIR`).

### Cheat codes

The original's cheats work as they did: open the pause screen (F1) and type
a code; its message replaces the pause screen's hint. As shipped they only
work once the game has been won; to have them from the start, run with
`ONI_CHEATS=1`:

```sh
ONI_CHEATS=1 cargo run --release -p oni-view
```

| Code | Does |
|---|---|
| `liveforever` | You take no damage (toggle) |
| `touchofdeath` | Your blows get through blocks and kill (toggle) |
| `canttouchthis` | You can't be knocked about (toggle) |
| `fatloot` | 6 clips, 6 cells, 6 hypos |
| `winlevel` / `loselevel` | Win / lose the level at once |
| `superammo` | Your guns never run dry (toggle) |
| `elderrune` | Health slowly comes back (toggle) |
| `moonshadow` | The phase cloak: invisibility that never runs out (toggle) |
| `chenille` | Daodan power: you glow, with a boss shield (toggle) |
| `carousel` | Slow motion (toggle) |
| `forcefield` | A full shield belt *(the port's own, not the original's)* |

Letters typed are kept (the last 20); a code they end with is carried out.
Toggles switch off when a new level begins (the original keeps regeneration
across levels). As shipped the cheats unlock once the game has been won; the
port keeps that with the saved game.

## Build & test

```sh
cargo build
cargo test            # includes the OniSplit golden test (skips if data absent)
cargo clippy --all-targets

# Inspect a file:
cargo run -p oni-dat --example onidump -- ../original/gamedata/level0_Final.dat
cargo run -p oni-dat --example onidump -- <file.dat> --tags --names
```

`level0_Final.dat` parses to **30527 instances** (11032 named, 64 templates);
**10988** are named + non-placeholder, matching OniSplit's export exactly, with
a per-tag histogram identical across all 44 exported tags.

### Regenerating the OniSplit oracle

Requires Mono (`brew install mono`) and `OniSplit.exe` from the AE `Tools/`.

```sh
OS="../original/tools/.../Tools/OniSplit.exe"
DAT="../original/gamedata/level0_Final.dat"
mono "$OS" -export /tmp/oracle_export "$DAT"
find /tmp/oracle_export -name '*.oni' -exec basename {} .oni \; \
  | cut -c1-4 | sort | uniq -c | sort -rn \
  | sed -E 's/^ *([0-9]+) (.*)$/\1 \2/' \
  > ../generated/oracle/level0_export_tag_histogram.txt
```

The test reads `../original/gamedata/level0_Final.dat` and
`../generated/oracle/level0_export_tag_histogram.txt` by default; override with
`ONI_DAT_LEVEL0` and `ONI_ORACLE_HISTOGRAM`.
