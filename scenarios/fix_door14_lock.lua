-- S3 (docs/deviations.md): on level 18, the volume past door 14 locks the
-- door behind Konoko, open or not: still open, she can walk back out and be
-- locked out (--faithful). Fixed: it closes, then locks.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/18/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/fix_door14_lock.lua

mission_begin(false)
step(5)
door(14, "unlock")
door(14, "open")
step(60)
assert(mission_call("t45"))
step(5)
local state, locked = door_state(14)
print("door 14:", state, locked)
assert(locked, "it locks")
if faithful() then
  assert(state == "open", "shipped: left open")
else
  assert(state == "closing" or state == "closed", "fixed: closed first")
end
assert(#mission_info().errors == 0, "no script errors")
print("ok")
