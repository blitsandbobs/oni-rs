-- Jumping: a standing jump rises off the ground, plays the jump clips
-- (start → air → land), then lands grounded. A running jump carries forward.
--
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/jump.lua

-- Settle standing.
step(30)
local _, y0, _ = pos()
assert(grounded(), "grounded before the jump")

-- Jump straight up; track the peak height and which clips play.
jump()
local peak = y0
local saw_air = false
local saw_land = false
local was_airborne = false
for _ = 1, 80 do
    step(1)
    local _, y, _ = pos()
    peak = math.max(peak, y)
    if airborne() then was_airborne = true end
    if anim():find("jump") and airborne() then saw_air = true end
    if anim():find("land") then saw_land = true end
end
print(string.format("standing jump: peak=%.1f (start y=%.1f) sawAir=%s sawLand=%s final anim=%s grounded=%s",
    peak, y0, tostring(saw_air), tostring(saw_land), anim(), tostring(grounded())))

assert(was_airborne, "the character left the ground")
assert(peak - y0 > 3.0, "the jump gains height, peak rise = " .. (peak - y0))
assert(saw_air, "a jump (airborne) clip played")
assert(saw_land, "a land clip played on touchdown")
assert(grounded(), "landed back on the ground")

-- Running jump: momentum carries forward through the arc.
facing(0)
input(true, false)
step(80)                      -- reach the run loop
local _, _, z0 = pos()
jump()
step(40)                      -- airborne arc
local _, _, z1 = pos()
print(string.format("running jump carried dZ=%.1f", z1 - z0))
assert(z1 - z0 > 8.0, "a running jump carries forward momentum, dZ=" .. (z1 - z0))

cam(214, 8, 300,  200, 8, 300)
screenshot("/tmp/oni_jump.png", 700, 1000)
