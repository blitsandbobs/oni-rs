-- Barabbas' own fighting (#31), on level 3: at
-- medium range he fires his beam and waits for it to recharge, from farther
-- off he advances lobbing grenades between beams; worn below 40% and far
-- enough off he stands and regenerates (the powerup clip, 8% of his health
-- a second) until past 80%, and enough hurt while at it stops him.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/barabbas_ai.lua

mission_begin()
local t = 0
repeat step(30) t = t + 30 until ((hud().letterbox or 0) == 0 and t > 300) or t > 6000
local b = named("Barabus")
assert(b and present(b))

-- Fighting at range: the modes he goes through.
local seen = {}
for _ = 1, 900 do
    step(1)
    set_hp(0, 100000)
    local m = boss_of(b)
    if m then seen[m] = true end
end
local list = {}
for m in pairs(seen) do list[#list + 1] = m end
table.sort(list)
print("modes:", table.concat(list, " "))
assert(seen.ShootBeam and seen.WaitForWeapon, "beam, then waiting for it")

-- Worn down, with Konoko far off: he regenerates.
local bx, by, bz = pos_of(b)
teleport(0, bx + 150, by + 5, bz)
local max = 300
set_hp(b, 0.3 * max)
local regen = nil
for i = 1, 600 do
    step(1)
    set_hp(0, 100000)
    if boss_of(b) == "Regenerate" then regen = i break end
end
assert(regen, "he regenerates")
local hp0 = hp(b)
step(120)
print("regenerating: hp", hp0, "->", hp(b), anim_of and anim_of(b) or "")
assert(hp(b) > hp0, "his health comes back")
-- Shot while at it (5% of his health or more): he stops.
assert(give_weapon(0, "w1_tap"))
local px, _, pz = pos()
local bx2, _, bz2 = pos_of(b)
facing(math.deg(math.atan(bx2 - px, bz2 - pz)))
step(15)
local stopped = false
for i = 1, 240 do
    if i % 20 == 0 then fire(1) end
    step(1)
    set_hp(0, 100000)
    if boss_of(b) ~= "Regenerate" then stopped = true break end
end
assert(stopped, "hurt enough, he stops regenerating")
print("ok")
