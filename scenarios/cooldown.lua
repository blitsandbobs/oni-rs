-- Q23, the weapon cooldown exploit (docs/QUIRKS.md): the Mercury Bow waits
-- a long while between shots. Dropped and picked up again, it still waits
-- (fixed); the shipped game zeroes the delay when the gun is let go, so the
-- bow is ready again at once (faithful).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/cooldown.lua

step(20)
assert(give_weapon(0, "w8_mbo"), "a Mercury Bow")
step(10)
fire(1)
step(3)
local _, _, _, wait = weapon(0)
print("after the shot: waits", wait)
assert(wait and wait > 30, "the bow waits between shots")

drop_weapon(0)
step(30)
assert(weapon(0) == nil, "dropped")
swap()
step(60)
local class, _, _, left = weapon(0)
assert(class == "w8_mbo", "picked up again: " .. tostring(class))
print("picked up again: waits", left)
if faithful() then
  assert(left == 0, "shipped: the drop cleared its delay")
else
  assert(left > 0, "fixed: the delay carries over the drop")
end
print("ok")
