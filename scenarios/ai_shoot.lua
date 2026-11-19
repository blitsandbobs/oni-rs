-- AI shooting (#6; docs/AI.md, *Shooting*): an armed enemy that sees the
-- player goes into combat, swings its aim onto her, and fires when its gun
-- points close enough — spaced by its skill's delay — and its shots land.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/ai_shoot.lua

step(20)
local px, py, pz = pos()
facing(0)
local d = spawn(px, py + 5, pz + 70, 180, "striker_easy_1")
goals(d)
watch(d)
team(d, "syndicate")
assert(give_weapon(d, "w1_tap"), "the striker gets a pistol")
local hp0 = hp(0)
local fired_at = nil
for t = 1, 900 do
    step(1)
    local _, ammo = weapon(d)
    if ammo and ammo < 10 and not fired_at then fired_at = t end
end
local _, left = weapon(d)
print("combat:", goal_of(d), "first shot at tick", fired_at, "rounds left", left, "player hp", hp0, "→", hp(0))
assert(fired_at, "the striker fires")
assert(left < 10, "rounds spent")
assert(hp(0) < hp0, "its shots land")
print("ai shoot ok")
