-- Level 2's ambush (#61; docs/PATHFINDING.md, *Doors*): after the intro the
-- partner cops go for the Strikers and the Strikers come out of their room
-- to fight. A locked door links to the pathfinding connections the original
-- links it to (two slots, nearest first; a replaced one keeps its link) —
-- linking it to every opening near it walled the Strikers in and kept a cop
-- standing.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/ambush.lua

mission_begin()
-- The intro: until the player has control again.
local t = 0
repeat step(30) t = t + 30 until (hud().letterbox or 0) == 0 and t > 600 and not passive_of(named("partner_cop_1") or 1) or t > 9000
print("intro over at tick", t)

local cops = { named("partner_cop_1"), named("partner_cop_2") }
local strikers = { named("ambush_striker_1"), named("ambush_striker_2"), named("ambush_striker_3") }
local start = {}
for _, s in ipairs(strikers) do local x, _, z = pos_of(s) start[s] = { x, z } end

local melee = {}
for _ = 1, 900 do
  step(1)
  for _, id in ipairs(cops) do
    if goal_of(id):match("Melee") then melee[id] = true end
  end
  for _, id in ipairs(strikers) do
    if goal_of(id):match("Melee") then melee[id] = true end
  end
end
for _, id in ipairs(cops) do print("cop", id, goal_of(id), "fought:", tostring(melee[id] or false)) end
local moved = 0
for _, s in ipairs(strikers) do
  local x, _, z = pos_of(s)
  local d = math.sqrt((x - start[s][1]) ^ 2 + (z - start[s][2]) ^ 2)
  print(string.format("striker %d moved %.0f, fought: %s", s, d, tostring(melee[s] or false)))
  if d > 20 then moved = moved + 1 end
end
for _, id in ipairs(cops) do assert(melee[id], "both partner cops fight the Strikers") end
assert(moved >= 2, "the Strikers come out to fight")
print("ok")
