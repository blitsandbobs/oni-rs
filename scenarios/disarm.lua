-- Disarming (#56): a throw's action frame takes the
-- victim's gun — into the thrower's hand if it has none in hand or
-- holstered, else knocked from the victim's to the floor
-- (`ONrCharacter_Update`'s throw swap). And the melee AI picks a disarm
-- throw against an armed enemy when its own hand is empty.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/disarm.lua

step(20)
local px, py, pz = pos()
facing(0)

-- An armed dummy 8 in front, facing Konoko: her front throw takes its gun.
local d = spawn(px, py + 5, pz + 8, 180)
assert(give_weapon(d, "w1_tap"))
step(60)
facing(0)
assert(weapon(d) == "w1_tap" and weapon(0) == nil, "it is armed, she isn't")
input(true, false); punch(); step(1); input(false, false)
print("throw:", anim(), anim_of(d))
for _ = 1, 140 do step(1) end
print("after: Konoko holds", weapon(0), "the dummy", weapon(d))
assert(weapon(0) == "w1_tap" and weapon(d) == nil, "the gun changed hands")

-- Armed herself: the next armed dummy's gun is knocked to the floor.
step(200)
set_facing(0, 0)
local qx, qy, qz = pos()
local e = spawn(qx, qy + 5, qz + 8, 180)
assert(give_weapon(e, "w1_tap"))
step(60)
set_facing(0, 0)
local loose = #loose_guns()
input(true, false); punch(); step(1); input(false, false)
print("armed throw:", anim(), anim_of(e))
for _ = 1, 140 do step(1) end
print("after: Konoko holds", weapon(0), "the dummy", weapon(e), "loose guns", #loose_guns() - loose)
assert(weapon(0) == "w1_tap" and weapon(e) == nil and #loose_guns() == loose + 1, "knocked loose")

-- The melee AI: a striker (its profile has "Disarm Front"), unarmed, takes
-- Konoko's pistol with it. (Kept forced, as its weighing allows it: an
-- armed target, an empty hand.)
step(200)
set_facing(0, 0)
local rx, ry, rz = pos()
local s = spawn(rx, ry + 5, rz + 20, 180, "striker_med_1")
step(30)
team(s, "syndicate")
spar(s, 0)
technique(s, "Disarm Front")
local took
for k = 1, 1800 do
    step(1)
    set_hp(0, 1000)
    set_facing(0, 0)
    technique(s, "Disarm Front")
    if weapon(s) == "w1_tap" then took = k break end
end
print("the striker holds", weapon(s), "after", took, "Konoko", weapon(0))
assert(took and weapon(0) == nil, "the AI disarmed her")
print("ok")
