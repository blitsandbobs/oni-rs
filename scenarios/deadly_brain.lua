-- Level 2's Deadly Brain (docs/PARTICLES.md, *Actions*): started, its
-- particles throw holographic readouts that stand around the brain, each
-- spiralled out to its own angle and radius (30–90) from the emitter —
-- `moveSpiral` sets their offset position. Unported, they stayed inside it.
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
assert(#readouts > 30, "a cloud of readouts")
assert(placed >= #readouts - 1 and far < 91, "each at its radius, 30–90, around the brain")
print("ok")
