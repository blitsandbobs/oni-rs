-- Doors (#17; docs/DOORS.md): a door opens for anyone allowed through who
-- comes near, and its leaf blocks only while closed — Konoko runs through
-- an open one; locked, it stops her; unlocked again, it opens as she comes.
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
print("doors OK")
