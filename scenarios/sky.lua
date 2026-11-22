-- The sky: the level's skybox drawn round the camera before
-- the level, unfogged. Level 2's sunset: from high above the level, the sky
-- is orange-red above the horizon, brightest toward +z (the sun painted into
-- its back face), and its faces meet where the camera looks across a corner.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,-500,0 --script scenarios/sky.lua

step(2)
local function sky(name, tx, ty, tz)
  local path = temp_path(name)
  cam(0, 1500, 0, tx, ty, tz)
  screenshot(path, 320, 180)
  -- The upper middle of the frame (clear of the HUD's rings).
  return image_mean(path, 0.35, 0.1, 0.65, 0.4)
end

local r, g, b = sky("sky_sun.png", 0, 1530, 100)
print(string.format("toward the sun: %.2f %.2f %.2f", r, g, b))
assert(r > 0.6 and g > 0.25 and b < 0.3, "a sunset toward +z")

local r2, g2, b2 = sky("sky_east.png", 100, 1530, 0)
print(string.format("east: %.2f %.2f %.2f", r2, g2, b2))
assert(r2 > 0.2 and r2 > b2, "sky to the east too")
assert(r + g > r2 + g2, "brighter toward the sun")

local r3, g3, b3 = sky("sky_up.png", 1, 1600, 0)
print(string.format("up: %.2f %.2f %.2f", r3, g3, b3))
assert(r3 > 0.2, "the top face")

-- Across a corner the two faces meet: the frame's two halves alike.
local path = temp_path("sky_corner.png")
cam(0, 1500, 0, 100, 1530, 100)
screenshot(path, 320, 180)
local lr, lg = image_mean(path, 0.4, 0.2, 0.49, 0.4)
local rr, rg = image_mean(path, 0.51, 0.2, 0.6, 0.4)
print(string.format("corner: %.2f %.2f | %.2f %.2f", lr, lg, rr, rg))
assert(math.abs(lr - rr) < 0.12 and math.abs(lg - rg) < 0.12, "the faces meet")
print("sky ok")
