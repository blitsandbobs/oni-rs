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

Bring your own Oni: the Anniversary Edition's `.7z` (or its extracted
folder). Run the game with nothing installed and its launcher asks for it
(the system's own file dialog), installs it with a progress bar — a few
seconds — and opens the main menu. Or install from the command line:

```sh
cargo run --release -p oni -- --install "path/to/Oni - Anniversary Edition.7z"
cargo run --release -p oni
```

The install goes to the platform's data folder — `~/Library/Application
Support/Oni` (macOS), `$XDG_DATA_HOME/oni` (Linux), `%LOCALAPPDATA%\Oni`
(Windows) — with the game data it was made from, so a new version converts
it again by itself. `ONI_ROOT` points the game at another converted tree;
developers convert straight into `../generated/oni`, which the game finds
too:

```sh
cargo run --release -p oni-convert -- ../original/gamedata ../generated/oni
```

Controls are the shipped defaults (W/S/A/D, mouse, Space, Shift, Ctrl, Q,
Tab, R, F1 for the pause screen). `--faithful` plays with the original's
known bugs (`docs/deviations.md`).

The game opens in a window (its size kept between runs) or fullscreen at
the display's native resolution; Alt+Enter switches between them, as do
`--fullscreen`, `--windowed` and `--window 2560x1440` on the command line.
Window sizes are the desktop's own (points on macOS, scaled pixels with
display scaling on Windows and Linux); the picture uses every physical
pixel. The HUD and menus scale with the window — 1.5× at 1080, 2× at
1440, 3× at 2160 of those units, on top of the display's own scaling;
`uiScale` in `settings.json` fixes it (1: as shipped on an unscaled
display).

### Starting any level

```sh
cargo run --release -p oni -- --unlock-all
```

unlocks every level and puts each one's save points in the main menu's Load
Game list (from the save points its scripts keep; one begun this way starts
there with nothing carried over, the player as the level's scripts set her
up). Saved games already kept stay as they are. The saved game lives in
the platform's data folder (below), the options in its config folder (or
both in `$ONI_SAVE_DIR`).

### Cheat codes

The original's cheats work as they did: open the pause screen (F1) and type
a code; its message replaces the pause screen's hint. As shipped they only
work once the game has been won; to have them from the start, run with
`ONI_CHEATS=1`:

```sh
ONI_CHEATS=1 cargo run --release -p oni
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
| `bighead` | Everyone's head four times the size (toggle) |
| `minime` | You at a quarter of your size (toggle) |
| `behemoth` | Godzilla: you at 1.8 times your size, and not knocked down (toggle) |
| `fistsoflegend` | Your blows send everyone flying (toggle) |
| `roughjustice` | Gatling guns: your guns fire far faster (toggle) |
| `munitionfrenzy` | A gun of every kind around you |
| `reservoirdogs` | Last man standing: everyone fights everyone (toggle) |
| `forcefield` | A full shield belt *(the port's own, not the original's)* |

Letters typed are kept (the last 20); a code they end with is carried out.
Toggles switch off when a new level begins (the original keeps regeneration,
gatling guns and last man standing across levels). Not carried out:
`shapeshifter`, `glassworld`, `killmequick`. As shipped the cheats unlock once the game has been won; the
port keeps that with the saved game.

## Packaging

`packaging/` builds the game for players (#105) — it contains none of the
original game; the launcher installs the player's own:

- macOS: `packaging/macos/build-app.sh [--universal]` → `target/package/Oni.app`
  and `Oni-macos.zip` (signed ad hoc, not notarized: the first start is from
  the context menu's Open).
- Linux: the Flatpak manifest `packaging/flatpak/io.github.blitsandbobs.Oni.yml`
  (`flatpak-builder`), with its desktop entry and AppStream metadata in
  `packaging/linux/`.
- Windows: a zip of `Oni.exe` (no console window in release builds).

The `package` workflow (`.github/workflows/package.yml`, run by hand or by a
`v*` tag) builds all three. A build includes `oni-bink` (GPL-3.0-or-later),
so it is distributed under the GPL-3.0 — `packaging/NOTICE.md` goes with it.

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
