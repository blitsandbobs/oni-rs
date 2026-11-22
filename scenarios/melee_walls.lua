-- The melee AI looks along the floor before it moves: maneuvers steer round
-- walls (and ledges), and give up rather than grind into one. An AI with its
-- back to a wall, told to retreat, doesn't keep backpedalling into the wall.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/melee_walls.lua

local RUN_BACKWARDS, RUN_BACKWARDS_START = 9, 218
step(20)
local px, py, pz = pos()
local free = probe(0, 180, 100)
print(string.format("wall behind the start: %.0f units", free))
assert(free < 40, "this test needs the wall behind the start point")

-- The AI 4 units in front of the wall, facing its target away from it.
local d = spawn(px, py + 5, pz + 2, 180)
local a = spawn(px, py + 5, pz - free + 4, 0)
step(20)
spar(a, d)
technique(a, "Retreat")
local stuck, worst, lastz = 0, 0, nil
for _ = 1, 240 do
    step(1)
    local _, _, z = pos_of(a)
    local t = type_of(a)
    if (t == RUN_BACKWARDS or t == RUN_BACKWARDS_START) and lastz and math.abs(z - lastz) < 0.05 then
        stuck = stuck + 1
        if stuck > worst then worst = stuck end
    else
        stuck = 0
    end
    lastz = z
end
print(string.format("longest backpedal into the wall: %d ticks", worst))
assert(worst < 20, "the AI doesn't grind backwards into the wall")
print("melee walls ok: maneuvers look before they move")
