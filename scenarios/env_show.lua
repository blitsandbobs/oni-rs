-- env_show (docs/lua.md): level parts the scripts show and hide — the
-- training room's barriers (reference 2010) stop Konoko while shown and let
-- her through once a script hides them.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/env_show.lua

mission_begin()
step(5)
mission_exec([[
F.t_free = function() C.input(1) C.lock_keys("keys_all") end
]])
mission_call("t_free")
step(1)

local function z() local _, _, zz = pos() return zz end
local function run_north()
    teleport(0, -1100, -338, -165)
    facing(180)
    step(20)
    input(true, false)
    step(90)
    input(false, false)
    return z()
end

mission_exec([[F.t_on = function() C.env_show(2010, 1) end F.t_off = function() C.env_show(2010, 0) end]])
mission_call("t_on")
step(1)
local stopped = run_north()
print(string.format("barrier shown: stopped at z %.1f", stopped))
assert(stopped > -194, "the barrier stops her")

mission_call("t_off")
step(1)
local through = run_north()
print(string.format("barrier hidden: got to z %.1f", through))
assert(through < -200, "hidden, it lets her through")
print("env_show OK")
