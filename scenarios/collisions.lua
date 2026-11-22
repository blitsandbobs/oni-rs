-- What movement meets: the counts the original
-- keeps of walls, objects and characters met, and an AI's records of them —
-- one met for a while weighs against moving toward it, and it moves the
-- least bad way near the one it wants.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/collisions.lua

step(20)
facing(0)
local x, y, z = pos()

-- An AI walks to a spot with someone standing in its way: it bumps into
-- them, notes it, and steers round them to get there.
local walker = spawn(x + 30, y + 5, z, 0)
local blocker = spawn(x + 33, y + 5, z + 40, 180)
step(40)
go_to(walker, x + 30, y, z + 90, "walk")
local met, records, got = 0, 0, nil
for t = 1, 900 do
    step(1)
    local _, _, chars, r = collisions(walker)
    met = met + chars
    records = math.max(records, r)
    if nav_state(walker) == "arrived" then got = t break end
end
print("walker: met the other", met, "ticks, records", records, "arrived after", got)
assert(met > 0 and records > 0, "it bumped into them and noted it")
assert(got, "and steered round them")

-- Konoko runs into the low wall at the hall's end: walls met, every tick.
input("forward")
local walls = 0
for _ = 1, 300 do
    step(1)
    local w = collisions(0)
    walls = math.max(walls, w)
end
input()
print("Konoko against the wall: walls met", walls)
assert(walls > 0, "a wall in the way is counted")
print("collisions ok")
