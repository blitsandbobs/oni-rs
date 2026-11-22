-- Starting a ground attack turns the attacker toward the nearest opponent in
-- front (within 90° and 40 units), so a slightly mis-aimed punch connects.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/autoface.lua

step(20)
local px, py, pz = pos()
local d = spawn(px, py + 5, pz + 8, 0) -- straight ahead (+Z), back turned
step(60)

local function wrap(a) a = a % 360; if a > 180 then a = a - 360 end; return a end

-- Aim 35° off to the left, then punch: the body snaps onto the dummy.
facing(35)
step(2)
punch()
step(2)
local dx, _, dz = pos_of(d)
local x, _, z = pos()
local toward = math.deg(math.atan(dx - x, dz - z))
local err = wrap(facing_deg() - toward)
print(string.format("facing %.1f°, dummy at %.1f°, error %.1f°", wrap(facing_deg()), toward, err))
assert(math.abs(err) < 3, "the punch turned the player onto the dummy")
print("autoface ok")
