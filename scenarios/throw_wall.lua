-- Q25, throws through walls (docs/QUIRKS.md): with a locked door between
-- them, Konoko's forward punch at a dummy 8 away on the other side stays a
-- punch (fixed). The shipped throw search (`AttemptThrow`) takes the nearest
-- foe in reach without looking for a wall, so it becomes a throw through the
-- door (--faithful).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/throw_wall.lua

local THROW_FORWARD_PUNCH = 106

-- Door 18 (across z = -590), locked: a wall.
step(5)
door(18, "lock")
teleport(0, -82, 2, -594)
facing(0)
local d = spawn(-82, 7, -586, 180)
step(60)
assert(door_state(18) == "closed", "the door stays shut")
local _, _, kz = pos()
local _, _, dz = pos_of(d)
print(string.format("Konoko at z %.1f, the dummy at %.1f", kz, dz))
assert(kz < -590 and dz > -590 and dz - kz < 12, "either side of the door, in reach")
facing(0)
input(true, false); punch(); step(1); input(false, false)
local mine = anim_type()
print(string.format("punch at a dummy behind the door: %d (%s)", mine, anim()))
if faithful() then
  assert(mine == THROW_FORWARD_PUNCH, "shipped: thrown through the door")
else
  assert(mine ~= THROW_FORWARD_PUNCH, "fixed: no throw through the door")
end
print("ok")
