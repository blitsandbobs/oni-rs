-- Pathfinding obstructions (#89): characters, placed objects and door
-- leaves are stamped into the rooms' grids (`PHrPrepareRoomForPath`), and
-- whether a stamp blocks depends on who asks (`PHrIgnoreObstruction`): never
-- one's own body; for an AI, not its combat target; objects always (Q29:
-- as shipped, an object stamps only its last row); a door if it is open or
-- can't be opened.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/pathfinding_obstructions.lua

mission_begin()
step(5)
mission_exec([[
F.t_free = function() C.input(1) C.lock_keys("keys_all") end
F.t_clear = function() C.obj_kill(0, 100000) end
F.t_truck = function() C.obj_create(201, 207) end
F.go = function() C.ai2_attack("Bay2_Thug_1", 0) end
]])
mission_call("t_free")
mission_call("t_clear")
teleport(0, 200, 8, 300)
step(20)
local px, py, pz = pos()

-- Two dummies: each is an obstruction to the other, never to itself.
local a = spawn(px + 20, py + 5, pz, 0)
local b = spawn(px - 20, py + 5, pz, 0)
step(25) -- (stamps 20 ticks old are made again)
local ax, ay, az = pos_of(a)
local bx, by, bz = pos_of(b)
local kind, who, blocks = nav_obstruction(bx, by, bz, a)
print("at b, for a:", kind, who, blocks)
assert(kind == "character" and who == b and blocks, "b's body blocks a")
kind, who, blocks = nav_obstruction(ax, ay, az, a)
assert(kind == "character" and who == a and not blocks, "a's own body doesn't")
-- A 5×5 block less its corners: two squares out (away from the player's
-- own block) is stamped, three isn't.
assert(nav_obstruction(bx - 8, by, bz, a) == "character", "two squares out")
assert(nav_obstruction(bx - 12, by, bz, a) == nil, "not three")

-- Its local probe (melee, dodging) stops at a body too: Konoko stands 20
-- along -x (her block reaches 8 or so toward it).
local free, stopped = nav_probe(a, -90, 30)
print(string.format("a's probe toward Konoko: free %.1f, stopped at %s", free, tostring(stopped)))
assert(free < 14 and stopped == "impassable", "her body stops a's probe")
free = nav_probe(a, 90, 15)
assert(free >= 15, "the other way is clear")

-- Come into play inside another body, an AI steps out of it, 15 away
-- (`AI2iMovementState_FindClearPoint`).
local c = spawn(bx + 1, by + 5, bz, 0)
local cx2, _, cz2 = pos_of(c)
local moved = math.sqrt((cx2 - bx) ^ 2 + (cz2 - bz) ^ 2)
print(string.format("spawned on b: now %.1f from it", moved))
assert(moved > 12, "it stepped out of b's body")
teleport(c, px + 200, py + 5, pz + 200) -- (out of the way)

-- An AI attacking the player may path through her (its combat target);
-- another body still blocks it.
local t = place("Bay2_Thug_1")
teleport(t, px + 40, py + 5, pz + 40)
assert(mission_call("go"))
step(25)
print("thug:", goal_of(t))
assert(goal_of(t):find("combat"), "it fights her")
kind, who, blocks = nav_obstruction(px, py, pz, t)
print("at the player, for the thug:", kind, who, blocks)
assert(kind == "character" and who == 0 and not blocks, "its target doesn't block it")
kind, who, blocks = nav_obstruction(bx, by, bz, t)
assert(kind == "character" and blocks, "a bystander does")

-- A placed object: the truck by the loading bay.
mission_call("t_truck")
step(25)
local o = objects()
assert(#o > 0, "the truck is there")
local seen, centre = 0, 0
for k, ob in ipairs(o) do
    local x, y, z = ob.at[1], ob.at[2], ob.at[3]
    local what, idx, bl = nav_obstruction(x, y + 2, z, a)
    if what == "object" then
        centre = centre + 1
        assert(bl, "an object blocks")
    end
    -- Anything at all stamped within 8 of it.
    for dz = -8, 8, 4 do
        if nav_obstruction(x, y + 2, z + dz, a) == "object" then
            seen = seen + 1
            break
        end
    end
end
print(string.format("truck parts: %d, stamped at their centre: %d, near them: %d", #o, centre, seen))
if faithful() then
    -- (A part's centre may fall in a neighbouring part's row.)
    assert(centre < #o, "as shipped, only an object's last row is stamped")
else
    assert(centre == #o, "an object's whole square is stamped")
end
-- A door: its leaf's base edge is stamped; it blocks only one that can't
-- open it (here: locked), or once open (its leaves stand where they slid to).
-- Door 2 (unlocked) is closed across z = -589.5, x -579.5 to -549.5.
step(25)
local dx, dy, dz = -564.5, 51, -589.5
kind, who, blocks = nav_obstruction(dx, dy, dz, a)
print("closed door 2:", kind, who, blocks)
assert(kind == "door" and not blocks, "a closed door one may open doesn't block")
door(2, "lock")
step(25)
kind, who, blocks = nav_obstruction(dx, dy, dz, a)
assert(kind == "door" and blocks, "a locked one does")
door(2, "unlock")
door(2, "open")
step(80)
assert(door_state(2) == "open", "it opened")
assert(nav_obstruction(dx, dy, dz, a) == nil, "open, the doorway is clear")
local e = door_obstruction(2)[1]
local ox, oz = (e[1] + e[4]) / 2, (e[3] + e[6]) / 2
kind, who, blocks = nav_obstruction(ox, dy, oz, a)
print(string.format("open door 2, where its leaf went (%.1f %.1f):", ox, oz), kind, who, blocks)
assert(kind == "door" and blocks, "and its leaf blocks where it slid to")
print("pathfinding obstructions ok")
