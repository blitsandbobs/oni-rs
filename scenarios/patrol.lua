-- AI movement (issue #1): the AI "presses the buttons" — its orders become the
-- same controls a player uses, so the character moves with its ordinary
-- locomotion clips. A route follower walks a square; held orders backpedal
-- and sidestep.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/patrol.lua

local STAND, IDLE = 6, 93
local function dist(x, z, p) return math.sqrt((x - p[1])^2 + (z - p[2])^2) end

step(20)
local px, py, pz = pos()
-- An AI character 10 units to the player's +X side (open floor), facing +Z.
local a = spawn(px + 10, py + 5, pz, 0)
step(60)
local ax, _, az = pos_of(a)

-- 1. Run a 15-unit square and come back to the start.
local pts = { {ax, az + 15}, {ax + 15, az + 15}, {ax + 15, az}, {ax, az} }
patrol(a, pts, "run")
local reached, order = {}, {}
local done_at = nil
for t = 1, 600 do
    step(1)
    local x, _, z = pos_of(a)
    for i, p in ipairs(pts) do
        if not reached[i] and dist(x, z, p) < 3.0 and (i == 1 or reached[i - 1]) then
            reached[i] = t
            order[#order + 1] = i
        end
    end
    if route_done(a) and not done_at then done_at = t end
end
print("corners reached at ticks: " .. table.concat({reached[1] or -1, reached[2] or -1, reached[3] or -1, reached[4] or -1}, ", "))
assert(#order == 4, "visited all four corners in order, got " .. #order)
assert(done_at, "the route finished")
local t = type_of(a)
assert(t == STAND or t == IDLE, "comes to rest at the end, got type " .. t)

-- 2. Held orders, sidestep right: facing +Z, the character's right is -X.
set_facing(a, 0)
ai_order(a, "stop")
step(30)
local x0 = pos_of(a)
ai_order(a, "right", "run", 0)
step(40)
local x1 = pos_of(a)
print(string.format("sidestep right: x %.1f -> %.1f (type %d)", x0, x1, type_of(a)))
assert(x1 < x0 - 2.0, "sidestepping right moved it toward -X")

-- 3. Backpedal (walking) moves it backward relative to its facing.
ai_order(a, "stop")
step(60)
local _, _, z0 = pos_of(a)
ai_order(a, "backpedal", "walk", 0)
step(90)
local _, _, z1 = pos_of(a)
print(string.format("backpedal: z %.1f -> %.1f (type %d)", z0, z1, type_of(a)))
assert(z1 < z0 - 2.0, "backpedalling moved it back (-Z)")
ai_order(a, "stop")
step(60)

print("patrol ok: route followed with ordinary locomotion; backpedal and sidestep orders work")
