-- The turn keys and the action key (the AI uses them; the player turns with
-- the mouse): lying down, a turn key rolls the character up sideways; in the
-- fight stance, the action key taunts.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/turn_keys.lua

local STANDING_TURN_LEFT, TAUNT, STANDING = 7, 94, 7
step(20)
knock(0, 79)
for _ = 1, 90 do step(1) end
turn_keys(true, false)
local rolled = false
for _ = 1, 60 do
    step(1)
    if anim_type() == STANDING_TURN_LEFT and anim():find("getup") then rolled = true end
end
turn_keys(false, false)
print("roll up with turn-left: " .. tostring(rolled) .. " (" .. anim() .. ")")
assert(rolled, "a turn key rolls the character up from the ground")

step(120)
punch(); step(30) -- into the fight stance
action(); step(2)
print("action in the fight stance: type " .. anim_type() .. " " .. anim())
assert(anim_type() == TAUNT, "the action key taunts")
print("turn keys ok: roll up from the ground; action taunts")
