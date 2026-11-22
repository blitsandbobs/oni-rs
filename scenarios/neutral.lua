-- Neutral characters (#14): a civilian with a neutral
-- behaviour notices Konoko and comes up to her; Action facing it starts the
-- conversation — its lines said, Konoko's keys held back meanwhile — and at
-- its end the behaviour's script runs (level 1: the truck driver goes and
-- opens the warehouse). An attack breaks a conversation off; on a giving
-- line the civilian walks up and hands over its gun.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/neutral.lua

local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

mission_begin(false)
mission_exec([[
F.sp2 = function() C.ai2_spawn("Bay6_Neutral_2") end
F.sp1 = function() C.ai2_spawn("Bay6_Neutral_1") end
F.give = function() C.ai2_neutralbehavior("Bay6_Neutral_1", "Give VDG Pistol (CivFem)") end
]])
step(2)

-- Konoko kept beside a civilian (who may be walking his patrol) until he
-- notices her.
local function noticed_near(id, max)
    for t = 1, max do
        local x, y, z = pos_of(id)
        teleport(0, x + 30, y, z)
        step(30)
        if goal_of(id) == "neutral" then return t end
    end
    return nil
end

local function face(n)
    local px, _, pz = pos()
    local nx, _, nz = pos_of(n)
    set_facing(0, math.deg(math.atan(nx - px, nz - pz)))
end

-- The truck driver (Talk_About_Truck: one line, can't be broken off, its
-- end script sends him to the warehouse console).
assert(mission_call("sp2"))
step(2)
local n = named("Bay6_Neutral_2")
local x, y, z = pos_of(n)
teleport(0, x + 30, y, z)
assert(wait_for(function() return goal_of(n) == "neutral" end, 200), "he notices Konoko and comes up")
assert(not neutral_of(n).accepted and neutral_of(n).line == nil, "waiting for her to answer")
face(n)
step(2)
action()
assert(wait_for(function() return neutral_of(n).with == 0 end, 60), "Action facing him: the conversation begins")
assert(wait_for(function() return saying(n) == "c07_02_06civmale" end, 10), "his line")
-- Her moves are held back; this one can't be walked out of.
local px, _, pz = pos()
input(true, false)
step(60)
input(false, false)
local qx, _, qz = pos()
assert(math.abs(qx - px) + math.abs(qz - pz) < 1 and neutral_of(n).with == 0, "she stays put, still talking")
assert(wait_for(function() return neutral_of(n).completed end, 900), "the line said, it ends")
assert(neutral_of(n).with == nil and neutral_of(0) == nil, "they part")
assert(wait_for(function() return console_state(9) == "active" end, 30), "his end script switches the warehouse console on")
assert(wait_for(function() return goal_of(n):match("^alarm") end, 60), "and he runs to it")
print("the truck driver's talk: ok")

-- Another civilian (Go_Upstairs), broken off with a punch.
assert(mission_call("sp1"))
step(2)
local m = named("Bay6_Neutral_1")
assert(noticed_near(m, 20), "the second civilian, on his patrol, notices her")
face(m)
step(2)
action()
assert(wait_for(function() return neutral_of(m).with == 0 end, 60), "talking")
punch()
step(2)
assert(neutral_of(m).with == nil and goal_of(m) ~= "neutral", "a punch breaks it off; back to his job")
assert(not neutral_of(m).completed, "not finished")
print("broken off: ok")

-- Given a giving behaviour by script: he walks up and hands over a pistol.
assert(mission_call("give"))
step(200) -- (broken off, he won't start again by himself for 180 ticks)
assert(neutral_of(m).behavior ~= nil, "the new behaviour")
assert(noticed_near(m, 20), "he comes up again")
face(m)
step(2)
action()
assert(wait_for(function() return neutral_of(m).items_given end, 900), "he gives")
local gun = held_gun(0)
assert(gun, "Konoko holds the pistol he gave")
assert(wait_for(function() return neutral_of(m).completed end, 900), "and it ends")
print("ok")
