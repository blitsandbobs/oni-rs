-- Effects that ride what they struck: a
-- phase-rifle bolt into a dummy leaves its glow (w3_phr_x02) on the body part
-- it hit, and when the dummy moves the glow goes with it.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/effects.lua

step(20)
local px, py, pz = pos()
local d = spawn(px, py + 5, pz + 30, 180)
step(40)
facing(0)
step(5)
assert(give_weapon(0, "w3_phr"), "a phase rifle")
step(12) -- (the gun swings from the hand to the aim over 8 ticks)
local hp0 = hp(d)
fire(1)
local glow
for _ = 1, 40 do
    step(1)
    glow = glow or particles_of("w3_phr_x02")[1]
    if glow then break end
end
assert(hp(d) < hp0, "the bolt landed")
assert(glow, "its glow on the body")
-- Move the dummy 20 sideways: the glow goes along.
local dx, dy, dz = pos_of(d)
teleport(d, dx + 20, dy, dz)
step(2)
local now = particles_of("w3_phr_x02")[1]
assert(now, "the glow lasts")
print(string.format("glow: (%.1f, %.1f, %.1f) → (%.1f, %.1f, %.1f)", glow[1], glow[2], glow[3], now[1], now[2], now[3]))
assert(math.abs(now[1] - glow[1] - 20) < 3, "it rides the struck part")
print("effects ok")
