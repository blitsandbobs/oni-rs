# oni-view scenarios

Headless Lua scenarios that drive the engine for validation — gravity, the
locomotion state machine, collision — without a window. They replace the old
`--sim`/`--walk`/`--facing` test flags: validation lives in these `.lua` files
(which assert) instead of in bespoke CLI flags.

## Running

```sh
cargo run --release -p oni-view -- \
  generated/oni/levels/01/environment.glb \
  --character generated/oni/common/characters/konoko_body_high/model.glb \
  --anim      generated/oni/common/animations/KONOKOrun_lt.glb \
  --char-pos  200,8,300 \
  --script    scenarios/<name>.lua
```

`--char-pos x,y,z` sets the start position (e.g. `200,20,300` for `drop.lua`).
`--anim` only needs to point at one clip in the set; the whole `KONOKO*` set is
loaded for the state machine (the prefix is derived from the filename, override
with `--anim-prefix`).

## Scenarios

| File | What it checks |
|------|----------------|
| `run.lua` | Idle → run (start → loop) → settle, driven by holding "forward". |
| `drop.lua` | Gravity: dropped from height, lands grounded at `y≈0`. Use `--char-pos 200,20,300`. |
| `collision.lua` | Move-and-slide: running into the lockers stops her (no tunnelling). |
| `stitch.lua` | Clip-to-clip blending: the idle→run interrupt blends (weight 1→0 over 8 frames) instead of snapping. |
| `strafe.lua` | A/D sidestep: lateral motion with the body still facing forward. |
| `turn.lua` | Facing slew: a requested turn moves the body a few degrees/tick (not a snap); a big turn plays a turn-in-place clip. |
| `walk.lua` | Walk modifier: holding walk covers far less ground than run over the same window. |
| `jump.lua` | Jump: rises off the ground (start→air→land clips), lands grounded; a running jump carries forward momentum. |

## Lua API

| Call | Effect |
|------|--------|
| `input(forward, backward)` | Set the move keys (W/S). |
| `strafe(left, right)` | Set the strafe keys (A/D sidestep). |
| `walk(on)` | The hold-to-walk modifier. |
| `jump()` / `airborne()` | Queue a jump / read whether the character is in the air. |
| `facing(deg)` | Snap the body + target to an absolute yaw. |
| `turn(delta_deg)` | Nudge the facing *target*; the body slews toward it over ticks. |
| `facing_deg()` / `desired_deg()` | Read the body yaw / the target yaw. |
| `step(n)` | Advance `n` ticks at 60 Hz (state machine + physics). |
| `cam(ex,ey,ez, tx,ty,tz)` | Place the camera (eye, look-at target). |
| `pos()` → `x,y,z` | The character's feet position. |
| `grounded()` → `bool` | Whether she's on the ground. |
| `stitch()` → `weight` | Current clip-blend weight (0 = not blending). |
| `anim()` → `name` | The clip currently playing. |
| `screenshot(path [,w,h])` | Render one frame to a PNG (default 800×600). |

Standard Lua is available (`print`, `assert`, `string.format`, `math`), so
scenarios can assert outcomes and fail loudly.

Implementation: `crates/oni-view/src/script.rs`. Lua is confined to `oni-view`;
the portable crates stay dependency-light.
