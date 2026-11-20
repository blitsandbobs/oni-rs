-- Level 2's Deadly Brain (docs/PARTICLES.md, *Actions*): started, its
-- particles throw holographic readouts that stand around the brain, each
-- spiralled out to its own angle and radius (30–90) from the emitter —
-- `moveSpiral` sets their offset position. Unported, they stayed inside it.
-- Each stands upright facing out, and shows its own frame of the animated
-- readout texture. Its steam and
-- coolant run too.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/deadly_brain.lua

mission_begin(false)
mission_exec([[F.brain_on = function() C.particle("brain", "start") C.particle("brain1", "start") end]])
step(5)
assert(mission_call("brain_on"))
step(180)
local emitter = particles_of("env_squid_e01")[1]
assert(emitter, "the brain's readout emitter runs")
local readouts = particles_of("env_squid_p01")
-- (Bar the one born this tick: placed at its first update.)
local placed, far = 0, 0
for _, q in ipairs(readouts) do
  local d = math.sqrt((q[1] - emitter[1]) ^ 2 + (q[2] - emitter[2]) ^ 2 + (q[3] - emitter[3]) ^ 2)
  if d > 29 then placed = placed + 1 end
  far = math.max(far, d)
end
print(string.format("%d readouts, %d out at their radius, the farthest %.1f", #readouts, placed, far))
-- Each faces out level along its ring (an exact 90° ring round the upright
-- emitter), so its arrow sprite stands upright (`P3rConstructOrientation`:
-- no up to be had towards the emitter, world up).
local level = 0
for _, q in ipairs(readouts) do if math.abs(q[5]) < 0.01 then level = level + 1 end end
print("facing out level:", level, "of", #readouts)
assert(level == #readouts, "every readout faces out level, standing upright")
assert(#readouts > 30, "a cloud of readouts")
assert(placed >= #readouts - 1 and far < 91, "each at its radius, 30–90, around the brain")

-- Each shows one of the readout texture's 5 frames, at random for good
-- (random start, held still): together, a jumble of displays.
local shown = {}
for _, t in ipairs(particle_textures("env_squid_p01")) do shown[t] = (shown[t] or 0) + 1 end
local kinds = 0
for t, n in pairs(shown) do kinds = kinds + 1 print("  frame", t, n) end
print("frames shown:", kinds)
assert(kinds >= 4, "the readouts show the texture's different frames")

-- Its steam and coolant (random-rate emitters: the first emission is due at
-- once, counted from time 0 — from now, they never came round), the steam
-- drifting up (`moveDrift`).
local steam, coolant = particles_of("env_steam3_p01"), particles_of("env_coolant_p01a")
print("steam", #steam, "coolant", #coolant)
assert(#steam > 0 and #coolant > 0, "the brain steams and drips")
print("ok")
