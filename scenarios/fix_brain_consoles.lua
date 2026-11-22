-- S4 (docs/deviations.md): restoring level 2's save point 4 (the Deadly
-- Brain) counts one console off the brain's next round: 3 of its 4 consoles
-- end it (--faithful). Fixed: all 4.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/fix_brain_consoles.lua

mission_begin(true, 4)
step(120)
local want = faithful() and 3 or 4
mission_exec(string.format(
  "if V.console_count ~= %d then error('consoles to go: ' .. tostring(V.console_count)) end", want))
print("consoles to go:", want)
assert(#mission_info().errors == 0, "no script errors")
print("ok")
