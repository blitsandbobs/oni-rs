-- Pre-placed corpses (#57): a level starts with the bodies
-- its designers left lying about, drawn with their class's model as they
-- fell. Level 1's warehouse has one, Chung, with the LSI by his hand; the
-- powerup draws (its model and the glow under it), Konoko picks it up, and
-- the level's script, seeing she has it, unlocks the way on (door 37).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/corpses.lua

local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

local c = corpses()
assert(#c == 1, "level 1 has one corpse")
assert(c[1].class == "generic_male_1" and c[1].drawn, "drawn with its class's model")
assert(math.abs(c[1].y - 51.6) < 0.5, "lying on the floor")

-- From the warehouse save point, after its intro.
mission_begin(true, 1)
assert(wait_for(function() return anim() == "KONOKOlev1_intro" end, 3000), "the save point's intro: she drops in")
assert(wait_for(function() return anim() ~= "KONOKOlev1_intro" end, 1000))
-- (Then the health lesson's data console, closed.)
assert(wait_for(function() return text_console() ~= nil end, 1500), "the health lesson")
text_console_key("close")
step(60)
local _, locked = door_state(37)
assert(locked, "the way on is locked")
local lsi
for _, p in ipairs(powerups()) do
    if p.kind == "lsi" then lsi = p end
end
assert(lsi, "the LSI lies by the body")
assert(math.abs(lsi.at[1] - c[1].x) < 15 and math.abs(lsi.at[3] - c[1].z) < 15)

-- Finding the body: its cutscene, then the script watches for the LSI.
teleport(0, 415, 51, -660)
assert(wait_for(function() return anim():match("watch") end, 1500), "the cutscene: she calls it in")
local took = wait_for(function() return not anim():match("watch") end, 4000)
assert(took, "the cutscene ends")
print("the cutscene took", took)
step(60)
assert(select(2, door_state(37)), "still locked without the LSI")
-- Konoko picks it up.
teleport(0, lsi.at[1], lsi.at[2] + 1, lsi.at[3])
step(5)
swap()
assert(wait_for(function()
    for _, p in ipairs(powerups()) do
        if p.kind == "lsi" then return false end
    end
    return true
end, 120), "picked up")
-- She has it: the script reads it out (a data console), then unlocks.
assert(wait_for(function() return text_console() ~= nil end, 600), "the LSI's data console")
assert(select(2, door_state(37)), "locked while it reads")
text_console_key("close")
assert(wait_for(function() return not select(2, door_state(37)) end, 120), "the script unlocks the door")
print("ok")
