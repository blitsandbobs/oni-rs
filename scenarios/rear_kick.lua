-- Hits connect by body contact, not by "is the target in front": an attack
-- lands when one of its live parts (a jab's fist, a kick's leg) touches the
-- other body. So a rear kick hits someone standing behind the attacker, and a
-- jab at a target out of reach misses.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/rear_kick.lua

local KICK_BACK = 29

step(20)
local px, py, pz = pos()
facing(0) -- facing +Z

-- A dummy right behind Konoko (-Z), facing away from her.
local d = spawn(px, py + 5, pz - 6, 180)
step(60)
facing(0)
local hp0 = hp(d)

-- Kick with "back" held: the rear kick.
input(false, true); kick(); step(1); input(false, false)
local kicked, landed = false, false
for _ = 1, 40 do
    step(1)
    if anim_type() == KICK_BACK then kicked = true end
    if hp(d) < hp0 then landed = true end
end
print(string.format("rear kick: played=%s, dummy hp %d -> %d", tostring(kicked), hp0, hp(d)))
assert(kicked, "kick + back plays the rear kick")
assert(landed, "the rear kick hits the dummy behind her")

-- A jab with nobody within reach in front connects with nothing.
step(90)
local before = hp(d)
facing(0)
punch()
for _ = 1, 30 do step(1) end
assert(hp(d) == before, "a jab away from the dummy hits nothing")
print("rear kick ok: blows connect by body contact, in any direction")
