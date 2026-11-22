-- Placed objects: none until a script creates them; each
-- drawn where its clip puts it; started, swapped and killed by script id;
-- characters collide with its sphere.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/objects.lua

assert(#objects() == 0, "nothing until a script makes it")
mission_begin()
step(5)
print("the level's own scripts made", #objects())
mission_exec([[
F.t_clear = function() C.obj_kill(0, 100000) end
F.t_free = function() C.input(1) C.lock_keys("keys_all") end
F.t_lifts = function() C.obj_create(101, 109) end
F.t_go = function() C.env_anim(101, 109) end
F.t_swap = function() C.env_setanim(101, "TwoForkLift01") end
F.t_gone = function() C.obj_kill(101, 109) end
F.t_truck = function() C.obj_create(201, 207) end
F.t_notruck = function() C.obj_kill(201, 207) end
]])
mission_call("t_free")
mission_call("t_clear")
step(1)
assert(#objects() == 0, "killed")

mission_call("t_lifts")
step(1)
local o = objects()
print("forklifts:", #o, o[1].name, string.format("at (%.1f %.1f %.1f)", o[1].at[1], o[1].at[2], o[1].at[3]))
assert(#o == 9 and not o[1].animating, "nine forklifts, standing")
local start = o[1].at
mission_call("t_go")
step(61)
o = objects()
local moved = math.sqrt((o[1].at[1] - start[1]) ^ 2 + (o[1].at[3] - start[3]) ^ 2)
print(string.format("after 60 ticks: frame %d, moved %.1f", o[1].frame, moved))
assert(o[1].animating and o[1].frame == 60, "a frame a tick")
mission_call("t_swap")
step(2)
assert(objects()[1].frame == 1, "a new clip from its start")
mission_call("t_gone")
step(1)
assert(#objects() == 0, "killed")

-- The truck's wheels (their spheres) in her way.
local function walk_at_wheel()
    teleport(0, 419, 2, -120)
    facing(90)
    step(20)
    input(true, false)
    local met = 0
    for _ = 1, 90 do
        step(1)
        local _, objs = collisions(0)
        met = met + objs
    end
    input(false, false)
    local x = pos()
    return x, met
end
local free, met_free = walk_at_wheel()
mission_call("t_truck")
step(1)
local blocked, met = walk_at_wheel()
mission_call("t_notruck")
print(string.format("walking at the truck: got to x %.1f without it, %.1f with it (met it %d ticks)", free, blocked, met))
assert(met_free == 0 and met > 0, "she meets its wheels' spheres")
print("objects ok")
