-- Gravity + floor landing (was the --sim drop test).
--
-- Run with the character dropped from 20 units up:
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,20,300 --script scenarios/drop.lua

-- No input: she just falls. 2 seconds is plenty to settle.
step(120)
local x, y, z = pos()
print(string.format("landed at feet %.1f,%.1f,%.1f  grounded=%s", x, y, z, tostring(grounded())))
assert(grounded(), "character settles on the ground")
assert(math.abs(y) < 0.5, "feet rest at floor level (y~0), got " .. y)

-- A full-body shot confirms she's standing on the floor, not through it.
cam(209, 7, 306,  200, 4.5, 300)
screenshot("/tmp/oni_drop.png", 760, 1000)
