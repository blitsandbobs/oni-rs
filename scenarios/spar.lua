-- The sparring partner (a stand-in melee AI, issue #5): it closes in on its
-- opponent, strikes with combos through the same move rules as the player,
-- is stopped by its opponent's body (characters can't walk into each other,
-- issue #29), gets up after a knockdown, and stops once its opponent is down
-- for good.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/spar.lua

local STANDING, FALLEN_BACK, FALLEN_FRONT = 7, 18, 50
local function dist(a, b)
    local ax, _, az = pos_of(a); local bx, _, bz = pos_of(b)
    return math.sqrt((ax - bx)^2 + (az - bz)^2)
end

step(20)
local px, py, pz = pos()
-- A passive target on open floor, its back to the sparring partner, which
-- starts 20 units behind it.
local d = spawn(px + 10, py + 5, pz + 4, 0)
local a = spawn(px + 10, py + 5, pz - 16, 0)
step(60)
team(d, "konoko") -- an enemy of the (Syndicate) attacker: friends' blows pass through
spar(a, d)

-- It closes in, never walks through its opponent, and lands hits.
local hp0, closed, closest = hp(d), nil, 99
for t = 1, 600 do
    step(1)
    local r = dist(a, d)
    if r < closest then closest = r end
    if not closed and r < 9 then closed = t end
end
print(string.format("closed in at t=%s, closest %.1f, target hp %d -> %d", tostring(closed), closest, hp0, hp(d)))
assert(closed, "the sparring partner closed in")
assert(closest > 3.0, "it never walked into its opponent's body (closest " .. closest .. ")")
assert(hp(d) < hp0 - 15, "it landed several hits")

-- Knocked down, it gets back up by itself.
knock(a, 79)
local up = nil
for t = 1, 300 do
    step(1)
    if t > 20 and state_of(a) == STANDING then up = t; break end
end
print("got up after " .. tostring(up) .. " ticks")
assert(up, "the sparring partner got up after a knockdown")

-- Once its opponent is dead it stops attacking.
set_hp(d, 1)
for _ = 1, 600 do step(1); if dead(d) then break end end
assert(dead(d), "it finished its opponent")
step(120)
local busy = 0
for _ = 1, 120 do
    step(1)
    if anim_of(a):find("comb") then busy = busy + 1 end
end
print("attack ticks after the opponent died: " .. busy)
assert(busy == 0, "it stops fighting a dead opponent")
print("spar ok: closes in, hits, gets up, stops when its opponent is dead")
