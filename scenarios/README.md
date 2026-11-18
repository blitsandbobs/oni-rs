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
The animation set is the character class's real collection chain (`--class`,
default `konoko_generic`); `--anim` is only used by the static viewer.
`--faithful` reproduces the original game's known bugs (`docs/QUIRKS.md`).

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
| `combo.lua` | Melee: punch/kick combos chain (P→PP→PPP) from the KONCOM combat set. |
| `fight.lua` | Hit detection: spawn a training dummy, punch it — HP drops + knockback in reach, misses out of reach. |
| `air_moves.lua` | The combat jump: punch / kick / crouch-tap in the air play the jump punch, jump kick and flip. |
| `block.lua` | Automatic blocking: a dummy facing the player head-on blocks a punch (no damage); turned 60° away it can't. |
| `reactions.lua` | Hit reactions: a behind hit, a combo knockdown, getting up (dummy and player), death. |
| `patrol.lua` | AI movement: a route follower runs a square with ordinary locomotion; held orders sidestep and backpedal. |
| `mouse_melee.lua` | Unarmed, the fire buttons are melee: fire 1 (left mouse) punches, fire 2 (right) kicks, fire 3 (middle) crouches. |
| `classes.lua` | Other character classes spawn with their own body, skin, animation set and hit points, and can be thrown (the victim plays the thrower's clip). |
| `throw.lua` | A forward punch / kick near an opponent becomes a front or behind throw (victim placed by the throw, locked out, hurt on landing); a running punch becomes a running throw; out of reach it's a plain forward punch. |
| `rear_kick.lua` | Blows connect by body contact: a rear kick hits a dummy behind the player; a jab at nobody misses. |
| `spar.lua` | The sparring partner closes in without walking through its opponent, lands hits, gets up after a knockdown, stops once its opponent is dead. |
| `autoface.lua` | Starting a punch turns the player onto a dummy 35° off-aim. |
| `ground_moves.lua` | Crouch, crouch-run, the roll (crouch tap mid-run), and a sideways jump out of a sidestep. |

## Lua API

| Call | Effect |
|------|--------|
| `input(forward, backward)` | Set the move keys (W/S). |
| `strafe(left, right)` | Set the strafe keys (A/D sidestep). |
| `walk(on)` / `crouch(on)` | Hold the walk / crouch modifier. |
| `jump()` / `airborne()` | Tap jump / read whether the character is in the air. |
| `punch()` / `kick()` / `crouch_tap()` | Tap a button (held for the next tick only; mash with a gap tick between taps). |
| `fire(n)` | Tap fire 1 / 2 / 3 (left / right / middle mouse; unarmed: punch / kick / crouch). |
| `state()` / `anim_type()` | The body state / current animation type (numbers, see `anim/types.rs`). |
| `aim_deg()` | The aim direction (desired facing without the diagonal / sidestep body lean). |
| `faithful(on)` | Reproduce the original's known bugs (`docs/QUIRKS.md`). |
| `seed(n)` | Reseed the weighted animation pick for repeatable runs. |
| `type_of(id)` / `state_of(id)` / `facing_of(id)` / `dead(id)` | Inspect any fighter. |
| `set_hp(id, n)` / `set_facing(id, deg)` / `knock(id, anim_type)` | Test hooks: set health, turn, force a reaction. |
| `patrol(id, {{x,z},…} [, pace, loop])` / `route_done(id)` | AI: walk a waypoint route (`"run"`/`"walk"`/`"creep"`) / read whether it finished. |
| `ai_order(id, dir [, pace, face_deg])` | AI: hold fixed orders — `"forward"`, `"backpedal"`, `"left"`, `"right"`, `"stop"`. |
| `spar(id, target)` | AI: make fighter `id` fight fighter `target` (the sparring partner). |
| `facing(deg)` | Snap the body + target to an absolute yaw. |
| `turn(delta_deg)` | Nudge the facing *target*; the body slews toward it over ticks. |
| `facing_deg()` / `desired_deg()` | Read the body yaw / the target yaw. |
| `spawn(x,y,z [,facing_deg [,class]])` → `id` | Spawn a training dummy of character class `class` (default: `--opponent`, else the player's class); returns its fighter id (default facing 180°). |
| `class_of(id)` → name | A fighter's character class. |
| `count()` → `n` | Number of fighters (player = 0, plus dummies). |
| `hp(id)` → `hp` | A fighter's hit points. |
| `pos_of(id)` → `x,y,z` | A fighter's feet position. |
| `anim_of(id)` → `name` | A fighter's current clip. |
| `step(n)` | Advance `n` ticks at 60 Hz (every fighter + hit resolution). |
| `cam(ex,ey,ez, tx,ty,tz)` | Place the camera (eye, look-at target). |
| `pos()` → `x,y,z` | The player's feet position. |
| `grounded()` → `bool` | Whether the player is on the ground. |
| `stitch()` → `weight` | Current clip-blend weight (0 = not blending). |
| `anim()` → `name` | The player's current clip. |
| `screenshot(path [,w,h])` | Render one frame to a PNG (default 800×600). |

The movement/combat calls (`input`, `strafe`, `jump`, `punch`, `facing`, …) act
on the **player** (fighter 0); `*_of(id)` read any fighter by its spawn id.

Standard Lua is available (`print`, `assert`, `string.format`, `math`), so
scenarios can assert outcomes and fail loudly.

Implementation: `crates/oni-view/src/script.rs`. Lua is confined to `oni-view`;
the portable crates stay dependency-light.
