-- Dodging (#6): an armed soldier holding its ground
-- in the cone of a gun firing at it — the burst pistol's — steps off the
-- line of fire once the danger has lasted its reaction time (meleeing, it
-- wouldn't: the melee AI moves it then).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/ai_dodge.lua

step(20)
local px, py, pz = pos()
facing(0)
local d = spawn(px, py + 5, pz + 60, 180, "striker_easy_1")
goals(d)
watch(d)
team(d, "syndicate")
assert(give_weapon(d, "w1_tap"))
set_hp(d, 2000) -- (to outlast the volley)
assert(give_weapon(0, "w2_sap"))
step(30)
local x0 = pos_of(d)
local most = 0
-- Fire 1 held: the autofire pistol sprays toward it.
hold_fire(true)
for t = 1, 200 do
    if t == 160 then hold_fire(false) end
    aim(0, 0) -- (pulling the kick back down, as a player does)
    step(1)
    local x = pos_of(d)
    most = math.max(most, math.abs(x - px))
end
print("striker off the line by", most, "(started", math.abs(x0 - px), ")")
assert(most > 8, "it stepped off the line of fire")
print("ai dodge ok")
