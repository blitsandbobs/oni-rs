-- Death (#12): a body stays where it fell — unless its class deletes its
-- dead after a delay (the mad bomber: 190 ticks), as the original does.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/death.lua

step(10)
local px, py, pz = pos()
local thug = spawn(px + 20, py + 5, pz, 0, "thug_wh_1")
local bomber = spawn(px - 20, py + 5, pz, 0, "mad_bomber")
step(30)
damage(thug, 10000)
damage(bomber, 10000)
assert(dead(thug) and dead(bomber), "both die")
step(150)
assert(present(bomber), "the bomber's body lies a while")
step(60)
print("after 210 ticks: thug", present(thug), "bomber", present(bomber))
assert(not present(bomber), "then its class deletes it")
step(600)
assert(present(thug), "a thug's body stays")
print("death OK")
