-- Throws: a forward punch or kick close to an opponent becomes a throw. The
-- victim is placed where the throw puts it, plays its side of the throw with
-- no control, and is hurt when it hits the floor. Facing each other gives a
-- front throw; coming from behind, a throw from behind.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/throw.lua

local THROW_FORWARD_PUNCH, THROW_BEHIND_KICK = 106, 107 + 2
local function thrown(t) return (t >= 96 and t <= 101) or (t >= 114 and t <= 119) or (t >= 128 and t <= 132) end

step(20)
local px, py, pz = pos()
facing(0)

-- A dummy 8 units in front, facing Konoko.
local d = spawn(px, py + 5, pz + 8, 180)
step(60)
facing(0)
local hp0 = hp(d)

input(true, false); punch(); step(1); input(false, false)
local mine, theirs = anim_type(), type_of(d)
print(string.format("forward punch near a facing dummy: me %d (%s), dummy %d (%s)", mine, anim(), theirs, anim_of(d)))
assert(mine == THROW_FORWARD_PUNCH, "the forward punch became a front throw")
assert(thrown(theirs), "the dummy plays its side of the throw")
local mx, _, mz = pos()
local tx, _, tz = pos_of(d)
local gap = math.sqrt((tx - mx)^2 + (tz - mz)^2)
print(string.format("victim placed %.1f units away (the throw puts it ~9.8 ahead)", gap))
assert(math.abs(gap - 9.8) < 1.0, "the victim is placed by the throw's offset")

local hurt = false
for _ = 1, 160 do
    step(1)
    if hp(d) < hp0 then hurt = true end
end
print(string.format("after the throw: dummy hp %d -> %d, state %d", hp0, hp(d), state_of(d)))
assert(hurt, "the throw hurts the victim")

-- From behind: Konoko turns round; a second dummy stands 8 units ahead of
-- her, facing the same way she does (its back to her).
step(120)
set_facing(0, 180)
step(5)
local qx, qy, qz = pos()
local e = spawn(qx, qy + 5, qz - 8, 180)
step(60)
set_facing(0, 180)
step(1)
input(true, false); kick(); step(1); input(false, false)
print(string.format("forward kick at a dummy's back: me %d (%s), dummy %d (%s)", anim_type(), anim(), type_of(e), anim_of(e)))
assert(anim_type() == THROW_BEHIND_KICK, "a throw from behind")
assert(thrown(type_of(e)), "the dummy is thrown")

-- Out of reach: a forward punch is just a punch.
step(240)
local qx, qy, qz = pos()
set_facing(0, 0)
local far = spawn(qx, qy + 5, qz + 60, 180)
step(60)
set_facing(0, 0)
input(true, false); punch(); step(1); input(false, false)
print(string.format("forward punch with nobody near: me %d (%s)", anim_type(), anim()))
assert(anim_type() == 31, "no throw without a victim in reach: the plain forward punch")

-- Running: a punch while running up to someone is a running throw.
step(90)
set_facing(0, 0)
input(true, false)
local done = false
for _ = 1, 200 do
    local x, _, z = pos()
    local fx, _, fz = pos_of(far)
    if not done and math.sqrt((fx - x)^2 + (fz - z)^2) < 13 then punch(); done = true end
    step(1)
    if done and anim_type() == 110 then break end
end
input(false, false)
print(string.format("punch while running in: me %d (%s), victim %d", anim_type(), anim(), type_of(far)))
assert(anim_type() == 110, "a running throw")
assert(thrown(type_of(far)), "the running throw's victim is thrown")
print("throw ok: front, behind and running throws; victim placed, locked out and hurt")
