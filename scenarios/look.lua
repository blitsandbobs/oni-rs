-- Where an AI looks (issue #30, `AI2rExecutor_UpdateAim`): an unarmed
-- agent told to look at a point turns its aim there — a sixth of a turn a
-- second at most — and its head and upper body follow through its class's
-- aiming screen; told to stop, the aim comes back to the middle and the
-- head with it.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/look.lua

local HEAD, PELVIS = 10, 0
-- The head's turn against the pelvis: the angle between their axes.
local function head_off(id)
    local h, p = bone(id, HEAD), bone(id, PELVIS)
    local function unit(v)
        local l = math.sqrt(v[1] ^ 2 + v[2] ^ 2 + v[3] ^ 2)
        return { v[1] / l, v[2] / l, v[3] / l }
    end
    local a, b = unit(h.y), unit(p.y)
    local d = a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
    return math.deg(math.acos(math.max(-1, math.min(1, d))))
end

step(20)
local px, py, pz = pos()
-- (Facing away from the player, so it has no one to glance at.)
local a = spawn(px + 12, py + 5, pz, 90)
team(a, "konoko")
goals(a)
step(90)
local ax, ay, az = pos_of(a)
local lr0, ud0 = aim_off(a)
assert(math.abs(lr0) < 1e-3 and math.abs(ud0) < 1e-3, "it starts looking ahead")
local h0 = head_off(a)

-- A point 35° off its facing (within the standing look range: the body
-- stays), a little above its shoulder.
local yaw = math.rad(90 + 35)
local tx, tz = ax + 40 * math.sin(yaw), az + 40 * math.cos(yaw)
look_at(a, tx, ay + 15 + 8, tz)
step(1)
local lr1 = aim_off(a)
print(string.format("first tick: %.2f°", lr1))
assert(math.abs(lr1) > 0 and math.abs(lr1) <= 6.01, "it swings at most 6° a tick")
step(40)
local lr, ud = aim_off(a)
local h1 = head_off(a)
print(string.format("looking: lr %.1f° ud %.1f°, head off the pelvis %.1f° → %.1f°", lr, ud, h0, h1))
assert(math.abs(math.abs(lr) - 35) < 3, "its aim is on the point")
assert(ud > 5, "and up toward it")
assert(math.abs(h1 - h0) > 10, "the head turned with the aim")

-- Stop looking: the aim comes back to the middle, the head with it.
look_at(a)
step(40)
local lr2, ud2 = aim_off(a)
local h2 = head_off(a)
print(string.format("stopped: lr %.1f° ud %.1f°, head %.1f°", lr2, ud2, h2))
assert(math.abs(lr2) < 1e-3 and math.abs(ud2) < 1e-3, "back to the middle")
assert(math.abs(h2 - h0) < 2, "the head back too")
print("ok")
