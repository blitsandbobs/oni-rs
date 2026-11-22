# Deviations from the original

Where oni-rs does something other than the shipped game, and why. How the
original works is in its source (`../original/source`); this is only what
differs, plus the data findings a reader can't get from the code.

## 1. Shipped bugs we fix (`--faithful` keeps them)

By default the engine does what the designers evidently intended; with
`--faithful` (play and script mode), the New Game dialog's **Faithful** box
or Lua `faithful(true)` it reproduces the shipped behaviour. Each is one
method on `Quirks` (`crates/oni/src/quirks.rs`).

| Id | Area | Shipped behaviour | Fixed behaviour | Visible for Konoko? |
|----|------|-------------------|-----------------|---------------------|
| Q1 | Jump | A jump with step-right held asks for the *right-landing* animation type instead of the right-jump type (in both the takeoff request and the "already jumping" check), so it never finds a takeoff clip and falls through to a neutral jump. | Asks for the right-jump type. | No — Konoko has no clip of either type for a standing takeoff; her sideways jumps come from jumping *while sidestepping*, which reaches the sideways takeoff through shortcuts and is unaffected. Matters for classes with a right-jump clip. |
| Q2 | Stun | Extending a stagger when the current clip is no longer a stagger reads an uninitialised result (undefined). | The stagger ends (no clip requested). Reproducing undefined behaviour is not meaningful, so `--faithful` does the same. | Rarely — a stagger stun outliving its stagger clip (e.g. knocked down mid-stagger). |
| Q3 | Blocking | The "is this character defensive?" test checks the current clip's start state twice where it seemingly meant to check its end state too. | **Kept as shipped in both modes**: checking the end state would stop a character who is already blocking from blocking the next hit (block clips end in a blocking state), so the slip is load-bearing. | Yes — repeated blocks work. |
| Q4 | Reactions | Upgrading a reaction to a knockdown (airborne victim, or a heavy hit on a staggered one) sends a knockdown that is already a body or foot knockdown to the *head* knockdown. | Knockdowns keep their own type. | Rarely: only body/foot knockdowns that get upgraded. |
| Q5 | Melee AI | A technique's *fearless* flag (ignore danger) is tested against the bits holding its start state, so "fearless" really means the technique starts prone, from a run start, running back/left… (start states 4–7, 12–15). | The technique's own flag. | Near ledges: which techniques dare a run-in toward a drop. |
| Q6 | Melee AI | Throws against a target that can't be thrown (knockdown-resistant, invulnerable) keep their weight when the AI chooses; it only finds out at the last moment and gives up. | Such throws aren't chosen. | Against resistant classes: wasted throw attempts. |
| Q7 | Melee AI | The run-in distance planned before an attack subtracts the target's radius twice. | Once. | Run-ins stop a little short in the shipped game. |
| Q8 | Melee AI | Hit prediction's body-contact step counts the target's height offset twice when deciding whether two bodies overlap vertically. | Once. | Only for targets standing higher or lower than the attacker. |
| Q9 | Perception | The vision cone's vertical limit is stored as a cosine but tested against the elevation's *sine*, so an authored 40° allows up to 50° above or below (and the editor draws it as authored). | The authored angle. | AIs on other floors: a little more (shipped) or less (fixed) up and down sight. |
| Q10 | Goals | Going to investigate a contact, the AI was meant to check its view of the spot every 15 ticks, but the check's timer is never moved on: it checks every tick, so its "six clear views" take 6 ticks. | Every 15 ticks (≈ 1.5 s of clear view). | Investigators stop up to 60 units short of a noise (shipped) or walk closer (fixed). |
| Q11 | Goals | Losing sight of its combat target, the AI arms the class's lost-contact delay (180 ticks) — then a debug `if (1)` pursues on the very next tick anyway. | **Kept as shipped in both modes** (decided 2026-11-20): the game's fights were balanced on it, and the level 1 training's combat demo relies on it — with the delay, a knocked-down ninjabot still fighting takes up Konoko on her platform, ends on a ledge no one can reach, and the demo stalls (#62). Marked `SHIPPED BALANCE` in `ai/goals.rs`. | Yes — breaking line of sight sends fighters hunting at once. |
| Q12 | Goals | When its combat target dies, the AI's search for another target always finds none (perception bug 3), so it never hands over to another enemy. | It takes up the best other living threat (fights a seen one, pursues a heard one). | Fights against several enemies. |
| — | Melee AI | The melee profile's own daze times are never read; the class's are (a header comment says the profile overrides). | **Kept as shipped in both modes**: the profiles carry mostly the editor default (60–100 ticks), while the classes' values are tuned per character (Konoko 6–24, most soldiers 96–138 or 132–168) — the slip is what the game was balanced with. | — |
| — | AI shooting | The fire check reads its near/far blend the wrong way round: the strict aiming cone and the class's best aiming angle apply only nose to nose (inside 10 units), and every target beyond 25 units gets four times the tolerance. | **Kept as shipped in both modes** (decided 2026-11-19): it *is* the game's AI accuracy — distant enemies fire whenever the gun is roughly on target — and the game was balanced on it. Marked `SHIPPED BALANCE` in `ai/ranged.rs`. | Yes — how often and how well enemies shoot at range. |
| — | Perception | The periodic reminder of each AI's most important contact (`AI2rKnowledge_Update`'s last loop) tests and notifies a stale character (`looking_character`, left by the vision loop, for `update_character`), and its time test is inverted (it skips contacts *not* notified for 60 ticks): in effect nobody is reminded. The Daodan DLL's `characterawareness` fixes the pointer only, so there every AI with a contact is re-notified every tick. | **Kept as shipped in both modes** (2026-11-21, #26): the reminder as meant (every 60 ticks) re-runs pursuit's handler with news of its target, which sends the AI back to the same spot and restarts its look-around each second — investigators stare instead of searching, and a pursuit lasts as long as the memory. Tried and reverted: `goals.lua`'s look-about fails. | Yes — how long AIs keep after a player they lost. |
| Q24 | AI dodging | The projectile check measures a projectile's closest approach from the world's origin instead of from the character (`AI2rKnowledge_FindNearbyProjectiles` hands the world point to `AI2rManeuver_NotifyNearbyProjectile`), so its weight is almost always far below zero: AIs effectively never dodge projectiles (they do dodge guns' firing cones). The community's fix (the Daodan DLL's `projaware`, on in the AE) meant to pass the body-relative point but, its stack offsets off, passes memory that reads as (0, 0, 0) for both the distance and the direction (OniGalore, *AE:Projectile awareness*). | As the Daodan plays it, since that is what today's players meet: every projectile on course, seen past no wall, is a direct hit at full weight (radius / 10, at most 0.8), and the AI sidesteps square to its path, to a side with room. Decided 2026-11-21 (#26), replacing "kept as shipped"; the body-relative version first tried was dropped for the Daodan's. | Yes — enemies sidestep Super Balls, rockets and bolts they see coming, even ones that would have missed. |
| — | Turrets | The aim is reckoned from the base plus the head and barrel pivots without the mount's rotation, so an upside-down (ceiling, flipped wall) turret aims some 8 units high and fires up close only when its tolerance grows. Also: `turret_deactivate -1` switches no turret, and the background effect stays dark after a deactivation. | **Kept as shipped in both modes** (shipped balance; the aim offset is worked out from the code, not yet seen in the game). Marked `SHIPPED BALANCE` in `world/turrets.rs`. | Yes — ceiling turrets on levels 8, 14, 18. |
| — | Sound | Per-play volume and pitch, detail timing and detail placement draw from the gameplay random numbers, so every sound played shifts later gameplay rolls. | **Sound has its own random numbers in both modes** (decided 2026-11-19: the port has no random-exact replay to keep). The other sound oddities (the never-set channel priority, a stolen ambient's silent last pass, the alt impulse keeping the first one's volume) are kept as shipped (§4). | No — invisible except as different gameplay rolls. |
| Q14 | Doors | Deciding whether a door is blocked, the check meant for each leaf tests the first leaf twice: someone in the way of only a double door's second leaf pauses it (both leaves) without the door ever counting it as blocked. | Either leaf counts: blocked 90 ticks, the door turns back. | A double door closing on someone stands half-shut until they move (shipped) or reopens for them (fixed). |
| Q15 | Melee AI | Hit prediction reckons a colliding target's velocity from its actual movement turned into its own frame, but the sideways part uses its x movement twice (`dx` in place of `dz`). | The movement turned properly. (A target not colliding moves as its animation does, in both modes.) | Slightly off predictions against someone sliding along a wall or another character. |
| Q16 | Camera | Deciding which walls between the camera and the player go see-through, the "all on one side" test reads the feet's side twice where it meant the head's. | Camera, feet and head all compared. | A wall between the camera and the player's head but not their feet stays solid (shipped). |
| Q17 | Camera | Deciding which placed objects are in the camera's way, the point-and-ray test hands its ray's *vector* to a segment test as the far *point*: the segment runs from the camera toward the world's origin. | The segment from the camera to the point beside the player's head. | Objects between the camera and the player stay solid (shipped) — the camera's view of Konoko blocked by a crate or a van's door. |
| Q18 | Cutscenes | Skipping a cutscene is all built — the fade to black, the sound off, the game run 32 ticks a frame unseen, the fade back — but its one trigger, Space, is commented out: cutscenes can't be skipped. | Space skips a cutscene (from 10 ticks into it). | Every cutscene sat through (shipped) or skippable (fixed). |
| Q19 | Menus | The options' Invert Mouse box is ticked for inverted (`ONrPersist_SetInvertMouseOn` takes the check), but the dialog data titles it "Off" (Subtitles' box says "On"): ticked, it reads "Off". | Titled "On", as Subtitles. | The options screen. |
| Q20 | Teleports | `ONrCharacter_Invis_Update` fades a teleporting character out over its teleport-in clip, and was to fade it back in over the teleport-out — but that branch tests the teleport-in type again, so it never runs (and the fade it would apply is capped by the alpha the fade-out left). | It fades back in over the teleport-out clip, up to the clip's action frame. | The super ninja (levels 11, 12) appears at once where he teleports to (shipped) or fades in there (fixed). |
| Q23 | Weapons | Letting go of a gun (`WPrRelease`) zeroes its fire delay and reload delay, so dropping it and picking it up again skips them — the community's "cooldown exploit" (Mercury Bow: fire, drop, pick up, fire; fixed by the Daodan DLL's `cooldowntimer`). | A dropped gun keeps what is left of both delays; they run on once it is in a hand again. | Yes — with the slow guns (Mercury Bow, Scram Cannon). |
| Q25 | Throws | The throw search (`AttemptThrow` → `ONiCharacter_FindNearestOpponent`: within 24 and 45°) never looks for a wall between: someone on the other side of a thin wall or a shut door can be thrown through it (a community-documented glitch; an optional fix in FPSDaodan). | Anyone behind a wall bodies collide with (pelvis to pelvis) isn't a candidate. | Yes — at doors and thin walls. |
| Q26 | Weapons | A reload takes its clip as it starts (`ONrCharacter_Reload` → `WPrPowerup_Use`) and fills the gun only at the reload clip's action frame (`WPrNotifyReload`); a knockdown in between cancels the reload and the clip is lost (community-documented; fixed in FPSDaodan). | The cancelled reload gives the clip back. | Yes — knocked down mid-reload. |
| Q27 | Cutscenes | The cutscene inserts (portraits, name tags) are placed on a 1024 × 768 virtual screen scaled to the window on each axis alone: on a wide screen they stretch (the original knew only 4:3 modes; the Daodan DLL's `widescreenportraits` narrows them). | Each keeps its shape (the smaller scale both ways), pinned by the edge or middle its position aligns it to. | Yes — every cutscene portrait on a wide screen. |
| Q28 | Lens flares | A fading lens flare is faded by the fog at its screen depth (`P3rModifyAlphaByFogFromDepth`), but `gl_calculate_fog_factor` returns 1 past the fog's end where its linear ramp reaches 0: a light beyond the fog shines on whole. | Gone beyond the fog's end, as the ramp it follows. | In fog-heavy places: distant lights no longer burn through the fog. |
| Q29 | Pathfinding | A placed object is stamped into the pathfinding grid (`PHrAddObjectToGrid`) as the squares round its sphere, but the row loop runs `for (v = radius; v <= radius; v++)`: only the square's last row is stamped, so paths run through objects and AIs walk into them. | The whole square. | Yes — AIs path round forklifts, trucks and crates instead of into them. |
| Q30 | AI turning | Turning its body, the AI asks for a step-turn clip (`pleaseTurnLeft` / `pleaseTurnRight`, played by a standing body) with the sign the other way round from the movement system's: `AI2rExecutor_Update` asks for the left turn when its facing falls, where `HandleStandingTurn` turns left for a rising one (facing +Z, +X is the left). | The clip of the way it turns. | Cosmetic: a standing AI's feet shuffle against its turn (shipped). |
| Q22 | Melee AI | Weighing a rifle disarm throw, `AI2iMelee_WeightTechnique` tests that the target holds a *pistol* (`!has_pistol`, copied from the pistol branch); the throw then aborts, the target having no rifle out. | It tests for a rifle. | AIs disarm only pistols (shipped) or rifles too (fixed). |
| Q21 | Corpses | Every level's corpse array (`max_corpses`) holds 20 bodies, its pre-placed ones among them (level 3 starts with 17): past that the oldest of the dead of play is recycled — a body vanishes — so on busy levels bodies disappear almost at once. | The array holds 50. | Bodies left lying in a fight's aftermath (shipped: at most 20 in the level). |
| Q13 | Corpses | A body killed in the air keeps its flight in its own frame but drifts as if that were the world's (the flight unturned). | The flight turned by the body's facing. | Bodies knocked out of the air drift the wrong way. |
| — | Character collision | Each pass takes off the push into only the nearest plane (the player gets two passes, an AI one, then a halt): at an obtuse inside corner the player turns it and an AI stops dead. Characters stop short of walls by up to a tick's motion, are never pushed out of what they are put into, and a quad's edge blocks with its face's plane. Falling isn't swept against walls; the floor pass lifts onto any floor under the 3×3 square up to half the body, so characters stand 1.5 past edges and a little above slopes. | **Kept as shipped in both modes**: it is how the game moves. | Yes — everywhere. |
| — | Decals | A decal's drawn opacity is doubled (anything half-opaque or more draws fully opaque; a fade shows only in its second half), its tint is never seen, it wraps round corners by the shot's direction rather than the wall's normal, and a class's decal count includes the level's own painted ones (which fade first and never return). | **Kept as shipped in both modes**: it is how the game looks. The rest: §4. | Yes — bullet holes, burns, level decals. |
| — | Placed objects | An object's collision sphere radius comes from a buggy test (the last coordinate it lets through, spec quirk 16); `env_setanim_block` with a clip still playing does nothing; `env_anim_block` waits only when given a second id. | **Kept as shipped in both modes**: the radius the code meant (the largest coordinate) would put invisible walls tens of units round long objects. | Yes — what stops characters round forklifts and trucks. |
| — | chr_talk | Given a speech type, `chr_talk` checks the name but never stores it: the line goes out with an uninitialised priority. | The named type is used. Undefined behaviour isn't reproducible, so `--faithful` does the same. | Rarely — one use in the shipped scripts (level 2). |

Not reproduced yet (the path isn't modelled): perception's list-order slips
(a new contact jumps to the top until the list is re-sorted; the visibility
lookup assumes the order) —
we keep the list ranked; a hit lowering a forced target's priority; the
alert line playing once per life (no voices yet); the swapped pistol / rifle
disarm flags (no weapons yet, #10); the hit prediction reading the
*attacker's* jump key for an airborne target (targets' flight isn't
predicted); a stale miss reason when the predicted clip runs out.

Kept in both modes (the goals): pursuit's *hunt* and *move*
modes stand still (never implemented); pursuit's "seen" / "unseen" settings
are picked by alert ≥ medium, not sight; a moving scan's timer running out
skips whatever patrol waypoint is current; a *stop* waypoint leaves a resumed
patrol stopped; idle characters never walk home. Pursuit's after-arrival check
without a job location reads an unset flag in the original — we pursue.

Not quirks (we don't model the affected path): a "dead → knockdown" upgrade
whose result is discarded, and an uninitialised fallback flag in the knockdown
clip search; both are inert in the shipped game too.

Community reports checked against the source and **not** reproduced as
quirks (#26):
- *A gun's minimum shooting distance acts as half* (ONWC): designed — the
  too-close weight is 1 − distance / minimum, and the AI shoots freely while
  it is under 0.5 (`AI2rTargeting_Update`, `AI2iCombat_Update`).
- *Long and Medium combat ranges are used only as an encounter starts*:
  designed — once it has been at Short range, an encounter's Medium and Long
  become their Retreat kinds for good (`AI2rCombat_GetRange`); the port
  follows it.
- *The airborne prediction uses FallGravity*: the melee prediction
  (`BFW_Totoro.c`) takes FallGravity until a body counts as jumped, though
  real flight always uses JumpGravity. The port predicts with JumpGravity:
  the same for a jumping attacker; only an attack while falling off a ledge
  differs.
- *Vision breaks pursuit; the Look timer never runs down*: Look has no timer
  of its own — after the first look the pursuit settings pick the next mode
  and Look again just goes on — part of the unfinished pursuit (with Hunt and
  Move, kept above).
- *Knocked out the wrong way by an attack without the knockdown flag*: no
  fault found — an upgraded hit is turned to its from-behind kind by the
  attacker's side afterwards (`ONiRemapAnimType_For_Direction`). Needs a
  reproduction.
- *Using a console starts an attack; throw chaining; a throw's wrong target
  clip* (fixed in the FPSDaodan fork): no mechanism documented; need a
  reproduction.
- Original bugs with no effect: Emit Particles ignores its count's fraction
  (reads the emitter enum; no shipped class has a fraction); `wp_fadetime`
  is never read (no script sets it); the Win32 polling input reads the
  middle mouse button as Shift (`MK_MBUTTON` = `VK_SHIFT`; not the port's
  input).
- Options hidden over a running game (Daodan `optionsvisible`): by design —
  Resume takes its place in the dialog.

**Candidate, left as shipped in both modes:**
- Fall damage is measured from the takeoff height to the landing height, not
  from the apex — a high jump onto a lower floor counts only the net drop.
  This may be intentional; left as shipped in both modes.

## 2. Level-script fixes (`--faithful` skips them)

**How it works.** Each patched level has a file
`crates/oni/src/mission/fixes/NN.lua`. It runs after the level's
translated scripts (`levels/NN/scripts.lua`) and replaces script functions by
name with `FIX(id, name, make)` (`mission/prelude.lua`). `make` gets the
original function and returns its replacement, which usually adds a step
before or after the original. Fixes never copy the scripts' text.

- If a fix names a function the scripts don't have, that is a script error:
  the fix has drifted from the scripts.
- `CHR_AT(name)` and `PLAYER_AT()` (feet x, y, z) are there for the fixes.
- With `--faithful` (`Quirks::scripts_as_shipped`) no fix runs.

**Sources.** The leads came from the IBB Fixes mod's changelog (OniGalore,
*IBB Fixes*) and the AE's own patched scripts. Only the Anniversary Edition
(AE) intro and manplant widescreen edits differ from vanilla, and none of
them is a gameplay fix. Every fix was checked in the vanilla scripts and
written from our own reading. The patches are not taken from the mod.

### Fixed

| Id | Level | Shipped | Fixed | Scenario |
|----|-------|---------|-------|----------|
| S1 | 02 manplant | The Deadly Brain's cutscene starts when Konoko leaves the volume in the room's antechamber. It fires whichever side she leaves by, and the volume's far side is at door 26. The save that follows (`save_point_4`) locks door 26 without closing it, wherever she is. If she runs back out right after the cutscene, she is locked out, and the save is made with her outside, so reloading keeps her there. | The save waits until she is back in the antechamber, then closes door 26 and locks it. | `fix_brain_lockout.lua` |
| S2 | 04 airport | Restoring save point 3 leaves the tarmac's volumes live (`tarmac`, `bomberboom`, `madbomberbait`); restoring save point 2 turns them off. Going back, the tarmac's guards spawn again, the bomber's blast replays, and the tarmac volume saves the game at save point 2 again. | Restoring save point 3 turns them off, as save point 2 does. | `fix_tarmac_respawn.lua` |
| S3 | 18 tctf_ii | Past door 14, a volume (`t45`) locks the door behind Konoko whether or not it is closed. If it is still open, she can walk back out and be locked out. | The door closes, then locks. | `fix_door14_lock.lua` |
| S4 | 02 manplant | Restoring save point 4 calls `brain_counter` once, taking one console off the count, so the brain's next round ends after 3 of its 4 consoles. | All 4 are needed again. | `fix_brain_consoles.lua` |
| S5 | 12 roof | Mukade, knocked off the roof's open north end, lands on a lower roof that is still above the height (550) at which his AI brings him back. He can't climb back up. His teleport in behind his enemy needs a roll that is kept until he next reaches melee, so if the roll goes against him, or Konoko stays within 80, he never comes back. The fight never ends, and it is the level's end. The community's mods add a volume that teleports him back. | Stranded off the roof for 3 seconds, he teleports back to its centre (flag 96), with his teleport clips. | `fix_mukade_stranded.lua` |

### Checked, not fixed

- **tctf `ta2_54t`, the welder's volume (level 8).** It spawns the welder, the
  level's only source of the LSI the elevator needs, so missing it looked
  like a soft-lock. It isn't one:
  - The top floor's one walkable way in is the ramp off the atrium bridge.
    The volume fills that doorway's whole width, and it spans the corridor
    between the welder's room (south) and the elevator (north). Konoko
    hugging the doorway's wall sets it off.
  - A deliberate running jump from the ramp's south edge clears the
    corridor's 9-unit atrium railing just south of the volume (Konoko's jump
    rises 18). She is then in the corridor with the welder not yet spawned.
    The original tests a volume the same way (feet-to-head segment), so this
    holds there too.
  - Even then, every way north to the elevator passes through the volume,
    and the corridor's east side is closed. The welder spawns before the
    elevator can be used; the cost is a detour back to his room.

- **The four save-point volumes.** Each is the only thing that writes its
  level's save point, and missing one only costs that autosave. Each script
  just saves; manplant's also spawns one striker on entry. None is glaring.
  Checked with plan views of the collision geometry, the room graph cut at
  the volume, and walking probes:
  - **tctf_ii `trigger_volume_01` (level 18): can't be missed.** It fills the
    corner landing where two stair flights meet.
  - **power `trigger_volume_30` (level 9): can be missed.** It fills its room
    but stops 13 units short of the north end. That end has a 9-unit gap in
    the west wall, so Konoko can walk from the east corridor into the west
    rooms without touching the volume. Walked in a probe.
  - **warehouse `tv78` (level 1): can be missed.** The strip lines the east
    side of the hall's dividing wall at the big door (48). The hall's two
    halves also connect the long way round: through the double doors at
    either end and the side doors in the dividing wall at z ±590, none of
    them locked. Taking that way also skips `mid_warehouse_target`, the
    3-minute escape timer. The level is won through a different volume
    (`trigger_volume_70`), so this can't block the ending. The detour comes
    from the room graph and the door states; it hasn't been walked.
  - **manplant `tech_ambush_1` (level 2): not settled.** The volume covers
    the whole doorway (door 11, which the scripts open after the hall
    fight). The other ways into the corridor beyond go through doors 12 and
    13, which the level locks at its start and never opens. The room graph
    does reach save point 3's area without that corridor, through door 4,
    which a console opens. Whether that way is open at that point in the
    level needs a playthrough.

- **power_II `setobjective_5` (level 10): can't be missed.** It covers the
  passage off its landing. The only uncovered part is a 2-unit strip against
  the passage wall, too narrow for a body. Missing it would only leave the
  objective text at 4.

### Minor, not fixed

- **Confirmed but minor.** None of these breaks progress:
  - airport_III's rappel can be reached before the roof fight its volume
    means to gate;
  - the warehouse compass sequence can play twice;
  - airport_III's tripwire repeats its alarm order;
  - both Muro bait volumes can fire, and `muro_bait` calls a missing
    `save_game_2`;
  - compound `trigger_volume_33` is live from the level's start;
  - manplant save point 3 saves silently (its `autosave` is on the next
    line's `door_lock`);
  - a tctf_ii striker can stay ignoring the player;
  - roof `t15` and `t17` fire again on re-entry;
  - two inside scripts only spam the console.

## 3. Port choices

What we do differently on purpose — simplifications, approximations, and
our own additions. Not affected by `--faithful`.

**Simulation**
- Our route follower turns on the spot when the next point is over 90° off
  or inside its turning circle (worked out from the generic turn rate and
  Konoko's speeds, not the class's), so characters don't orbit corners; the
  original slows to a walk when it would overturn.
- Avoidance leaves out the movement modifiers the original also steers
  round; the combat advance has no hazard cut-off (dodges steer separately)
  and tests blockage against the level only.
- Pathfinding plans the whole path at once (the original plans a room's grid
  on entering it), without the reuse cache or octree room lookup: so every
  room's obstructions on the way are read as they stand when the path is
  planned. Doors a character may not open also shut their portals to it.
  An AI standing inside an obstruction isn't moved out of it, and a body met
  while moving isn't passed through (`AI2rTryToPathfindThrough` at
  collision time).
- A melee AI lying down that has picked a technique whose positioning
  starts with getting up doesn't first wait to face its target
  (`AI2cMeleeStandingTurnAngle`: "turn on the spot"): a body on the ground
  can't turn (`ONrCharacter_IsUnableToTurn`), and at the first position step
  the original has no timer, so it would lie there for good.
- Melee look-ahead on levels without nav data: danger is a floor drop beyond
  the class's fall-damage height, found by a collision probe.
- Placed characters have no script id (found by name); the player is 0;
  unnamed setups are `ai_<n>`. A brainless passive fighter gets up after
  1.5 s (stands in for the daze timer); passive dummies deal no hits.
  `Brain::Spar` remains for a class with no melee profile (none ship).
- Equally specific animation picks are seeded, so runs repeat.
- Laser beams are simulated each tick against each body part's bounding
  sphere; the original tests them while drawing, against boxes. Contrails'
  broken links, too, are found each tick, not while drawing.
- Detail levels aren't modelled: the particle system and glass always run at
  high detail; the Quality slider is frozen at high and greyed (#37). Lower
  settings would only remove content (58 effect classes at medium, 202 at
  low, including the Scram Cannon's damaging debris).
- There is no body LOD: a body's particle and shield parts come from the
  drawn (high) body, where the original uses the lowest; a fading invisible
  body is drawn at normal detail; the corpse ring's low-detail hand-off isn't
  modelled.
- Overlay quaternions are the conjugates of the original's:
  `MUrQuat_Mul(layer, body)` is `body · layer` here (#47).
- Our cheats spawn no guns; `forcefield` (a full shield belt) is ours;
  toggles reset each level (the original keeps regeneration);
  `ONI_CHEATS=1` allows cheats before the game is won.

**Rendering**
- Lens flares: a frustum test widened by one octree leaf (16) with a far
  cut, instead of the camera's octree visibility rays. Flares hidden behind
  characters is our option (#58, on by default; untick *Hide Lens Flares*
  for the shipped look).
- A turret's idle effect counts as "drawn" when in the camera's view (the
  original asks whether its octree nodes were seen).
- No dynamic lighting: `obj_shade` / `env_shade` replace white;
  `obj_force_draw` does nothing (nothing is culled). The reticle is fogged
  (#65, won't fix). Gamma is a final pass on the frame, HUD included (the
  original set the display's ramp).
- Magic shells mix in linear light (computed in display values, converted to
  our sRGB frame).
- Moving door leaves are the door's closed-leaf level triangles moved with
  each leaf; a double door's triangles go to the nearer leaf origin.
- The sky draws its box only (no planets, stars or sky flare; no visible data
  needs them), with clamp-to-edge seams.
- The HUD is laid out in the window's logical pixels.

**Audio**
- Volume follows the PC (DirectSound) law, −33.3·(1−v) dB, silent at 0.05
  and below; `--sound-law mac` gives the Mac build's linear law. A volume
  above 1 is clamped (DirectSound rejected it and kept the old one).
- The mix runs at 44.1 kHz with 16-bit, resampled clips; the original mixed
  at 22.05 kHz (nothing above ~11 kHz). No matching low-pass. The 47 mono
  44.1 kHz `zap*` sounds play at their own rate (as the source reads, PC
  would play them an octave low; the Mac importer resampled them).
- Everything the original timed per frame or in wall-clock sixtieths
  (fades, detail timers, box curves, occlusion slew, earshot, flybys) runs
  per 60 Hz tick; dialogue and its waits run on game ticks, so a cutscene
  skip runs through them too. Ambient chaining is queued by the simulation;
  the audio callback only renders. Headless runs render a tick of audio per
  tick; with no device, play is silent.

**Front-end**
- `oni` with no arguments opens the menu; assets come from
  `$ONI_ROOT`, else a `generated/oni` here or above. Command-line level
  launches stay as the developers' way in (Esc opens the menu there too).
- The New Game dialog has our **Faithful** box. Subtitles are off by
  default (`--subtitles`). Resolution is kept but unused (the window sizes
  freely). Settings live in `$ONI_SAVE_DIR/settings.json` (or
  `~/.oni-rs/`), apart from the save; continues in `continues.json` there
  (in memory for scripted runs). `--unlock-all`, `--save-point N`,
  `--outro` are ours.
- The mouse is grabbed only while focused; the focusing click doesn't
  punch. A popup's title is cut on one line (the original wraps it).
- The movie's sound has its own mixer; a game begun from the command line
  closes after the outro.

**Level scripts** — the BSL interpreter bugs we don't reproduce (no shipped
script relies on them): an operator reducing at most one pending operator,
no short-circuit, assignments in a skipped branch after a local `var`,
`return` in a skipped branch, a trailing `#` comment joining the next line,
string `+`, the 6-frame call limit and the 32 KB handler-name buffer. Only
the warehouse's `need_ammo` meets the skipped-branch bug, harmlessly.

## 4. Kept as shipped (not quirks)

Behaviours that look like our bugs but are the original's, kept in both
modes:
- **Bosses:** Mutant Muro's shield eases between updates (0.7–0.94); his
  first thunderbolt waits 5 s; Barabbas never advances without grenades;
  the research lab's unkillable Barabbas has one regeneration pool worth his
  whole health.
- **Camera:** the look direction (sin yaw, sin pitch, cos yaw) isn't
  normalized; the steep-up swing uses (pitch−0.9)×(π/2−0.9); the canter
  shrinks by ×(1.2−share). Invert mouse is on by default.
- **Fog:** a direct `gl_fog_*` set leaves a running ramp going;
  `gl_fog_end_changeto` steps by the start's step.
- **HUD:** a flashing shield's after-shadow gets a wrapped opacity; an
  integer division leaves cutscene portraits at scale 1; the first-match
  subtitle lookup shows `…_force_field` for `…_fields`.
- **Particles:** a two-sided neighbour quad flips its plane per corner; a
  decal on a quad not gathered is laid from the nearest; a round into the
  back of a one-sided quad holes the far side; the burst action floors a
  fractional count; a bounce ignores glancing elasticity; a hit and the same
  particle's blast both damage the character struck; "velocity changed" is
  never cleared; an action mask affects only the update list; level
  collision ignores a particle's radius; `moveSpiral`'s sine is negative
  below 180°; a random or attractor emitter's first emission is due at once.
- **Sound:** an avoid-repeat group keeps its 10th draw; the weighted pick
  favours the first permutation by one; a sound found out of earshot has
  already taken a channel; a fade slower than 0.001 a tick never moves;
  `sound_*_volume` hits the first slot of that name, even a finished one;
  script ambients stay Highest priority and sound objects Low for good;
  re-entering a box mid-fade jumps the volume; AI speech starts at the eye;
  `sound_dialog_play` drops a new line while one plays; a stolen script
  ambient ends for good.
- **Original AI bugs noted, not changed:** "run for alarm" never remembers
  it tried; the alarm keeps checking contacts after deciding to fight;
  entering combat without a target half sets it up; the vision delay timer
  doesn't suppress the normal checks.

## 5. Data findings

What the shipped data holds that the code won't tell you:
- **Missing:** 85 sounds the data names but doesn't ship, including
  `pause_screen_open/close`, `button_click`, `door_success`, `door_unlock`,
  `health_full` and the timer sounds (so the countdown is silent); 4 door
  textures; level 4's sky `sky13`; level 8's `BOSS01menacing`; `intro.bik`
  (no release ships it).
- **Oddities:** level 6's `d_RW_SPRK` decal faces away from its wall (never
  laid); `w7_scc` on unbreakable glass names its crack as a plain particle;
  three impact entries name flash classes that don't exist; level 2's
  scripts name Shinatama's lines with a capital her sounds lack, and the
  hash-bucket lookup (`AUrString_GetHashValue` masks the wrong bits) misses
  some; laser ids aren't unique (level 2 reuses them); the Deadly Brain's
  2-second lines carry 3 seconds of subtitles; `sunset_sun` is 1×1 (the sun
  is painted into `sunset_back`).
- **Never used, so not ported:** the `changeClass` particle action,
  `OBcFlags_FaceCollision` on placed objects (one sphere per object is
  exact), keys (the 4 keyed doors open by script or one-way), the
  draw-reticle-over-walls weapon flag, diary move diagrams, sky stars and
  flares, door event lists on level 1.
- **Shapes:** Konoko's standing combat clips are in the `KONCOM*`
  collection (load the full chain); the next-level chain is 1, 2, 3, 4, 6,
  8, 9, 10, 11, 12, 13, 14, 18, 19; level 2's locked door 3 gates room 3→4
  one way only; the 45 melee profiles live only in `level0_Final`; combat
  settings in level 0's `CMBT` objects; the vanilla BSL (AE package
  `05100Vanilla_BSL`) is the scripts' source — AE patched compound and
  manplant.
