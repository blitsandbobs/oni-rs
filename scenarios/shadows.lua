-- Character shadows (docs/PARTICLES.md, *Shadows*): standing, Konoko's
-- shadow lies flat on the floor under her pelvis, 0.05 up, at her class's
-- opacity (48) and size (6 each way); jumping, it shrinks and fades with her
-- height; a script's `chr_shadow` takes it away and gives it back.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/shadows.lua

step(30)
local x, y, z = pos()
local s = shadow(0)
assert(s, "standing: a shadow")
local a, b, c, d = quad_plane(s.quad)
local lo, hi, minx, maxx = math.huge, -math.huge, math.huge, -math.huge
for _, p in ipairs(s.positions) do
    local e = a * p[1] + b * p[2] + c * p[3] + d
    lo, hi = math.min(lo, e), math.max(hi, e)
    minx, maxx = math.min(minx, p[1]), math.max(maxx, p[1])
end
print("standing: alpha", s.alpha, "vertices", s.vertices, "off the floor", lo, hi, "width", maxx - minx, "floor normal y", b)
assert(s.alpha == 48, "her class's opacity")
assert(b > 0.99, "on a floor")
assert(math.abs(lo - 0.05) < 0.01 and math.abs(hi - 0.05) < 0.01, "flat on the floor, 0.05 up")
assert(maxx - minx > 11 and maxx - minx < 12.5, "6 each way (clipped to the floor)")

-- Jump: smaller and fainter at the top.
jump()
local faintest, peak = 48, y
for _ = 1, 60 do
    step(1)
    local _, yy, _ = pos()
    peak = math.max(peak, yy)
    local t = shadow(0)
    if t then faintest = math.min(faintest, t.alpha) end
end
print("jump: peak", peak - y, "faintest", faintest)
assert(peak - y > 3, "she left the floor")
assert(faintest < 48, "the shadow fades as she rises")

-- Scripts take it away and give it back.
mission_begin()
step(5)
mission_exec([[F.no_shadow = function() C.chr_shadow(0, 0) end
F.shadow_on = function() C.chr_shadow(0, 1) end]])
mission_call("no_shadow")
step(2)
assert(shadow(0) == nil, "chr_shadow 0: none")
mission_call("shadow_on")
step(2)
assert(shadow(0), "chr_shadow 1: back")
print("shadows ok")
