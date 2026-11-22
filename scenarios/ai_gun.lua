-- Going for a gun (#6): an unarmed enemy whose
-- profile shoots, rolling its class's chance to look for a gun, runs to a
-- loaded one lying near, bends for it (the pickup clip) and then shoots.
-- Rolling its running-pickup chance, it rolls over the gun instead and takes
-- it on the way (a roll that can pick up).
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

-- On the run: a second striker, its running-pickup chance made sure, rolls
-- (a crouch-move clip) and takes the gun mid-roll.
local d2 = spawn(px + 60, py + 5, pz + 80, 180, "striker_easy_1")
goals(d2)
gun_chance(d2, 100)
running_pickup_chance(d2, 100)
team(d2, "syndicate")
local x2, y2, z2 = pos_of(d2)
assert(spawn_weapon("w1_tap", x2, y2 + 2, z2 - 40), "a pistol ahead of it")
local rolled, took2 = false, nil
for t = 1, 1200 do
    step(1)
    if anim_of(d2):match("crouch_") then rolled = true end
    if weapon(d2) == "w1_tap" then took2 = t break end
end
print("on the run: rolled", rolled, "took it after", took2)
assert(rolled and took2, "rolled over the gun and took it")
print("ai gun ok")
