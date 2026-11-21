# oni-view scenarios

Headless Lua scenarios that drive the engine for validation — gravity, the
locomotion state machine, collision — without a window. They replace the old
`--sim`/`--walk`/`--facing` test flags: validation lives in these `.lua` files
(which assert) instead of in bespoke CLI flags.


`scripts/check.sh` runs both passes (with clippy and the tests) in parallel
— the commit gate. Run them all with `scenarios/run_all.sh` (and again with `--faithful`): a
scenario fails when its script errors (an `assert`), which makes `oni-view
--script` exit non-zero. They run 8 at a time (`-j N` to change; `-j 1` one
by one), each its own process; the report comes in file order. A pass takes
about 9 s (one by one, about 30 s; the longest, `mission.lua`, plays the
whole training in about 6.5 s). Scenarios that write files (screenshots)
must each use their own path.

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
| `pause.lua` | The pause screen on level 3: F1 opens it on the objectives (current first, Previous to older), items and weapons only once met, the diary up to this level on its first page, the help; the game stands still; Close. |
| `mouse_melee.lua` | Unarmed, the fire buttons are melee: fire 1 (left mouse) punches, fire 2 (right) kicks, fire 3 (middle) crouches. |
| `collisions.lua` | What movement meets: an AI walking past someone in its way bumps them, records it and steers round; Konoko against a wall counts it. |
| `loose.lua` | Loose things: a gun let fall lands fast with its impact and lies still; a dying thug's hypo falls and comes to rest; a lifted Striker rises toward Konoko and falls when let go. |
| `objects.lua` | Placed objects: none until created; forklifts made, run their clips a frame a tick, swap clips, killed; the truck's wheels stop Konoko. |
| `object_particles.lua` | Level 8's opening: the black van's headlights come with it, tagged after it (the script starts them by name), ride it as it drives in, and a pistol's sight aimed at it stops on it. |
| `door_jello.lua` | Level 1's door 18: shut, its own quads between the camera and Konoko go see-through and are drawn so; opening, its quads hide and its moving leaf goes see-through as an object (fixed mode; Q17 as shipped) and blocks a lens flare's ray. |
| `neutral.lua` | Neutral characters (#14), level 1: the truck driver notices Konoko and comes up; Action facing him starts his line (her moves held back; this one can't be walked out of); at its end his script switches the warehouse console on and he runs to it. A punch breaks another civilian's talk off; given a giving behaviour by script, he walks up and hands Konoko a pistol. |
| `object_jello.lua` | Level 3's van: its rear door between Konoko and the camera goes see-through (fixed) and stays solid as shipped (Q17, the ray read as a point); away from it nothing is. |
| `door_sound.lua` | A closed door keeps an ordinary noise from an AI beyond it but lets a gunshot through (muffled); open, it lets anything through. |
| `gamma.lua` | The gamma option: 0.5 leaves a frame as drawn, 1 brightens it, 0 darkens it (the original's display ramp). |
| `game_flow.lua` | Save points: `save_game` keeps Konoko (place, health, gun, clips) and `restore_game` puts her back; a script's splash screen is asked for; `win` wins; Konoko dying runs her die script, which fades out and loses the level. |
| `cinematics.lua` | Cutscene portraits: a name tag slides from off the right to the top right (inset 20 × 65), slides away and goes on `cinematic_stop`; −1 takes the texture's size; a missing texture shows nothing; `end_cutscene` takes them all. |
| `flinch.lua` | Flinches: a pistol shot plays the Striker's hit overlay over his idle; a punch from behind on the knockdown-resistant Mutant Muro only flinches him. |
| `fog.lua` | The fog: the draw engine's on a level (grey 0.25, 0.975–1); a script's red fog reddens the far warehouse and none doesn't; `gl_fog_start_changeto` eases the start over its frames. |
| `timer.lua` | The countdown: `timer_start` shows 0:12 → 0:11 → 0:09.3, blinking under twenty seconds; at zero its script runs and 0:00.0 blinks five seconds; `timer_stop` ends one early (no script). |
| `script_ai.lua` | Scripts steering characters (level 2): `chr_create` makes setup 1050 at its flag, known by its script id; a cutscene makes the AIs passive and puts Konoko's gun away, its end undoes both; `ai2_takecontrol` + `ai2_movetoflag` walk her to a flag and turn her to its facing, `ai2_lookatchar` turns her toward someone; `ai2_makeblind` / `ai2_forget`. |
| `ambush.lua` | Level 2's ambush: after the intro both partner cops fight the Strikers, the Strikers come out of their room (doors linked to pathfinding connections as the original links them) and reach Konoko in her room though no route leads there (the advance's local walk). |
| `barabus.lua` | Level 3's opening: Barabus (unkillable, infinite ammo) fires on Konoko without his gun running down; worn down he is left on 1 and defeated, his rocket scene removes him, objective 2 follows. |
| `auto_aim.lua` | Konoko's arm follows her gun: aiming 30° and 60° to each side, and when auto-aim turns the gun onto an enemy 4° off her aim (the gun stays in her hand). |
| `downed.lua` | A downed Striker doesn't turn while it lies: not toward Konoko walking round it, not when ordered to face elsewhere. |
| `hypo.lua` | Hypos: a quarter of full health given a point every 7 ticks; a hurt eats 2.5 × its damage from it; at full a whole boost to double, a point a tick, then draining back a point every 11 ticks; none, or already boosting, nothing. |
| `lens.lua` | Lens flares: none in sight at the start; the lock light beside a warehouse door, seen from the side, shows (red: locked) and goes again out of sight. |
| `hud.lua` | The HUD: messages print, time out and fade; a new objective is prompted until read; the compass takes and drops a target; a blow lights the hit ring on its side and fades; a new kind of gun is announced once; standing by a gun prompts to pick it up; a cutscene letterboxes and hides the rings; fades out and in; a dialogue subtitle stays while the line plays; scripts fill and hide its parts. |
| `camera.lua` | The game camera: 33 back and 23 up behind Konoko, sinking as she looks up and rising as she looks down, easing round a turn; look mode swings the view and drifts back; the corridor wall behind her goes see-through; `cm_detach`, `cm_reset` and `cm_interpolate` (busy, then free). |
| `door_leaves.lua` | Door leaves move as boxes; a leaf closing on Konoko holds still, then the door turns back for her; a double door blocked at its second leaf stays half-shut in faithful mode (Q14). |
| `doors.lua` | Doors (#17): one opens as Konoko comes near and she runs through; it closes behind her; locked it stops her; unlocked it opens again. |
| `look.lua` | Where an AI looks (#30): an unarmed agent told to look at a point swings its aim there (at most 6° a tick) and its head and upper body follow through its aiming screen; told to stop, both come back to the middle. |
| `lasers.lua` | Laser triggers: level 1's training lasers start off, sweep when switched on, trip on Konoko (the low beams, not the one over her head), hide and switch off. |
| `decals.lua` | Decals: level 1 lays its 116 on their walls, 0.05 off; a pistol round leaves a bullet hole the class's size on the wall it hit, and its spark sits 0.4 off the wall facing out; it is drawn only from in front of its one-sided wall, and fades to 20/255 when the wall goes see-through. (`impacts.lua` also checks a punch's flash takes the dummy's health colour.) |
| `deadly_brain.lua` | Level 2's Deadly Brain started: its readouts spiral out from the emitter to their radius (30–90) around the brain (`moveSpiral`). |
| `animated_textures.lua` | Level 3's goo runs through its 15 frames at 5 ticks each by the game time: five ticks on, one frame on. |
| `shadows.lua` | Character shadows: standing, flat on the floor under her pelvis 0.05 up, at the class's opacity and size; a jump fades it; `chr_shadow` takes it away and gives it back. |
| `effects.lua` | An effect rides what it struck: a phase-rifle bolt's glow stays on the dummy's struck part as the dummy moves. |
| `reach.lua` | How high a jump reaches (the original's collision): on level 1's training a running jump gets from a small crate onto the tall one; the ledge after the laser room is out of reach of a jump but not of a flip. |
| `sight.lua` | The gun's sight: from the marker on the gun, stopping 2 short of the wall or a character; red / blue lasers, a tunnel without one; none for others, an empty gun, or a gun through a wall (which doesn't fire). |
| `gun_hold.lua` | The gun in the hand: it rides the hand through a pickup, points down the aim standing, is held to the aiming screen's reach; a left-handed ninja holds it in the left hand. |
| `glass.lua` | A shot breaks a glass pane, which bursts into five falling shards: level 1's range has 18 targets (references 3001–3018) that `env_broken` counts; `env_show` breaks and mends glass. |
| `barabbas_ai.lua` | Barabbas on level 3: at range he goes through his beam, waiting for it, and advancing with grenades; worn to 30% with Konoko far off he regenerates (the powerup clip, his health coming back), and shot for 5% of his health meanwhile he stops. |
| `superninja_ai.lua` | The super ninja on level 12's roof: from afar his fireball clip launches its homing particle; closing in he turns invisible (or teleports behind); beaten about close in he teleports out to a retreat flag far off; dropped below the roof he hangs at 550 and teleports back up onto it. |
| `muro_ai.lua` | Mutant Muro on level 19: his super shield up at once; from afar, after 5 s, the thunderbolt — homing bolts that pull Konoko in while his shield drops; close in, hurt with his shield down, he turtles. |
| `anim_particles.lua` | Animation particles: Konoko's running kick makes its trail with the clip, riding her foot, gone when the clip ends. |
| `script_extras.lua` | The last engine commands on level 1: at most 6 ticks a frame, a cutscene owing the rest till a sync mark; `slowmo` halves the speed for its ticks; `env_shade` / `obj_shade` light level parts and objects flat; `killed_griffen` is what `did_kill_griffen` says; `chr_super` raises Konoko's super amount (0.03 a tick) and runs her three super particles; a boss shield flares red when shot; `ai2_followme` brings an AI to her; `ai2_barabbas_retrievegun` puts the unreloadable gun in an empty hand. |
| `costumes.lua` | Placement flags on level 1: a warehouse thug with a random costume takes one of the `thug_wh` classes (several over 12 spawns); an ambush Striker that upgrades takes a `striker_easy` class on normal and a `striker_med` one on hard; the scripts' `difficulty()` is the game's. |
| `shield_invis.lua` | Shield belts and invisibility (and the cheat codes): `forcefield` typed on the pause screen (only once allowed) gives a full shield, shown climbing 2 a tick to a glow of 89; a shot into a shielded dummy takes shield, not health, and flares the parts struck; `moonshadow` fades Konoko out, unseen by a Striker facing her after 90 ticks, a punch shows her at 0.6; off, she fades back; a picked-up cloak runs down 1800 ticks. |
| `corpses.lua` | Level 1's pre-placed corpse (Chung) is drawn lying on the warehouse floor with the LSI by his hand; from save point 1, finding the body plays its cutscene, Konoko picks up the LSI, the script's data console reads it and door 37 unlocks. |
| `ai_shoot.lua` | An armed enemy that sees the player goes into combat, aims and fires; its shots land; empty, it fights hand to hand. |
| `ai_gun.lua` | An unarmed enemy that shoots goes for a loaded gun lying near, bends for it and shoots with it. |
| `patrol_shoot.lua` | Level 4: a patrolling thug shoots at its flag for the waypoint's time (rounds spent), then goes on to its scan. |
| `ai_dodge.lua` | An armed soldier holding its ground steps off the line of a gun spraying at it once the danger has lasted its reaction time. |
| `ambience.lua` | Level 1's sound objects: a box eases its ambient in over 2 s and out after leaving (then halted); a sphere's ambient plays placed while near and plays out once left. |
| `gunfire.lua` | Level 1's pistol: the report and the bullet's whine (particle ambients), wall hits by material, ricochets and casings, and the click as a held trigger runs dry. |
| `object_sounds.lua` | Level 1: door 18 opening plays its class's sound; ammo taken the game's way plays the receive sound; low health starts its loop, healing ends it. |
| `script_sound.lua` | Level 1's opening: its music starts, and Shinatama's lines play one at a time, each blocking script waiting for the last. |
| `voices.lua` | Level 1: a striker punched again and again grunts and cries out as it dies; an AI striker seeing the player says its startle line. |
| `impacts.lua` | Level 1: Konoko's running footsteps sound by the floor (tile) and her light weight; a punch landing on a dummy sounds by the struck part, and swooshes (her variant's punch sound). |
| `sounds.lua` | Level 1: glass-breaking impulses — placed and panned (PC law), the alt past three copies, nothing out of earshot, a full stereo pool gives up its quietest channel, all play out. |
| `turrets.lua` | Level 2: a wall turret switched on hums (in sound, loop panned to its side, out sound), targets the player, fires and hurts her; deactivating "every turret" switches none; after its time it rests and switches off. |
| `weapons.lua` | A gun turns fire 1 into a shot: the pistol's particle hits the dummy once for 8–10, the magazine drops by one, its kick stays in the aim, ten shots empty it, and with a clip an empty gun reloads on the press. |
| `envanim.lua` | `chr_envanim` carries Konoko along an object clip and leaves her at its end; `chr_nocollision` lets her walk through the training barrier. |
| `env_show.lua` | `env_show`: the training room's barrier (reference 2010) stops Konoko while shown; a script hides it and she runs through. |
| `script_orders.lua` | Script orders through the real dispatcher: poison doses (5 every 30 calls; a gap restarts it), an unstoppable character takes blows without reacting, `ai2_kill` smites every AI but not the player. |
| `stairs.lua` | Stairs: Konoko runs up a flight (steps aren't collided with — the ramp under them is); the Bay 4 comguy climbs to its alarm console (#32). |
| `hearing.lua` | Hearing bodies (#13): a crouched walk is silent, running is heard, a landed blow alarms (medium). |
| `cutscene_skip.lua` | Skipping a cutscene: too soon (under 10 ticks in) nothing; then 20 frames' fade to black with the sound off, 32 ticks a frame unseen until the cutscene ends, 20 frames' fade back, the sound back. |
| `disarm.lua` | Throws take guns: an unarmed Konoko's front throw takes an armed dummy's pistol, an armed one's knocks it to the floor; a striker's "Disarm Front" takes hers. |
| `death.lua` | Death (#12): a body stays — unless its class deletes its dead after a delay (the mad bomber, 190 ticks). |
| `teams.lua` | Teams (#11): a friend's punch passes through; an enemy's lands; auto-facing turns toward an enemy, never a friend. |
| `text_console.lua` | Level 3's data console: `text_console` opens its 3 pages in a modal dialog — the game stands still; Space turns the page and closes on the last (Close), Esc closes; no such console opens nothing. |
| `alarm.lua` | Consoles and alarms: an AI sent to raise the alarm runs to the alarm console, uses it and goes back to its job; Konoko tripping it sends the AIs answering it after her. |
| `film.lua` | Films (`playback`): a recording puts Konoko at its start, walks her by its controls, and ends. |
| `menu.lua` | The main menu: Options at the start, Resume over a game; New Game's prompt and its faithful box; Load Game's rows (level 1, the warehouse) and keys; the options' defaults; a slider held pages on until its thumb meets the mouse; Quit asks; Escape; the level-load dialog. `MENU_SHOTS=dir` writes each dialog's picture. |
| `mission.lua` | The level's own scripts (#20): level 1 begins as in the game (player at its placement, `main` runs the training intro), the training waits for the player to turn and to run, a trigger volume runs its entry script when the player stands in it, Action at the lesson's console uses it, a double-tapped forward dashes, and the dash course (17 markers in order, then the finish, in time) ends with "Track run successful"; the review console; the door lesson (the training unlocks a manual door, Action opens it); the jump lessons; the crouch lessons — the combat demo (two trainees turn Syndicate and beat the third), creep, somersault, cartwheel and slide — until the training switches the lasers on; the flip; the fighting lessons (lasers off, a punch, a kick, the punch-punch and punch-punch-kick combos, hitting the drone, a throw, the block room); the shooting range (pick up the pistol, fire, break the 18 glass targets, pick up the clips put out when it runs dry and reload, take the rifle, shoot down the two training droids — collecting cells and reloading as needed); the last review console ends the training. |
| `goals.lua` | Goals (#4): a level's thug patrols its path (guarding two flags in turn), sees an enemy (glance → startle → combat), beats it, checks the body and resumes its patrol; a noise sends it to investigate and look around; a patroller too calm for its job stands idle; a non-combatant panics at an enemy (cowering) and calms down once it is dead. |
| `perception.lua` | An AI doesn't see behind it, hears a noise, glances round, sees Konoko (definite → combat, startled) and fights; out of its cones it sees nothing. |
| `pathfinding.lua` | Walking through the level: round a pillar, across the warehouse, and failing cleanly for a point no room contains. |
| `melee_walls.lua` | The melee AI looks before it moves: told to retreat with its back to a wall, it gives up instead of backpedalling into it. |
| `turn_keys.lua` | Lying down, a turn key rolls the character up; in the fight stance the action key taunts (the AI's get-up rolls and taunts). |
| `melee_ai.lua` | The melee AI: Konoko and Striker AIs land hits on a dummy with their profiles' techniques; a knocked-down AI lies dazed for its class's time, then gets up. |
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
| `collisions(id)` | What a fighter's movement met last tick: walls, objects, characters, and its AI's collision records and how many are serious. |
| `loose_guns()` / `powerups()` / `lift(id, by, ticks)` | The guns no one holds (`{class, at, moving}`), the powerups lying about (`{kind, at, falling}`); lift a fighter toward another as "picked up" damage does. `spawn_weapon(class, x, y, z, falling)` lets a gun fall instead of floating. |
| `objects()` | The live placed objects: `{{name, at, frame, animating}, …}`. |
| `door_leaves(id)` | Door id's leaf boxes this tick: `{{at, half, solid, paused}, …}`. |
| `sight(id)` / `gun_through_wall(id)` | A fighter's gun sight this tick: `{from, to, target, laser, tunnel}` (target `none` / `environment` / `character`, laser `{r, g, b}` or nil, tunnel its sprite count), nil without one; whether its gun pokes through a wall. |
| `bone(id, k)` / `held_gun(id)` | A fighter's posed bone k, or the gun in its hand: `{x, y, z, at}` (axes and place), nil without one. |
| `hold_jump(on)` / `feet_offset()` | Hold (or release) jump — the held, higher jump; how far the player's posed body's lowest point (arms left out) is above its location (the original's `feetLocation`). |
| `strafe(left, right)` | Set the strafe keys (A/D sidestep). |
| `walk(on)` / `crouch(on)` | Hold the walk / crouch modifier. |
| `jump()` / `airborne()` | Tap jump / read whether the character is in the air. |
| `punch()` / `kick()` / `crouch_tap()` | Tap a button (held for the next tick only; mash with a gap tick between taps). |
| `fire(n)` | Tap fire 1 / 2 / 3 (left / right / middle mouse; unarmed: punch / kick / crouch). |
| `state()` / `anim_type()` | The body state / current animation type (numbers, see `anim/types.rs`). |
| `aim_deg()` | The aim direction (desired facing without the diagonal / sidestep body lean). |
| `faithful(on)` / `faithful()` | Reproduce the original's known bugs (`docs/QUIRKS.md`); with no argument, whether it does. |
| `seed(n)` | Reseed the weighted animation pick for repeatable runs. |
| `type_of(id)` / `state_of(id)` / `facing_of(id)` / `dead(id)` | Inspect any fighter. |
| `set_hp(id, n)` / `set_facing(id, deg)` / `knock(id, anim_type)` | Test hooks: set health, turn, force a reaction. |
| `patrol(id, {{x,z},…} [, pace, loop])` / `route_done(id)` | AI: walk a waypoint route (`"run"`/`"walk"`/`"creep"`) / read whether it finished. |
| `ai_order(id, dir [, pace, face_deg])` | AI: hold fixed orders — `"forward"`, `"backpedal"`, `"left"`, `"right"`, `"stop"`. |
| `spar(id, target)` | Make a fighter fight another with its class's melee AI. |
| `turn_keys(l, r)` / `action()` | Hold the turn keys / tap the action key. |
| `probe(id, deg, dist)` → `free, ledge` | Look along the floor from a fighter: distance before a wall, distance to a drop (or nil). |
| `difficulty(level)` → `notice, dodge, block` | Set the AI's difficulty ("easy" / "normal" / "hard"); returns its multipliers. |
| `go_to(id, x, y, z [, pace])` / `nav_state(id)` / `nav_path(id)` | Walk a fighter to a point through the level (planned path); its state ("following", "arrived", "failed: …"); its waypoints `{x, z, radius, next}`. |
| `mission_begin()` / `mission_info()` / `mission_call(name [, arg])` / `named(name)` / `teleport(id, x, y, z)` | The level's scripts: begin the level (player placed, characters present from the start spawned, `main` run; `mission_begin(false)`: the scripts loaded only; `mission_begin(true, n)`: from save point `n`); what they're doing (`tasks`, `errors`, `messages`, `objective`, `outcome`, `spawned`, `stubbed` commands); start a script function; a character's id by name; move a fighter. |
| `alarm(id [, console])` / `console_state(id)` / `console_reset(id)` | Send an AI to raise the alarm (a console id, or the nearest alarm console); a console's state (`inactive` / `active` / `triggered`); reset one. |
| `play_film(id, name [, mode])` | Plays one of the level's films on a fighter (`""` from its start, `"fromhere"`, `"interp"`); its length. |
| `present(id)` | Whether a fighter is still in the level (not deleted). |
| `damage(id, n [, dx, dz])` | Deals `n` damage to a fighter (it dies at 0 health), with a small shove along (dx, dz) if given (the blow's way: it lights the hit ring). |
| `give_weapon(id, class)` / `weapon(id)` | Put a new gun of `class` (`w1_tap` …) in a fighter's hand; what it holds: class, ammo, firing (nil: unarmed). |
| `aim(id, pitch)` / `aim_of(id)` | Set / read a fighter's aim pitch (radians, up positive). |
| `use_hypo(id)` / `hypo_left(id)` | Use a hypo as Tab does (whether it was used); the health it still has to give. |
| `lens_flares()` / `lens_flare_near(x, y, z)` | How many lens flares there are, and how many are shown now; the opacity share of the flare nearest a point. |
| `game()` | The game flow: the level's outcome (`victory`: true won, false lost), a splash screen asked for (`splash`), the save point kept (`save_point`). |
| `hud()` / `hud_triangles()` / `read_objective()` / `subtitles(on)` | The HUD: its text areas' lines (`messages`, `subtitles`, `console`), `objective` / `new_objective`, the compass `target`, `letterbox` depth, `fade` opacity, `filled` / `flashing` bits, `show` (left, right), the player's `hits`; how many triangles it draws; F1 (the objective read); the subtitle option. |
| `camera()` / `jello()` | The game camera: its place, view direction and whether a script waits on it (`x, y, z, vx, vy, vz, busy`); how many level quads it makes see-through. |
| `fog()` / `image_mean(path, x0, y0, x1, y1)` | The fog now: `r, g, b, start, end` (screen depth); a saved screenshot's mean colour over a region (fractions), `r, g, b`. |
| `temp_path(name)` | A path in the temp folder of this process's own (`oni_<pid>_<name>`): for a screenshot a scenario reads back. The fixed and faithful passes run every scenario at the same time, so a fixed path would be shared. |
| `jello_objects()` | The placed objects see-through this tick (in the camera's way), by name. |
| `pause_toggle()` / `pause_screen()` / `pause_press(b)` | F1 on the pause screen; its state `{tab, page (from 1), pages, lines, hint, previous, next}` or nil; a button: `"objective"`, `"items"`, `"weapons"`, `"diary"`, `"help"`, `"previous"`, `"next"`, `"close"`. |
| `jello_doors()` | `{quads, leaves}`: how many see-through quads this tick are closed doors' own, and the open or moving doors' leaves see-through as objects (`"door id:leaf"`). |
| `object_in_way(ax, ay, az, bx, by, bz)` | Whether the segment meets an animating object — a placed object's sphere, an open or moving door's leaf (a lens flare's test, `AMrRayToObject`). |
| `text_console()` / `text_console_key(k)` / `text_console_click(x, y)` | The data console showing its text, `{name, page (from 1), pages, last, lines, button}` (`button`: its button's `{x, y, w, h}` on 640 × 480) or nil; presses Space (`"next"`) or Esc (`"close"`) on it; clicks at screen x, y (true: on the button, Next or Close). |
| `lock_of(id)` | The frames fighter `id`'s scripted animation still holds it (`animationLockFrames`; 0: free). |
| `jello_drawn()` / `set_jello(q, on)` | How many see-through triangles the last screenshot drew (and, second, how many were doors'); marks level quad `q` see-through or not, as the camera does (until its next update). |
| `ray(x1, y1, z1, x2, y2, z2)` | The first level quad between two points (any flags): `{quad, at={x,y,z}, flags, object_tag, jello}`, or nil. |
| `animated_textures()` / `particle_textures(class)` | The animated textures the level and its models draw, `{name, frame}` each (as of the last screenshot); the texture (file stem) each live particle of a class shows now. |
| `decals_drawn(x, y, z)` | The decals drawn from that eye now: `{quad, alpha}` each (alpha 0–1, as drawn), in drawing order. |
| `look_mode(on)` / `aim_lr([v])` | Hold the look-mode key; read (or set) the player's look-mode turn (radians). |
| `reload(id)` / `drop_weapon(id [, knock])` / `give_ammo(id, clips [, cells])` | Reload, let go of the gun, add spare clips / cells. |
| `inventory(id)` → `clips, cells, hypos` | What a fighter carries besides guns. |
| `powerup(id, kind [, amount])` | Give a powerup the game's way (`ammo`, `cell`, `hypo`, `shield`, `invis`, `lsi`) → how much it took. |
| `shadow(id)` | Fighter `id`'s shadow this frame: `{alpha, quad, vertices, positions}`, or nil. |
| `frame_ticks(n)` / `time_scale()` / `env_shade_of(ref)` / `obj_shades()` | The ticks a frame runs with n passed on the clock (it owes and catches up in a cutscene); 2 in slow motion, else 1; a level reference's flat shade `{r, g, b}` or nil; each live object's flat shade or false. (`effects(id)` also gives `super`, `super_shield`, `super_particles`, `boss_shield`.) |
| `cheats(on)` / `type_keys(s)` | Allow the cheat codes (as once the game is won); type letters on the pause screen → the last cheat message. |
| `boss_of(id)` | A boss's mode now (`"ShootBeam"`, `"Regenerate"`, …) or nil. |
| `effects(id)` / `magic_drawn()` | A fighter's shield and invisibility: `{shield, shield_shown, shield_active, shield_parts, daodan, invisibility, invisible_for, invis_active, invis_alpha, seen}`; how many magic effects the last screenshot drew. |
| `corpses()` | The level's pre-placed corpses: `{class, x, y, z, drawn}` each (where its pelvis lies; whether its class has a model). |
| `decals([class])` / `quad_plane(q)` / `particles_of(class)` / `health_shade(id)` / `show_decals(on)` | The live decals (of a class), the level's first, oldest first: `class`, `static`, `alpha` (0–255), `vertices`, `triangles`, `positions`, `quad` made on; a level quad's plane (`a, b, c, d`); where each particle of a class is (`{x, y, z, dx, dy, dz [, tint]}`: its forward axis, its colour when a variable); a fighter's health colour; whether screenshots draw decals. |
| `env_particles(tag)` / `particle(tag, command [, event])` / `console_screen(id)` | How many of the level's environmental particles with that tag are alive; send them a command as scripts do; the texture a console's screen shows now. |
| `dialog()` | Whether a scripted dialogue line is on. |
| `saying(id)` | The line a character is saying (an ambient's name), or nil. |
| `sounds()` | The playing ambient sounds (`docs/AUDIO.md`): each `name`, `stage` (`in`, `body`, `stopping`, `out`, `detailonly`), `clip` (the sound playing now), `left` / `right` gains. |
| `sound_objects()` | The level's sound objects, each `ambient`, `x`, `y`, `z`, `box` (else a sphere), `state` (`Off`, `Starting`, `Playing`, `Stopping`), `volume`, `playing`. |
| `impulse(name [, x, y, z [, volume]])` / `channels()` / `sound_volume(v)` | Play an impulse sound (placed or not) → its channel 0–15 or nil; the 16 channels (`clip`, `locked`, `volume` linear, `left` / `right` output gains); set the overall volume to head for. |
| `turret(id, what)` / `turret_state(id)` | Switch turrets by script id (`activate`, `deactivate`, `reset`; −1 every); a turret's state (`active`, `yaw`, `pitch` degrees, `target`, `firing`, `shots`). |
| `hold_fire(on)` / `dodge_of(id)` | Hold fire 1 down; an agent's dodge now (x, z, weight; nil: none). |
| `gun_chance(id, percent)` | Set an agent's chance to go for a gun (its class's, else). |
| `env_show(ref, on)` / `env_broken(from [, to])` | Show / hide a script reference's level part; how many references in the range have all their glass broken. |
| `shots()` / `weapons_lying()` | Live particles; guns lying free. |
| `door(id, what)` / `door_state(id)` | Order a door (`open`, `close`, `lock`, `unlock`, `jam`, `unjam`); its state (`closed` / `opening` / `open` / `closing`) and whether it is locked. |
| `laser(id, what [, factor])` / `laser_state(id)` / `beams()` | Order a laser trigger (`activate`, `deactivate`, `reset`, `hide`, `show`, `speed`); its active / on / triggered state and beams out; every beam now (`{x1,y1,z1,x2,y2,z2}`). |
| `eye_of(id)` | Where an agent's eyes look (degrees; nil: along its body). |
| `set_gamma(g)` | The gamma option for the frames drawn (0–1; 0.5 as drawn). |
| `menu_open([in_game])` / `menu_state()` / `menu_click(id)` / `menu_key(name)` / `menu_close()` | The main menu (`docs/MENU.md`): bring it up; the dialog on top (`dialog`, `depth`, `visible` {id = title}, `rows`, `selected` "level savepoint", `checks`, `sliders`, `popups`); click a control or press a key, getting what the menu asks for (`new_game`, `new_game faithful`, `load L S`, `resume`, `quit`, `options`); take it down. `menu_press(id, f)` holds the left button a fraction `f` across a control, `menu_timer(n)` fires the window manager's timer n times, `menu_release()` lets go; `menu_loading(percent)` shows the level-load dialog alone (screenshots draw it), `menu_loading()` hides it. |
| `skip_cutscene()` / `cutscene_skip()` | Ask to skip the cutscene (as Space does); where a skip is (`idle`, `starting`, `black`, `skipping`, `fading in`) and the screen's black (0–1, nil: none). |
| `look_at(id, x, y, z)` / `look_at(id)` / `aim_off(id)` | Make an agent look at a point, or stop looking; a fighter's aim off its body, left-right and up-down (degrees). |
| `mission_exec(code)` | Defines script functions of the scenario's own (translated-Lua form, `F.name = function() C.command(…) end`) in the running mission; run them with `mission_call`. |
| `cinematics()` | The cutscene portraits showing: `{name, x, y, w, h}` on the 1024 × 768 virtual screen. |
| `overlays_of(id)` | The partial-body clips playing on a fighter: `{slot = recoil / inventory / hit, clip, type, frame}` each. |
| `timer()` | The countdown: what it shows now (nil while blinked off), ticks left, `on` / `fail`; nils with none. |
| `passive_of(id)` / `script_id_of(id)` | Whether an AI is passive (a script, a cutscene); the id the scripts know a character by (the player 0, a `chr_create`d character its setup's; nil otherwise). |
| `neutral_of(id)` | A fighter's neutral interaction (`docs/NEUTRAL.md`): `{behavior, with, line (0-based; nil before talking), accepted, begun, items_given, completed, resume_at}`, or nil without a behaviour or an interaction. |
| `place(name)` / `goals(id)` / `goal_of(id)` | Goals: place one of the level's characters by name (class, position, job, alerts, profiles; returns its id); give a spawned character default goals (idle); what an agent is doing (`job patrol <path> @<waypoint>`, `pursuit goTo`, `combat Melee → 2`, `idle`, …). `nav_state(id)` also reads an agent's current walk. |
| `watch(id)` / `alert_of(id)` / `contact_of(id, other)` / `team(id, name)` / `noise(x, y, z, kind, volume [, maker])` | Perception: make a placed character watch and listen; its alert level; how sure it is of another (`definite` … `forgotten`, nil); set its team; make a noise (`interesting`, `danger`, `melee`, `gunshot`, `ignore`). |
| `technique(id, name)` | Make melee AI `id` use its profile's technique `name` next (once it can). |
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
