-- A body that falls out of the world (ONI_Character.c: in the air over
-- ONcMaxFramesInAir, 5 s, it takes all its hit points; dying and still
-- falling 30 ticks later it goes with no corpse). Off level 12's roof, west
-- of its wall, there is nothing below: a striker dropped there dies in the
-- air and is gone without a corpse. (Found by the shakedown tour: it fell
-- forever, its fatal fall a stub and a falling corpse's air time frozen.)
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/12/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 1735,650,-3100 --script scenarios/fall_out_of_world.lua

step(10)
local corpses0 = #corpses()
local d = spawn(1500, 700, -3100, 0, "striker_easy_1")
local died, gone
for t = 1, 600 do
  step(1)
  if not died and dead(d) then died = t end
  if not present(d) then gone = t break end
end
local _, y = pos_of(d)
print(string.format("died at tick %s, gone at tick %s (y %.0f)", tostring(died), tostring(gone), y))
assert(died and died >= 290 and died <= 320, "dies after 5 s in the air")
assert(gone and gone <= died + 60, "and is gone soon after")
assert(#corpses() == corpses0, "with no corpse")
print("ok")
