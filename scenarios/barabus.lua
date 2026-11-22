-- Level 3's opening: Barabus. He is
-- placed unkillable with infinite ammo: he fires without his gun running
-- down, and a blow that would kill him leaves him on 1 and defeats him
-- (`ONrCharacter_TakeDamage`) — knocked down, his defeated script runs: the
-- rocket scene, after which he is gone and objective 2 follows. (His die
-- script isn't his defeat: he never dies.)
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/barabus.lua

mission_begin()
local t = 0
repeat step(30) t = t + 30 until ((hud().letterbox or 0) == 0 and t > 300) or t > 6000
local b = named("Barabus")
assert(b and present(b), "Barabus is there after the intro")
print("intro over at tick", t)

-- He shoots Konoko (kept alive) for ten seconds: his gun never runs down.
local _, ammo0 = weapon(b)
local shot = 0
for _ = 1, 600 do
  local before = hp(0)
  step(1)
  if hp(0) < before then shot = shot + 1 end
  set_hp(0, 100000)
end
local gun, ammo1 = weapon(b)
print("hits on Konoko:", shot, "his", gun, "ammo", ammo0, "→", ammo1)
assert(shot > 10, "he fires on her")
assert(ammo1 == ammo0, "infinite ammo: his gun doesn't run down")

-- Worn down: defeated, not killed.
local hits = 0
repeat damage(b, 20) step(15) hits = hits + 1 until hp(b) <= 1 or hits > 100
print("defeated after", hits, "blows, hp", hp(b))
assert(hp(b) == 1 and present(b), "an unkillable one is left on 1")
for _ = 1, 3000 do step(1) if not present(b) then break end end
assert(not present(b), "his defeat runs the rocket scene, which removes him")
step(300)
print("objective:", hud().objective)
assert(hud().objective == 2, "and the level goes on")
print("ok")
