-- The super ninja's own fighting (#31), on level 12's
-- rooftop: from afar he throws a fireball (his clip's particle, homing);
-- closing in he turns invisible or teleports in behind his enemy; beaten
-- about close in he teleports away to a retreat flag; and falling off the
-- roof he hangs below it, then teleports back up to its edge.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/12/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/superninja_ai.lua

local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        set_hp(0, 100000)
        if cond() then return t end
    end
    return nil
end

mission_begin(false)
step(5)
local n = place("OutroNinja")
-- (Both on the roof: x 1588–1882, z −3326…−2946 at 644.)
teleport(n, 1735, 646, -3300)
teleport(0, 1735, 650, -3150)
mission_exec([[F.go = function() C.ai2_attack("OutroNinja", 0) end]])
assert(mission_call("go"))

-- From afar: the fireball, launched from his hand.
assert(wait_for(function() return anim_of(n) == "NINCOMfireball" end, 60), "the fireball clip")
assert(wait_for(function() return #particles_of("w10_sni_p01") > 0 end, 60), "the fireball flies")
print("fireball: ok")

-- Closing in: invisible, or a teleport in behind her.
assert(wait_for(function() return effects(n).invisibility > 0 or anim_of(n):match("teleport") end, 600), "invisible or teleporting")
print("medium range: ok", effects(n).invisibility, anim_of(n))

-- Beaten about close in (35 hurt within 50), seen: he teleports away.
local _, ky = pos()
assert(ky > 640, "Konoko still on the roof")
assert(wait_for(function() return effects(n).invisibility == 0 and not anim_of(n):match("teleport") end, 700), "seen again")
teleport(n, 1720, 646, -3140)
teleport(0, 1750, 650, -3140)
step(20)
assert(give_weapon(0, "w1_tap"))
local beat = 0
local fled = wait_for(function()
    beat = beat + 1
    local qx, _, qz = pos()
    local mx, _, mz = pos_of(n)
    facing(math.deg(math.atan(mx - qx, mz - qz)))
    if beat % 10 == 0 then fire(1) end
    return anim_of(n) == "NINCOMteleport_in"
end, 900)
assert(fled, "he teleports away")
assert(wait_for(function() return anim_of(n) == "NINCOMteleport_out" end, 120), "and out")
step(2)
local qx, _, qz = pos()
local mx, _, mz = pos_of(n)
local away = math.sqrt((mx - qx) ^ 2 + (mz - qz) ^ 2)
print("teleported to", mx, mz, "distance", away)
assert(away > 80, "well away from her")

-- Off the roof: he hangs at 550, then teleports back to its edge.
teleport(n, mx, 300, mz)
step(30)
local _, hy = pos_of(n)
assert(math.abs(hy - 550) < 2, "hanging at 550, got " .. hy)
assert(wait_for(function() return anim_of(n) == "NINCOMteleport_out" end, 200), "teleported back")
step(2)
local _, by = pos_of(n)
assert(by > 600, "on the roof again, at " .. by)
print("ok")
