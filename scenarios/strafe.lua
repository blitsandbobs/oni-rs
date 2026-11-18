-- Strafe: holding step-left sidesteps sideways while the body keeps facing
-- forward (A/D are stepleft/stepright in Oni, not turn).
--
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/strafe.lua

-- Face +Z (forward); strafe left. Motion should be lateral (X), not forward (Z).
facing(0)
local x0, _, z0 = pos()
strafe(true, false)
step(50)
local x1, y1, z1 = pos()
local dx, dz = x1 - x0, z1 - z0
print(string.format("strafed to %.1f,%.1f,%.1f  dX=%.1f dZ=%.1f  anim=%s",
    x1, y1, z1, dx, dz, anim()))

assert(math.abs(dx) > 2.0, "she travelled sideways, got dX=" .. dx)
assert(math.abs(dx) > math.abs(dz) * 2.0,
    "motion is lateral, not forward: dX=" .. dx .. " dZ=" .. dz)
assert(anim():find("ss") ~= nil, "a sidestep clip is playing, got " .. anim())
-- Body never turned to face the movement — still looking forward.
assert(math.abs(facing_deg()) < 5.0, "body keeps facing forward, got " .. facing_deg())
assert(grounded(), "still on the ground")

cam(x1 + 2, 10, z1 - 14,  x1, 6, z1)
screenshot("/tmp/oni_strafe.png", 760, 900)
