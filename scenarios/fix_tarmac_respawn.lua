-- S2 (docs/SCRIPT_FIXES.md): restored at level 4's save point 3, the tarmac's
-- volumes are still live: walking back onto it spawns its guards again and
-- saves the game back at save point 2 (--faithful). Fixed: they are off, as
-- the save point 2 restore has them.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/04/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/fix_tarmac_respawn.lua

mission_begin(true, 3)
step(120)
teleport(0, -97, -103, 1014) -- the tarmac's volume
step(60)
local guard = named("Tarmac_Striker_1")
print("tarmac guard:", guard, "save point:", game().save_point)
if faithful() then
  assert(guard, "shipped: the tarmac's guards come back")
else
  assert(not guard, "fixed: they stay gone")
  assert(game().save_point ~= 2, "and the game isn't saved back at save point 2")
end
assert(#mission_info().errors == 0, "no script errors")
print("ok")
