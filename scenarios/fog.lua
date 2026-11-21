-- The fog (docs/RENDERING.md, *Fog*): linear in screen depth between its
-- start and end, toward its colour — the draw engine's (grey 0.25, 0.975 to
-- 1) on each level until its scripts set the `gl_fog_*` variables;
-- `gl_fog_start_changeto` eases the start over frames.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/fog.lua

mission_begin(false)
mission_exec([[
F.red = function() G.gl_fog_red = 1 G.gl_fog_green = 0 G.gl_fog_blue = 0 G.gl_fog_start = 0.9 end
F.clear = function() G.gl_fog_start = 1 G.gl_fog_end = 1 end
F.ease = function() C.gl_fog_start_changeto(0.99, 60) end
]])
step(5)
local r, g, b, s, e = fog()
print("default:", r, g, b, s, e)
assert(math.abs(r - 0.25) < 1e-6 and math.abs(s - 0.975) < 1e-6 and e == 1, "the draw engine's fog")

-- A red fog, close in: the far end of the warehouse goes red; with none
-- (start and end at the far plane) it doesn't.
local function far_red()
  teleport(0, 200, 8, 330)
  set_facing(0, 180)
  step(60)
  -- (A file of this run's own: the faithful pass runs it at the same time.)
  local p = temp_path("fog.png")
  screenshot(p, 160, 90)
  local r, g, b = image_mean(p, 0.3, 0.1, 0.7, 0.4)
  os.remove(p)
  return r, g, b
end
assert(mission_call("clear"))
step(1)
local cr, cg = far_red()
assert(mission_call("red"))
step(1)
local rr, rg = far_red()
print(string.format("far: clear r %.2f g %.2f, red fog r %.2f g %.2f", cr, cg, rr, rg))
assert(rr - rg > (cr - cg) + 0.2, "a red fog reddens the distance")

-- Eased: halfway after 30 ticks, there after 60.
assert(mission_call("ease"))
step(30)
local _, _, _, mid = fog()
step(32)
local _, _, _, done = fog()
print(string.format("eased start: %.4f then %.4f", mid, done))
assert(math.abs(mid - 0.945) < 0.003 and math.abs(done - 0.99) < 1e-4, "eased over its frames")
print("ok")
