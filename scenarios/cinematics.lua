-- Cutscene portraits: cinematic_start
-- slides a framed picture from one screen position to another (off the
-- right edge to the top right, inset 20 × 65), cinematic_stop slides it away
-- and removes it, a size of −1 takes the texture's own, a texture that
-- doesn't exist shows nothing, and end_cutscene takes them all.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/cinematics.lua

mission_begin(false)
mission_exec([[
F.c_in = function() C.cinematic_start("KONnametag", 180, 180, 16, 3, 20, false) end
F.c_out = function() C.cinematic_stop("KONnametag", 16, 30) end
F.c_natural = function() C.cinematic_start("KONlistening", -1, -1, 5, 5, 0, true) end
F.c_missing = function() C.cinematic_start("no_such_texture", 180, 180, 1, 1, 0, false) end
F.c_end = function() C.begin_cutscene() C.end_cutscene() end
]])
local function run(f) assert(mission_call(f), f) step(1) end
local function find(name) for _, c in ipairs(cinematics()) do if c.name == name then return c end end end
step(5)

run("c_in")
local c = find("KONnametag")
print(string.format("starts at %.0f, %.0f (%dx%d)", c.x, c.y, c.w, c.h))
assert(c.x >= 1024, "starts off the right edge")
assert(c.w == 180 and c.h == 180, "its size, as given (the original's 640-scale factor is 1)")
step(60)
c = find("KONnametag")
print(string.format("slid to %.0f, %.0f", c.x, c.y))
assert(c.x == 1024 - 20 - 180 and c.y == 65, "at the top right, inset 20 × 65")

run("c_out")
step(60)
print("after stopping:", find("KONnametag") and "still there" or "gone")
assert(find("KONnametag") == nil, "slid away and gone")

run("c_natural")
c = find("KONlistening")
print(string.format("own size: %dx%d, centred at %.0f, %.0f", c.w, c.h, c.x, c.y))
assert(c.w > 0 and c.h > 0 and math.abs(c.x - (1024 - c.w) / 2) < 1 and math.abs(c.y - (728 - c.h) / 2) < 1, "its texture's size, centred")

run("c_missing")
assert(find("no_such_texture") == nil, "no texture, nothing shown")

run("c_end")
print("after end_cutscene:", #cinematics())
assert(#cinematics() == 0, "end_cutscene takes them all")
print("ok")
