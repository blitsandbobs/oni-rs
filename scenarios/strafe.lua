-- Strafe: holding step-left sidesteps sideways while the body keeps facing
-- forward (A/D are stepleft/stepright in Oni, not turn).
--
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/strafe.lua

-- Face +Z (forward); strafe left. Motion should be lateral (X), not forward (Z).
facing(0)
local x0, _, z0 = pos()
strafe(true, false)
step(50)
local x1, y1, z1 = pos()
local dx, dz = x1 - x0, z1 - z0
print(string.format("strafed to %.1f,%.1f,%.1f  dX=%.1f dZ=%.1f  anim=%s",
    x1, y1, z1, dx, dz, anim()))

assert(math.abs(dx) > 2.0, "she travelled sideways, got dX=" .. dx)
assert(math.abs(dx) > math.abs(dz) * 2.0,
    "motion is lateral, not forward: dX=" .. dx .. " dZ=" .. dz)
assert(anim():find("ss") ~= nil, "a sidestep clip is playing, got " .. anim())
-- Body never turned to face the movement — still looking forward.
assert(math.abs(facing_deg()) < 5.0, "body keeps facing forward, got " .. facing_deg())
assert(grounded(), "still on the ground")

cam(x1 + 2, 10, z1 - 14,  x1, 6, z1)
screenshot("/tmp/oni_strafe.png", 760, 900)

-- Diagonal strafe: forward + step-right travels forward AND to the player's
-- right, playing the forward run clip. As in Oni, the body leans into the
-- diagonal (a -45° facing modifier) while the aim — and the camera — stays put. "Right" is
-- camera.right() = forward × Y = -X at yaw 0, so the move is -X (toward the
-- player's right) and +Z (forward) — NOT +X (that would be screen-left).
facing(0)
input(true, false)      -- forward
strafe(false, true)     -- + step-right
local dx0, _, dz0 = pos()
step(40)
local dx2, _, dz2 = pos()
local ddx, ddz = dx2 - dx0, dz2 - dz0
print(string.format("diagonal fwd+right: dX=%.1f dZ=%.1f anim=%s", ddx, ddz, anim()))
assert(ddx < -3.0 and ddz > 3.0,
    "travels diagonally forward-right (-X and +Z): dX=" .. ddx .. " dZ=" .. ddz)
assert(anim():find("run") ~= nil, "diagonal uses a run clip, got " .. anim())
local lean = facing_deg()
if lean > 180 then lean = lean - 360 end
print(string.format("diagonal fwd+right: body %.1f°, aim %.1f°", lean, aim_deg()))
assert(math.abs(lean + 45) < 6, "body leans 45° right into the diagonal, got " .. lean)
assert(math.abs(aim_deg()) < 1 or math.abs(aim_deg() - 360) < 1, "aim stays forward, got " .. aim_deg())

-- And the mirror: forward + step-left travels forward and to the player's LEFT
-- (+X, +Z). This is the case that was inverted before the fix.
facing(0)
input(true, false)      -- forward
strafe(true, false)     -- + step-left
local lx0, _, lz0 = pos()
step(40)
local lx2, _, lz2 = pos()
local ldx, ldz = lx2 - lx0, lz2 - lz0
print(string.format("diagonal fwd+left:  dX=%.1f dZ=%.1f anim=%s", ldx, ldz, anim()))
assert(ldx > 3.0 and ldz > 3.0,
    "travels diagonally forward-left (+X and +Z): dX=" .. ldx .. " dZ=" .. ldz)
