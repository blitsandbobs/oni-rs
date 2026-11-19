-- Decals (docs/PARTICLES.md, *Decals*): level 1's own (signs, vents, the
-- training's track bands) are laid on their walls at the start, 0.05 off
-- the surface; a pistol round into a wall leaves a bullet hole there and
-- throws its sparks off the wall along its normal.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/decals.lua

step(2)

-- How far a decal's corners are from the plane of the quad it was made on.
local function off_plane(d)
    local a, b, c, dd = quad_plane(d.quad)
    local lo, hi = math.huge, -math.huge
    for _, p in ipairs(d.positions) do
        local e = a * p[1] + b * p[2] + c * p[3] + dd
        lo, hi = math.min(lo, e), math.max(hi, e)
    end
    return lo, hi
end

local level = decals()
print("level decals:", #level)
assert(#level == 116, "level 1 lays its 116 decals")
local flat = 0
for _, d in ipairs(level) do
    assert(d.static, "a level's decal is kept for good")
    assert(d.alpha == 255, "fully opaque")
    local lo, hi = off_plane(d)
    if math.abs(lo - 0.05) < 1e-3 and math.abs(hi - 0.05) < 1e-3 then flat = flat + 1 end
end
print("flat on their walls, 0.05 off:", flat)
assert(flat >= 100, "most lie flat, lifted 0.05 off their wall")

-- A pistol round into the wall ahead.
local px, py, pz = pos()
facing(0)
step(5)
assert(give_weapon(0, "w1_tap"), "a pistol")
step(2)
aim(0, 0.1)
fire(1)
local sparks, spark = 0, nil
for _ = 1, 30 do
    step(1)
    local s = particles_of("w1_tap_x01")
    sparks = math.max(sparks, #s)
    spark = spark or s[1]
end
local holes = decals("d__BULLETHOLE")
print("bullet holes:", #holes, "sparks seen:", sparks)
assert(#holes == 1, "the round leaves a bullet hole")
local h = holes[1]
assert(not h.static, "a bullet hole is dynamic")
local lo, hi = off_plane(h)
local span = 0
for _, p in ipairs(h.positions) do
    for _, q in ipairs(h.positions) do
        span = math.max(span, math.sqrt((p[1] - q[1]) ^ 2 + (p[2] - q[2]) ^ 2 + (p[3] - q[3]) ^ 2))
    end
end
print("hole off its wall:", lo, hi, "vertices", h.vertices, "across", span)
-- Half-size 0.38–0.45: corner to corner 1.07–1.27.
assert(span > 1.0 and span < 1.3, "the hole is its class's size")
assert(lo > 0.04 and hi < 0.06, "the hole lies on the wall it hit, 0.05 off")
assert(sparks > 0, "the hit throws sparks")
-- The spark: 0.2 off the wall from the bullet's impact, 0.2 more from its
-- own placing; turned along the wall's normal.
local a, b, c, dd = quad_plane(h.quad)
local dist = a * spark[1] + b * spark[2] + c * spark[3] + dd
local along = a * spark[4] + b * spark[5] + c * spark[6]
print("spark off the wall:", dist, "facing out:", along)
assert(math.abs(dist - 0.4) < 0.02, "the spark sits 0.4 off the wall")
assert(along > 0.99, "the spark points out of the wall")
print("decals ok")
