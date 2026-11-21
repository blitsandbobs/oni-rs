-- S5 (docs/SCRIPT_FIXES.md): level 12's boss, Mukade, knocked off the roof's
-- open north end onto the lower roof below (still above the height his AI
-- brings him back from) can't climb back. With Konoko near the edge he never
-- teleports either: the fight, the level's end, stalls (--faithful). Fixed:
-- stranded for 3 seconds, he teleports back to the roof's centre.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/12/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/fix_mukade_stranded.lua

mission_begin(true, 4) -- the fight, restored
step(60)
local n = named("OutroNinja")
assert(n, "Mukade is there")
teleport(0, 1685, 650, -2960) -- at the roof's north edge
teleport(n, 1685, 632, -2900) -- on the lower roof below it
local back
for t = 1, 900 do
  step(1)
  set_hp(0, 100000)
  local x, y, z = pos_of(n)
  if not back and y > 640 and z < -2946 and z > -3326 and x > 1588 and x < 1882 then back = t end
end
print("back on the roof at tick", back, pos_of(n))
if faithful() then
  assert(not back, "shipped: stranded")
else
  assert(back and back < 400, "fixed: teleported back")
end
assert(#mission_info().errors == 0, "no script errors")
print("ok")
