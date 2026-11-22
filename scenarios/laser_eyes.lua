-- Laser emitters' eyes (#78): each emitter is drawn — its
-- class's emitter model, moving along the rail with it (`OBJiTrigger_Draw`).
-- Level 1's three training lasers: an eye at each beam's start, staying
-- there as the beams sweep.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/laser_eyes.lua

step(5)
teleport(0, -1060, -295, -470)
step(30)
for id = 1, 3 do laser(id, "activate") end
local function check(when)
  local eyes, beams_now = laser_eyes(), beams()
  assert(#eyes == 3 and #beams_now == 3, "three eyes, three beams")
  for _, b in ipairs(beams_now) do
    local near = 1e9
    for _, e in ipairs(eyes) do
      near = math.min(near, math.sqrt((e[1] - b[1]) ^ 2 + (e[2] - b[2]) ^ 2 + (e[3] - b[3]) ^ 2))
    end
    assert(near < 1.5, when .. ": an eye at each beam's start (" .. near .. ")")
  end
  return eyes[1][3]
end
step(1)
local z0 = check("at once")
step(100)
local z1 = check("after sweeping")
print(string.format("an eye moved along its rail from z %.1f to %.1f", z0, z1))
assert(math.abs(z1 - z0) > 5, "the eyes move with their emitters")
print("ok")
