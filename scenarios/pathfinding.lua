-- Pathfinding (issue #2): a character walks to a point through the level —
-- across rooms (portals between room volumes) and around obstacles (each
-- room's grid) — and gives up cleanly on a point no room contains.
--
--   cargo run --release -p oni-view -- \
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

-- Behind a pillar (straight ahead is blocked): it goes round.
local a = spawn(px + 5, py + 5, pz, 0)
step(10)
local s, travelled = walk(a, px, py, pz - 40, 600)
print(string.format("behind the pillar: %s, travelled %.0f for a straight line of 40", s, travelled))
assert(s == "arrived", "it got there")
assert(travelled > 45, "it went round, not through")

-- Across the warehouse.
local b = spawn(px + 5, py + 5, pz + 10, 0)
step(10)
s, travelled = walk(b, px + 120, py, pz + 120, 1500)
print(string.format("across the warehouse: %s, travelled %.0f", s, travelled))
assert(s == "arrived", "it crossed the warehouse")

-- Nowhere: high above the level.
local c = spawn(px - 5, py + 5, pz, 0)
step(10)
s = walk(c, px, py + 900, pz, 600)
print("into thin air: " .. s)
assert(s:find("failed"), "an unreachable point fails cleanly")
print("pathfinding ok: round obstacles, across rooms, and giving up")
