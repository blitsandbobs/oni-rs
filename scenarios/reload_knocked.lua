-- Q26, an interrupted reload (docs/deviations.md): a reload
-- takes a clip as it starts and fills the gun at its clip's action frame.
-- Knocked down before that (which also knocks the gun from her hand),
-- Konoko gets the clip back (fixed); the shipped game loses it
-- (--faithful).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/reload_knocked.lua

step(20)
assert(give_weapon(0, "w1_tap"))
give_ammo(0, 2, 0)
step(10)
fire(1)
step(30)
local clips = inventory(0)
print("before:", clips, "clips")
assert(reload(0), "the reload starts")
step(2)
assert(inventory(0) == clips - 1, "the reload takes a clip at once")
knock(0, 79) -- a frontal head knockdown, before the clip goes in
step(90)
-- (The knockdown also knocks the gun from her hand, unfilled.)
local left = inventory(0)
print("after the knockdown:", left, "clips; in hand:", weapon(0))
if faithful() then
  assert(left == clips - 1, "shipped: the clip is lost")
else
  assert(left == clips, "fixed: the clip comes back")
end
print("ok")
