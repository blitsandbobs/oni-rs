-- Glass (#23; docs/PARTICLES.md): a pane whose texture's material is glass
-- breaks when a shot hits it — no longer drawn or solid, and the shots
-- after it fly through. Level 1's shooting range has 18 such targets
-- (script references 3001–3018), which `env_broken` counts.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/glass.lua

assert(env_broken(3001, 3018) == 0, "all 18 targets whole")

-- In the shooting range, 30 units in front of the first column of targets
-- (the lowest pane's middle is 8 above the floor), facing it (+X).
teleport(0, -540, -297, -517.9)
step(30)
-- (The pistol's shot leaves 3° off its barrel and auto-aim only helps
-- against characters: aim a little right and low.)
facing(93)
step(10)
assert(give_weapon(0, "w1_tap"))
aim(0, -0.15)
step(5)
fire(1)
step(20)
local broken = env_broken(3001, 3018)
print("targets broken:", broken)
assert(broken == 1, "one shot, one target")
assert(env_broken(3003) == 1, "the lowest pane of the first column")

-- env_show hides glass by breaking it, and showing it mends it.
env_show(3001, false)
assert(env_broken(3001) == 1)
env_show(3001, true)
assert(env_broken(3001) == 0)
print("glass ok")
