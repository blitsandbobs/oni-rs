-- Characters moved by the level: an
-- environment animation (chr_envanim) carries a character along an object
-- clip — its pelvis on the clip's path, through everything — until the
-- clip's end; chr_nocollision lets one walk through walls.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/envanim.lua

mission_begin()
step(5)
mission_exec([[
F.t_free = function() C.input(1) C.lock_keys("keys_all") end
F.t_ride = function() C.chr_envanim(0, "KonBipedBox02", "norotation") end
F.t_ghost = function() C.chr_nocollision(0, 1) end
F.t_solid = function() C.chr_nocollision(0, 0) end
F.t_wall = function() C.env_show(2010, 1) end
]])
mission_call("t_free")
step(1)

-- The ride: from the clip's start (about x -519.5, z -712; 159 high at the
-- pelvis), down to its end, then on her own again. With "norotation" she is
-- drawn unturned (facing 0) whatever her facing: here facing the other way.
set_facing(0, 0)
step(2)
local unturned = bone(0, 0).z
set_facing(0, 180)
mission_call("t_ride")
step(2)
local riding = bone(0, 0).z
local dot = unturned[1] * riding[1] + unturned[2] * riding[2] + unturned[3] * riding[3]
print(string.format("riding, her pelvis's turn against facing 0: %.2f", dot))
assert(dot > 0.9, "drawn unturned, not with her facing")
local x, y, z = pos()
print(string.format("riding: at (%.1f %.1f %.1f)", x, y, z))
assert(math.abs(x + 519.5) < 1 and math.abs(z + 712.1) < 1 and y > 140 and y < 160, "on the clip's path")
step(200)
local _, y2 = pos()
assert(y2 < y - 20, "carried down along it")
step(200)
local x3, y3, z3 = pos()
print(string.format("after the ride: (%.1f %.1f %.1f)", x3, y3, z3))
assert(math.abs(x3 + 518.5) < 3 and math.abs(z3 + 668) < 3, "left at its end")

-- Collision off: through the training room's barrier.
mission_call("t_wall")
local function run_north()
    teleport(0, -1100, -338, -165)
    facing(180)
    step(20)
    input(true, false)
    step(90)
    input(false, false)
    local _, _, zz = pos()
    return zz
end
local stopped = run_north()
mission_call("t_ghost")
local through = run_north()
mission_call("t_solid")
print(string.format("barrier: stopped at z %.1f; with collision off got to %.1f", stopped, through))
assert(stopped > -194 and through < -200, "with collision off she walks through it")
print("envanim ok")
