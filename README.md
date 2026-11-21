# oni-rs

A ground-up, portable **Rust** reimplementation of [Oni](https://en.wikipedia.org/wiki/Oni_%28video_game%29)'s
data formats, built for long-term preservation.

The original 2000/2001 engine (`../original/source`) only builds with dead toolchains
(Visual C++ 6 + DirectX/Glide on Windows, CodeWarrior + RAVE on Mac OS Classic).
Rather than resurrect those, this project **reimplements the engine in clean
Rust, subsystem by subsystem**, using the original C as the authoritative spec
and the real game data as the test oracle. The first thing to preserve is the
ability to *read the data*, so that is where we start.

## Validation strategy

Every format crate is checked against **OniSplit**, the community's C#/.NET tool
that is the de-facto reference implementation of Oni's file formats. We run it
under Mono on the real Anniversary Edition data and assert our output matches.

## Workspace layout

| Crate | Purpose | Status |
|-------|---------|--------|
| `oni-fourcc` | `FourCc` four-character tag type (`TXMP`, `SNDD`, …) | ✅ |
| `oni-bytes`  | Endian-aware, bounds-checked byte cursor (PC LE / Mac BE) | ✅ |
| `oni-dat`    | The `.dat` container: enumerate every instance (tag, name, size, flags, data) | ✅ milestone 2 |
| `oni-texture` | `TXMP` textures: pixel formats, `.raw` resolution, decode to RGBA8888 | ✅ milestone 3 |
| `oni-sound`   | `SNDD` sounds: parse `SStFormat`, extract MS-ADPCM audio to WAV | ✅ milestone 4 |
| `oni-bink`    | Bink 1 movies (`outro.bik`): container, video and audio decoding, FFmpeg-exact; vendored from ra2-engine (GPL-3.0-or-later) | ✅ decoding (#59) |
| `oni-template` | generic swap-code walker + **generated standalone layouts** for 133 templates | ✅ milestone 5 |
| `oni-template-gen` | one-time codegen: parse the C `tm_template` headers → `oni-template/src/generated.rs` (dev tool; not shipped) | ✅ |
| `oni-geom`    | `M3GM` geometry + `TRBS` character bodies → glTF (`.glb`) | ✅ milestone 6/8 |
| `oni-convert` | GameDataFolder → modern `oni/` tree: assets + cross-file resolution + `level.json` | ✅ milestone 7 |
| `oni-view` | Viewer: renders a level `environment.glb` with `wgpu` (textures, baked lighting, mipmaps) and animated characters (`model.glb` + `TRAM` clip); windowed free-fly or headless screenshot | ✅ engine E1–E2 |

Reference data and OniSplit oracle outputs live **outside** the workspace in
`../original/` and `../generated/` (copyrighted assets — not committed).

## The `.dat` container format

Recovered from `OniSource/.../BFW_TemplateManager/{BFW_TM_Private.h, BFW_TM_Game.c}`.
A level `.dat` is a flat image; its companion `.raw` (and sometimes `.sep`)
holds bulk payloads referenced by instances.

```text
offset 0                 Header (64 bytes)
                           u64 totalTemplateChecksum
                           u32 version            = 'VR31' (byte order => endianness)
                           u16 sizeofHeader(64) / InstanceDesc(20) / TemplateDesc(16) / NameDesc(8)
                           u32 numInstances / numNames / numTemplates
                           u32 dataBlockOffset / dataBlockLength
                           u32 nameBlockOffset / nameBlockLength
                           u32 pad[4]
offset 64                InstanceDescriptor[numInstances]  (20 bytes each)
                           u32 templateTag, dataOffset, nameOffset, size, flags
+ numInstances*20        NameDescriptor[numNames]          (8 bytes each)
+ numNames*8             TemplateDescriptor[numTemplates]  (16 bytes each)
dataBlockOffset          instance data block
nameBlockOffset          NUL-terminated name strings
```

Key rules the loader applies (and we reproduce):
- **Endianness** comes from whether `version` reads as `VR31` little- or big-endian.
- `templateTag` is the instance's tag; `dataOffset`/`nameOffset` are offsets into
  the data/name blocks.
- A **placeholder** instance (`flags & 2`) has no data of its own (forward
  reference to another file).
- A **unique** instance (`flags & 1`) has *no name*; all others are named.
- Each instance's data is preceded by 8 bytes of "pre-data" (`TMcPreDataSize`),
  not included in `instance_data()`.

## Converter output structure (target)

The end goal is a one-time converter a user runs on their own CD, producing a
modern, engine-neutral tree (so the game code never carries the 2000-era
formats). Settled layout — **hybrid, glTF for 3D, PNG textures, WAV audio**:

```
oni/
├── oni.json                 # manifest: converter version, source hash, formats
├── common/                  # shared assets (from level0)
│   ├── characters/<name>/   # character.json + model.glb + animations/*.glb
│   ├── weapons/<name>.glb   ·  props/<name>.glb  ·  particles/*.json
│   ├── textures/*.png  ·  materials/*.json  ·  fonts/  ·  ui/  ·  text/
│   └── audio/{sfx,music,dialogue}/*.wav
├── levels/NN_<slug>/        # level.json (placement/lighting/triggers/doors) + environment.glb + textures/
└── scripts/<level>/*.bsl
```

Cross-references (Oni instance IDs) become relative paths / stable string IDs.

## Playing the game

Convert the original game data once (a few seconds), then run the game; with
no arguments it opens Oni's main menu:

```sh
cargo run --release -p oni-convert -- ../original/gamedata ../generated/oni
cargo run --release -p oni-view
```

Controls are the shipped defaults (W/S/A/D, mouse, Space, Shift, Ctrl, Q,
Tab, R, F1 for the pause screen); `CLAUDE.md` lists them with the
developers' launch options.

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

Toggles switch off when a new level begins. The rest of the original's codes
and the details: `docs/CHEATS.md`.

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

## Roadmap

1. ✅ **`.dat` container** — enumerate instances.
2. ✅ **`.raw` resolution + `TXMP` textures** — all 5 pixel formats present in
   level0 (DXT1, ARGB4444, RGB555, RGB888, ARGB1555) decode to RGBA8888;
   pixel bytes match OniSplit's DDS extraction byte-for-byte for all 1320 named
   textures (`oni-texture`, run `txmp_extract` to dump PNGs).
3. ✅ **`SNDD` sound** — `.raw` audio (MS-ADPCM, mono/stereo, 22050/44100 Hz)
   wrapped to WAV; `data` chunk byte-identical to OniSplit `-extract:wav` for all
   2380 level0 sounds (`oni-sound`, run `sndd_extract`).
4. ✅ **Swap-code-driven field decoder** (`oni-template`) — a generic walker
   over a `Layout` yields named, offset-tagged fields, plus **generated,
   standalone layouts** for 133 templates emitted by `oni-template-gen` from the
   C `tm_template` headers (committed as `generated.rs`; the C source is then no
   longer needed). Validated: generated TXMP and SNDD layouts reproduce the
   hand-written `oni-texture`/`oni-sound` parsers exactly (1957 + 2380
   instances), and every instance of all 63/64 covered level0 tags (30476
   instances) decodes without overrun.
   - Found along the way: `UUtBool` is 1 byte (its manual `tm_pad[3]` aligns to
     4); and `SNDD`'s released source drifted from the data (its `SStFormat` is
     commented out) — corrected via a documented override in `overrides.rs`.
   - **Whole-corpus coverage:** across all 15 shipped level `.dat` files, **95
     distinct template tags, 95 covered (100%)**, all 44,178 instances decode
     without overrun (`tests/all_levels_coverage.rs`). The `TRSC` gap was a tag
     whose description string held parentheses — fixed in the scanner.
   - MS-ADPCM→PCM decode and OniSplit-XML field-name parity remain future work.
5. ✅ **Geometry `M3GM` → glTF** (`oni-geom`) — resolves the point/texcoord/
   index arrays, walks the triangle strip (with `0x80000000` restart markers,
   `triNormalIndexArray.numIndices` triangles) exactly as the engine's software
   path, and emits `.glb`. All 7239 M3GM across every level decode with 100%
   in-range topology (358k triangles).
6. **`TRBS` character bodies** — skinned meshes + skeleton → glTF with joints.
7. ✅ **The converter** (`oni-convert`) — `cargo run -p oni-convert -- <GameDataFolder> <out>`:
   - `common/` (globally-named assets, deduped by name) + `levels/NN/` (level-local
     unnamed assets) + `oni.json` with per-file SHA-256 provenance.
   - textures→PNG, sounds→WAV, static meshes→glTF.
   - **Cross-file resolution:** named assets form a global pool in `common/`, so
     references resolve across files (567/571 mesh→texture refs resolve on disk;
     the 4 misses are door textures genuinely absent from the data).
   - **`level.json`:** per-level semantic objects (`ONLV` level descriptor, AI
     graph, triggers…) decoded via the generic registry, with template refs
     emitted as `{"$ref": "<name>"}` links.
8. ✅ **Characters** (`oni-geom::character`) — `TRBS` rigid-skeletal bodies
   (19 named bones: `TRGA` geometry + `TRTA` translations + `TRIA` hierarchy) as
   a glTF node tree with real bone names (`pelvis`, `left_calf`, …). Posed by a
   neutral idle: `TRAM` frame-0 quaternions set each node's rotation (glTF
   composes `local = T·R`, matching the engine). The converter bakes `NINJAidle1`
   into 86 `common/characters/<name>/model.glb` — standing, recognizable
   characters. Validated against OniSplit's DAE (identical skeleton) and by
   render.
9. ✅ **Animation clips** (`oni-geom::character::decode_animation`) — every
   `TRAM` keyframe of every bone (gap-walked compressed streams; 6-byte Euler or
   16-byte float; self-describing `numParts`/`compressionSize`/`numFrames`). The
   converter writes **2606** clips to `common/animations/<name>.glb`: a
   named-bone reference skeleton + per-bone rotation channels, retargetable by
   bone name. Verified: Konoko runs when a clip is retargeted onto her rig.
10. ✅ **Refinements** — (a) `level.json` now nests fields (`position:{x,y,z}`);
    (b) **MS-ADPCM → PCM**: sounds decode to 16-bit PCM WAV, byte-exact vs
    ffmpeg (mono + stereo); (c) **`.sep`** texture path for retail PC / Mac CDs
    (pixels via `pixelStorage`); (d) per-character `character.json` rig manifest
    (bones, parts, `animationSet`) + `animations/index.json` grouping the 2606
    clips into 51 sets. (Oni has no static per-character animation list —
    animations resolve by name convention, e.g. `KONOKO…`.)
11. ✅ **Level environments** (`oni-geom::level`) — the `AKEV` world geometry:
    "gunk quads" reference points, texcoords and a `textureMapIndex`, so each
    level exports to `levels/NN/environment.glb` as **material-bound submeshes**,
    one per texture (level 1: 144 materials, ~64k triangles — a walkable,
    textured level). Each quad's `object_tag` links it to a placed object.
12. ✅ **Object placement** — each quad's `object_tag` is
    `(OBJcTypeIndex << 24) | id`, so the environment splits into the **static
    world** plus every **placed object** as its own pivot node in
    `environment.glb` (level 1: 485 objects — 364 furniture, 97 doors, 21
    consoles, 3 triggers — each a separate, transformable entity). `level.json`
    gains `placedObjects` (the authored `OBOA` list: 180 named objects with
    `position`/`orientation`/`scale`/`scriptId`/`isDoor`) and
    `environmentGeometry` cross-linking each baked object to its glTF node.
13. ✅ **Materials & baked lighting** — per-corner `shades[4]` become glTF
    `COLOR_0` vertex colors (Oni's baked lighting, multiplied with the texture);
    `Transparent`-flagged quads get `alphaMode:"BLEND"` materials; `Invisible`
    collision-only quads are culled (level 1: 64k → 55k triangles) and
    `Triangle`-flagged quads emit a single tri.
14. ✅ **Object animation & gameplay** (`oni-geom::objanim`) — `OBAN` scripted
    object motion (sliding doors, lifts, driving vehicles) → per-level
    `object_animations.json` (keyframed quat+translation; **1067 clips** across
    all levels). The level's gameplay logic (triggers `AITR`, enemy setups
    `AISA`, spawns `ONSA`, doors `OBDC`, objectives `OPge`, particles `ENVP`,
    corpses `CRSA`) is captured in `level.json` via the generic decoder.
15. ✅ **Character classes** (`oni-geom::charclass`) — `ONCC` → `common/
    characters/classes.json` (124 classes: body, variant, textures, aiming
    screens, and the **exact animation set**). The struct drifts from source, so
    the fields are found by **template-reference scan** (each ref identified by
    the tag it points at) and the animation list comes from the `TRAC`
    collection following `recursiveLookup` inheritance (konoko: 640 animations) —
    replacing the earlier name-convention heuristic. Gameplay **scalars** (hit
    points, scale range, per-damage-type resistance, shot time, AI timings,
    flags) are read from the struct's *stable tail*, anchored on the body ref
    (grunts 25 HP → bosses up to 400).
16. ✅ **Text, cutscenes & fonts** — `IGSt` user-facing strings (tutorials, HUD,
    subtitles) → per-level `text.json`; `TStr` internal table → `strings.json`
    (1567 strings); `FILM` cutscene recordings (initial pose + per-key camera/
    input) → per-level `films.json` (326 clips); `TSFT` font metrics →
    `common/fonts.json`.

### Sound extraction (`oni-sound`)

```sh
cargo run --release -p oni-sound --example sndd_extract -- \
  ../original/gamedata/level0_Final.dat /tmp/oni_wav
# byte-exact regression (OniSplit -extract:wav output):
ONI_WAV_DIR=/tmp/oni_wav_oracle cargo test -p oni-sound --test sndd_oracle
```

### Texture decoding (`oni-texture`)

```sh
cargo run --release -p oni-texture --example txmp_extract -- \
  ../original/gamedata/level0_Final.dat /tmp/oni_png
```

The byte-exact regression test runs when `ONI_DDS_DIR` points at OniSplit
`-extract:dds` output:

```sh
ONI_DDS_DIR=/tmp/oni_dds cargo test -p oni-texture --test txmp_oracle
```
