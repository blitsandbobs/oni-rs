-- Doors (#17): a door opens for anyone allowed through who
-- comes near, and its leaf blocks only while closed — Konoko runs through
-- an open one; locked, it stops her; unlocked again, it opens as she comes.
-- An AI plans no way through a door it may not open.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/doors.lua

local function z() local _, _, zz = pos() return zz end
local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

-- Door 18 (an unlocked warehouse door across z = -590).
step(5)
teleport(0, -82, 2, -545)
facing(180)
step(30)
assert(door_state(18) == "closed", "closed while nobody is near")
input(true, false)
local took = wait_for(function() return door_state(18) ~= "closed" end, 120)
print("opens as she comes near, after", took, string.format("(at z %.0f)", z()))
assert(took and z() > -590 + 0, "it opens before she gets there")
took = wait_for(function() return z() < -610 end, 200)
input(false, false)
assert(took, "she runs through")

-- It closes behind her (3 s open, then nobody near).
teleport(0, -82, 2, -680)
took = wait_for(function() return door_state(18) == "closed" end, 600)
print("closed behind her after", took)
assert(took, "it closes when nobody is near")

-- Locked: it stays shut and stops her.
door(18, "lock")
teleport(0, -82, 2, -640)
facing(0)
step(20)
input(true, false)
step(150)
input(false, false)
print(string.format("locked: %s, she got to z %.1f", door_state(18), z()))
assert(door_state(18) == "closed" and z() < -588, "a locked door stops her")

-- Unlocked: it opens and she goes on.
door(18, "unlock")
input(true, false)
took = wait_for(function() return z() > -570 end, 200)
input(false, false)
assert(took, "unlocked, it lets her through")
-- A manual door (15, on the floor above): nothing until Action, facing it.
door(15, "unlock")
teleport(0, -172, 52, -654)
facing(270)
step(60)
assert(door_state(15) == "closed", "a manual door doesn't open by itself")
action()
step(5)
print("Action at the manual door:", door_state(15))
assert(door_state(15) ~= "closed", "Action opens it")
input(true, false)
local function x() local xx = pos() return xx end
took = wait_for(function() return x() < -215 end, 200)
input(false, false)
assert(took, "and she goes through")

-- An AI's way: a locked door shuts it (no way round here), unlocked it
-- walks through.
teleport(0, 200, 8, 300) -- Konoko out of the way
local a = spawn(-82, 6, -545, 180, "striker_easy_1")
step(240)
door(18, "lock")
go_to(a, -82, 0, -640, "run")
step(60)
print("AI, door locked:", nav_state(a))
assert(nav_state(a) ~= "following", "no way through a locked door")
door(18, "unlock")
go_to(a, -82, 0, -640, "run")
took = wait_for(function() return nav_state(a) == "arrived" end, 400)
print("AI, door unlocked: arrived after", took)
assert(took, "unlocked, the AI walks through")
print("doors OK")
