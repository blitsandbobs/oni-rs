-- The mouse buttons are Oni's fire buttons; without a weapon the game turns
-- fire 1 into a punch, fire 2 into a kick and fire 3 into crouch — so an
-- unarmed Konoko punches with the left mouse button and kicks with the right.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/mouse_melee.lua

local PUNCH, KICK, CROUCH_STATE = 22, 12, 19
step(60)

fire(1); step(2)
print("fire 1 (left mouse): type " .. anim_type() .. " " .. anim())
assert(anim_type() == PUNCH, "unarmed fire 1 punches")
step(90)

fire(2); step(2)
print("fire 2 (right mouse): type " .. anim_type() .. " " .. anim())
assert(anim_type() == KICK, "unarmed fire 2 kicks")
step(90)

-- Fire 3 counts as crouch while held: tap it every other tick for a while.
for _ = 1, 30 do fire(3); step(1); fire(3); step(1) end
print("fire 3 (middle mouse): state " .. state())
assert(state() == CROUCH_STATE or anim():find("crouch"), "fire 3 crouches")
print("mouse melee ok: left = punch, right = kick, middle = crouch while unarmed")
