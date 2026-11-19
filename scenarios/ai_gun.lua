-- Going for a gun (#6; docs/AI.md, *Shooting*): an unarmed enemy whose
-- profile shoots, rolling its class's chance to look for a gun, runs to a
-- loaded one lying near, bends for it (the pickup clip) and then shoots.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/ai_gun.lua

step(20)
local px, py, pz = pos()
facing(0)
local d = spawn(px, py + 5, pz + 80, 180, "striker_easy_1")
goals(d)
gun_chance(d, 100) -- (its class rolls 30%: make it sure)
watch(d)
team(d, "syndicate")
local x, y, z = pos_of(d)
assert(spawn_weapon("w1_tap", x + 10, y + 2, z - 28), "a pistol lies between them")
local took = nil
for t = 1, 1200 do
    step(1)
    if weapon(d) == "w1_tap" then took = t break end
end
print("picked up the pistol after", took, goal_of(d))
assert(took, "it went for the gun")
local hp0 = hp(0)
step(600)
print("player hp", hp0, "→", hp(0))
assert(hp(0) < hp0, "and shoots with it")
print("ai gun ok")
