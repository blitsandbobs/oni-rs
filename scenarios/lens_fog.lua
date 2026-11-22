-- Lens flares fade with the fog (#76): a
-- fading flare loses as much as a thing at its depth would to the fog
-- (`P3rModifyAlphaByFogFromDepth`). In a thick fog the lights beyond its
-- end are gone (fixed); as shipped (Q28, --faithful) the fog's factor reads
-- 1 past its end and they shine on whole.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/lens_fog.lua

mission_begin(false)
step(30)
-- (Beside the warehouse door, its lock light in view: as lens.lua.)
teleport(0, -251, 2, -623)
set_facing(0, 150)
step(120)
local _, seen, clear = lens_flares()
mission_exec([[F.thick = function() G.gl_fog_start = 0.0 G.gl_fog_end = 0.1 end]])
assert(mission_call("thick"))
step(2)
local _, _, fogged = lens_flares()
print(string.format("flares seen %d; drawn in a clear fog %d, in a thick one %d", seen, clear, fogged))
assert(clear > 0, "flares in view")
if faithful() then
  assert(fogged == clear, "shipped: past the fog's end they shine on whole")
else
  assert(fogged < clear, "fixed: beyond the fog's end they are gone")
end
print("ok")
