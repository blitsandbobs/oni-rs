-- Films (scripts' `playback`): a recorded performance drives a character —
-- it is put at the film's start, its controls come from the recording, and
-- it stops when the film ends.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/film.lua

step(5)
-- Level 1's outro has Konoko walk off: 223 ticks of recorded walking.
local n = play_film(0, "OutroKonokoWalk")
assert(n == 223, "the film is found")
local x0, _, z0 = pos()
print(string.format("starts at %.0f %.0f", x0, z0))
assert(math.abs(x0 - 48) < 2 and math.abs(z0 - 310) < 2, "put at the film's start")
local walked = false
for _ = 1, 120 do
    step(1)
    walked = walked or anim():find("walk") ~= nil
end
local x1, _, z1 = pos()
local d = math.sqrt((x1 - x0) ^ 2 + (z1 - z0) ^ 2)
print(string.format("walked %.0f units", d))
assert(walked and d > 15, "the recording walks her")
step(150)
local x2, _, z2 = pos()
step(60)
local x3, _, z3 = pos()
assert(math.abs(x3 - x2) + math.abs(z3 - z2) < 0.5, "the film is over: she stands")
print("film OK")
