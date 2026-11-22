-- Pathfinding (issue #2): a character walks to a point through the level —
-- across rooms (portals between room volumes) and around obstacles (each
-- room's grid) — and gives up cleanly on a point no room contains.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/pathfinding.lua

step(20)
local px, py, pz = pos()

local function walk(id, x, y, z, ticks)
    go_to(id, x, y, z)
    local travelled, lx, _, lz = 0, pos_of(id)
    local s
    for _ = 1, ticks do
        step(1)
        local ax, _, az = pos_of(id)
        travelled = travelled + math.sqrt((ax - lx)^2 + (az - lz)^2)
        lx, lz = ax, az
        s = nav_state(id)
        if s == "arrived" or s:find("failed") then break end
    end
    return s, travelled
end

-- Behind a pillar (straight ahead is blocked): it goes round. (She steps
-- aside: it starts where her body would be in its way.)
local a = spawn(px + 5, py + 5, pz, 0)
teleport(0, px - 30, py, pz + 30)
step(10)
local s, travelled = walk(a, px, py, pz - 40, 600)
print(string.format("behind the pillar: %s, travelled %.0f for a straight line of 40", s, travelled))
assert(s == "arrived", "it got there")
assert(travelled > 45, "it went round, not through")

-- Out of the warehouse, 300 west: through five rooms, each room's way
-- planned on coming into it (`AI2iPath_NewNode`).
local b = spawn(px + 5, py + 5, pz + 10, 0)
step(10)
go_to(b, px - 300, py, pz)
step(1)
local _, rooms = nav_legs(b)
local furthest = 0
travelled = 0
local lx, _, lz = pos_of(b)
for _ = 1, 1500 do
    step(1)
    local ax, _, az = pos_of(b)
    travelled = travelled + math.sqrt((ax - lx) ^ 2 + (az - lz) ^ 2)
    lx, lz = ax, az
    local leg = nav_legs(b)
    furthest = math.max(furthest, leg or 0)
    s = nav_state(b)
    if s == "arrived" or s:find("failed") then break end
end
print(string.format("300 west: %s, travelled %.0f, through %s rooms (reached room %d of them)",
    s, travelled, tostring(rooms), furthest + 1))
assert(s == "arrived", "it got there")
assert(rooms and rooms > 1 and furthest + 1 == rooms, "room after room")

-- A new order soon after, near the same place, keeps the path
-- (`AI2iPath_MakePath`: within 90 ticks and 20); far off, a new one.
go_to(b, px + 5, py, pz + 10)
step(5)
local _, _, planned = nav_legs(b)
go_to(b, px + 8, py, pz + 14)
step(2)
local _, _, kept = nav_legs(b)
assert(kept == planned, "kept for a point 5 off")
go_to(b, px - 60, py, pz + 10)
step(2)
local _, _, fresh = nav_legs(b)
print("path planned at", planned, "kept:", kept, "after a far order:", fresh)
assert(fresh and fresh ~= planned, "a new path for a far point")

-- Nowhere: high above the level.
local c = spawn(px - 5, py + 5, pz, 0)
step(10)
s = walk(c, px, py + 900, pz, 600)
print("into thin air: " .. s)
assert(s:find("failed"), "an unreachable point fails cleanly")
print("pathfinding ok: round obstacles, across rooms, and giving up")
