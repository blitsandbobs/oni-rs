-- Stairs (level geometry flags; oni-convert's convert_environment): Oni's stairs are
-- steps drawn but not collided with, over a ramp characters walk — Konoko
-- runs straight up a flight in the warehouse, and an AI's path climbs it.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/stairs.lua

step(5)
teleport(0, 229, 52, 552)
facing(180)
step(30)
input(true, false)
step(90)
input(false, false)
local _, y, z = pos()
print(string.format("ran up to y %.1f (z %.0f)", y, z))
assert(y > 62, "she runs up the steps")

-- Level 1's comguy runs from its post up these stairs to the alarm
-- console on the floor above (issue #32: it used to stop at the first step).
teleport(0, 200, 8, 300)
local c = place("Bay4_Comguy_1")
step(30)
alarm(c, 6)
local took
for t = 1, 1500 do
    step(1)
    if console_state(6) == "triggered" then took = t break end
end
print("comguy raised the alarm upstairs after", took)
assert(took, "it climbs the stairs to the console")
print("stairs OK")
