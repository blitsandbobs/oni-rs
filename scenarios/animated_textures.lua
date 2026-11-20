-- Animated level textures (docs/PARTICLES.md, *Animated textures*): level
-- 3's goo runs through its 15 frames at 5 ticks each, by the game time
-- (`M3iResolveTexture`) — five ticks on, one frame on.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/animated_textures.lua

local function goo()
  screenshot("/tmp/oni_animated_textures.png", 32, 24)
  for _, a in ipairs(animated_textures()) do
    if a.name == "GOO~1" then return a.frame end
  end
end

step(3)
local seen = {}
local f = goo()
assert(f, "the level's goo animates")
for _ = 1, 20 do
  step(5)
  local g = goo()
  assert(g == (f + 1) % 15, string.format("five ticks on, one frame on (%d → %d)", f, g))
  seen[g] = true
  f = g
end
local n = 0
for _ in pairs(seen) do n = n + 1 end
print("goo frames seen:", n)
assert(n == 15, "round all 15 frames")
print("ok")
