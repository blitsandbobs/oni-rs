-- Body emitters (#74): overpowered, Konoko
-- glows all over — the glow's emitter puts its particles on her bones, at
-- a radius (`bodyBones`; Muro's glows and his bolts' emitter use her
-- surface the same way), riding the parts they came from. Before, all
-- came from one point at her hips.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --play --script scenarios/body_glow.lua

step(10)
set_hp(0, 400)
step(90)
local ps = particles_of("superglow_p01")
local _, y0 = pos()
local lo, hi = 1e9, -1e9
for _, p in ipairs(ps) do
  lo = math.min(lo, p[2] - y0)
  hi = math.max(hi, p[2] - y0)
end
print(string.format("%d glow particles, %.1f to %.1f above her feet", #ps, lo, hi))
assert(#ps > 20, "she glows")
assert(lo < 3 and hi > 13, "from her feet to her head")
print("ok")
