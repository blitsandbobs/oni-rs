-- Mutant Muro's own fighting (#31), on level 19: his
-- super shield up from the start; from afar (with his first thunderbolt
-- held back 5 s, as shipped) the thunderbolt — its homing bolts pull his
-- enemy in, and his shield drops while he throws it; close in he fights
-- hand to hand, and hurt there with his shield down he turtles.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/19/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/muro_ai.lua

local function wait_for(cond, max, each)
    for t = 1, max do
        step(1)
        set_hp(0, 100000)
        if each then each(t) end
        if cond() then return t end
    end
    return nil
end
local function dist(a, b)
    local x1, _, z1 = pos_of(a)
    local x2, _, z2 = pos_of(b)
    return math.sqrt((x1 - x2) ^ 2 + (z1 - z2) ^ 2)
end

mission_begin(false)
step(5)
local m = place("MutantMuro")
local x, y, z = pos_of(m)
-- (Open floor along +z from where he stands.)
teleport(0, x, y + 5, z + 220)
mission_exec([[F.go = function() C.ai2_attack("MutantMuro", 0) end]])
assert(mission_call("go"))

-- His shield, up at once.
assert(wait_for(function() return effects(m).super > 0.5 end, 30), "shield up")
print("shield: ok", effects(m).super)

-- From afar, kept there: the thunderbolt, after 5 s.
-- (Every half second, so she lands in between: kept in the air she would
-- die of the fall after 5 s, as in the original.)
local keep = function(t)
    if t % 30 ~= 1 then return end
    local a, b, c = pos_of(m)
    teleport(0, a, b + 5, c + 220)
end
local t = wait_for(function() return anim_of(m) == "MUTCOMthunderbolt_start" end, 420, keep)
assert(t and t > 250, "the thunderbolt, held back 5 s: " .. tostring(t))
assert(wait_for(function() return #particles_of("w13_muro_p01") > 0 end, 120), "bolts fly")
assert(wait_for(function() return effects(m).super == 0 end, 120), "his shield drops")
print("thunderbolt: ok")

-- The bolts pull her in, and close in he fights.
assert(wait_for(function() return dist(0, m) < 60 end, 900), "pulled in")
assert(wait_for(function() return not anim_of(m):match("thunderbolt") end, 300), "out of the thunderbolt")
print("pulled in: ok", dist(0, m))

-- Hurt close in, shield down: he turtles (his crouch back).
assert(give_weapon(0, "w1_tap"))
local beat = 0
local turtled = wait_for(function() return anim_of(m) == "MUTCOMcrouch_bk" end, 600, function()
    beat = beat + 1
    local a, b, c = pos_of(m)
    local qx, _, qz = pos()
    if math.sqrt((a - qx) ^ 2 + (c - qz) ^ 2) > 25 then teleport(0, a, b + 5, c + 20) end
    qx, _, qz = pos()
    facing(math.deg(math.atan(a - qx, c - qz)))
    if beat % 8 == 0 then fire(1) end
end)
assert(turtled, "he turtles")
print("ok")
