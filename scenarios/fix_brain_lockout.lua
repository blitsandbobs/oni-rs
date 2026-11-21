-- S1 (docs/SCRIPT_FIXES.md): the Deadly Brain's save point (level 2) locks
-- door 26 wherever Konoko is. Run back out past it as the save comes and
-- the shipped game locks her out, the door left open and the save made with
-- her outside (--faithful). Fixed: the save waits until she is back in the
-- antechamber, then closes the door and locks it.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos -265,-74,262 --script scenarios/fix_brain_lockout.lua

mission_begin(false)
step(5)
door(26, "open")
step(60)
teleport(0, -265, -74, 262) -- past door 26, outside
step(5)
assert(mission_call("save_point_4"))
step(20)
local state, locked = door_state(26)
print("outside:", state, locked, game().save_point)
if faithful() then
  assert(locked and state ~= "closed", "shipped: locked, open, with her outside")
  assert(game().save_point == 4, "and saved so")
else
  assert(not locked and game().save_point == nil, "fixed: the save waits for her")
  teleport(0, -265, -74, 232) -- the antechamber
  step(10)
  state, locked = door_state(26)
  print("inside:", state, locked, game().save_point)
  assert(locked and (state == "closing" or state == "closed"), "then closes and locks the door")
  assert(game().save_point == 4, "and saves")
end
assert(#mission_info().errors == 0, "no script errors")
print("ok")
