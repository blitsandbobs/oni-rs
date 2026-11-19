-- Weapons (#10; docs/WEAPONS.md): a gun in the hand turns fire 1 from a
-- punch into a shot. The pistol's particle flies down the aim, hits the
-- dummy once for its 8–10 damage, and the magazine drops by one; an empty
-- gun reloads from a spare clip.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/weapons.lua

step(20)
local px, py, pz = pos()
local d = spawn(px, py + 5, pz + 60, 180)
step(40)
facing(0)
step(5)

assert(give_weapon(0, "w1_tap"), "the pistol class exists")
local class, ammo = weapon(0)
assert(class == "w1_tap" and ammo == 10, "a full pistol in hand: " .. tostring(class) .. " " .. tostring(ammo))
step(2)

local before = hp(d)
fire(1)
step(20)
local _, left = weapon(0)
local after = hp(d)
print(string.format("ammo %d → %d, dummy hp %d → %d", ammo, left, before, after))
assert(left == 9, "one shot used one round")
assert(before - after >= 8 and before - after <= 10, "one pistol round: 8–10 damage")

-- The pistol's kick stays in the player's aim (a "recoil direct" gun):
-- about 3° up a shot, to pull back down by hand.
local pitch = aim_of(0)
assert(pitch > 0.04 and pitch < 0.07, "kicked up ~3°: " .. pitch)

-- Empty it (the chamber needs 12 ticks between shots), pulling the aim
-- back down each time: every round lands.
before = hp(d)
for _ = 1, 9 do
  aim(0, 0)
  fire(1)
  step(14)
end
_, left = weapon(0)
assert(left == 0, "empty after ten shots: " .. left)
local dealt = before - hp(d)
assert(dealt >= 72 and dealt <= 90, "nine more hits: " .. dealt)
-- Empty without a clip: fire 1 just clicks.
fire(1)
step(5)
_, left = weapon(0)
assert(left == 0)
-- With a clip, an empty gun reloads on the press, and fills later.
give_ammo(0, 1)
fire(1)
step(60)
_, left = weapon(0)
assert(left == 10, "reloaded: " .. left)
print("weapons ok")
